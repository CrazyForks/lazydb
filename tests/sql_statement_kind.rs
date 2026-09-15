use lazydb::sql::{SqlDialect, SqlStatementKind, classify_statement_kind};

#[test]
fn classifies_common_statement_families() {
    let cases = [
        ("-- comment\nSELECT * FROM users", SqlStatementKind::Dql),
        (
            "INSERT INTO users (name) VALUES ('Ada')",
            SqlStatementKind::Dml,
        ),
        (
            "ALTER TABLE users ADD COLUMN active BOOLEAN",
            SqlStatementKind::Ddl,
        ),
        ("GRANT SELECT ON users TO analyst", SqlStatementKind::Dcl),
        ("COMMIT", SqlStatementKind::Tcl),
        ("CALL refresh_users()", SqlStatementKind::Other),
    ];

    for (sql, expected) in cases {
        assert_eq!(
            classify_statement_kind(sql, SqlDialect::Postgres),
            expected,
            "{sql}"
        );
    }
}

#[test]
fn classifies_queries_without_using_execution_risk() {
    assert_eq!(
        classify_statement_kind("SELECT * FROM users FOR UPDATE", SqlDialect::Postgres),
        SqlStatementKind::Dql
    );
    assert_eq!(
        classify_statement_kind(
            "WITH changed AS (UPDATE users SET active = true RETURNING id) SELECT * FROM changed",
            SqlDialect::Postgres
        ),
        SqlStatementKind::Dml
    );
}

#[test]
fn aggregates_multiple_statements() {
    assert_eq!(
        classify_statement_kind("SELECT 1; SELECT 2", SqlDialect::Postgres),
        SqlStatementKind::Dql
    );
    assert_eq!(
        classify_statement_kind("SELECT 1; DELETE FROM users", SqlDialect::Postgres),
        SqlStatementKind::Mixed
    );
    assert_eq!(
        classify_statement_kind("SELECT 1; CALL refresh_users()", SqlDialect::Postgres),
        SqlStatementKind::Other
    );
}

#[test]
fn supports_sql_server_go_batches() {
    assert_eq!(
        classify_statement_kind("SELECT 1\nGO\nSELECT 2", SqlDialect::SqlServer),
        SqlStatementKind::Dql
    );
}
