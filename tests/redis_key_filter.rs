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
    assert_eq!(
        tab.visible_ids(),
        vec![
            KeyTreeNodeId::Prefix(b"cache:".to_vec()),
            KeyTreeNodeId::Prefix(b"user:".to_vec())
        ]
    );
    tab.find.as_mut().unwrap().query.insert('1');
    assert!(tab.find.as_ref().unwrap().matches.is_empty());
    tab.confirm_find();
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
        tab.visible_rows()
            .iter()
            .any(|row| row.id == KeyTreeNodeId::Prefix(b"cache:".to_vec()))
    );
    assert!(
        tab.visible_rows()
            .iter()
            .any(|row| row.id == KeyTreeNodeId::Prefix(b"user:".to_vec()))
    );
}

#[test]
fn confirmed_results_can_be_collapsed_without_affecting_the_browsing_tree() {
    let mut tab = tab();
    tab.open_find();
    tab.find.as_mut().unwrap().query.insert('1');
    let original_expanded = tab.tree.expanded.clone();
    tab.confirm_find();

    let folder = KeyTreeNodeId::Prefix(b"cache:".to_vec());
    assert!(
        tab.visible_ids()
            .contains(&KeyTreeNodeId::Key(b"cache:1".to_vec()))
    );
    assert!(tab.toggle_prefix(&folder));
    assert!(
        !tab.visible_ids()
            .contains(&KeyTreeNodeId::Key(b"cache:1".to_vec()))
    );
    assert_eq!(tab.tree.expanded, original_expanded);
}

#[test]
fn editing_does_not_change_selection_or_expand_collapsed_prefixes() {
    let mut tab = tab();
    let folder = KeyTreeNodeId::Prefix(b"cache:".to_vec());
    tab.tree.select(Some(folder.clone()));
    let original_selected = tab.tree.selected.clone();
    let original_expanded = tab.tree.expanded.clone();
    tab.open_find();
    tab.find.as_mut().unwrap().query.insert('1');

    assert_eq!(tab.tree.selected, original_selected);
    assert_eq!(tab.tree.expanded, original_expanded);
    assert_eq!(
        tab.visible_ids(),
        vec![folder, KeyTreeNodeId::Prefix(b"user:".to_vec())]
    );
}
