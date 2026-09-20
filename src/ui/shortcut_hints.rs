use std::borrow::Cow;

use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    buffer::CellWidth,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthChar;

use super::{HitRegion, HitTarget, Theme, UiState};

const SEPARATOR: &str = "   ";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShortcutHint<'a> {
    pub key: Cow<'a, str>,
    pub description: Cow<'a, str>,
    pub activation: Option<Vec<KeyEvent>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HintPlacement {
    pub index: usize,
    pub area: Rect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SingleLineLayout {
    pub line: Line<'static>,
    pub placements: Vec<HintPlacement>,
    pub omitted: usize,
}

impl<'a> ShortcutHint<'a> {
    pub(crate) fn new(key: impl Into<Cow<'a, str>>, description: impl Into<Cow<'a, str>>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
            activation: None,
        }
    }

    pub(crate) fn with_keys(
        key: impl Into<Cow<'a, str>>,
        description: impl Into<Cow<'a, str>>,
        keys: impl Into<Vec<KeyEvent>>,
    ) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
            activation: Some(keys.into()),
        }
    }
}

pub(super) fn line(
    hints: &[ShortcutHint<'_>],
    width: u16,
    theme: Theme,
    background: Color,
) -> Line<'static> {
    let width = usize::from(width);
    if width == 0 || hints.is_empty() {
        return Line::default();
    }

    let selected = packed_count(hints, width);
    let mut spans = Vec::new();
    for (index, hint) in hints.iter().take(selected).enumerate() {
        if index > 0 {
            spans.push(separator_span(theme, background));
        }
        spans.push(Span::styled(
            hint.key.to_string(),
            Style::new()
                .fg(theme.action)
                .bg(background)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" ", Style::new().bg(background)));
        spans.push(Span::styled(
            hint.description.to_string(),
            Style::new().fg(theme.text).bg(background),
        ));
    }

    let omitted = hints.len() - selected;
    if omitted > 0 {
        let used_width = visible_width(hints, selected);
        if selected > 0 {
            spans.push(separator_span(theme, background));
        }
        let separator_width = usize::from(selected > 0) * SEPARATOR.len();
        let remaining = width.saturating_sub(used_width + separator_width);
        let marker = truncate_to_cells(&format!("... (+{omitted})"), remaining);
        spans.push(Span::styled(
            marker,
            Style::new().fg(theme.muted).bg(background),
        ));
    }

    Line::from(spans)
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    hints: &[ShortcutHint<'_>],
    theme: Theme,
    background: Color,
    alignment: Alignment,
) {
    frame.render_widget(
        Paragraph::new(line(hints, area.width, theme, background))
            .style(Style::new().bg(background))
            .alignment(alignment),
        area,
    );
}

pub(super) fn single_line_layout(
    hints: &[ShortcutHint<'_>],
    area: Rect,
    theme: Theme,
    background: Color,
    alignment: Alignment,
) -> SingleLineLayout {
    if area.is_empty() || hints.is_empty() {
        return SingleLineLayout {
            line: Line::default(),
            placements: Vec::new(),
            omitted: hints.len(),
        };
    }

    let selected = packed_count(hints, usize::from(area.width));
    let omitted = hints.len().saturating_sub(selected);
    let marker = if omitted == 0 {
        String::new()
    } else {
        format!("... (+{omitted})")
    };
    let marker_separator = if selected > 0 && !marker.is_empty() {
        SEPARATOR
    } else {
        ""
    };
    let visible_width = visible_width(hints, selected)
        .saturating_add(usize::from(marker_separator.cell_width()))
        .saturating_add(usize::from(marker.cell_width()));
    let offset = match alignment {
        Alignment::Left => 0,
        Alignment::Center => usize::from(area.width.saturating_sub(visible_width as u16) / 2),
        Alignment::Right => usize::from(area.width.saturating_sub(visible_width as u16)),
    };
    let mut x = area.x.saturating_add(offset as u16);
    let mut spans = Vec::new();
    let mut placements = Vec::new();
    for (index, hint) in hints.iter().take(selected).enumerate() {
        if index > 0 {
            spans.push(separator_span(theme, background));
            x = x.saturating_add(SEPARATOR.cell_width());
        }
        let key_width = hint.key.as_ref().cell_width();
        let description_width = hint.description.as_ref().cell_width();
        let item_width = key_width
            .saturating_add(1)
            .saturating_add(description_width);
        let clipped = item_width.min(area.right().saturating_sub(x));
        if clipped > 0 {
            placements.push(HintPlacement {
                index,
                area: Rect::new(x, area.y, clipped, 1),
            });
        }
        spans.push(Span::styled(
            hint.key.to_string(),
            theme.dialog_help_key().bg(background),
        ));
        spans.push(Span::styled(" ", Style::new().bg(background)));
        spans.push(Span::styled(
            hint.description.to_string(),
            theme.dialog_help_description().bg(background),
        ));
        x = x.saturating_add(item_width);
    }
    if !marker.is_empty() {
        if selected > 0 {
            spans.push(separator_span(theme, background));
        }
        spans.push(Span::styled(
            marker,
            Style::new().fg(theme.muted).bg(background),
        ));
    }

    SingleLineLayout {
        line: Line::from(spans),
        placements,
        omitted,
    }
}

pub(super) fn render_interactive(
    frame: &mut Frame<'_>,
    area: Rect,
    hints: &[ShortcutHint<'_>],
    theme: Theme,
    background: Color,
    alignment: Alignment,
    state: &mut UiState,
) {
    if area.height == 1 {
        let layout = single_line_layout(hints, area, theme, background, alignment);
        frame.render_widget(
            Paragraph::new(layout.line.clone())
                .style(Style::new().bg(background))
                .alignment(alignment),
            area,
        );
        for placement in layout.placements {
            if let Some(keys) = hints
                .get(placement.index)
                .and_then(|hint| hint.activation.as_ref())
            {
                state.hit_regions.push(HitRegion {
                    area: placement.area,
                    target: HitTarget::Shortcut(keys.clone()),
                });
            }
        }
        return;
    }
    let rendered = lines(hints, area.width, theme, background);
    frame.render_widget(
        Paragraph::new(rendered.clone())
            .style(Style::new().bg(background))
            .alignment(alignment),
        area,
    );
    if area.is_empty() {
        return;
    }

    let mut rows: Vec<(Vec<usize>, u16)> = vec![(Vec::new(), 0)];
    for (index, hint) in hints.iter().enumerate() {
        let width = hint.key.as_ref().cell_width();
        let text_width = hint.description.as_ref().cell_width();
        let item_width = width.saturating_add(1).saturating_add(text_width);
        let row_width = rows.last().map_or(0, |(_, width)| *width);
        let separator = if row_width == 0 {
            0
        } else {
            SEPARATOR.cell_width()
        };
        if row_width > 0
            && row_width
                .saturating_add(separator)
                .saturating_add(item_width)
                > area.width
        {
            rows.push((Vec::new(), 0));
        }
        let (indices, width) = rows.last_mut().expect("shortcut rows always have one row");
        let separator = if indices.is_empty() {
            0
        } else {
            SEPARATOR.cell_width()
        };
        indices.push(index);
        *width = width.saturating_add(separator).saturating_add(item_width);
    }
    if rows.is_empty() {
        return;
    }
    for (row_index, (indices, row_width)) in rows.into_iter().enumerate() {
        let mut x = match alignment {
            Alignment::Center => area
                .x
                .saturating_add(area.width.saturating_sub(row_width) / 2),
            Alignment::Right => area.x.saturating_add(area.width.saturating_sub(row_width)),
            Alignment::Left => area.x,
        };
        let y = area.y.saturating_add(row_index as u16);
        for index in indices {
            let hint = &hints[index];
            let width = hint.key.as_ref().cell_width();
            let text_width = hint.description.as_ref().cell_width();
            let item_width = width.saturating_add(1).saturating_add(text_width);
            if x >= area.right() {
                break;
            }
            if let Some(keys) = hint.activation.as_ref()
                && item_width > 0
                && y < area.bottom()
            {
                let clipped = item_width.min(area.right().saturating_sub(x));
                state.hit_regions.push(HitRegion {
                    area: Rect::new(x, y, clipped, 1),
                    target: HitTarget::Shortcut(keys.clone()),
                });
            }
            x = x.saturating_add(item_width);
            x = x.saturating_add(SEPARATOR.cell_width());
        }
    }
}

pub(super) fn lines(
    hints: &[ShortcutHint<'_>],
    width: u16,
    theme: Theme,
    background: Color,
) -> Vec<Line<'static>> {
    let width = usize::from(width);
    if width == 0 || hints.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut current = Line::default();
    let mut current_width: usize = 0;
    for (index, hint) in hints.iter().enumerate() {
        let separator = if index > 0 && current_width > 0 {
            SEPARATOR
        } else {
            ""
        };
        let key = hint.key.to_string();
        let description = hint.description.to_string();
        let full_width = separator.cell_width() + key.cell_width() + 1 + description.cell_width();
        if usize::from(full_width) > width {
            if !current.spans.is_empty() {
                lines.push(current);
                current = Line::default();
                current_width = 0;
            }
            for chunk in wrap_cells(&key, width) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::new()
                        .fg(theme.action)
                        .bg(background)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            for chunk in wrap_cells(&description, width) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::new().fg(theme.text).bg(background),
                )));
            }
            continue;
        }
        if current_width > 0 && current_width + usize::from(full_width) > width {
            lines.push(current);
            current = Line::default();
            current_width = 0;
        }
        if current_width > 0 {
            current.spans.push(separator_span(theme, background));
            current_width += usize::from(SEPARATOR.cell_width());
        }
        let key_style = Style::new()
            .fg(theme.action)
            .bg(background)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::new().fg(theme.text).bg(background);
        let mut remaining = width.saturating_sub(current_width);
        let mut key_part = String::new();
        for character in key.chars() {
            let character_width = character.width().unwrap_or(0);
            if character_width > remaining {
                break;
            }
            key_part.push(character);
            remaining -= character_width;
        }
        current.spans.push(Span::styled(key_part, key_style));
        if remaining > 0 {
            current
                .spans
                .push(Span::styled(" ", Style::new().bg(background)));
            remaining -= 1;
        }
        let mut description_part = String::new();
        for character in description.chars() {
            let character_width = character.width().unwrap_or(0);
            if character_width > remaining {
                if !description_part.is_empty() {
                    current
                        .spans
                        .push(Span::styled(description_part, text_style));
                    lines.push(current);
                    current = Line::default();
                    description_part = String::new();
                }
                remaining = width;
                if character_width > remaining {
                    continue;
                }
            }
            description_part.push(character);
            remaining -= character_width;
        }
        if !description_part.is_empty() {
            current
                .spans
                .push(Span::styled(description_part, text_style));
        }
        current_width = width.saturating_sub(remaining);
        if current_width >= width {
            lines.push(current);
            current = Line::default();
            current_width = 0;
        }
    }
    if !current.spans.is_empty() {
        lines.push(current);
    }
    lines
}

fn wrap_cells(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used > 0 && used + character_width > width {
            chunks.push(std::mem::take(&mut chunk));
            used = 0;
        }
        if character_width <= width {
            chunk.push(character);
            used += character_width;
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

fn packed_count(hints: &[ShortcutHint<'_>], width: usize) -> usize {
    for count in (0..=hints.len()).rev() {
        let omitted = hints.len() - count;
        let marker_width = if omitted > 0 {
            usize::from(format!("... (+{omitted})").cell_width())
        } else {
            0
        };
        let marker_separator = usize::from(count > 0 && omitted > 0) * SEPARATOR.len();
        let candidate_width = visible_width(hints, count) + marker_separator + marker_width;
        if candidate_width <= width {
            return count;
        }
    }
    0
}

fn visible_width(hints: &[ShortcutHint<'_>], count: usize) -> usize {
    hints
        .iter()
        .take(count)
        .map(|hint| {
            usize::from(hint.key.as_ref().cell_width())
                + 1
                + usize::from(hint.description.as_ref().cell_width())
        })
        .sum::<usize>()
        + count.saturating_sub(1) * SEPARATOR.len()
}

fn separator_span(theme: Theme, background: Color) -> Span<'static> {
    Span::styled(SEPARATOR, Style::new().fg(theme.muted).bg(background))
}

fn truncate_to_cells(value: &str, width: usize) -> String {
    let mut used = 0;
    value
        .chars()
        .take_while(|character| {
            let character_width = character.width().unwrap_or(0);
            if used + character_width > width {
                false
            } else {
                used += character_width;
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier};

    use super::{ShortcutHint, line, lines, single_line_layout};
    use crate::{cli::ColorMode, ui::Theme};

    fn plain_text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn line_styles_keys_descriptions_and_separators_independently() {
        let theme = Theme::deep_space();
        let rendered = line(
            &[
                ShortcutHint::new("Enter", "save"),
                ShortcutHint::new("Esc", "cancel"),
            ],
            80,
            theme,
            theme.surface,
        );

        assert_eq!(plain_text(&rendered), "Enter save   Esc cancel");
        assert_eq!(rendered.spans[0].style.fg, Some(theme.action));
        assert!(
            rendered.spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(rendered.spans[2].style.fg, Some(theme.text));
        assert_eq!(rendered.spans[3].style.fg, Some(theme.muted));
        assert_eq!(rendered.spans[4].style.fg, Some(theme.action));
        assert_eq!(rendered.spans[6].style.fg, Some(theme.text));
    }

    #[test]
    fn single_line_layout_keeps_complete_items_and_reports_omissions() {
        let theme = Theme::deep_space();
        let layout = single_line_layout(
            &[
                ShortcutHint::new("Tab", "next"),
                ShortcutHint::new("Shift+Tab", "previous"),
                ShortcutHint::new("Esc", "cancel"),
            ],
            Rect::new(0, 0, 28, 1),
            theme,
            theme.surface,
            ratatui::layout::Alignment::Left,
        );

        assert_eq!(layout.omitted, 2);
        assert_eq!(layout.placements.len(), 1);
        assert_eq!(plain_text(&layout.line), "Tab next   ... (+2)");
        assert!(
            layout
                .placements
                .iter()
                .all(|placement| placement.area.y == 0)
        );
    }

    #[test]
    fn single_line_layout_uses_display_cells_for_wide_text() {
        let theme = Theme::deep_space();
        let layout = single_line_layout(
            &[ShortcutHint::new("界", "move")],
            Rect::new(3, 4, 8, 1),
            theme,
            theme.surface,
            ratatui::layout::Alignment::Center,
        );

        assert_eq!(plain_text(&layout.line), "界 move");
        assert_eq!(layout.placements[0].area, Rect::new(3, 4, 7, 1));
    }

    #[test]
    fn packing_keeps_complete_hints_and_reports_omissions() {
        let theme = Theme::deep_space();
        let hints = [
            ShortcutHint::new("j/k", "move"),
            ShortcutHint::new("Enter", "open"),
            ShortcutHint::new("/", "find"),
        ];

        assert_eq!(
            plain_text(&line(&hints, 19, theme, theme.surface)),
            "j/k move   ... (+2)"
        );
        assert_eq!(
            plain_text(&line(&hints, 40, theme, theme.surface)),
            "j/k move   Enter open   / find"
        );
    }

    #[test]
    fn packing_measures_unicode_terminal_cells_and_handles_tiny_widths() {
        let theme = Theme::deep_space();
        let hints = [
            ShortcutHint::new("界", "move"),
            ShortcutHint::new("Enter", "open"),
        ];

        assert_eq!(
            plain_text(&line(&hints, 18, theme, theme.surface)),
            "界 move   ... (+1)"
        );
        assert_eq!(
            plain_text(&line(
                &[ShortcutHint::new("long", "hint")],
                3,
                theme,
                theme.surface
            )),
            "..."
        );
        assert!(plain_text(&line(&[], 20, theme, theme.surface)).is_empty());
        assert!(
            plain_text(&line(
                &[ShortcutHint::new("a", "b")],
                0,
                theme,
                theme.surface
            ))
            .is_empty()
        );
    }

    #[test]
    fn lines_wraps_every_hint_without_an_omission_marker() {
        let theme = Theme::deep_space();
        let hints = [
            ShortcutHint::new("j/k", "move row"),
            ShortcutHint::new("dd", "delete column"),
            ShortcutHint::new("Esc", "close editor"),
        ];
        let rendered = lines(&hints, 18, theme, theme.surface);
        let text = rendered
            .iter()
            .map(plain_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("j/k") && text.contains("move row"));
        assert!(text.contains("dd") && text.contains("delete column"));
        assert!(text.contains("Esc") && text.contains("close editor"));
        assert!(!text.contains("... (+"));
    }

    #[test]
    fn packing_uses_final_omitted_count_when_selecting_units() {
        let theme = Theme::deep_space();
        let hints = [
            ShortcutHint::new("aaaa", "x"),
            ShortcutHint::new("bb", "x"),
            ShortcutHint::new("cc", "x"),
        ];

        let rendered = plain_text(&line(&hints, 17, theme, theme.surface));
        assert_eq!(rendered, "aaaa x   ... (+2)");
    }

    #[test]
    fn plain_theme_keeps_keys_bold_when_colors_are_reset() {
        let theme = Theme::for_color_mode(ColorMode::Never);
        let rendered = line(
            &[ShortcutHint::new("Enter", "save")],
            20,
            theme,
            theme.surface,
        );

        assert_eq!(rendered.spans[0].style.fg, Some(Color::Reset));
        assert_eq!(rendered.spans[2].style.fg, Some(Color::Reset));
        assert!(
            rendered.spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            !rendered.spans[2]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }
}
