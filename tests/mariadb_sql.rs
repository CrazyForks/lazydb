use lazydb::sql::{SqlDialect, classify_sql};

#[test]
fn mariadb_has_an_independent_dialect_but_reuses_mysql_parser_rules() {
    assert_eq!(
        SqlDialect::for_database_kind(lazydb::profile::DatabaseKind::MariaDb),
        SqlDialect::MariaDb
    );
    let analysis = classify_sql("CREATE TABLE t (id INT)", SqlDialect::MariaDb);
    assert_eq!(analysis.statement_count, 1);
}

#[test]
fn delimiter_directive_is_detected_without_treating_it_as_server_sql() {
    assert!(lazydb::sql::mariadb::has_client_delimiter_directive(
        "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; END//\nDELIMITER ;"
    ));
    assert!(!lazydb::sql::mariadb::has_client_delimiter_directive(
        "SELECT 'DELIMITER //';"
    ));
}
