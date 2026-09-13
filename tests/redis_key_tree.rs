use lazydb::{
    db::redis::types::{RedisKeyId, RedisTarget},
    model::redis_key_tree::{KeyTreeNodeId, KeyTreeState},
};
use uuid::Uuid;

fn key(value: &[u8]) -> RedisKeyId {
    RedisKeyId {
        target: RedisTarget {
            profile_id: Uuid::nil(),
            database: 0,
        },
        key: value.to_vec(),
    }
}

#[test]
fn tree_keeps_prefixes_and_full_key_identities() {
    let mut tree = KeyTreeState::default();
    tree.rebuild(&[key(b"user:1001"), key(b"user:1002"), key(b"user")]);
    assert!(tree.contains(&KeyTreeNodeId::Prefix(b"user:".to_vec())));
    assert!(tree.contains(&KeyTreeNodeId::Key(b"user".to_vec())));
    assert!(tree.contains(&KeyTreeNodeId::Key(b"user:1001".to_vec())));
}

#[test]
fn prefix_selection_does_not_create_a_preview() {
    let mut tree = KeyTreeState::default();
    tree.rebuild(&[key(b"a:b")]);
    tree.select(Some(KeyTreeNodeId::Prefix(b"a:".to_vec())));
    assert!(tree.selected_key().is_none());
}
