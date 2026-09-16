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
