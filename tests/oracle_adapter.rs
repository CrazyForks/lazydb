#![cfg(feature = "driver-oracle")]

use lazydb::{
    db::{
        DatabaseConnection,
        catalog::{CatalogRequest, CatalogRequestKey, CatalogTarget},
    },
    identity::ConnectionIdentity,
    profile::import_connection_url,
};
use secrecy::SecretString;

#[tokio::test]
async fn oracle_probe_uses_the_configured_service_when_credentials_are_available() {
    let (Ok(url), Ok(user), Ok(password)) = (
        std::env::var("LAZYDB_TEST_ORACLE_URL"),
        std::env::var("LAZYDB_TEST_ORACLE_USER"),
        std::env::var("LAZYDB_TEST_ORACLE_PASSWORD"),
    ) else {
        return;
    };
    let mut imported = import_connection_url(&url, Some("oracle-test")).unwrap();
    imported.profile.user = Some(user);
    let connection =
        match DatabaseConnection::connect(&imported.profile, Some(&SecretString::from(password)))
            .await
        {
            Ok(connection) => connection,
            Err(error) if error.message.contains("DPI-1047") => return,
            Err(error) => panic!("Oracle connection failed: {error}"),
        };
    let info = connection.probe().await.unwrap();
    assert_eq!(info.kind, lazydb::profile::DatabaseKind::Oracle);
    assert_eq!(info.database.to_ascii_lowercase(), "supportdb");
    assert!(!info.version.trim().is_empty());
    connection.close().await;
}

#[tokio::test]
async fn oracle_discovers_and_loads_basic_catalog_when_configured() {
    let (Ok(url), Ok(user), Ok(password)) = (
        std::env::var("LAZYDB_TEST_ORACLE_URL"),
        std::env::var("LAZYDB_TEST_ORACLE_USER"),
        std::env::var("LAZYDB_TEST_ORACLE_PASSWORD"),
    ) else {
        return;
    };
    let mut imported = import_connection_url(&url, Some("oracle-catalog-test")).unwrap();
    imported.profile.user = Some(user);
    let connection =
        match DatabaseConnection::connect(&imported.profile, Some(&SecretString::from(password)))
            .await
        {
            Ok(connection) => connection,
            Err(error) if error.message.contains("DPI-1047") => return,
            Err(error) => panic!("Oracle connection failed: {error}"),
        };
    let identity = ConnectionIdentity {
        profile_id: imported.profile.id,
        generation: 1,
    };
    let request = CatalogRequest {
        key: CatalogRequestKey {
            connection: identity,
            catalog_epoch: 1,
            request_id: 1,
            target: CatalogTarget::Databases,
            cursor: None,
        },
        scope: imported.profile.catalog_scope.clone(),
        page_size: 10,
    };
    let page = connection.load_catalog_page(&request).await.unwrap();
    assert!(!page.entries.is_empty());
    let database = page.entries[0].id.clone();
    let schema_request = CatalogRequest {
        key: CatalogRequestKey {
            connection: identity,
            catalog_epoch: 1,
            request_id: 2,
            target: CatalogTarget::Schemas {
                database: database.clone(),
            },
            cursor: None,
        },
        scope: imported.profile.catalog_scope.clone(),
        page_size: 10,
    };
    let schema_page = connection.load_catalog_page(&schema_request).await.unwrap();
    assert!(!schema_page.entries.is_empty());
    let schema = schema_page.entries[0].id.clone();
    let groups_request = CatalogRequest {
        key: CatalogRequestKey {
            connection: identity,
            catalog_epoch: 1,
            request_id: 3,
            target: CatalogTarget::Groups {
                schema: schema.clone(),
            },
            cursor: None,
        },
        scope: imported.profile.catalog_scope.clone(),
        page_size: 10,
    };
    let groups_page = connection.load_catalog_page(&groups_request).await.unwrap();
    assert_eq!(groups_page.group_summaries.len(), 3);
    let objects_request = CatalogRequest {
        key: CatalogRequestKey {
            connection: identity,
            catalog_epoch: 1,
            request_id: 4,
            target: CatalogTarget::Objects {
                schema,
                group: lazydb::db::catalog::ObjectGroup::Tables,
            },
            cursor: None,
        },
        scope: imported.profile.catalog_scope.clone(),
        page_size: 10,
    };
    let objects_page = connection
        .load_catalog_page(&objects_request)
        .await
        .unwrap();
    assert!(!objects_page.entries.is_empty());
    let relation = objects_page.entries[0].id.clone();
    let preview = connection
        .preview_relation(
            &relation,
            &Default::default(),
            lazydb::model::pagination::PageRequest::first(lazydb::model::pagination::PageSize::Ten),
        )
        .await
        .unwrap();
    assert!(!preview.result.result_sets.is_empty());
    let ddl = connection.relation_ddl(&relation).await.unwrap();
    assert!(ddl.sql.contains("CREATE TABLE"));
    connection.close().await;
}

#[tokio::test]
async fn oracle_reads_typed_read_only_results_when_configured() {
    let (Ok(url), Ok(user), Ok(password)) = (
        std::env::var("LAZYDB_TEST_ORACLE_URL"),
        std::env::var("LAZYDB_TEST_ORACLE_USER"),
        std::env::var("LAZYDB_TEST_ORACLE_PASSWORD"),
    ) else {
        return;
    };
    let mut imported = import_connection_url(&url, Some("oracle-values-test")).unwrap();
    imported.profile.user = Some(user);
    let connection =
        match DatabaseConnection::connect(&imported.profile, Some(&SecretString::from(password)))
            .await
        {
            Ok(connection) => connection,
            Err(error) if error.message.contains("DPI-1047") => return,
            Err(error) => panic!("Oracle connection failed: {error}"),
        };
    let outcome = connection
        .execute(
            "SELECT CAST(42 AS NUMBER(10,0)) AS n, TIMESTAMP '2024-01-02 03:04:05.123456' AS ts, 'Ada' AS name FROM dual",
        )
        .await
        .unwrap();
    let row = &outcome.result_sets[0].rows[0];
    assert_eq!(row[0], lazydb::db::value::CellValue::Integer(42));
    assert!(matches!(row[1], lazydb::db::value::CellValue::DateTime(_)));
    assert_eq!(row[2], lazydb::db::value::CellValue::Text("Ada".into()));
    connection.close().await;
}
