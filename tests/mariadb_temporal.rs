mod support;

use lazydb::profile::import_connection_url;

#[tokio::test]
async fn mariadb_system_versioned_definition_is_preserved_as_native_ddl() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB temporal test")).unwrap();
    let database = lazydb::db::DatabaseConnection::connect(
        &imported.profile,
        imported.transient_password.as_ref(),
    )
    .await
    .unwrap();
    let table = "lazydb_mariadb_temporal_probe";
    database
        .execute(&format!(
            "DROP TABLE IF EXISTS `{table}`; CREATE TABLE `{table}` (id INT PRIMARY KEY, value VARCHAR(32), \
             row_start TIMESTAMP(6) GENERATED ALWAYS AS ROW START, \
             row_end TIMESTAMP(6) GENERATED ALWAYS AS ROW END, \
             PERIOD FOR SYSTEM_TIME (row_start, row_end)) WITH SYSTEM VERSIONING"
        ))
        .await
        .unwrap();
    let ddl = database
        .execute(&format!("SHOW CREATE TABLE `{table}`"))
        .await
        .unwrap();
    let statement = ddl.result_sets.last().unwrap().rows[0][1].clipboard_text();
    let upper = statement.to_ascii_uppercase();
    assert!(upper.contains("SYSTEM VERSIONING") || upper.contains("PERIOD FOR SYSTEM_TIME"));
    database
        .execute(&format!("DROP TABLE `{table}`"))
        .await
        .unwrap();
    database.close().await;
}
