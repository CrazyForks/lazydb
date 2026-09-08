use lazydb::lsp::completion::complete_document;
use lazydb::lsp::document::Document;
use lazydb::sql::CompletionIndex;
use tower_lsp_server::ls_types::{CompletionResponse, Position, Uri};

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
