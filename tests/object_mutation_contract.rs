use lazydb::db::catalog::{CatalogId, CatalogKind, NamespaceModel};
use lazydb::db::catalog_change_set::{CatalogFieldChanges, FieldChange};
use lazydb::db::catalog_mutation::{
    CatalogMutationAnchor, CatalogMutationAvailability, CatalogMutationCapabilities,
    CatalogMutationMode, CatalogMutationOption, CatalogObjectType,
};
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
