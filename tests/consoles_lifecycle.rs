use lazydb::{
    action::{Action, Command},
    app::App,
    model::{execution_target::ExecutionTarget, workspace::Overlay},
    persistence::workspace::{PersistedConsole, PersistedProfileWorkspace, WorkspaceSnapshot},
    profile::import_connection_url,
};
use uuid::Uuid;

fn server() -> lazydb::db::ServerInfo {
    lazydb::db::ServerInfo {
        kind: lazydb::profile::DatabaseKind::Sqlite,
        version: "test".into(),
        database: ":memory:".into(),
        current_user: None,
    }
}

#[test]
fn connecting_a_profile_without_saved_consoles_keeps_workspace_empty() {
    let profile = import_connection_url(":memory:", Some("connected"))
        .unwrap()
        .profile;
    let profile_id = profile.id;
    let mut app = App::new(vec![profile]);
    let generation = match app.update(Action::RequestConnect(profile_id)).as_slice() {
        [Command::Connect { generation, .. }] => *generation,
        commands => panic!("unexpected commands: {commands:?}"),
    };

    app.update(Action::ConnectionSucceeded {
        profile_id,
        generation,
        server: server(),
        mutation_capabilities: Default::default(),
    });

    assert!(app.tabs.is_empty());
    assert!(app.sql_editors.is_empty());
}

fn console(id: Uuid, name: &str, target: ExecutionTarget) -> PersistedConsole {
    PersistedConsole {
        id,
        name: name.into(),
        sql_file: format!("{id}.sql").into(),
        target: Some(target),
        transaction_mode: Default::default(),
        open: false,
    }
}

#[test]
fn profile_workspace_documents_are_kept_when_no_connection_is_active() {
    let profile = import_connection_url(":memory:", Some("saved"))
        .unwrap()
        .profile;
    let id = Uuid::new_v4();
    let snapshot = WorkspaceSnapshot {
        active_profile: Some(profile.id),
        profiles: vec![PersistedProfileWorkspace {
            profile_id: profile.id,
            active_tab: None,
            consoles: vec![console(
                id,
                "saved console",
                ExecutionTarget::from_profile(&profile),
            )],
            tabs: Vec::new(),
        }],
        sql: vec![(id, "select saved".into())],
        active_console: Uuid::nil(),
        consoles: Vec::new(),
        tabs: Vec::new(),
        recent_targets: Vec::new(),
    };
    let mut app = App::new(vec![profile]);

    app.restore_workspace(snapshot, None);

    assert_eq!(app.sql_editors.len(), 1);
    assert_eq!(app.editor_text(id).unwrap(), "select saved");
    assert!(app.tabs.is_empty());
    assert!(app.connection.active_identity().is_none());
}

#[test]
fn cold_restore_exposes_all_documents_without_connecting() {
    let first = import_connection_url(":memory:", Some("first"))
        .unwrap()
        .profile;
    let second = import_connection_url(":memory:", Some("second"))
        .unwrap()
        .profile;
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let snapshot = WorkspaceSnapshot {
        active_profile: None,
        profiles: Vec::new(),
        sql: vec![
            (first_id, "select first".into()),
            (second_id, "select second".into()),
        ],
        active_console: Uuid::nil(),
        consoles: vec![
            console(
                first_id,
                "first console",
                ExecutionTarget::from_profile(&first),
            ),
            console(
                second_id,
                "second console",
                ExecutionTarget::from_profile(&second),
            ),
        ],
        tabs: Vec::new(),
        recent_targets: Vec::new(),
    };
    let mut app = App::new(vec![first, second]);

    app.restore_workspace(snapshot, None);

    assert_eq!(app.sql_editors.len(), 2);
    assert_eq!(app.editor_text(first_id).unwrap(), "select first");
    assert_eq!(app.editor_text(second_id).unwrap(), "select second");
    assert!(app.tabs.is_empty());
    assert!(app.connection.active_identity().is_none());

    let commands = app.update(Action::OpenSqlEditorList);

    assert!(
        commands
            .iter()
            .all(|command| !matches!(command, Command::Connect { .. }))
    );
    assert!(matches!(app.overlay, Some(Overlay::SqlEditorList(_))));
}

#[test]
fn default_console_name_uses_maximum_across_closed_documents() {
    let first = import_connection_url(":memory:", Some("first"))
        .unwrap()
        .profile;
    let second = import_connection_url(":memory:", Some("second"))
        .unwrap()
        .profile;
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let snapshot = WorkspaceSnapshot {
        active_profile: None,
        profiles: Vec::new(),
        sql: vec![(first_id, String::new()), (second_id, String::new())],
        active_console: Uuid::nil(),
        consoles: vec![
            console(first_id, "console_2", ExecutionTarget::from_profile(&first)),
            console(
                second_id,
                "console_9",
                ExecutionTarget::from_profile(&second),
            ),
        ],
        tabs: Vec::new(),
        recent_targets: Vec::new(),
    };
    let mut app = App::new(vec![first, second]);
    app.restore_workspace(snapshot, None);

    app.update(Action::NewConsoleNamed("console_10".into()));
    assert!(
        app.sql_editors
            .iter()
            .any(|record| record.name == "console_10")
    );
}

#[test]
fn activating_an_offline_saved_console_starts_its_connection() {
    let profile = import_connection_url(":memory:", Some("saved"))
        .unwrap()
        .profile;
    let id = Uuid::new_v4();
    let target = ExecutionTarget::from_profile(&profile);
    let snapshot = WorkspaceSnapshot {
        active_profile: None,
        profiles: Vec::new(),
        sql: vec![(id, "select saved".into())],
        active_console: Uuid::nil(),
        consoles: vec![console(id, "saved console", target.clone())],
        tabs: Vec::new(),
        recent_targets: Vec::new(),
    };
    let mut app = App::new(vec![profile]);
    app.restore_workspace(snapshot, None);

    let commands = app.update(Action::ActivateSqlEditor(id));

    assert!(app.tabs.iter().any(|tab| tab.id() == id));
    assert!(commands.iter().any(|command| {
        matches!(command, Command::Connect { target: requested, .. } if requested == &target)
    }));
}

#[test]
fn activating_two_consoles_on_one_offline_target_is_single_flight() {
    let profile = import_connection_url(":memory:", Some("shared"))
        .unwrap()
        .profile;
    let target = ExecutionTarget::from_profile(&profile);
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let snapshot = WorkspaceSnapshot {
        active_profile: None,
        profiles: Vec::new(),
        sql: vec![(first_id, String::new()), (second_id, String::new())],
        active_console: Uuid::nil(),
        consoles: vec![
            console(first_id, "first", target.clone()),
            console(second_id, "second", target.clone()),
        ],
        tabs: Vec::new(),
        recent_targets: Vec::new(),
    };
    let mut app = App::new(vec![profile]);
    app.restore_workspace(snapshot, None);

    let first_commands = app.update(Action::ActivateSqlEditor(first_id));
    let second_commands = app.update(Action::ActivateSqlEditor(second_id));

    assert_eq!(
        first_commands
            .iter()
            .filter(|command| matches!(command, Command::Connect { .. }))
            .count(),
        1
    );
    assert!(
        second_commands
            .iter()
            .all(|command| !matches!(command, Command::Connect { .. }))
    );
}
