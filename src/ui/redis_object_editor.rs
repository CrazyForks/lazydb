use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    app::App,
    model::redis_object_editor::{
        RedisEditorTtlMode, RedisObjectEditorFocus, RedisObjectEditorState,
    },
    ui::{Theme, UiState},
};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &RedisObjectEditorState,
    _app: &App,
    ui: &mut UiState,
    theme: Theme,
) {
    let popup = super::centered(area, 92.min(area.width), 18.min(area.height));
    frame.render_widget(Clear, popup);
    let title = match editor.mode {
        crate::db::redis::mutation::RedisMutationMode::Create => " NEW REDIS KEY ",
        crate::db::redis::mutation::RedisMutationMode::Edit => " EDIT REDIS KEY ",
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    if editor.mode == crate::db::redis::mutation::RedisMutationMode::Create
        && editor.focus == RedisObjectEditorFocus::Key
    {
        super::render_text_input(
            frame,
            rows[0],
            "KEY    ",
            &editor.key,
            Style::new().fg(theme.accent),
            ui,
        );
    } else {
        render_line(
            frame,
            rows[0],
            "KEY",
            editor.key.value(),
            editor.focus == RedisObjectEditorFocus::Key,
            theme,
        );
    }
    render_line(
        frame,
        rows[1],
        "TYPE",
        &format!("{:?}  (←/→ to change)", editor.value_type),
        editor.focus == RedisObjectEditorFocus::Type,
        theme,
    );
    if editor.ttl_mode == RedisEditorTtlMode::Expires && editor.focus == RedisObjectEditorFocus::Ttl
    {
        super::render_text_input(
            frame,
            rows[2],
            "TTL ms ",
            &editor.ttl,
            Style::new().fg(if editor.focus == RedisObjectEditorFocus::Ttl {
                theme.accent
            } else {
                theme.muted
            }),
            ui,
        );
    } else {
        render_line(
            frame,
            rows[2],
            "TTL",
            &ttl_label(editor),
            editor.focus == RedisObjectEditorFocus::Ttl,
            theme,
        );
    }
    let value_label = match editor.value_type {
        crate::db::redis::read::RedisType::String => "VALUE (utf-8 or 0xHEX)",
        crate::db::redis::read::RedisType::Hash | crate::db::redis::read::RedisType::Stream => {
            "VALUE (key<TAB>value per line)"
        }
        crate::db::redis::read::RedisType::SortedSet => "VALUE (score<TAB>member per line)",
        _ => "VALUE (one item per line)",
    };
    let value = Paragraph::new(vec![
        Line::from(Span::styled(
            value_label,
            Style::new().fg(theme.muted).add_modifier(Modifier::BOLD),
        )),
        Line::from(crate::security::sanitize_terminal_text(
            editor.value.value(),
        )),
    ])
    .style(
        Style::new().fg(if editor.focus == RedisObjectEditorFocus::Value {
            theme.text
        } else {
            theme.muted
        }),
    )
    .wrap(Wrap { trim: false });
    frame.render_widget(value, rows[3]);
    if let Some(error) = &editor.error {
        frame.render_widget(
            Paragraph::new(error.as_str()).style(Style::new().fg(theme.error)),
            rows[4],
        );
    }
    let hints = if editor.busy {
        vec![crate::ui::shortcut_hints::ShortcutHint::with_keys(
            "Esc",
            "wait for completion",
            [KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)],
        )]
    } else {
        vec![
            crate::ui::shortcut_hints::ShortcutHint::with_keys(
                "Tab",
                "next field",
                [KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)],
            ),
            crate::ui::shortcut_hints::ShortcutHint::with_keys(
                "Shift+Tab",
                "previous field",
                [KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE)],
            ),
            crate::ui::shortcut_hints::ShortcutHint::new("Left/Right", "change type/TTL"),
            crate::ui::shortcut_hints::ShortcutHint::with_keys(
                "Enter",
                "apply",
                [KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)],
            ),
            crate::ui::shortcut_hints::ShortcutHint::with_keys(
                "Esc",
                "cancel",
                [KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)],
            ),
        ]
    };
    crate::ui::shortcut_hints::render_interactive(
        frame,
        rows[5],
        &hints,
        theme,
        theme.surface,
        ratatui::layout::Alignment::Left,
        ui,
    );
}

fn render_line(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    theme: Theme,
) {
    let line = Line::from(vec![
        Span::styled(
            format!("{label:<6}"),
            Style::new().fg(theme.muted).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            value,
            Style::new().fg(if focused { theme.accent } else { theme.text }),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn ttl_label(editor: &RedisObjectEditorState) -> String {
    match editor.ttl_mode {
        RedisEditorTtlMode::Preserve => "preserve".into(),
        RedisEditorTtlMode::Persistent => "persistent".into(),
        RedisEditorTtlMode::Expires => format!("{} ms", editor.ttl.value()),
    }
}
