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
