use lazydb::sql::{SqlDialect, diagnose_sql};

#[test]
fn valid_sql_has_no_diagnostics() {
    assert!(diagnose_sql("select 1", SqlDialect::Generic).is_empty());
}

#[test]
fn parser_failures_are_reported_over_the_statement_when_unlocated() {
    let diagnostics = diagnose_sql("select *\nfrom users\nwhere", SqlDialect::Generic);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "sql-parser");
    assert_eq!(diagnostics[0].range.start, 0);
    assert_eq!(
        diagnostics[0].range.end,
        "select *\nfrom users\nwhere".len()
    );
}

#[test]
fn tokenizer_failures_use_the_structured_token_location() {
    let diagnostics = diagnose_sql("select *\nfrom users\n'", SqlDialect::Generic);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "sql-tokenizer");
    assert_eq!(diagnostics[0].range.start, "select *\nfrom users\n".len());
}
