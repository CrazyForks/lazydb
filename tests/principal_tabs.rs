//! Integration tests for the DDL-only principal (user/role) workspace tab.
//!
//! Verifies the user-visible contract: a single reused tab, the
//! `{name}@{connection}` title, DDL-only content with no DATA/DDL selector or
//! `RELATION DDL` chrome, and rejection of stale responses.

use lazydb::{
    action::Action,
    app::App,
    db::principal::{PrincipalDdl, PrincipalEntry, PrincipalId, PrincipalKind, PrincipalScope},
    identity::ConnectionIdentity,
    model::{principal::PrincipalDdlRequest, tab::WorkspaceTab},
    profile::import_connection_url,
    ui::{self, UiState},
};
use ratatui::{Terminal, backend::TestBackend};
use uuid::Uuid;

fn app_with_profile() -> (App, Uuid) {
    let profile = import_connection_url("sqlite::memory:", Some("orbital-lab"))
        .unwrap()
        .profile;
    let profile_id = profile.id;
    let app = App::new(vec![profile]);
    (app, profile_id)
}

fn entry(profile_id: Uuid) -> PrincipalEntry {
    PrincipalEntry {
        id: PrincipalId {
            profile_id,
            scope: PrincipalScope::Cluster,
            native_id: "10".to_owned(),
            host: None,
        },
        kind: PrincipalKind::User,
        name: "alice".to_owned(),
        native_kind: "login_role".to_owned(),
        system: false,
    }
}

fn principal_tab_index(app: &App) -> usize {
    app.tabs
        .iter()
        .position(|tab| matches!(tab, WorkspaceTab::PrincipalDdl(_)))
        .expect("principal tab")
}

fn render(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = UiState::new();
    terminal
        .draw(|frame| ui::render_with_state_using_icons(frame, app, &mut state, Default::default()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for y in 0..height {
        for x in 0..width {
            output.push_str(buffer[(x, y)].symbol());
        }
        output.push('\n');
    }
    output
}

#[test]
fn unopened_connection_does_not_render_users_and_roles() {
    let (app, _) = app_with_profile();
    let screen = render(&app, 120, 40);
    assert!(screen.contains("orbital-lab"));
    assert!(!screen.contains("Users & Roles"));
}

#[test]
fn opening_the_same_principal_reuses_one_ddl_only_tab() {
    let (mut app, profile_id) = app_with_profile();
    let tab_count_before = app.tabs.len();

    app.update(Action::OpenPrincipal {
        profile_id,
        entry: entry(profile_id),
    });
    let first = principal_tab_index(&app);
    app.update(Action::OpenPrincipal {
        profile_id,
        entry: entry(profile_id),
    });

    assert_eq!(app.tabs.len(), tab_count_before + 1);
    assert_eq!(principal_tab_index(&app), first);
    match &app.tabs[first] {
        WorkspaceTab::PrincipalDdl(tab) => {
            assert_eq!(tab.entry.name, "alice");
            assert_eq!(tab.title(), "alice");
        }
        _ => unreachable!(),
    }
}

#[test]
fn principal_tab_title_is_name_at_connection_and_has_no_relation_chrome() {
    let (mut app, profile_id) = app_with_profile();
    app.update(Action::OpenPrincipal {
        profile_id,
        entry: entry(profile_id),
    });

    let output = render(&app, 120, 30);
    assert!(output.contains("alice@orbital-lab"), "{output}");
    assert!(!output.contains("RELATION DDL"), "{output}");
    assert!(!output.contains(" DATA "), "{output}");
    assert!(!output.contains("(ctrl-o)"), "{output}");
}

#[test]
fn applying_ddl_fills_the_read_only_editor_and_stale_responses_are_rejected() {
    let (mut app, profile_id) = app_with_profile();
    app.update(Action::OpenPrincipal {
        profile_id,
        entry: entry(profile_id),
    });
    let index = principal_tab_index(&app);
    let (tab_id, tab_generation, entry) = match &app.tabs[index] {
        WorkspaceTab::PrincipalDdl(tab) => (tab.id, tab.generation, tab.entry.clone()),
        _ => unreachable!(),
    };
    let connection = ConnectionIdentity {
        profile_id,
        generation: 1,
    };
    let request = PrincipalDdlRequest {
        tab_id,
        tab_generation,
        request_id: 7,
        connection,
        entry: entry.clone(),
    };
    if let WorkspaceTab::PrincipalDdl(tab) = &mut app.tabs[index] {
        tab.begin_load(request.clone());
    }

    // A stale request id must be ignored.
    let mut stale = request.clone();
    stale.request_id = 8;
    app.update(Action::PrincipalDdlLoaded {
        request: stale,
        ddl: PrincipalDdl {
            principal: entry.clone(),
            sql: "CREATE ROLE stale;".to_owned(),
        },
    });
    let output = render(&app, 120, 30);
    assert!(!output.contains("CREATE ROLE stale"), "{output}");

    app.update(Action::PrincipalDdlLoaded {
        request,
        ddl: PrincipalDdl {
            principal: entry,
            sql: "CREATE ROLE alice LOGIN;".to_owned(),
        },
    });
    let output = render(&app, 120, 30);
    assert!(output.contains("CREATE ROLE alice LOGIN;"), "{output}");
}
