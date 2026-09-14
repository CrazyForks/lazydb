use lazydb::{
    action::{Action, Command},
    app::App,
    db::redis::types::{RedisKeyId, RedisTarget},
    model::execution_target::ExecutionTarget,
    model::redis_key_tree::KeyTreeNodeId,
    model::workspace::ConnectionStatus,
    model::{
        redis_browser::{RedisPreviewState, RedisValuePageState},
        tab::WorkspaceTab,
    },
    persistence::workspace::{PersistedTab, WorkspaceStore},
    profile::{DatabaseKind, import_connection_url},
};
use tempfile::TempDir;
use uuid::Uuid;

fn redis_profile(name: &str) -> lazydb::profile::ConnectionProfile {
    import_connection_url("redis://localhost:6379", Some(name))
        .unwrap()
        .profile
}

fn connect_redis(app: &mut App, profile_id: Uuid, database: &str) {
    let generation = match app.update(Action::RequestConnect(profile_id)).as_slice() {
        [Command::Connect { generation, .. }] => *generation,
        commands => panic!("unexpected commands: {commands:?}"),
    };
    app.update(Action::ConnectionSucceeded {
        profile_id,
        generation,
        server: lazydb::db::ServerInfo {
            kind: DatabaseKind::Redis,
            version: "7.2".into(),
            database: database.into(),
            current_user: None,
        },
        mutation_capabilities: Default::default(),
    });
}

#[test]
fn opening_a_database_creates_one_empty_redis_browser_tab() {
    let profile_id = Uuid::from_u128(1);
    let mut app = App::new(Vec::new());
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 1;
    app.connection.target = Some(ExecutionTarget {
        profile_id,
        database: "2".into(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connected;
    let commands = app.update(Action::OpenRedisDatabase {
        profile_id,
        database: 2,
    });
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, lazydb::action::Command::ScanRedisKeys(_)))
            .count(),
        1
    );
    assert_eq!(app.tabs.len(), 2);
    let first_id = match app.tabs.last().unwrap() {
        WorkspaceTab::RedisBrowser(tab) => {
            assert_eq!(
                tab.target,
                RedisTarget {
                    profile_id,
                    database: 2
                }
            );
            assert_eq!(tab.preview, RedisPreviewState::Empty);
            tab.id
        }
        _ => panic!("expected Redis browser tab"),
    };
    app.update(Action::OpenRedisDatabase {
        profile_id,
        database: 2,
    });
    assert_eq!(app.tabs.len(), 2);
    assert!(app.tabs.iter().any(|tab| tab.id() == first_id));
}

#[test]
fn opening_redis_on_second_profile_produces_a_valid_snapshot() {
    let first = redis_profile("first");
    let second = redis_profile("second");
    let first_id = first.id;
    let second_id = second.id;
    let mut app = App::new(vec![first, second]);

    connect_redis(&mut app, first_id, "0");
    let commands = app.update(Action::OpenRedisDatabase {
        profile_id: second_id,
        database: 0,
    });
    let generation = commands
        .iter()
        .find_map(|command| match command {
            Command::Connect { generation, .. } => Some(*generation),
            _ => None,
        })
        .expect("opening the second profile should request its connection");
    app.update(Action::ConnectionSucceeded {
        profile_id: second_id,
        generation,
        server: lazydb::db::ServerInfo {
            kind: DatabaseKind::Redis,
            version: "7.2".into(),
            database: "0".into(),
            current_user: None,
        },
        mutation_capabilities: Default::default(),
    });
    app.update(Action::OpenRedisDatabase {
        profile_id: second_id,
        database: 0,
    });

    let redis_tabs = app
        .tabs
        .iter()
        .filter_map(|tab| match tab {
            WorkspaceTab::RedisBrowser(tab) => Some(tab),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(redis_tabs.len(), 1);
    assert_eq!(redis_tabs[0].target.profile_id, second_id);

    let snapshot = app.workspace_snapshot();
    let tab_count = snapshot
        .profiles
        .iter()
        .flat_map(|profile| profile.tabs.iter())
        .filter(|tab| matches!(tab, PersistedTab::RedisBrowser { .. }))
        .count();
    assert_eq!(tab_count, 1);

    let temp = TempDir::new().unwrap();
    let store = WorkspaceStore::new(temp.path().join("workspace.toml"), temp.path().join("sql"));
    store.save(&snapshot).unwrap();
}

#[test]
fn reopening_an_existing_not_loaded_tab_dispatches_its_first_scan() {
    let profile_id = Uuid::from_u128(101);
    let mut app = App::new(Vec::new());
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 3;
    app.connection.target = Some(ExecutionTarget {
        profile_id,
        database: "0".into(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connected;
    let tab = lazydb::model::redis_browser::RedisBrowserTab::new(
        Uuid::from_u128(102),
        RedisTarget {
            profile_id,
            database: 0,
        },
    );
    app.tabs.push(WorkspaceTab::RedisBrowser(tab));
    app.active_tab = app.tabs.len() - 1;
    let commands = app.update(Action::OpenRedisDatabase {
        profile_id,
        database: 0,
    });
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, lazydb::action::Command::ScanRedisKeys(_)))
    );
}

#[test]
fn reopening_a_complete_empty_tab_does_not_repeat_scan() {
    let profile_id = Uuid::from_u128(121);
    let mut app = App::new(Vec::new());
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 1;
    app.connection.target = Some(ExecutionTarget {
        profile_id,
        database: "0".into(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connected;
    let mut tab = lazydb::model::redis_browser::RedisBrowserTab::new(
        Uuid::from_u128(122),
        RedisTarget {
            profile_id,
            database: 0,
        },
    );
    tab.keyspace.position = lazydb::db::redis::types::ScanPosition::Complete;
    tab.keyspace.status = lazydb::model::keyspace::KeyspaceStatus::CompleteEmpty;
    app.tabs.push(WorkspaceTab::RedisBrowser(tab));
    app.active_tab = app.tabs.len() - 1;
    let commands = app.update(Action::OpenRedisDatabase {
        profile_id,
        database: 0,
    });
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, lazydb::action::Command::ScanRedisKeys(_)))
    );
}

#[test]
fn keyspace_status_distinguishes_initial_empty_partial_and_failure() {
    use lazydb::model::keyspace::{KeyspaceState, KeyspaceStatus};
    let target = RedisTarget {
        profile_id: Uuid::nil(),
        database: 0,
    };
    let mut state = KeyspaceState::new(Uuid::nil(), target, b"*".to_vec());
    assert_eq!(state.status, KeyspaceStatus::NotLoaded);
    state.status = KeyspaceStatus::Partial;
    assert_ne!(state.status, KeyspaceStatus::CompleteEmpty);
    state.status = KeyspaceStatus::Failed("NOPERM".into());
    assert!(matches!(state.status, KeyspaceStatus::Failed(_)));
}

#[test]
fn selecting_a_key_creates_a_preview_command_but_prefix_selection_stays_empty() {
    let mut app = App::new(Vec::new());
    app.connection.profile_id = Some(Uuid::from_u128(1));
    app.connection.generation = 1;
    app.connection.target = Some(ExecutionTarget {
        profile_id: Uuid::from_u128(1),
        database: "2".into(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connected;
    app.update(Action::OpenRedisDatabase {
        profile_id: Uuid::from_u128(1),
        database: 2,
    });
    let tab_id = app.tabs.last().unwrap().id();
    if let Some(WorkspaceTab::RedisBrowser(tab)) = app.tabs.last_mut() {
        tab.tree.rebuild(&[RedisKeyId {
            target: tab.target.clone(),
            key: b"user:1".to_vec(),
        }]);
    }
    let commands = app.select_redis_key(tab_id, Some(KeyTreeNodeId::Key(b"user:1".to_vec())));
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        commands[0],
        lazydb::action::Command::LoadRedisValuePreview { .. }
    ));
    assert!(
        app.select_redis_key(tab_id, Some(KeyTreeNodeId::Prefix(b"user:".to_vec())))
            .is_empty()
    );
    assert!(
        matches!(app.tabs.last().unwrap(), WorkspaceTab::RedisBrowser(tab) if tab.preview == RedisPreviewState::Empty)
    );
}

#[test]
fn selecting_keys_advances_preview_generation_so_old_results_can_be_rejected() {
    let mut tab = lazydb::model::redis_browser::RedisBrowserTab::new(
        Uuid::from_u128(1),
        RedisTarget {
            profile_id: Uuid::from_u128(2),
            database: 2,
        },
    );
    tab.select(Some(KeyTreeNodeId::Key(b"a".to_vec())));
    let first = tab.preview_generation;
    tab.select(Some(KeyTreeNodeId::Key(b"b".to_vec())));
    assert!(first < tab.preview_generation);
}

#[test]
fn selecting_a_key_starts_a_typed_value_page_lifecycle() {
    let mut tab = lazydb::model::redis_browser::RedisBrowserTab::new(
        Uuid::from_u128(1),
        RedisTarget {
            profile_id: Uuid::from_u128(2),
            database: 0,
        },
    );
    tab.tree.rebuild(&[RedisKeyId {
        target: tab.target.clone(),
        key: b"user:1".to_vec(),
    }]);
    tab.select(Some(KeyTreeNodeId::Key(b"user:1".to_vec())));
    assert!(matches!(
        tab.value_page,
        RedisValuePageState::Loading { .. }
    ));
    tab.select(None);
    assert_eq!(tab.value_page, RedisValuePageState::Empty);
}

#[test]
fn redis_browser_focus_cycles_explorer_keys_preview() {
    let mut app = App::new(Vec::new());
    app.tabs.push(WorkspaceTab::RedisBrowser(
        lazydb::model::redis_browser::RedisBrowserTab::new(
            Uuid::from_u128(30),
            RedisTarget {
                profile_id: Uuid::from_u128(31),
                database: 0,
            },
        ),
    ));
    app.active_tab = app.tabs.len() - 1;
    app.focus = lazydb::model::workspace::Focus::Results;
    app.update(Action::FocusNext);
    assert!(
        matches!(&app.tabs[app.active_tab], WorkspaceTab::RedisBrowser(tab) if tab.focus == lazydb::model::redis_browser::RedisBrowserFocus::Preview)
    );
    app.update(Action::FocusNext);
    assert_eq!(app.focus, lazydb::model::workspace::Focus::Explorer);
    app.update(Action::FocusPrevious);
    assert!(
        matches!(&app.tabs[app.active_tab], WorkspaceTab::RedisBrowser(tab) if tab.focus == lazydb::model::redis_browser::RedisBrowserFocus::Preview)
    );
    app.update(Action::FocusPrevious);
    assert!(
        matches!(&app.tabs[app.active_tab], WorkspaceTab::RedisBrowser(tab) if tab.focus == lazydb::model::redis_browser::RedisBrowserFocus::Keys)
    );
}

#[test]
fn redis_browser_find_is_tab_local_and_edits_without_reading_a_key() {
    let mut tab = lazydb::model::redis_browser::RedisBrowserTab::new(
        Uuid::from_u128(5),
        RedisTarget {
            profile_id: Uuid::from_u128(6),
            database: 1,
        },
    );
    tab.keyspace.keys.push(RedisKeyId {
        target: tab.target.clone(),
        key: b"user:1".to_vec(),
    });
    tab.keyspace.keys.push(RedisKeyId {
        target: tab.target.clone(),
        key: b"session:2".to_vec(),
    });
    tab.rebuild_tree();
    tab.tree
        .expanded
        .insert(KeyTreeNodeId::Prefix(b"user:".to_vec()));
    tab.open_find();
    assert_eq!(tab.find.as_ref().unwrap().rows.len(), 3);
    tab.find.as_mut().unwrap().query.insert('1');
    tab.update_find();
    assert!(tab.find.as_ref().unwrap().matches.is_empty());
    tab.confirm_find();
    assert!(
        tab.find
            .as_ref()
            .unwrap()
            .matches
            .iter()
            .any(|id| *id == KeyTreeNodeId::Key(b"user:1".to_vec()))
    );
    assert_eq!(tab.preview, RedisPreviewState::Empty);
}
