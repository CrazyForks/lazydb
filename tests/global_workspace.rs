use lazydb::{
    action::{Action, Command},
    app::App,
    db::ServerInfo,
    persistence::workspace::WorkspaceStore,
    profile::{DatabaseKind, import_connection_url},
};
use tempfile::TempDir;

fn memory_profile(name: &str) -> lazydb::profile::ConnectionProfile {
    import_connection_url(":memory:", Some(name))
        .unwrap()
        .profile
}

fn server(database: &str) -> ServerInfo {
    ServerInfo {
        kind: DatabaseKind::Sqlite,
        version: "3.50".into(),
        database: database.into(),
        current_user: None,
    }
}

fn connect(app: &mut App, profile_id: uuid::Uuid, database: &str) {
    if app.active_console_opt().is_none() {
        app.update(Action::NewConsole);
    }
    let generation = match app
        .update(Action::RequestProfileConnect { profile_id })
        .as_slice()
    {
        [Command::Connect { generation, .. }] => *generation,
        commands => panic!("unexpected commands: {commands:?}"),
    };
    app.update(Action::ConnectionSucceeded {
        profile_id,
        generation,
        server: server(database),
        mutation_capabilities: Default::default(),
    });
    let target = lazydb::model::execution_target::ExecutionTarget::from_profile(
        app.profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .unwrap(),
    );
    if let Some(console) = app.active_console_opt_mut() {
        console.execution_target = Some(target.clone());
    }
    let console_id = app.active_console_opt().map(|console| console.id);
    if let Some(record) = app
        .sql_editors
        .iter_mut()
        .find(|record| Some(record.id) == console_id)
    {
        record.execution_target = Some(target);
    }
}

#[test]
fn saving_after_opening_two_profiles_does_not_duplicate_console_ids() {
    let first = memory_profile("first");
    let second = memory_profile("second");
    let first_id = first.id;
    let second_id = second.id;
    let mut app = App::new(vec![first, second]);

    connect(&mut app, first_id, "first");
    app.update(Action::ReplaceEditor("SELECT first".into()));
    let first_console_id = app.active_console().id;

    connect(&mut app, second_id, "second");
    app.update(Action::NewConsole);
    app.update(Action::ReplaceEditor("SELECT second".into()));

    let temp = TempDir::new().unwrap();
    let store = WorkspaceStore::new(temp.path().join("workspace.toml"), temp.path().join("sql"));
    let result = store.save(&app.workspace_snapshot());

    let snapshot = app.workspace_snapshot();
    assert!(
        snapshot
            .profiles
            .iter()
            .flat_map(|profile| profile.consoles.iter())
            .filter(|console| console.id == first_console_id)
            .count()
            .gt(&0)
    );
    let console_ids = snapshot
        .profiles
        .iter()
        .flat_map(|profile| profile.consoles.iter().map(|console| console.id))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(console_ids.len(), 2);
    assert_eq!(snapshot.profiles.len(), 2);
    assert!(snapshot.profiles.iter().all(|profile| {
        profile.active_tab.is_none_or(|active| {
            profile.tabs.iter().any(|tab| match tab {
                lazydb::persistence::workspace::PersistedTab::Console { console_id } => {
                    *console_id == active
                }
                _ => false,
            })
        })
    }));
    assert!(snapshot.sql.iter().any(|(_, text)| text == "SELECT first"));
    assert!(snapshot.sql.iter().any(|(_, text)| text == "SELECT second"));
    assert!(
        result.is_ok(),
        "two-profile workspace should save: {result:?}"
    );
}

#[test]
fn restoring_a_saved_two_profile_workspace_keeps_both_console_documents_visible() {
    let first = memory_profile("first");
    let second = memory_profile("second");
    let first_id = first.id;
    let second_id = second.id;
    let mut app = App::new(vec![first.clone(), second.clone()]);

    connect(&mut app, first_id, "first");
    app.update(Action::ReplaceEditor("SELECT first".into()));
    connect(&mut app, second_id, "second");
    app.update(Action::NewConsole);
    app.update(Action::ReplaceEditor("SELECT second".into()));

    let temp = TempDir::new().unwrap();
    let store = WorkspaceStore::new(temp.path().join("workspace.toml"), temp.path().join("sql"));
    store.save(&app.workspace_snapshot()).unwrap();
    let snapshot = store.load().unwrap().unwrap();

    let mut restored = App::new(vec![first, second]);
    restored.connection.profile_id = Some(second_id);
    restored.restore_workspace(snapshot, Some(second_id));

    assert_eq!(restored.sql_editors.len(), 2);
    assert_eq!(restored.tabs.len(), 2);
    let texts = restored
        .sql_editors
        .iter()
        .map(|record| restored.editor_text(record.id).unwrap())
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|text| text == "SELECT first"));
    assert!(texts.iter().any(|text| text == "SELECT second"));
}

#[test]
fn switching_profiles_keeps_the_first_live_editor() {
    let first = memory_profile("first");
    let second = memory_profile("second");
    let first_id = first.id;
    let second_id = second.id;
    let mut app = App::new(vec![first, second]);

    connect(&mut app, first_id, "first");
    app.update(Action::ReplaceEditor("SELECT 'latest first'".into()));
    let console_id = app.active_console().id;

    connect(&mut app, second_id, "second");

    assert_eq!(
        app.editor_text(console_id).unwrap(),
        "SELECT 'latest first'"
    );
}

#[test]
fn saving_after_switching_profiles_uses_the_latest_shared_editor_text() {
    let first = memory_profile("first");
    let second = memory_profile("second");
    let first_id = first.id;
    let second_id = second.id;
    let mut app = App::new(vec![first, second]);

    connect(&mut app, first_id, "first");
    let first_console_id = app.active_console().id;
    app.update(Action::ReplaceEditor("SELECT 'new first'".into()));
    connect(&mut app, second_id, "second");

    let snapshot = app.workspace_snapshot();
    let first_sql = snapshot
        .sql
        .iter()
        .find(|(id, _)| *id == first_console_id)
        .map(|(_, text)| text.as_str());
    assert_eq!(first_sql, Some("SELECT 'new first'"));
}
