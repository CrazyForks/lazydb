use lazydb::lsp::diagnostics::diagnostics_for_document;
use lazydb::lsp::document::Document;
use tower_lsp_server::ls_types::{DiagnosticSeverity, Uri};

#[test]
fn lsp_diagnostics_include_source_code_and_error_severity() {
    let document = Document {
        uri: "file:///tmp/query.sql".parse::<Uri>().expect("URI"),
        language_id: "sql".into(),
        version: 2,
        text: "select * from users where".into(),
    };
    let diagnostics = diagnostics_for_document(&document, lazydb::sql::SqlDialect::Generic);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].source.as_deref(), Some("lazydb"));
    assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diagnostics[0].range.start.line, 0);
}
