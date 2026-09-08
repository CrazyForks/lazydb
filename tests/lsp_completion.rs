use lazydb::lsp::completion::complete_document;
use lazydb::lsp::document::Document;
use lazydb::lsp::position::PositionIndex;
use lazydb::{
    db::catalog::{
        CatalogEntry, CatalogId, CatalogKind, CatalogMetadata, ColumnMetadata, OptionalMetadata,
        QualifiedName,
    },
    sql::{CompletionIndex, SqlDialect},
};
use tower_lsp_server::ls_types::{CompletionItemKind, CompletionResponse, Position, Uri};
use tower_lsp_server::ls_types::{CompletionTextEdit, TextEdit};
use uuid::Uuid;

fn qualified(database: &str, schema: Option<&str>, object: &str) -> QualifiedName {
    QualifiedName {
        database: Some(database.into()),
        schema: schema.map(Into::into),
        object: object.into(),
    }
}

fn catalog_fixture() -> Vec<CatalogEntry> {
    let connection = Uuid::new_v4();
    let app = CatalogId::new(connection, CatalogKind::Database, ["app"]);
    let public = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    let audit = CatalogId::new(connection, CatalogKind::Schema, ["app", "audit"]);
    let other_database = CatalogId::new(connection, CatalogKind::Database, ["other"]);
    let other_tools = CatalogId::new(connection, CatalogKind::Schema, ["other", "tools"]);
    let users = CatalogId::new(
        connection,
        CatalogKind::Table,
        ["app", "public", "sys_user"],
    );
    let audit_users = CatalogId::new(connection, CatalogKind::Table, ["app", "audit", "sys_user"]);
    let other_users = CatalogId::new(
        connection,
        CatalogKind::Table,
        ["other", "tools", "sys_user"],
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
            app.clone(),
            qualified("app", Some("public"), "public"),
            "schema",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::schema(
            audit.clone(),
            app,
            qualified("app", Some("audit"), "audit"),
            "schema",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::database(
            other_database.clone(),
            qualified("other", None, "other"),
            "database",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::schema(
            other_tools.clone(),
            other_database,
            qualified("other", Some("tools"), "tools"),
            "schema",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::relation(
            users.clone(),
            public,
            qualified("app", Some("public"), "sys_user"),
            "table",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::relation_child(
            CatalogId::new(
                connection,
                CatalogKind::Column,
                ["app", "public", "sys_user", "id"],
            ),
            users,
            qualified("app", Some("public"), "id"),
            "column",
            OptionalMetadata::Unsupported,
            CatalogMetadata::Column(ColumnMetadata::new(1, "integer", false)),
        )
        .unwrap(),
        CatalogEntry::relation(
            audit_users,
            audit,
            qualified("app", Some("audit"), "sys_user"),
            "table",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::relation(
            other_users,
            other_tools,
            qualified("other", Some("tools"), "sys_user"),
            "table",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
    ]
}

fn document(text: &str) -> Document {
    Document {
        uri: "file:///tmp/query.sql".parse::<Uri>().expect("URI"),
        language_id: "sql".into(),
        version: 1,
        text: text.into(),
    }
}

fn update_fixture() -> Vec<CatalogEntry> {
    let connection = Uuid::new_v4();
    let schema = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    let table = CatalogId::new(connection, CatalogKind::Table, ["app", "public", "users"]);
    let mut entries = vec![
        CatalogEntry::relation(
            table.clone(),
            schema,
            QualifiedName {
                database: Some("app".into()),
                schema: Some("public".into()),
                object: "users".into(),
            },
            "table",
            OptionalMetadata::Supported(None),
            true,
        )
        .expect("table fixture"),
    ];
    for (position, name) in ["email", "name"].into_iter().enumerate() {
        entries.push(
            CatalogEntry::relation_child(
                CatalogId::new(
                    connection,
                    CatalogKind::Column,
                    ["app", "public", "users", name],
                ),
                table.clone(),
                QualifiedName {
                    database: Some("app".into()),
                    schema: Some("public".into()),
                    object: name.into(),
                },
                "column",
                OptionalMetadata::Unsupported,
                CatalogMetadata::Column(ColumnMetadata::new(position as u32 + 1, "text", true)),
            )
            .expect("column fixture"),
        );
    }
    entries
}

#[test]
fn sql_completion_returns_keyword_text_edit() {
    let document = document("sel");
    let CompletionResponse::List(list) = complete_document(
        &document,
        Position::new(0, 3),
        lazydb::sql::SqlDialect::Generic,
        &CompletionIndex::new(&[]),
        false,
    ) else {
        panic!("expected completion list")
    };
    let select = list
        .items
        .iter()
        .find(|item| item.label == "SELECT")
        .expect("SELECT completion");
    let edit = select.text_edit.as_ref().expect("text edit");
    let encoded = serde_json::to_value(edit).expect("serialize edit");
    assert_eq!(encoded["newText"], "SELECT");
    assert_eq!(encoded["range"]["start"]["character"], 0);
    assert_eq!(encoded["range"]["end"]["character"], 3);
}

#[test]
fn builtin_lsp_default_value_completion_maps_utf16_ranges() {
    let text = "CREATE TABLE t (\n  📊 TIMESTAMP DEFAULT CURRENT_TIM";
    let document = Document {
        uri: "file:///tmp/query.sql".parse::<Uri>().expect("URI"),
        language_id: "sql".into(),
        version: 1,
        text: text.into(),
    };
    let CompletionResponse::List(list) = complete_document(
        &document,
        Position::new(1, 34),
        lazydb::sql::SqlDialect::Postgres,
        &CompletionIndex::new(&[]),
        false,
    ) else {
        panic!("expected completion list")
    };
    let item = list
        .items
        .iter()
        .find(|item| item.label == "CURRENT_TIMESTAMP")
        .expect("CURRENT_TIMESTAMP completion");
    assert_eq!(
        item.kind,
        Some(tower_lsp_server::ls_types::CompletionItemKind::KEYWORD)
    );
    let edit = item.text_edit.as_ref().expect("text edit");
    let encoded = serde_json::to_value(edit).expect("serialize edit");
    assert_eq!(encoded["newText"], "CURRENT_TIMESTAMP");
    assert_eq!(encoded["range"]["start"]["line"], 1);
    assert_eq!(encoded["range"]["start"]["character"], 23);
    assert_eq!(encoded["range"]["end"]["character"], 34);
}

#[test]
fn update_set_column_completion_returns_text_edit_range() {
    let text = "UPDATE users SET na";
    let document = document(text);
    let CompletionResponse::List(list) = complete_document(
        &document,
        Position::new(0, text.len() as u32),
        SqlDialect::Postgres,
        &CompletionIndex::new(&update_fixture()),
        false,
    ) else {
        panic!("expected completion list")
    };
    let name = list
        .items
        .iter()
        .find(|item| item.label == "name")
        .expect("name completion");
    let edit = name.text_edit.as_ref().expect("text edit");
    let encoded = serde_json::to_value(edit).expect("serialize edit");
    assert_eq!(encoded["newText"], "name");
    assert_eq!(encoded["range"]["start"]["line"], 0);
    assert_eq!(encoded["range"]["start"]["character"], 17);
    assert_eq!(encoded["range"]["end"]["line"], 0);
    assert_eq!(encoded["range"]["end"]["character"], 19);
}

#[test]
fn update_set_column_completion_uses_field_kind() {
    let text = "UPDATE users SET e";
    let document = document(text);
    let CompletionResponse::List(list) = complete_document(
        &document,
        Position::new(0, text.len() as u32),
        SqlDialect::Postgres,
        &CompletionIndex::new(&update_fixture()),
        false,
    ) else {
        panic!("expected completion list")
    };
    let email = list
        .items
        .iter()
        .find(|item| item.label == "email")
        .expect("email completion");
    assert_eq!(email.kind, Some(CompletionItemKind::FIELD));
}

#[test]
fn update_set_column_completion_does_not_offer_tables() {
    let text = "UPDATE users SET ";
    let document = document(text);
    let CompletionResponse::List(list) = complete_document(
        &document,
        Position::new(0, text.len() as u32),
        SqlDialect::Postgres,
        &CompletionIndex::new(&update_fixture()),
        false,
    ) else {
        panic!("expected completion list")
    };
    assert!(
        list.items
            .iter()
            .all(|item| { item.kind == Some(CompletionItemKind::FIELD) && item.label != "users" })
    );
}

#[test]
fn update_set_column_completion_requires_target_table() {
    let entries = update_fixture();
    let columns = entries.into_iter().skip(1).collect::<Vec<_>>();
    let text = "UPDATE users SET ";
    let document = document(text);
    let CompletionResponse::List(list) = complete_document(
        &document,
        Position::new(0, text.len() as u32),
        SqlDialect::Postgres,
        &CompletionIndex::new(&columns),
        false,
    ) else {
        panic!("expected completion list")
    };
    assert!(list.items.is_empty());
}

#[test]
fn update_set_column_completion_uses_current_statement_after_prefix() {
    let text = "SELECT * FROM users; UPDATE users SET e";
    let document = document(text);
    let CompletionResponse::List(list) = complete_document(
        &document,
        Position::new(0, text.len() as u32),
        SqlDialect::Postgres,
        &CompletionIndex::new(&update_fixture()),
        false,
    ) else {
        panic!("expected completion list")
    };
    assert_eq!(
        list.items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        vec!["email"]
    );
}

#[test]
fn delete_completion_returns_from_and_where_keyword_edits() {
    for (text, position, label, start, end) in [
        ("DELETE fro", Position::new(0, 10), "FROM", 7, 10),
        (
            "SELECT 1; DELETE FROM users w",
            Position::new(0, 29),
            "WHERE",
            28,
            29,
        ),
    ] {
        let document = Document {
            uri: "file:///tmp/delete.sql".parse::<Uri>().expect("URI"),
            language_id: "sql".into(),
            version: 1,
            text: text.into(),
        };
        let CompletionResponse::List(list) = complete_document(
            &document,
            position,
            lazydb::sql::SqlDialect::Generic,
            &CompletionIndex::new(&[]),
            false,
        ) else {
            panic!("expected completion list")
        };
        let item = list
            .items
            .iter()
            .find(|item| item.label == label)
            .expect("DELETE keyword completion");
        assert_eq!(
            item.kind,
            Some(tower_lsp_server::ls_types::CompletionItemKind::KEYWORD)
        );
        let edit = item.text_edit.as_ref().expect("text edit");
        let encoded = serde_json::to_value(edit).expect("serialize edit");
        assert_eq!(encoded["newText"], label);
        assert_eq!(encoded["range"]["start"]["character"], start);
        assert_eq!(encoded["range"]["end"]["character"], end);
    }
}

fn completion_item<'a>(
    list: &'a CompletionResponse,
    kind: tower_lsp_server::ls_types::CompletionItemKind,
    label: &'a str,
) -> &'a tower_lsp_server::ls_types::CompletionItem {
    let CompletionResponse::List(list) = list else {
        panic!("expected completion list")
    };
    list.items
        .iter()
        .find(|item| item.kind == Some(kind) && item.label == label)
        .unwrap_or_else(|| panic!("missing completion {label:?}"))
}

fn apply_text_edit(text: &str, item: &tower_lsp_server::ls_types::CompletionItem) -> String {
    let CompletionTextEdit::Edit(TextEdit { range, new_text }) =
        item.text_edit.as_ref().expect("text edit")
    else {
        panic!("expected plain text edit")
    };
    let positions = PositionIndex::new(text.to_owned());
    let start = positions.offset(range.start);
    let end = positions.offset(range.end);
    let mut result = String::new();
    result.push_str(&text[..start]);
    result.push_str(new_text);
    result.push_str(&text[end..]);
    result
}

#[test]
fn lsp_table_completion_inserts_only_the_table_name() {
    let document = Document {
        uri: "file:///tmp/query.sql".parse::<Uri>().expect("URI"),
        language_id: "sql".into(),
        version: 1,
        text: "SELECT * FROM sys_".into(),
    };
    let response = complete_document(
        &document,
        Position::new(0, document.text.encode_utf16().count() as u32),
        lazydb::sql::SqlDialect::Generic,
        &CompletionIndex::new(&catalog_fixture()),
        false,
    );
    let table = completion_item(
        &response,
        tower_lsp_server::ls_types::CompletionItemKind::CLASS,
        "sys_user",
    );
    assert_eq!(table.detail.as_deref(), Some("(app.public)"));
    assert_eq!(
        apply_text_edit(&document.text, table),
        "SELECT * FROM sys_user"
    );
}
