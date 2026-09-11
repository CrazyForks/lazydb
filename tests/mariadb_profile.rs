use lazydb::{
    db::mysql::supports_catalog_version_for_kind,
    model::profile_manager::DRIVER_ORDER,
    profile::{ConnectionUrlFormat, DatabaseKind, parse_connection_url},
};
use secrecy::ExposeSecret;

#[test]
fn parses_mariadb_url_with_mysql_compatible_connection_fields() {
    let parsed = parse_connection_url(
        "mariadb://user:p%40ss@example.test:3307/app?useSSL=true&readOnly=true",
    )
    .unwrap();

    assert_eq!(parsed.kind, DatabaseKind::MariaDb);
    assert_eq!(parsed.format, ConnectionUrlFormat::MariaDb);
    assert_eq!(parsed.host.as_deref(), Some("example.test"));
    assert_eq!(parsed.port, Some(3307));
    assert_eq!(parsed.user.as_deref(), Some("user"));
    assert_eq!(
        parsed.password.as_ref().map(|value| value.expose_secret()),
        Some("p@ss")
    );
    assert_eq!(parsed.database.as_deref(), Some("app"));
    assert!(parsed.read_only);
}

#[test]
fn mariadb_uses_the_mysql_namespace_and_dialect_rules() {
    let scope = lazydb::profile::CatalogScope::for_profile(DatabaseKind::MariaDb, "app", None);
    assert!(scope.allows_schema("app", "app"));

    assert_eq!(DRIVER_ORDER[2], DatabaseKind::MariaDb);
}

#[test]
fn catalog_version_gate_distinguishes_mysql_and_mariadb() {
    assert!(supports_catalog_version_for_kind(
        DatabaseKind::MySql,
        "8.0.36"
    ));
    assert!(!supports_catalog_version_for_kind(
        DatabaseKind::MySql,
        "10.11.8-MariaDB"
    ));
    assert!(supports_catalog_version_for_kind(
        DatabaseKind::MariaDb,
        "10.11.8-MariaDB"
    ));
    assert!(!supports_catalog_version_for_kind(
        DatabaseKind::MariaDb,
        "8.0.36"
    ));
}

#[tokio::test]
async fn connects_to_configured_mariadb_when_test_service_is_available() {
    let Ok(url) = std::env::var("LAZYDB_TEST_MARIADB_URL") else {
        return;
    };
    let imported = lazydb::profile::import_connection_url(&url, Some("MariaDB test")).unwrap();
    let connection = lazydb::db::DatabaseConnection::connect(
        &imported.profile,
        imported.transient_password.as_ref(),
    )
    .await
    .unwrap();
    let info = connection.probe().await.unwrap();
    assert_eq!(info.kind, DatabaseKind::MariaDb);
    assert!(info.version.to_ascii_lowercase().contains("mariadb"));
    connection.close().await;
}
