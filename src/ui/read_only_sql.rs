use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Paragraph},
};
use uuid::Uuid;

use crate::model::editor::EditorRenderSnapshot;

use super::{
    CursorSpec, CursorStyle, UiState, editor_line_spans, mouse_selection_cells,
    register_text_selection_target, render_editor_scrollbars, theme::Theme,
};

/// Render a read-only editor snapshot without assuming which workspace tab
/// owns the editor session. DDL and modal viewers can therefore share the
/// same selection, scrolling, and syntax rendering behavior.
pub(crate) struct ReadOnlySqlEditor<'a> {
    pub session_id: Uuid,
    pub snapshot: &'a EditorRenderSnapshot,
    pub block: Block<'a>,
    pub focused: bool,
}

impl ReadOnlySqlEditor<'_> {
    pub(crate) fn render(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        theme: Theme,
        state: &mut UiState,
    ) {
        let inner = self.block.inner(area);
        let viewport_height = usize::from(inner.height);
        register_text_selection_target(state, self.session_id, inner, self.snapshot);
        frame.render_widget(self.block, area);
        for (row, line) in self.snapshot.lines.iter().take(viewport_height).enumerate() {
            let selected = self
                .snapshot
                .selection_cells
                .iter()
                .any(|(selected_line, _, _)| *selected_line == line.line);
            let spans = editor_line_spans(
                line,
                self.snapshot,
                theme,
                true,
                None,
                &mouse_selection_cells(state, self.session_id, self.snapshot, line),
                None,
            );
            frame.render_widget(
                Paragraph::new(Line::from(spans))
                    .style(ratatui::style::Style::new().bg(if selected {
                        theme.selection
                    } else {
                        theme.surface
                    }))
                    .scroll((
                        0,
                        self.snapshot.horizontal_offset.min(u16::MAX as usize) as u16,
                    )),
                Rect::new(inner.x, inner.y.saturating_add(row as u16), inner.width, 1),
            );
        }
        render_editor_scrollbars(
            frame,
            area,
            Some(self.session_id),
            self.snapshot,
            theme,
            state,
            Some(Rect::new(
                area.x.saturating_add(1),
                area.bottom().saturating_sub(1),
                area.width.saturating_sub(2),
                1,
            )),
        );
        if self.focused
            && let Some((x, y)) = self.snapshot.cursor_screen_cell
        {
            state.cursor = Some(CursorSpec {
                position: Position::new(inner.x.saturating_add(x), inner.y.saturating_add(y)),
                style: CursorStyle::Block,
            });
        }
    }
}
