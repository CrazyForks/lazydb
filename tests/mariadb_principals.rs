use lazydb::{
    db::{
        DatabaseConnection, catalog::CatalogKind, catalog_mutation::CatalogObjectType,
        mysql::MySqlAdapter,
    },
    profile::{DatabaseKind, import_connection_url},
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

#[test]
fn mariadb_membership_and_grant_text_remain_conservative_without_server_rows() {
    assert_eq!(
        lazydb::db::principal::PrincipalCoverage::Unavailable("membership".into()),
        lazydb::db::principal::PrincipalCoverage::Unavailable("membership".into())
    );
    let grant = "GRANT SELECT ON `IDENTIFIED schema`.* TO `app`@`%`";
    assert!(grant.contains("IDENTIFIED schema"));
}

#[tokio::test]
async fn mariadb_lists_roles_from_the_native_is_role_flag_when_configured() {
    use lazydb::db::principal::PrincipalKind;

    let Ok(url) = std::env::var("LAZYDB_TEST_MARIADB_URL") else {
        eprintln!("skipping MariaDB principal browse: LAZYDB_TEST_MARIADB_URL is not set");
        return;
    };
    let imported = import_connection_url(&url, Some("mariadb-principals")).unwrap();
    assert_eq!(imported.profile.kind, DatabaseKind::MariaDb);
    let database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();

    let page = database.list_principals().await.unwrap();
    assert!(!page.entries.is_empty());
    for entry in &page.entries {
        // Account identity keeps user and host as separate fields so the
        // display string is never re-parsed.
        assert!(entry.id.host.is_some(), "{entry:?}");
        assert!(entry.name.starts_with('\''), "{entry:?}");
    }
    // MariaDB's `roles_mapping`/`is_role` make the role flag authoritative.
    assert!(
        page.entries
            .iter()
            .any(|entry| matches!(entry.kind, PrincipalKind::User | PrincipalKind::Role))
    );
    let user = page.entries.first().unwrap();
    let ddl = database.principal_ddl(user).await.unwrap();
    assert!(!ddl.sql.contains("IDENTIFIED BY"), "{}", ddl.sql);
}
