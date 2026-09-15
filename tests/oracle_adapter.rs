#![cfg(feature = "driver-oracle")]

use lazydb::{
    db::{
        DatabaseConnection,
        catalog::{CatalogRequest, CatalogRequestKey, CatalogTarget},
        catalog_mutation::{
            CatalogMutationAnchor, CatalogMutationMode, CatalogMutationRequest, CatalogObjectType,
        },
    },
    identity::ConnectionIdentity,
    profile::import_connection_url,
    sql::{SqlDialect, build_paginated_query},
};
use secrecy::SecretString;
use uuid::Uuid;

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
async fn oracle_create_table_with_default_types_round_trips_when_configured() {
    let (Ok(url), Ok(user), Ok(password)) = (
        std::env::var("LAZYDB_TEST_ORACLE_URL"),
        std::env::var("LAZYDB_TEST_ORACLE_USER"),
        std::env::var("LAZYDB_TEST_ORACLE_PASSWORD"),
    ) else {
        eprintln!("Skipping Oracle mutation test: Oracle test credentials are not configured");
        return;
    };

    let mut imported = import_connection_url(&url, Some("oracle-mutation-test")).unwrap();
    imported.profile.user = Some(user);
    let connection =
        DatabaseConnection::connect(&imported.profile, Some(&SecretString::from(password)))
            .await
            .unwrap();
    let Some(schema) = imported.profile.user.clone() else {
        panic!("Oracle mutation test requires a configured user/schema");
    };
    let table_name = format!("LAZYDB_T_{}", Uuid::new_v4().simple());
    let profile_id = imported.profile.id;
    let request = CatalogMutationRequest {
        connection: ConnectionIdentity {
            profile_id,
            generation: 1,
        },
        request_id: 1,
        catalog_epoch: 1,
        mode: CatalogMutationMode::Create,
        anchor: CatalogMutationAnchor::Group {
            schema: lazydb::db::catalog::CatalogId::new(
                profile_id,
                lazydb::db::catalog::CatalogKind::Schema,
                [
                    imported.profile.database.clone().unwrap_or_default(),
                    schema.clone(),
                ],
            ),
            group: lazydb::db::catalog::ObjectGroup::Tables,
        },
        object_type: CatalogObjectType::Catalog(lazydb::db::catalog::CatalogKind::Table),
        current_database: imported.profile.database.clone(),
    };
    let mut table = lazydb::model::catalog_editor::TableDraft::new_for_database(
        schema.clone(),
        lazydb::profile::DatabaseKind::Oracle,
    );
    table.name.set(&table_name);
    table.columns[0].name.set("name");
    let plan = connection
        .plan_catalog_mutation(
            request,
            lazydb::model::catalog_editor::CatalogDraft::Table(table),
            None,
        )
        .unwrap();
    connection.execute_catalog_mutation(&plan).await.unwrap();

    let check = format!(
        "SELECT DATA_TYPE, CHAR_LENGTH FROM USER_TAB_COLUMNS WHERE TABLE_NAME = '{}' AND COLUMN_NAME = 'NAME'",
        table_name
    );
    let result = connection.execute(&check).await.unwrap();
    let rows = &result.result_sets[0].rows;
    assert!(rows.iter().any(|row| {
        row.first()
            .is_some_and(|value| value.clipboard_text().contains("VARCHAR2"))
    }));

    let drop = format!("DROP TABLE \"{}\" PURGE", table_name);
    let _ = connection.execute(&drop).await;
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
    assert_eq!(
        groups_page.total_count,
        lazydb::db::catalog::CatalogCount::Exact(3)
    );
    assert!(groups_page.group_summaries.iter().all(|summary| matches!(
        summary.object_count,
        lazydb::db::catalog::CatalogCount::Exact(_)
    )));

    let mut table_entries = Vec::new();
    for (request_id, group) in [
        (4, lazydb::db::catalog::ObjectGroup::Tables),
        (5, lazydb::db::catalog::ObjectGroup::Views),
        (6, lazydb::db::catalog::ObjectGroup::Sequences),
    ] {
        let mut objects_request = CatalogRequest {
            key: CatalogRequestKey {
                connection: identity,
                catalog_epoch: 1,
                request_id,
                target: CatalogTarget::Objects {
                    schema: schema.clone(),
                    group,
                },
                cursor: None,
            },
            scope: imported.profile.catalog_scope.clone(),
            page_size: 2,
        };
        let mut expected_total = None;
        let mut seen = std::collections::HashSet::new();
        loop {
            let page = connection
                .load_catalog_page(&objects_request)
                .await
                .unwrap();
            page.validate_for(&objects_request).unwrap();
            if let Some(total) = expected_total {
                assert_eq!(page.total_count, total);
            } else {
                assert!(matches!(
                    page.total_count,
                    lazydb::db::catalog::CatalogCount::Exact(_)
                ));
                expected_total = Some(page.total_count);
            }
            for entry in &page.entries {
                assert!(seen.insert(entry.id.clone()));
            }
            if group == lazydb::db::catalog::ObjectGroup::Tables {
                table_entries.extend(page.entries.iter().cloned());
            }
            let Some(cursor) = page.next_cursor else {
                break;
            };
            objects_request.key.request_id += 1;
            objects_request.key.cursor = Some(cursor);
        }
        assert_eq!(
            expected_total,
            Some(lazydb::db::catalog::CatalogCount::Exact(seen.len() as u64))
        );
    }
    assert!(!table_entries.is_empty());
    let relation = table_entries[0].id.clone();
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
    let scoped_ddl = connection
        .relation_ddl_with_scope(&relation, &imported.profile.catalog_scope)
        .await
        .unwrap();
    assert!(scoped_ddl.sql.contains("CREATE TABLE"));
    connection.close().await;
}

#[tokio::test]
async fn oracle_sequences_catalog_loads_when_configured() {
    let (Ok(url), Ok(user), Ok(password)) = (
        std::env::var("LAZYDB_TEST_ORACLE_URL"),
        std::env::var("LAZYDB_TEST_ORACLE_USER"),
        std::env::var("LAZYDB_TEST_ORACLE_PASSWORD"),
    ) else {
        return;
    };
    let mut imported = import_connection_url(&url, Some("oracle-sequences-test")).unwrap();
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
    let database_request = CatalogRequest {
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
    let database_page = connection
        .load_catalog_page(&database_request)
        .await
        .unwrap();
    let database = database_page.entries[0].id.clone();
    let schema_request = CatalogRequest {
        key: CatalogRequestKey {
            connection: identity,
            catalog_epoch: 1,
            request_id: 2,
            target: CatalogTarget::Schemas { database },
            cursor: None,
        },
        scope: imported.profile.catalog_scope.clone(),
        page_size: 10,
    };
    let schema_page = connection.load_catalog_page(&schema_request).await.unwrap();
    let schema = schema_page.entries[0].id.clone();
    let sequence_request = CatalogRequest {
        key: CatalogRequestKey {
            connection: identity,
            catalog_epoch: 1,
            request_id: 3,
            target: CatalogTarget::Objects {
                schema: schema.clone(),
                group: lazydb::db::catalog::ObjectGroup::Sequences,
            },
            cursor: None,
        },
        scope: imported.profile.catalog_scope.clone(),
        page_size: 10,
    };
    let sequence_page = connection
        .load_catalog_page(&sequence_request)
        .await
        .unwrap();
    for entry in &sequence_page.entries {
        assert_eq!(entry.id.kind, lazydb::db::catalog::CatalogKind::Sequence);
        assert_eq!(entry.parent_id.as_ref(), Some(&schema));
        assert_eq!(
            entry.qualified_name.schema,
            schema.native_path.last().cloned()
        );
    }
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

#[tokio::test]
#[ignore = "requires a configured Oracle 12c+ test database"]
async fn oracle_query_pagination_required() {
    let url = std::env::var("LAZYDB_TEST_ORACLE_URL").expect("LAZYDB_TEST_ORACLE_URL is required");
    let user =
        std::env::var("LAZYDB_TEST_ORACLE_USER").expect("LAZYDB_TEST_ORACLE_USER is required");
    let password = std::env::var("LAZYDB_TEST_ORACLE_PASSWORD")
        .expect("LAZYDB_TEST_ORACLE_PASSWORD is required");
    let mut imported = import_connection_url(&url, Some("oracle-pagination-required")).unwrap();
    imported.profile.user = Some(user);
    let connection =
        DatabaseConnection::connect(&imported.profile, Some(&SecretString::from(password)))
            .await
            .expect("Oracle connection must be available for required pagination test");

    let source = (1..=25)
        .map(|id| format!("SELECT CAST({id} AS NUMBER(10,0)) AS id FROM dual"))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let source = format!("SELECT id FROM ({source}) ordered_rows ORDER BY id");
    let page_size = lazydb::model::pagination::PageSize::Ten;
    for (offset, expected_visible, expected_next) in [(0, 10, true), (10, 10, true), (20, 5, false)]
    {
        let page = lazydb::model::pagination::PageRequest::at(page_size, offset);
        let query = build_paginated_query(&source, SqlDialect::Oracle, page)
            .expect("test source should be a supported read-only query");
        let outcome = connection.execute(&query.page_sql).await.unwrap();
        let fetched = outcome.stats.row_count;
        let pagination = lazydb::model::pagination::ResultPagination::from_page(page, fetched);
        assert_eq!(pagination.visible_rows, expected_visible);
        assert_eq!(pagination.has_next, expected_next);
    }

    let count_query = build_paginated_query(
        &source,
        SqlDialect::Oracle,
        lazydb::model::pagination::PageRequest::first(page_size),
    )
    .unwrap();
    let count = connection.execute(&count_query.count_sql).await.unwrap();
    assert_eq!(
        count.result_sets[0].rows[0][0],
        lazydb::db::value::CellValue::Integer(25)
    );
    connection.close().await;
}
