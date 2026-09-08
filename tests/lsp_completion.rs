use lazydb::db::catalog::{
    CatalogEntry, CatalogId, CatalogKind, CatalogMetadata, ColumnMetadata, OptionalMetadata,
    QualifiedName,
};
use lazydb::lsp::completion::complete_document;
use lazydb::lsp::document::Document;
use lazydb::lsp::position::PositionIndex;
use lazydb::sql::CompletionIndex;
use tower_lsp_server::ls_types::{CompletionResponse, CompletionTextEdit, Position, TextEdit, Uri};
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

#[test]
fn sql_completion_returns_keyword_text_edit() {
    let document = Document {
        uri: "file:///tmp/query.sql".parse::<Uri>().expect("URI"),
        language_id: "sql".into(),
        version: 1,
        text: "sel".into(),
    };
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
