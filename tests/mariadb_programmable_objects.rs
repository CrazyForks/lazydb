mod support;

use lazydb::profile::import_connection_url;

#[tokio::test]
async fn mariadb_trigger_definition_is_available_through_show_create() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB programmable object test")).unwrap();
    let database = lazydb::db::DatabaseConnection::connect(
        &imported.profile,
        imported.transient_password.as_ref(),
    )
    .await
    .unwrap();
    let table = "lazydb_mariadb_trigger_probe";
    let trigger = "lazydb_mariadb_trigger_probe_insert";
    database
        .execute(&format!(
            "DROP TRIGGER IF EXISTS `{trigger}`; DROP TABLE IF EXISTS `{table}`; \
             CREATE TABLE `{table}` (id INT PRIMARY KEY, value VARCHAR(32)); \
             CREATE TRIGGER `{trigger}` BEFORE INSERT ON `{table}` FOR EACH ROW SET NEW.value = COALESCE(NEW.value, 'default')"
        ))
        .await
        .unwrap();
    let ddl = database
        .execute(&format!("SHOW CREATE TRIGGER `{trigger}`"))
        .await
        .unwrap();
    let statement = ddl.result_sets.last().unwrap().rows[0][2].clipboard_text();
    assert!(!statement.trim().is_empty());
    database
        .execute(&format!("DROP TRIGGER `{trigger}`; DROP TABLE `{table}`"))
        .await
        .unwrap();
    database.close().await;
}
