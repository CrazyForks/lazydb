use lazydb::db::catalog::{CatalogId, CatalogKind, NamespaceModel};
use lazydb::db::catalog_mutation::{
    CatalogMutationAnchor, CatalogMutationAvailability, CatalogMutationCapabilities,
    CatalogMutationOption, CatalogObjectType,
};
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
