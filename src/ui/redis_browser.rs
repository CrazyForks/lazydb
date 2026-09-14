use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, TableState, Wrap},
};

use crate::model::redis_browser::RedisValuePageState;
use crate::{
    app::App,
    db::catalog::ObjectGroup,
    model::workspace::Focus,
    model::{
        redis_browser::{RedisBrowserFocus, RedisPreviewState},
        redis_key_tree::VisibleKeyTreeRow,
    },
    ui::{Theme, icons::IconSet},
};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    app: &App,
    ui: &mut crate::ui::UiState,
    theme: Theme,
    icons: IconSet,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);
    let Some(crate::model::tab::WorkspaceTab::RedisBrowser(tab)) = app.tabs.get(app.active_tab)
    else {
        return;
    };
    let keys_focused = app.focus == Focus::Results && tab.focus == RedisBrowserFocus::Keys;
    let preview_focused = app.focus == Focus::Results && tab.focus == RedisBrowserFocus::Preview;
    let keys_block = super::panel_block("", keys_focused, theme);
    let mut keys_area = keys_block.inner(columns[0]);
    if tab.find.is_some() {
        keys_area.height = keys_area.height.saturating_sub(1);
    }
    let inner_preview = super::panel_block("Preview", preview_focused, theme);
    let preview_area = inner_preview.inner(columns[1]);
    frame.render_widget(
        keys_block
            .title(keys_title(tab))
            .border_style(Style::new().fg(if keys_focused {
                theme.accent
            } else {
                theme.border
            })),
        columns[0],
    );
    frame.render_widget(
        inner_preview.border_style(Style::new().fg(if preview_focused {
            theme.accent
        } else {
            theme.border
        })),
        columns[1],
    );
    let rows = tab.visible_rows();
    let (rows, selected_id, query, phase, matches) = if let Some(find) = tab.find.as_ref() {
        (
            find.filtered_rows.to_vec(),
            tab.tree.selected.clone(),
            Some(find.query.value().to_owned()),
            Some(find.phase),
            Some(find.matches.len()),
        )
    } else {
        (rows, tab.tree.selected.clone(), None, None, None)
    };
    let status = if rows.is_empty() {
        Some(keyspace_empty_text(&tab.keyspace.status))
    } else {
        match &tab.keyspace.status {
            crate::model::keyspace::KeyspaceStatus::Partial => {
                Some("Continue scanning: press r".into())
            }
            crate::model::keyspace::KeyspaceStatus::Failed(message) => {
                Some(format!("{message} - r=Retry"))
            }
            crate::model::keyspace::KeyspaceStatus::Paused { .. } => {
                Some("Scan paused at client cache limit".into())
            }
            crate::model::keyspace::KeyspaceStatus::Stale => {
                Some("Showing stale keys; press r to refresh".into())
            }
            _ => None,
        }
    };
    let status_rows = if matches!(
        tab.keyspace.status,
        crate::model::keyspace::KeyspaceStatus::Failed(_)
    ) {
        2
    } else {
        u16::from(status.is_some())
    };
    let row_area = Rect::new(
        keys_area.x,
        keys_area.y,
        keys_area.width,
        keys_area
            .height
            .saturating_sub(status_rows + u16::from(query.is_some())),
    );
    ui.redis_keys_viewport_rows = Some((tab.id, row_area.height as usize));
    let visible_rows = rows
        .iter()
        .skip(tab.scroll)
        .take(row_area.height as usize)
        .collect::<Vec<_>>();
    let keys_scroll_track = Rect::new(
        keys_area.right().saturating_sub(1),
        keys_area.y,
        1,
        keys_area.height,
    );
    let keys = visible_rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            render_row(
                row,
                selected_id.as_ref() == Some(&row.id),
                ui,
                tab.id,
                row_area,
                index,
                theme,
                icons,
            )
        })
        .collect::<Vec<_>>();
    let key_body = if let Some(status) = status {
        let footer = Rect::new(
            keys_area.x,
            keys_area.bottom().saturating_sub(status_rows),
            keys_area.width,
            status_rows,
        );
        frame.render_widget(Paragraph::new(keys), row_area);
        frame.render_widget(
            Paragraph::new(status)
                .style(Style::new().fg(theme.muted).bg(theme.surface))
                .wrap(Wrap { trim: true }),
            footer,
        );
        row_area
    } else {
        frame.render_widget(Paragraph::new(keys), keys_area);
        keys_area
    };
    if let Some(query) = query {
        let search_area = Rect::new(
            columns[0].x + 1,
            key_body.bottom().saturating_sub(1),
            columns[0].width.saturating_sub(2),
            1,
        );
        ui.hit_regions.push(crate::ui::HitRegion {
            area: search_area,
            target: crate::ui::HitTarget::RedisFindInput(tab.id),
        });
        let phase = if phase == Some(crate::model::redis_browser::RedisFindPhase::Editing) {
            "/"
        } else {
            "?"
        };
        frame.render_widget(
            Paragraph::new(format!("{phase}{query}  {} matches", matches.unwrap_or(0))).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::new().fg(
                        if tab.find.as_ref().is_some_and(|find| {
                            find.phase == crate::model::redis_browser::RedisFindPhase::Editing
                        }) {
                            theme.accent
                        } else {
                            theme.border
                        },
                    )),
            ),
            search_area,
        );
        if tab
            .find
            .as_ref()
            .is_some_and(|find| find.phase == crate::model::redis_browser::RedisFindPhase::Editing)
        {
            let cursor_x = search_area.x.saturating_add(1).saturating_add(
                query
                    .chars()
                    .count()
                    .min(search_area.width.saturating_sub(2) as usize) as u16,
            );
            ui.cursor = Some(crate::ui::CursorSpec {
                position: ratatui::layout::Position::new(cursor_x, search_area.y),
                style: crate::ui::CursorStyle::Bar,
            });
        }
    }
    if let Some(geometry) = crate::ui::scrollbar::geometry(
        keys_scroll_track,
        row_area.height as usize,
        rows.len(),
        tab.scroll,
    ) {
        crate::ui::scrollbar::render_vertical(frame, keys_scroll_track, geometry, theme);
    }
    let preview_lines = match &tab.value_page {
        RedisValuePageState::Ready(page) => crate::ui::redis_value::page_lines(page),
        RedisValuePageState::Loading { key } => {
            vec![Line::from(format!("Loading {}", display_bytes(&key.key)))]
        }
        RedisValuePageState::Failed { key, message } => vec![
            Line::from(display_bytes(&key.key)),
            Line::from(message.as_str()),
        ],
        RedisValuePageState::Empty => match &tab.preview {
            RedisPreviewState::Empty => vec![Line::from("Select a key to preview")],
            RedisPreviewState::Loading { key } => {
                vec![Line::from(format!("Loading {}", display_bytes(&key.key)))]
            }
            RedisPreviewState::Ready { key, content } => vec![
                Line::from(display_bytes(&key.key)),
                Line::from(content.as_str()),
            ],
            RedisPreviewState::Failed { key, message } => vec![
                Line::from(display_bytes(&key.key)),
                Line::from(message.as_str()),
            ],
        },
    };
    let preview_session = tab.preview_editor_id;
    let (value_area, header_area) = if preview_area.height >= 3 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(preview_area);
        (chunks[1], chunks[0])
    } else {
        (preview_area, Rect::default())
    };
    if header_area.height > 0 {
        let metadata = match &tab.value_page {
            RedisValuePageState::Ready(page) => Some(&page.metadata),
            _ => None,
        };
        let key = metadata.map_or_else(
            || {
                tab.tree
                    .selected_key()
                    .map(display_bytes)
                    .unwrap_or_else(|| "No key selected".into())
            },
            |metadata| display_bytes(&metadata.key.key),
        );
        let (kind, size, ttl) =
            metadata.map_or(("—".into(), "—".into(), "—".into()), |metadata| {
                (
                    format!("{:?}", metadata.value_type),
                    crate::ui::redis_value::format_bytes(metadata.memory_usage_bytes),
                    crate::ui::redis_value::format_ttl(&metadata.ttl),
                )
            });
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    key,
                    Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
                )),
                Line::from(vec![
                    Span::styled(
                        format!("[ {kind} ] "),
                        Style::new().fg(theme.action).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("[ Size {size} ] "), Style::new().fg(theme.warning)),
                    Span::styled(format!("[ TTL {ttl} ]"), Style::new().fg(theme.muted)),
                ]),
            ]),
            header_area,
        );
        let format_label = format!(" {} ▾ ", preview_format_label(tab.format.selected));
        let format_area = Rect::new(
            preview_area
                .right()
                .saturating_sub(format_label.len() as u16),
            header_area.y + 1,
            format_label.len() as u16,
            1,
        );
        ui.hit_regions.push(crate::ui::HitRegion {
            area: format_area,
            target: crate::ui::HitTarget::RedisPreviewFormat(tab.id),
        });
        frame.render_widget(
            Paragraph::new(format_label).style(Style::new().fg(theme.action)),
            format_area,
        );
    }
    if tab.format.view() == crate::value_preview::ValueView::Table
        && let RedisValuePageState::Ready(page) = &tab.value_page
    {
        render_table_preview(
            frame,
            value_area,
            page,
            tab.preview_scroll,
            tab.id,
            ui,
            theme,
        );
        ui.redis_preview_viewport_rows = Some((
            tab.id,
            value_area.height.saturating_sub(1) as usize,
            crate::value_preview::table::from_page(&page.value)
                .rows
                .len()
                + 1,
        ));
        return;
    }
    if let Ok(snapshot) = app.redis_preview_snapshot(
        tab.id,
        crate::model::editor::EditorViewport {
            width: value_area.width.saturating_sub(1) as usize,
            height: value_area.height as usize,
        },
    ) {
        super::register_text_selection_target(ui, preview_session, value_area, &snapshot);
        for (row, line) in snapshot
            .lines
            .iter()
            .take(value_area.height as usize)
            .enumerate()
        {
            frame.render_widget(
                Paragraph::new(Line::from(crate::ui::editor_line_spans(
                    line,
                    &snapshot,
                    theme,
                    false,
                    None,
                    &super::mouse_selection_cells(ui, preview_session, &snapshot, line),
                    None,
                )))
                .style(Style::new().bg(theme.surface)),
                Rect::new(value_area.x, value_area.y + row as u16, value_area.width, 1),
            );
        }
        ui.redis_preview_viewport_rows =
            Some((tab.id, snapshot.viewport.height, snapshot.total_lines));
        super::render_editor_scrollbars(
            frame,
            value_area,
            Some(preview_session),
            &snapshot,
            theme,
            ui,
            None,
        );
    } else {
        let preview_content_rows = preview_lines.len();
        frame.render_widget(
            Paragraph::new(preview_lines)
                .style(Style::new().bg(theme.surface))
                .scroll((tab.preview_scroll.min(u16::MAX as usize) as u16, 0)),
            value_area,
        );
        ui.redis_preview_viewport_rows =
            Some((tab.id, preview_area.height as usize, preview_content_rows));
    }
}

fn render_table_preview(
    frame: &mut Frame<'_>,
    area: Rect,
    page: &crate::db::redis::read::RedisValuePage,
    offset: usize,
    tab_id: uuid::Uuid,
    ui: &mut crate::ui::UiState,
    theme: Theme,
) {
    let table = crate::value_preview::table::from_page(&page.value);
    let widths = table
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let content_width = table
                .rows
                .iter()
                .map(|row| row.cells.get(index).map_or(0, |cell| cell.chars().count()))
                .max()
                .unwrap_or(0)
                .max(column.chars().count())
                .min(32) as u16;
            Constraint::Length(content_width.max(6))
        })
        .collect::<Vec<_>>();
    let rows = table
        .rows
        .iter()
        .skip(offset)
        .map(|row| Row::new(row.cells.clone()));
    for (visible_row, row) in table.rows.iter().skip(offset).enumerate() {
        let y = area.y.saturating_add(1 + visible_row as u16);
        if y >= area.bottom() {
            break;
        }
        let mut x = area.x;
        for (column, cell) in row.cells.iter().enumerate() {
            let width =
                (cell.chars().count().clamp(6, 32) as u16).min(area.right().saturating_sub(x));
            if width == 0 {
                break;
            }
            ui.hit_regions.push(crate::ui::HitRegion {
                area: Rect::new(x, y, width, 1),
                target: crate::ui::HitTarget::RedisPreviewTableCell {
                    tab_id,
                    row: offset + visible_row,
                    column,
                },
            });
            x = x.saturating_add(width + 1);
        }
    }
    let widget = Table::new(rows, widths)
        .header(
            Row::new(table.columns).style(
                Style::new()
                    .fg(theme.grid_header_text)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .style(Style::new().fg(theme.text).bg(theme.surface))
        .row_highlight_style(Style::new().bg(theme.selection))
        .column_spacing(1);
    frame.render_stateful_widget(widget, area, &mut TableState::default());
}

fn preview_format_label(format: crate::value_preview::PreviewFormat) -> &'static str {
    match format.view {
        crate::value_preview::ValueView::Raw => "RAW",
        crate::value_preview::ValueView::Json => "JSON",
        crate::value_preview::ValueView::Yaml => "YAML",
        crate::value_preview::ValueView::Table => "Table",
        crate::value_preview::ValueView::Hex => "Hex",
    }
}

fn keys_title(tab: &crate::model::redis_browser::RedisBrowserTab) -> String {
    format!(
        "Keys · DB {} · {} loaded",
        tab.target.database,
        tab.keyspace.keys.len()
    )
}

fn keyspace_empty_text(status: &crate::model::keyspace::KeyspaceStatus) -> String {
    use crate::model::keyspace::KeyspaceStatus;
    match status {
        KeyspaceStatus::NotLoaded | KeyspaceStatus::Idle => "Not loaded — Enter to load".into(),
        KeyspaceStatus::Loading => "Loading keys…".into(),
        KeyspaceStatus::Partial => "No keys in batch — press r to continue".into(),
        KeyspaceStatus::CompleteEmpty => "No matching keys".into(),
        KeyspaceStatus::Complete => "No keys".into(),
        KeyspaceStatus::Failed(message) => format!("{message} - r=Retry"),
        KeyspaceStatus::Paused { loaded } => {
            format!("Client cache limit reached ({loaded} loaded)")
        }
        KeyspaceStatus::Stale => "Showing stale keys; refresh to reload".into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_row(
    row: &VisibleKeyTreeRow,
    selected: bool,
    ui: &mut crate::ui::UiState,
    tab_id: uuid::Uuid,
    area: Rect,
    visible_index: usize,
    theme: Theme,
    icons: IconSet,
) -> Line<'static> {
    let marker = if !row.expandable {
        "  "
    } else if row.expanded {
        "▾ "
    } else {
        "▸ "
    };
    let label = if row.is_key && row.expandable {
        format!("{} [key]", display_bytes(&row.label))
    } else {
        display_bytes(&row.label)
    };
    let background = if selected {
        theme.selection
    } else {
        theme.surface
    };
    let style = if selected {
        Style::new()
            .bg(background)
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().bg(background).fg(theme.text)
    };
    let icon_style = Style::new().bg(background).fg(if row.expandable {
        theme.muted
    } else {
        theme.text
    });
    ui.hit_regions.push(crate::ui::HitRegion {
        area: Rect::new(
            area.x + 2,
            area.y + visible_index as u16,
            area.width.saturating_sub(2),
            1,
        ),
        target: crate::ui::HitTarget::RedisKeyNode {
            tab_id,
            node: row.id.clone(),
        },
    });
    if row.expandable {
        ui.hit_regions.push(crate::ui::HitRegion {
            area: Rect::new(area.x, area.y + visible_index as u16, 2, 1),
            target: crate::ui::HitTarget::RedisKeyToggle {
                tab_id,
                node: row.id.clone(),
            },
        });
    }
    let icon = if row.expandable {
        icons.group(ObjectGroup::Tables, row.expanded)
    } else {
        icons.redis_key()
    };
    Line::from(vec![
        Span::styled(format!("{}{} ", "  ".repeat(row.depth), marker), style),
        Span::styled(format!("{} ", icon), icon_style),
        Span::styled(label, style),
        Span::styled(
            " ".repeat(area.width.saturating_sub(1) as usize),
            Style::new().bg(background),
        ),
    ])
}

fn display_bytes(value: &[u8]) -> String {
    crate::model::redis_key_text::display_bytes(value)
}
