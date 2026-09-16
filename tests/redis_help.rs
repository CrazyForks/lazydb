use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lazydb::{
    action::Action,
    app::App,
    db::redis::types::RedisTarget,
    input::keymap::Keymap,
    model::{
        redis_browser::{RedisBrowserFocus, RedisBrowserTab},
        tab::WorkspaceTab,
        workspace::Focus,
    },
};
use uuid::Uuid;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn window(code: char) -> [KeyEvent; 2] {
    [
        KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        key(KeyCode::Char(code)),
    ]
}

fn redis_app(focus: RedisBrowserFocus) -> App {
    let mut app = App::new(Vec::new());
    app.tabs
        .push(WorkspaceTab::RedisBrowser(RedisBrowserTab::new(
            Uuid::from_u128(1),
            RedisTarget {
                profile_id: Uuid::from_u128(2),
                database: 0,
            },
        )));
    app.active_tab = 1;
    app.focus = Focus::Results;
    if let WorkspaceTab::RedisBrowser(tab) = &mut app.tabs[app.active_tab] {
        tab.focus = focus;
    }
    app
}

#[test]
fn redis_keys_help_opens_from_default_bindings() {
    let app = redis_app(RedisBrowserFocus::Keys);
    let mut keymap = Keymap::default();

    assert_eq!(
        keymap.map(key(KeyCode::Char('?')), &app),
        Some(Action::ShowHelp)
    );
    assert_eq!(keymap.map(key(KeyCode::F(1)), &app), Some(Action::ShowHelp));
}

#[test]
fn redis_preview_help_opens_from_default_bindings() {
    let app = redis_app(RedisBrowserFocus::Preview);
    let mut keymap = Keymap::default();

    assert_eq!(
        keymap.map(key(KeyCode::Char('?')), &app),
        Some(Action::ShowHelp)
    );
    assert_eq!(keymap.map(key(KeyCode::F(1)), &app), Some(Action::ShowHelp));
}

#[test]
fn redis_help_does_not_cross_contaminate_key_find_state() {
    let mut app = redis_app(RedisBrowserFocus::Keys);
    if let WorkspaceTab::RedisBrowser(tab) = &mut app.tabs[app.active_tab] {
        tab.open_find();
        tab.confirm_find();
        tab.focus = RedisBrowserFocus::Preview;
    }
    let mut keymap = Keymap::default();

    assert_eq!(
        keymap.map(key(KeyCode::Char('?')), &app),
        Some(Action::ShowHelp)
    );
}

#[test]
fn redis_keys_ctrl_w_width_commands_target_the_inner_pane() {
    let app = redis_app(RedisBrowserFocus::Keys);
    let mut keymap = Keymap::default();

    assert_eq!(
        keymap.map(
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            &app
        ),
        None
    );
    assert_eq!(
        keymap.map(key(KeyCode::Char('>')), &app),
        Some(Action::ResizePane(lazydb::model::workspace::PaneResize {
            split: lazydb::model::workspace::PaneSplit::RedisKeysWidth,
            delta: 1,
        }))
    );
}

#[test]
fn redis_ctrl_w_horizontal_navigation_visits_explorer_keys_preview() {
    let mut app = redis_app(RedisBrowserFocus::Preview);
    app.focus = Focus::Explorer;
    let mut keymap = Keymap::default();

    for (event, expected) in [
        (window('l'), RedisBrowserFocus::Keys),
        (window('l'), RedisBrowserFocus::Preview),
    ] {
        assert_eq!(keymap.map(event[0], &app), None);
        let action = keymap.map(event[1], &app).expect("focus action");
        app.update(action);
        assert_eq!(app.focus, Focus::Results);
        assert!(matches!(
            &app.tabs[app.active_tab],
            WorkspaceTab::RedisBrowser(tab) if tab.focus == expected
        ));
    }

    for (event, expected) in [
        (window('h'), RedisBrowserFocus::Keys),
        (window('h'), RedisBrowserFocus::Keys),
    ] {
        assert_eq!(keymap.map(event[0], &app), None);
        let action = keymap.map(event[1], &app);
        if let Some(action) = action {
            app.update(action);
        }
        assert!(matches!(
            &app.tabs[app.active_tab],
            WorkspaceTab::RedisBrowser(tab) if tab.focus == expected
        ));
    }
    assert_eq!(app.focus, Focus::Explorer);
}

#[test]
fn redis_ctrl_w_horizontal_edges_consume_the_sequence_without_wrapping() {
    let mut app = redis_app(RedisBrowserFocus::Preview);
    app.focus = Focus::Results;
    let mut keymap = Keymap::default();

    assert_eq!(keymap.map(window('l')[0], &app), None);
    assert_eq!(keymap.map(window('l')[1], &app), None);
    assert!(
        keymap
            .sequence_state(&app, std::time::Instant::now())
            .is_none()
    );
    assert_eq!(app.focus, Focus::Results);
    assert!(matches!(
        &app.tabs[app.active_tab],
        WorkspaceTab::RedisBrowser(tab) if tab.focus == RedisBrowserFocus::Preview
    ));

    app.focus = Focus::Explorer;
    assert_eq!(keymap.map(window('h')[0], &app), None);
    assert_eq!(keymap.map(window('h')[1], &app), None);
    assert!(
        keymap
            .sequence_state(&app, std::time::Instant::now())
            .is_none()
    );
    assert_eq!(app.focus, Focus::Explorer);
}
