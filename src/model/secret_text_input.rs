use secrecy::{ExposeSecret, SecretString, zeroize::Zeroizing};

const SECRET_TEXT_HISTORY_LIMIT: usize = 20;

#[derive(Clone)]
struct SecretTextSnapshot {
    value: SecretString,
    cursor: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SecretEditGroup {
    Insert,
    Backspace,
    Delete,
}

#[derive(Clone, Default)]
struct SecretTextHistory {
    undo: Vec<SecretTextSnapshot>,
    redo: Vec<SecretTextSnapshot>,
    group: Option<(SecretEditGroup, SecretTextSnapshot)>,
}

#[derive(Clone)]
pub(crate) struct SecretTextInput {
    value: SecretString,
    cursor: usize,
    history: SecretTextHistory,
}

impl std::fmt::Debug for SecretTextInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl PartialEq for SecretTextInput {
    fn eq(&self, other: &Self) -> bool {
        self.value() == other.value() && self.cursor == other.cursor
    }
}

impl Eq for SecretTextInput {}

impl Default for SecretTextInput {
    fn default() -> Self {
        Self {
            value: SecretString::from(String::new()),
            cursor: 0,
            history: SecretTextHistory::default(),
        }
    }
}

impl SecretTextInput {
    pub(crate) fn value(&self) -> &str {
        self.value.expose_secret()
    }

    pub(crate) fn secret(&self) -> &SecretString {
        &self.value
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn set_cursor(&mut self, cursor: usize) {
        self.finish_edit_group();
        self.cursor = cursor.min(self.value().chars().count());
    }

    pub(crate) fn set(&mut self, value: impl Into<String>) {
        self.value = SecretString::from(value.into());
        self.cursor = self.value().chars().count();
        self.history = SecretTextHistory::default();
    }

    pub(crate) fn insert(&mut self, character: char) -> bool {
        let before = self.snapshot();
        let mut value = Zeroizing::new(self.value().to_owned());
        let offset = character_byte_index(&value, self.cursor);
        value.insert(offset, character);
        self.value = SecretString::from(std::mem::take(&mut *value));
        self.cursor += 1;
        self.record_grouped(SecretEditGroup::Insert, before);
        true
    }

    pub(crate) fn paste(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        let before = self.snapshot();
        let mut value = Zeroizing::new(self.value().to_owned());
        let offset = character_byte_index(&value, self.cursor);
        value.insert_str(offset, text);
        self.value = SecretString::from(std::mem::take(&mut *value));
        self.cursor += text.chars().count();
        self.record_atomic(before);
        true
    }

    pub(crate) fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let before = self.snapshot();
        let mut value = Zeroizing::new(self.value().to_owned());
        let start = character_byte_index(&value, self.cursor - 1);
        let end = character_byte_index(&value, self.cursor);
        value.replace_range(start..end, "");
        self.value = SecretString::from(std::mem::take(&mut *value));
        self.cursor -= 1;
        self.record_grouped(SecretEditGroup::Backspace, before);
        true
    }

    pub(crate) fn delete_previous_word(&mut self) -> bool {
        let before = self.snapshot();
        let mut value = Zeroizing::new(self.value().to_owned());
        let mut cursor = self.cursor;
        delete_previous_word(&mut value, &mut cursor);
        if cursor == self.cursor {
            return false;
        }
        self.value = SecretString::from(std::mem::take(&mut *value));
        self.cursor = cursor;
        self.record_atomic(before);
        true
    }

    pub(crate) fn delete_to_start(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let before = self.snapshot();
        let mut value = Zeroizing::new(self.value().to_owned());
        let end = character_byte_index(&value, self.cursor);
        value.replace_range(..end, "");
        self.value = SecretString::from(std::mem::take(&mut *value));
        self.cursor = 0;
        self.record_atomic(before);
        true
    }

    pub(crate) fn delete(&mut self) -> bool {
        let start = character_byte_index(self.value(), self.cursor);
        if start == self.value().len() {
            return false;
        }
        let before = self.snapshot();
        let mut value = Zeroizing::new(self.value().to_owned());
        let end = character_byte_index(&value, self.cursor + 1);
        value.replace_range(start..end, "");
        self.value = SecretString::from(std::mem::take(&mut *value));
        self.record_grouped(SecretEditGroup::Delete, before);
        true
    }

    pub(crate) fn move_left(&mut self) {
        self.finish_edit_group();
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub(crate) fn move_right(&mut self) {
        self.finish_edit_group();
        self.cursor = (self.cursor + 1).min(self.value().chars().count());
    }

    pub(crate) fn move_home(&mut self) {
        self.finish_edit_group();
        self.cursor = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.finish_edit_group();
        self.cursor = self.value().chars().count();
    }

    pub(crate) fn undo(&mut self) -> bool {
        self.finish_edit_group();
        let Some(previous) = self.history.undo.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.push_redo(current);
        self.restore(previous);
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        self.finish_edit_group();
        let Some(next) = self.history.redo.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.push_undo(current);
        self.restore(next);
        true
    }

    pub(crate) fn finish_edit_group(&mut self) {
        if let Some((_, start)) = self.history.group.take() {
            self.push_undo(start);
        }
    }

    fn snapshot(&self) -> SecretTextSnapshot {
        SecretTextSnapshot {
            value: self.value.clone(),
            cursor: self.cursor,
        }
    }

    fn restore(&mut self, snapshot: SecretTextSnapshot) {
        self.value = snapshot.value;
        self.cursor = snapshot.cursor.min(self.value().chars().count());
    }

    fn record_grouped(&mut self, group: SecretEditGroup, before: SecretTextSnapshot) {
        if !self
            .history
            .group
            .as_ref()
            .is_some_and(|(current, _)| *current == group)
        {
            self.finish_edit_group();
            self.history.group = Some((group, before));
        }
        self.history.redo.clear();
    }

    fn record_atomic(&mut self, before: SecretTextSnapshot) {
        self.finish_edit_group();
        self.push_undo(before);
        self.history.redo.clear();
    }

    fn push_undo(&mut self, snapshot: SecretTextSnapshot) {
        if self.history.undo.last().is_some_and(|last| {
            last.cursor == snapshot.cursor
                && last.value.expose_secret() == snapshot.value.expose_secret()
        }) {
            return;
        }
        if self.history.undo.len() == SECRET_TEXT_HISTORY_LIMIT {
            self.history.undo.remove(0);
        }
        self.history.undo.push(snapshot);
    }

    fn push_redo(&mut self, snapshot: SecretTextSnapshot) {
        if self.history.redo.len() == SECRET_TEXT_HISTORY_LIMIT {
            self.history.redo.remove(0);
        }
        self.history.redo.push(snapshot);
    }
}

fn character_byte_index(value: &str, character_index: usize) -> usize {
    value
        .char_indices()
        .nth(character_index)
        .map_or(value.len(), |(byte_index, _)| byte_index)
}

fn delete_previous_word(value: &mut String, cursor: &mut usize) {
    while *cursor > 0
        && value[..character_byte_index(value, *cursor)]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        let start = character_byte_index(value, *cursor - 1);
        let end = character_byte_index(value, *cursor);
        value.replace_range(start..end, "");
        *cursor -= 1;
    }
    while *cursor > 0
        && !value[..character_byte_index(value, *cursor)]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        let start = character_byte_index(value, *cursor - 1);
        let end = character_byte_index(value, *cursor);
        value.replace_range(start..end, "");
        *cursor -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::SecretTextInput;

    #[test]
    fn secret_input_supports_unicode_editing_and_redacted_debug() {
        let mut input = SecretTextInput::default();
        input.insert('密');
        input.insert('码');
        input.move_left();
        input.insert('X');

        assert_eq!(input.value(), "密X码");
        assert_eq!(input.cursor(), 2);
        assert_eq!(format!("{input:?}"), "[REDACTED]");
    }

    #[test]
    fn secret_input_undo_and_redo_restore_value_without_exposing_it() {
        let mut input = SecretTextInput::default();
        input.paste("secret");
        assert!(input.undo());
        assert_eq!(input.value(), "");
        assert!(input.redo());
        assert_eq!(input.value(), "secret");
        assert_eq!(format!("{input:?}"), "[REDACTED]");
    }
}
