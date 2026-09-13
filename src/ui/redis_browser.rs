use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{
    app::App,
    model::{
        redis_browser::{RedisBrowserFocus, RedisPreviewState},
        redis_key_tree::VisibleKeyTreeRow,
    },
};

pub fn render(frame: &mut Frame<'_>, area: Rect, app: &App, ui: &mut crate::ui::UiState) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);
    let Some(crate::model::tab::WorkspaceTab::RedisBrowser(tab)) = app.tabs.get(app.active_tab)
    else {
        return;
    };
    let keys_block = Block::default().borders(Borders::ALL);
    let mut keys_area = keys_block.inner(columns[0]);
    ui.redis_keys_viewport_rows = Some((tab.id, keys_area.height as usize));
    if tab.find.is_some() {
        keys_area.height = keys_area.height.saturating_sub(1);
    }
    let inner_preview = Block::default().borders(Borders::ALL).title("Preview");
    let preview_area = inner_preview.inner(columns[1]);
    frame.render_widget(
        keys_block
            .title(keys_title(tab))
            .border_style(panel_style(tab.focus == RedisBrowserFocus::Keys)),
        columns[0],
    );
    frame.render_widget(
        inner_preview.border_style(panel_style(tab.focus == RedisBrowserFocus::Preview)),
        columns[1],
    );
    let rows = tab.tree.visible_rows();
    let (rows, selected_id, query, phase, matches) = if let Some(find) = tab.find.as_ref() {
        let matching = find
            .matches
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        (
            find.rows
                .iter()
                .filter_map(|(id, label)| {
                    rows.iter()
                        .find(|row| &row.id == id)
                        .cloned()
                        .map(|mut row| {
                            if matching.contains(id) {
                                row.label = label.as_bytes().to_vec();
                            }
                            row
                        })
                })
                .collect::<Vec<_>>(),
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
    let visible_rows = rows
        .iter()
        .skip(tab.scroll)
        .take(row_area.height as usize)
        .collect::<Vec<_>>();
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
                .style(Style::default().fg(Color::Gray))
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
                Block::default().borders(Borders::TOP).border_style(
                    if tab.find.as_ref().is_some_and(|find| {
                        find.phase == crate::model::redis_browser::RedisFindPhase::Editing
                    }) {
                        panel_style(true)
                    } else {
                        panel_style(false)
                    },
                ),
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
    let preview = match &tab.preview {
        RedisPreviewState::Empty => Paragraph::new(Line::from("Select a key to preview")),
        RedisPreviewState::Loading { key } => {
            Paragraph::new(Line::from(format!("Loading {}", display_bytes(&key.key))))
        }
        RedisPreviewState::Ready { key, content } => Paragraph::new(vec![
            Line::from(display_bytes(&key.key)),
            Line::from(content.as_str()),
        ]),
        RedisPreviewState::Failed { key, message } => Paragraph::new(vec![
            Line::from(display_bytes(&key.key)),
            Line::from(message.as_str()),
        ])
        .style(Style::default().fg(Color::Red)),
    };
    frame.render_widget(preview, preview_area);
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

fn render_row(
    row: &VisibleKeyTreeRow,
    selected: bool,
    ui: &mut crate::ui::UiState,
    tab_id: uuid::Uuid,
    area: Rect,
    visible_index: usize,
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
    let style = if selected {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    } else {
        Style::default()
    };
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
    Line::from(format!("{}{}{}", "  ".repeat(row.depth), marker, label)).style(style)
}

fn panel_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn display_bytes(value: &[u8]) -> String {
    String::from_utf8(value.to_vec())
        .unwrap_or_else(|_| value.iter().map(|byte| format!("\\x{byte:02x}")).collect())
}
