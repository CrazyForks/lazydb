use lazydb::{
    db::redis::types::{RedisKeyId, RedisTarget},
    model::{redis_browser::RedisBrowserTab, redis_key_tree::KeyTreeNodeId},
};
use uuid::Uuid;

fn tab() -> RedisBrowserTab {
    let target = RedisTarget {
        profile_id: Uuid::nil(),
        database: 0,
    };
    let mut tab = RedisBrowserTab::new(Uuid::nil(), target.clone());
    tab.keyspace.keys = [b"user:1".as_slice(), b"cache:1".as_slice()]
        .into_iter()
        .map(|key| RedisKeyId {
            target: target.clone(),
            key: key.to_vec(),
        })
        .collect();
    tab.rebuild_tree();
    tab
}

#[test]
fn filter_matches_loaded_keys_inside_collapsed_folders_and_keeps_ancestors() {
    let mut tab = tab();
    tab.open_find();
    tab.find.as_mut().unwrap().query.insert('1');
    tab.update_find();

    let find = tab.find.as_ref().unwrap();
    assert_eq!(
        find.matches,
        vec![
            KeyTreeNodeId::Key(b"cache:1".to_vec()),
            KeyTreeNodeId::Key(b"user:1".to_vec())
        ]
    );
    assert!(
        find.filtered_rows
            .iter()
            .any(|row| row.id == KeyTreeNodeId::Prefix(b"cache:".to_vec()))
    );
    assert!(
        find.filtered_rows
            .iter()
            .any(|row| row.id == KeyTreeNodeId::Prefix(b"user:".to_vec()))
    );
}
