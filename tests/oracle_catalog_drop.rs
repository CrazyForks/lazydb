#![cfg(feature = "driver-oracle")]

use lazydb::{
    db::{
        DatabaseConnection,
        catalog::{CatalogEntry, CatalogId, CatalogKind, OptionalMetadata, QualifiedName},
        catalog_drop::CatalogDropRequest,
    },
    identity::ConnectionIdentity,
    profile::import_connection_url,
};
use secrecy::SecretString;
use uuid::Uuid;

async fn oracle_connection() -> Option<(DatabaseConnection, lazydb::profile::ConnectionProfile)> {
    let (Ok(url), Ok(user), Ok(password)) = (
        std::env::var("LAZYDB_TEST_ORACLE_URL"),
        std::env::var("LAZYDB_TEST_ORACLE_USER"),
        std::env::var("LAZYDB_TEST_ORACLE_PASSWORD"),
    ) else {
        eprintln!("Skipping Oracle catalog drop test: Oracle credentials are not configured");
        return None;
    };
    let mut imported = import_connection_url(&url, Some("oracle-catalog-drop-test")).unwrap();
    imported.profile.user = Some(user);
    let connection =
        match DatabaseConnection::connect(&imported.profile, Some(&SecretString::from(password)))
            .await
        {
            Ok(connection) => connection,
            Err(error) if error.message.contains("DPI-1047") => {
                eprintln!("Skipping Oracle catalog drop test: Oracle client is unavailable");
                return None;
            }
            Err(error) => panic!("Oracle connection failed: {error}"),
        };
    Some((connection, imported.profile))
}

fn table_entry(profile_id: Uuid, database: &str, schema: &str, table: &str) -> CatalogEntry {
    let id = CatalogId::new(profile_id, CatalogKind::Table, [database, schema, table]);
    CatalogEntry {
        id,
        parent_id: Some(CatalogId::new(
            profile_id,
            CatalogKind::Schema,
            [database, schema],
        )),
        kind: CatalogKind::Table,
        native_kind: "table".into(),
        qualified_name: QualifiedName {
            database: Some(database.into()),
            schema: Some(schema.into()),
            object: table.into(),
        },
        comment: OptionalMetadata::Unsupported,
        metadata: Default::default(),
        expandable: false,
        relation_id: None,
    }
}

#[tokio::test]
async fn oracle_catalog_drop_round_trips_a_generated_table() {
    let Some((connection, profile)) = oracle_connection().await else {
        return;
    };
    let schema = profile.user.clone().expect("Oracle test user is required");
    let database = profile
        .database
        .clone()
        .expect("Oracle service is required");
    let table = format!("LAZYDB_D_{}", Uuid::new_v4().simple());
    let create = format!(
        "CREATE TABLE {}.{} (ID NUMBER)",
        connection.quote_identifier(&schema),
        connection.quote_identifier(&table)
    );
    connection.execute(&create).await.unwrap();

    let entry = table_entry(profile.id, &database, &schema, &table);
    let request = CatalogDropRequest::new(
        ConnectionIdentity {
            profile_id: profile.id,
            generation: 1,
        },
        entry.id.clone(),
        1,
    )
    .with_entry(entry.clone());
    let plan = connection.plan_catalog_drop(request, &entry).unwrap();
    assert!(plan.sql().starts_with("DROP TABLE"));
    connection.execute(plan.sql()).await.unwrap();
    let check = connection
        .execute(&format!(
            "SELECT TABLE_NAME FROM ALL_TABLES WHERE OWNER = '{}' AND TABLE_NAME = '{}'",
            schema.replace('\'', "''").to_ascii_uppercase(),
            table.replace('\'', "''").to_ascii_uppercase()
        ))
        .await
        .unwrap();
    assert_eq!(check.stats.row_count, 0);
    connection.close().await;
}
