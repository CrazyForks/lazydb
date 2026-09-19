mod support;

use lazydb::{
    db::{
        DatabaseConnection,
        catalog::{
            CatalogCount, CatalogCursor, CatalogKind, CatalogRequest, CatalogRequestKey,
            CatalogSearchObjectScope, CatalogSearchRequest, CatalogTarget, ObjectGroup,
        },
    },
    identity::ConnectionIdentity,
    profile::{CatalogScope, CatalogSelection, DatabaseScope, import_connection_url},
};

fn request(
    profile_id: uuid::Uuid,
    target: CatalogTarget,
    scope: CatalogScope,
    page_size: usize,
    cursor: Option<CatalogCursor>,
    request_id: u64,
) -> CatalogRequest {
    CatalogRequest {
        key: CatalogRequestKey {
            connection: ConnectionIdentity {
                profile_id,
                generation: 7,
            },
            catalog_epoch: 1,
            request_id,
            target,
            cursor,
        },
        scope,
        page_size,
    }
}

#[tokio::test]
async fn mariadb_11_4_catalog_loads_tables_views_and_sequences() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB catalog test")).unwrap();
    let profile_id = imported.profile.id;
    let database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let database_name = database.probe().await.unwrap().database;
    assert!(!database_name.is_empty());
    let scope = CatalogScope {
        databases: CatalogSelection::Selected(vec![DatabaseScope {
            name: database_name.clone(),
            schemas: CatalogSelection::All,
        }]),
    };
    let schema_id = lazydb::db::catalog::CatalogId::new(
        profile_id,
        CatalogKind::Schema,
        [database_name.clone(), database_name.clone()],
    );
    let prefix = format!("lazydb_catalog_{}", uuid::Uuid::new_v4().simple());
    let table_name = format!("{prefix}_table");
    let view_name = format!("{prefix}_view");
    let q_table = lazydb::db::mysql::quote_identifier(&table_name);
    let q_view = lazydb::db::mysql::quote_identifier(&view_name);
    let sequence_one = format!("{prefix}_one");
    let sequence_two = format!("{prefix}_two");
    let q_one = lazydb::db::mysql::quote_identifier(&sequence_one);
    let q_two = lazydb::db::mysql::quote_identifier(&sequence_two);
    database
        .execute(&format!(
            "CREATE TABLE {q_table} (id INT PRIMARY KEY, value VARCHAR(32)); \
             CREATE VIEW {q_view} AS SELECT id, value FROM {q_table}; \
             CREATE SEQUENCE {q_one}; CREATE SEQUENCE {q_two}"
        ))
        .await
        .unwrap();

    let result = async {
        let groups_target = CatalogTarget::groups(schema_id.clone()).unwrap();
        let groups_request = request(profile_id, groups_target, scope.clone(), 100, None, 1);
        let groups = database.load_catalog_page(&groups_request).await?;
        groups.validate_for(&groups_request).unwrap();
        assert!(groups.group_summaries.iter().any(|summary| {
            summary.group == ObjectGroup::Tables
                && matches!(summary.object_count, CatalogCount::Exact(count) if count >= 1)
        }));
        assert!(groups.group_summaries.iter().any(|summary| {
            summary.group == ObjectGroup::Views
                && matches!(summary.object_count, CatalogCount::Exact(count) if count >= 1)
        }));
        assert_eq!(
            groups
                .group_summaries
                .iter()
                .find(|summary| summary.group == ObjectGroup::Sequences)
                .unwrap()
                .object_count,
            CatalogCount::Exact(2)
        );

        let sequence_target =
            CatalogTarget::objects(schema_id.clone(), ObjectGroup::Sequences).unwrap();
        let sequence_request = request(
            profile_id,
            sequence_target.clone(),
            scope.clone(),
            1,
            None,
            2,
        );
        let sequences = database.load_catalog_page(&sequence_request).await?;
        sequences.validate_for(&sequence_request).unwrap();
        assert_eq!(sequences.entries.len(), 1);
        let next_cursor = sequences.next_cursor.clone().unwrap();
        let sequence_page_request = request(
            profile_id,
            sequence_target,
            scope.clone(),
            1,
            Some(next_cursor),
            3,
        );
        let second_sequences = database.load_catalog_page(&sequence_page_request).await?;
        second_sequences
            .validate_for(&sequence_page_request)
            .unwrap();
        assert_eq!(second_sequences.entries.len(), 1);
        assert!(second_sequences.next_cursor.is_none());
        let all_sequences = sequences
            .entries
            .iter()
            .chain(second_sequences.entries.iter())
            .collect::<Vec<_>>();
        assert!(all_sequences.iter().all(|entry| {
            entry.kind == CatalogKind::Sequence
                && entry.id.native_path.len() == 3
                && !entry.expandable
        }));
        assert!(
            all_sequences
                .iter()
                .any(|entry| entry.qualified_name.object == sequence_one)
        );
        assert!(
            all_sequences
                .iter()
                .any(|entry| entry.qualified_name.object == sequence_two)
        );

        let search_request = CatalogSearchRequest {
            connection: ConnectionIdentity {
                profile_id,
                generation: 7,
            },
            session_id: 1,
            generation: 1,
            query: prefix,
            scope,
            object_scope: CatalogSearchObjectScope::AllObjects,
            limit: 100,
        };
        let search = database.search_catalog(&search_request).await?;
        search.validate_for(&search_request).unwrap();
        assert_eq!(
            search
                .hits
                .iter()
                .filter(|hit| hit.entry.kind == CatalogKind::Sequence)
                .count(),
            2
        );
        Ok::<(), lazydb::db::DatabaseError>(())
    }
    .await;

    database
        .execute(&format!(
            "DROP VIEW {q_view}; DROP SEQUENCE {q_one}; DROP SEQUENCE {q_two}; DROP TABLE {q_table}"
        ))
        .await
        .unwrap();
    result.unwrap();
    database.close().await;
}
