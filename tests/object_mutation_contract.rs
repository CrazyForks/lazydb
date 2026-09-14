use lazydb::db::catalog_mutation::{
    CatalogMutationAvailability, CatalogMutationCapabilities, CatalogMutationOption,
    CatalogObjectType,
};
use lazydb::db::catalog::CatalogKind;

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
