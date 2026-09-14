mod support;

use lazydb::{
    db::{DatabaseConnection, value::CellValue},
    profile::import_connection_url,
};

#[tokio::test]
async fn mariadb_smoke_covers_probe_results_and_multi_result_execution() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB adapter test")).unwrap();
    let database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();

    let server = database.probe().await.unwrap();
    assert!(server.version.to_ascii_lowercase().contains("mariadb"));

    let outcome = database
        .execute("SELECT 1 AS n, TRUE AS ok, 'Ada' AS name, NULL AS missing")
        .await
        .unwrap();
    let result = outcome.result_sets.last().unwrap();
    assert_eq!(result.columns.len(), 4);
    assert_eq!(result.rows[0][0], CellValue::Integer(1));
    assert!(matches!(
        result.rows[0][1],
        CellValue::Boolean(true) | CellValue::Integer(1)
    ));
    assert_eq!(result.rows[0][2], CellValue::Text("Ada".into()));
    assert_eq!(result.rows[0][3], CellValue::Null);

    let multiple = database
        .execute("SELECT 1 AS first; SELECT 2 AS second")
        .await
        .unwrap();
    assert_eq!(multiple.result_sets.len(), 2);
    database.close().await;
}
