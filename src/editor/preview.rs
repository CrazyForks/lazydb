use std::sync::Arc;

use crate::security::{DisplayLineProjection, project_editor_line};

/// Immutable, revision-scoped data shared by read-only preview snapshots.
/// Cursor, selection, viewport, and theme state deliberately live outside it.
#[derive(Debug)]
pub(super) struct PreviewDocument {
    pub(super) text: String,
    pub(super) lines: Vec<String>,
    pub(super) line_starts: Vec<usize>,
    pub(super) projections: Vec<DisplayLineProjection>,
    pub(super) max_line_width: usize,
}

impl PreviewDocument {
    pub(super) fn from_text(text: String) -> Self {
        let lines = text.split('\n').map(str::to_owned).collect::<Vec<_>>();
        let mut line_starts = Vec::with_capacity(lines.len());
        let mut offset = 0;
        for line in &lines {
            line_starts.push(offset);
            offset += line.len() + 1;
        }
        let projections = lines
            .iter()
            .map(|line| project_editor_line(line))
            .collect::<Vec<_>>();
        let max_line_width = projections
            .iter()
            .map(|line| line.source_to_display_cells.last().copied().unwrap_or(0))
            .max()
            .unwrap_or(0);
        Self {
            text,
            lines,
            line_starts,
            projections,
            max_line_width,
        }
    }
}

pub(super) type SharedPreviewDocument = Arc<PreviewDocument>;

#[derive(Clone, Debug)]
pub(super) struct PreviewWrapIndex {
    pub(super) width: usize,
    pub(super) starts: Vec<Vec<usize>>,
    pub(super) line_visual_starts: Vec<usize>,
    pub(super) total_rows: usize,
}

impl PreviewWrapIndex {
    pub(super) fn from_document(document: &PreviewDocument, width: usize) -> Self {
        let width = width.max(1);
        let mut starts = Vec::with_capacity(document.projections.len());
        let mut line_visual_starts = Vec::with_capacity(document.projections.len() + 1);
        let mut total_rows = 0;
        for projection in &document.projections {
            line_visual_starts.push(total_rows);
            let cells = projection
                .source_to_display_cells
                .last()
                .copied()
                .unwrap_or(0);
            let mut line_starts = Vec::new();
            let mut offset = 0;
            loop {
                line_starts.push(offset);
                if cells <= offset + width {
                    break;
                }
                let boundary = projection
                    .source_to_display_cells
                    .partition_point(|cell| *cell <= offset + width)
                    .saturating_sub(1);
                let end = projection
                    .source_to_display_cells
                    .get(boundary)
                    .copied()
                    .filter(|cell| *cell > offset)
                    .unwrap_or(offset + width);
                offset = end;
            }
            total_rows += line_starts.len();
            starts.push(line_starts);
        }
        line_visual_starts.push(total_rows);
        Self {
            width,
            starts,
            line_visual_starts,
            total_rows,
        }
    }

    pub(super) fn cursor_row(&self, line: usize, cell: usize) -> usize {
        let line = line.min(self.starts.len().saturating_sub(1));
        let segment = self.starts[line]
            .partition_point(|offset| *offset <= cell)
            .saturating_sub(1);
        self.line_visual_starts[line] + segment
    }
}
