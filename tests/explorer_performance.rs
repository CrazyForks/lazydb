use lazydb::{
    db::catalog::{
        CatalogCompleteness, CatalogCount, CatalogEntry, CatalogId, CatalogKind, ObjectGroup,
        OptionalMetadata, QualifiedName,
    },
    model::explorer::{CatalogGroupState, CatalogTree, ExplorerNodeId, ExplorerTreeState},
};
use uuid::Uuid;

#[test]
fn projection_visits_only_expanded_subtrees_with_ten_thousand_objects() {
    let profile = Uuid::from_u128(1);
    let database = database_entry(profile);
    let mut entries = vec![database.clone()];
    let mut schemas = Vec::with_capacity(100);

    for schema_index in 0..100 {
        let schema = schema_entry(profile, &database.id, schema_index);
        for relation_index in 0..100 {
            entries.push(relation_entry(
                profile,
                &schema.id,
                schema_index,
                relation_index,
            ));
        }
        schemas.push(schema.clone());
        entries.push(schema);
    }

    // Parents may appear anywhere in a replacement batch; identity and adjacency
    // validation must not require a pre-sorted database response.
    entries[1..].rotate_right(1);

    let mut tree = CatalogTree::new(profile);
    tree.insert_subtree(entries).unwrap();
    for schema in &schemas {
        tree.set_group_state(
            &schema.id,
            ObjectGroup::Tables,
            CatalogGroupState {
                count: CatalogCount::Exact(100),
                completeness: CatalogCompleteness::Complete,
            },
        )
        .unwrap();
    }

    let mut explorer = ExplorerTreeState::default();
    explorer.add_profile(profile);
    explorer.profiles.get_mut(&profile).unwrap().catalog = tree;
    explorer.expanded.extend([
        ExplorerNodeId::Profile(profile),
        ExplorerNodeId::Catalog(database.id.clone()),
        ExplorerNodeId::Catalog(schemas[0].id.clone()),
        ExplorerNodeId::Group {
            parent: schemas[0].id.clone(),
            group: ObjectGroup::Tables,
        },
    ]);

    let (rows, visited_catalog_entries) = explorer.visible_with_visit_count();
    assert_eq!(visited_catalog_entries, 201);
    assert_eq!(rows.len(), 203);

    for schema in &schemas {
        explorer
            .expanded
            .insert(ExplorerNodeId::Catalog(schema.id.clone()));
        explorer.expanded.insert(ExplorerNodeId::Group {
            parent: schema.id.clone(),
            group: ObjectGroup::Tables,
        });
    }

    let (rows, visited_catalog_entries) = explorer.visible_with_visit_count();
    assert_eq!(visited_catalog_entries, 10_101);
    assert_eq!(rows.len(), 10_202);
}

/// Build one fully expanded schema/Tables group holding `table_count` objects.
fn expanded_tables_explorer(table_count: usize) -> (ExplorerTreeState, Vec<CatalogId>) {
    let profile = Uuid::from_u128(1);
    let database = database_entry(profile);
    let schema = schema_entry(profile, &database.id, 0);
    let mut entries = vec![database.clone(), schema.clone()];
    let mut table_ids = Vec::with_capacity(table_count);
    for index in 0..table_count {
        let table = relation_entry(profile, &schema.id, 0, index);
        table_ids.push(table.id.clone());
        entries.push(table);
    }

    let mut tree = CatalogTree::new(profile);
    tree.insert_subtree(entries).unwrap();
    tree.set_group_state(
        &schema.id,
        ObjectGroup::Tables,
        CatalogGroupState {
            count: CatalogCount::Exact(table_count as u64),
            completeness: CatalogCompleteness::Complete,
        },
    )
    .unwrap();

    let mut explorer = ExplorerTreeState::default();
    explorer.add_profile(profile);
    explorer.profiles.get_mut(&profile).unwrap().catalog = tree;
    explorer.expanded.extend([
        ExplorerNodeId::Profile(profile),
        ExplorerNodeId::Catalog(database.id.clone()),
        ExplorerNodeId::Catalog(schema.id.clone()),
        ExplorerNodeId::Group {
            parent: schema.id.clone(),
            group: ObjectGroup::Tables,
        },
    ]);
    explorer.viewport_height = 30;
    explorer.selected = Some(ExplorerNodeId::Catalog(table_ids[0].clone()));
    (explorer, table_ids)
}

#[test]
fn expanded_tables_group_keeps_navigation_bounded_for_nine_hundred_fifty_six_tables() {
    let (mut explorer, table_ids) = expanded_tables_explorer(956);
    let (rows, visits) = explorer.visible_with_visit_count();
    assert_eq!(visits, 958);
    assert_eq!(rows.len(), 960);
    assert_eq!(
        rows.last().unwrap().id,
        ExplorerNodeId::Catalog(table_ids[955].clone())
    );

    for _ in 0..2_000 {
        explorer.move_selection(1, 30);
        assert!(explorer.scroll < explorer.visible().len());
    }
    assert_eq!(
        explorer.selected,
        Some(ExplorerNodeId::Catalog(table_ids[955].clone()))
    );
}

/// Manual measurement only; run with `--release --ignored --nocapture`.
#[test]
#[ignore = "performance measurement, not a CI gate"]
fn navigation_and_viewport_timing_for_large_expanded_lists() {
    for table_count in [956usize, 10_000] {
        let (mut explorer, _) = expanded_tables_explorer(table_count);
        for _ in 0..100 {
            explorer.move_selection(1, 30);
        }

        let mut move_samples = Vec::with_capacity(2_000);
        for _ in 0..1_000 {
            let start = std::time::Instant::now();
            explorer.move_selection(1, 30);
            move_samples.push(start.elapsed());
            let start = std::time::Instant::now();
            explorer.move_selection(-1, 30);
            move_samples.push(start.elapsed());
        }

        let mut viewport_samples = Vec::with_capacity(1_000);
        for _ in 0..1_000 {
            let start = std::time::Instant::now();
            let _ = explorer.viewport(30);
            viewport_samples.push(start.elapsed());
        }

        println!(
            "tables={table_count} move p50={:?} p95={:?} viewport p50={:?} p95={:?}",
            percentile(&move_samples, 50),
            percentile(&move_samples, 95),
            percentile(&viewport_samples, 50),
            percentile(&viewport_samples, 95),
        );
    }

    // Cost the old scrollbar paid on every draw by formatting all rows for display.
    // This conversion code is unchanged, so one measurement documents both revisions.
    let (explorer, _) = expanded_tables_explorer(956);
    let state = lazydb::model::workspace::ExplorerState {
        normalized: explorer,
        ..lazydb::model::workspace::ExplorerState::default()
    };
    let mut display_samples = Vec::with_capacity(100);
    for _ in 0..100 {
        let start = std::time::Instant::now();
        let _ = state.visible();
        display_samples.push(start.elapsed());
    }
    println!(
        "tables=956 full display conversion removed from each draw p50={:?} p95={:?}",
        percentile(&display_samples, 50),
        percentile(&display_samples, 95),
    );
}

fn percentile(samples: &[std::time::Duration], percentile: usize) -> std::time::Duration {
    let mut sorted = samples.to_vec();
    sorted.sort();
    let index = (sorted.len() * percentile / 100).min(sorted.len() - 1);
    sorted[index]
}

fn database_entry(profile: Uuid) -> CatalogEntry {
    CatalogEntry::database(
        CatalogId::new(profile, CatalogKind::Database, ["app"]),
        QualifiedName {
            database: Some("app".to_owned()),
            schema: None,
            object: "app".to_owned(),
        },
        "database",
        OptionalMetadata::Supported(None),
        true,
    )
    .unwrap()
}

fn schema_entry(profile: Uuid, database: &CatalogId, index: usize) -> CatalogEntry {
    let name = format!("schema_{index:03}");
    CatalogEntry::schema(
        CatalogId::new(profile, CatalogKind::Schema, [name.as_str()]),
        database.clone(),
        QualifiedName {
            database: Some("app".to_owned()),
            schema: Some(name.clone()),
            object: name,
        },
        "schema",
        OptionalMetadata::Supported(None),
        true,
    )
    .unwrap()
}

fn relation_entry(
    profile: Uuid,
    schema: &CatalogId,
    schema_index: usize,
    relation_index: usize,
) -> CatalogEntry {
    let name = format!("table_{schema_index:03}_{relation_index:03}");
    CatalogEntry::relation(
        CatalogId::new(profile, CatalogKind::Table, [name.as_str()]),
        schema.clone(),
        QualifiedName {
            database: Some("app".to_owned()),
            schema: Some(format!("schema_{schema_index:03}")),
            object: name,
        },
        "table",
        OptionalMetadata::Supported(None),
        false,
    )
    .unwrap()
}
