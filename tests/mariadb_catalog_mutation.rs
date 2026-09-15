mod support;

use lazydb::{db::DatabaseConnection, profile::import_connection_url};

#[tokio::test]
async fn mariadb_table_and_view_definition_round_trip_preserves_native_options() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB catalog mutation test")).unwrap();
    let database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let table = "lazydb_mariadb_catalog_probe";
    let view = "lazydb_mariadb_catalog_view";
    database
        .execute(&format!(
            "DROP VIEW IF EXISTS `{view}`; DROP TABLE IF EXISTS `{table}`; \
             CREATE TABLE `{table}` (id INT NOT NULL AUTO_INCREMENT PRIMARY KEY, value VARCHAR(32) NULL, \
             generated VARCHAR(64) AS (CONCAT(value, '-generated')) STORED) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='mariadb catalog'; \
             CREATE VIEW `{view}` AS SELECT id, value FROM `{table}`"
        ))
        .await
        .unwrap();
    let table_ddl = database
        .execute(&format!("SHOW CREATE TABLE `{table}`"))
        .await
        .unwrap();
    let table_sql = table_ddl.result_sets.last().unwrap().rows[0][1].clipboard_text();
    assert!(table_sql.contains("AUTO_INCREMENT"));
    assert!(table_sql.to_ascii_lowercase().contains("utf8mb4"));
    assert!(table_sql.contains("generated"));
    let view_ddl = database
        .execute(&format!("SHOW CREATE VIEW `{view}`"))
        .await
        .unwrap();
    assert!(
        view_ddl.result_sets.last().unwrap().rows[0][1]
            .clipboard_text()
            .to_ascii_uppercase()
            .contains("SELECT")
    );
    database
        .execute(&format!("DROP VIEW `{view}`; DROP TABLE `{table}`"))
        .await
        .unwrap();
    database.close().await;
}
