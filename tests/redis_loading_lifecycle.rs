use lazydb::{
    action::{Action, Command},
    app::App,
    db::redis::types::RedisTarget,
    model::{
        execution_target::ExecutionTarget, keyspace::KeyspaceStatus,
        redis_browser::RedisBrowserTab, tab::WorkspaceTab, workspace::ConnectionStatus,
    },
};
use uuid::Uuid;

fn connected_app(profile_id: Uuid, database: u32) -> App {
    let mut app = App::new(Vec::new());
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 7;
    app.connection.target = Some(ExecutionTarget {
        profile_id,
        database: database.to_string(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connected;
    app
}

#[test]
fn opening_existing_not_loaded_tab_dispatches_first_scan() {
    let profile_id = Uuid::from_u128(1);
    let mut app = connected_app(profile_id, 0);
    let tab = RedisBrowserTab::new(
        Uuid::from_u128(2),
        RedisTarget {
            profile_id,
            database: 0,
        },
    );
    app.tabs.push(WorkspaceTab::RedisBrowser(tab));
    app.active_tab = 2;

    let commands = app.update(Action::OpenRedisDatabase {
        profile_id,
        database: 0,
    });

    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, Command::ScanRedisKeys(_)))
            .count(),
        1
    );
}

#[test]
fn reopening_complete_empty_tab_does_not_rescan() {
    let profile_id = Uuid::from_u128(3);
    let mut app = connected_app(profile_id, 0);
    let mut tab = RedisBrowserTab::new(
        Uuid::from_u128(4),
        RedisTarget {
            profile_id,
            database: 0,
        },
    );
    tab.keyspace.position = lazydb::db::redis::types::ScanPosition::Complete;
    tab.keyspace.status = KeyspaceStatus::CompleteEmpty;
    app.tabs.push(WorkspaceTab::RedisBrowser(tab));
    app.active_tab = 2;

    let commands = app.update(Action::OpenRedisDatabase {
        profile_id,
        database: 0,
    });

    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, Command::ScanRedisKeys(_)))
    );
}

#[test]
fn refresh_failure_preserves_existing_keys_as_stale_snapshot() {
    let profile_id = Uuid::from_u128(5);
    let mut tab = RedisBrowserTab::new(
        Uuid::from_u128(6),
        RedisTarget {
            profile_id,
            database: 0,
        },
    );
    tab.keyspace
        .keys
        .push(lazydb::db::redis::types::RedisKeyId {
            target: tab.target.clone(),
            key: b"kept".to_vec(),
        });
    tab.keyspace.status = KeyspaceStatus::Complete;
    tab.keyspace.position = lazydb::db::redis::types::ScanPosition::Complete;
    tab.keyspace.refresh();
    assert_eq!(tab.keyspace.status, KeyspaceStatus::Stale);
    assert_eq!(tab.keyspace.keys.len(), 1);
    let identity = tab
        .keyspace
        .start_scan(lazydb::identity::ConnectionIdentity {
            profile_id,
            generation: 9,
        })
        .unwrap();
    assert!(tab.keyspace.fail(&identity, "NOPERM"));
    assert_eq!(tab.keyspace.keys[0].key, b"kept");
    assert!(matches!(tab.keyspace.status, KeyspaceStatus::Failed(_)));
}

#[test]
fn scan_budget_pause_is_not_resumable_with_the_same_cursor() {
    let mut tab = RedisBrowserTab::new(
        Uuid::from_u128(7),
        RedisTarget {
            profile_id: Uuid::from_u128(7),
            database: 0,
        },
    );
    tab.keyspace.status = KeyspaceStatus::Paused {
        loaded: tab.keyspace.keys.len(),
    };
    assert!(matches!(tab.keyspace.status, KeyspaceStatus::Paused { .. }));
    assert!(
        tab.keyspace
            .start_scan(lazydb::identity::ConnectionIdentity {
                profile_id: Uuid::from_u128(7),
                generation: 1,
            })
            .is_none()
    );
}

#[test]
fn opening_a_browser_for_another_db_creates_tab_and_connect_intent_together() {
    let profile = lazydb::profile::import_connection_url("redis://localhost/1", Some("cache"))
        .unwrap()
        .profile;
    let profile_id = profile.id;
    let mut app = App::new(vec![profile]);
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 3;
    app.connection.target = Some(ExecutionTarget {
        profile_id,
        database: "1".into(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connected;

    let commands = app.update(Action::OpenRedisDatabase {
        profile_id,
        database: 0,
    });
    assert!(
        app.tabs
            .iter()
            .any(|tab| matches!(tab, WorkspaceTab::RedisBrowser(tab) if tab.target.database == 0))
    );
    assert!(commands.iter().any(
        |command| matches!(command, Command::Connect { target, .. } if target.database == "0")
    ));
}

#[test]
fn next_tab_activates_an_unloaded_redis_browser() {
    let profile_id = Uuid::from_u128(8);
    let mut app = connected_app(profile_id, 0);
    app.tabs.push(WorkspaceTab::RedisBrowser(RedisBrowserTab::new(
        Uuid::from_u128(9),
        RedisTarget {
            profile_id,
            database: 0,
        },
    )));
    app.tabs.push(WorkspaceTab::RedisBrowser(RedisBrowserTab::new(
        Uuid::from_u128(10),
        RedisTarget {
            profile_id,
            database: 0,
        },
    )));
    app.active_tab = 0;

    let commands = app.update(Action::NextTab);

    assert!(commands.iter().any(|command| {
        matches!(command, Command::ScanRedisKeys(request) if request.identity.target.database == 0)
    }));
}

#[test]
fn connection_success_activates_a_restored_redis_browser() {
    let profile_id = Uuid::from_u128(11);
    let profile = lazydb::profile::import_connection_url("redis://localhost:6379", Some("restored"))
        .unwrap()
        .profile;
    let mut app = App::new(vec![lazydb::profile::ConnectionProfile {
        id: profile_id,
        ..profile
    }]);
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 7;
    app.connection.target = Some(ExecutionTarget {
        profile_id,
        database: "0".into(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connecting;
    app.connection.pending_profile_id = Some(profile_id);
    app.connection.pending_generation = Some(7);
    app.connection.pending_target = app.connection.target.clone();
    app.active_workspace_profile = Some(profile_id);
    app.tabs.push(WorkspaceTab::RedisBrowser(RedisBrowserTab::new(
        Uuid::from_u128(12),
        RedisTarget {
            profile_id,
            database: 0,
        },
    )));
    app.active_tab = app.tabs.len() - 1;

    let commands = app.update(Action::ConnectionSucceeded {
        profile_id,
        generation: 7,
        server: lazydb::db::ServerInfo {
            kind: lazydb::profile::DatabaseKind::Redis,
            version: "7.2".into(),
            database: "0".into(),
            current_user: None,
        },
        mutation_capabilities: Default::default(),
    });

    assert!(commands
        .iter()
        .any(|command| matches!(command, Command::ScanRedisKeys(_))));
}

#[test]
fn activating_an_unloaded_redis_browser_for_another_database_requests_connection_first() {
    let profile = lazydb::profile::import_connection_url("redis://localhost:6379", Some("target"))
        .unwrap()
        .profile;
    let profile_id = profile.id;
    let mut app = App::new(vec![profile]);
    app.connection.profile_id = Some(profile_id);
    app.connection.generation = 7;
    app.connection.target = Some(ExecutionTarget {
        profile_id,
        database: "0".into(),
        schema: None,
    });
    app.connection.status = ConnectionStatus::Connected;
    app.tabs.push(WorkspaceTab::RedisBrowser(RedisBrowserTab::new(
        Uuid::from_u128(13),
        RedisTarget {
            profile_id,
            database: 1,
        },
    )));
    app.active_tab = 0;

    let commands = app.update(Action::ActivateTab(0));

    assert!(commands.iter().any(|command| {
        matches!(command, Command::Connect { target, .. } if target.database == "1")
    }));
    assert!(!commands
        .iter()
        .any(|command| matches!(command, Command::ScanRedisKeys(_))));
}

#[test]
fn activating_an_unloaded_redis_browser_is_idempotent_while_loading() {
    let profile_id = Uuid::from_u128(14);
    let mut app = connected_app(profile_id, 0);
    app.tabs.push(WorkspaceTab::RedisBrowser(RedisBrowserTab::new(
        Uuid::from_u128(15),
        RedisTarget {
            profile_id,
            database: 0,
        },
    )));
    app.active_tab = app.tabs.len() - 1;

    let first = app.update(Action::ActivateTab(app.active_tab));
    assert_eq!(
        first
            .iter()
            .filter(|command| matches!(command, Command::ScanRedisKeys(_)))
            .count(),
        1
    );

    let second = app.update(Action::ActivateTab(app.active_tab));
    assert!(!second
        .iter()
        .any(|command| matches!(command, Command::ScanRedisKeys(_))));
}

#[test]
fn previous_tab_activates_an_unloaded_redis_browser() {
    let profile_id = Uuid::from_u128(16);
    let mut app = connected_app(profile_id, 0);
    app.tabs.push(WorkspaceTab::RedisBrowser(RedisBrowserTab::new(
        Uuid::from_u128(17),
        RedisTarget {
            profile_id,
            database: 0,
        },
    )));
    app.tabs.push(WorkspaceTab::RedisBrowser(RedisBrowserTab::new(
        Uuid::from_u128(18),
        RedisTarget {
            profile_id,
            database: 0,
        },
    )));
    app.active_tab = 2;

    let commands = app.update(Action::PreviousTab);

    assert!(commands
        .iter()
        .any(|command| matches!(command, Command::ScanRedisKeys(_))));
}
