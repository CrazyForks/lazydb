use lazydb::{db::capabilities::DatabaseCapabilities, profile::DatabaseKind};

#[test]
fn static_capabilities_match_the_current_adapter_contract() {
    assert_eq!(
        DatabaseCapabilities::for_kind(DatabaseKind::Postgres),
        DatabaseCapabilities {
            catalog: true,
            relation_ddl: true,
            relation_edit: true,
            monitoring: true,
            manual_transactions: true,
            cancellation: true,
        }
    );
    assert!(!DatabaseCapabilities::for_kind(DatabaseKind::MySql).relation_edit);
    assert!(!DatabaseCapabilities::for_kind(DatabaseKind::SqlServer).monitoring);
    assert!(!DatabaseCapabilities::for_kind(DatabaseKind::Sqlite).monitoring);
}

#[test]
fn capability_fields_are_conservative_for_features_not_implemented_by_adapters() {
    for kind in [
        DatabaseKind::MySql,
        DatabaseKind::SqlServer,
        DatabaseKind::Sqlite,
    ] {
        let capabilities = DatabaseCapabilities::for_kind(kind);
        assert!(capabilities.catalog);
        assert!(capabilities.relation_ddl);
        assert!(!capabilities.relation_edit);
    }
}
