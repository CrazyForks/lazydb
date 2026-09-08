use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use lazydb::{
    db::{
        DatabaseConnection,
        catalog::{CatalogKind, CatalogSearchRequest},
        mutation::{
            DeleteRowMutation, InputValue, InsertRowMutation, RelationMutation,
            RelationMutationRequest, RowLocator, RowVersion, UpdateCellMutation,
            metadata_fingerprint,
        },
        transaction::TransactionBackend,
        value::CellValue,
    },
    identity::ConnectionIdentity,
    model::{
        pagination::{PageRequest, PageSize},
        relation::RelationPreviewOptions,
    },
    profile::{CatalogScope, CatalogSelection, DatabaseScope, import_connection_url},
};
use uuid::Uuid;

fn test_scope(database: &str, schema: &str) -> CatalogScope {
    CatalogScope {
        databases: CatalogSelection::Selected(vec![DatabaseScope {
            name: database.to_owned(),
            schemas: CatalogSelection::Selected(vec![schema.to_owned()]),
        }]),
    }
}

async fn find_relation(
    database: &DatabaseConnection,
    profile_id: Uuid,
    scope: &CatalogScope,
    name: &str,
    kind: CatalogKind,
) -> lazydb::db::catalog::CatalogId {
    database
        .search_catalog(&CatalogSearchRequest {
            connection: ConnectionIdentity {
                profile_id,
                generation: 1,
            },
            session_id: 1,
            generation: 1,
            query: name.to_owned(),
            scope: scope.clone(),
            limit: 20,
        })
        .await
        .unwrap()
        .hits
        .into_iter()
        .find(|hit| hit.entry.kind == kind && hit.entry.qualified_name.object == name)
        .unwrap()
        .entry
        .id
}

#[tokio::test]
async fn preview_relation_versions_align_with_supported_rows_and_pagination() {
    let Ok(url) = std::env::var("LAZYDB_TEST_POSTGRES_URL") else {
        eprintln!("skipping PostgreSQL preview regression: LAZYDB_TEST_POSTGRES_URL is not set");
        return;
    };
    let imported = import_connection_url(&url, Some("postgres-preview")).unwrap();
    let profile_id = imported.profile.id;
    let mut database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let database_name = database.probe().await.unwrap().database;
    let suffix = Uuid::new_v4().simple().to_string();
    let schema = format!("lazydb_preview_{suffix}");
    let qschema = quote_identifier(&schema);
    database
        .execute(&format!("CREATE SCHEMA {qschema}"))
        .await
        .unwrap();
    database.close().await;
    let mut profile = imported.profile.clone();
    profile.catalog_scope = test_scope(&database_name, &schema);
    database = DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
        .await
        .unwrap();
    let result = AssertUnwindSafe(async {
        database.execute(&format!(
            "CREATE TABLE {qschema}.ordinary (id integer PRIMARY KEY, __lazydb_row_version text);\
             INSERT INTO {qschema}.ordinary SELECT value, 'business' FROM generate_series(1, 501) AS value;\
             CREATE TABLE {qschema}.empty (id integer PRIMARY KEY);\
             CREATE VIEW {qschema}.ordinary_view AS SELECT * FROM {qschema}.ordinary;\
             CREATE TABLE {qschema}.inherit_parent (id integer PRIMARY KEY);\
             CREATE TABLE {qschema}.inherit_child () INHERITS ({qschema}.inherit_parent);\
             CREATE TABLE {qschema}.partitioned (id integer) PARTITION BY RANGE (id);\
             CREATE TABLE {qschema}.partition_leaf PARTITION OF {qschema}.partitioned FOR VALUES FROM (0) TO (100)"
        )).await.unwrap();
        let scope = test_scope(&database_name, &schema);
        let ordinary = find_relation(&database, profile_id, &scope, "ordinary", CatalogKind::Table).await;
        let preview = database.preview_relation(&ordinary, &Default::default(), PageRequest::first(PageSize::Ten)).await.unwrap();
        assert_eq!(preview.result.result_sets[0].columns.len(), 2);
        assert_eq!(preview.result.result_sets[0].rows.len(), 10);
        assert_eq!(preview.row_versions.as_ref().unwrap().len(), 10);
        assert!(preview.pagination.has_next);
        assert!(preview.sql.contains("LIMIT 11 OFFSET 0"));
        let second_page = database.preview_relation(&ordinary, &Default::default(), PageRequest::at(PageSize::Ten, 10)).await.unwrap();
        assert_eq!(second_page.result.result_sets[0].rows.len(), 10);
        assert_eq!(second_page.row_versions.as_ref().unwrap().len(), 10);
        let large_page = database
            .preview_relation(
                &ordinary,
                &Default::default(),
                PageRequest::first(PageSize::FiveHundred),
            )
            .await
            .unwrap();
        assert_eq!(large_page.result.result_sets[0].rows.len(), 500);
        assert_eq!(large_page.row_versions.as_ref().unwrap().len(), 500);
        assert!(large_page.pagination.has_next);
        let options = RelationPreviewOptions {
            where_clause: Some(format!("{}.id > 500", quote_identifier("ordinary"))),
            order_by_clause: Some(format!("{}.id DESC", quote_identifier("ordinary"))),
        };
        let page = database.preview_relation(&ordinary, &options, PageRequest { size: PageSize::Ten, offset: 0, resolve_total: true }).await.unwrap();
        assert_eq!(page.result.result_sets[0].rows.len(), 1);
        assert_eq!(page.row_versions.as_ref().unwrap().len(), 1);
        assert_eq!(page.pagination.total, lazydb::model::pagination::TotalRows::Exact(1));
        let empty = find_relation(&database, profile_id, &scope, "empty", CatalogKind::Table).await;
        let empty_preview = database.preview_relation(&empty, &Default::default(), PageRequest::first(PageSize::Ten)).await.unwrap();
        assert!(empty_preview.result.result_sets[0].rows.is_empty());
        assert!(empty_preview.row_versions.as_ref().is_some_and(Vec::is_empty));
        for (name, kind) in [("ordinary_view", CatalogKind::View), ("inherit_parent", CatalogKind::Table), ("inherit_child", CatalogKind::Table), ("partitioned", CatalogKind::Table), ("partition_leaf", CatalogKind::Table)] {
            let relation = find_relation(&database, profile_id, &scope, name, kind).await;
            let preview = database.preview_relation(&relation, &Default::default(), PageRequest::first(PageSize::Ten)).await.unwrap();
            assert_eq!(preview.row_versions, None, "unsupported relation {name} must not expose xmin");
        }
    }).catch_unwind().await;
    database.close().await;
    let cleanup =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap()
            .execute(&format!("DROP SCHEMA IF EXISTS {qschema} CASCADE"))
            .await;
    if let Err(panic) = result {
        cleanup.unwrap();
        std::panic::resume_unwind(panic);
    }
    cleanup.unwrap();
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[tokio::test]
async fn postgres_relation_identity_resolves_oid_after_rename() {
    let Ok(url) = std::env::var("LAZYDB_TEST_POSTGRES_URL") else {
        eprintln!(
            "skipping PostgreSQL relation identity regression: LAZYDB_TEST_POSTGRES_URL is not set"
        );
        return;
    };
    let imported = import_connection_url(&url, Some("postgres-relation-identity")).unwrap();
    let profile_id = imported.profile.id;
    let database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let database_name = database.probe().await.unwrap().database;
    let suffix = Uuid::new_v4().simple().to_string();
    let schema = format!("lazydb_identity_{suffix}");
    let old_name = format!("before_{suffix}");
    let new_name = format!("after_{suffix}");
    let qschema = quote_identifier(&schema);
    let qold = quote_identifier(&old_name);
    let qnew = quote_identifier(&new_name);
    database
        .execute(&format!(
            "CREATE SCHEMA {qschema}; CREATE TABLE {qschema}.{qold} (id integer)"
        ))
        .await
        .unwrap();
    let mut profile = imported.profile.clone();
    profile.catalog_scope = test_scope(&database_name, &schema);
    let scoped = DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
        .await
        .unwrap();
    let stale = find_relation(
        &scoped,
        profile_id,
        &profile.catalog_scope,
        &old_name,
        CatalogKind::Table,
    )
    .await;
    scoped
        .execute(&format!("ALTER TABLE {qschema}.{qold} RENAME TO {qnew}"))
        .await
        .unwrap();

    let resolved = scoped
        .resolve_relation_identity(&stale)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resolved.qualified_name.object, new_name);
    assert_eq!(
        resolved.qualified_name.schema.as_deref(),
        Some(schema.as_str())
    );
    assert_eq!(resolved.id.native_path[2], new_name);
    assert!(
        scoped
            .preview_relation(
                &stale,
                &Default::default(),
                PageRequest::first(PageSize::Ten)
            )
            .await
            .is_err()
    );

    scoped.close().await;
    database.close().await;
    let cleanup =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap()
            .execute(&format!("DROP SCHEMA IF EXISTS {qschema} CASCADE"))
            .await;
    cleanup.unwrap();
}

fn count_rows(outcome: &lazydb::db::query::QueryOutcome) -> i64 {
    match &outcome.result_sets.last().unwrap().rows[0][0] {
        CellValue::Integer(value) => *value,
        CellValue::Unsigned(value) => *value as i64,
        value => panic!("unexpected row count value: {value:?}"),
    }
}

#[tokio::test]
async fn delete_two_all_types_rows() {
    let Ok(url) = std::env::var("LAZYDB_TEST_POSTGRES_URL") else {
        eprintln!(
            "skipping PostgreSQL relation mutation regression: LAZYDB_TEST_POSTGRES_URL is not set"
        );
        return;
    };

    let imported = import_connection_url(&url, Some("postgres-relation-mutations")).unwrap();
    let profile_id = imported.profile.id;
    let mut database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let database_name = database.probe().await.unwrap().database;
    let suffix = Uuid::new_v4().simple().to_string();
    let schema = format!("lazydb_row_version_{suffix}");
    let table = format!("all_types_{suffix}");
    let qschema = quote_identifier(&schema);
    let qtable = quote_identifier(&table);
    let qualified_table = format!("{qschema}.{qtable}");

    database
        .execute(&format!("CREATE SCHEMA {qschema}"))
        .await
        .unwrap();
    database.close().await;
    let mut profile = imported.profile.clone();
    profile.catalog_scope = test_scope(&database_name, &schema);
    database = DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
        .await
        .unwrap();

    let result = AssertUnwindSafe(async {
        database
            .execute(&format!(
                "CREATE TABLE {qualified_table} (\
                   id bigint PRIMARY KEY, short_text varchar(80), exact_num numeric(12,2),\
                   is_active boolean, small_num smallint, whole_num integer, big_num bigint,\
                   ratio real, precise double precision, event_date date, event_time time,\
                   event_timestamp timestamp, event_tz timestamptz, duration interval,\
                   token uuid, metadata json, attributes jsonb, tags text[], ip_address inet,\
                   location point, nullable_text text, nullable_num numeric,\
                   nullable_json jsonb, nullable_tags text[]\
                 );\
                 INSERT INTO {qualified_table} VALUES\
                   (1, 'first', 12.34, true, 2, 20, 200, 1.25, 2.5, '2026-01-02',\
                    '03:04:05', '2026-01-02 03:04:05', '2026-01-02 03:04:05+00',\
                    interval '1 month 2 days 3 hours', '00000000-0000-4000-8000-000000000001',\
                    '{{\"kind\":\"one\"}}', '{{\"rank\":1}}', '{{one,\"two words\"}}',\
                    '192.0.2.1', point(1,2), NULL, NULL, NULL, NULL),\
                   (2, 'second', 56.78, false, 3, 30, 300, 2.5, 5.0, '2026-02-03',\
                    '04:05:06', '2026-02-03 04:05:06', '2026-02-03 04:05:06+00',\
                    interval '-4 days 5.25 seconds', '00000000-0000-4000-8000-000000000002',\
                    '{{\"kind\":\"two\"}}', '{{\"rank\":2}}', '{{three,four}}',\
                    '2001:db8::1', point(-3,4), NULL, NULL, NULL, NULL),\
                   (3, 'third', 0.01, true, 4, 40, 400, 3.75, 7.5, '2026-03-04',\
                    '05:06:07', '2026-03-04 05:06:07', '2026-03-04 05:06:07+00',\
                    interval '0 seconds', '00000000-0000-4000-8000-000000000003',\
                    '{{\"kind\":\"three\"}}', '{{\"rank\":3}}', '{{five}}',\
                    '198.51.100.3', point(5,6), NULL, NULL, NULL, NULL),\
                   (4, 'fourth', 99.99, false, 5, 50, 500, 4.0, 10.0, '2026-04-05',\
                    '06:07:08', '2026-04-05 06:07:08', '2026-04-05 06:07:08+00',\
                    interval '2 mons', '00000000-0000-4000-8000-000000000004',\
                    '{{\"kind\":\"four\"}}', '{{\"rank\":4}}', '{{six}}',\
                    '203.0.113.4', point(7,8), 'nullable', 10.00, '{{\"present\":true}}', '{{seven}}')"
            ))
            .await
            .unwrap();

        let scope = test_scope(&database_name, &schema);
        let search = database
            .search_catalog(&CatalogSearchRequest {
                connection: ConnectionIdentity { profile_id, generation: 1 },
                session_id: 1,
                generation: 1,
                query: table.clone(),
                scope: scope.clone(),
                limit: 10,
            })
            .await
            .unwrap();
        let relation = search
            .hits
            .into_iter()
            .find(|hit| hit.entry.kind == CatalogKind::Table && hit.entry.qualified_name.object == table)
            .unwrap()
            .entry
            .id;
        let ddl = database.relation_ddl(&relation).await.unwrap();
        let metadata = metadata_fingerprint(&ddl);
        assert_eq!(metadata.columns.len(), 24);
        assert_eq!(metadata.primary_key, vec!["id"]);

        let preview = database
            .preview_relation(&relation, &Default::default(), PageRequest::first(PageSize::Ten))
            .await
            .unwrap();
        assert_eq!(preview.result.result_sets.last().unwrap().rows.len(), 4);
        assert!(preview.result.result_sets.last().unwrap().rows.iter().all(|row| row.len() == 24));

        let versions = preview.row_versions.as_ref().unwrap();
        let rows = &preview.result.result_sets.last().unwrap().rows;
        let mut backend = match &database {
            DatabaseConnection::Postgres(adapter) => adapter.transaction_backend().await.unwrap(),
            _ => unreachable!(),
        };
        let mut request = RelationMutationRequest {
            tab_id: Uuid::nil(),
            tab_generation: 1,
            edit_generation: 1,
            row_id: lazydb::model::relation_edit::EditableRowId(1),
            connection: ConnectionIdentity { profile_id, generation: 1 },
            target: lazydb::model::execution_target::ExecutionTarget {
                profile_id,
                database: database_name.clone(),
                schema: Some(schema.clone()),
            },
            relation: relation.clone(),
            relation_key: lazydb::model::relation::RelationKey {
                profile_id,
                object_id: relation.clone(),
            },
            scope: scope.clone(),
            metadata,
            operation: RelationMutation::DeleteRows(
                rows.iter()
                    .take(2)
                    .enumerate()
                    .map(|(index, row)| DeleteRowMutation {
                        row_id: lazydb::model::relation_edit::EditableRowId(index as u64 + 1),
                        row: RowLocator {
                            columns: vec![0],
                            values: vec![row[0].clone()],
                        },
                        original: row.clone(),
                        version: Some(versions[index]),
                    })
                    .collect(),
            ),
        };
        let external =
            DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
                .await
                .unwrap();
        external
            .execute(&format!(
                "UPDATE {qualified_table} SET short_text = 'external' WHERE id = 2"
            ))
            .await
            .unwrap();
        external.close().await;
        let mut externally_stale = request.clone();
        if let RelationMutation::DeleteRows(rows) = &mut externally_stale.operation {
            rows.truncate(2);
        }
        backend.begin().await.unwrap();
        assert!(backend.relation_mutation(externally_stale).await.is_err());
        backend.rollback().await.unwrap();
        let mut missing_version = request.clone();
        if let RelationMutation::DeleteRows(rows) = &mut missing_version.operation {
            rows[0].version = None;
        }
        backend.begin().await.unwrap();
        assert!(backend.relation_mutation(missing_version).await.is_err());
        backend.rollback().await.unwrap();

        let mut stale_version = request.clone();
        if let RelationMutation::DeleteRows(rows) = &mut stale_version.operation {
            rows[0].version = Some(RowVersion::PostgresXmin(0));
        }
        backend.begin().await.unwrap();
        assert!(backend.relation_mutation(stale_version).await.is_err());
        backend.rollback().await.unwrap();

        assert_eq!(
            count_rows(
                &database
                    .execute(&format!("SELECT COUNT(*) FROM {qualified_table}"))
                    .await
                    .unwrap()
            ),
            4
        );

        let mut malformed_locator = request.clone();
        if let RelationMutation::DeleteRows(rows) = &mut malformed_locator.operation {
            rows[0].row.columns = vec![];
        }
        backend.begin().await.unwrap();
        assert!(backend.relation_mutation(malformed_locator).await.is_err());
        backend.rollback().await.unwrap();

        let mut atomic_conflict = request.clone();
        if let RelationMutation::DeleteRows(rows) = &mut atomic_conflict.operation {
            rows[1].version = Some(RowVersion::PostgresXmin(4_000_000_000));
        }
        backend.begin().await.unwrap();
        assert!(backend.relation_mutation(atomic_conflict).await.is_err());
        backend.rollback().await.unwrap();
        assert_eq!(
            count_rows(
                &database
                    .execute(&format!("SELECT COUNT(*) FROM {qualified_table}"))
                    .await
                    .unwrap()
            ),
            4
        );

        let mut first_then_stale = request.clone();
        if let RelationMutation::DeleteRows(rows) = &mut first_then_stale.operation {
            rows.truncate(1);
        }
        let mut stale_after_success = request.clone();
        if let RelationMutation::DeleteRows(rows) = &mut stale_after_success.operation {
            rows.truncate(1);
            rows[0].row_id = lazydb::model::relation_edit::EditableRowId(2);
            rows[0].row = match &request.operation {
                RelationMutation::DeleteRows(original) => original[1].row.clone(),
                _ => unreachable!(),
            };
            rows[0].original = match &request.operation {
                RelationMutation::DeleteRows(original) => original[1].original.clone(),
                _ => unreachable!(),
            };
            rows[0].version = Some(RowVersion::PostgresXmin(4_000_000_000));
        }
        backend.begin().await.unwrap();
        assert_eq!(
            backend.relation_mutation(first_then_stale).await.unwrap(),
            lazydb::db::mutation::MutationResult::Deleted { rows: 1 }
        );
        assert!(backend.relation_mutation(stale_after_success).await.is_err());
        backend.rollback().await.unwrap();
        assert_eq!(
            count_rows(
                &database
                    .execute(&format!("SELECT COUNT(*) FROM {qualified_table}"))
                    .await
                    .unwrap()
            ),
            4
        );

        let current_version = database
            .execute(&format!(
                "SELECT xmin::text FROM {qualified_table} WHERE id = 2"
            ))
            .await
            .unwrap()
            .result_sets
            .last()
            .and_then(|set| set.rows.first())
            .and_then(|row| match &row[0] {
                CellValue::Text(value) => value.parse().ok(),
                _ => None,
            })
            .map(RowVersion::PostgresXmin)
            .expect("row 2 must have a current xmin");
        if let RelationMutation::DeleteRows(rows) = &mut request.operation {
            rows[1].version = Some(current_version);
        }
        backend.begin().await.unwrap();
        assert_eq!(
            backend.relation_mutation(request).await.unwrap(),
            lazydb::db::mutation::MutationResult::Deleted { rows: 2 }
        );
        backend.commit().await.unwrap();
    })
    .catch_unwind()
    .await;

    database.close().await;
    let verification = DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
        .await
        .unwrap();
    assert_eq!(
        count_rows(
            &verification
                .execute(&format!("SELECT COUNT(*) FROM {qualified_table}"))
                .await
                .unwrap()
        ),
        2
    );
    let cleanup = verification
        .execute(&format!("DROP SCHEMA IF EXISTS {qschema} CASCADE"))
        .await;
    verification.close().await;
    if let Err(panic) = result {
        cleanup.unwrap_or_else(|error| panic!("fixture cleanup failed after test panic: {error}"));
        std::panic::resume_unwind(panic);
    }
    cleanup.unwrap();
}

#[tokio::test]
async fn returning_versions_support_update_and_insert_then_delete() {
    let Ok(url) = std::env::var("LAZYDB_TEST_POSTGRES_URL") else {
        eprintln!("skipping PostgreSQL RETURNING regression: LAZYDB_TEST_POSTGRES_URL is not set");
        return;
    };

    let imported = import_connection_url(&url, Some("postgres-returning")).unwrap();
    let profile_id = imported.profile.id;
    let mut database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let database_name = database.probe().await.unwrap().database;
    let suffix = Uuid::new_v4().simple().to_string();
    let schema = format!("lazydb_returning_{suffix}");
    let table = format!("items_{suffix}");
    let qschema = quote_identifier(&schema);
    let qtable = quote_identifier(&table);
    let qualified_table = format!("{qschema}.{qtable}");
    database
        .execute(&format!(
            "CREATE SCHEMA {qschema}; CREATE TABLE {qualified_table} (id integer PRIMARY KEY, value text NOT NULL DEFAULT 'default')"
        ))
        .await
        .unwrap();
    database.close().await;

    let mut profile = imported.profile.clone();
    profile.catalog_scope = test_scope(&database_name, &schema);
    database = DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
        .await
        .unwrap();
    let result = AssertUnwindSafe(async {
        database
            .execute(&format!(
                "INSERT INTO {qualified_table} (id, value) VALUES (1, 'before')"
            ))
            .await
            .unwrap();
        let scope = test_scope(&database_name, &schema);
        let relation =
            find_relation(&database, profile_id, &scope, &table, CatalogKind::Table).await;
        let ddl = database.relation_ddl(&relation).await.unwrap();
        let metadata = metadata_fingerprint(&ddl);
        let target = lazydb::model::execution_target::ExecutionTarget {
            profile_id,
            database: database_name.clone(),
            schema: Some(schema.clone()),
        };
        let relation_key = lazydb::model::relation::RelationKey {
            profile_id,
            object_id: relation.clone(),
        };
        let connection = ConnectionIdentity {
            profile_id,
            generation: 1,
        };
        let request = |row_id, operation| RelationMutationRequest {
            tab_id: Uuid::nil(),
            tab_generation: 1,
            edit_generation: 1,
            row_id,
            connection,
            target: target.clone(),
            relation: relation.clone(),
            relation_key: relation_key.clone(),
            scope: scope.clone(),
            metadata: metadata.clone(),
            operation,
        };

        let mut backend = match &database {
            DatabaseConnection::Postgres(adapter) => adapter.transaction_backend().await.unwrap(),
            _ => unreachable!(),
        };
        backend.begin().await.unwrap();
        assert!(matches!(
            backend
                .relation_mutation(request(
                    lazydb::model::relation_edit::EditableRowId(1),
                    RelationMutation::UpdateCell(UpdateCellMutation {
                        row: RowLocator {
                            columns: vec![0],
                            values: vec![CellValue::Integer(1)],
                        },
                        column: 1,
                        original: CellValue::Text("before".into()),
                        value: InputValue::Value(CellValue::Text("first success".into())),
                    }),
                ))
                .await
                .unwrap(),
            lazydb::db::mutation::MutationResult::Updated { .. }
        ));
        assert!(
            backend
                .relation_mutation(request(
                    lazydb::model::relation_edit::EditableRowId(1),
                    RelationMutation::UpdateCell(UpdateCellMutation {
                        row: RowLocator {
                            columns: vec![0],
                            values: vec![CellValue::Integer(1)],
                        },
                        column: 1,
                        original: CellValue::Text("first success".into()),
                        value: InputValue::Null,
                    }),
                ))
                .await
                .is_err()
        );
        backend.rollback().await.unwrap();
        assert_eq!(
            database
                .execute(&format!("SELECT value FROM {qualified_table} WHERE id = 1"))
                .await
                .unwrap()
                .result_sets
                .last()
                .unwrap()
                .rows[0][0],
            CellValue::Text("before".into())
        );

        backend.begin().await.unwrap();
        let update = backend
            .relation_mutation(request(
                lazydb::model::relation_edit::EditableRowId(1),
                RelationMutation::UpdateCell(UpdateCellMutation {
                    row: RowLocator {
                        columns: vec![0],
                        values: vec![CellValue::Integer(1)],
                    },
                    column: 1,
                    original: CellValue::Text("before".into()),
                    value: InputValue::Value(CellValue::Text("after".into())),
                }),
            ))
            .await
            .unwrap();
        let lazydb::db::mutation::MutationResult::Updated { row, version } = update else {
            panic!("expected updated row");
        };
        assert_eq!(row.len(), 2);
        let version = version.expect("UPDATE must return xmin");
        assert!(matches!(version, RowVersion::PostgresXmin(_)));
        backend
            .relation_mutation(request(
                lazydb::model::relation_edit::EditableRowId(1),
                RelationMutation::DeleteRows(vec![DeleteRowMutation {
                    row_id: lazydb::model::relation_edit::EditableRowId(1),
                    row: RowLocator {
                        columns: vec![0],
                        values: vec![row[0].clone()],
                    },
                    original: row,
                    version: Some(version),
                }]),
            ))
            .await
            .unwrap();

        let insert = backend
            .relation_mutation(request(
                lazydb::model::relation_edit::EditableRowId(2),
                RelationMutation::InsertRow(InsertRowMutation {
                    columns: vec![0],
                    values: vec![InputValue::Value(CellValue::Integer(2))],
                }),
            ))
            .await
            .unwrap();
        let lazydb::db::mutation::MutationResult::Inserted { row, version } = insert else {
            panic!("expected inserted row");
        };
        assert_eq!(
            row,
            vec![CellValue::Integer(2), CellValue::Text("default".into())]
        );
        let version = version.expect("INSERT must return xmin");
        backend
            .relation_mutation(request(
                lazydb::model::relation_edit::EditableRowId(2),
                RelationMutation::DeleteRows(vec![DeleteRowMutation {
                    row_id: lazydb::model::relation_edit::EditableRowId(2),
                    row: RowLocator {
                        columns: vec![0],
                        values: vec![row[0].clone()],
                    },
                    original: row,
                    version: Some(version),
                }]),
            ))
            .await
            .unwrap();
        backend.commit().await.unwrap();
        assert_eq!(
            count_rows(
                &database
                    .execute(&format!("SELECT COUNT(*) FROM {qualified_table}"))
                    .await
                    .unwrap()
            ),
            0
        );
    })
    .catch_unwind()
    .await;
    database.close().await;
    let cleanup = DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
        .await
        .unwrap()
        .execute(&format!("DROP SCHEMA IF EXISTS {qschema} CASCADE"))
        .await;
    if let Err(panic) = result {
        cleanup.unwrap();
        std::panic::resume_unwind(panic);
    }
    cleanup.unwrap();
}

#[tokio::test]
async fn type_roundtrip_uses_native_assignment_bindings() {
    let Ok(url) = std::env::var("LAZYDB_TEST_POSTGRES_URL") else {
        eprintln!(
            "skipping PostgreSQL type round-trip regression: LAZYDB_TEST_POSTGRES_URL is not set"
        );
        return;
    };

    let imported = import_connection_url(&url, Some("postgres-type-roundtrip")).unwrap();
    let profile_id = imported.profile.id;
    let mut database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let database_name = database.probe().await.unwrap().database;
    let suffix = Uuid::new_v4().simple().to_string();
    let schema = format!("lazydb_type_roundtrip_{suffix}");
    let table = format!("values_{suffix}");
    let qschema = quote_identifier(&schema);
    let qualified_table = format!("{qschema}.{}", quote_identifier(&table));
    database
        .execute(&format!(
            "CREATE SCHEMA {qschema}; CREATE TABLE {qualified_table} (\
                id integer PRIMARY KEY, exact_num numeric(38,10), duration interval, tags text[],\
                address inet, network cidr, token uuid, document jsonb, location point,\
                optional text DEFAULT 'default', nullable text)"
        ))
        .await
        .unwrap();
    database.close().await;

    let mut profile = imported.profile.clone();
    profile.catalog_scope = test_scope(&database_name, &schema);
    database = DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
        .await
        .unwrap();
    let result = AssertUnwindSafe(async {
        let scope = test_scope(&database_name, &schema);
        let relation =
            find_relation(&database, profile_id, &scope, &table, CatalogKind::Table).await;
        let metadata = metadata_fingerprint(&database.relation_ddl(&relation).await.unwrap());
        let target = lazydb::model::execution_target::ExecutionTarget {
            profile_id,
            database: database_name.clone(),
            schema: Some(schema.clone()),
        };
        let relation_key = lazydb::model::relation::RelationKey {
            profile_id,
            object_id: relation.clone(),
        };
        let connection = ConnectionIdentity {
            profile_id,
            generation: 1,
        };
        let request = |operation| RelationMutationRequest {
            tab_id: Uuid::nil(),
            tab_generation: 1,
            edit_generation: 1,
            row_id: lazydb::model::relation_edit::EditableRowId(1),
            connection,
            target: target.clone(),
            relation: relation.clone(),
            relation_key: relation_key.clone(),
            scope: scope.clone(),
            metadata: metadata.clone(),
            operation,
        };
        let mut backend = match &database {
            DatabaseConnection::Postgres(adapter) => adapter.transaction_backend().await.unwrap(),
            _ => unreachable!(),
        };
        backend.begin().await.unwrap();

        let inserted = backend
            .relation_mutation(request(RelationMutation::InsertRow(InsertRowMutation {
                columns: (0..11).collect(),
                values: vec![
                    InputValue::Value(CellValue::Integer(1)),
                    InputValue::Value(CellValue::Text("123456789012345678.1234567890".into())),
                    InputValue::Value(CellValue::Text("-2 mons 3 days -04:05:06.25".into())),
                    InputValue::Value(CellValue::Text(
                        "{\"NULL\",NULL,\"with,comma\",\"back\\\\slash\"}".into(),
                    )),
                    InputValue::Value(CellValue::Text("192.0.2.42".into())),
                    InputValue::Value(CellValue::Text("2001:db8::/32".into())),
                    InputValue::Value(CellValue::Text(
                        "00000000-0000-4000-8000-000000000001".into(),
                    )),
                    InputValue::Value(CellValue::Text(r#"{"kind":"one"}"#.into())),
                    InputValue::Value(CellValue::Text("(1,2)".into())),
                    InputValue::Default,
                    InputValue::Null,
                ],
            })))
            .await
            .unwrap();
        let lazydb::db::mutation::MutationResult::Inserted { row, version } = inserted else {
            panic!("expected inserted row");
        };
        assert_eq!(
            row[1],
            CellValue::Text("123456789012345678.123456789000".into())
        );
        assert_eq!(
            row[3],
            CellValue::Text(r#"{"NULL",NULL,"with,comma","back\\slash"}"#.into())
        );
        assert_eq!(row[4], CellValue::Text("192.0.2.42".into()));
        assert_eq!(row[5], CellValue::Text("2001:db8::/32".into()));
        assert_eq!(row[9], CellValue::Text("default".into()));
        assert_eq!(row[10], CellValue::Null);

        let version = version.expect("INSERT must return xmin");
        let updated = backend
            .relation_mutation(request(RelationMutation::UpdateCell(UpdateCellMutation {
                row: RowLocator {
                    columns: vec![0],
                    values: vec![row[0].clone()],
                },
                column: 2,
                original: row[2].clone(),
                value: InputValue::Value(CellValue::Text(
                    "1 year 2 mons 3 days 01:15:05.25".into(),
                )),
            })))
            .await
            .unwrap();
        let lazydb::db::mutation::MutationResult::Updated {
            row,
            version: updated_version,
        } = updated
        else {
            panic!("expected updated row");
        };
        assert!(matches!(row[2], CellValue::Text(_)));
        let version = updated_version.unwrap_or(version);
        backend
            .relation_mutation(request(RelationMutation::DeleteRows(vec![
                DeleteRowMutation {
                    row_id: lazydb::model::relation_edit::EditableRowId(1),
                    row: RowLocator {
                        columns: vec![0],
                        values: vec![row[0].clone()],
                    },
                    original: row,
                    version: Some(version),
                },
            ])))
            .await
            .unwrap();
        backend.commit().await.unwrap();
    })
    .catch_unwind()
    .await;
    database.close().await;
    let cleanup = DatabaseConnection::connect(&profile, imported.transient_password.as_ref())
        .await
        .unwrap()
        .execute(&format!("DROP SCHEMA IF EXISTS {qschema} CASCADE"))
        .await;
    if let Err(panic) = result {
        cleanup.unwrap();
        std::panic::resume_unwind(panic);
    }
    cleanup.unwrap();
}
