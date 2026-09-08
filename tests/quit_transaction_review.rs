use lazydb::{
    action::{Action, Command},
    app::App,
    model::{
        relation::RelationTab,
        relation_edit::{EditableRowState, RelationEditSession},
        tab::WorkspaceTab,
        transaction::TransactionExitChoice,
        workspace::Overlay,
    },
};

fn dirty_relation(title: &str) -> RelationTab {
    let mut relation = RelationTab::new(title);
    let mut edit = RelationEditSession::from_rows(vec![vec![]]);
    edit.rows[0].state = EditableRowState::Updated {
        changed_columns: Default::default(),
    };
    relation.edit = Some(edit);
    relation
}

#[test]
fn quit_dirty_relation_opens_review() {
    let mut app = App::new(Vec::new());
    let relation = dirty_relation("users");
    let relation_id = relation.id;
    app.tabs.push(WorkspaceTab::Relation(relation));

    let commands = app.update(Action::Quit);

    assert!(commands.is_empty());
    assert!(!app.should_quit);
    assert!(matches!(
        app.overlay,
        Some(Overlay::RelationTransactionConfirm {
            tab_id,
            choice: TransactionExitChoice::Cancel,
            ..
        }) if tab_id == relation_id
    ));
}

#[test]
fn quit_reviews_non_active_relation() {
    let mut app = App::new(Vec::new());
    let relation = dirty_relation("users");
    let relation_id = relation.id;
    app.tabs.push(WorkspaceTab::Relation(relation));

    assert!(app.update(Action::Quit).is_empty());
    assert_eq!(app.active_tab, 0);
    assert!(matches!(
        app.overlay,
        Some(Overlay::RelationTransactionConfirm { tab_id, .. }) if tab_id == relation_id
    ));
}

#[test]
fn cancel_quit_preserves_edits() {
    let mut app = App::new(Vec::new());
    app.tabs
        .push(WorkspaceTab::Relation(dirty_relation("users")));

    app.update(Action::Quit);
    app.update(Action::CancelTransactionExit);

    assert!(app.overlay.is_none());
    assert!(!app.should_quit);
    assert!(app.tabs.iter().any(|tab| {
        matches!(tab, WorkspaceTab::Relation(relation) if relation.edit.as_ref().is_some_and(|edit| {
            edit.rows
                .iter()
                .any(|row| !matches!(row.state, EditableRowState::Clean))
        }))
    }));

    let commands = app.update(Action::OpenTransactionControl);
    assert!(commands.is_empty());
    assert!(!matches!(
        app.overlay,
        Some(Overlay::TransactionExitConfirm { .. })
    ));
}

#[test]
fn local_relation_rollback_rechecks_quit_and_starts_workspace_flush() {
    let mut app = App::new(Vec::new());
    app.tabs
        .push(WorkspaceTab::Relation(dirty_relation("users")));

    app.update(Action::Quit);
    let commands = app.update(Action::ConfirmTransactionExitChoice(
        TransactionExitChoice::Rollback,
    ));

    assert!(matches!(
        commands.as_slice(),
        [
            Command::PersistWorkspace { .. },
            Command::FlushWorkspace { .. }
        ]
    ));
    assert!(!app.should_quit);
    assert!(app.overlay.is_none());
}

#[test]
fn repeated_quit_does_not_replace_the_active_review() {
    let mut app = App::new(Vec::new());
    let relation = dirty_relation("users");
    let relation_id = relation.id;
    app.tabs.push(WorkspaceTab::Relation(relation));

    app.update(Action::Quit);
    let first = app.overlay.clone();
    app.update(Action::Quit);

    assert_eq!(app.overlay, first);
    assert!(matches!(
        app.overlay,
        Some(Overlay::RelationTransactionConfirm { tab_id, .. }) if tab_id == relation_id
    ));
}

#[allow(dead_code)]
fn _command_is_quit(command: &Command) -> bool {
    matches!(command, Command::Quit)
}
