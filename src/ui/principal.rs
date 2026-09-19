//! DDL-only workspace view for database users and roles.
//!
//! The view intentionally has no DATA/DDL selector, no `RELATION DDL` title,
//! and no relation data grid: a principal tab only ever shows read-only DDL.
//! Rendering is delegated to [`super::read_only_sql::ReadOnlySqlEditor`] so the
//! syntax highlighting, scrollbars, mouse handling and Vim bindings match the
//! existing relation DDL view exactly.

use super::{loading, panel_block, read_only_sql::ReadOnlySqlEditor, theme::Theme};
use crate::{
    app::App,
    model::{editor::EditorViewport, tab::WorkspaceTab, workspace::Focus},
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
/// The first chunk is an optional status row shown while loading or after a
/// failure; the second chunk always owns the editor. Unlike the relation view
/// there is no selector row, so the editor keeps every remaining row.
pub(crate) fn principal_ddl_layout(area: Rect, has_status: bool) -> [Rect; 2] {
    if has_status {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(area);
        [chunks[0], chunks[1]]
    } else {
        [Rect::default(), area]
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
    let layout = principal_ddl_layout(area, tab.load.status().is_some());
    let block = ddl_block(app.focus == Focus::Results, Theme::default());
    let inner = block.inner(layout[1]);
    Some((
        tab.editor_id,
        EditorViewport {
            width: inner.width as usize,
            height: inner.height as usize,
        },
    ))
}

fn ddl_block(focused: bool, theme: Theme) -> ratatui::widgets::Block<'static> {
    // No title: the requirement removes the "DDL" sub-tab and its label
    // because the principal tab contains nothing else.
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
    let layout = principal_ddl_layout(area, has_status);
    state.hit_regions.push(HitRegion {
        area,
        target: HitTarget::Focus(Focus::Results),
    });
    let mut block = ddl_block(app.focus == Focus::Results, theme);
    if let Some(snapshot) = tab.load.snapshot() {
        let provenance = provenance_label(app, snapshot.connection, tab.entry.id.profile_id);
        block = block.title_top(Line::raw(format!(" {provenance} ")).right_aligned());
    }
    if let Some((message, is_error)) = status {
        let inner = panel_block("", false, theme).inner(layout[0]);
        render_status(
            frame,
            inner,
            &message,
            is_error,
            app.focus == Focus::Results,
            theme,
        );
    }
    render_ddl_editor(frame, layout[1], app, theme, state, block);
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
