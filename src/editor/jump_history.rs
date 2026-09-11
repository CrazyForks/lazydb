use crate::model::editor::EditorPosition;

const HISTORY_LIMIT: usize = 100;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct JumpHistory {
    entries: Vec<EditorPosition>,
    // None means the cursor is at the live end, after the last saved position.
    cursor: Option<usize>,
}

impl JumpHistory {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.cursor = None;
    }
    pub(super) fn record_jump(&mut self, from: EditorPosition) {
        if let Some(cursor) = self.cursor {
            self.entries.truncate(cursor.saturating_add(1));
            self.cursor = None;
        }
        if self.entries.last().copied() == Some(from) {
            return;
        }
        self.entries.push(from);
        if self.entries.len() > HISTORY_LIMIT {
            self.entries.remove(0);
        }
    }

    pub(super) fn backward(
        &mut self,
        current: EditorPosition,
        count: usize,
    ) -> Option<EditorPosition> {
        if count == 0 {
            return None;
        }
        if self.cursor.is_none() {
            if self.entries.last().copied() != Some(current) {
                self.entries.push(current);
                if self.entries.len() > HISTORY_LIMIT {
                    self.entries.remove(0);
                }
            }
            self.cursor = self.entries.len().checked_sub(1);
        }
        let cursor = self.cursor?;
        let target = cursor.saturating_sub(count);
        self.cursor = Some(target);
        self.entries.get(target).copied()
    }

    pub(super) fn forward(&mut self, count: usize) -> Option<EditorPosition> {
        if count == 0 {
            return None;
        }
        let cursor = self.cursor?;
        let last = self.entries.len().checked_sub(1)?;
        let target = cursor.saturating_add(count).min(last);
        let position = self.entries[target];
        if target == last {
            self.cursor = None;
        } else {
            self.cursor = Some(target);
        }
        Some(position)
    }

    pub(super) fn remap(&mut self, mut map: impl FnMut(EditorPosition) -> EditorPosition) {
        for position in &mut self.entries {
            *position = map(*position);
        }
    }
}

pub(super) fn remap_position(
    old_text: &str,
    new_text: &str,
    start: usize,
    end: usize,
    replacement_len: usize,
    position: EditorPosition,
) -> EditorPosition {
    let old_offset = char_position_to_byte(old_text, position);
    let start = start.min(old_text.len());
    let end = end.max(start).min(old_text.len());
    let new_offset = if old_offset < start {
        old_offset
    } else if start == end {
        start
            .saturating_add(replacement_len)
            .saturating_add(old_offset.saturating_sub(start))
    } else if old_offset < end {
        start
    } else {
        start
            .saturating_add(replacement_len)
            .saturating_add(old_offset.saturating_sub(end))
    };
    byte_to_char_position(new_text, new_offset.min(new_text.len()))
}

pub(super) fn remap_text_position(
    old_text: &str,
    new_text: &str,
    position: EditorPosition,
) -> EditorPosition {
    if old_text == new_text {
        return position;
    }
    let mut start = 0;
    while start < old_text.len()
        && start < new_text.len()
        && old_text.as_bytes()[start] == new_text.as_bytes()[start]
    {
        start += 1;
    }
    while start > 0 && (!old_text.is_char_boundary(start) || !new_text.is_char_boundary(start)) {
        start -= 1;
    }
    let mut old_end = old_text.len();
    let mut new_end = new_text.len();
    while old_end > start
        && new_end > start
        && old_text.as_bytes()[old_end - 1] == new_text.as_bytes()[new_end - 1]
    {
        old_end -= 1;
        new_end -= 1;
    }
    while old_end < old_text.len()
        && (!old_text.is_char_boundary(old_end) || !new_text.is_char_boundary(new_end))
    {
        old_end += 1;
        new_end += 1;
    }
    remap_position(
        old_text,
        new_text,
        start,
        old_end,
        new_end.saturating_sub(start),
        position,
    )
}

fn char_position_to_byte(text: &str, position: EditorPosition) -> usize {
    text.split('\n')
        .take(position.line)
        .map(|line| line.len() + 1)
        .sum::<usize>()
        .saturating_add(
            text.split('\n')
                .nth(position.line)
                .unwrap_or_default()
                .char_indices()
                .nth(position.column)
                .map_or_else(
                    || {
                        text.split('\n')
                            .nth(position.line)
                            .unwrap_or_default()
                            .len()
                    },
                    |(offset, _)| offset,
                ),
        )
        .min(text.len())
}

fn byte_to_char_position(text: &str, offset: usize) -> EditorPosition {
    let offset = offset.min(text.len());
    let line = text[..offset].matches('\n').count();
    let line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    EditorPosition {
        line,
        column: text[line_start..offset].chars().count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(line: usize, column: usize) -> EditorPosition {
        EditorPosition { line, column }
    }

    #[test]
    fn walks_backward_and_forward_through_saved_positions() {
        let (a, b, c) = (position(1, 0), position(2, 1), position(3, 2));
        let mut history = JumpHistory::default();
        history.record_jump(a);
        history.record_jump(b);

        assert_eq!(history.backward(c, 1), Some(b));
        assert_eq!(history.backward(b, 1), Some(a));
        assert_eq!(history.forward(1), Some(b));
        assert_eq!(history.forward(1), Some(c));
        assert_eq!(history.forward(1), None);
    }

    #[test]
    fn new_jump_after_backward_discards_forward_branch() {
        let (a, b, c, d) = (
            position(1, 0),
            position(2, 0),
            position(3, 0),
            position(4, 0),
        );
        let mut history = JumpHistory::default();
        history.record_jump(a);
        history.record_jump(b);
        assert_eq!(history.backward(c, 1), Some(b));
        history.record_jump(b);
        assert_eq!(history.backward(d, 1), Some(b));
        assert_eq!(history.forward(1), Some(d));
    }

    #[test]
    fn ignores_adjacent_duplicates_and_clamps_counts() {
        let (a, b) = (position(1, 0), position(2, 0));
        let mut history = JumpHistory::default();
        history.record_jump(a);
        history.record_jump(a);
        assert_eq!(history.backward(b, 1000), Some(a));
        assert_eq!(history.forward(1000), Some(b));
        assert_eq!(history.backward(b, 0), None);
    }

    #[test]
    fn keeps_at_most_one_hundred_entries() {
        let mut history = JumpHistory::default();
        for line in 0..101 {
            history.record_jump(position(line, 0));
        }
        assert_eq!(history.entries.len(), 100);
        assert_eq!(
            history.backward(position(101, 0), 100),
            Some(position(2, 0))
        );
    }

    #[test]
    fn remaps_all_saved_positions() {
        let mut history = JumpHistory::default();
        history.record_jump(position(1, 0));
        history.record_jump(position(2, 0));
        history.remap(|cursor| position(cursor.line + 1, cursor.column + 2));
        assert_eq!(history.entries, vec![position(2, 2), position(3, 2)]);
    }

    #[test]
    fn remaps_positions_across_unicode_and_line_edits() {
        let old = "ab\n数据\nend";
        let new = "ab\n🙂数据\nend";
        assert_eq!(
            remap_position(old, new, 3, 3, 4, position(2, 1)),
            position(2, 1)
        );
        assert_eq!(
            remap_position(old, new, 3, 3, 4, position(1, 1)),
            position(1, 2)
        );
    }

    #[test]
    fn deleted_positions_converge_to_the_replacement_start() {
        let old = "first\nsecond\nthird";
        let new = "first\nthird";
        assert_eq!(
            remap_position(old, new, 6, 13, 0, position(1, 3)),
            position(1, 0)
        );
        assert_eq!(
            remap_position(old, new, 6, 13, 0, position(2, 0)),
            position(1, 0)
        );
    }

    #[test]
    fn infers_a_single_edit_from_before_and_after_text() {
        assert_eq!(
            remap_text_position("a\nb", "a\nnew\nb", position(1, 0)),
            position(2, 0)
        );
        assert_eq!(
            remap_text_position("数据", "🙂数据", position(0, 1)),
            position(0, 2)
        );
    }
}
