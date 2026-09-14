use ratatui::{
    Frame,
    buffer::CellWidth,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
};
use unicode_width::UnicodeWidthChar;

use crate::{app::App, security::sanitize_terminal_text};

use super::{HitRegion, HitTarget, UiState, icons::IconSet, render_text_input, theme::Theme};

pub(super) struct OmniLayout {
    pub popup: Rect,
    pub input: Rect,
    pub results: Rect,
    pub status: Option<Rect>,
}

pub(super) fn layout(area: Rect, has_status: bool) -> Option<OmniLayout> {
    let width = area.width.min(88).saturating_sub(2);
    let height = area.height.min(22).saturating_sub(2);
    if width < 10 || height < 5 {
        return None;
    }

    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 3,
        width,
        height,
    );
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let input = Rect::new(inner.x, inner.y, inner.width, 1);
    let status =
        has_status.then(|| Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1));
    let results_height = inner.height.saturating_sub(2 + u16::from(has_status));
    let results = Rect::new(
        inner.x,
        inner.y.saturating_add(2),
        inner.width,
        results_height,
    );
    Some(OmniLayout {
        popup,
        input,
        results,
        status,
    })
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    app: &App,
    state: &mut UiState,
    theme: Theme,
    icons: IconSet,
) {
    let Some(omni) = app.omni.as_ref() else {
        return;
    };
    let area = frame.area();
    let visible = omni.visible_items();
    let status = omni.status.as_deref().unwrap_or(if visible.is_empty() {
        "No matching actions or objects"
    } else {
        ""
    });
    let Some(layout) = layout(area, !status.is_empty()) else {
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new("Terminal too small for Omni. Resize or press Esc.")
                .style(Style::new().fg(theme.text).bg(theme.surface)),
            area,
        );
        return;
    };
    let popup = layout.popup;
    frame.render_widget(Clear, popup);
    let footer = if matches!(omni.step, crate::model::omni::OmniStep::Root) {
        " ↑↓ select  Enter open  Esc close "
    } else {
        " ↑↓ select  Enter continue  Esc back "
    };
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().fg(theme.text).bg(theme.surface_raised))
        .title(" OMNI ")
        .title_bottom(footer);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    render_text_input(
        frame,
        layout.input,
        "> ",
        &omni.query,
        Style::new().fg(theme.text).bg(theme.surface_raised),
        state,
    );
    state.hit_regions.push(HitRegion {
        area: popup,
        target: HitTarget::Omni,
    });

    let rows = usize::from(layout.results.height);
    let (start, end) = visible_window(
        visible.len(),
        rows,
        omni.scroll,
        omni.selected
            .as_ref()
            .and_then(|id| visible.iter().position(|item| &item.id == id)),
    );
    let items = visible[start..end]
        .iter()
        .enumerate()
        .map(|(offset, item)| {
            let selected = Some(&item.id) == omni.selected.as_ref();
            let title = sanitize_terminal_text(&item.title);
            let subtitle = sanitize_terminal_text(&item.subtitle);
            let width = usize::from(inner.width);
            let marker = if selected { "> " } else { "  " };
            let marker_width = usize::from(marker.cell_width());
            let base_style = Style::new().fg(theme.text).bg(if selected {
                theme.selection
            } else {
                theme.surface_raised
            });
            let title_style = if selected {
                base_style.add_modifier(Modifier::BOLD)
            } else {
                base_style
            };
            let subtitle_style = base_style.fg(theme.muted);
            let opened_style = base_style.fg(theme.success);
            let opened = if item.opened { " [open]" } else { "" };
            let opened_width = usize::from(opened.cell_width());
            let subtitle_width = usize::from(subtitle.cell_width());
            let icon = match item.kind {
                crate::model::omni::OmniItemKind::Command
                | crate::model::omni::OmniItemKind::Action => icons.omni_command(),
                crate::model::omni::OmniItemKind::Connection(kind) => icons.database(kind),
                crate::model::omni::OmniItemKind::Console => icons.omni_console(),
                crate::model::omni::OmniItemKind::Catalog(kind) => icons.catalog(kind),
                crate::model::omni::OmniItemKind::Recent => icons.omni_recent(),
                crate::model::omni::OmniItemKind::Resume => icons.omni_resume(),
            };
            let icon_width = usize::from(icon.cell_width());
            let available = width.saturating_sub(marker_width + icon_width + 1);
            let (title_width, subtitle_width) = if subtitle.is_empty() {
                (available.saturating_sub(opened_width), 0)
            } else {
                let subtitle_budget = subtitle_width.min(available / 3);
                (
                    available.saturating_sub(subtitle_budget + 2 + opened_width),
                    subtitle_budget,
                )
            };
            let mut spans = vec![Span::styled(
                marker,
                if selected {
                    base_style.fg(theme.accent)
                } else {
                    base_style
                },
            )];
            spans.push(Span::styled(icon, base_style.fg(theme.accent)));
            spans.push(Span::styled(" ", base_style));
            spans.push(Span::styled(
                truncate_cells(&title, title_width),
                title_style,
            ));
            if subtitle_width > 0 {
                spans.push(Span::styled("  ", base_style));
                spans.push(Span::styled(
                    truncate_cells(&subtitle, subtitle_width),
                    subtitle_style,
                ));
            }
            if !opened.is_empty() {
                spans.push(Span::styled(opened, opened_style));
            }
            let line = Line::from(spans);
            let row = inner.y.saturating_add(2).saturating_add(offset as u16);
            state.hit_regions.push(HitRegion {
                area: Rect::new(inner.x, row, inner.width, 1),
                target: HitTarget::OmniItem(start + offset),
            });
            ListItem::new(line).style(base_style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).style(Style::new().bg(theme.surface_raised)),
        layout.results,
    );

    if let Some(status_area) = layout.status {
        frame.render_widget(
            Paragraph::new(sanitize_terminal_text(status))
                .style(Style::new().fg(theme.muted).bg(theme.surface_raised)),
            status_area,
        );
    }
}

fn visible_window(
    item_count: usize,
    row_count: usize,
    suggested_start: usize,
    selected: Option<usize>,
) -> (usize, usize) {
    if item_count == 0 || row_count == 0 {
        return (0, 0);
    }
    let max_start = item_count.saturating_sub(row_count);
    let mut start = suggested_start.min(max_start);
    if let Some(selected) = selected {
        if selected < start {
            start = selected;
        } else if selected >= start.saturating_add(row_count) {
            start = selected.saturating_sub(row_count.saturating_sub(1));
        }
        start = start.min(max_start);
    }
    (start, start.saturating_add(row_count).min(item_count))
}

fn truncate_cells(value: &str, width: usize) -> String {
    let mut used = 0;
    value
        .chars()
        .take_while(|character| {
            let next = character.width().unwrap_or(0);
            if used + next > width {
                false
            } else {
                used += next;
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::layout;

    #[test]
    fn layout_keeps_results_and_status_separate() {
        let layout = layout(Rect::new(4, 3, 80, 24), true).unwrap();
        assert!(layout.popup.x >= 4);
        assert!(layout.popup.right() <= 84);
        assert!(layout.popup.y >= 3);
        assert!(layout.popup.bottom() <= 27);
        assert!(!layout.results.intersects(layout.status.unwrap()));
        assert!(layout.results.bottom() <= layout.popup.bottom());
    }

    #[test]
    fn layout_rejects_terminal_that_is_too_small() {
        assert!(layout(Rect::new(0, 0, 9, 4), false).is_none());
    }
}
