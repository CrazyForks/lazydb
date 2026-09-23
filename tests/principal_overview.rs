use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use lazydb::ui::{self, HitTarget, UiState};
use lazydb::{
    action::Action,
    app::App,
    db::principal::{
        PrincipalEntry, PrincipalId, PrincipalKind, PrincipalMembership, PrincipalPermission,
        PrincipalReadTarget, PrincipalScope,
    },
    identity::ConnectionIdentity,
    input::keymap::Keymap,
    model::{
        principal::{PrincipalDetailsRequest, PrincipalView},
        tab::WorkspaceTab,
        workspace::Focus,
    },
    profile::import_connection_url,
};
use ratatui::{Terminal, backend::TestBackend};
use uuid::Uuid;

fn principal_entry(profile_id: Uuid) -> PrincipalEntry {
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

fn app_with_access(rows: usize) -> (App, usize) {
    let profile = import_connection_url("sqlite::memory:", Some("access-test"))
        .unwrap()
        .profile;
    let profile_id = profile.id;
    let mut app = App::new(vec![profile]);
    let entry = principal_entry(profile_id);
    app.update(Action::OpenPrincipal {
        profile_id,
        entry: entry.clone(),
    });
    let tab_index = app
        .tabs
        .iter()
        .position(|tab| matches!(tab, WorkspaceTab::PrincipalDdl(_)))
        .unwrap();
    let tab = match &app.tabs[tab_index] {
        WorkspaceTab::PrincipalDdl(tab) => tab,
        _ => unreachable!(),
    };
    let request = PrincipalDetailsRequest {
        tab_id: tab.id,
        tab_generation: tab.generation,
        request_id: tab.next_request_id,
        connection: ConnectionIdentity {
            profile_id,
            generation: 1,
        },
        entry: entry.clone(),
        target: PrincipalReadTarget {
            principal: entry.id.clone(),
            database: None,
        },
    };
    if let WorkspaceTab::PrincipalDdl(tab) = &mut app.tabs[tab_index] {
        tab.begin_details_load(request.clone());
    }
    app.update(Action::PrincipalDetailsLoaded {
        request,
        details: lazydb::db::principal::PrincipalDetails {
            principal: entry,
            database: None,
            permissions: (0..rows)
                .map(|index| PrincipalPermission {
                    target: format!("public.table_{index:03}"),
                    mutation_target: None,
                    privilege: "SELECT".to_owned(),
                    source: "direct".to_owned(),
                    grantable: false,
                    source_kind: lazydb::db::principal::PrincipalPermissionSource::Direct,
                })
                .collect(),
            member_of: (0..rows)
                .map(|index| PrincipalMembership {
                    role: format!("role_{index:03}"),
                    member: "alice".to_owned(),
                    admin_option: false,
                })
                .collect(),
            members: (0..rows)
                .map(|index| PrincipalMembership {
                    role: "readers".to_owned(),
                    member: format!("member_{index:03}"),
                    admin_option: false,
                })
                .collect(),
            permissions_coverage: lazydb::db::principal::PrincipalCoverage::Complete,
            membership_coverage: lazydb::db::principal::PrincipalCoverage::Complete,
        },
    });
    app.focus = Focus::Results;
    (app, tab_index)
}

fn access_selection(app: &App, tab_index: usize) -> usize {
    match &app.tabs[tab_index] {
        WorkspaceTab::PrincipalDdl(tab) => tab.access_selection(),
        _ => unreachable!(),
    }
}

fn access_offset(app: &App, tab_index: usize) -> usize {
    match &app.tabs[tab_index] {
        WorkspaceTab::PrincipalDdl(tab) => tab.access_offset(),
        _ => unreachable!(),
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn overview_routes_vim_and_arrow_navigation_to_access_selection() {
    let (mut app, tab_index) = app_with_access(12);
    let tab_id = match &app.tabs[tab_index] {
        WorkspaceTab::PrincipalDdl(tab) => tab.id,
        _ => unreachable!(),
    };
    app.update(Action::PrincipalAccessViewportChanged { tab_id, rows: 4 });
    let mut keymap = Keymap::default();

    for event in [
        key(KeyCode::Char('j')),
        key(KeyCode::Down),
        key(KeyCode::Char('k')),
        key(KeyCode::Up),
    ] {
        let action = keymap.map(event, &app).expect("navigation action");
        assert!(
            matches!(action, Action::MovePrincipalAccess(_)),
            "{action:?}"
        );
        app.update(action);
    }
    assert_eq!(access_selection(&app, tab_index), 0);
    for _ in 0..5 {
        let action = keymap.map(key(KeyCode::Char('j')), &app).unwrap();
        app.update(action);
    }
    assert_eq!(access_selection(&app, tab_index), 5);
    assert_eq!(access_offset(&app, tab_index), 2);

    let page = keymap
        .map(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
            &app,
        )
        .unwrap();
    assert!(matches!(
        page,
        Action::PagePrincipalAccess {
            direction: 1,
            half_page: false
        }
    ));
    app.update(page);
    assert_eq!(access_selection(&app, tab_index), 9);
    assert_eq!(access_offset(&app, tab_index), 6);

    let half_page = keymap
        .map(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            &app,
        )
        .unwrap();
    app.update(half_page);
    assert_eq!(access_selection(&app, tab_index), 7);

    let page_up = keymap
        .map(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            &app,
        )
        .unwrap();
    app.update(page_up);
    assert_eq!(access_selection(&app, tab_index), 3);

    let half_page_down = keymap
        .map(
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
            &app,
        )
        .unwrap();
    app.update(half_page_down);
    assert_eq!(access_selection(&app, tab_index), 5);

    for _ in 0..20 {
        app.update(Action::MovePrincipalAccess(1));
    }
    assert_eq!(access_selection(&app, tab_index), 11);
    assert_eq!(access_offset(&app, tab_index), 8);

    keymap.map(key(KeyCode::Char('o')), &app).unwrap();
    app.update(Action::SetPrincipalView(PrincipalView::Ddl));
    let ddl_action = keymap.map(key(KeyCode::Char('j')), &app);
    assert!(matches!(ddl_action, Some(Action::ReadOnlyEditorKey { .. })));
}

#[test]
fn access_scrollbar_and_body_wheel_share_the_access_offset() {
    let (mut app, tab_index) = app_with_access(40);
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut ui_state = UiState::new();
    terminal
        .draw(|frame| {
            ui::render_with_state_using_icons(frame, &app, &mut ui_state, Default::default())
        })
        .unwrap();

    let (_, visible, body) = ui_state.principal_access_viewport.unwrap();
    assert!(visible > 3);
    assert!(body.height > 0);
    let tab_id = match &app.tabs[tab_index] {
        WorkspaceTab::PrincipalDdl(tab) => tab.id,
        _ => unreachable!(),
    };
    app.update(Action::PrincipalAccessViewportChanged {
        tab_id,
        rows: visible,
    });
    let empty_body_row = body.y.saturating_add(body.height.saturating_sub(1));
    let action = lazydb::input::mouse::map_mouse(
        mouse(MouseEventKind::ScrollDown, body.x, empty_body_row),
        &ui_state,
        &app,
    )
    .expect("body scroll action");
    assert_eq!(action, Action::ScrollPrincipalAccess(3));
    app.update(action);
    assert_eq!(access_offset(&app, tab_index), 3);

    terminal
        .draw(|frame| {
            ui::render_with_state_using_icons(frame, &app, &mut ui_state, Default::default())
        })
        .unwrap();
    let page_target = ui_state
        .hit_regions
        .iter()
        .find_map(|region| match region.target {
            HitTarget::PrincipalAccessScrollbarPage { offset, .. } if offset > 0 => {
                Some((region.area.x, region.area.y, offset))
            }
            _ => None,
        })
        .expect("scrollbar page region");
    let page_action = lazydb::input::mouse::map_mouse(
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            page_target.0,
            page_target.1,
        ),
        &ui_state,
        &app,
    )
    .unwrap();
    app.update(page_action);
    assert_eq!(access_offset(&app, tab_index), page_target.2);
    terminal
        .draw(|frame| {
            ui::render_with_state_using_icons(frame, &app, &mut ui_state, Default::default())
        })
        .unwrap();
    let thumb = ui_state
        .hit_regions
        .iter()
        .find_map(|region| match region.target {
            HitTarget::PrincipalAccessScrollbarThumb {
                tab_id,
                section,
                track_start,
                track_length,
                thumb_start,
                thumb_length,
                max_offset,
            } => Some((
                region.area.x,
                region.area.y,
                tab_id,
                section,
                track_start,
                track_length,
                thumb_start,
                thumb_length,
                max_offset,
            )),
            _ => None,
        })
        .unwrap();
    let thumb_click = lazydb::input::mouse::map_mouse(
        mouse(MouseEventKind::Down(MouseButton::Left), thumb.0, thumb.1),
        &ui_state,
        &app,
    )
    .expect("thumb drag start");
    assert!(matches!(thumb_click, Action::SetPrincipalAccessOffset(_)));
    assert!(ui_state.principal_access_scrollbar_drag.borrow().is_some());
    app.update(thumb_click);
    let drag_row = thumb.6.saturating_add(thumb.7);
    let drag = lazydb::input::mouse::map_mouse(
        mouse(MouseEventKind::Drag(MouseButton::Left), thumb.0, drag_row),
        &ui_state,
        &app,
    )
    .expect("thumb drag update");
    assert!(matches!(drag, Action::SetPrincipalAccessOffset(_)));
    app.update(drag);
    assert!(
        access_offset(&app, tab_index) > 3,
        "offset after drag: {} thumb={thumb:?}",
        access_offset(&app, tab_index)
    );
    lazydb::input::mouse::map_mouse(
        mouse(MouseEventKind::Up(MouseButton::Left), thumb.0, drag_row),
        &ui_state,
        &app,
    );
    assert!(ui_state.principal_access_scrollbar_drag.borrow().is_none());
}

#[test]
fn empty_access_has_no_scrollbar_and_resize_cancels_a_stale_thumb_drag() {
    let (mut empty_app, empty_tab) = app_with_access(0);
    let mut empty_terminal = Terminal::new(TestBackend::new(100, 22)).unwrap();
    let mut empty_ui = UiState::new();
    empty_terminal
        .draw(|frame| {
            ui::render_with_state_using_icons(frame, &empty_app, &mut empty_ui, Default::default())
        })
        .unwrap();
    let empty_tab_id = match &empty_app.tabs[empty_tab] {
        WorkspaceTab::PrincipalDdl(tab) => tab.id,
        _ => unreachable!(),
    };
    let empty_rows = empty_ui.principal_access_viewport.unwrap().1;
    empty_app.update(Action::PrincipalAccessViewportChanged {
        tab_id: empty_tab_id,
        rows: empty_rows,
    });
    assert!(!empty_ui.hit_regions.iter().any(|region| matches!(
        region.target,
        HitTarget::PrincipalAccessScrollbarThumb { .. }
    )));
    assert!(
        !empty_ui
            .hit_regions
            .iter()
            .any(|region| matches!(region.target, HitTarget::PrincipalAccessItem(_)))
    );
    let empty_move = Action::MovePrincipalAccess(1);
    empty_app.update(empty_move);
    assert_eq!(access_selection(&empty_app, empty_tab), 0);

    let (mut app, _) = app_with_access(40);
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let mut ui_state = UiState::new();
    terminal
        .draw(|frame| {
            ui::render_with_state_using_icons(frame, &app, &mut ui_state, Default::default())
        })
        .unwrap();
    let (tab_id, visible, _) = ui_state.principal_access_viewport.unwrap();
    app.update(Action::PrincipalAccessViewportChanged {
        tab_id,
        rows: visible,
    });
    let (x, y) = ui_state
        .hit_regions
        .iter()
        .find_map(|region| {
            matches!(
                region.target,
                HitTarget::PrincipalAccessScrollbarThumb { .. }
            )
            .then_some((region.area.x, region.area.y))
        })
        .unwrap();
    let start = lazydb::input::mouse::map_mouse(
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        &ui_state,
        &app,
    )
    .unwrap();
    app.update(start);
    assert!(ui_state.principal_access_scrollbar_drag.borrow().is_some());

    let mut resized = Terminal::new(TestBackend::new(120, 20)).unwrap();
    resized
        .draw(|frame| {
            ui::render_with_state_using_icons(frame, &app, &mut ui_state, Default::default())
        })
        .unwrap();
    let stale_drag = lazydb::input::mouse::map_mouse(
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            x,
            y.saturating_add(1),
        ),
        &ui_state,
        &app,
    );
    assert!(stale_drag.is_none());
    assert!(ui_state.principal_access_scrollbar_drag.borrow().is_none());
}

#[test]
fn local_access_find_highlights_and_cycles_matches_without_filtering_rows() {
    let (mut app, tab_index) = app_with_access(12);
    let tab_id = match &app.tabs[tab_index] {
        WorkspaceTab::PrincipalDdl(tab) => tab.id,
        _ => unreachable!(),
    };
    app.update(Action::PrincipalAccessViewportChanged { tab_id, rows: 4 });
    let mut keymap = Keymap::default();

    let open = keymap.map(key(KeyCode::Char('/')), &app).unwrap();
    assert_eq!(open, Action::PrincipalAccessFindOpen);
    app.update(open);
    for character in "table_00".chars() {
        let edit = keymap.map(key(KeyCode::Char(character)), &app).unwrap();
        app.update(edit);
    }
    let question_mark = keymap.map(key(KeyCode::Char('?')), &app).unwrap();
    assert!(matches!(
        question_mark,
        Action::PrincipalAccessFindEdit(lazydb::model::text_input::TextInputEdit::Insert('?'))
    ));
    app.update(question_mark);
    app.update(Action::PrincipalAccessFindEdit(
        lazydb::model::text_input::TextInputEdit::Backspace,
    ));
    let first = access_selection(&app, tab_index);
    assert_eq!(first, 0);
    let find = match &app.tabs[tab_index] {
        WorkspaceTab::PrincipalDdl(tab) => tab.access_find.as_ref().unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(find.position(), (1, 10));
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut ui_state = UiState::new();
    terminal
        .draw(|frame| {
            ui::render_with_state_using_icons(frame, &app, &mut ui_state, Default::default())
        })
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("table_00"));
    assert!(screen.contains("1/10"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.fg == ui::theme::Theme::default().warning)
    );

    app.update(Action::PrincipalAccessFindConfirm);
    app.update(Action::PrincipalAccessFindNext);
    assert_eq!(access_selection(&app, tab_index), 1);
    app.update(Action::PrincipalAccessFindPrevious);
    assert_eq!(access_selection(&app, tab_index), 0);
    app.update(Action::SelectPrincipalAccessItem(7));
    app.update(Action::PrincipalAccessFindNext);
    assert_eq!(access_selection(&app, tab_index), 8);
    app.update(Action::PrincipalAccessFindClose);
    assert!(matches!(
        &app.tabs[tab_index],
        WorkspaceTab::PrincipalDdl(tab) if tab.access_find.is_none()
    ));
    assert_eq!(
        keymap.map(key(KeyCode::Char('?')), &app),
        Some(Action::ShowHelp)
    );
}
