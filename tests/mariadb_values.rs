mod support;

use lazydb::{
    db::{DatabaseConnection, value::CellValue},
    profile::import_connection_url,
};

#[tokio::test]
async fn mariadb_time_preserves_extended_duration_values() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB values test")).unwrap();
    let database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let result = database
        .execute("SELECT CAST('-01:02:03.123456' AS TIME(6)) AS negative, CAST('800:00:00.654321' AS TIME(6)) AS extended")
        .await
        .unwrap()
        .result_sets
        .pop()
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert!(matches!(&result.rows[0][0], CellValue::Text(value) if value.contains("-01:02:03")));
    assert!(matches!(&result.rows[0][1], CellValue::Text(value) if value.contains("800:00:00")));
    database.close().await;
}
