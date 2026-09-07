use ratatui::style::Color;
use serde::Deserialize;
use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::Theme;

const EXPECTED_VERSION: u64 = 1;
const MAX_SOURCE_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SourceError {
    Io(String),
    NotRegularFile,
    Oversized,
    Invalid(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "unable to read external theme: {error}"),
            Self::NotRegularFile => {
                write!(formatter, "external theme source is not a regular file")
            }
            Self::Oversized => write!(formatter, "external theme exceeds {MAX_SOURCE_BYTES} bytes"),
            Self::Invalid(error) => write!(formatter, "invalid external theme: {error}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SourceOutcome {
    Unchanged,
    ThemeChanged(Theme),
    Error(SourceError),
}

#[derive(Debug)]
pub(crate) struct ExternalThemeSource {
    path: PathBuf,
    last_bytes: Option<Vec<u8>>,
    last_effective_theme: Option<Theme>,
    last_reported_error: Option<SourceError>,
}

impl ExternalThemeSource {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            last_bytes: None,
            last_effective_theme: None,
            last_reported_error: None,
        }
    }

    pub(crate) async fn check(&mut self) -> SourceOutcome {
        let path = self.path.clone();
        let read = tokio::task::spawn_blocking(move || read_source(&path))
            .await
            .expect("external theme source reader task must not panic");

        match read {
            Ok(ReadResult::Bytes(bytes)) => self.apply_bytes(bytes),
            Ok(ReadResult::Error { error, bytes }) => {
                if let Some(bytes) = bytes {
                    if self.last_bytes.as_deref() == Some(bytes.as_slice()) {
                        return SourceOutcome::Unchanged;
                    }
                    self.last_bytes = Some(bytes);
                }
                self.report_error(error)
            }
            Err(error) => self.report_error(error),
        }
    }

    fn apply_bytes(&mut self, bytes: Vec<u8>) -> SourceOutcome {
        if self.last_bytes.as_deref() == Some(bytes.as_slice()) {
            if self.last_effective_theme.is_some() {
                self.last_reported_error = None;
            }
            return SourceOutcome::Unchanged;
        }

        self.last_bytes = Some(bytes.clone());
        match String::from_utf8(bytes)
            .map_err(|error| SourceError::Invalid(error.to_string()))
            .and_then(|input| parse(&input).map_err(SourceError::Invalid))
        {
            Ok(theme) => {
                self.last_reported_error = None;
                if self.last_effective_theme == Some(theme) {
                    SourceOutcome::Unchanged
                } else {
                    self.last_effective_theme = Some(theme);
                    SourceOutcome::ThemeChanged(theme)
                }
            }
            Err(error) => self.report_error(error),
        }
    }

    fn report_error(&mut self, error: SourceError) -> SourceOutcome {
        if self.last_reported_error.as_ref() == Some(&error) {
            SourceOutcome::Unchanged
        } else {
            self.last_reported_error = Some(error.clone());
            SourceOutcome::Error(error)
        }
    }
}

enum ReadResult {
    Bytes(Vec<u8>),
    Error {
        error: SourceError,
        bytes: Option<Vec<u8>>,
    },
}

fn read_source(path: &Path) -> Result<ReadResult, SourceError> {
    let metadata = std::fs::metadata(path).map_err(io_error)?;
    if !metadata.is_file() {
        return Ok(ReadResult::Error {
            error: SourceError::NotRegularFile,
            bytes: None,
        });
    }

    let mut file = File::open(path).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Ok(ReadResult::Error {
            error: SourceError::NotRegularFile,
            bytes: None,
        });
    }

    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Ok(ReadResult::Error {
            error: SourceError::Oversized,
            bytes: Some(bytes),
        });
    }
    Ok(ReadResult::Bytes(bytes))
}

fn io_error(error: io::Error) -> SourceError {
    SourceError::Io(error.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u64,
    colors: Colors,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Colors {
    background: String,
    surface: String,
    surface_raised: String,
    border: String,
    grid_header: String,
    grid_header_text: String,
    grid_border: String,
    text: String,
    muted: String,
    accent: String,
    action: String,
    syntax_relation: String,
    syntax_relation_alias: String,
    syntax_column: String,
    syntax_function: String,
    success: String,
    warning: String,
    error: String,
    selection: String,
    mouse_selection: String,
    row_updated: String,
    row_deleted: String,
    row_deleted_background: String,
    row_inserted: String,
    syntax_keyword: String,
    syntax_identifier: String,
    syntax_number: String,
    syntax_parameter: String,
    syntax_type: String,
    syntax_string: String,
    syntax_comment: String,
    syntax_operator: String,
    syntax_punctuation: String,
}

pub fn parse(input: &str) -> Result<Theme, String> {
    let document: Document = serde_json::from_str(input).map_err(|error| error.to_string())?;
    if document.version != EXPECTED_VERSION {
        return Err(format!("unsupported theme version {}", document.version));
    }

    let colors = document.colors;
    Ok(Theme {
        background: parse_color("background", &colors.background)?,
        surface: parse_color("surface", &colors.surface)?,
        surface_raised: parse_color("surface_raised", &colors.surface_raised)?,
        border: parse_color("border", &colors.border)?,
        grid_header: parse_color("grid_header", &colors.grid_header)?,
        grid_header_text: parse_color("grid_header_text", &colors.grid_header_text)?,
        grid_border: parse_color("grid_border", &colors.grid_border)?,
        text: parse_color("text", &colors.text)?,
        muted: parse_color("muted", &colors.muted)?,
        accent: parse_color("accent", &colors.accent)?,
        action: parse_color("action", &colors.action)?,
        syntax_relation: parse_color("syntax_relation", &colors.syntax_relation)?,
        syntax_relation_alias: parse_color("syntax_relation_alias", &colors.syntax_relation_alias)?,
        syntax_column: parse_color("syntax_column", &colors.syntax_column)?,
        syntax_function: parse_color("syntax_function", &colors.syntax_function)?,
        success: parse_color("success", &colors.success)?,
        warning: parse_color("warning", &colors.warning)?,
        error: parse_color("error", &colors.error)?,
        selection: parse_color("selection", &colors.selection)?,
        mouse_selection: parse_color("mouse_selection", &colors.mouse_selection)?,
        row_updated: parse_color("row_updated", &colors.row_updated)?,
        row_deleted: parse_color("row_deleted", &colors.row_deleted)?,
        row_deleted_background: parse_color(
            "row_deleted_background",
            &colors.row_deleted_background,
        )?,
        row_inserted: parse_color("row_inserted", &colors.row_inserted)?,
        syntax_keyword: parse_color("syntax_keyword", &colors.syntax_keyword)?,
        syntax_identifier: parse_color("syntax_identifier", &colors.syntax_identifier)?,
        syntax_number: parse_color("syntax_number", &colors.syntax_number)?,
        syntax_parameter: parse_color("syntax_parameter", &colors.syntax_parameter)?,
        syntax_type: parse_color("syntax_type", &colors.syntax_type)?,
        syntax_string: parse_color("syntax_string", &colors.syntax_string)?,
        syntax_comment: parse_color("syntax_comment", &colors.syntax_comment)?,
        syntax_operator: parse_color("syntax_operator", &colors.syntax_operator)?,
        syntax_punctuation: parse_color("syntax_punctuation", &colors.syntax_punctuation)?,
    })
}

fn parse_color(name: &str, value: &str) -> Result<Color, String> {
    if value == "default" {
        return Ok(Color::Reset);
    }
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return Err(format!("invalid color for {name}"));
    }
    let channels = (1..7)
        .step_by(2)
        .map(|start| u8::from_str_radix(&value[start..start + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("invalid color for {name}"))?;
    Ok(Color::Rgb(channels[0], channels[1], channels[2]))
}

#[cfg(test)]
mod external_theme_tests {
    use ratatui::style::Color;
    use serde_json::{Value, json};

    use super::parse;
    use crate::ui::theme::SyntaxColor;

    pub(super) fn fixture() -> Value {
        json!({
            "version": 1,
            "colors": {
                "background": "#010203", "surface": "#040506", "surface_raised": "#070809",
                "border": "#0A0B0C", "grid_header": "#0D0E0F", "grid_header_text": "#101112",
                "grid_border": "#131415", "text": "#161718", "muted": "#191A1B",
                "accent": "#1C1D1E", "action": "#1F2021", "syntax_relation": "#222324",
                "syntax_relation_alias": "#252627", "syntax_column": "#28292A", "syntax_function": "#2B2C2D",
                "success": "#2E2F30", "warning": "#313233", "error": "#343536",
                "selection": "#373839", "mouse_selection": "#3A3B3C", "row_updated": "#3D3E3F",
                "row_deleted": "#404142", "row_deleted_background": "#434445", "row_inserted": "#464748",
                "syntax_keyword": "#494A4B", "syntax_identifier": "#4C4D4E", "syntax_number": "#4F5051",
                "syntax_parameter": "#525354", "syntax_type": "#555657", "syntax_string": "#58595A",
                "syntax_comment": "#5B5C5D", "syntax_operator": "#5E5F60", "syntax_punctuation": "#616263"
            }
        })
    }

    #[test]
    fn external_theme_accepts_complete_rgb_snapshot() {
        let theme = parse(&serde_json::to_string(&fixture()).unwrap()).unwrap();
        assert_eq!(theme.background, Color::Rgb(1, 2, 3));
        assert_eq!(
            theme.syntax_color(SyntaxColor::Keyword),
            Color::Rgb(73, 74, 75)
        );
        assert_eq!(
            theme.syntax_color(SyntaxColor::Punctuation),
            Color::Rgb(97, 98, 99)
        );
    }

    #[test]
    fn external_theme_preserves_default_color() {
        let mut fixture = fixture();
        fixture["colors"]["background"] = json!("default");
        fixture["colors"]["syntax_string"] = json!("default");
        let theme = parse(&serde_json::to_string(&fixture).unwrap()).unwrap();
        assert_eq!(theme.background, Color::Reset);
        assert_eq!(theme.syntax_string, Color::Reset);
    }

    #[test]
    fn external_theme_rejects_missing_and_unknown_fields() {
        let mut missing = fixture();
        missing["colors"].as_object_mut().unwrap().remove("text");
        assert!(parse(&serde_json::to_string(&missing).unwrap()).is_err());

        let mut unknown = fixture();
        unknown["colors"]["unexpected"] = json!("#000000");
        assert!(parse(&serde_json::to_string(&unknown).unwrap()).is_err());
    }

    #[test]
    fn external_theme_rejects_invalid_version_and_colors() {
        let mut version = fixture();
        version["version"] = json!(2);
        assert!(parse(&serde_json::to_string(&version).unwrap()).is_err());

        for value in ["#12345", "123456", "#GG0000", "null"] {
            let mut invalid = fixture();
            invalid["colors"]["accent"] = json!(value);
            assert!(parse(&serde_json::to_string(&invalid).unwrap()).is_err());
        }
    }
}

#[cfg(test)]
mod external_theme_source_tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{ExternalThemeSource, SourceError, SourceOutcome, external_theme_tests};
    use crate::ui::theme::Theme;

    fn write_fixture(path: &std::path::Path) {
        fs::write(
            path,
            serde_json::to_vec(&external_theme_tests::fixture()).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn source_loads_first_snapshot_and_ignores_unchanged_bytes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("theme.json");
        write_fixture(&path);
        let mut source = ExternalThemeSource::new(&path);

        assert!(matches!(
            source.check().await,
            SourceOutcome::ThemeChanged(_)
        ));
        assert_eq!(source.check().await, SourceOutcome::Unchanged);
    }

    #[tokio::test]
    async fn source_detects_same_size_raw_byte_changes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("theme.json");
        write_fixture(&path);
        let mut source = ExternalThemeSource::new(&path);
        assert!(matches!(
            source.check().await,
            SourceOutcome::ThemeChanged(_)
        ));

        let mut changed = external_theme_tests::fixture();
        changed["colors"]["accent"] = json!("#1C1D1F");
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();

        assert!(matches!(
            source.check().await,
            SourceOutcome::ThemeChanged(_)
        ));
    }

    #[tokio::test]
    async fn source_compares_effective_theme_after_raw_change() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("theme.json");
        write_fixture(&path);
        let mut source = ExternalThemeSource::new(&path);
        assert!(matches!(
            source.check().await,
            SourceOutcome::ThemeChanged(_)
        ));

        let mut equivalent =
            serde_json::to_string_pretty(&external_theme_tests::fixture()).unwrap();
        equivalent.push('\n');
        fs::write(&path, equivalent).unwrap();

        assert_eq!(source.check().await, SourceOutcome::Unchanged);
    }

    #[tokio::test]
    async fn source_deduplicates_invalid_errors_and_recovers() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("theme.json");
        fs::write(&path, b"{}").unwrap();
        let mut source = ExternalThemeSource::new(&path);

        assert!(matches!(
            source.check().await,
            SourceOutcome::Error(SourceError::Invalid(_))
        ));
        assert_eq!(source.check().await, SourceOutcome::Unchanged);

        write_fixture(&path);
        assert!(matches!(
            source.check().await,
            SourceOutcome::ThemeChanged(_)
        ));
    }

    #[tokio::test]
    async fn source_deduplicates_deletion_and_recovers_after_recreation() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("theme.json");
        write_fixture(&path);
        let mut source = ExternalThemeSource::new(&path);
        assert!(matches!(
            source.check().await,
            SourceOutcome::ThemeChanged(_)
        ));

        fs::remove_file(&path).unwrap();
        assert!(matches!(
            source.check().await,
            SourceOutcome::Error(SourceError::Io(_))
        ));
        assert_eq!(source.check().await, SourceOutcome::Unchanged);

        write_fixture(&path);
        assert!(matches!(source.check().await, SourceOutcome::Unchanged));
    }

    #[tokio::test]
    async fn source_rejects_oversized_input_and_non_regular_paths() {
        let directory = tempdir().unwrap();
        let oversized = directory.path().join("oversized.json");
        fs::write(&oversized, vec![b'x'; 16 * 1024 + 1]).unwrap();
        let mut oversized_source = ExternalThemeSource::new(&oversized);
        assert_eq!(
            oversized_source.check().await,
            SourceOutcome::Error(SourceError::Oversized)
        );

        let mut directory_source = ExternalThemeSource::new(directory.path());
        assert_eq!(
            directory_source.check().await,
            SourceOutcome::Error(SourceError::NotRegularFile)
        );
    }

    #[test]
    fn source_theme_fallback_type_remains_comparable() {
        assert_eq!(Theme::deep_space(), Theme::default());
    }
}
