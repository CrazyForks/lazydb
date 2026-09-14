use lazydb::db::catalog::{CatalogId, CatalogKind, NamespaceModel};
use lazydb::db::catalog_change_set::{CatalogFieldChanges, FieldChange};
use lazydb::db::catalog_mutation::{
    CatalogMutationAnchor, CatalogMutationAvailability, CatalogMutationCapabilities,
    CatalogMutationMode, CatalogMutationOption, CatalogObjectType, MutationCompletion,
    MutationProgress,
};
use lazydb::db::oracle::OracleAdapter;
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
    assert!(capabilities.edit.is_empty());
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
