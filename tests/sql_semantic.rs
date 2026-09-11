use lazydb::db::catalog::{
    CatalogEntry, CatalogId, CatalogKind, CatalogMetadata, ColumnMetadata, OptionalMetadata,
    QualifiedName,
};
use lazydb::sql::{
    CatalogCoverage, CatalogNamespace, CatalogSnapshot, RelationResolution, SemanticContext,
    SqlDialect, analyze_semantics,
};
use uuid::Uuid;

fn relation(name: &str, database: &str, schema: &str) -> CatalogEntry {
    let profile = Uuid::new_v4();
    let schema_id = CatalogId::new(profile, CatalogKind::Schema, [database, schema]);
    CatalogEntry::relation(
        CatalogId::new(profile, CatalogKind::Table, [database, schema, name]),
        schema_id,
        QualifiedName {
            database: Some(database.to_owned()),
            schema: Some(schema.to_owned()),
            object: name.to_owned(),
        },
        "TABLE",
        Default::default(),
        true,
    )
    .expect("valid relation")
}

fn column(relation: &CatalogEntry, name: &str) -> CatalogEntry {
    let id = CatalogId::new(
        relation.id.profile_id(),
        CatalogKind::Column,
        [
            relation.qualified_name.database.as_deref().unwrap(),
            relation.qualified_name.schema.as_deref().unwrap(),
            &relation.qualified_name.object,
            name,
        ],
    );
    CatalogEntry::relation_child(
        id,
        relation.id.clone(),
        QualifiedName {
            database: relation.qualified_name.database.clone(),
            schema: relation.qualified_name.schema.clone(),
            object: name.to_owned(),
        },
        "TEXT",
        OptionalMetadata::Supported(None),
        CatalogMetadata::Column(ColumnMetadata::new(1, "TEXT", true)),
    )
    .expect("valid column")
}

#[test]
fn empty_namespace_is_unknown_until_catalog_is_complete() {
    let namespace = CatalogNamespace::new(Some("moss_biz"), Some("test_schema"));
    let snapshot = CatalogSnapshot::new([], [(namespace.clone(), CatalogCoverage::NotLoaded)]);

    assert_eq!(snapshot.coverage(&namespace), CatalogCoverage::NotLoaded);
    assert!(!snapshot.coverage(&namespace).can_prove_missing());
}

#[test]
fn namespace_coverage_uses_the_most_conservative_group_status() {
    let namespace = CatalogNamespace::new(Some("moss_biz"), Some("test_schema"));
    let snapshot = CatalogSnapshot::new(
        [],
        [
            (namespace.clone(), CatalogCoverage::Complete),
            (namespace.clone(), CatalogCoverage::Loading),
        ],
    );

    assert_eq!(snapshot.coverage(&namespace), CatalogCoverage::Loading);
}

#[test]
fn complete_empty_namespace_can_prove_missing() {
    let namespace = CatalogNamespace::new(Some("moss_biz"), Some("test_schema"));
    let snapshot = CatalogSnapshot::new([], [(namespace.clone(), CatalogCoverage::Complete)]);

    assert!(snapshot.coverage(&namespace).can_prove_missing());
    assert_eq!(snapshot.relations().count(), 0);
}

#[test]
fn snapshot_keeps_relations_and_context_separate_from_coverage() {
    let namespace = CatalogNamespace::new(Some("moss_biz"), Some("test_schema"));
    let snapshot = CatalogSnapshot::new(
        [relation("sys_user", "moss_biz", "test_schema")],
        [(namespace.clone(), CatalogCoverage::Complete)],
    );
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));

    assert_eq!(snapshot.relations().count(), 1);
    assert_eq!(context.default_namespace(), namespace);
}

#[test]
fn unqualified_relation_is_limited_to_the_active_namespace() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let namespace = context.default_namespace();
    let snapshot = CatalogSnapshot::new(
        [relation("sys_user", "moss_biz", "other_schema")],
        [(namespace, CatalogCoverage::Complete)],
    );

    assert_eq!(
        snapshot.resolve_relation(&["sys_user"], &context),
        RelationResolution::Missing
    );
}

#[test]
fn qualified_relation_uses_the_named_schema_in_the_active_database() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let namespace = CatalogNamespace::new(Some("moss_biz"), Some("other_schema"));
    let entry = relation("sys_user", "moss_biz", "other_schema");
    let expected = entry.id.clone();
    let snapshot = CatalogSnapshot::new([entry], [(namespace, CatalogCoverage::Complete)]);

    assert_eq!(
        snapshot.resolve_relation(&["other_schema", "sys_user"], &context),
        RelationResolution::Resolved(expected)
    );
}

#[test]
fn unloaded_namespace_does_not_report_a_missing_relation() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let snapshot = CatalogSnapshot::default();

    assert_eq!(
        snapshot.resolve_relation(&["sys_user"], &context),
        RelationResolution::Unknown
    );
}

#[test]
fn semantic_analysis_reports_missing_table_and_column_in_their_source_ranges() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let relation = relation("sys_user", "moss_biz", "test_schema");
    let relation_id = relation.id.clone();
    let snapshot = CatalogSnapshot::new(
        [relation.clone(), column(&relation, "id")],
        [(context.default_namespace(), CatalogCoverage::Complete)],
    )
    .with_column_coverage([(relation_id, CatalogCoverage::Complete)]);
    let analysis = analyze_semantics(
        "select u.id, u.missing from sys_user as u",
        &context,
        &snapshot,
    );

    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, "sql-unknown-column");
    assert_eq!(
        analysis.diagnostics[0]
            .range
            .get("select u.id, u.missing from sys_user as u"),
        Some("missing")
    );
}

#[test]
fn semantic_analysis_reports_missing_relation_without_fabricating_column_errors() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let snapshot = CatalogSnapshot::new(
        [],
        [(context.default_namespace(), CatalogCoverage::Complete)],
    );
    let analysis = analyze_semantics("select u.id from missing_table as u", &context, &snapshot);

    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, "sql-unknown-relation");
}

#[test]
fn cte_output_columns_are_checked_in_the_outer_scope() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let relation = relation("sys_user", "moss_biz", "test_schema");
    let relation_id = relation.id.clone();
    let snapshot = CatalogSnapshot::new(
        [relation.clone(), column(&relation, "id")],
        [(context.default_namespace(), CatalogCoverage::Complete)],
    )
    .with_column_coverage([(relation_id, CatalogCoverage::Complete)]);
    let analysis = analyze_semantics(
        "with users(id) as (select u.id from sys_user u) select users.missing from users",
        &context,
        &snapshot,
    );

    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, "sql-unknown-column");
    assert!(!analysis.incomplete);
}

#[test]
fn derived_tables_are_conservative_until_their_output_shape_is_known() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let snapshot = CatalogSnapshot::new(
        [],
        [(context.default_namespace(), CatalogCoverage::Complete)],
    );
    let analysis = analyze_semantics(
        "select derived.maybe_column from (select 1) as derived",
        &context,
        &snapshot,
    );

    assert!(analysis.diagnostics.is_empty());
    assert!(analysis.incomplete);
}

#[test]
fn dml_target_columns_use_the_target_relation_metadata() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let relation = relation("sys_user", "moss_biz", "test_schema");
    let relation_id = relation.id.clone();
    let snapshot = CatalogSnapshot::new(
        [relation.clone(), column(&relation, "id")],
        [(context.default_namespace(), CatalogCoverage::Complete)],
    )
    .with_column_coverage([(relation_id, CatalogCoverage::Complete)]);

    let insert = analyze_semantics(
        "insert into sys_user (missing) values (1)",
        &context,
        &snapshot,
    );
    assert_eq!(insert.diagnostics.len(), 1);
    assert_eq!(insert.diagnostics[0].code, "sql-unknown-column");

    let update = analyze_semantics(
        "update sys_user set missing = 1 where missing = 2",
        &context,
        &snapshot,
    );
    assert_eq!(update.diagnostics.len(), 2);
    assert!(
        update
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "sql-unknown-column")
    );

    let delete = analyze_semantics(
        "delete from sys_user where missing = 2",
        &context,
        &snapshot,
    );
    assert_eq!(delete.diagnostics.len(), 1);
    assert_eq!(delete.diagnostics[0].code, "sql-unknown-column");
}

#[test]
fn order_by_output_alias_is_not_reported_as_a_missing_column() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("test_schema"));
    let relation = relation("sys_user", "moss_biz", "test_schema");
    let relation_id = relation.id.clone();
    let snapshot = CatalogSnapshot::new(
        [relation.clone(), column(&relation, "id")],
        [(context.default_namespace(), CatalogCoverage::Complete)],
    )
    .with_column_coverage([(relation_id, CatalogCoverage::Complete)]);

    let analysis = analyze_semantics(
        "select id as user_id from sys_user order by user_id",
        &context,
        &snapshot,
    );

    assert!(analysis.diagnostics.is_empty(), "{analysis:?}");
}

#[test]
fn sqlite_main_qualified_relation_uses_the_main_schema() {
    let context = SemanticContext::new(SqlDialect::Sqlite, Some("database.db"), Some("main"));
    let entry = relation("users", "database.db", "main");
    let expected = entry.id.clone();
    let namespace = CatalogNamespace::new(Some("database.db"), Some("main"));
    let snapshot = CatalogSnapshot::new([entry], [(namespace, CatalogCoverage::Complete)]);

    assert_eq!(
        snapshot.resolve_relation(&["main", "users"], &context),
        RelationResolution::Resolved(expected)
    );
}

#[test]
fn qualified_missing_schema_is_reported_at_the_schema_identifier() {
    let context = SemanticContext::new(SqlDialect::Postgres, Some("moss_biz"), Some("public"));
    let profile = Uuid::new_v4();
    let database = CatalogEntry::database(
        CatalogId::new(profile, CatalogKind::Database, ["moss_biz"]),
        QualifiedName {
            database: None,
            schema: None,
            object: "moss_biz".into(),
        },
        "DATABASE",
        Default::default(),
        true,
    )
    .unwrap();
    let schema = CatalogEntry::schema(
        CatalogId::new(profile, CatalogKind::Schema, ["moss_biz", "public"]),
        CatalogId::new(profile, CatalogKind::Database, ["moss_biz"]),
        QualifiedName {
            database: Some("moss_biz".into()),
            schema: None,
            object: "public".into(),
        },
        "SCHEMA",
        Default::default(),
        true,
    )
    .unwrap();
    let snapshot = CatalogSnapshot::new(
        [database, schema],
        [(
            CatalogNamespace::new(Some("moss_biz"), Some("missing")),
            CatalogCoverage::Complete,
        )],
    );
    let analysis = analyze_semantics("select * from missing.users", &context, &snapshot);

    assert_eq!(analysis.diagnostics.len(), 1);
    assert_eq!(analysis.diagnostics[0].code, "sql-unknown-schema");
    assert_eq!(
        analysis.diagnostics[0]
            .range
            .get("select * from missing.users"),
        Some("missing")
    );
}
