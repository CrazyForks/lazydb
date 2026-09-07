use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde::de::IgnoredAny;

use crate::{db::value::CellValue, model::text_input::TextInput, profile::DatabaseKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellEditorKind {
    Boolean,
    Date,
    Time,
    DateTime,
    Timestamp,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ColumnEditorDescription {
    pub(crate) kind: CellEditorKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellEditorBuffer {
    Text(TextInput),
    Typed {
        kind: CellEditorKind,
        draft: TypedDraft,
    },
    Null(TextInput),
    Unprovided(TextInput),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypedDraft {
    Boolean(TextInput),
    Temporal(TemporalDraft),
    Json(JsonBuffer),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonBuffer {
    value: String,
    cursor: usize,
    sql_null: bool,
    history: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonTokenKind {
    String,
    Number,
    Boolean,
    Null,
    Punctuation,
    Whitespace,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonToken {
    pub start: usize,
    pub end: usize,
    pub kind: JsonTokenKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonValidationError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl JsonBuffer {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self {
            value,
            cursor,
            sql_null: false,
            history: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }
    pub fn is_sql_null(&self) -> bool {
        self.sql_null
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn line(&self) -> usize {
        self.value[..self.byte_index(self.cursor)]
            .matches('\n')
            .count()
    }
    pub fn column(&self) -> usize {
        self.value[..self.byte_index(self.cursor)]
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
    }

    pub fn insert(&mut self, character: char) {
        self.record();
        let index = self.byte_index(self.cursor);
        self.value.insert(index, character);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.record();
        let end = self.byte_index(self.cursor);
        let start = self.byte_index(self.cursor - 1);
        self.value.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.value.chars().count() {
            return;
        }
        self.record();
        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.value.replace_range(start..end, "");
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.value.chars().count());
    }
    pub fn move_home(&mut self) {
        self.cursor = self.line_start(self.cursor);
    }
    pub fn move_end(&mut self) {
        self.cursor = self.line_end(self.cursor);
    }
    pub fn move_up(&mut self) {
        self.move_vertical(-1);
    }
    pub fn move_down(&mut self) {
        self.move_vertical(1);
    }
    pub fn undo(&mut self) {
        if let Some((value, cursor)) = self.history.pop() {
            self.redo.push((self.value.clone(), self.cursor));
            self.value = value;
            self.cursor = cursor;
        }
    }
    pub fn redo(&mut self) {
        if let Some((value, cursor)) = self.redo.pop() {
            self.history.push((self.value.clone(), self.cursor));
            self.value = value;
            self.cursor = cursor;
        }
    }

    pub fn validate(&self) -> Result<(), JsonValidationError> {
        validate_json(self.value())
    }
    pub fn format(&self) -> Result<String, JsonValidationError> {
        format_json(self.value())
    }
    pub fn tokens(&self) -> Vec<JsonToken> {
        tokenize_json(self.value())
    }
    pub fn replace_value(&mut self, value: String) {
        self.value = value;
        self.cursor = self.value.chars().count();
    }

    fn record(&mut self) {
        self.history.push((self.value.clone(), self.cursor));
        self.redo.clear();
    }
    fn byte_index(&self, cursor: usize) -> usize {
        self.value
            .char_indices()
            .nth(cursor.min(self.value.chars().count()))
            .map_or(self.value.len(), |(index, _)| index)
    }
    fn line_start(&self, cursor: usize) -> usize {
        self.value[..self.byte_index(cursor)]
            .rfind('\n')
            .map_or(0, |index| self.value[..index].chars().count() + 1)
    }
    fn line_end(&self, cursor: usize) -> usize {
        self.value[self.byte_index(cursor)..].find('\n').map_or(
            self.value.chars().count(),
            |index| {
                cursor
                    + self.value[self.byte_index(cursor)..self.byte_index(cursor) + index]
                        .chars()
                        .count()
            },
        )
    }
    fn move_vertical(&mut self, direction: isize) {
        let column = self.column();
        let line = self.line().saturating_add_signed(direction);
        let start = self
            .value
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>();
        let target = self.value[start..].split('\n').next().unwrap_or_default();
        self.cursor = self.value[..start].chars().count() + column.min(target.chars().count());
    }
}

pub fn validate_json(value: &str) -> Result<(), JsonValidationError> {
    serde_json::from_str::<IgnoredAny>(value)
        .map(|_| ())
        .map_err(|error| {
            let line = error.line();
            let column = error.column();
            JsonValidationError {
                message: error.to_string(),
                line,
                column,
            }
        })
}

pub fn format_json(value: &str) -> Result<String, JsonValidationError> {
    validate_json(value)?;
    let tokens = tokenize_json(value)
        .into_iter()
        .filter(|token| token.kind != JsonTokenKind::Whitespace)
        .collect::<Vec<_>>();
    let mut output = String::new();
    let mut indent = 0usize;
    let mut line_start = true;
    for (position, token) in tokens.iter().enumerate() {
        let text = &value[token.start..token.end];
        match text {
            "{" | "[" => {
                output.push_str(text);
                indent += 1;
                if tokens.get(position + 1).is_some_and(|next| {
                    &value[next.start..next.end] != (if text == "{" { "}" } else { "]" })
                }) {
                    output.push('\n');
                    line_start = true;
                }
            }
            "}" | "]" => {
                indent = indent.saturating_sub(1);
                if !line_start {
                    output.push('\n');
                }
                output.push_str(&"  ".repeat(indent));
                output.push_str(text);
                line_start = false;
            }
            "," => {
                output.push(',');
                output.push('\n');
                line_start = true;
            }
            ":" => output.push_str(": "),
            _ => {
                if line_start {
                    output.push_str(&"  ".repeat(indent));
                    line_start = false;
                }
                output.push_str(text);
            }
        }
    }
    Ok(output)
}

pub fn tokenize_json(value: &str) -> Vec<JsonToken> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < value.len() {
        let start = index;
        let byte = value.as_bytes()[index];
        let (kind, end) = match byte {
            b' ' | b'\t' | b'\r' | b'\n' => (
                JsonTokenKind::Whitespace,
                value[index..]
                    .find(|c: char| !c.is_whitespace())
                    .map_or(value.len(), |offset| index + offset),
            ),
            b'"' => {
                let mut cursor = index + 1;
                let mut escaped = false;
                while cursor < value.len() {
                    let character = value.as_bytes()[cursor];
                    if !escaped && character == b'"' {
                        cursor += 1;
                        break;
                    }
                    escaped = !escaped && character == b'\\';
                    if character != b'\\' {
                        escaped = false;
                    }
                    cursor += 1;
                }
                (JsonTokenKind::String, cursor)
            }
            b'{' | b'}' | b'[' | b']' | b':' | b',' => (JsonTokenKind::Punctuation, index + 1),
            b'-' | b'0'..=b'9' => (
                JsonTokenKind::Number,
                value[index..]
                    .find(|c: char| {
                        !(c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
                    })
                    .map_or(value.len(), |offset| index + offset),
            ),
            _ if value[index..].starts_with("true") || value[index..].starts_with("false") => (
                JsonTokenKind::Boolean,
                index
                    + if value[index..].starts_with("true") {
                        4
                    } else {
                        5
                    },
            ),
            _ if value[index..].starts_with("null") => (JsonTokenKind::Null, index + 4),
            _ => (
                JsonTokenKind::Invalid,
                index + value[index..].chars().next().map_or(1, char::len_utf8),
            ),
        };
        tokens.push(JsonToken { start, end, kind });
        index = end.max(start + 1);
    }
    tokens
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemporalDraft {
    kind: CellEditorKind,
    input: TextInput,
    segment: usize,
    fraction_width: usize,
}

impl TemporalDraft {
    pub fn date(value: NaiveDate) -> Self {
        Self::new(CellEditorKind::Date, value.format("%Y-%m-%d").to_string())
    }

    pub fn from_time(value: NaiveTime) -> Self {
        Self::new(CellEditorKind::Time, format_time(value))
    }

    pub fn from_datetime(value: NaiveDateTime) -> Self {
        Self::new(CellEditorKind::DateTime, format_datetime(value))
    }

    pub fn from_timestamp(value: DateTime<FixedOffset>) -> Self {
        let mut draft = Self::new(CellEditorKind::Timestamp, format_timestamp(value));
        draft.fraction_width = value.nanosecond().to_string().len().max(1);
        draft
    }

    fn new(kind: CellEditorKind, value: String) -> Self {
        let fraction_width = value
            .split_once('.')
            .and_then(|(_, fraction)| fraction.split_once(' ').or(Some((fraction, ""))))
            .map(|(fraction, _)| fraction.len())
            .unwrap_or(0);
        Self {
            kind,
            input: TextInput::from(value),
            segment: 0,
            fraction_width,
        }
    }

    pub fn kind(&self) -> CellEditorKind {
        self.kind
    }

    pub fn input(&self) -> &TextInput {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut TextInput {
        &mut self.input
    }

    pub fn render(&self) -> &str {
        self.input.value()
    }

    pub fn set_segment(&mut self, segment: usize, value: &str) {
        self.segment = segment.min(self.segment_count().saturating_sub(1));
        let Some((start, end)) = self.segment_range(self.segment) else {
            return;
        };
        let width = end - start;
        let value = value.chars().take(width).collect::<String>();
        let mut text = self.input.value().to_owned();
        text.replace_range(start..end, &format!("{value:0>width$}"));
        self.input.set(text);
    }

    pub fn move_segment(&mut self, direction: isize) {
        self.segment = self
            .segment
            .saturating_add_signed(direction)
            .min(self.segment_count().saturating_sub(1));
        if let Some((start, _)) = self.segment_range(self.segment) {
            self.input.set_cursor(start);
        }
    }

    pub fn shift_month(&mut self, direction: isize) {
        if self.kind != CellEditorKind::Date
            && self.kind != CellEditorKind::DateTime
            && self.kind != CellEditorKind::Timestamp
        {
            return;
        }
        let date = match self.kind {
            CellEditorKind::Date => self.parse_date().ok(),
            CellEditorKind::DateTime => self.parse_datetime().ok().map(|value| value.date()),
            CellEditorKind::Timestamp => {
                self.parse_timestamp().ok().map(|value| value.date_naive())
            }
            _ => None,
        };
        let Some(date) = date else { return };
        let month = date.month0() as isize + direction;
        let year = date.year() + month.div_euclid(12) as i32;
        let month = month.rem_euclid(12) as u32 + 1;
        let day = date.day().min(days_in_month(year, month));
        let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
            return;
        };
        let value = match self.kind {
            CellEditorKind::Date => date.format("%Y-%m-%d").to_string(),
            CellEditorKind::DateTime => self
                .parse_datetime()
                .ok()
                .map(|value| NaiveDateTime::new(date, value.time()))
                .map(format_datetime)
                .unwrap_or_else(|| date.format("%Y-%m-%d").to_string()),
            CellEditorKind::Timestamp => self
                .parse_timestamp()
                .ok()
                .and_then(|value| value.with_year(year))
                .and_then(|value| value.with_month(month))
                .and_then(|value| value.with_day(day))
                .map(format_timestamp)
                .unwrap_or_else(|| date.format("%Y-%m-%d").to_string()),
            _ => return,
        };
        self.input.set(value);
    }

    pub fn insert(&mut self, character: char) {
        if !(character.is_ascii_digit()
            || self.kind == CellEditorKind::Timestamp && character == ':')
        {
            return;
        }
        let Some((start, end)) = self.segment_range(self.segment) else {
            return;
        };
        let mut text = self.input.value().to_owned();
        let cursor = self.input.cursor().clamp(start, end.saturating_sub(1));
        text.replace_range(cursor..cursor + 1, &character.to_string());
        self.input.set(text);
        self.input.set_cursor((cursor + 1).min(end));
    }

    pub fn parse(&self) -> Result<CellValue, String> {
        match self.kind {
            CellEditorKind::Date => self.parse_date().map(CellValue::Date),
            CellEditorKind::Time => self.parse_time().map(CellValue::Time),
            CellEditorKind::DateTime => self.parse_datetime().map(CellValue::DateTime),
            CellEditorKind::Timestamp => self.parse_timestamp().map(CellValue::Timestamp),
            _ => Err("not a temporal draft".into()),
        }
    }

    pub fn parse_date(&self) -> Result<NaiveDate, String> {
        NaiveDate::parse_from_str(self.render(), "%Y-%m-%d")
            .map_err(|_| "invalid date; expected YYYY-MM-DD".into())
    }

    pub fn parse_time(&self) -> Result<NaiveTime, String> {
        NaiveTime::parse_from_str(self.render(), "%H:%M:%S%.f")
            .map_err(|_| "invalid time; expected HH:MM:SS[.fraction]".into())
    }

    pub fn parse_datetime(&self) -> Result<NaiveDateTime, String> {
        NaiveDateTime::parse_from_str(self.render(), "%Y-%m-%d %H:%M:%S%.f")
            .map_err(|_| "invalid datetime; expected YYYY-MM-DD HH:MM:SS[.fraction]".into())
    }

    pub fn parse_timestamp(&self) -> Result<DateTime<FixedOffset>, String> {
        DateTime::parse_from_str(self.render(), "%Y-%m-%d %H:%M:%S%.f %:z")
            .map_err(|_| "invalid timestamp; expected YYYY-MM-DD HH:MM:SS[.fraction] ±HH:MM".into())
    }

    pub fn error(&self) -> Option<String> {
        self.parse().err()
    }

    pub fn calendar_label(&self) -> Option<String> {
        let date = match self.kind {
            CellEditorKind::Date => self.parse_date().ok(),
            CellEditorKind::DateTime => self.parse_datetime().ok().map(|value| value.date()),
            CellEditorKind::Timestamp => {
                self.parse_timestamp().ok().map(|value| value.date_naive())
            }
            CellEditorKind::Time => None,
            _ => None,
        }?;
        Some(date.format("%B %Y  [ and ] month").to_string())
    }

    fn segment_count(&self) -> usize {
        match self.kind {
            CellEditorKind::Date => 3,
            CellEditorKind::Time => 3,
            CellEditorKind::DateTime => 6,
            CellEditorKind::Timestamp => 8,
            _ => 0,
        }
    }

    fn segment_range(&self, segment: usize) -> Option<(usize, usize)> {
        let ranges = match self.kind {
            CellEditorKind::Date => vec![(0, 4), (5, 7), (8, 10)],
            CellEditorKind::Time => vec![(0, 2), (3, 5), (6, 8)],
            CellEditorKind::DateTime => vec![(0, 4), (5, 7), (8, 10), (11, 13), (14, 16), (17, 19)],
            CellEditorKind::Timestamp => {
                let fraction_end = if self.fraction_width == 0 {
                    19
                } else {
                    20 + self.fraction_width
                };
                let offset_start = if self.fraction_width == 0 {
                    20
                } else {
                    fraction_end + 1
                };
                vec![
                    (0, 4),
                    (5, 7),
                    (8, 10),
                    (11, 13),
                    (14, 16),
                    (17, 19),
                    (20, fraction_end),
                    (offset_start, offset_start + 6),
                ]
            }
            _ => Vec::new(),
        };
        ranges.get(segment).copied()
    }
}

impl Default for CellEditorBuffer {
    fn default() -> Self {
        Self::Text(TextInput::default())
    }
}

impl CellEditorBuffer {
    pub(crate) fn from_value(
        value: &CellValue,
        description: Option<ColumnEditorDescription>,
    ) -> Self {
        match value {
            CellValue::Null
                if description
                    .is_some_and(|description| description.kind == CellEditorKind::Json) =>
            {
                Self::Typed {
                    kind: CellEditorKind::Json,
                    draft: TypedDraft::Json(JsonBuffer {
                        sql_null: true,
                        ..JsonBuffer::new("null")
                    }),
                }
            }
            CellValue::Null => Self::Null(TextInput::from("null")),
            _ => {
                let input = TextInput::from(value.clipboard_text());
                match description {
                    Some(description) => Self::typed(description.kind, input, value),
                    None => Self::Text(input),
                }
            }
        }
    }

    pub(crate) fn unprovided() -> Self {
        Self::Unprovided(TextInput::default())
    }

    pub(crate) fn input(&self) -> Option<&TextInput> {
        match self {
            Self::Text(input) => Some(input),
            Self::Typed { draft, .. } => match draft {
                TypedDraft::Boolean(input) => Some(input),
                TypedDraft::Temporal(draft) => Some(draft.input()),
                TypedDraft::Json(_) => None,
            },
            Self::Null(input) => Some(input),
            Self::Unprovided(_) => None,
        }
    }

    pub(crate) fn input_mut(&mut self) -> &mut TextInput {
        if matches!(self, Self::Unprovided(_)) {
            *self = Self::Text(TextInput::default());
        }
        match self {
            Self::Text(input) => input,
            Self::Typed { draft, .. } => match draft {
                TypedDraft::Boolean(input) => input,
                TypedDraft::Temporal(draft) => draft.input_mut(),
                TypedDraft::Json(_) => unreachable!("JSON uses its multiline buffer API"),
            },
            Self::Null(input) | Self::Unprovided(input) => input,
        }
    }

    pub(crate) fn json_buffer(&self) -> Option<&JsonBuffer> {
        match self {
            Self::Typed {
                draft: TypedDraft::Json(buffer),
                ..
            } => Some(buffer),
            _ => None,
        }
    }

    pub(crate) fn json_buffer_mut(&mut self) -> Option<&mut JsonBuffer> {
        match self {
            Self::Typed {
                draft: TypedDraft::Json(buffer),
                ..
            } => Some(buffer),
            _ => None,
        }
    }

    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Text(input) | Self::Null(input) => Some(input.value()),
            Self::Typed { draft, .. } => Some(match draft {
                TypedDraft::Boolean(input) => input.value(),
                TypedDraft::Temporal(draft) => draft.render(),
                TypedDraft::Json(buffer) => buffer.value(),
            }),
            Self::Unprovided(_) => None,
        }
    }

    pub(crate) fn is_unprovided(&self) -> bool {
        matches!(self, Self::Unprovided(_))
    }

    pub(crate) fn boolean_selection(&self) -> Option<bool> {
        let Self::Typed {
            kind: CellEditorKind::Boolean,
            draft: TypedDraft::Boolean(input),
        } = self
        else {
            return None;
        };
        match input.value().trim().to_ascii_lowercase().as_str() {
            "true" | "t" => Some(true),
            "false" | "f" => Some(false),
            _ => None,
        }
    }

    pub(crate) fn set_boolean(&mut self, value: bool) {
        if let Self::Typed {
            kind: CellEditorKind::Boolean,
            draft: TypedDraft::Boolean(input),
        } = self
        {
            *input = TextInput::from(if value { "true" } else { "false" });
        }
    }

    pub(crate) fn move_boolean(&mut self, direction: isize) {
        if direction != 0
            && let Some(current) = self.boolean_selection()
        {
            self.set_boolean(!current);
        }
    }

    fn typed(kind: CellEditorKind, input: TextInput, value: &CellValue) -> Self {
        let draft = match (kind, value) {
            (CellEditorKind::Boolean, _) => TypedDraft::Boolean(input),
            (CellEditorKind::Json, CellValue::Text(value)) => {
                TypedDraft::Json(JsonBuffer::new(value.clone()))
            }
            (CellEditorKind::Json, _) => {
                TypedDraft::Json(JsonBuffer::new(input.value().to_owned()))
            }
            (CellEditorKind::Date, CellValue::Date(value)) => {
                TypedDraft::Temporal(TemporalDraft::date(*value))
            }
            (CellEditorKind::Time, CellValue::Time(value)) => {
                TypedDraft::Temporal(TemporalDraft::from_time(*value))
            }
            (CellEditorKind::DateTime, CellValue::DateTime(value)) => {
                TypedDraft::Temporal(TemporalDraft::from_datetime(*value))
            }
            (CellEditorKind::Timestamp, CellValue::Timestamp(value)) => {
                TypedDraft::Temporal(TemporalDraft::from_timestamp(*value))
            }
            _ => TypedDraft::Boolean(input),
        };
        Self::Typed { kind, draft }
    }

    pub(crate) fn temporal_move_to(&mut self, segment: isize) {
        if let Self::Typed {
            draft: TypedDraft::Temporal(draft),
            ..
        } = self
        {
            draft.move_segment(segment);
        }
    }

    pub(crate) fn temporal_shift_month(&mut self, direction: isize) {
        if let Self::Typed {
            draft: TypedDraft::Temporal(draft),
            ..
        } = self
        {
            draft.shift_month(direction);
        }
    }

    pub(crate) fn temporal_insert(&mut self, character: char) {
        if let Self::Typed {
            draft: TypedDraft::Temporal(draft),
            ..
        } = self
        {
            draft.insert(character);
        }
    }

    pub(crate) fn temporal_parse(&self) -> Option<Result<CellValue, String>> {
        match self {
            Self::Typed {
                draft: TypedDraft::Temporal(draft),
                ..
            } => Some(draft.parse()),
            _ => None,
        }
    }
}

pub(crate) fn classify_column_type(
    database_kind: DatabaseKind,
    type_name: &str,
) -> Option<ColumnEditorDescription> {
    let type_name = normalize_type_name(type_name);
    let base_name = type_name.split('(').next().unwrap_or(&type_name).trim();

    let kind = match database_kind {
        DatabaseKind::Postgres => match base_name {
            "bool" | "boolean" => Some(CellEditorKind::Boolean),
            "date" => Some(CellEditorKind::Date),
            "time" => Some(CellEditorKind::Time),
            "timestamp" => Some(CellEditorKind::DateTime),
            "timestamptz" => Some(CellEditorKind::Timestamp),
            "json" | "jsonb" => Some(CellEditorKind::Json),
            _ => None,
        },
        DatabaseKind::SqlServer => match base_name {
            "bit" => Some(CellEditorKind::Boolean),
            "date" => Some(CellEditorKind::Date),
            "time" => Some(CellEditorKind::Time),
            "datetime2" => Some(CellEditorKind::DateTime),
            "datetimeoffset" => Some(CellEditorKind::Timestamp),
            _ => None,
        },
        DatabaseKind::MySql | DatabaseKind::Sqlite => None,
    }?;

    Some(ColumnEditorDescription { kind })
}

fn normalize_type_name(type_name: &str) -> String {
    type_name.trim().to_ascii_lowercase()
}

fn format_time(value: NaiveTime) -> String {
    let base = value.format("%H:%M:%S").to_string();
    format_fraction(base, value.nanosecond())
}

fn format_datetime(value: NaiveDateTime) -> String {
    format_fraction(
        value.format("%Y-%m-%d %H:%M:%S").to_string(),
        value.nanosecond(),
    )
}

fn format_timestamp(value: DateTime<FixedOffset>) -> String {
    format!(
        "{} {}",
        format_fraction(
            value.format("%Y-%m-%d %H:%M:%S").to_string(),
            value.nanosecond()
        ),
        value.format("%:z")
    )
}

fn format_fraction(mut base: String, nanoseconds: u32) -> String {
    if nanoseconds != 0 {
        base.push('.');
        base.push_str(format!("{nanoseconds:09}").trim_end_matches('0'));
    }
    base
}

fn days_in_month(year: i32, month: u32) -> u32 {
    (28..=31)
        .rev()
        .find(|day| NaiveDate::from_ymd_opt(year, month, *day).is_some())
        .unwrap_or(28)
}

#[cfg(test)]
mod tests {
    use crate::db::value::CellValue;
    use crate::model::text_input::TextInput;
    use crate::profile::DatabaseKind;
    use chrono::{DateTime, NaiveDate, NaiveTime};

    use super::{
        CellEditorBuffer, CellEditorKind, JsonBuffer, JsonTokenKind, TemporalDraft,
        classify_column_type, format_json, validate_json,
    };

    #[test]
    fn json_buffer_supports_real_multiline_editing_and_cursor_movement() {
        let mut buffer = JsonBuffer::new("{\"name\": \"Ada\"}");
        buffer.move_home();
        buffer.insert('{');
        buffer.insert('\n');
        buffer.move_down();
        assert_eq!(buffer.line(), 1);
        assert!(buffer.value().contains('\n'));
    }

    #[test]
    fn json_validation_preserves_source_semantics_and_reports_line_and_column() {
        assert!(validate_json("{\"n\":90071992547409931234567890,\"x\":true,\"z\":null}").is_ok());
        let error = validate_json("{\n  \"name\": \"unterminated\n}").unwrap_err();
        assert_eq!((error.line, error.column), (2, 23));
    }

    #[test]
    fn json_formatting_does_not_change_invalid_source_and_handles_escaping_unicode_and_duplicates()
    {
        let source =
            r#"{"x":"\u263A","x":2,"big":90071992547409931234567890,"ok":false,"nil":null}"#;
        let formatted = format_json(source).unwrap();
        assert!(formatted.contains("90071992547409931234567890"));
        assert!(formatted.contains("\\u263A") || formatted.contains('☺'));
        assert!(format_json("{broken").is_err());
    }

    #[test]
    fn json_tokenizer_highlights_incomplete_strings_and_invalid_fragments() {
        let tokens = JsonBuffer::new("{\"key\": \"unfinished").tokens();
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == JsonTokenKind::String)
        );
        assert_eq!(
            tokens.first().map(|token| token.kind),
            Some(JsonTokenKind::Punctuation)
        );
    }

    #[test]
    fn temporal_draft_round_trips_fraction_precision_and_fixed_offset() {
        let timestamp = DateTime::parse_from_rfc3339("2026-08-28T10:20:31.120400+05:30").unwrap();
        let draft = TemporalDraft::from_timestamp(timestamp);
        assert_eq!(draft.render(), "2026-08-28 10:20:31.1204 +05:30");
        assert_eq!(draft.parse_timestamp().unwrap(), timestamp);

        let time = NaiveTime::parse_from_str("10:20:31.120400", "%H:%M:%S%.f").unwrap();
        assert_eq!(TemporalDraft::from_time(time).render(), "10:20:31.1204");
    }

    #[test]
    fn temporal_draft_rejects_impossible_segment_values() {
        let mut draft = TemporalDraft::date(NaiveDate::from_ymd_opt(2026, 8, 28).unwrap());
        draft.set_segment(1, "13");
        assert!(draft.parse_date().is_err());
        assert!(draft.error().is_some());
    }

    #[test]
    fn typed_temporal_buffer_edits_segments_instead_of_appending_to_formatted_text() {
        let value = CellValue::Date(NaiveDate::from_ymd_opt(2026, 8, 28).unwrap());
        let description = classify_column_type(DatabaseKind::Postgres, "date");
        let mut buffer = CellEditorBuffer::from_value(&value, description);
        buffer.temporal_move_to(1);
        buffer.temporal_insert('9');
        assert_eq!(buffer.value(), Some("2026-98-28"));
        assert!(!matches!(buffer, CellEditorBuffer::Text(_)));
    }

    #[test]
    fn classifies_supported_postgres_types() {
        let cases = [
            ("bool", CellEditorKind::Boolean),
            ("BOOLEAN", CellEditorKind::Boolean),
            ("date", CellEditorKind::Date),
            ("time(6)", CellEditorKind::Time),
            ("timestamp (3)", CellEditorKind::DateTime),
            ("timestamptz", CellEditorKind::Timestamp),
            ("json", CellEditorKind::Json),
            ("JSONB", CellEditorKind::Json),
        ];

        for (type_name, expected) in cases {
            assert_eq!(
                classify_column_type(DatabaseKind::Postgres, type_name)
                    .map(|description| description.kind),
                Some(expected),
                "type_name={type_name}"
            );
        }
    }

    #[test]
    fn classifies_supported_sql_server_types_without_treating_timestamp_as_temporal() {
        let cases = [
            ("bit", CellEditorKind::Boolean),
            ("DATE", CellEditorKind::Date),
            ("time(7)", CellEditorKind::Time),
            ("datetime2 (3)", CellEditorKind::DateTime),
            ("datetimeoffset", CellEditorKind::Timestamp),
        ];

        for (type_name, expected) in cases {
            assert_eq!(
                classify_column_type(DatabaseKind::SqlServer, type_name)
                    .map(|description| description.kind),
                Some(expected),
                "type_name={type_name}"
            );
        }

        for type_name in ["timestamp", "rowversion", "TIMESTAMP (8)", "ROWVERSION"] {
            assert_eq!(
                classify_column_type(DatabaseKind::SqlServer, type_name),
                None,
                "type_name={type_name}"
            );
        }
    }

    #[test]
    fn keeps_mysql_and_sqlite_type_inference_conservative() {
        for type_name in ["tinyint(1)", "TINYINT (1)", "boolean", "date", "json"] {
            assert_eq!(
                classify_column_type(DatabaseKind::MySql, type_name),
                None,
                "mysql type_name={type_name}"
            );
        }

        for type_name in ["1", "true", "2024-01-01", "text", "json"] {
            assert_eq!(
                classify_column_type(DatabaseKind::Sqlite, type_name),
                None,
                "sqlite type_name={type_name}"
            );
        }
    }

    #[test]
    fn editor_buffer_uses_complete_values_and_distinguishes_null_states() {
        let description = classify_column_type(DatabaseKind::Postgres, "text");
        assert_eq!(
            CellEditorBuffer::from_value(&CellValue::Text("NULL".into()), description),
            CellEditorBuffer::Text(TextInput::from("NULL"))
        );
        assert_eq!(
            CellEditorBuffer::from_value(&CellValue::Null, description),
            CellEditorBuffer::Null(TextInput::from("null"))
        );
        assert_eq!(
            CellEditorBuffer::unprovided(),
            CellEditorBuffer::Unprovided(TextInput::default())
        );
    }

    #[test]
    fn typing_into_null_or_unprovided_buffer_creates_an_empty_text_fallback() {
        let mut unprovided = CellEditorBuffer::unprovided();
        unprovided.input_mut().insert('x');
        assert_eq!(unprovided.value(), Some("x"));
    }

    #[test]
    fn boolean_buffer_exposes_selection_and_preserves_explicit_null_states() {
        let description = classify_column_type(DatabaseKind::Postgres, "boolean");
        let mut value = CellEditorBuffer::from_value(&CellValue::Boolean(true), description);
        assert_eq!(value.boolean_selection(), Some(true));
        value.set_boolean(false);
        assert_eq!(value.boolean_selection(), Some(false));

        assert_eq!(
            CellEditorBuffer::from_value(&CellValue::Null, description).boolean_selection(),
            None
        );
        assert!(CellEditorBuffer::unprovided().is_unprovided());
    }

    #[test]
    fn boolean_selection_cycles_left_and_right() {
        let description = classify_column_type(DatabaseKind::Postgres, "boolean");
        let mut value = CellEditorBuffer::from_value(&CellValue::Boolean(false), description);
        value.move_boolean(1);
        assert_eq!(value.boolean_selection(), Some(true));
        value.move_boolean(-1);
        assert_eq!(value.boolean_selection(), Some(false));
    }
}
