//! Integration tests for the principal (user/role) workspace tab.
//!
//! Verifies the user-visible contract: a single reused tab, the
//! `{name}@{connection}` title, the Overview/DDL selector, and rejection of
//! stale responses.

use lazydb::{
    action::Action,
    app::App,
    db::principal::{
        PrincipalDdl, PrincipalDetails, PrincipalEntry, PrincipalId, PrincipalKind,
        PrincipalMembership, PrincipalPermission, PrincipalReadTarget, PrincipalScope,
    },
    identity::ConnectionIdentity,
    model::{
        principal::{PrincipalDdlRequest, PrincipalDetailsRequest, PrincipalView},
        tab::WorkspaceTab,
    },
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
    assert!(output.contains("OVERVIEW"), "{output}");
    assert!(output.contains("DDL"), "{output}");
    assert!(output.contains("Permissions"), "{output}");
}

#[test]
fn principal_tab_defaults_to_overview_and_can_switch_to_ddl() {
    let (mut app, profile_id) = app_with_profile();
    app.update(Action::OpenPrincipal {
        profile_id,
        entry: entry(profile_id),
    });
    let index = principal_tab_index(&app);
    assert!(matches!(
        &app.tabs[index],
        WorkspaceTab::PrincipalDdl(tab) if tab.view == PrincipalView::Overview
    ));
    app.update(Action::SetPrincipalView(PrincipalView::Ddl));
    assert!(matches!(
        &app.tabs[index],
        WorkspaceTab::PrincipalDdl(tab) if tab.view == PrincipalView::Ddl
    ));
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
        tab.view = PrincipalView::Ddl;
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
            principal: entry.clone(),
            sql: "CREATE ROLE alice LOGIN;\nGRANT readers TO alice;".to_owned(),
        },
    });
    let output = render(&app, 120, 30);
    assert!(output.contains("CREATE ROLE alice LOGIN;"), "{output}");
    app.update(Action::SetPrincipalView(PrincipalView::Overview));
    let details = PrincipalDetails {
        principal: entry.clone(),
        database: None,
        permissions: vec![PrincipalPermission {
            target: "public.orders".to_owned(),
            mutation_target: Some(lazydb::db::principal::PrincipalMutationTarget::Relation {
                schema: "public".to_owned(),
                relation: "orders".to_owned(),
            }),
            privilege: "SELECT".to_owned(),
            source: "direct".to_owned(),
            grantable: false,
            source_kind: lazydb::db::principal::PrincipalPermissionSource::Direct,
        }],
        member_of: vec![PrincipalMembership {
            role: "readers".to_owned(),
            member: "alice".to_owned(),
            admin_option: false,
        }],
        members: Vec::new(),
        permissions_coverage: lazydb::db::principal::PrincipalCoverage::Complete,
        membership_coverage: lazydb::db::principal::PrincipalCoverage::Complete,
    };
    let details_request = match &app.tabs[index] {
        WorkspaceTab::PrincipalDdl(tab) => {
            let mut request = details_request_for_test(tab);
            request.request_id = 99;
            request
        }
        _ => unreachable!(),
    };
    if let WorkspaceTab::PrincipalDdl(tab) = &mut app.tabs[index] {
        tab.begin_details_load(details_request.clone());
    }
    app.update(Action::PrincipalDetailsLoaded {
        request: details_request,
        details,
    });
    let output = render(&app, 120, 30);
    assert!(output.contains("public.orders"), "{output}");
    assert!(output.contains("SELECT"), "{output}");
    assert!(output.contains("Direct"), "{output}");
}

#[test]
fn failed_permission_details_render_the_database_error_instead_of_empty_snapshot() {
    let (mut app, profile_id) = app_with_profile();
    app.update(Action::OpenPrincipal {
        profile_id,
        entry: entry(profile_id),
    });
    let index = principal_tab_index(&app);
    let request = match &app.tabs[index] {
        WorkspaceTab::PrincipalDdl(tab) => details_request_for_test(tab),
        _ => unreachable!(),
    };
    if let WorkspaceTab::PrincipalDdl(tab) = &mut app.tabs[index] {
        tab.begin_details_load(request.clone());
    }
    app.update(Action::PrincipalDetailsFailed {
        request,
        message: "permission denied for schema private".to_owned(),
    });
    let output = render(&app, 120, 30);
    assert!(
        output.contains("permission denied for schema private"),
        "{output}"
    );
    assert!(
        !output.contains("No permission snapshot available."),
        "{output}"
    );
}

fn details_request_for_test(
    tab: &lazydb::model::principal::PrincipalDdlTab,
) -> PrincipalDetailsRequest {
    PrincipalDetailsRequest {
        tab_id: tab.id,
        tab_generation: tab.generation,
        request_id: tab.next_request_id,
        connection: ConnectionIdentity {
            profile_id: tab.entry.id.profile_id,
            generation: 1,
        },
        entry: tab.entry.clone(),
        target: PrincipalReadTarget {
            principal: tab.entry.id.clone(),
            database: None,
        },
    }
}
