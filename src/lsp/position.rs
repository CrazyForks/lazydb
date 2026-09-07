use tower_lsp_server::ls_types::{Position, Range};

/// Converts LSP UTF-16 line/character positions to UTF-8 byte offsets.
#[derive(Clone, Debug)]
pub struct PositionIndex {
    line_starts: Vec<usize>,
    text: String,
}

impl PositionIndex {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(offset + 1);
            }
        }
        Self { line_starts, text }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn offset(&self, position: Position) -> usize {
        let start = *self
            .line_starts
            .get(position.line as usize)
            .unwrap_or(&self.text.len());
        let end = self
            .line_starts
            .get(position.line as usize + 1)
            .copied()
            .unwrap_or(self.text.len())
            .min(self.text.len());
        let line = &self.text[start..end];
        let mut utf16 = 0;
        for (relative, character) in line.char_indices() {
            if utf16 >= position.character {
                return start + relative;
            }
            utf16 += character.len_utf16() as u32;
            if utf16 > position.character {
                return start + relative;
            }
        }
        end
    }

    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self
            .line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let start = self.line_starts[line];
        let character = self.text[start..offset]
            .chars()
            .map(|character| character.len_utf16() as u32)
            .sum();
        Position::new(line as u32, character)
    }

    pub fn range(&self, start: usize, end: usize) -> Range {
        Range::new(self.position(start), self.position(end))
    }
}
