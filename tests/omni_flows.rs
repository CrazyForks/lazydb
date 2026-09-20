use lazydb::{
    action::Action,
    app::App,
    commands::CommandId,
    model::{
        omni::{OmniItemAction, OmniItemId, OmniStep},
        text_input::TextInputEdit,
    },
    profile::import_connection_url,
};

#[test]
fn new_console_flow_cancels_without_creating_and_confirm_creates_named_console() {
    let mut app = App::new(Vec::new());
    let before = app.sql_editors.len();
    app.update(Action::OpenOmni);
    app.omni.as_mut().unwrap().selected = Some(OmniItemId::Command(CommandId::NewConsole));
    app.update(Action::OmniConfirm);

    assert_eq!(app.sql_editors.len(), before);
    assert_eq!(app.omni.as_ref().unwrap().step, OmniStep::PickConnection);
    let unbound = app
        .omni
        .as_ref()
        .unwrap()
        .items
        .iter()
        .find(|item| item.title == "No connection")
        .unwrap()
        .id
        .clone();
    app.omni.as_mut().unwrap().selected = Some(unbound);
    app.update(Action::OmniConfirm);
    assert_eq!(
        app.omni.as_ref().unwrap().step,
        OmniStep::NameConsole {
            profile_id: None,
            target: None,
        }
    );

    for character in "scratch".chars() {
        app.update(Action::OmniEdit(TextInputEdit::Insert(character)));
    }
    app.update(Action::OmniConfirm);

    assert!(app.omni.is_none());
    assert_eq!(app.sql_editors.len(), before + 1);
    assert_eq!(app.active_console().name, "scratch");
}

#[test]
fn new_console_flow_offers_no_connection_even_when_profiles_exist() {
    let profile = import_connection_url(":memory:", Some("configured"))
        .unwrap()
        .profile;
    let mut app = App::new(vec![profile]);
    app.update(Action::OpenOmni);
    app.omni.as_mut().unwrap().selected = Some(OmniItemId::Command(CommandId::NewConsole));
    app.update(Action::OmniConfirm);

    let omni = app.omni.as_mut().unwrap();
    assert_eq!(omni.step, OmniStep::PickConnection);
    let unbound = omni
        .items
        .iter()
        .find(|item| item.title == "No connection")
        .unwrap();
    assert_eq!(unbound.action, OmniItemAction::CreateUnboundConsole);
    omni.selected = Some(unbound.id.clone());
    app.update(Action::OmniConfirm);

    assert_eq!(
        app.omni.as_ref().unwrap().step,
        OmniStep::NameConsole {
            profile_id: None,
            target: None,
        }
    );
    for character in "unbound".chars() {
        app.update(Action::OmniEdit(TextInputEdit::Insert(character)));
    }
    let commands = app.update(Action::OmniConfirm);

    assert!(app.omni.is_none());
    assert_eq!(app.active_console().name, "unbound");
    assert!(app.active_console().execution_target.is_none());
    assert!(
        commands
            .iter()
            .all(|command| !matches!(command, lazydb::action::Command::Connect { .. }))
    );
}

#[test]
fn escape_returns_from_object_action_step_before_closing_omni() {
    let mut app = App::new(Vec::new());
    app.update(Action::OpenOmni);
    app.omni
        .as_mut()
        .unwrap()
        .push_step(lazydb::model::omni::OmniStep::PickConnection);
    app.update(Action::OmniCancel);

    assert!(app.omni.is_some());
    assert_eq!(
        app.omni.as_ref().unwrap().step,
        lazydb::model::omni::OmniStep::Root
    );
    app.update(Action::OmniCancel);
    assert!(app.omni.is_none());
}
