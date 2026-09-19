use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::{
    Theme, UiState,
    shortcut_hints::{self, ShortcutHint},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogTone {
    Normal,
    Danger,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialogButton<'a> {
    pub label: &'a str,
    pub tone: DialogTone,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DialogActionArea {
    pub index: usize,
    pub area: Rect,
}

pub fn render_actions(
    frame: &mut Frame<'_>,
    area: Rect,
    buttons: &[DialogButton<'_>],
    focused: usize,
    theme: Theme,
) -> Vec<DialogActionArea> {
    render_actions_with_focus(frame, area, buttons, Some(focused), theme)
}

pub fn render_actions_without_focus(
    frame: &mut Frame<'_>,
    area: Rect,
    buttons: &[DialogButton<'_>],
    theme: Theme,
) -> Vec<DialogActionArea> {
    render_actions_with_focus(frame, area, buttons, None, theme)
}

fn render_actions_with_focus(
    frame: &mut Frame<'_>,
    area: Rect,
    buttons: &[DialogButton<'_>],
    focused: Option<usize>,
    theme: Theme,
) -> Vec<DialogActionArea> {
    if buttons.is_empty() || area.height == 0 || area.width == 0 {
        return Vec::new();
    }

    let labels = buttons
        .iter()
        .map(|button| format_button(button.label, false))
        .collect::<Vec<_>>();
    let total_width = labels
        .iter()
        .map(|label| label.width() as u16)
        .sum::<u16>()
        .saturating_add((labels.len().saturating_sub(1) * 2) as u16);
    let compact = total_width > area.width;
    let mut hit_regions = Vec::new();

    if compact {
        for (index, button) in buttons.iter().enumerate() {
            let y = area.y.saturating_add(index as u16);
            if y >= area.bottom() {
                break;
            }
            let label = format_button(button.label, focused == Some(index));
            let width = label.width().min(area.width as usize) as u16;
            let row = Rect::new(area.x, y, width, 1);
            render_button(frame, row, &label, *button, focused == Some(index), theme);
            if button.enabled {
                hit_regions.push(DialogActionArea { index, area: row });
            }
        }
        return hit_regions;
    }

    let mut x = area
        .x
        .saturating_add(area.width.saturating_sub(total_width) / 2);
    for (index, button) in buttons.iter().enumerate() {
        let label = format_button(button.label, focused == Some(index));
        let width = label.width() as u16;
        let row = Rect::new(x, area.y, width, 1);
        render_button(frame, row, &label, *button, focused == Some(index), theme);
        if button.enabled {
            hit_regions.push(DialogActionArea { index, area: row });
        }
        x = x.saturating_add(width).saturating_add(2);
    }
    hit_regions
}

pub fn render_frame(frame: &mut Frame<'_>, popup: Rect, title: &str, theme: Theme) -> Rect {
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(title)
        .title_style(theme.title(true))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().fg(theme.text).bg(theme.surface_raised));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    inner
}

pub fn render_body(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>, theme: Theme) {
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(theme.text).bg(theme.surface_raised))
            .wrap(Wrap { trim: true }),
        area,
    );
}

pub fn render_interactive_hint(
    frame: &mut Frame<'_>,
    area: Rect,
    text: &'static str,
    theme: Theme,
    state: &mut UiState,
) {
    let hints = text
        .split("   ")
        .filter_map(|part| {
            let (key, description) = part.split_once(char::is_whitespace)?;
            let key = key.trim();
            let description = description.trim();
            if key.is_empty() || description.is_empty() {
                return Some(ShortcutHint::new(part, ""));
            }
            let event = shortcut_event(key)?;
            Some(ShortcutHint::with_keys(key, description, [event]))
        })
        .collect::<Vec<_>>();
    shortcut_hints::render_interactive(
        frame,
        area,
        &hints,
        theme,
        theme.surface,
        Alignment::Center,
        state,
    );
}

fn shortcut_event(key: &str) -> Option<KeyEvent> {
    let (key, modifiers) = if let Some(key) = key.strip_prefix("Ctrl+") {
        (key, KeyModifiers::CONTROL)
    } else if let Some(key) = key.strip_prefix("Alt+") {
        (key, KeyModifiers::ALT)
    } else {
        (key, KeyModifiers::NONE)
    };
    let code = match key {
        "Enter" => KeyCode::Enter,
        "Esc" => KeyCode::Esc,
        "Tab" => KeyCode::Tab,
        "BackTab" | "Shift+Tab" => KeyCode::BackTab,
        "Up" | "↑" => KeyCode::Up,
        "Down" | "↓" => KeyCode::Down,
        "Left" | "←" => KeyCode::Left,
        "Right" | "→" => KeyCode::Right,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Space" => KeyCode::Char(' '),
        value if value.chars().count() == 1 => KeyCode::Char(value.chars().next()?),
        _ => return None,
    };
    Some(KeyEvent::new(code, modifiers))
}

fn format_button(label: &str, focused: bool) -> String {
    if focused {
        format!("[ > {label} ]")
    } else {
        format!("[   {label} ]")
    }
}

fn render_button(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    button: DialogButton<'_>,
    focused: bool,
    theme: Theme,
) {
    let tone_color = match button.tone {
        DialogTone::Normal => theme.accent,
        DialogTone::Danger => theme.error,
    };
    let style = if !button.enabled {
        Style::new().fg(theme.muted).bg(theme.surface)
    } else if focused {
        Style::new()
            .fg(theme.background)
            .bg(tone_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new()
            .fg(if matches!(button.tone, DialogTone::Danger) {
                theme.error
            } else {
                theme.text
            })
            .bg(theme.surface)
    };
    frame.render_widget(Paragraph::new(Span::styled(label, style)), area);
}

#[allow(dead_code)]
fn _semantic_color(tone: DialogTone, theme: Theme) -> Color {
    match tone {
        DialogTone::Normal => theme.accent,
        DialogTone::Danger => theme.error,
    }
}
