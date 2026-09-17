//! Lossless, bounded value preview primitives.
//!
//! Redis values remain bytes until a view explicitly asks for a text or
//! structured projection.  This keeps format detection independent from the
//! way a value happens to be rendered in the TUI.

pub mod cache;
pub mod decode;
pub mod detect;
pub mod edit;
pub mod java;
pub mod php;
pub mod pickle;
pub mod protobuf;
pub mod table;

use std::fmt;

pub const MAX_PREVIEW_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PREVIEW_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ValueEncoding {
    Text,
    Java,
    Php,
    Pickle,
    Protobuf,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ValueView {
    Raw,
    Json,
    Yaml,
    Table,
    Hex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeStatus {
    Complete,
    NeedsMoreData,
    Unsupported,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PreviewFormat {
    pub encoding: ValueEncoding,
    pub view: ValueView,
}

impl PreviewFormat {
    pub const RAW: Self = Self {
        encoding: ValueEncoding::Text,
        view: ValueView::Raw,
    };

    pub const HEX: Self = Self {
        encoding: ValueEncoding::Unknown,
        view: ValueView::Hex,
    };

    pub const JSON: Self = Self {
        encoding: ValueEncoding::Text,
        view: ValueView::Json,
    };

    pub const YAML: Self = Self {
        encoding: ValueEncoding::Text,
        view: ValueView::Yaml,
    };

    pub const TABLE: Self = Self {
        encoding: ValueEncoding::Text,
        view: ValueView::Table,
    };

    pub const fn preset(encoding: ValueEncoding) -> Self {
        Self {
            encoding,
            view: ValueView::Json,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodeError {
    pub status: DecodeStatus,
    pub message: String,
    pub offset: Option<usize>,
}

impl DecodeError {
    pub fn new(status: DecodeStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            offset: None,
        }
    }

    pub fn at(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(offset) = self.offset {
            write!(formatter, "{} at byte {}", self.message, offset)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for DecodeError {}

/// A format candidate returned by detection.  A candidate is deliberately
/// separate from the selected format so low-confidence binary guesses do not
/// unexpectedly change the user's view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatCandidate {
    pub format: PreviewFormat,
    pub confidence: u8,
    pub reason: &'static str,
    pub status: DecodeStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_and_view_are_independent() {
        let java_yaml = PreviewFormat {
            encoding: ValueEncoding::Java,
            view: ValueView::Yaml,
        };
        assert_eq!(java_yaml.encoding, ValueEncoding::Java);
        assert_eq!(java_yaml.view, ValueView::Yaml);
        assert_eq!(
            PreviewFormat::preset(ValueEncoding::Php).view,
            ValueView::Json
        );
    }

    #[test]
    fn decode_errors_keep_status_and_optional_offset() {
        let error = DecodeError::new(DecodeStatus::NeedsMoreData, "truncated JSON").at(8);
        assert_eq!(error.to_string(), "truncated JSON at byte 8");
    }
}
