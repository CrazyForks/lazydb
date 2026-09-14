use lazydb::db::catalog::{CatalogId, CatalogKind, NamespaceModel};
use lazydb::db::catalog_change_set::{CatalogFieldChanges, FieldChange};
use lazydb::db::catalog_mutation::{
    CatalogMutationAnchor, CatalogMutationAvailability, CatalogMutationCapabilities,
    CatalogMutationMode, CatalogMutationOption, CatalogObjectType, MutationCompletion,
    MutationProgress,
};
use lazydb::db::catalog_mutation::{CatalogRebuildPlan, CatalogRebuildStep};
use lazydb::db::mssql::MsSqlAdapter;
use lazydb::db::mysql::MySqlAdapter;
use lazydb::db::oracle::OracleAdapter;
use lazydb::db::sqlite::SqliteAdapter;
use lazydb::model::catalog_editor::{CatalogDraft, TableDraft};
use lazydb::model::explorer::ExplorerNodeId;
use lazydb::model::explorer_actions::{ExplorerActionAvailability, ExplorerActionContext};
use lazydb::profile::DatabaseKind;
use uuid::Uuid;

fn available(object_type: CatalogObjectType) -> CatalogMutationOption {
    CatalogMutationOption {
        object_type,
        availability: CatalogMutationAvailability::Available,
    }
}

#[test]
fn capabilities_distinguish_available_unavailable_and_unimplemented_operations() {
    let capabilities = CatalogMutationCapabilities {
        create: vec![available(CatalogObjectType::Catalog(CatalogKind::Schema))],
        ..CatalogMutationCapabilities::default()
    };

    assert_eq!(
        capabilities.create_availability(CatalogObjectType::Catalog(CatalogKind::Schema)),
        Some(CatalogMutationAvailability::Available)
    );
    assert_eq!(
        capabilities.create_availability(CatalogObjectType::Catalog(CatalogKind::Table)),
        None,
        "an operation not advertised by an adapter must not be treated as available"
    );
}

#[test]
fn default_capabilities_do_not_accidentally_expose_mutations() {
    let capabilities = CatalogMutationCapabilities::default();

    assert!(capabilities.profile_create.is_empty());
    assert!(capabilities.create.is_empty());
    assert!(capabilities.edit.is_empty());
}

#[test]
fn database_is_schema_does_not_offer_a_phantom_schema_creation() {
    let capabilities = CatalogMutationCapabilities {
        create: vec![
            available(CatalogObjectType::Catalog(CatalogKind::Table)),
            available(CatalogObjectType::Catalog(CatalogKind::View)),
        ],
        ..CatalogMutationCapabilities::default()
    };
    let database = CatalogId::new(
        Uuid::from_u128(1),
        CatalogKind::Database,
        vec!["app".to_owned()],
    );

    let options = capabilities
        .create_options_for_namespace(
            &CatalogMutationAnchor::Catalog(database),
            None,
            NamespaceModel::DatabaseIsSchema,
        )
        .expect("valid database anchor");

    assert_eq!(
        options,
        vec![
            CatalogObjectType::Catalog(CatalogKind::Table),
            CatalogObjectType::Catalog(CatalogKind::View),
        ]
    );
}

#[test]
fn explorer_action_resolution_returns_a_reason_instead_of_silently_dropping_a_key() {
    let capabilities = CatalogMutationCapabilities::default();
    let context = ExplorerActionContext {
        database_kind: DatabaseKind::Oracle,
        connected: true,
        read_only: false,
        namespace_model: NamespaceModel::DatabaseAndSchema,
        capabilities: &capabilities,
        selected_entry: None,
    };
    let selected = ExplorerNodeId::Profile(Uuid::from_u128(1));

    assert!(matches!(
        context.resolve(Some(&selected), CatalogMutationMode::Create),
        ExplorerActionAvailability::Unavailable(reason) if reason.contains("Oracle")
    ));
}

#[test]
fn mutation_change_sets_preserve_unknown_and_unchanged_fields() {
    let changes = CatalogFieldChanges::<String> {
        name: FieldChange::changed("renamed".to_owned()),
        comment: FieldChange::Unknown,
    };

    assert!(changes.has_changes());
    assert_eq!(
        changes.name.as_changed().map(String::as_str),
        Some("renamed")
    );
    assert_eq!(changes.comment.as_changed(), None);
}

#[test]
fn mutation_progress_distinguishes_rollback_partial_apply_and_unknown_commit() {
    assert_eq!(
        MutationProgress::failed(1, vec![0]).completion,
        MutationCompletion::Failed
    );
    assert_eq!(
        MutationProgress::partially_applied(1, vec![0]).completion,
        MutationCompletion::PartiallyApplied
    );
    assert_eq!(
        MutationProgress::outcome_unknown(vec![0]).completion,
        MutationCompletion::OutcomeUnknown
    );
    assert_eq!(MutationProgress::succeeded(2).completed_steps, vec![0, 1]);
}

#[test]
fn oracle_advertises_only_the_object_groups_with_creation_plans() {
    let capabilities = OracleAdapter::catalog_mutation_capabilities();
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::Table))
            .is_some()
    );
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::View))
            .is_some()
    );
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::Sequence))
            .is_some()
    );
    assert_eq!(capabilities.edit.len(), 3);
}

#[test]
fn oracle_advertises_editing_only_for_loaded_definition_types() {
    let capabilities = OracleAdapter::catalog_mutation_capabilities();
    assert!(capabilities.edit.iter().all(|option| matches!(
        option.object_type,
        CatalogObjectType::Catalog(CatalogKind::Table | CatalogKind::View | CatalogKind::Sequence)
    )));
}

#[test]
fn oracle_table_edit_plan_renames_only_the_table_identity() {
    let profile_id = Uuid::from_u128(9);
    let object = CatalogId::new(profile_id, CatalogKind::Table, ["SERVICE", "APP", "OLD"]);
    let request = lazydb::db::catalog_mutation::CatalogMutationRequest {
        connection: lazydb::identity::ConnectionIdentity {
            profile_id,
            generation: 1,
        },
        request_id: 2,
        catalog_epoch: 3,
        mode: CatalogMutationMode::Edit,
        anchor: CatalogMutationAnchor::Catalog(object),
        object_type: CatalogObjectType::Catalog(CatalogKind::Table),
        current_database: Some("SERVICE".to_owned()),
    };
    let mut table = TableDraft::new("APP");
    table.name.set("NEW");
    let baseline = lazydb::db::catalog_mutation::CatalogObjectDefinition::Table(
        lazydb::db::catalog_mutation::TableDefinition {
            database: "SERVICE".into(),
            schema: "APP".into(),
            name: "OLD".into(),
            owner: "APP".into(),
            comment: lazydb::db::catalog::OptionalMetadata::Unsupported,
            columns: vec![],
            indexes: vec![],
            constraints: vec![],
            baseline_fingerprint: "old".into(),
        },
    );
    let plan =
        OracleAdapter::plan_catalog_mutation(request, CatalogDraft::Table(table), Some(baseline))
            .expect("Oracle rename plan should be valid");
    assert_eq!(
        plan.statements(),
        &["ALTER TABLE \"APP\".\"OLD\" RENAME TO \"NEW\""]
    );
}

#[test]
fn oracle_view_edit_plan_replaces_the_definition() {
    let profile_id = Uuid::from_u128(10);
    let object = CatalogId::new(profile_id, CatalogKind::View, ["SERVICE", "APP", "V"]);
    let request = lazydb::db::catalog_mutation::CatalogMutationRequest {
        connection: lazydb::identity::ConnectionIdentity {
            profile_id,
            generation: 1,
        },
        request_id: 3,
        catalog_epoch: 4,
        mode: CatalogMutationMode::Edit,
        anchor: CatalogMutationAnchor::Catalog(object),
        object_type: CatalogObjectType::Catalog(CatalogKind::View),
        current_database: Some("SERVICE".to_owned()),
    };
    let mut view = lazydb::model::catalog_editor::ViewDraft {
        name: "V".into(),
        schema: "APP".into(),
        owner: "APP".into(),
        comment: Default::default(),
        query: "SELECT 1 FROM dual".into(),
        output_columns: Default::default(),
        security_barrier: lazydb::db::catalog_mutation::ViewOption::unavailable("not applicable"),
        security_invoker: lazydb::db::catalog_mutation::ViewOption::unavailable("not applicable"),
        check_option: lazydb::db::catalog_mutation::ViewOption::unavailable("not applicable"),
        focus: lazydb::model::catalog_editor::CatalogFormFocus::Name,
    };
    view.query.set("SELECT 2 FROM dual");
    let baseline = lazydb::db::catalog_mutation::CatalogObjectDefinition::View(
        lazydb::db::catalog_mutation::ViewDefinition {
            database: "SERVICE".into(),
            schema: "APP".into(),
            name: "V".into(),
            owner: "APP".into(),
            comment: lazydb::db::catalog::OptionalMetadata::Unsupported,
            query: "SELECT 1 FROM dual".into(),
            output_columns: vec![],
            security_barrier: lazydb::db::catalog_mutation::ViewOption::unavailable(
                "not applicable",
            ),
            security_invoker: lazydb::db::catalog_mutation::ViewOption::unavailable(
                "not applicable",
            ),
            check_option: lazydb::db::catalog_mutation::ViewOption::unavailable("not applicable"),
            baseline_fingerprint: "old".into(),
        },
    );
    let plan =
        OracleAdapter::plan_catalog_mutation(request, CatalogDraft::View(view), Some(baseline))
            .expect("Oracle view replace plan should be valid");
    assert_eq!(
        plan.statements(),
        &["CREATE OR REPLACE VIEW \"APP\".\"V\" AS SELECT 2 FROM dual"]
    );
}

#[test]
fn oracle_sequence_edit_plan_updates_runtime_attributes() {
    let profile_id = Uuid::from_u128(11);
    let object = CatalogId::new(profile_id, CatalogKind::Sequence, ["SERVICE", "APP", "SEQ"]);
    let request = lazydb::db::catalog_mutation::CatalogMutationRequest {
        connection: lazydb::identity::ConnectionIdentity {
            profile_id,
            generation: 1,
        },
        request_id: 4,
        catalog_epoch: 5,
        mode: CatalogMutationMode::Edit,
        anchor: CatalogMutationAnchor::Catalog(object),
        object_type: CatalogObjectType::Catalog(CatalogKind::Sequence),
        current_database: Some("SERVICE".to_owned()),
    };
    let sequence = lazydb::model::catalog_editor::SequenceDraft {
        name: "SEQ".into(),
        schema: "APP".into(),
        owner: "APP".into(),
        comment: Default::default(),
        data_type: "NUMBER".into(),
        increment: "5".into(),
        min_value: lazydb::model::catalog_editor::SequenceBoundDraft::from(
            lazydb::db::catalog_mutation::SequenceBound::Unset,
        ),
        max_value: lazydb::model::catalog_editor::SequenceBoundDraft::from(
            lazydb::db::catalog_mutation::SequenceBound::Unset,
        ),
        start_value: "1".into(),
        restart_value: Default::default(),
        cache: "20".into(),
        cycle: true,
        owned_by: "NONE".into(),
        focus: lazydb::model::catalog_editor::CatalogFormFocus::Name,
    };
    sequence.validate().unwrap();
    let baseline = lazydb::db::catalog_mutation::CatalogObjectDefinition::Sequence(
        lazydb::db::catalog_mutation::SequenceDefinition {
            database: "SERVICE".into(),
            schema: "APP".into(),
            name: "SEQ".into(),
            owner: "APP".into(),
            comment: lazydb::db::catalog::OptionalMetadata::Unsupported,
            data_type: "NUMBER".into(),
            increment: "1".into(),
            min_value: lazydb::db::catalog_mutation::SequenceBound::Unset,
            max_value: lazydb::db::catalog_mutation::SequenceBound::Unset,
            start_value: "1".into(),
            cache: "1".into(),
            cycle: false,
            owned_by: None,
            baseline_fingerprint: "old".into(),
        },
    );
    let plan = OracleAdapter::plan_catalog_mutation(
        request,
        CatalogDraft::Sequence(sequence),
        Some(baseline),
    )
    .expect("Oracle sequence edit plan should be valid");
    assert_eq!(
        plan.statements(),
        &["ALTER SEQUENCE \"APP\".\"SEQ\" INCREMENT BY 5 CACHE 20 CYCLE"]
    );
}

#[test]
fn mysql_database_is_schema_advertises_table_and_view_creation() {
    let capabilities = MySqlAdapter::catalog_mutation_capabilities();
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::Table))
            .is_some()
    );
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::View))
            .is_some()
    );
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::Schema))
            .is_none()
    );
    assert_eq!(capabilities.edit.len(), 1);
}

#[test]
fn mysql_table_create_plan_uses_backtick_quoting() {
    let profile_id = Uuid::from_u128(8);
    let request = lazydb::db::catalog_mutation::CatalogMutationRequest {
        connection: lazydb::identity::ConnectionIdentity {
            profile_id,
            generation: 1,
        },
        request_id: 1,
        catalog_epoch: 1,
        mode: CatalogMutationMode::Create,
        anchor: CatalogMutationAnchor::Group {
            schema: CatalogId::new(profile_id, CatalogKind::Schema, ["shop`db", "shop`db"]),
            group: lazydb::db::catalog::ObjectGroup::Tables,
        },
        object_type: CatalogObjectType::Catalog(CatalogKind::Table),
        current_database: Some("shop`db".to_owned()),
    };
    let mut table = TableDraft::new("shop`db");
    table.name.set("line`item");
    table.columns[0].name.set("id");
    table.columns[0].native_type.set("BIGINT");
    let plan = MySqlAdapter::plan_catalog_mutation(request, CatalogDraft::Table(table), None)
        .expect("MySQL table plan should be valid");
    assert_eq!(
        plan.statements()[0],
        "CREATE TABLE `shop``db`.`line``item` (`id` BIGINT)"
    );
}

#[test]
fn mysql_compatible_catalog_version_gates_remain_engine_specific() {
    assert!(lazydb::db::mysql::supports_catalog_version_for_kind(
        DatabaseKind::MySql,
        "8.0.13"
    ));
    assert!(!lazydb::db::mysql::supports_catalog_version_for_kind(
        DatabaseKind::MySql,
        "10.5.0-MariaDB"
    ));
    assert!(lazydb::db::mysql::supports_catalog_version_for_kind(
        DatabaseKind::MariaDb,
        "10.5.0-MariaDB"
    ));
    assert!(!lazydb::db::mysql::supports_catalog_version_for_kind(
        DatabaseKind::MariaDb,
        "8.0.13"
    ));
}

#[test]
fn sqlite_exposes_only_native_table_and_view_creation() {
    let capabilities = SqliteAdapter::catalog_mutation_capabilities();
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::Table))
            .is_some()
    );
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::View))
            .is_some()
    );
    assert!(
        capabilities
            .create_availability(CatalogObjectType::Catalog(CatalogKind::Schema))
            .is_none()
    );
    assert!(capabilities.edit.is_empty());
}

#[test]
fn sql_server_exposes_only_native_table_and_view_creation() {
    let capabilities = MsSqlAdapter::catalog_mutation_capabilities();
    assert!(
        capabilities
            .create
            .iter()
            .any(|option| { option.object_type == CatalogObjectType::Catalog(CatalogKind::Table) })
    );
    assert!(
        capabilities
            .create
            .iter()
            .any(|option| { option.object_type == CatalogObjectType::Catalog(CatalogKind::View) })
    );
    assert!(capabilities.edit.is_empty());
}

#[test]
fn sqlite_rebuild_plan_requires_lossless_ordered_steps() {
    let plan = CatalogRebuildPlan {
        steps: vec![
            CatalogRebuildStep::CreateReplacement,
            CatalogRebuildStep::CopyRows {
                column_mapping: vec![("old".to_owned(), "new".to_owned())],
            },
            CatalogRebuildStep::DropOriginal,
            CatalogRebuildStep::RenameReplacement,
            CatalogRebuildStep::RestoreIndexes,
            CatalogRebuildStep::RestoreTriggers,
            CatalogRebuildStep::Validate,
        ],
        preserves_data: true,
        preserves_indexes: true,
        preserves_triggers: true,
    };
    assert!(plan.validate().is_ok());
}

#[test]
fn oracle_create_plans_quote_names_and_target_the_selected_group() {
    let profile_id = Uuid::from_u128(7);
    let schema = CatalogId::new(profile_id, CatalogKind::Schema, ["SERVICE", "APP"]);
    let connection = lazydb::identity::ConnectionIdentity {
        profile_id,
        generation: 1,
    };
    let request = lazydb::db::catalog_mutation::CatalogMutationRequest {
        connection,
        request_id: 1,
        catalog_epoch: 1,
        mode: CatalogMutationMode::Create,
        anchor: CatalogMutationAnchor::Group {
            schema: schema.clone(),
            group: lazydb::db::catalog::ObjectGroup::Tables,
        },
        object_type: CatalogObjectType::Catalog(CatalogKind::Table),
        current_database: Some("SERVICE".to_owned()),
    };
    let mut table = TableDraft::new("APP");
    table.name.set("Order\"Items");
    table.columns[0].name.set("id");
    table.columns[0].native_type.set("NUMBER");
    let plan = OracleAdapter::plan_catalog_mutation(request, CatalogDraft::Table(table), None)
        .expect("Oracle table plan should be valid");
    assert_eq!(
        plan.statements()[0],
        "CREATE TABLE \"APP\".\"Order\"\"Items\" (\"id\" NUMBER)"
    );
    assert_eq!(plan.step_count(), 1);
}
