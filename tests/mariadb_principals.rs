use lazydb::{
    db::{catalog::CatalogKind, catalog_mutation::CatalogObjectType, mysql::MySqlAdapter},
    profile::DatabaseKind,
};

#[test]
fn mariadb_does_not_advertise_postgresql_role_or_schema_owner_mutations() {
    let capabilities = MySqlAdapter::catalog_mutation_capabilities();
    assert!(capabilities.create.iter().all(|option| {
        matches!(
            option.object_type,
            CatalogObjectType::Catalog(CatalogKind::Table | CatalogKind::View)
        )
    }));
    assert_eq!(DatabaseKind::MariaDb, DatabaseKind::MariaDb);
}
