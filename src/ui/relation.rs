use super::{animation, loading};
use super::{panel_block, render_text_input, theme::Theme};
use crate::{
    app::App,
    model::{
        editor::EditorViewport,
        relation::{RelationLoad, RelationSnapshotProvenance, RelationView},
        tab::WorkspaceTab,
        workspace::Focus,
    },
    security::sanitize_terminal_text,
    ui::{HitRegion, HitTarget},
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::Style,
    text::Line,
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
    state: &mut super::UiState,
) {
    let Some(WorkspaceTab::Relation(tab)) = app.tabs.get(app.active_tab) else {
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    let regions = super::render_tab_selectors(
        frame,
        chunks[0],
        &["DATA", "DDL"],
        usize::from(tab.view == RelationView::Ddl),
        theme,
    );
    for (region, view) in regions
        .into_iter()
        .zip([RelationView::Data, RelationView::Ddl])
    {
        state.hit_regions.push(HitRegion {
            area: region,
            target: HitTarget::RelationView(view),
        });
    }
    match tab.view {
        RelationView::Data => render_data(frame, chunks[1], app, theme, state),
        RelationView::Ddl => render_ddl(frame, chunks[1], app, theme, state),
    }
    if let Some(crate::model::relation_edit::RelationEditSession {
        mode: crate::model::relation_edit::RelationGridMode::EditCell(editor),
        ..
    }) = &tab.edit
    {
        let popup_width = area.width.min(72);
        let json = editor.input.json_buffer();
        let popup_height = area.height.min(if json.is_some() { 16 } else { 7 });
        let popup = Rect::new(
            area.x
                .saturating_add(area.width.saturating_sub(popup_width) / 2),
            area.y
                .saturating_add(area.height.saturating_sub(popup_height) / 2),
            popup_width,
            popup_height,
        );
        frame.render_widget(ratatui::widgets::Clear, popup);
        let block = panel_block(" CELL EDITOR ", true, theme);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let is_boolean = matches!(
            &editor.input,
            crate::model::cell_editor::CellEditorBuffer::Typed {
                kind: crate::model::cell_editor::CellEditorKind::Boolean,
                draft: crate::model::cell_editor::TypedDraft::Boolean(_),
                ..
            }
        );
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);
        if let Some(json) = json {
            let lines = json
                .value()
                .split('\n')
                .map(|source| {
                    let spans = json_line_spans(source);
                    Line::from(spans)
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(lines), inner);
            frame.render_widget(
                Paragraph::new("Enter newline  Ctrl-S apply  Ctrl-F format  Esc cancel")
                    .style(Style::new().fg(theme.muted)),
                Rect::new(
                    inner.x,
                    inner.y.saturating_add(inner.height.saturating_sub(1)),
                    inner.width,
                    1,
                ),
            );
        } else if is_boolean {
            render_boolean_editor(frame, sections[0], editor, theme);
            frame.render_widget(
                Paragraph::new("Left/Right  Space  t/f").style(Style::new().fg(theme.muted)),
                sections[1],
            );
        } else if let crate::model::cell_editor::CellEditorBuffer::Typed {
            draft: crate::model::cell_editor::TypedDraft::Temporal(draft),
            ..
        } = &editor.input
        {
            render_text_input(frame, sections[0], "", draft.input(), theme.base(), state);
            if let Some(label) = draft.calendar_label() {
                frame.render_widget(
                    Paragraph::new(label).style(Style::new().fg(theme.muted)),
                    sections[1],
                );
            }
            frame.render_widget(
                Paragraph::new("Left/Right field  [/] month  Enter apply")
                    .style(Style::new().fg(theme.muted)),
                sections[2],
            );
        } else if let Some(input) = editor.input.input() {
            render_text_input(frame, sections[0], "", input, theme.base(), state);
        } else if editor.input.is_unprovided() {
            frame.render_widget(
                Paragraph::new("DEFAULT (unprovided)").style(Style::new().fg(theme.muted)),
                sections[0],
            );
        } else if matches!(
            editor.input,
            crate::model::cell_editor::CellEditorBuffer::Null(_)
        ) {
            frame.render_widget(
                Paragraph::new("NULL (explicit)").style(Style::new().fg(theme.muted)),
                sections[0],
            );
        }
        if let Some(error) = &editor.error {
            frame.render_widget(
                Paragraph::new(Line::from(error.as_str()).style(Style::new().fg(theme.error))),
                sections[3],
            );
        }
    }
}

fn json_line_spans(source: &str) -> Vec<ratatui::text::Span<'static>> {
    crate::model::cell_editor::tokenize_json(source)
        .into_iter()
        .map(|token| {
            let text = source
                .get(token.start.min(source.len())..token.end.min(source.len()))
                .unwrap_or_default();
            let color = match token.kind {
                crate::model::cell_editor::JsonTokenKind::String => ratatui::style::Color::Green,
                crate::model::cell_editor::JsonTokenKind::Number => ratatui::style::Color::Yellow,
                crate::model::cell_editor::JsonTokenKind::Boolean
                | crate::model::cell_editor::JsonTokenKind::Null => ratatui::style::Color::Magenta,
                crate::model::cell_editor::JsonTokenKind::Punctuation => {
                    ratatui::style::Color::Cyan
                }
                _ => ratatui::style::Color::Reset,
            };
            ratatui::text::Span::styled(text.to_owned(), Style::new().fg(color))
        })
        .collect()
}

fn render_boolean_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &crate::model::relation_edit::CellEditorState,
    theme: Theme,
) {
    let selected = editor.input.boolean_selection();
    let true_style = if selected == Some(true) {
        Style::new().fg(theme.background).bg(theme.accent)
    } else {
        Style::new().fg(theme.muted)
    };
    let false_style = if selected == Some(false) {
        Style::new().fg(theme.background).bg(theme.accent)
    } else {
        Style::new().fg(theme.muted)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            ratatui::text::Span::styled(" true ", true_style),
            ratatui::text::Span::raw("  "),
            ratatui::text::Span::styled(" false ", false_style),
        ])),
        area,
    );
}

#[cfg(test)]
fn cell_editor_value(editor: &crate::model::relation_edit::CellEditorState) -> String {
    editor.input.value().unwrap_or_default().to_owned()
}

fn render_data(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
    state: &mut super::UiState,
) {
    let Some(WorkspaceTab::Relation(tab)) = app.tabs.get(app.active_tab) else {
        return;
    };
    let (snapshot, status) = match &tab.data {
        RelationLoad::Ready(snapshot) => (Some(snapshot), None),
        RelationLoad::Loading { previous, .. } => (
            previous.as_ref(),
            Some((
                if previous.is_some() {
                    "Refreshing relation data"
                } else {
                    "Loading relation data"
                },
                false,
                true,
            )),
        ),
        RelationLoad::Failed { message, previous } => {
            (previous.as_ref(), Some((message.as_str(), true, false)))
        }
        RelationLoad::Cancelled { previous } => {
            (previous.as_ref(), Some(("Cancelled", true, false)))
        }
        RelationLoad::Empty => (None, Some(("No relation data", false, false))),
    };
    if let Some(snapshot) = snapshot {
        let mut result = snapshot
            .value
            .result
            .result_sets
            .last()
            .cloned()
            .unwrap_or_default();
        if let Some(edit) = &tab.edit {
            result.rows = edit.rows.iter().map(|row| row.current.clone()).collect();
        }
        let block = panel_block(" RELATION DATA ", app.focus == Focus::Results, theme);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let query_height = super::query_bar::height(&tab.query, inner.width, state.activity_icons);
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(query_height),
                Constraint::Length(u16::from(status.is_some())),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);
        let query_cursor = super::query_bar::render(
            frame,
            body[0],
            &tab.query,
            theme,
            state,
            state.activity_icons,
            app.sql_dialect(),
        );
        if let Some((message, retry, cancel)) = status {
            if cancel {
                render_loading_status(
                    frame,
                    body[1],
                    message,
                    theme,
                    state,
                    tab,
                    RelationView::Data,
                );
            } else {
                render_status(frame, body[1], message, retry, cancel, theme, state);
            }
        }
        render_relation_result_table(
            frame,
            body[2],
            tab.id,
            &result,
            tab.grid.clone(),
            &tab.grid.column_widths,
            theme,
            ratatui::widgets::Block::default().style(Style::new().bg(theme.surface)),
            state,
            tab.edit.as_ref(),
            tab.query
                .submitted
                .order_by_clause
                .as_deref()
                .unwrap_or_default(),
            app.sql_dialect(),
        );
        let sql = sanitize_terminal_text(&snapshot.value.sql);
        let footer = body[3];
        let provenance = tab
            .provenance(
                RelationView::Data,
                app.connection.active_identity(),
                app.active_profile(),
            )
            .map(provenance_label)
            .unwrap_or("UNKNOWN");
        frame.render_widget(
            Paragraph::new(format!(
                "SQL: {sql}  {} rows  Snapshot: {provenance}",
                result.rows.len()
            ))
            .style(Style::new().fg(theme.muted).bg(theme.surface)),
            footer,
        );
        state.hit_regions.push(HitRegion {
            area: footer,
            target: HitTarget::OpenTextDetail(super::readonly_detail_request(
                "Relation snapshot",
                format!(
                    "SQL: {sql}\nRows: {}\nSnapshot: {provenance}",
                    result.rows.len()
                ),
            )),
        });
        super::pagination::render(
            frame,
            body[4],
            tab.pagination,
            super::pagination::PaginationKind::Relation,
            theme,
            state,
            matches!(tab.data, RelationLoad::Ready(_)),
        );
        if let (Some(completion), Some(cursor)) = (&tab.query.completion, query_cursor) {
            super::render_data_query_completion_popup(
                frame,
                completion,
                theme,
                state,
                super::CompletionAnchor {
                    viewport: area,
                    cursor,
                    replacement_start_x: None,
                },
            );
        }
    } else if let Some((message, retry, cancel)) = status {
        let block = panel_block(" RELATION DATA ", app.focus == Focus::Results, theme);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(super::query_bar::height(
                    &tab.query,
                    inner.width,
                    state.activity_icons,
                )),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);
        let query_cursor = super::query_bar::render(
            frame,
            body[0],
            &tab.query,
            theme,
            state,
            state.activity_icons,
            app.sql_dialect(),
        );
        if cancel {
            let identity = relation_loading_identity(tab, RelationView::Data);
            let elapsed = state.animations.elapsed(&identity).unwrap_or_default();
            frame.render_widget(
                loading::LoadingViewport {
                    mode: state.animation_mode(),
                    icons: state.activity_icons,
                    elapsed,
                    label: message,
                    helper: animation::show_loading_helper(elapsed)
                        .then_some("Waiting for the first result set..."),
                    cancellable: true,
                    theme,
                    block: ratatui::widgets::Block::default().style(Style::new().bg(theme.surface)),
                },
                body[1],
            );
            state.hit_regions.push(HitRegion {
                area: body[1],
                target: HitTarget::RelationCancel,
            });
        } else {
            render_status(frame, body[1], message, retry, cancel, theme, state);
        }
        if let (Some(completion), Some(cursor)) = (&tab.query.completion, query_cursor) {
            super::render_data_query_completion_popup(
                frame,
                completion,
                theme,
                state,
                super::CompletionAnchor {
                    viewport: area,
                    cursor,
                    replacement_start_x: None,
                },
            );
        }
        super::pagination::render(
            frame,
            body[2],
            tab.pagination,
            super::pagination::PaginationKind::Relation,
            theme,
            state,
            false,
        );
    }
}

fn relation_loading_identity(
    tab: &crate::model::relation::RelationTab,
    view: RelationView,
) -> animation::LoadIdentity {
    let request = match view {
        RelationView::Data => match &tab.data {
            RelationLoad::Loading { request, .. } => request.clone(),
            _ => panic!("relation data loading identity requested while idle"),
        },
        RelationView::Ddl => match &tab.ddl {
            RelationLoad::Loading { request, .. } => request.clone(),
            _ => panic!("relation ddl loading identity requested while idle"),
        },
    };
    animation::LoadIdentity::Relation(request)
}

fn render_loading_status(
    frame: &mut Frame<'_>,
    area: Rect,
    message: &str,
    theme: Theme,
    state: &mut super::UiState,
    tab: &crate::model::relation::RelationTab,
    view: RelationView,
) {
    let identity = relation_loading_identity(tab, view);
    let elapsed = state.animations.elapsed(&identity).unwrap_or_default();
    let detail = if match view {
        RelationView::Data => matches!(
            &tab.data,
            RelationLoad::Loading {
                previous: Some(_),
                ..
            }
        ),
        RelationView::Ddl => matches!(
            &tab.ddl,
            RelationLoad::Loading {
                previous: Some(_),
                ..
            }
        ),
    } {
        Some("showing previous snapshot")
    } else {
        None
    };
    frame.render_widget(
        loading::ActivityIndicator {
            mode: state.animation_mode(),
            icons: state.activity_icons,
            elapsed,
            label: message,
            detail,
            cancellable: true,
            style: Style::new().fg(theme.action).bg(theme.surface_raised),
        },
        area,
    );
    state.hit_regions.push(HitRegion {
        area,
        target: HitTarget::RelationCancel,
    });
}

#[allow(clippy::too_many_arguments)]
fn render_relation_result_table(
    frame: &mut Frame<'_>,
    area: Rect,
    tab_id: uuid::Uuid,
    result: &crate::db::query::ResultSet,
    grid: crate::model::tab::DataGridState,
    overrides: &[Option<u16>],
    theme: Theme,
    block: ratatui::widgets::Block<'_>,
    state: &mut super::UiState,
    edit: Option<&crate::model::relation_edit::RelationEditSession>,
    order_by_clause: &str,
    dialect: crate::sql::SqlDialect,
) {
    let icons = state.activity_icons;
    let column_names = result
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let sort_projection =
        crate::sql::relation_column_sort_projection(order_by_clause, &column_names, dialect)
            .unwrap_or_else(|_| vec![None; column_names.len()]);
    super::data_grid::render(
        frame,
        area,
        tab_id,
        result,
        grid,
        overrides,
        theme,
        block,
        state,
        edit,
        icons,
        Some(&sort_projection),
        true,
    );
}

fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    message: &str,
    retry: bool,
    cancel: bool,
    theme: Theme,
    state: &mut super::UiState,
) {
    let detail_message = sanitize_terminal_text(message);
    let message = clean(message);
    let label = if retry {
        "r  retry"
    } else if cancel {
        "Ctrl-C  cancel"
    } else {
        ""
    };
    let text = if label.is_empty() {
        message.clone()
    } else {
        format!("{}  [{}]", message, label)
    };
    let retry_width = if retry { 10 } else { 0 };
    let cancel_width = if cancel { 14 } else { 0 };
    let message_area = Rect::new(
        area.x,
        area.y,
        area.width.saturating_sub(retry_width + cancel_width),
        area.height,
    );
    frame.render_widget(
        Paragraph::new(text).style(Style::new().fg(theme.warning).bg(theme.surface_raised)),
        message_area,
    );
    if !message_area.is_empty() {
        state.hit_regions.push(HitRegion {
            area: message_area,
            target: HitTarget::OpenTextDetail(crate::model::text_detail::TextDetailRequest::new(
                "Relation error",
                uuid::Uuid::nil(),
                0,
                detail_message.clone(),
                detail_message,
                None,
            )),
        });
    }
    if retry {
        state.hit_regions.push(HitRegion {
            area: Rect::new(message_area.right(), area.y, retry_width, area.height),
            target: HitTarget::RelationRetry,
        });
    }
    if cancel {
        state.hit_regions.push(HitRegion {
            area: Rect::new(
                message_area.right().saturating_add(retry_width),
                area.y,
                cancel_width,
                area.height,
            ),
            target: HitTarget::RelationCancel,
        });
    }
}

fn render_ddl(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    theme: Theme,
    _state: &mut super::UiState,
) {
    let Some(WorkspaceTab::Relation(tab)) = app.tabs.get(app.active_tab) else {
        return;
    };
    let status = match &tab.ddl {
        RelationLoad::Ready(_) => None,
        RelationLoad::Loading { .. } => Some(("Refreshing", false, true)),
        RelationLoad::Failed { message, .. } => Some((message.as_str(), true, false)),
        RelationLoad::Cancelled { .. } => Some(("Cancelled", true, false)),
        RelationLoad::Empty => Some(("No DDL available", false, false)),
    };
    let mut block = panel_block(" RELATION DDL ", app.focus == Focus::Results, theme);
    let snapshot = match &tab.ddl {
        RelationLoad::Ready(snapshot)
        | RelationLoad::Loading {
            previous: Some(snapshot),
            ..
        }
        | RelationLoad::Failed {
            previous: Some(snapshot),
            ..
        }
        | RelationLoad::Cancelled {
            previous: Some(snapshot),
        } => Some(snapshot),
        _ => None,
    };
    if let Some(snapshot) = snapshot {
        let source = match snapshot.value.provenance {
            crate::db::catalog::DdlProvenance::NativeCatalog => "NATIVE CATALOG",
            crate::db::catalog::DdlProvenance::AdapterGenerated => "GENERATED",
        };
        let provenance = tab
            .provenance(
                RelationView::Ddl,
                app.connection.active_identity(),
                app.active_profile(),
            )
            .map(provenance_label)
            .unwrap_or("UNKNOWN");
        let position = format!(
            "ROW {}  COL {}",
            tab.ddl_viewport.row_offset.saturating_add(1),
            tab.ddl_viewport.column_offset.saturating_add(1)
        );
        let available = usize::from(area.width.saturating_sub(2));
        let left_width = UnicodeWidthStr::width(" RELATION DDL ");
        let retain_provenance = provenance != "LIVE";
        let full_context = format!("{source}  {position}  {provenance}");
        let source_and_provenance = format!("{source}  {provenance}");
        let parts = if left_width + UnicodeWidthStr::width(full_context.as_str()) + 2 <= available {
            full_context
        } else if retain_provenance
            && left_width + UnicodeWidthStr::width(source_and_provenance.as_str()) + 2 <= available
        {
            source_and_provenance
        } else if retain_provenance {
            provenance.to_owned()
        } else {
            source.to_owned()
        };
        block = block.title_top(Line::raw(format!(" {parts} ")).right_aligned());
    }
    if let Some((message, retry, cancel)) = status {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(area);
        if cancel {
            render_loading_status(
                frame,
                chunks[0],
                message,
                theme,
                _state,
                tab,
                RelationView::Ddl,
            );
        } else {
            render_status(frame, chunks[0], message, retry, cancel, theme, _state);
        }
        render_ddl_editor(frame, chunks[1], app, theme, _state, block);
        return;
    }
    render_ddl_editor(frame, area, app, theme, _state, block);
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
    state.editor_viewport = Some(viewport);
    let Ok(snapshot) = app.active_ddl_editor_snapshot(viewport) else {
        frame.render_widget(block, area);
        return;
    };
    let ddl_session_id = if let Some(tab) = app.tabs.get(app.active_tab)
        && let crate::model::tab::WorkspaceTab::Relation(tab) = tab
    {
        super::register_text_selection_target(
            state,
            tab.ddl_editor_id,
            Rect::new(inner.x, inner.y, inner.width, inner.height),
            &snapshot,
        );
        Some(tab.ddl_editor_id)
    } else {
        None
    };
    frame.render_widget(block, area);
    for (row, line) in snapshot.lines.iter().take(viewport.height).enumerate() {
        let y = inner.y.saturating_add(row as u16);
        let spans = super::editor_line_spans(
            line,
            &snapshot,
            theme,
            true,
            None,
            &super::mouse_selection_cells(
                state,
                ddl_session_id.unwrap_or_default(),
                &snapshot,
                line,
            ),
        );
        let selected = snapshot
            .selection_cells
            .iter()
            .any(|(selected_line, _, _)| *selected_line == line.line);
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .style(Style::new().bg(if selected {
                    theme.selection
                } else {
                    theme.surface
                }))
                .scroll((0, snapshot.horizontal_offset.min(u16::MAX as usize) as u16)),
            Rect::new(inner.x, y, inner.width, 1),
        );
    }
    super::render_editor_scrollbars(frame, area, ddl_session_id, &snapshot, theme, state);
    if app.focus == Focus::Results
        && app.overlay.is_none()
        && let Some((x, y)) = snapshot.cursor_screen_cell
    {
        state.cursor = Some(super::CursorSpec {
            position: Position::new(inner.x.saturating_add(x), inner.y.saturating_add(y)),
            style: super::CursorStyle::Block,
        });
    }
}

#[cfg(test)]
fn ddl_text(sql: &str) -> String {
    sanitize_terminal_text(sql)
}

fn clean(value: &str) -> String {
    sanitize_terminal_text(value).chars().take(240).collect()
}

#[cfg(test)]
mod relation_status_tests {
    use super::*;

    #[test]
    fn relation_error_detail_keeps_retry_and_cancel_targets_independent() {
        let mut state = super::super::UiState::new();
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 4))
            .expect("test terminal");
        terminal
            .draw(|frame| {
                render_status(
                    frame,
                    frame.area(),
                    "relation failed",
                    true,
                    true,
                    Theme::default(),
                    &mut state,
                );
            })
            .expect("render relation error");

        let detail = state
            .hit_regions
            .iter()
            .find(|region| matches!(region.target, HitTarget::OpenTextDetail(_)))
            .expect("detail target");
        assert!(
            state
                .hit_regions
                .iter()
                .any(|region| region.target == HitTarget::RelationRetry)
        );
        assert!(
            state
                .hit_regions
                .iter()
                .any(|region| region.target == HitTarget::RelationCancel)
        );
        assert!(
            !state
                .hit_regions
                .iter()
                .filter(|region| region.target == HitTarget::RelationRetry)
                .any(|region| region.area.intersects(detail.area))
        );
        assert!(
            !state
                .hit_regions
                .iter()
                .filter(|region| region.target == HitTarget::RelationCancel)
                .any(|region| region.area.intersects(detail.area))
        );
    }
}
pub(crate) fn provenance_label(value: RelationSnapshotProvenance) -> &'static str {
    match value {
        RelationSnapshotProvenance::Live => "LIVE",
        RelationSnapshotProvenance::OfflineSnapshot => "OFFLINE SNAPSHOT",
        RelationSnapshotProvenance::ProfileDeletedSnapshot => "PROFILE DELETED SNAPSHOT",
        RelationSnapshotProvenance::OutOfScopeSnapshot => "OUT OF SCOPE SNAPSHOT",
    }
}

#[cfg(test)]
mod tests {
    use super::cell_editor_value;
    use crate::model::{
        cell_editor::{CellEditorBuffer, CellEditorKind, JsonBuffer, TemporalDraft, TypedDraft},
        relation::RelationTab,
        relation_edit::{CellEditorState, RelationEditSession, RelationGridMode},
        tab::WorkspaceTab,
        text_input::TextInput,
    };
    use chrono::NaiveDate;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn cell_editor_value_contains_only_the_cell_content() {
        let editor = CellEditorState {
            row: 5,
            column: 8,
            input: CellEditorBuffer::Text(TextInput::from("failed")),
            error: None,
        };

        assert_eq!(cell_editor_value(&editor), "failed");
        assert!(!cell_editor_value(&editor).contains("Edit cell"));
        assert!(!cell_editor_value(&editor).contains("[6, 9]"));
    }

    #[test]
    fn typed_cell_editor_rendering_is_safe_in_tiny_areas() {
        let editors = [
            CellEditorBuffer::Typed {
                kind: CellEditorKind::Json,
                draft: TypedDraft::Json(JsonBuffer::new("{}")),
            },
            CellEditorBuffer::Typed {
                kind: CellEditorKind::Date,
                draft: TypedDraft::Temporal(TemporalDraft::date(
                    NaiveDate::from_ymd_opt(2026, 8, 28).unwrap(),
                )),
            },
            CellEditorBuffer::Typed {
                kind: CellEditorKind::Boolean,
                draft: TypedDraft::Boolean(TextInput::from("true")),
            },
        ];

        for input in editors {
            let mut app = crate::app::App::new(Vec::new());
            app.tabs
                .push(WorkspaceTab::Relation(RelationTab::new("users")));
            app.active_tab = 1;
            if let WorkspaceTab::Relation(tab) = &mut app.tabs[1] {
                let mut edit =
                    RelationEditSession::from_rows(vec![vec![crate::db::value::CellValue::Text(
                        "x".into(),
                    )]]);
                edit.mode = RelationGridMode::EditCell(CellEditorState {
                    row: 0,
                    column: 0,
                    input,
                    error: None,
                });
                tab.edit = Some(edit);
            }

            let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
            terminal
                .draw(|frame| {
                    super::render(
                        frame,
                        frame.area(),
                        &app,
                        super::Theme::default(),
                        &mut super::super::UiState::new(),
                    );
                })
                .unwrap();
        }
    }

    #[test]
    fn ddl_text_sanitizes_without_the_clean_length_limit() {
        let sql = "SELECT [31m".to_owned() + &"x".repeat(300);
        let rendered = super::ddl_text(&sql);
        assert!(rendered.len() > 240);
        assert!(rendered.contains("<ESC>"));
        assert!(!rendered.contains('\u{1b}'));
    }
}
