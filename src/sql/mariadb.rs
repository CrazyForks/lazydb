use super::SqlDialect;

/// Detects a MariaDB client-side delimiter directive without interpreting it
/// as server SQL. Procedure splitting is intentionally deferred to the
/// delimiter-aware execution path.
pub fn has_client_delimiter_directive(sql: &str) -> bool {
    sql.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.len() >= 9 && trimmed[..9].eq_ignore_ascii_case("delimiter")
    })
}

pub const fn is_mariadb(dialect: SqlDialect) -> bool {
    matches!(dialect, SqlDialect::MariaDb)
}
