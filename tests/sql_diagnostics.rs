use lazydb::sql::{SqlDialect, diagnose_sql};

#[test]
fn valid_sql_has_no_diagnostics() {
    assert!(diagnose_sql("select 1", SqlDialect::Generic).is_empty());
}

#[test]
fn parser_eof_failures_are_reported_at_the_end_of_sql() {
    let diagnostics = diagnose_sql("select *\nfrom users\nwhere", SqlDialect::Generic);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "sql-parser");
    assert_eq!(
        diagnostics[0].range.start,
        "select *\nfrom users\nwhere".len()
    );
    assert_eq!(
        diagnostics[0].range.end,
        "select *\nfrom users\nwhere".len()
    );
}

#[test]
fn parser_locations_use_utf8_character_boundaries() {
    let text = "SELECT '\u{1f600}' FROM users WHERE )";
    let diagnostics = diagnose_sql(text, SqlDialect::Postgres);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].range.start, text.find(')').unwrap());
    assert_eq!(diagnostics[0].range.end, text.len());
}

#[test]
fn tokenizer_failures_use_the_structured_token_location() {
    let diagnostics = diagnose_sql("select *\nfrom users\n'", SqlDialect::Generic);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "sql-tokenizer");
    assert_eq!(diagnostics[0].range.start, "select *\nfrom users\n".len());
}
