use lazydb::{
    action::Action,
    app::App,
    db::ServerInfo,
    db::catalog::{
        CatalogEntry, CatalogId, CatalogKind, CatalogMetadata, ColumnMetadata, OptionalMetadata,
        QualifiedName,
    },
    profile::{CatalogScope, CatalogSelection, DatabaseKind, DatabaseScope, import_connection_url},
    sql::{
        CompletionContext, CompletionIndex, CompletionKind, SqlDialect, TextRange, complete,
        completion_dependencies, quote_identifier, should_offer_completion,
    },
};
use uuid::Uuid;

#[test]
fn builtin_default_current_timestamp_without_catalog() {
    let sql = "CREATE TABLE test1(\n id BIGINT NOT NULL,\n \"name\" text,\n create_time TIMESTAMP NOT NULL DEFAULT CURRENT_TIM\n)";
    let cursor = sql.find("CURRENT_TIM").unwrap() + "CURRENT_TIM".len();
    let candidates = complete(
        sql,
        cursor,
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.label == "CURRENT_TIMESTAMP")
        .expect("CURRENT_TIMESTAMP completion");
    assert_eq!(candidate.insert_text, "CURRENT_TIMESTAMP");
    assert_eq!(candidate.kind, CompletionKind::Keyword);
    assert_eq!(
        candidate.replace,
        TextRange::new(cursor - "CURRENT_TIM".len(), cursor)
    );
}

#[test]
fn builtin_current_timestamp_is_available_in_select_expression() {
    let sql = "SELECT CURRENT_TIM";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "CURRENT_TIMESTAMP"),
        "{candidates:?}"
    );
}

#[test]
fn builtin_select_expression_match_is_case_insensitive() {
    let sql = "SELECT current_tim";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        candidates.iter().any(|candidate| {
            candidate.label == "CURRENT_TIMESTAMP" && candidate.insert_text == "CURRENT_TIMESTAMP"
        }),
        "{candidates:?}"
    );
}

#[test]
fn builtin_select_dialect_specific_functions_are_isolated() {
    let cases = [
        (SqlDialect::Postgres, "GETDATE", 0),
        (SqlDialect::MySql, "NOW", 1),
        (SqlDialect::SqlServer, "GETDATE", 1),
        (SqlDialect::Sqlite, "NOW", 0),
    ];
    for (dialect, prefix, expected_any) in cases {
        let sql = format!("SELECT {prefix}");
        let candidates = complete(
            &sql,
            sql.len(),
            dialect,
            &CompletionIndex::default(),
            CompletionContext::default(),
        );
        let present = candidates
            .iter()
            .any(|candidate| candidate.label == dialect_bound_name(prefix));
        assert_eq!(present, expected_any == 1, "{dialect:?}: {candidates:?}");
    }
}

fn dialect_bound_name(prefix: &str) -> &'static str {
    match prefix {
        "NOW" => "NOW",
        "GETDATE" => "GETDATE",
        _ => unreachable!(),
    }
}

#[test]
fn builtin_select_empty_prefix_only_offers_special_expressions() {
    let sql = "SELECT ";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "CURRENT_TIMESTAMP"),
        "{candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.label != "COALESCE"),
        "{candidates:?}"
    );
}

#[test]
fn builtin_select_not_offered_in_relation_or_qualifier_positions() {
    let from = complete(
        "SELECT * FROM cur",
        "SELECT * FROM cur".len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        from.iter()
            .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"),
        "{from:?}"
    );
    let qualified = complete(
        "SELECT t.cur FROM test t",
        "SELECT t.cur".len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        qualified
            .iter()
            .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"),
        "{qualified:?}"
    );
}

#[test]
fn builtin_sqlite_function_is_not_excluded_by_catalog_dialect_filter() {
    let sql = "SELECT LENGTH";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Sqlite,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        candidates.iter().any(|candidate| {
            candidate.label == "LENGTH" && candidate.kind == CompletionKind::Function
        }),
        "{candidates:?}"
    );
}

fn has_builtin(
    candidates: &[lazydb::sql::CompletionCandidate],
    label: &str,
    kind: lazydb::sql::CompletionKind,
) -> bool {
    candidates
        .iter()
        .any(|candidate| candidate.label == label && candidate.kind == kind)
}

#[test]
fn builtin_default_expression_scope_handles_parentheses_and_boundaries() {
    let dialect = SqlDialect::Postgres;
    let index = CompletionIndex::default();
    let cases: &[(&str, &str)] = &[
        (
            "CREATE TABLE t (ts TIMESTAMP DEFAULT CURRENT_TIM",
            "CURRENT_TIM",
        ),
        (
            "CREATE TABLE t (ts TIMESTAMP DEFAULT (CURRENT_TIM)",
            "CURRENT_TIM",
        ),
        (
            "CREATE TABLE t (n DECIMAL(10, 2), ts TIMESTAMP DEFAULT CURRENT_TIM)",
            "CURRENT_TIM",
        ),
        (
            "CREATE TABLE t (ts TIMESTAMP DEFAULT COALESCE(NULL, CURRENT_TIM))",
            "CURRENT_TIM",
        ),
    ];
    for (sql, needle) in cases {
        let cursor = sql.find(needle).unwrap() + needle.len();
        let candidates = complete(sql, cursor, dialect, &index, CompletionContext::default());
        assert!(
            has_builtin(&candidates, "CURRENT_TIMESTAMP", CompletionKind::Keyword),
            "{sql}: {candidates:?}"
        );
    }
}

#[test]
fn builtin_default_value_restores_type_and_constraint_context() {
    let dialect = SqlDialect::Postgres;
    let index = CompletionIndex::default();
    let type_case = "CREATE TABLE t (ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP, n IN";
    let candidates = complete(
        type_case,
        type_case.len(),
        dialect,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.kind == CompletionKind::DataType),
        "expected type candidates: {candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"),
        "{candidates:?}"
    );

    let constraint_case = "CREATE TABLE t (ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT N";
    let candidates = complete(
        constraint_case,
        constraint_case.len(),
        dialect,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "NOT NULL"),
        "expected NOT NULL keyword: {candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"),
        "{candidates:?}"
    );
}

#[test]
fn builtin_default_null_value_does_not_break_column_boundary() {
    let sql = "CREATE TABLE t (ts TIMESTAMP DEFAULT NULL, n IN";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.kind == CompletionKind::DataType),
        "expected type candidates: {candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"),
        "{candidates:?}"
    );
}

#[test]
fn builtin_default_ignores_quoted_identifiers_and_string_literals() {
    let sql = "CREATE TABLE t (\"default\" TIMESTAMP NOT N";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "NOT NULL"),
        "expected NOT NULL keyword: {candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"),
        "{candidates:?}"
    );

    let literal = "CREATE TABLE t (a TEXT DEFAULT 'DEFAULT CURRENT_TIM";
    let candidates = complete(
        literal,
        literal.len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"),
        "string literal must not offer builtins: {candidates:?}"
    );
}

#[test]
fn builtin_default_value_dialect_layout_match() {
    let index = CompletionIndex::default();
    let pg = complete(
        "CREATE TABLE t (ts TIMESTAMP DEFAULT NOW",
        "CREATE TABLE t (ts TIMESTAMP DEFAULT NOW".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let mysql = complete(
        "CREATE TABLE t (ts TIMESTAMP DEFAULT NOW",
        "CREATE TABLE t (ts TIMESTAMP DEFAULT NOW".len(),
        SqlDialect::MySql,
        &index,
        CompletionContext::default(),
    );
    assert!(
        pg.iter().any(|candidate| candidate.label == "NOW"),
        "{pg:?}"
    );
    assert!(
        mysql.iter().all(|candidate| candidate.label != "NOW"),
        "{mysql:?}"
    );
}

#[test]
fn builtin_alter_table_default_forms_offer_builtins() {
    let index = CompletionIndex::default();
    let cases = [
        (
            "ALTER TABLE t ADD COLUMN c TIMESTAMP DEFAULT CURRENT_TIM",
            SqlDialect::Postgres,
            "CURRENT_TIMESTAMP",
        ),
        (
            "ALTER TABLE t ALTER COLUMN c SET DEFAULT CURRENT_TIM",
            SqlDialect::Postgres,
            "CURRENT_TIMESTAMP",
        ),
        (
            "ALTER TABLE t ADD COLUMN c TIMESTAMP DEFAULT CURRENT_TIM",
            SqlDialect::MySql,
            "CURRENT_TIMESTAMP",
        ),
        (
            "ALTER TABLE t ALTER COLUMN c SET DEFAULT CURRENT_TIM",
            SqlDialect::MySql,
            "CURRENT_TIMESTAMP",
        ),
        (
            "ALTER TABLE t ADD c INT DEFAULT GETD",
            SqlDialect::SqlServer,
            "GETDATE",
        ),
        (
            "ALTER TABLE t ADD COLUMN c TIMESTAMP DEFAULT CURRENT_TIM",
            SqlDialect::Sqlite,
            "CURRENT_TIMESTAMP",
        ),
    ];
    for (sql, dialect, expected) in cases {
        let candidates = complete(
            sql,
            sql.len(),
            dialect,
            &index,
            CompletionContext::default(),
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.label == expected),
            "{dialect:?} {sql}: {candidates:?}"
        );
    }
}

#[test]
fn builtin_alter_non_default_actions_offer_no_builtins() {
    let index = CompletionIndex::default();
    let cases = [
        (
            "ALTER TABLE t ALTER COLUMN c DROP DEFAULT",
            SqlDialect::Postgres,
        ),
        (
            "ALTER TABLE t RENAME COLUMN a TO default",
            SqlDialect::Postgres,
        ),
        ("ALTER TABLE t DROP COLUMN c", SqlDialect::Postgres),
        (
            "ALTER TABLE t ADD CONSTRAINT ck DEFAULT 0 FOR c",
            SqlDialect::SqlServer,
        ),
    ];
    for (sql, dialect) in cases {
        let candidates = complete(
            sql,
            sql.len(),
            dialect,
            &index,
            CompletionContext::default(),
        );
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"
                    && candidate.label != "GETDATE"),
            "{dialect:?} {sql}: {candidates:?}"
        );
    }
}

#[test]
fn builtin_filter_subqueries_and_ordering_positions() {
    let index = CompletionIndex::default();
    let subquery = "SELECT * FROM t WHERE x IN (SELECT CUR";
    let candidates = complete(
        subquery,
        subquery.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "CURRENT_TIMESTAMP"),
        "subquery expression should offer builtins: {candidates:?}"
    );

    let expression = "SELECT a FROM t ORDER BY CUR";
    let candidates = complete(
        expression,
        expression.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "CURRENT_TIMESTAMP"),
        "ordering expression should offer builtins: {candidates:?}"
    );

    let direction = "SELECT a FROM t ORDER BY a ASC";
    let candidates = complete(
        direction,
        direction.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.label != "CURRENT_TIMESTAMP"),
        "ordering direction state must not offer builtins: {candidates:?}"
    );
}

#[test]
fn builtin_filter_comment_position_is_not_offered_by_trigger() {
    let sql = "SELECT /* CUR";
    assert!(!should_offer_completion(sql, sql.len()));
}

#[test]
fn builtin_identity_keeps_user_functions_and_avoid_internal_duplicates() {
    let mut entries = fixture();
    let connection = entries[0].id.profile_id();
    let schema = entries[1].id.clone();
    let function = CatalogEntry::object(
        CatalogId::new(connection, CatalogKind::Function, ["app", "public", "NOW"]),
        schema.clone(),
        qualified("app", Some("public"), "NOW"),
        "user function",
        OptionalMetadata::Supported(None),
        false,
    )
    .unwrap();
    entries.push(function);
    let index = CompletionIndex::new(&entries);
    let sql = "SELECT NOW";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let now = candidates
        .iter()
        .filter(|candidate| candidate.label == "NOW")
        .count();
    assert!(
        now >= 2,
        "static builtin and catalog function must both survive: {candidates:?}"
    );
}

#[test]
fn builtin_internal_list_has_no_duplicate_labels() {
    let sql = "SELECT C";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    let mut labels: Vec<&str> = candidates
        .iter()
        .filter(|candidate| candidate.detail.is_some())
        .map(|candidate| candidate.label.as_str())
        .collect();
    labels.sort_unstable();
    let before = labels.len();
    labels.dedup();
    assert_eq!(
        before,
        labels.len(),
        "duplicate builtin labels: {candidates:?}"
    );
}

#[test]
fn builtin_default_completion_does_not_request_relation_children() {
    let sql = "CREATE TABLE t (ts TIMESTAMP DEFAULT CURRENT_TIM";
    let cursor = sql.find("CURRENT_TIM").unwrap();
    let dependencies = completion_dependencies(
        sql,
        cursor,
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    assert!(
        dependencies.relation_children.is_empty(),
        "default value completion must not add catalog requests: {dependencies:?}"
    );
}

#[test]
fn builtin_app_completion_popup_and_accept_work_end_to_end() {
    let profile = import_connection_url("postgres://localhost/app", Some("app"))
        .unwrap()
        .profile;
    let profile_id = profile.id;
    let mut app = App::new(vec![profile]);
    app.update(Action::ConnectionSucceeded {
        profile_id,
        generation: 1,
        server: ServerInfo {
            kind: DatabaseKind::Postgres,
            version: "16.4".into(),
            database: "app".into(),
            current_user: None,
        },
        mutation_capabilities: Default::default(),
    });
    let sql = "CREATE TABLE test1(\n id BIGINT NOT NULL,\n \"name\" text,\n create_time TIMESTAMP NOT NULL DEFAULT CURRENT_TIM";
    app.update(Action::ReplaceEditor(sql[..sql.len() - 2].into()));
    for key in ['G', '$', 'a', 'I', 'M'] {
        app.update(Action::EditorKey(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(key),
            crossterm::event::KeyModifiers::NONE,
        )));
    }
    app.update(Action::CompletionExplicit);
    let popup = app
        .active_console_opt()
        .and_then(|tab| tab.completion.as_ref())
        .expect("completion popup");
    assert!(
        popup
            .candidates
            .iter()
            .any(|candidate| candidate.label == "CURRENT_TIMESTAMP"),
        "{:?}",
        popup
            .candidates
            .iter()
            .map(|c| &c.label)
            .collect::<Vec<_>>()
    );
    let mut steps = 0;
    while app
        .active_console_opt()
        .and_then(|tab| tab.completion.as_ref())
        .and_then(|popup| popup.candidates.get(popup.selected))
        .map(|candidate| candidate.label.as_str())
        != Some("CURRENT_TIMESTAMP")
    {
        app.update(Action::CompletionNext);
        steps += 1;
        assert!(steps < 20, "could not navigate to CURRENT_TIMESTAMP");
    }
    app.update(Action::CompletionAccept);
    let text = app.active_editor_text().unwrap();
    assert!(
        text.trim_end().ends_with("DEFAULT CURRENT_TIMESTAMP"),
        "accepted completion must replace prefix without quotes: {text:?}"
    );
    assert!(
        !text.contains("CURRENT_TIMESTAMP\""),
        "builtin expression must not be quoted: {text:?}"
    );
}

fn fixture() -> Vec<CatalogEntry> {
    let connection = Uuid::new_v4();
    let database = CatalogId::new(connection, CatalogKind::Database, ["app"]);
    let schema = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    let table = CatalogId::new(connection, CatalogKind::Table, ["app", "public", "users"]);
    vec![
        CatalogEntry::database(
            database.clone(),
            qualified("app", None, "app"),
            "database",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::schema(
            schema.clone(),
            database,
            qualified("app", Some("public"), "public"),
            "schema",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::relation(
            table.clone(),
            schema,
            qualified("app", Some("public"), "users"),
            "table",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
        CatalogEntry::relation_child(
            CatalogId::new(
                connection,
                CatalogKind::Column,
                ["app", "public", "users", "odd name"],
            ),
            table,
            qualified("app", Some("public"), "odd name"),
            "column",
            OptionalMetadata::Unsupported,
            CatalogMetadata::Column(ColumnMetadata::new(1, "text\x1b[31m", true)),
        )
        .unwrap(),
    ]
}

fn compact_match_fixture() -> Vec<CatalogEntry> {
    let mut entries = fixture();
    let connection = entries[0].id.profile_id();
    let schema = entries[1].id.clone();
    for name in ["sys_user", "sysuser_archive"] {
        entries.push(
            CatalogEntry::relation(
                CatalogId::new(connection, CatalogKind::Table, ["app", "public", name]),
                schema.clone(),
                qualified("app", Some("public"), name),
                "table",
                OptionalMetadata::Supported(None),
                true,
            )
            .unwrap(),
        );
    }
    let users = entries[2].id.clone();
    entries.push(
        CatalogEntry::relation_child(
            CatalogId::new(
                connection,
                CatalogKind::Column,
                ["app", "public", "users", "user_id"],
            ),
            users,
            qualified("app", Some("public"), "user_id"),
            "column",
            OptionalMetadata::Unsupported,
            CatalogMetadata::Column(ColumnMetadata::new(2, "bigint", false)),
        )
        .unwrap(),
    );
    entries
}

#[test]
fn completion_keeps_all_matching_candidates() {
    let mut entries = fixture();
    let connection = entries[0].id.profile_id();
    let schema = entries[1].id.clone();
    for index in 0..25 {
        let name = format!("candidate_{index:02}");
        entries.push(
            CatalogEntry::relation(
                CatalogId::new(connection, CatalogKind::Table, ["app", "public", &name]),
                schema.clone(),
                qualified("app", Some("public"), &name),
                "table",
                OptionalMetadata::Supported(None),
                true,
            )
            .unwrap(),
        );
    }
    let index = CompletionIndex::new(&entries);
    let candidates = complete(
        "select * from candidate_",
        "select * from candidate_".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext {
            database: Some("app"),
            schema: Some("public"),
        },
    );
    let labels = candidates
        .iter()
        .filter(|candidate| candidate.kind == CompletionKind::Table)
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert_eq!(labels.len(), 25);
    assert!(labels.contains(&"candidate_10"));
    assert!(labels.contains(&"candidate_24"));
}

fn contextual_fixture() -> Vec<CatalogEntry> {
    let connection = Uuid::new_v4();
    let schema = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    let mut entries = Vec::new();
    for (table_name, columns) in [
        (
            "sys_user",
            &[
                "update_time",
                "update_user",
                "update_user_phone",
                "user_type",
                "username",
            ][..],
        ),
        ("user_agreement_accept", &["agreement_id"][..]),
        ("unit_mtmm_capacity", &["capacity_id"][..]),
    ] {
        let table = CatalogId::new(
            connection,
            CatalogKind::Table,
            ["app", "public", table_name],
        );
        entries.push(
            CatalogEntry::relation(
                table.clone(),
                schema.clone(),
                qualified("app", Some("public"), table_name),
                "table",
                OptionalMetadata::Supported(None),
                true,
            )
            .unwrap(),
        );
        entries.extend(columns.iter().enumerate().map(|(position, column)| {
            CatalogEntry::relation_child(
                CatalogId::new(
                    connection,
                    CatalogKind::Column,
                    ["app", "public", table_name, column],
                ),
                table.clone(),
                qualified("app", Some("public"), column),
                "column",
                OptionalMetadata::Unsupported,
                CatalogMetadata::Column(ColumnMetadata::new(position as u32 + 1, "text", true)),
            )
            .unwrap()
        }));
    }
    entries
}

fn multi_relation_fixture() -> Vec<CatalogEntry> {
    let connection = Uuid::new_v4();
    let schema = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    let mut entries = Vec::new();
    for (table_name, columns) in [
        ("users", &["id", "user_name"][..]),
        ("roles", &["id", "role_name"][..]),
        ("audit_log", &["audit_message"][..]),
    ] {
        let table = CatalogId::new(
            connection,
            CatalogKind::Table,
            ["app", "public", table_name],
        );
        entries.push(
            CatalogEntry::relation(
                table.clone(),
                schema.clone(),
                qualified("app", Some("public"), table_name),
                "table",
                OptionalMetadata::Supported(None),
                true,
            )
            .unwrap(),
        );
        entries.extend(columns.iter().enumerate().map(|(position, column)| {
            CatalogEntry::relation_child(
                CatalogId::new(
                    connection,
                    CatalogKind::Column,
                    ["app", "public", table_name, column],
                ),
                table.clone(),
                qualified("app", Some("public"), column),
                "column",
                OptionalMetadata::Unsupported,
                CatalogMetadata::Column(ColumnMetadata::new(position as u32 + 1, "text", true)),
            )
            .unwrap()
        }));
    }
    entries
}

#[test]
fn select_expression_excludes_relation_candidates() {
    let index = CompletionIndex::new(&contextual_fixture());
    let sql = "select u from sys_user";
    let candidates = complete(
        sql,
        "select u".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );

    assert!(candidates.iter().any(|candidate| {
        candidate.kind == CompletionKind::Column && candidate.label == "username"
    }));
    assert!(candidates.iter().all(|candidate| !matches!(
        candidate.kind,
        CompletionKind::Database
            | CompletionKind::Schema
            | CompletionKind::Table
            | CompletionKind::View
    )));
}

#[test]
fn where_expression_excludes_relation_candidates() {
    let index = CompletionIndex::new(&contextual_fixture());
    let sql = "select * from sys_user\nwhere ";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );

    assert!(candidates.iter().any(|candidate| {
        candidate.kind == CompletionKind::Column && candidate.label == "update_time"
    }));
    assert!(candidates.iter().all(|candidate| !matches!(
        candidate.kind,
        CompletionKind::Database
            | CompletionKind::Schema
            | CompletionKind::Table
            | CompletionKind::View
    )));
}

#[test]
fn missing_columns_do_not_fall_back_to_global_relations() {
    let mut entries = contextual_fixture();
    entries.retain(|entry| entry.kind != CatalogKind::Column);
    let index = CompletionIndex::new(&entries);
    let sql = "select * from sys_user where ";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );

    assert!(candidates.iter().all(|candidate| !matches!(
        candidate.kind,
        CompletionKind::Database
            | CompletionKind::Schema
            | CompletionKind::Table
            | CompletionKind::View
    )));
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.kind == CompletionKind::Keyword)
    );
}

#[test]
fn statement_and_expression_keywords_are_contextual() {
    let index = CompletionIndex::new(&contextual_fixture());
    let statement = complete(
        "u",
        1,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        statement
            .iter()
            .any(|candidate| candidate.label == "UPDATE")
    );

    let projection_sql = "select u from sys_user";
    let projection = complete(
        projection_sql,
        "select u".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        !projection
            .iter()
            .any(|candidate| candidate.label == "UPDATE")
    );
}

#[test]
fn order_by_clause_completion_without_catalog() {
    let sql = "select 1 where 1 = 1 orde";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &CompletionIndex::default(),
        CompletionContext::default(),
    );
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.label == "ORDER BY")
        .expect("a completed predicate should offer ORDER BY");

    assert_eq!(candidate.insert_text, "ORDER BY");
    assert_eq!(candidate.kind, CompletionKind::Keyword);
    assert_eq!(candidate.replace.start, sql.len() - "orde".len());
    assert_eq!(candidate.replace.end, sql.len());
}

#[test]
fn order_by_clause_completion_respects_expression_boundaries() {
    let index = CompletionIndex::default();
    for sql in [
        "select 1 where 1 = 1 orde",
        "select 1 from users orde",
        "select 1 from users group by 1 orde",
        "select 1 from users group by 1 having 1 = 1 orde",
        "select 1 orde",
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.label == "ORDER BY"),
            "expected ORDER BY for {sql}: {candidates:?}"
        );
    }

    for sql in [
        "select 1 where 1 = orde",
        "select 1 where 1 between 1 and orde",
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.label == "ORDER BY")
        );
    }
}

#[test]
fn order_by_clause_completion_handles_waiting_for_by() {
    let index = CompletionIndex::default();
    for sql in ["select 1 from users order ", "select 1 from users order b"] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert!(
            candidates.iter().any(|candidate| candidate.label == "BY"),
            "expected BY for {sql}: {candidates:?}"
        );
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.label == "ORDER BY")
        );
    }
}

#[test]
fn order_by_clause_completion_replaces_only_the_current_prefix() {
    for (sql, expected, label) in [
        ("select 1 from users ord", "ORDER BY", "ORDER BY"),
        ("select 1 from users order b", "BY", "BY"),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &CompletionIndex::default(),
            CompletionContext::default(),
        );
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.label == label)
            .expect("expected ORDER BY replacement candidate");
        let replaced = format!(
            "{}{}{}",
            &sql[..candidate.replace.start],
            expected,
            &sql[candidate.replace.end..]
        );
        assert_eq!(
            replaced,
            if label == "ORDER BY" {
                "select 1 from users ORDER BY"
            } else {
                "select 1 from users order BY"
            }
        );
    }
}

#[test]
fn order_by_completion_offers_columns_then_direction() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let columns = complete(
        "select * from users u join roles r on u.id = r.id order by ",
        "select * from users u join roles r on u.id = r.id order by ".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        columns
            .iter()
            .any(|candidate| candidate.label == "user_name")
    );
    assert!(!columns.iter().any(|candidate| candidate.label == "ASC"));

    let directions = complete(
        "select * from users u join roles r on u.id = r.id order by u.user_name ",
        "select * from users u join roles r on u.id = r.id order by u.user_name ".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(directions.iter().any(|candidate| candidate.label == "ASC"));
    assert!(directions.iter().any(|candidate| candidate.label == "DESC"));
}

#[test]
fn order_by_completion_filters_direction_by_expression_state() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    for sql in [
        "select * from users u join roles r on u.id = r.id order by u.user_name + ",
        "select * from users u join roles r on u.id = r.id order by coalesce(u.user_name, ",
        "select * from users u join roles r on u.id = r.id order by u.user_name, ",
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert!(!candidates.iter().any(|candidate| candidate.label == "ASC"));
        assert!(!candidates.iter().any(|candidate| candidate.label == "DESC"));
    }
}

#[test]
fn order_by_completion_filters_null_placement_by_dialect() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select * from users order by user_name ";
    for (dialect, supports_nulls) in [
        (SqlDialect::Postgres, true),
        (SqlDialect::Sqlite, true),
        (SqlDialect::Generic, true),
        (SqlDialect::MySql, false),
        (SqlDialect::SqlServer, false),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            dialect,
            &index,
            CompletionContext::default(),
        );
        assert_eq!(
            candidates
                .iter()
                .any(|candidate| candidate.label == "NULLS FIRST"),
            supports_nulls,
            "unexpected NULLS support for {dialect:?}: {candidates:?}"
        );
    }
}

#[test]
fn order_by_completion_advances_after_direction_and_null_placement() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let after_direction = complete(
        "select * from users order by user_name desc ",
        "select * from users order by user_name desc ".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        !after_direction
            .iter()
            .any(|candidate| candidate.label == "ASC")
    );
    assert!(
        !after_direction
            .iter()
            .any(|candidate| candidate.label == "DESC")
    );
    assert!(
        after_direction
            .iter()
            .any(|candidate| candidate.label == "NULLS FIRST")
    );

    let after_null_placement = complete(
        "select * from users order by user_name desc nulls last ",
        "select * from users order by user_name desc nulls last ".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        !after_null_placement
            .iter()
            .any(|candidate| candidate.label == "ASC" || candidate.label == "NULLS FIRST")
    );
}

#[test]
fn order_by_completion_respects_relation_qualifiers() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select * from users u join roles r on u.id = r.id order by r.";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "role_name")
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.label == "user_name")
    );
    assert!(!candidates.iter().any(|candidate| candidate.label == "ASC"));
}

#[test]
fn order_by_completion_dependencies_include_visible_relations() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select * from users u join roles r on u.id = r.id order by ";
    let dependencies = completion_dependencies(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert_eq!(dependencies.relation_children.len(), 2);
}

#[test]
fn ordering_completion_trigger_is_limited_to_ordering_positions() {
    for (sql, expected) in [
        ("select 1 from users order by user_name ", true),
        ("select 1 from users order by user_name, ", true),
        ("select 1 from users where user_name = ", false),
        ("select 'order by ' ", false),
        ("select 1 -- order by \n", false),
    ] {
        assert_eq!(
            should_offer_completion(sql, sql.len()),
            expected,
            "unexpected trigger result for {sql:?}"
        );
    }
}

#[test]
fn order_by_completion_keeps_nested_query_scopes_separate() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let outer = "select * from users u where u.id in (select r.id from roles r order by ";
    let candidates = complete(
        outer,
        outer.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "role_name")
    );

    let inner = "select * from users u where u.id in (select r.id from roles r order by r.";
    let candidates = complete(
        inner,
        inner.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "role_name")
    );
}

#[test]
fn insert_completion_offers_only_into_keyword() {
    let index = CompletionIndex::new(&contextual_fixture());

    for sql in ["insert ", "insert i"] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );

        assert_eq!(
            candidates
                .iter()
                .filter(|candidate| candidate.kind == CompletionKind::Keyword)
                .map(|candidate| candidate.label.as_str())
                .collect::<Vec<_>>(),
            vec!["INTO"],
            "unexpected keyword candidates for {sql}: {candidates:?}"
        );
    }
}

#[test]
fn insert_context_does_not_leak_into_statement_or_relation_completion() {
    let index = CompletionIndex::new(&fixture());

    let statement = complete(
        "i",
        1,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert_eq!(
        statement.first().map(|candidate| candidate.label.as_str()),
        Some("INSERT")
    );
    assert!(statement.iter().all(|candidate| candidate.label != "INTO"));

    let relation_sql = "insert into u";
    let relation = complete(
        relation_sql,
        relation_sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(relation.iter().any(|candidate| {
        candidate.kind == CompletionKind::Table && candidate.label == "users"
    }));
}

#[test]
fn automatic_completion_requires_an_identifier_prefix() {
    for sql in [
        "",
        " ",
        "\n",
        "select ",
        "select * from users;",
        "select * from users;\n",
    ] {
        assert!(!should_offer_completion(sql, sql.len()), "{sql:?}");
    }
    for sql in [
        "s",
        "select * from us",
        "select u.",
        "select u1.",
        "select * from public.",
    ] {
        assert!(should_offer_completion(sql, sql.len()), "{sql:?}");
    }
}

#[test]
fn automatic_completion_ignores_non_identifier_dots_and_literals() {
    for sql in ["1.", "select 'users'", "-- users", "select * from users; "] {
        assert!(!should_offer_completion(sql, sql.len()), "{sql:?}");
    }
}

#[test]
fn ddl_completion_trigger_matches_structural_context() {
    for sql in [
        "CREATE ",
        "ALTER ",
        "DROP ",
        "TRUNCATE TABLE ",
        "CREATE INDEX ix ON ",
        "ALTER TABLE users DROP COLUMN ",
    ] {
        assert!(!should_offer_completion(sql, sql.len()), "{sql}");
    }
    for sql in [
        "SELECT 'CREATE '",
        "-- CREATE ",
        "CREATE TABLE \"drop\" (id INTEGER)",
    ] {
        assert!(!should_offer_completion(sql, sql.len()), "{sql}");
    }
}

#[test]
fn ddl_completion_handles_incomplete_input_without_global_leakage() {
    let index = CompletionIndex::new(&fixture());
    for (sql, dialect) in [
        ("CREATE", SqlDialect::Postgres),
        ("CREATE TABLE", SqlDialect::Postgres),
        ("CREATE TABLE users (", SqlDialect::Postgres),
        ("CREATE TABLE users (id VARCHAR(", SqlDialect::Postgres),
        ("ALTER TABLE users DROP", SqlDialect::Postgres),
        ("REFERENCES users (", SqlDialect::Postgres),
        ("DROP TABLE \"unterminated", SqlDialect::Postgres),
        (
            "CREATE TABLE users (label TEXT DEFAULT 'unterminated",
            SqlDialect::Postgres,
        ),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            dialect,
            &index,
            CompletionContext::default(),
        );
        assert!(candidates.len() <= 10, "{sql}: {candidates:?}");
        assert!(
            !candidates.iter().any(|candidate| {
                matches!(
                    candidate.kind,
                    CompletionKind::Database | CompletionKind::Schema
                )
            }),
            "incomplete DDL leaked namespace candidates for {sql}: {candidates:?}"
        );
    }
}

#[test]
fn ddl_completion_only_uses_the_statement_at_cursor() {
    let index = CompletionIndex::new(&fixture());
    let first = "SELECT * FROM users; DROP TABLE us";
    let candidates = complete(
        first,
        first.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind == CompletionKind::Table)
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "users")
    );

    let second = "CREATE TABLE draft (id INTEGER); SELECT * FROM us";
    let candidates = complete(
        second,
        second.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(candidates.iter().all(|candidate| {
        candidate.kind == CompletionKind::Table || candidate.kind == CompletionKind::View
    }));

    let third = "CREATE VIEW draft AS SELECT * FROM users; ALTER TABLE us";
    let candidates = complete(
        third,
        third.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind == CompletionKind::Table)
    );
}

#[test]
fn statement_completion_offers_ddl_commands() {
    let index = CompletionIndex::default();
    for (sql, expected) in [
        ("cre", "CREATE"),
        ("alt", "ALTER"),
        ("dro", "DROP"),
        ("tru", "TRUNCATE"),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert_eq!(
            candidates.first().map(|candidate| candidate.label.as_str()),
            Some(expected)
        );
    }
}

#[test]
fn ddl_object_and_type_keywords_follow_dialect_matrix() {
    for (dialect, expected, rejected) in [
        (
            SqlDialect::Postgres,
            &[
                "TABLE",
                "VIEW",
                "MATERIALIZED VIEW",
                "SEQUENCE",
                "TYPE",
                "TRIGGER",
            ][..],
            &["AUTOINCREMENT"][..],
        ),
        (
            SqlDialect::MySql,
            &["TABLE", "VIEW", "TRIGGER"][..],
            &["MATERIALIZED VIEW", "SEQUENCE", "TYPE"][..],
        ),
        (
            SqlDialect::SqlServer,
            &["TABLE", "VIEW", "TRIGGER"][..],
            &["MATERIALIZED VIEW", "TYPE"][..],
        ),
        (
            SqlDialect::Sqlite,
            &["TABLE", "VIEW", "TRIGGER"][..],
            &["MATERIALIZED VIEW", "SEQUENCE", "TYPE"][..],
        ),
    ] {
        for keyword in expected {
            let sql = format!(
                "CREATE {}",
                &keyword[..keyword
                    .char_indices()
                    .nth(1)
                    .map_or(keyword.len(), |(index, _)| index)]
            );
            let candidates = complete(
                &sql,
                sql.len(),
                dialect,
                &CompletionIndex::default(),
                CompletionContext::default(),
            );
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.label == *keyword),
                "{dialect:?}: {keyword}: {candidates:?}"
            );
        }
        for keyword in rejected {
            let prefix = &keyword[..keyword
                .char_indices()
                .nth(1)
                .map_or(keyword.len(), |(index, _)| index)];
            let sql = format!("CREATE {prefix}");
            let candidates = complete(
                &sql,
                sql.len(),
                dialect,
                &CompletionIndex::default(),
                CompletionContext::default(),
            );
            assert!(
                !candidates
                    .iter()
                    .any(|candidate| candidate.label == *keyword),
                "{dialect:?}: {keyword}: {candidates:?}"
            );
        }
    }

    for (dialect, sql, expected, rejected) in [
        (
            SqlDialect::Postgres,
            "CREATE TABLE t (id js",
            "JSONB",
            "JSON",
        ),
        (SqlDialect::MySql, "CREATE TABLE t (id js", "JSON", "JSONB"),
        (
            SqlDialect::SqlServer,
            "CREATE TABLE t (id nv",
            "NVARCHAR",
            "JSONB",
        ),
        (
            SqlDialect::Sqlite,
            "CREATE TABLE t (id in",
            "INTEGER",
            "JSONB",
        ),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            dialect,
            &CompletionIndex::default(),
            CompletionContext::default(),
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.label == expected),
            "{dialect:?}: {candidates:?}"
        );
        assert!(
            !candidates
                .iter()
                .any(|candidate| candidate.label == rejected),
            "{dialect:?}: {candidates:?}"
        );
    }
}

#[test]
fn ddl_object_completion_filters_catalog_kind() {
    let index = CompletionIndex::new(&fixture());
    let candidates = complete(
        "DROP TABLE us",
        13,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.kind == CompletionKind::Table && candidate.label == "users"),
        "{candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind == CompletionKind::Table)
    );
}

#[test]
fn ddl_keywords_ignore_quoted_identifiers_and_literals() {
    let index = CompletionIndex::default();
    let quoted_sql = "CREATE TABLE t (\"drop\" INTEGER, na";
    let quoted = complete(
        quoted_sql,
        quoted_sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(!quoted.iter().any(|candidate| candidate.label == "DROP"));
    let literal = complete(
        "SELECT 'create' WHERE cre",
        26,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(!literal.iter().any(|candidate| candidate.label == "CREATE"));
}

#[test]
fn create_table_completion_offers_dialect_data_types() {
    let index = CompletionIndex::default();
    for (dialect, sql, expected, rejected) in [
        (
            SqlDialect::Postgres,
            "CREATE TABLE t (id j",
            "JSONB",
            "JSON",
        ),
        (SqlDialect::MySql, "CREATE TABLE t (id j", "JSON", "JSONB"),
        (
            SqlDialect::SqlServer,
            "CREATE TABLE t (id unique",
            "UNIQUEIDENTIFIER",
            "JSONB",
        ),
        (SqlDialect::Sqlite, "CREATE TABLE t (id var", "", "VARCHAR"),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            dialect,
            &index,
            CompletionContext::default(),
        );
        if expected.is_empty() {
            assert!(
                candidates
                    .iter()
                    .all(|candidate| candidate.label != rejected)
            );
        } else {
            assert!(
                candidates.iter().any(|candidate| {
                    candidate.kind == CompletionKind::DataType && candidate.label == expected
                }),
                "{dialect:?} {sql}: {candidates:?}"
            );
            assert!(
                candidates
                    .iter()
                    .all(|candidate| candidate.label != rejected)
            );
        }
    }
}

#[test]
fn create_table_completion_offers_column_constraints_after_type() {
    let index = CompletionIndex::default();
    let sql = "CREATE TABLE t (id INTEGER n";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates.iter().any(|candidate| {
            candidate.kind == CompletionKind::Keyword && candidate.label == "NOT NULL"
        }),
        "{candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind == CompletionKind::Keyword)
    );
}

#[test]
fn ddl_completion_covers_table_elements_alter_actions_and_children() {
    let mut entries = fixture();
    let users = entries[2].id.clone();
    for (kind, name) in [
        (CatalogKind::Column, "id"),
        (CatalogKind::Column, "email"),
        (CatalogKind::Index, "ix_users_email"),
        (CatalogKind::PrimaryKey, "users_pkey"),
        (CatalogKind::UniqueConstraint, "users_email_key"),
        (CatalogKind::ForeignKey, "users_role_fkey"),
        (CatalogKind::CheckConstraint, "users_email_check"),
    ] {
        let metadata = match kind {
            CatalogKind::Column => CatalogMetadata::Column(ColumnMetadata::new(2, "text", true)),
            CatalogKind::Index => CatalogMetadata::Index(lazydb::db::catalog::IndexMetadata {
                columns: vec!["email".into()],
                unique: false,
            }),
            CatalogKind::PrimaryKey => {
                CatalogMetadata::Constraint(lazydb::db::catalog::ConstraintMetadata::PrimaryKey {
                    columns: vec!["id".into()],
                })
            }
            CatalogKind::UniqueConstraint => {
                CatalogMetadata::Constraint(lazydb::db::catalog::ConstraintMetadata::Unique {
                    columns: vec!["email".into()],
                })
            }
            CatalogKind::ForeignKey => {
                CatalogMetadata::Constraint(lazydb::db::catalog::ConstraintMetadata::ForeignKey {
                    columns: vec!["id".into()],
                    referenced_relation: qualified("app", Some("public"), "roles"),
                    referenced_columns: vec!["id".into()],
                })
            }
            CatalogKind::CheckConstraint => {
                CatalogMetadata::Constraint(lazydb::db::catalog::ConstraintMetadata::Check {
                    expression: "true".into(),
                })
            }
            _ => unreachable!(),
        };
        entries.push(
            CatalogEntry::relation_child(
                CatalogId::new(users.profile_id(), kind, ["app", "public", "users", name]),
                users.clone(),
                qualified("app", Some("public"), name),
                "child",
                OptionalMetadata::Supported(None),
                metadata,
            )
            .unwrap(),
        );
    }
    let index = CompletionIndex::new(&entries);
    let candidates = complete(
        "ALTER TABLE users ",
        "ALTER TABLE users ".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(candidates.iter().any(|candidate| candidate.label == "ADD"));
    assert!(candidates.iter().any(|candidate| candidate.label == "DROP"));

    for (sql, kind, expected) in [
        (
            "ALTER TABLE users DROP COLUMN e",
            CompletionKind::Column,
            "email",
        ),
        (
            "ALTER TABLE users ALTER COLUMN i",
            CompletionKind::Column,
            "id",
        ),
        (
            "ALTER TABLE users DROP CONSTRAINT users_",
            CompletionKind::Constraint,
            "users_email_key",
        ),
        (
            "ALTER TABLE users DROP INDEX ix_",
            CompletionKind::Index,
            "ix_users_email",
        ),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.kind == kind && candidate.label == expected),
            "{sql}: {candidates:?}"
        );
        assert!(candidates.iter().all(|candidate| candidate.kind == kind));
    }
}

#[test]
fn ddl_completion_reports_shared_relation_child_dependencies() {
    let index = CompletionIndex::new(&fixture());
    let users = index
        .entries()
        .iter()
        .find(|entry| entry.kind == CatalogKind::Table)
        .unwrap()
        .id
        .clone();
    for sql in [
        "SELECT u. FROM users u",
        "ALTER TABLE users DROP COLUMN ",
        "CREATE INDEX ix_users ON users (",
        "REFERENCES users (",
    ] {
        let dependencies = completion_dependencies(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert_eq!(dependencies.relation_children, vec![users.clone()], "{sql}");
    }
    assert!(
        completion_dependencies(
            "DROP TABLE users",
            "DROP TABLE users".len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        )
        .relation_children
        .is_empty()
    );
}

#[test]
fn references_and_create_index_complete_target_columns() {
    let index = CompletionIndex::new(&contextual_fixture());
    for sql in [
        "CREATE TABLE orders (user_id BIGINT REFERENCES users (u",
        "CREATE INDEX ix_users ON users (u",
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert!(
            candidates.iter().any(|candidate| {
                candidate.kind == CompletionKind::Column && candidate.label == "username"
            }),
            "{sql}: {candidates:?}"
        );
        assert!(candidates.iter().all(|candidate| {
            candidate.kind == CompletionKind::Column || candidate.kind == CompletionKind::Keyword
        }));
    }
}

#[test]
fn ddl_existing_object_targets_are_strictly_filtered() {
    let mut entries = fixture();
    let connection = entries[0].id.profile_id();
    let schema = entries[1].id.clone();
    for (kind, name) in [
        (CatalogKind::View, "report"),
        (CatalogKind::Index, "ix_users"),
        (CatalogKind::Sequence, "order_seq"),
        (CatalogKind::Type, "status_type"),
        (CatalogKind::Function, "format_user"),
        (CatalogKind::Procedure, "refresh_users"),
        (CatalogKind::Trigger, "users_trigger"),
    ] {
        let entry = if matches!(kind, CatalogKind::View | CatalogKind::MaterializedView) {
            CatalogEntry::relation(
                CatalogId::new(connection, kind, ["app", "public", name]),
                schema.clone(),
                qualified("app", Some("public"), name),
                "view",
                OptionalMetadata::Supported(None),
                false,
            )
        } else if kind == CatalogKind::Trigger {
            CatalogEntry::relation_object(
                CatalogId::new(connection, kind, ["app", "public", name]),
                schema.clone(),
                entries[2].id.clone(),
                qualified("app", Some("public"), name),
                "trigger",
                OptionalMetadata::Supported(None),
            )
        } else if kind == CatalogKind::Index {
            CatalogEntry::relation_child(
                CatalogId::new(connection, kind, ["app", "public", "users", name]),
                entries[2].id.clone(),
                qualified("app", Some("public"), name),
                "index",
                OptionalMetadata::Supported(None),
                CatalogMetadata::Index(lazydb::db::catalog::IndexMetadata {
                    columns: Vec::new(),
                    unique: false,
                }),
            )
        } else {
            CatalogEntry::object(
                CatalogId::new(connection, kind, ["app", "public", name]),
                schema.clone(),
                qualified("app", Some("public"), name),
                "object",
                OptionalMetadata::Supported(None),
                false,
            )
        };
        entries.push(entry.unwrap());
    }
    let index = CompletionIndex::new(&entries);
    for (sql, kind, name) in [
        ("DROP VIEW rep", CompletionKind::View, "report"),
        ("DROP INDEX ix_", CompletionKind::Index, "ix_users"),
        ("DROP SEQUENCE ord", CompletionKind::Sequence, "order_seq"),
        ("DROP TYPE sta", CompletionKind::Type, "status_type"),
        ("DROP FUNCTION for", CompletionKind::Function, "format_user"),
        (
            "DROP PROCEDURE ref",
            CompletionKind::Procedure,
            "refresh_users",
        ),
        ("DROP TRIGGER use", CompletionKind::Trigger, "users_trigger"),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.kind == kind && candidate.label == name),
            "{sql}: {candidates:?}"
        );
        assert!(
            candidates.iter().all(|candidate| candidate.kind == kind),
            "{sql}: {candidates:?}"
        );
    }
}

#[test]
fn ddl_existing_objects_support_qualified_names() {
    let mut entries = fixture();
    let index_id = CatalogId::new(
        entries[2].id.profile_id(),
        CatalogKind::Index,
        ["app", "public", "users", "ix_users"],
    );
    entries.push(
        CatalogEntry::relation_child(
            index_id,
            entries[2].id.clone(),
            qualified("app", Some("public"), "ix_users"),
            "index",
            OptionalMetadata::Supported(None),
            CatalogMetadata::Index(lazydb::db::catalog::IndexMetadata {
                columns: vec!["id".into()],
                unique: false,
            }),
        )
        .unwrap(),
    );
    let index = CompletionIndex::new(&entries);

    let table = complete(
        "DROP TABLE app.public.us",
        "DROP TABLE app.public.us".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        table.iter().any(|candidate| {
            candidate.kind == CompletionKind::Table
                && candidate.label == "users"
                && candidate.insert_text == "\"users\""
        }),
        "{table:?}"
    );
    assert!(
        table
            .iter()
            .all(|candidate| candidate.kind == CompletionKind::Table)
    );

    let object = complete(
        "DROP INDEX app.public.ix_",
        "DROP INDEX app.public.ix_".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(object.iter().any(|candidate| {
        candidate.kind == CompletionKind::Index && candidate.label == "ix_users"
    }));
    assert!(
        object
            .iter()
            .all(|candidate| candidate.kind == CompletionKind::Index)
    );
}

#[test]
fn create_index_and_view_handoff_complete_the_following_relation_or_query() {
    let index = CompletionIndex::new(&fixture());
    let create_index = "CREATE INDEX ix_users ON us";
    let candidates = complete(
        create_index,
        create_index.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.kind == CompletionKind::Table && candidate.label == "users")
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind == CompletionKind::Table)
    );

    let index = CompletionIndex::new(&contextual_fixture());
    let view = "CREATE VIEW active_users AS sel";
    let candidates = complete(
        view,
        view.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert_eq!(
        candidates.first().map(|candidate| candidate.label.as_str()),
        Some("SELECT")
    );

    let query = "CREATE VIEW active_users AS SELECT u FROM sys_user";
    let candidates = complete(
        query,
        "CREATE VIEW active_users AS SELECT u".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.kind == CompletionKind::Column
                && candidate.label == "username"),
        "{candidates:?}"
    );
}

#[test]
fn sql_server_drop_index_on_completes_relation() {
    let index = CompletionIndex::new(&fixture());
    let sql = "DROP INDEX ix_users ON us";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::SqlServer,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.kind == CompletionKind::Table && candidate.label == "users")
    );
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.kind == CompletionKind::Table)
    );
}

#[test]
fn completed_projection_prefers_from_keyword() {
    let index = CompletionIndex::new(&contextual_fixture());

    for sql in ["select * f", "select \"username\" f"] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );

        assert_eq!(
            candidates.first().map(|candidate| candidate.label.as_str()),
            Some("FROM"),
            "unexpected candidates for {sql}: {candidates:?}"
        );
    }
}

#[test]
fn incomplete_projection_prefers_expression_keyword() {
    let index = CompletionIndex::new(&contextual_fixture());

    for sql in ["select f", "select username, f", "select username + f"] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );

        assert_eq!(
            candidates.first().map(|candidate| candidate.label.as_str()),
            Some("FALSE"),
            "unexpected candidates for {sql}: {candidates:?}"
        );
        assert!(
            candidates.iter().all(|candidate| candidate.label != "FROM"),
            "FROM should not be offered for {sql}: {candidates:?}"
        );
    }
}

#[test]
fn predicate_completion_offers_predicate_keywords() {
    let index = CompletionIndex::new(&contextual_fixture());
    let sql = "select * from sys_user where n";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );

    assert!(candidates.iter().any(|candidate| {
        candidate.kind == CompletionKind::Keyword && candidate.label == "NOT"
    }));
}

#[test]
fn strings_and_comments_do_not_create_relation_bindings() {
    let index = CompletionIndex::new(&contextual_fixture());
    for sql in [
        "select 'from user_agreement_accept' as note from sys_user where a",
        "select 1 /* join user_agreement_accept */ from sys_user where a",
        "select 1 -- join user_agreement_accept\nfrom sys_user where a",
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert!(!candidates.iter().any(|candidate| {
            candidate.kind == CompletionKind::Column && candidate.label == "agreement_id"
        }));
    }
}

#[test]
fn quoted_relation_identifiers_are_tokenized_as_single_words() {
    let index = CompletionIndex::new(&contextual_fixture());
    for (sql, dialect) in [
        (
            "select update_ from \"sys_user\" where update_",
            SqlDialect::Postgres,
        ),
        (
            "select update_ from `sys_user` where update_",
            SqlDialect::MySql,
        ),
    ] {
        let candidates = complete(
            sql,
            sql.len(),
            dialect,
            &index,
            CompletionContext::default(),
        );
        assert!(candidates.iter().any(|candidate| {
            candidate.kind == CompletionKind::Column && candidate.label == "update_time"
        }));
    }
}

#[test]
fn join_predicate_sees_both_relation_bindings() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select * from users u join roles r on ";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let labels = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"user_name"));
    assert!(labels.contains(&"role_name"));
    assert!(!labels.contains(&"audit_message"));
}

#[test]
fn comma_from_list_sees_each_relation_binding() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select  from users u, roles r";
    let candidates = complete(
        sql,
        "select ".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let labels = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"user_name"));
    assert!(labels.contains(&"role_name"));
    assert!(!labels.contains(&"audit_message"));
}

#[test]
fn alias_qualified_completion_only_uses_that_binding() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select r. from users u join roles r on u.id = r.id";
    let candidates = complete(
        sql,
        "select r.".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let labels = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"role_name"));
    assert!(!labels.contains(&"user_name"));
}

#[test]
fn active_schema_resolves_duplicate_unqualified_relations() {
    let connection = Uuid::new_v4();
    let mut entries = Vec::new();
    for (schema_name, column) in [("public", "public_value"), ("audit", "audit_value")] {
        let schema = CatalogId::new(connection, CatalogKind::Schema, ["app", schema_name]);
        let table = CatalogId::new(
            connection,
            CatalogKind::Table,
            ["app", schema_name, "events"],
        );
        entries.push(
            CatalogEntry::relation(
                table.clone(),
                schema,
                qualified("app", Some(schema_name), "events"),
                "table",
                OptionalMetadata::Supported(None),
                true,
            )
            .unwrap(),
        );
        entries.push(
            CatalogEntry::relation_child(
                CatalogId::new(
                    connection,
                    CatalogKind::Column,
                    ["app", schema_name, "events", column],
                ),
                table,
                qualified("app", Some(schema_name), column),
                "column",
                OptionalMetadata::Unsupported,
                CatalogMetadata::Column(ColumnMetadata::new(1, "text", true)),
            )
            .unwrap(),
        );
    }
    let index = CompletionIndex::new(&entries);
    let sql = "select  from events";
    let candidates = complete(
        sql,
        "select ".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext {
            database: Some("app"),
            schema: Some("audit"),
        },
    );
    let labels = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"audit_value"));
    assert!(!labels.contains(&"public_value"));
}

#[test]
fn subquery_does_not_leak_its_relations_to_the_outer_query() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select * from users u where exists (select 1 from roles r) and ";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let labels = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"user_name"));
    assert!(!labels.contains(&"role_name"));
}

#[test]
fn correlated_subquery_can_see_enclosing_relations() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select * from users u where exists (select 1 from roles r where )";
    let cursor = sql.len() - 1;
    let candidates = complete(
        sql,
        cursor,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let labels = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"user_name"));
    assert!(labels.contains(&"role_name"));
}

#[test]
fn sibling_subquery_relations_are_not_visible() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let sql = "select * from users u where exists (select 1 from roles r) and exists (select 1 from audit_log a where )";
    let cursor = sql.len() - 1;
    let candidates = complete(
        sql,
        cursor,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let labels = candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect::<Vec<_>>();

    assert!(labels.contains(&"user_name"));
    assert!(labels.contains(&"audit_message"));
    assert!(!labels.contains(&"role_name"));
}

#[test]
fn relation_completion_ignores_identifier_separators() {
    let index = CompletionIndex::new(&compact_match_fixture());
    let sql = "select * from sysuser";
    let candidates = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext {
            database: Some("app"),
            schema: Some("public"),
        },
    );

    assert!(candidates.iter().any(|candidate| {
        candidate.kind == CompletionKind::Table
            && candidate.label == "sys_user"
            && candidate.insert_text == "sys_user"
    }));
    assert_eq!(
        candidates
            .iter()
            .filter(|candidate| candidate.label == "sys_user")
            .count(),
        1
    );
}

#[test]
fn ordinary_prefix_ranks_above_compact_prefix() {
    let index = CompletionIndex::new(&compact_match_fixture());
    let sql = "select * from sysuser";
    let labels = complete(
        sql,
        sql.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    )
    .into_iter()
    .filter(|candidate| candidate.kind == CompletionKind::Table)
    .map(|candidate| candidate.label)
    .collect::<Vec<_>>();

    assert_eq!(labels[..2], ["sysuser_archive", "sys_user"]);
}

#[test]
fn alias_column_completion_ignores_identifier_separators() {
    let index = CompletionIndex::new(&compact_match_fixture());
    let sql = "select u.userid from users u";
    let candidates = complete(
        sql,
        "select u.userid".len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );

    assert!(candidates.iter().any(|candidate| {
        candidate.kind == CompletionKind::Column && candidate.label == "user_id"
    }));
}

#[test]
fn completion_is_contextual_and_quotes_raw_names() {
    let index = CompletionIndex::new(&fixture());
    let candidates = complete(
        "select * from us",
        16,
        SqlDialect::Postgres,
        &index,
        CompletionContext {
            database: Some("app"),
            schema: Some("public"),
        },
    );
    assert!(candidates.iter().any(
        |candidate| candidate.kind == CompletionKind::Table && candidate.insert_text == "users"
    ));
    assert_eq!(
        quote_identifier("odd\" name", SqlDialect::Postgres),
        "\"odd\"\" name\""
    );
    assert_eq!(
        quote_identifier("odd`name", SqlDialect::MySql),
        "`odd``name`"
    );
    assert_eq!(
        quote_identifier("odd]name", SqlDialect::SqlServer),
        "[odd]]name]"
    );
}

#[test]
fn sql_server_completion_understands_bracketed_relations() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let text = "SELECT [users]. FROM [public].[users]";
    let candidates = complete(
        text,
        "SELECT [users].".len(),
        SqlDialect::SqlServer,
        &index,
        CompletionContext {
            database: Some("app"),
            schema: Some("public"),
        },
    );
    assert!(candidates.iter().any(|candidate| {
        candidate.kind == CompletionKind::Column && candidate.insert_text == "[id]"
    }));

    let variable = "SELECT @user";
    assert!(
        complete(
            variable,
            variable.len(),
            SqlDialect::SqlServer,
            &index,
            CompletionContext::default(),
        )
        .is_empty()
    );
}

#[test]
fn sql_server_completion_uses_case_insensitive_active_database_and_schema() {
    let index = CompletionIndex::new(&multi_relation_fixture());
    let text = "SELECT * FROM ";
    let candidates = complete(
        text,
        text.len(),
        SqlDialect::SqlServer,
        &index,
        CompletionContext {
            database: Some("APP"),
            schema: Some("PUBLIC"),
        },
    );
    let users = candidates
        .iter()
        .find(|candidate| candidate.kind == CompletionKind::Table && candidate.label == "users")
        .expect("active SQL Server target should resolve users");
    assert_eq!(users.insert_text, "users");
}

#[test]
fn completion_includes_databases_and_qualified_children() {
    let index = CompletionIndex::new(&fixture());

    let databases = complete(
        "select * from ",
        14,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(databases.iter().any(|candidate| {
        candidate.kind == CompletionKind::Database && candidate.label == "app"
    }));

    let schema_text = "select * from app.";
    let schemas = complete(
        schema_text,
        schema_text.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(schemas.iter().any(|candidate| {
        candidate.kind == CompletionKind::Schema && candidate.label == "public"
    }));

    let table_text = "select * from app.public.";
    let tables = complete(
        table_text,
        table_text.len(),
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(tables.iter().any(|candidate| {
        candidate.kind == CompletionKind::Table
            && candidate.label == "users"
            && candidate.detail.as_deref() == Some("(app.public)")
    }));
}

#[test]
fn alias_column_completion_uses_relation_columns_and_native_type() {
    let index = CompletionIndex::new(&fixture());
    let candidates = complete(
        "select u. from users u",
        9,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let column = candidates
        .iter()
        .find(|candidate| candidate.kind == CompletionKind::Column)
        .expect("alias should resolve to table columns");
    assert_eq!(column.label, "odd name");
    assert_eq!(column.detail.as_deref(), Some("text<ESC>[31m"));
}

#[test]
fn column_completion_detail_uses_short_type_spelling() {
    let mut entries = fixture();
    let connection = entries[0].id.profile_id();
    let table = entries[2].id.clone();
    for (name, native_type) in [
        ("code", "character varying(30)"),
        ("created_at", "timestamp without time zone"),
    ] {
        entries.push(
            CatalogEntry::relation_child(
                CatalogId::new(
                    connection,
                    CatalogKind::Column,
                    ["app", "public", "users", name],
                ),
                table.clone(),
                qualified("app", Some("public"), name),
                "column",
                OptionalMetadata::Unsupported,
                CatalogMetadata::Column(ColumnMetadata::new(2, native_type, true)),
            )
            .unwrap(),
        );
    }
    let index = CompletionIndex::new(&entries);
    let candidates = complete(
        "select c from users",
        8,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );

    let detail = |label: &str| {
        candidates
            .iter()
            .find(|candidate| candidate.label == label)
            .and_then(|candidate| candidate.detail.clone())
    };
    assert_eq!(detail("code").as_deref(), Some("varchar(30)"));
    assert_eq!(detail("created_at").as_deref(), Some("timestamp"));
}

#[test]
fn unqualified_columns_are_limited_to_relations_in_current_statement() {
    let mut entries = fixture();
    let connection = entries[0].id.profile_id();
    let other_schema = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    let other_table = CatalogId::new(connection, CatalogKind::Table, ["app", "public", "roles"]);
    entries.push(
        CatalogEntry::relation(
            other_table.clone(),
            other_schema,
            qualified("app", Some("public"), "roles"),
            "table",
            OptionalMetadata::Supported(None),
            true,
        )
        .unwrap(),
    );
    entries.push(
        CatalogEntry::relation_child(
            CatalogId::new(
                connection,
                CatalogKind::Column,
                ["app", "public", "roles", "user_id"],
            ),
            other_table,
            qualified("app", Some("public"), "user_id"),
            "column",
            OptionalMetadata::Unsupported,
            CatalogMetadata::Column(ColumnMetadata::new(1, "bigint", false)),
        )
        .unwrap(),
    );
    let index = CompletionIndex::new(&entries);
    let candidates = complete(
        "select odd from users u",
        10,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.label == "odd name")
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.label == "user_id")
    );
}

#[test]
fn hostile_display_text_does_not_change_insertion() {
    let mut nodes = fixture();
    let hostile_profile = Uuid::new_v4();
    let hostile_schema = CatalogId::new(hostile_profile, CatalogKind::Schema, ["x", "public"]);
    nodes.push(
        CatalogEntry::relation(
            CatalogId::new(
                hostile_profile,
                CatalogKind::Table,
                ["x", "public", "\x1b[2J"],
            ),
            hostile_schema,
            qualified("x", Some("public"), "\x1b[2J"),
            "table",
            OptionalMetadata::Supported(Some("\x00detail".into())),
            false,
        )
        .unwrap(),
    );
    let index = CompletionIndex::new(&nodes);
    let candidates = complete(
        "from ",
        5,
        SqlDialect::Postgres,
        &index,
        CompletionContext::default(),
    );
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.insert_text.contains("2J"))
        .unwrap();
    assert!(!candidate.label.contains('\x1b'));
    assert!(candidate.insert_text.contains("\x1b[2J"));
}

#[test]
fn statement_keywords_rank_before_matching_catalog_names() {
    let connection = Uuid::new_v4();
    let nodes = [("sales", "s"), ("data", "d"), ("facts", "f")]
        .into_iter()
        .map(|(name, _)| {
            CatalogEntry::relation(
                CatalogId::new(connection, CatalogKind::Table, ["app", "public", name]),
                CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]),
                qualified("app", Some("public"), name),
                "table",
                OptionalMetadata::Supported(None),
                false,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let index = CompletionIndex::new(&nodes);

    for (prefix, keyword) in [("s", "SELECT"), ("d", "DELETE"), ("u", "UPDATE")] {
        let candidates = complete(
            prefix,
            prefix.len(),
            SqlDialect::Postgres,
            &index,
            CompletionContext::default(),
        );
        assert_eq!(
            candidates.first().map(|candidate| candidate.label.as_str()),
            Some(keyword)
        );
        assert_eq!(
            candidates.first().map(|candidate| candidate.kind),
            Some(CompletionKind::Keyword)
        );
    }
}

#[test]
fn index_retains_only_completion_relevant_entries() {
    let mut entries = fixture();
    let profile = entries[0].id.profile_id();
    entries.push(
        CatalogEntry::object(
            CatalogId::new(profile, CatalogKind::Sequence, ["app", "public", "seq"]),
            CatalogId::new(profile, CatalogKind::Schema, ["app", "public"]),
            qualified("app", Some("public"), "seq"),
            "sequence",
            OptionalMetadata::Supported(None),
            false,
        )
        .unwrap(),
    );
    let index = CompletionIndex::new(&entries);
    assert!(index.entries().iter().all(|entry| matches!(
        entry.kind,
        CatalogKind::Database
            | CatalogKind::Schema
            | CatalogKind::Table
            | CatalogKind::View
            | CatalogKind::MaterializedView
            | CatalogKind::Column
            | CatalogKind::Function
            | CatalogKind::Procedure
            | CatalogKind::Sequence
            | CatalogKind::Type
            | CatalogKind::Index
            | CatalogKind::PrimaryKey
            | CatalogKind::UniqueConstraint
            | CatalogKind::ForeignKey
            | CatalogKind::CheckConstraint
            | CatalogKind::Trigger
    )));
}

#[test]
fn scoped_index_deduplicates_replaces_removed_entries_and_rejects_out_of_scope_entries() {
    let entries = fixture();
    let profile = entries[0].id.profile_id();
    let scope = CatalogScope {
        databases: CatalogSelection::Selected(vec![DatabaseScope {
            name: "app".into(),
            schemas: CatalogSelection::Selected(vec!["public".into()]),
        }]),
    };
    let mut index = CompletionIndex::default();
    index.replace_scoped(&entries, &scope);
    index.append_scoped(&[entries[2].clone(), entries[3].clone()], &scope);

    assert_eq!(
        index
            .entries()
            .iter()
            .filter(|entry| entry.id == entries[2].id)
            .count(),
        1
    );
    assert!(index.entries().iter().all(|entry| {
        entry.kind == CatalogKind::Database
            || (entry.qualified_name.database.as_deref() == Some("app")
                && entry.qualified_name.schema.as_deref() == Some("public"))
    }));

    let other_database = CatalogEntry::relation(
        CatalogId::new(profile, CatalogKind::Table, ["other", "public", "orders"]),
        CatalogId::new(profile, CatalogKind::Schema, ["other", "public"]),
        qualified("other", Some("public"), "orders"),
        "table",
        OptionalMetadata::Supported(None),
        false,
    )
    .unwrap();
    index.append_scoped(&[other_database], &scope);
    assert!(
        !index
            .entries()
            .iter()
            .any(|entry| entry.qualified_name.database.as_deref() == Some("other"))
    );

    index.replace_scoped(&[entries[0].clone()], &scope);
    assert_eq!(index.entries().len(), 1);
    assert_eq!(index.entries()[0].kind, CatalogKind::Database);
}

#[test]
fn app_completion_prefers_the_active_console_target_schema() {
    let mut profile = import_connection_url("postgres://localhost/app", Some("app"))
        .unwrap()
        .profile;
    profile.default_schema = Some("audit".into());
    profile.catalog_scope = CatalogScope::for_profile(DatabaseKind::Postgres, "app", Some("audit"));
    let profile_id = profile.id;
    let entries = ["public", "audit"]
        .map(|schema| {
            CatalogEntry::relation(
                CatalogId::new(profile_id, CatalogKind::Table, ["app", schema, "orders"]),
                CatalogId::new(profile_id, CatalogKind::Schema, ["app", schema]),
                qualified("app", Some(schema), "orders"),
                "table",
                OptionalMetadata::Supported(None),
                false,
            )
            .unwrap()
        })
        .to_vec();
    let mut app = App::new(vec![profile]);
    app.update(Action::ConnectionSucceeded {
        profile_id,
        generation: 1,
        server: ServerInfo {
            kind: DatabaseKind::Postgres,
            version: "16.4".into(),
            database: "app".into(),
            current_user: None,
        },
        mutation_capabilities: Default::default(),
    });
    app.active_console_mut()
        .execution_target
        .as_mut()
        .unwrap()
        .schema = Some("public".into());
    app.explorer.completion_index = CompletionIndex::new(&entries);
    app.update(Action::ReplaceEditor("select * from or".into()));
    app.update(Action::EditorKey(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('A'),
        crossterm::event::KeyModifiers::NONE,
    )));

    app.update(Action::CompletionExplicit);

    let popup = app.active_console().completion.as_ref().unwrap();
    let scores = popup
        .candidates
        .iter()
        .filter(|candidate| {
            candidate.label == "orders" && candidate.detail.as_deref() == Some("(app.public)")
        })
        .map(|candidate| candidate.score.schema)
        .collect::<Vec<_>>();
    assert_eq!(scores, [1]);
}

#[test]
fn relation_completion_uses_shortest_target_relative_reference() {
    let connection = Uuid::new_v4();
    let database = CatalogId::new(connection, CatalogKind::Database, ["app"]);
    let public = CatalogId::new(connection, CatalogKind::Schema, ["app", "public"]);
    let mut entries = vec![
        CatalogEntry::database(
            database.clone(),
            qualified("app", None, "app"),
            "database",
            OptionalMetadata::Supported(None),
            false,
        )
        .unwrap(),
        CatalogEntry::schema(
            public.clone(),
            database,
            qualified("app", Some("public"), "public"),
            "schema",
            OptionalMetadata::Supported(None),
            false,
        )
        .unwrap(),
    ];
    entries.extend(
        [
            ("app", "public", "users"),
            ("app", "audit", "users"),
            ("analytics", "bi", "users"),
        ]
        .into_iter()
        .map(|(database, schema, object)| {
            CatalogEntry::relation(
                CatalogId::new(connection, CatalogKind::Table, [database, schema, object]),
                CatalogId::new(connection, CatalogKind::Schema, [database, schema]),
                qualified(database, Some(schema), object),
                "table",
                OptionalMetadata::Supported(None),
                false,
            )
            .unwrap()
        })
        .collect::<Vec<_>>(),
    );
    let index = CompletionIndex::new(&entries);
    let context = CompletionContext {
        database: Some("app"),
        schema: Some("public"),
    };
    let candidates = complete(
        "select * from us",
        16,
        SqlDialect::Postgres,
        &index,
        context,
    );
    let by_detail = |detail: &str| {
        candidates
            .iter()
            .find(|candidate| {
                candidate.label == "users" && candidate.detail.as_deref() == Some(detail)
            })
            .unwrap()
    };
    assert_eq!(by_detail("(app.public)").insert_text, "users");
    assert_eq!(by_detail("(app.audit)").insert_text, "audit.users");
    assert_eq!(
        by_detail("(analytics.bi)").insert_text,
        "analytics.bi.users"
    );

    let qualified_text = "select * from app.public.us";
    let qualified_candidates = complete(
        qualified_text,
        qualified_text.len(),
        SqlDialect::Postgres,
        &index,
        context,
    );
    assert_eq!(
        qualified_candidates
            .iter()
            .map(|candidate| candidate.label.as_str())
            .collect::<Vec<_>>(),
        ["users"]
    );
    assert_eq!(
        qualified_candidates[0].detail.as_deref(),
        Some("(app.public)")
    );
}

#[test]
fn relation_completion_deduplicates_mirrored_database_and_schema_detail() {
    let connection = Uuid::new_v4();
    let entry = CatalogEntry::relation(
        CatalogId::new(connection, CatalogKind::Table, ["app", "app", "users"]),
        CatalogId::new(connection, CatalogKind::Schema, ["app", "app"]),
        qualified("app", Some("app"), "users"),
        "table",
        OptionalMetadata::Supported(None),
        false,
    )
    .unwrap();
    let candidates = complete(
        "select * from us",
        16,
        SqlDialect::MySql,
        &CompletionIndex::new(&[entry]),
        CompletionContext::default(),
    );

    assert_eq!(candidates[0].label, "users");
    assert_eq!(candidates[0].detail.as_deref(), Some("(app)"));
}

fn qualified(database: &str, schema: Option<&str>, object: &str) -> QualifiedName {
    QualifiedName {
        database: Some(database.into()),
        schema: schema.map(str::to_owned),
        object: object.into(),
    }
}
