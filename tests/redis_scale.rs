use lazydb::db::redis::{
    key_store::{KeyStore, MemoryKeyStore},
    types::RedisTarget,
};

#[test]
#[ignore = "explicit scale benchmark"]
fn memory_store_indexes_one_million_binary_keys() {
    let mut store = MemoryKeyStore::new(RedisTarget {
        profile_id: uuid::Uuid::nil(),
        database: 0,
    });
    let keys = (0..1_000_000_u32)
        .map(|index| format!("user:{index}").into_bytes())
        .collect::<Vec<_>>();
    assert_eq!(store.insert_batch(&keys), keys.len());
    assert_eq!(store.count(), keys.len());
    assert!(store.bytes() > keys.len());
    assert_eq!(store.page_after(None, 100).keys.len(), 100);
}
