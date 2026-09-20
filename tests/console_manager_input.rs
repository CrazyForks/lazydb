use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lazydb::{
    action::{Action, Command},
    app::App,
    input::keymap::Keymap,
    model::{
        dashboard::DashboardTab,
        relation::RelationTab,
        sql_editor_list::SqlEditorListMode,
        tab::WorkspaceTab,
        workspace::{Focus, Overlay},
    },
    profile::import_connection_url,
};

fn offline_app() -> App {
    let profile = import_connection_url("postgresql://user@127.0.0.1:1/app", Some("offline"))
        .unwrap()
        .profile;
    let mut app = App::new(vec![profile]);
    app.reveal_startup_profile(None);
    assert!(app.active_console_opt().is_none());
    assert!(app.tabs.is_empty());
    assert!(app.connection.profile_id.is_none());
    app
}

fn press(app: &mut App, keymap: &mut Keymap, code: KeyCode) -> Vec<Command> {
    let action = keymap
        .map(KeyEvent::new(code, KeyModifiers::NONE), app)
        .expect("console manager should map this key");
    app.update(action)
}

fn open_manager(app: &mut App, _keymap: &mut Keymap) {
    app.update(Action::OpenSqlEditorList);
    assert!(matches!(app.overlay, Some(Overlay::SqlEditorList(_))));
}

#[test]
fn console_manager_empty_startup_creates_offline_console() {
    let mut app = offline_app();
    let profile_id = app.profiles[0].id;
    let mut keymap = Keymap::default();
    open_manager(&mut app, &mut keymap);
    let commands = press(&mut app, &mut keymap, KeyCode::Char('a'));
    assert_eq!(app.tabs.len(), 1);
    let console = app.active_console_opt().unwrap();
    assert_eq!(console.name, "console_1");
    assert_eq!(
        console.execution_target.as_ref().unwrap().profile_id,
        profile_id
    );
    assert_eq!(app.focus, Focus::Editor);
    assert!(app.overlay.is_none());
    assert_eq!(
        app.connection.status,
        lazydb::model::workspace::ConnectionStatus::Disconnected
    );
    assert!(app.connection.pending_target.is_none());
    assert!(
        commands
            .iter()
            .all(|command| !matches!(command, Command::Connect { .. }))
    );
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::PersistWorkspace { .. }))
    );
}

#[test]
fn console_manager_empty_startup_escape_closes_overlay() {
    let mut app = offline_app();
    let mut keymap = Keymap::default();
    open_manager(&mut app, &mut keymap);
    press(&mut app, &mut keymap, KeyCode::Esc);
    assert!(app.overlay.is_none());
    assert!(app.tabs.is_empty());
}

#[test]
fn console_manager_empty_startup_search_can_be_cancelled() {
    let mut app = offline_app();
    let mut keymap = Keymap::default();
    open_manager(&mut app, &mut keymap);
    press(&mut app, &mut keymap, KeyCode::Char('/'));
    assert!(
        matches!(&app.overlay, Some(Overlay::SqlEditorList(list)) if list.mode == SqlEditorListMode::Search)
    );
    press(&mut app, &mut keymap, KeyCode::Char('x'));
    assert!(
        matches!(&app.overlay, Some(Overlay::SqlEditorList(list)) if list.visible_query() == "x")
    );
    press(&mut app, &mut keymap, KeyCode::Esc);
    assert!(
        matches!(&app.overlay, Some(Overlay::SqlEditorList(list)) if list.mode == SqlEditorListMode::Browse)
    );
    press(&mut app, &mut keymap, KeyCode::Esc);
    assert!(app.overlay.is_none());
}

#[test]
fn console_manager_empty_list_ignores_navigation_and_can_still_close() {
    let mut app = offline_app();
    let mut keymap = Keymap::default();
    open_manager(&mut app, &mut keymap);

    for key in [
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::Enter,
        KeyCode::Char('r'),
        KeyCode::Char('d'),
    ] {
        press(&mut app, &mut keymap, key);
    }
    assert!(app.tabs.is_empty());
    assert!(matches!(app.overlay, Some(Overlay::SqlEditorList(_))));
    press(&mut app, &mut keymap, KeyCode::Esc);
    assert!(app.overlay.is_none());
}

#[test]
fn console_manager_without_profiles_creates_unbound_console() {
    let mut app = App::new(Vec::new());
    app.update(Action::CloseActiveTab);
    assert!(app.active_console_opt().is_none());
    let mut keymap = Keymap::default();
    open_manager(&mut app, &mut keymap);

    let commands = press(&mut app, &mut keymap, KeyCode::Char('a'));
    assert_eq!(app.tabs.len(), 1);
    assert!(app.active_console_opt().unwrap().execution_target.is_none());
    assert!(app.overlay.is_none());
    assert!(
        commands
            .iter()
            .all(|command| matches!(command, Command::PersistWorkspace { .. }))
    );
}

#[test]
fn console_manager_works_without_active_console_on_relation_and_dashboard_tabs() {
    for (kind, tab) in [
        WorkspaceTab::Relation(RelationTab::new("items")),
        WorkspaceTab::Dashboard(DashboardTab::new()),
    ]
    .into_iter()
    .enumerate()
    {
        let mut app = offline_app();
        app.active_workspace_profile = Some(app.profiles[0].id);
        app.tabs.push(tab);
        app.active_tab = 0;
        assert!(app.active_console_opt().is_none());
        let mut keymap = Keymap::default();
        open_manager(&mut app, &mut keymap);
        let commands = press(&mut app, &mut keymap, KeyCode::Char('a'));

        assert!(
            !commands.is_empty(),
            "tab kind {kind} did not create a console"
        );
        assert!(
            app.active_console_opt().is_some(),
            "tab kind {kind} has no active console"
        );
        assert!(
            app.tabs
                .iter()
                .any(|tab| matches!(tab, WorkspaceTab::Sql(_)))
        );
        assert_eq!(app.focus, Focus::Editor);
        assert!(app.overlay.is_none());
    }
}

#[test]
fn editor_and_sql_execution_remain_blocked_without_active_console() {
    let mut app = offline_app();
    let before = app.tabs.len();
    assert!(
        app.update(Action::EditorKey(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )))
        .is_empty()
    );
    assert!(app.update(Action::RunActiveSql).is_empty());
    assert_eq!(app.tabs.len(), before);
}
