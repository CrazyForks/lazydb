mod support;

use lazydb::profile::import_connection_url;

#[tokio::test]
async fn mariadb_relation_mutation_fixture_supports_primary_key_round_trip() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB relation mutation test")).unwrap();
    let database = lazydb::db::DatabaseConnection::connect(
        &imported.profile,
        imported.transient_password.as_ref(),
    )
    .await
    .unwrap();
    let table = "lazydb_mariadb_relation_mutation_probe";
    database
        .execute(&format!(
            "DROP TABLE IF EXISTS `{table}`; CREATE TABLE `{table}` (id INT PRIMARY KEY, value VARCHAR(32) NULL)"
        ))
        .await
        .unwrap();
    database
        .execute(&format!("INSERT INTO `{table}` VALUES (1, 'before')"))
        .await
        .unwrap();
    database
        .execute(&format!(
            "UPDATE `{table}` SET value = 'after' WHERE id = 1"
        ))
        .await
        .unwrap();
    let result = database
        .execute(&format!("SELECT id, value FROM `{table}` WHERE id = 1"))
        .await
        .unwrap();
    let row = &result.result_sets.last().unwrap().rows[0];
    assert_eq!(row[0].clipboard_text(), "1");
    assert_eq!(row[1].clipboard_text(), "after");
    database
        .execute(&format!("DELETE FROM `{table}` WHERE id = 1"))
        .await
        .unwrap();
    database
        .execute(&format!("DROP TABLE `{table}`"))
        .await
        .unwrap();
    database.close().await;
}
