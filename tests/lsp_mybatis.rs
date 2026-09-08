use lazydb::lsp::completion::complete_document_with_embedded_sql;
use lazydb::lsp::diagnostics::diagnostics_for_document;
use lazydb::lsp::document::Document;
use lazydb::sql::{CompletionContext, CompletionIndex, SqlDialect};
use tower_lsp_server::ls_types::{CompletionResponse, Position, Uri};

fn xml(text: &str) -> Document {
    Document {
        uri: "file:///tmp/mapper.xml".parse::<Uri>().expect("URI"),
        language_id: "xml".into(),
        version: 1,
        text: text.into(),
    }
}

#[test]
fn static_mapper_errors_are_mapped_back_to_xml_ranges() {
    let document = xml("<select id=\"x\">SELECT * FROM users WHERE</select>");
    let diagnostics = diagnostics_for_document(&document, SqlDialect::Generic);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].range.start.character > 0);
}

#[test]
fn non_sql_xml_position_returns_no_sql_completion() {
    let document = xml("<mapper namespace=\"demo\"></mapper>");
    let CompletionResponse::List(list) = complete_document_with_embedded_sql(
        &document,
        Position::new(0, 20),
        SqlDialect::Generic,
        &CompletionIndex::new(&[]),
        false,
        CompletionContext::default(),
    ) else {
        panic!("expected completion list")
    };
    assert!(list.items.is_empty());
}
