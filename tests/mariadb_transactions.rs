mod support;

use lazydb::profile::import_connection_url;

#[tokio::test]
async fn mariadb_transaction_round_trip_preserves_commit_and_rollback_results() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB transaction test")).unwrap();
    let database = lazydb::db::DatabaseConnection::connect(
        &imported.profile,
        imported.transient_password.as_ref(),
    )
    .await
    .unwrap();
    let table = "lazydb_mariadb_transaction_probe";
    database
        .execute(&format!(
            "DROP TABLE IF EXISTS `{table}`; CREATE TABLE `{table}` (id INT PRIMARY KEY, value VARCHAR(32))"
        ))
        .await
        .unwrap();
    database
        .execute(&format!(
            "START TRANSACTION; INSERT INTO `{table}` VALUES (1, 'rollback'); ROLLBACK"
        ))
        .await
        .unwrap();
    let after_rollback = database
        .execute(&format!("SELECT COUNT(*) AS count FROM `{table}`"))
        .await
        .unwrap();
    assert_eq!(
        after_rollback.result_sets.last().unwrap().rows[0][0].clipboard_text(),
        "0"
    );

    database
        .execute(&format!(
            "START TRANSACTION; INSERT INTO `{table}` VALUES (2, 'commit'); COMMIT"
        ))
        .await
        .unwrap();
    let after_commit = database
        .execute(&format!("SELECT COUNT(*) AS count FROM `{table}`"))
        .await
        .unwrap();
    assert_eq!(
        after_commit.result_sets.last().unwrap().rows[0][0].clipboard_text(),
        "1"
    );
    database
        .execute(&format!("DROP TABLE `{table}`"))
        .await
        .unwrap();
    database.close().await;
}

#[tokio::test]
async fn mariadb_sql_error_does_not_poison_the_connection_pool() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB transaction error test")).unwrap();
    let database = lazydb::db::DatabaseConnection::connect(
        &imported.profile,
        imported.transient_password.as_ref(),
    )
    .await
    .unwrap();
    assert!(
        database
            .execute("SELECT * FROM lazydb_missing_transaction_table")
            .await
            .is_err()
    );
    assert_eq!(
        database
            .execute("SELECT 1")
            .await
            .unwrap()
            .result_sets
            .len(),
        1
    );
    database.close().await;
}
