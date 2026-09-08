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
