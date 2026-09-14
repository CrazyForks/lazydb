use lazydb::{
    action::{Action, Command},
    app::App,
    db::redis::types::{RedisKeyId, RedisTarget},
    model::keyspace::KeyspaceState,
};
use uuid::Uuid;

#[test]
fn removing_loaded_key_updates_keyspace_cache_and_store() {
    let target = RedisTarget {
        profile_id: Uuid::nil(),
        database: 0,
    };
    let mut state = KeyspaceState::new(Uuid::nil(), target.clone(), b"*".to_vec());
    state.keys = vec![RedisKeyId {
        target,
        key: b"user:1".to_vec(),
    }];
    state.status = lazydb::model::keyspace::KeyspaceStatus::Complete;
    assert!(state.remove_key(b"user:1"));
    assert!(state.keys.is_empty());
    assert_eq!(state.stored_count(), 0);
    assert_eq!(state.stored_bytes(), 0);
    assert!(!state.remove_key(b"user:1"));
}

#[test]
fn deleted_key_is_not_reinserted_by_a_late_scan_batch_until_refresh() {
    use lazydb::db::redis::types::{KeyScanBatch, RedisRequestIdentity, ScanPosition};
    use lazydb::identity::ConnectionIdentity;

    let target = RedisTarget {
        profile_id: Uuid::nil(),
        database: 0,
    };
    let mut state = KeyspaceState::new(Uuid::nil(), target.clone(), b"*".to_vec());
    let connection = ConnectionIdentity {
        profile_id: Uuid::nil(),
        generation: 1,
    };
    let identity = RedisRequestIdentity {
        connection,
        target: target.clone(),
        owner_id: Uuid::nil(),
        generation: 0,
        request_id: 1,
    };
    assert!(state.start_scan(connection).is_some());
    assert!(!state.remove_key(b"user:1"));
    assert!(state.apply_batch(KeyScanBatch {
        identity,
        keys: vec![b"user:1".to_vec(), b"user:2".to_vec()],
        next: ScanPosition::Complete,
    }));
    assert_eq!(
        state
            .keys
            .iter()
            .map(|key| key.key.as_slice())
            .collect::<Vec<_>>(),
        vec![b"user:2"]
    );
    state.refresh();
    assert!(state.start_scan(connection).is_some());
    let identity = state.identity().unwrap();
    assert!(state.apply_batch(KeyScanBatch {
        identity,
        keys: vec![b"user:1".to_vec()],
        next: ScanPosition::Complete,
    }));
    assert_eq!(state.keys[0].key, b"user:1");
}

#[test]
fn deleting_selected_key_creates_targeted_del_command() {
    let profile_id = Uuid::from_u128(10);
    let mut app = App::new(Vec::new());
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 1;
    app.connection.status = lazydb::model::workspace::ConnectionStatus::Connected;
    app.connection.target = Some(lazydb::model::execution_target::ExecutionTarget {
        profile_id,
        database: "0".into(),
        schema: None,
    });
    app.tabs
        .push(lazydb::model::tab::WorkspaceTab::RedisBrowser(
            lazydb::model::redis_browser::RedisBrowserTab::new(
                Uuid::from_u128(11),
                RedisTarget {
                    profile_id,
                    database: 0,
                },
            ),
        ));
    app.active_tab = app.tabs.len() - 1;
    app.focus = lazydb::model::workspace::Focus::Results;
    if let lazydb::model::tab::WorkspaceTab::RedisBrowser(tab) = app.tabs.last_mut().unwrap() {
        tab.tree.rebuild(&[RedisKeyId {
            target: tab.target.clone(),
            key: b"user:1".to_vec(),
        }]);
        tab.tree
            .select(Some(lazydb::model::redis_key_tree::KeyTreeNodeId::Key(
                b"user:1".to_vec(),
            )));
    }
    let commands = app.update(Action::RedisDeleteKey);
    assert!(matches!(
        commands.as_slice(),
        [Command::DeleteRedisKey { key, .. }] if key.key == b"user:1"
    ));
}
