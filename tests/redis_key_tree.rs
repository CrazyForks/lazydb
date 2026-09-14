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

#[test]
fn incremental_insert_preserves_state_and_matches_full_rebuild() {
    let mut incremental = KeyTreeState::default();
    incremental.rebuild(&[key(b"user"), key(b"user:1001")]);
    incremental
        .expanded
        .insert(KeyTreeNodeId::Prefix(b"user:".to_vec()));
    incremental.select(Some(KeyTreeNodeId::Key(b"user:1001".to_vec())));
    incremental.insert_keys(&[key(b"user:1002"), key(b"cache:1")]);

    let mut rebuilt = KeyTreeState::default();
    rebuilt.rebuild(&[
        key(b"user"),
        key(b"user:1001"),
        key(b"user:1002"),
        key(b"cache:1"),
    ]);
    assert_eq!(incremental.visible_rows(), rebuilt.visible_rows());
    assert_eq!(
        incremental.selected,
        Some(KeyTreeNodeId::Key(b"user:1001".to_vec()))
    );
    assert!(incremental.contains(&KeyTreeNodeId::Key(b"cache:1".to_vec())));
}

#[test]
fn collapsed_prefix_hides_its_own_key_and_descendants() {
    let mut tree = KeyTreeState::default();
    tree.rebuild(&[key(b"user:1"), key(b"user:2")]);

    let prefix = KeyTreeNodeId::Prefix(b"user:".to_vec());
    let first_key = KeyTreeNodeId::Key(b"user:1".to_vec());
    let second_key = KeyTreeNodeId::Key(b"user:2".to_vec());

    assert_eq!(tree.visible_ids(), vec![prefix.clone()]);
    assert_eq!(tree.parent_of(&first_key), Some(prefix.clone()));
    assert_eq!(tree.first_child(&prefix), Some(first_key.clone()));

    tree.expanded.insert(prefix.clone());
    assert_eq!(tree.visible_ids(), vec![prefix, first_key, second_key]);
}
