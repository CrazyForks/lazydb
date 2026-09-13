use lazydb::db::redis::discovery::{
    RedisDatabaseDiscovery, RedisDatabaseInfo, RedisDiscoveryCompleteness,
};

#[test]
fn discovery_model_preserves_partial_state_and_database_order() {
    let discovery = RedisDatabaseDiscovery {
        databases: vec![
            RedisDatabaseInfo {
                database: 0,
                keys: Some(3),
                expires: Some(1),
            },
            RedisDatabaseInfo {
                database: 2,
                keys: None,
                expires: None,
            },
        ],
        completeness: RedisDiscoveryCompleteness::Partial,
        warnings: vec!["CONFIG unavailable".into()],
    };
    assert_eq!(discovery.databases[0].database, 0);
    assert_eq!(discovery.databases[1].database, 2);
    assert_eq!(discovery.completeness, RedisDiscoveryCompleteness::Partial);
    assert_eq!(discovery.warnings.len(), 1);
}
