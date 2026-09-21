//! Workspace view for database users and roles.
//!
//! A principal tab has a compact overview and a read-only DDL view.
//! Rendering is delegated to [`super::read_only_sql::ReadOnlySqlEditor`] so the
//! syntax highlighting, scrollbars, mouse handling and Vim bindings match the
//! existing relation DDL view exactly.

use super::{loading, panel_block, read_only_sql::ReadOnlySqlEditor, theme::Theme};
use crate::{
    app::App,
    model::{
        editor::EditorViewport, principal::PrincipalView, tab::WorkspaceTab, workspace::Focus,
    },
    ui::{HitRegion, HitTarget},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::Paragraph,
};

/// Vertical split used by the principal DDL view.
///
/// The first chunk contains the Overview/DDL selector, the second is an
/// optional status row, and the third owns the editor.
pub(crate) fn principal_ddl_layout(area: Rect, has_status: bool) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_status {
            vec![
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
            ]
        } else {
            vec![
                Constraint::Length(2),
                Constraint::Length(0),
                Constraint::Min(1),
            ]
        })
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

fn render_tabs(
    frame: &mut Frame<'_>,
    area: Rect,
    view: PrincipalView,
    theme: Theme,
    state: &mut super::UiState,
) {
    let regions = super::render_tab_selectors(
        frame,
        area,
        &["OVERVIEW", "DDL"],
        usize::from(view == PrincipalView::Ddl),
        theme,
    );
    for (region, view) in regions
        .into_iter()
        .zip([PrincipalView::Overview, PrincipalView::Ddl])
    {
        state.hit_regions.push(HitRegion {
            area: region,
            target: HitTarget::PrincipalView(view),
        });
    }
}

/// Editor viewport geometry for the active principal tab.
///
/// `runtime.rs` and `render` must agree on this so mouse coordinates and
/// scrollbar thumbs line up.
pub(crate) fn principal_ddl_viewport(
    area: Rect,
    app: &App,
) -> Option<(uuid::Uuid, EditorViewport)> {
    let Some(WorkspaceTab::PrincipalDdl(tab)) = app.tabs.get(app.active_tab) else {
        return None;
    };
    if tab.view != PrincipalView::Ddl {
        return None;
    }
    let layout = principal_ddl_layout(area, tab.load.status().is_some());
    let block = ddl_block(app.focus == Focus::Results, Theme::default());
    let inner = block.inner(layout[2]);
    Some((
        tab.editor_id,
        EditorViewport {
            width: inner.width as usize,
            height: inner.height as usize,
        },
    ))
}

fn ddl_block(focused: bool, theme: Theme) -> ratatui::widgets::Block<'static> {
    panel_block("", focused, theme)
}

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
    state: &mut super::UiState,
) {
    let Some(WorkspaceTab::PrincipalDdl(tab)) = app.tabs.get(app.active_tab) else {
        return;
    };
    let status = tab.load.status();
    let has_status = status.is_some();
    if tab.view == PrincipalView::Overview {
        render_overview(frame, area, app, theme, state);
        return;
    }
    let layout = principal_ddl_layout(area, has_status);
    render_tabs(frame, layout[0], tab.view, theme, state);
    state.hit_regions.push(HitRegion {
        area: layout[2],
        target: HitTarget::Focus(Focus::Results),
    });
    let mut block = ddl_block(app.focus == Focus::Results, theme);
    if let Some(snapshot) = tab.load.snapshot() {
        let provenance = provenance_label(app, snapshot.connection, tab.entry.id.profile_id);
        block = block.title_top(Line::raw(format!(" {provenance} ")).right_aligned());
    }
    if let Some((message, is_error)) = status {
        let inner = panel_block("", false, theme).inner(layout[1]);
        render_status(
            frame,
            inner,
            &message,
            is_error,
            app.focus == Focus::Results,
            theme,
        );
    }
    render_ddl_editor(frame, layout[2], app, theme, state, block);
}

fn render_overview(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
    state: &mut super::UiState,
) {
    let Some(WorkspaceTab::PrincipalDdl(tab)) = app.tabs.get(app.active_tab) else {
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
    render_tabs(frame, chunks[0], tab.view, theme, state);
    let identity = match tab.entry.kind {
        crate::db::principal::PrincipalKind::User => "USER",
        crate::db::principal::PrincipalKind::Role => "ROLE",
    };
    let summary = format!(
        " {identity}  {}   native: {}   scope: {}",
        tab.entry.name,
        tab.entry.native_kind,
        principal_scope(&tab.entry.id.scope),
    );
    frame.render_widget(
        Paragraph::new(summary)
            .block(panel_block(
                " PRINCIPAL ",
                app.focus == Focus::Results,
                theme,
            ))
            .style(Style::new().fg(theme.text).bg(theme.surface)),
        chunks[1],
    );
    let details = tab.details.snapshot();
    let status = if tab.entry.system {
        "System-managed principal; modification is restricted."
    } else if details.is_none() {
        "Permissions are loading from the active database.  Press o for DDL."
    } else {
        "Current statements from the active database.  Press o for DDL."
    };
    let mut access_lines = vec![Line::raw("Permissions     Member of     Members")];
    if let Some(details) = details {
        for (index, permission) in details.permissions.iter().take(5).enumerate() {
            state.hit_regions.push(HitRegion {
                area: Rect::new(
                    chunks[2].x,
                    chunks[2].y.saturating_add(1 + index as u16),
                    chunks[2].width,
                    1,
                ),
                target: HitTarget::PrincipalPermission(index),
            });
            access_lines.push(Line::raw(format!(
                "{} {}  {}  {}:{:?}{}",
                if index == tab.selected_permission {
                    ">"
                } else {
                    " "
                },
                permission.target,
                permission.privilege,
                permission.source,
                format_args!(":{:?}", permission.source_kind),
                if permission.grantable {
                    "  GRANTABLE"
                } else {
                    ""
                }
            )));
        }
        for membership in details.member_of.iter().take(2) {
            access_lines.push(Line::raw(format!(
                "MEMBER OF  {}{}",
                membership.role,
                if membership.admin_option {
                    "  ADMIN"
                } else {
                    ""
                }
            )));
        }
    } else {
        access_lines.push(Line::styled(
            "No permission snapshot available.",
            Style::new().fg(theme.muted),
        ));
    }
    access_lines.push(Line::styled(status, Style::new().fg(theme.muted)));
    access_lines.push(Line::styled(
        match app.principal_capability(tab.entry.id.profile_id) {
            Some(crate::db::principal::PrincipalCapability::DetailsAndMutation) =>
                "Grant/Revoke: select a permission, then g/v; DDL is applied only after confirmation.",
            Some(crate::db::principal::PrincipalCapability::Details) =>
                "Permissions are read-only for this database connection.",
            Some(crate::db::principal::PrincipalCapability::DdlOnly) =>
                "This database exposes principal DDL only.",
            _ => "Principal permissions are unsupported for this database.",
        },
        Style::new().fg(theme.muted),
    ));
    if let Some(form) = tab.mutation_draft.as_ref() {
        access_lines.push(Line::styled(
            format!(
                "FORM {:?}  {:?}  {}  {}",
                form.selected_field,
                form.draft.section,
                if form.draft.grant { "GRANT" } else { "REVOKE" },
                form.draft.privilege
            ),
            Style::new().fg(theme.action),
        ));
    }
    frame.render_widget(
        Paragraph::new(access_lines).block(panel_block(" ACCESS ", false, theme)),
        chunks[2],
    );
    state.hit_regions.push(HitRegion {
        area,
        target: HitTarget::Focus(Focus::Results),
    });
}

fn principal_scope(scope: &crate::db::principal::PrincipalScope) -> String {
    match scope {
        crate::db::principal::PrincipalScope::Cluster => "cluster".to_owned(),
        crate::db::principal::PrincipalScope::Server => "server".to_owned(),
        crate::db::principal::PrincipalScope::Database(database) => format!("database:{database}"),
    }
}

fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    message: &str,
    is_error: bool,
    focused: bool,
    theme: Theme,
) {
    let style = if is_error {
        Style::new().fg(theme.error)
    } else {
        Style::new().fg(theme.action)
    }
    .bg(theme.surface)
    .add_modifier(if focused {
        Modifier::empty()
    } else {
        Modifier::DIM
    });
    let indicator = loading::activity_text(message, None, false, std::time::Duration::ZERO);
    frame.render_widget(Paragraph::new(indicator).style(style), area);
}

fn render_ddl_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
    state: &mut super::UiState,
    block: ratatui::widgets::Block<'_>,
) {
    let inner = block.inner(area);
    let viewport = EditorViewport {
        width: inner.width as usize,
        height: inner.height as usize,
    };
    let Some(WorkspaceTab::PrincipalDdl(tab)) = app.tabs.get(app.active_tab) else {
        frame.render_widget(block, area);
        return;
    };
    let Some(snapshot) = app.active_principal_editor_snapshot(viewport).ok() else {
        frame.render_widget(block, area);
        return;
    };
    let editor_id = tab.editor_id;
    super::register_text_selection_target(
        state,
        editor_id,
        Rect::new(inner.x, inner.y, inner.width, inner.height),
        &snapshot,
    );
    ReadOnlySqlEditor {
        session_id: editor_id,
        snapshot: &snapshot,
        block,
        focused: app.focus == Focus::Results && app.overlay.is_none(),
        show_line_numbers: true,
    }
    .render(frame, area, theme, state);
}

fn provenance_label(
    app: &App,
    snapshot: crate::identity::ConnectionIdentity,
    profile_id: uuid::Uuid,
) -> &'static str {
    if app.profiles.iter().all(|profile| profile.id != profile_id) {
        return "PROFILE DELETED";
    }
    if app.connection.active_identity() == Some(snapshot) {
        "LIVE"
    } else {
        "OFFLINE"
    }
}
