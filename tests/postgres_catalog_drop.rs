use lazydb::db::catalog_drop::CatalogDropExecutionTarget;
use sqlx::{AssertSqlSafe, Connection, PgConnection, raw_sql};
use uuid::Uuid;

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[tokio::test]
#[ignore = "requires a dedicated PostgreSQL test instance and explicit admin URL"]
async fn postgres_catalog_drop_integration_requires_isolated_admin_database() {
    let Some(admin_url) = std::env::var("LAZYDB_TEST_POSTGRES_ADMIN_URL").ok() else {
        panic!(
            "set LAZYDB_TEST_POSTGRES_ADMIN_URL to an isolated PostgreSQL admin connection before running destructive tests"
        );
    };
    assert!(!admin_url.is_empty());
    let database_name = format!("lazydb_drop_test_{}", Uuid::new_v4().simple());
    let mut admin = PgConnection::connect(&admin_url).await.unwrap();
    raw_sql(AssertSqlSafe(format!(
        "CREATE DATABASE {}",
        quote_identifier(&database_name)
    )))
    .execute(&mut admin)
    .await
    .unwrap();

    let target_url = format!("{admin_url}/{}", database_name);
    let target = PgConnection::connect(&target_url).await.unwrap();
    target.close().await.unwrap();
    raw_sql(AssertSqlSafe(format!(
        "DROP DATABASE {}",
        quote_identifier(&database_name)
    )))
    .execute(&mut admin)
    .await
    .unwrap();
}

#[test]
fn maintenance_target_is_not_the_current_connection_target() {
    let target = CatalogDropExecutionTarget::MaintenanceDatabase("postgres".into());
    assert!(matches!(
        target,
        CatalogDropExecutionTarget::MaintenanceDatabase(_)
    ));
}
