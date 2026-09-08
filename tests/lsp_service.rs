use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use lazydb::db::catalog::{
    CatalogEntry, CatalogId, CatalogKind, CatalogMetadata, CatalogTarget, ColumnMetadata,
    ObjectGroup, OptionalMetadata, QualifiedName,
};
use lazydb::lsp::catalog::{CatalogTargetLoader, CatalogTargetService};
use lazydb::lsp::completion::complete_document_with_catalog;
use lazydb::lsp::document::Document;
use lazydb::profile::{CatalogScope, CatalogSelection};
use lazydb::sql::CompletionContext;
use tower_lsp_server::ls_types::{CompletionResponse as LspResponse, Position, Uri};
use uuid::Uuid;

fn qualified(database: &str, schema: Option<&str>, object: &str) -> QualifiedName {
    QualifiedName {
        database: Some(database.into()),
        schema: schema.map(Into::into),
        object: object.into(),
    }
}

#[derive(Default)]
struct ServiceFake {
    responses: std::collections::HashMap<
        CatalogTarget,
        Result<Vec<CatalogEntry>, lazydb::db::DatabaseError>,
    >,
    calls: Mutex<Vec<String>>,
    block_children: bool,
    fail_first_databases: std::sync::atomic::AtomicBool,
}

impl ServiceFake {
    fn respond(&mut self, target: CatalogTarget, entries: Vec<CatalogEntry>) {
        self.responses.insert(target, Ok(entries));
    }
}

#[async_trait::async_trait]
impl CatalogTargetLoader for ServiceFake {
    async fn load(
        &self,
        target: CatalogTarget,
        _scope: &CatalogScope,
    ) -> anyhow::Result<Vec<CatalogEntry>> {
        self.calls.lock().unwrap().push(format!("{target:?}"));
        if matches!(target, CatalogTarget::Databases)
            && self.fail_first_databases.swap(false, Ordering::SeqCst)
        {
            return Err(anyhow::anyhow!(lazydb::db::DatabaseError::configuration(
                "database listing denied"
            )));
        }
        if matches!(target, CatalogTarget::RelationChildren { .. }) && self.block_children {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
        match self.responses.get(&target) {
            Some(Ok(entries)) => Ok(entries.clone()),
            Some(Err(error)) => Err(anyhow::Error::new(error.clone())),
            None => Err(anyhow::Error::new(
                lazydb::db::DatabaseError::configuration(format!(
                    "no configured response for {}",
                    target.description()
                )),
            )),
        }
    }
}

fn all_scope() -> CatalogScope {
    CatalogScope {
        databases: CatalogSelection::All,
    }
}

fn catalog_fixture() -> Vec<CatalogEntry> {
    let connection = Uuid::new_v4();
    let app = CatalogId::new(connection, CatalogKind::Database, ["app"]);
    let public = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    let users = CatalogId::new(connection, CatalogKind::Table, ["app", "public", "users"]);
    let invoices = CatalogId::new(
        connection,
        CatalogKind::Table,
        ["app", "public", "invoices"],
    );
    vec![
        CatalogEntry::database(
            app.clone(),
            qualified("app", None, "app"),
            "database",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::schema(
            public.clone(),
            app,
            qualified("app", Some("public"), "public"),
            "schema",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::relation(
            users.clone(),
            public.clone(),
            qualified("app", Some("public"), "users"),
            "table",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::relation(
            invoices.clone(),
            public,
            qualified("app", Some("public"), "invoices"),
            "table",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::relation_child(
            CatalogId::new(
                connection,
                CatalogKind::Column,
                ["app", "public", "users", "id"],
            ),
            users,
            qualified("app", Some("public"), "id"),
            "column",
            OptionalMetadata::Unsupported,
            CatalogMetadata::Column(ColumnMetadata::new(1, "integer", false)),
        )
        .unwrap(),
        CatalogEntry::relation_child(
            CatalogId::new(
                connection,
                CatalogKind::Column,
                ["app", "public", "invoices", "amount"],
            ),
            invoices,
            qualified("app", Some("public"), "amount"),
            "column",
            OptionalMetadata::Unsupported,
            CatalogMetadata::Column(ColumnMetadata::new(1, "decimal", false)),
        )
        .unwrap(),
    ]
}

fn configured_fake() -> ServiceFake {
    let entries = catalog_fixture();
    let mut fake = ServiceFake::default();
    let connection = entries[0].id.profile_id();
    let app = CatalogId::new(connection, CatalogKind::Database, ["app"]);
    let public = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    fake.respond(
        CatalogTarget::Databases,
        entries
            .iter()
            .filter(|entry| entry.kind == CatalogKind::Database)
            .cloned()
            .collect(),
    );
    fake.respond(
        CatalogTarget::Schemas {
            database: app.clone(),
        },
        entries
            .iter()
            .filter(|entry| entry.kind == CatalogKind::Schema)
            .cloned()
            .collect(),
    );
    for group in [
        ObjectGroup::Tables,
        ObjectGroup::Views,
        ObjectGroup::MaterializedViews,
    ] {
        fake.respond(
            CatalogTarget::Objects {
                schema: public.clone(),
                group,
            },
            entries
                .iter()
                .filter(|entry| {
                    entry.kind.is_relation()
                        && entry.parent_id.as_ref() == Some(&public)
                        && group.contains_kind(entry.id.kind)
                })
                .cloned()
                .collect(),
        );
    }
    for relation in entries.iter().filter(|entry| entry.id.kind.is_relation()) {
        fake.respond(
            CatalogTarget::RelationChildren {
                relation: relation.id.clone(),
            },
            entries
                .iter()
                .filter(|entry| entry.relation_id.as_ref() == Some(&relation.id))
                .cloned()
                .collect(),
        );
    }
    fake
}

fn document(text: &str) -> Document {
    Document {
        uri: "file:///tmp/query.sql".parse::<Uri>().expect("URI"),
        language_id: "sql".into(),
        version: 1,
        text: text.into(),
    }
}

fn calls_for(fake: &ServiceFake, pattern: &str) -> Vec<String> {
    fake.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call.starts_with(pattern))
        .cloned()
        .collect()
}

fn child_calls(fake: &ServiceFake) -> Vec<String> {
    fake.calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| call.starts_with("RelationChildren"))
        .cloned()
        .collect()
}

#[tokio::test]
async fn from_completion_does_not_preload_columns() {
    let fake = Arc::new(configured_fake());
    let service = CatalogTargetService::new(fake.clone(), all_scope(), Some("app".into()), None);
    let document = document("select * from use");
    let response = complete_document_with_catalog(
        &document,
        Position::new(0, document.text.encode_utf16().count() as u32),
        lazydb::sql::SqlDialect::Generic,
        &service,
        CompletionContext::default(),
    )
    .await;
    let LspResponse::List(list) = response else {
        panic!("expected completion list")
    };
    assert!(!list.is_incomplete);
    let table = list
        .items
        .iter()
        .find(|item| item.label == "users")
        .expect("users table candidate");
    assert_eq!(table.detail.as_deref(), Some("(app.public)"));
    assert_eq!(
        calls_for(fake.as_ref(), "RelationChildren").len(),
        0,
        "FROM completion must not preload any columns"
    );
}

#[tokio::test]
async fn alias_column_completion_loads_only_that_relation_children() {
    let fake = Arc::new(configured_fake());
    let service = CatalogTargetService::new(fake.clone(), all_scope(), Some("app".into()), None);
    let document = document("select u. from users u");
    let response = complete_document_with_catalog(
        &document,
        Position::new(0, "select u.".encode_utf16().count() as u32),
        lazydb::sql::SqlDialect::Generic,
        &service,
        CompletionContext::default(),
    )
    .await;
    let LspResponse::List(list) = response else {
        panic!("expected completion list")
    };
    assert!(!list.is_incomplete);
    assert!(
        list.items.iter().any(|item| item.label == "id"),
        "alias completion should offer the relation columns"
    );
    let child_calls = child_calls(fake.as_ref());
    assert_eq!(
        child_calls.len(),
        1,
        "only the referenced relation children should load: {child_calls:?}"
    );
    assert!(
        child_calls[0].contains("users"),
        "the loaded relation must be the referenced one"
    );
    assert!(
        child_calls.iter().all(|call| !call.contains("invoices")),
        "unreferenced tables must not load columns"
    );
}

#[tokio::test]
async fn duplicate_requests_do_not_duplicate_io() {
    let fake = Arc::new(configured_fake());
    let service = CatalogTargetService::new(fake.clone(), all_scope(), Some("app".into()), None);
    service.ensure(CatalogTarget::Databases).await;
    service.ensure(CatalogTarget::Databases).await;
    assert_eq!(
        calls_for(fake.as_ref(), "Databases").len(),
        1,
        "cached target must not be re-requested"
    );
}

#[tokio::test]
async fn blocked_target_marks_incomplete_after_budget() {
    let mut fake = configured_fake();
    fake.block_children = true;
    fake.respond(
        CatalogTarget::RelationChildren {
            relation: CatalogId::new(
                Uuid::new_v4(),
                CatalogKind::Table,
                ["app", "public", "users"],
            ),
        },
        Vec::new(),
    );
    let service = CatalogTargetService::new(Arc::new(fake), all_scope(), Some("app".into()), None);
    let (_, incomplete) = service
        .ensure_many(
            vec![CatalogTarget::RelationChildren {
                relation: CatalogId::new(
                    Uuid::new_v4(),
                    CatalogKind::Table,
                    ["app", "public", "users"],
                ),
            }],
            std::time::Duration::from_millis(10),
        )
        .await;
    assert!(
        incomplete,
        "a target that never completes within the budget must mark the batch incomplete"
    );
}

#[tokio::test]
async fn failed_target_retries_after_cooldown() {
    let configured = configured_fake();
    configured
        .fail_first_databases
        .store(true, Ordering::SeqCst);
    let fake = Arc::new(configured);
    let cursor = Arc::new(AtomicU64::new(0));
    let cursor_clone = cursor.clone();
    let clock = move || {
        std::time::Instant::now()
            + std::time::Duration::from_secs(cursor_clone.load(Ordering::SeqCst))
    };
    let service = Arc::new(CatalogTargetService::with_clock(
        fake.clone(),
        all_scope(),
        Some("app".into()),
        None,
        Arc::new(clock),
    ));
    let first = service.ensure(CatalogTarget::Databases).await;
    assert!(first.incomplete, "first attempt must fail");
    let during_cooldown = service.ensure(CatalogTarget::Databases).await;
    assert!(
        during_cooldown.incomplete,
        "cooldown must prevent immediate retry"
    );
    cursor.store(3, Ordering::SeqCst);
    let retry = service.ensure(CatalogTarget::Databases).await;
    assert!(!retry.incomplete, "retry after cooldown must succeed");
    assert_eq!(
        calls_for(fake.as_ref(), "Databases").len(),
        2,
        "exactly one retry after cooldown"
    );
}

#[tokio::test]
async fn xml_completion_uses_catalog_pipeline_with_source_mapping() {
    let fake = Arc::new(configured_fake());
    let service = CatalogTargetService::new(fake.clone(), all_scope(), Some("app".into()), None);
    let text = "<select id=\"x\">select * from use</select>";
    let document = Document {
        uri: "file:///tmp/mapper.xml".parse::<Uri>().expect("URI"),
        language_id: "xml".into(),
        version: 1,
        text: text.into(),
    };
    let sql_start = "<select id=\"x\">".len();
    let cursor = "select * from use".len();
    let response = complete_document_with_catalog(
        &document,
        Position::new(0, (sql_start + cursor) as u32),
        lazydb::sql::SqlDialect::Generic,
        &service,
        CompletionContext::default(),
    )
    .await;
    let LspResponse::List(list) = response else {
        panic!("expected completion list")
    };
    let table = list
        .items
        .iter()
        .find(|item| item.label == "users")
        .expect("xml completion should find the table");
    let tower_lsp_server::ls_types::CompletionTextEdit::Edit(edit) =
        table.text_edit.as_ref().expect("text edit")
    else {
        panic!("expected plain text edit")
    };
    assert_eq!(edit.new_text, "users");
    assert_eq!(edit.range.start.character as usize, sql_start + 14);
    assert_eq!(edit.range.end.character as usize, sql_start + cursor);
}

#[tokio::test]
async fn explicit_qualified_path_warms_only_matching_schema() {
    let entries = catalog_fixture();
    let mut fake = ServiceFake::default();
    let connection = entries[0].id.profile_id();
    let app = CatalogId::new(connection, CatalogKind::Database, ["app"]);
    let tools = CatalogId::new(connection, CatalogKind::Schema, ["app", "tools"]);
    let orders = CatalogId::new(connection, CatalogKind::Table, ["app", "tools", "orders"]);
    fake.respond(
        CatalogTarget::Databases,
        entries
            .iter()
            .filter(|entry| entry.kind == CatalogKind::Database)
            .cloned()
            .collect(),
    );
    fake.respond(
        CatalogTarget::Schemas {
            database: app.clone(),
        },
        vec![
            CatalogEntry::schema(
                tools.clone(),
                app,
                qualified("app", Some("tools"), "tools"),
                "schema",
                OptionalMetadata::Supported(None),
                true,
            )
            .unwrap(),
        ],
    );
    fake.respond(
        CatalogTarget::Objects {
            schema: tools.clone(),
            group: ObjectGroup::Tables,
        },
        vec![
            CatalogEntry::relation(
                orders.clone(),
                tools,
                qualified("app", Some("tools"), "orders"),
                "table",
                OptionalMetadata::Supported(None),
                true,
            )
            .unwrap(),
        ],
    );
    let service = CatalogTargetService::new(Arc::new(fake), all_scope(), Some("app".into()), None);
    let document = document("select * from tools.");
    let response = complete_document_with_catalog(
        &document,
        Position::new(0, document.text.encode_utf16().count() as u32),
        lazydb::sql::SqlDialect::Generic,
        &service,
        CompletionContext::default(),
    )
    .await;
    let LspResponse::List(list) = response else {
        panic!("expected completion list")
    };
    assert!(
        list.items.iter().any(|item| item.label == "orders"),
        "qualified path should warm the referenced schema"
    );
}
