use lazydb::lsp::completion::complete_document;
use lazydb::lsp::document::Document;
use lazydb::{
    db::catalog::{
        CatalogEntry, CatalogId, CatalogKind, CatalogMetadata, ColumnMetadata, OptionalMetadata,
        QualifiedName,
    },
    sql::{CompletionIndex, SqlDialect},
};
use tower_lsp_server::ls_types::{CompletionItemKind, CompletionResponse, Position, Uri};
use uuid::Uuid;

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
    assert_eq!(encoded["newText"], "\"name\"");
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
