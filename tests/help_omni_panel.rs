use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lazydb::{
    action::Action, app::App, config::HelpPanelView, input::keymap::Keymap,
    model::workspace::Overlay,
};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn help_shortcut_opens_help_and_tab_switches_to_omni() {
    let mut app = App::new(Vec::new());
    let mut keymap = Keymap::default();

    app.update(Action::ShowHelp);
    assert!(matches!(
        app.overlay,
        Some(lazydb::model::workspace::Overlay::Help(_))
    ));

    assert_eq!(
        keymap.map(key(KeyCode::Tab), &app),
        Some(Action::ToggleHelpPanel)
    );
    app.update(Action::ToggleHelpPanel);
    assert!(app.overlay.is_none());
    assert!(app.omni.is_some());

    app.update(Action::ToggleHelpPanel);
    assert!(app.omni.is_none());
    assert!(matches!(
        app.overlay,
        Some(lazydb::model::workspace::Overlay::Help(_))
    ));
}

#[test]
fn configured_default_panel_opens_omni() {
    let mut app = App::new(Vec::new());
    app.set_help_panel_view(HelpPanelView::Omni);

    app.update(Action::ShowHelp);

    assert!(app.overlay.is_none());
    assert!(app.omni.is_some());
}

#[test]
fn dismissing_help_does_not_retain_a_hidden_omni_session() {
    let mut app = App::new(Vec::new());
    app.update(Action::ShowHelp);
    app.update(Action::ToggleHelpPanel);
    assert!(app.omni.is_some());
    app.update(Action::ToggleHelpPanel);
    assert!(app.overlay.is_some());
    app.update(Action::DismissOverlay);

    app.update(Action::ShowHelp);
    assert!(app.omni.is_none());
    assert!(app.overlay.is_some());
}

#[test]
fn unified_panel_restores_the_real_overlay_after_closing() {
    let mut app = App::new(Vec::new());
    app.overlay = Some(Overlay::Message {
        title: "source".into(),
        body: "body".into(),
    });
    app.update(Action::ShowHelp);
    assert!(matches!(app.overlay, Some(Overlay::Help(_))));

    app.update(Action::ToggleHelpPanel);
    app.update(Action::OmniDismiss);

    assert!(matches!(app.overlay, Some(Overlay::Message { .. })));
}
