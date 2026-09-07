use std::fmt::Write;

use percent_encoding::percent_decode_str;
use secrecy::{ExposeSecret, SecretString};
use unicode_width::UnicodeWidthChar;
use url::Url;

/// A secret that is safe to carry through cloned runtime commands.
/// Equality is intentionally non-observing so application state cannot become
/// a password oracle; the value is only exposed at the execution boundary.
#[derive(Clone)]
pub struct RedactedSecret(SecretString);

impl RedactedSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }
    pub fn is_empty(&self) -> bool {
        self.0.expose_secret().is_empty()
    }
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl std::fmt::Debug for RedactedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl PartialEq for RedactedSecret {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for RedactedSecret {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayLineProjection {
    pub text: String,
    pub source_to_display_cells: Vec<usize>,
}

pub fn sanitize_terminal_text(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\n' | '\t' => sanitized.push(character),
            '\u{1b}' => sanitized.push_str("<ESC>"),
            '\r' => sanitized.push_str("<CR>"),
            value if value.is_control() => {
                let _ = write!(sanitized, "<0x{:02X}>", value as u32);
            }
            value => sanitized.push(value),
        }
    }
    sanitized
}

pub fn project_editor_line(value: &str) -> DisplayLineProjection {
    let mut text = String::with_capacity(value.len());
    let mut source_to_display_cells = Vec::with_capacity(value.chars().count() + 1);
    let mut cells = 0;
    source_to_display_cells.push(0);
    for character in value.chars() {
        match character {
            '\t' => {
                let spaces = 4 - (cells % 4);
                text.extend(std::iter::repeat_n(' ', spaces));
                cells += spaces;
            }
            '\n' => text.push_str("<LF>"),
            '\r' => text.push_str("<CR>"),
            '\u{1b}' => text.push_str("<ESC>"),
            value if value.is_control() => {
                let _ = write!(text, "<0x{:02X}>", value as u32);
            }
            value => text.push(value),
        }
        cells += match character {
            '\t' => 0,
            '\n' | '\r' => 4,
            '\u{1b}' => 5,
            value if value.is_control() => format!("<0x{:02X}>", value as u32).len(),
            value => value.width().unwrap_or(0),
        };
        source_to_display_cells.push(cells);
    }
    DisplayLineProjection {
        text,
        source_to_display_cells,
    }
}

pub fn redact_connection_string(value: &str) -> String {
    let (prefix, raw) = if let Some(raw) = value.strip_prefix("jdbc:") {
        ("jdbc:", raw)
    } else {
        ("", value)
    };

    let Ok(mut url) = Url::parse(raw) else {
        return redact_fallback(value);
    };

    if url.password().is_some() {
        let _ = url.set_password(Some("***"));
    }

    redact_query_credentials(&format!("{prefix}{url}"), "***")
}

fn is_password_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("password")
        || key.eq_ignore_ascii_case("passwd")
        || key.eq_ignore_ascii_case("pwd")
}

fn redact_fallback(value: &str) -> String {
    redact_query_credentials(value, "***")
}

pub fn redact_url_query_credentials(value: &str) -> String {
    redact_query_credentials(value, "[REDACTED]")
}

fn redact_query_credentials(value: &str, replacement: &str) -> String {
    let Some(query_start) = value.find('?') else {
        return redact_properties(value, replacement);
    };
    let (prefix, query) = value.split_at(query_start + 1);
    format!("{prefix}{}", redact_query_parts(query, replacement))
}

fn redact_query_parts(query: &str, replacement: &str) -> String {
    query
        .split_inclusive(['&', ';'])
        .map(|part| {
            let (body, delimiter) = part.split_at(
                part.len()
                    .saturating_sub(part.ends_with(['&', ';']) as usize),
            );
            let Some((key, _)) = body.split_once('=') else {
                return part.to_owned();
            };
            let decoded_key = percent_decode_str(key).decode_utf8_lossy();
            if is_password_key(&decoded_key) {
                format!("{key}={replacement}{delimiter}")
            } else {
                part.to_owned()
            }
        })
        .collect()
}

fn redact_properties(value: &str, replacement: &str) -> String {
    value
        .split_inclusive(';')
        .map(|part| {
            let (body, delimiter) =
                part.split_at(part.len().saturating_sub(part.ends_with(';') as usize));
            let Some((key, _)) = body.split_once('=') else {
                return part.to_owned();
            };
            if is_password_key(key.trim_start_matches(';')) {
                format!(
                    "{}={replacement}{delimiter}",
                    &body[..body.find('=').unwrap_or(0)]
                )
            } else {
                part.to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{redact_connection_string, sanitize_terminal_text};

    #[test]
    fn removes_terminal_control_sequences() {
        let hostile = "safe\u{1b}]52;c;dGVzdA==\u{7} text\u{1b}[2J";
        let sanitized = sanitize_terminal_text(hostile);

        assert!(!sanitized.contains('\u{1b}'));
        assert!(!sanitized.contains('\u{7}'));
        assert!(sanitized.contains("<ESC>"));
        assert!(sanitized.contains("<0x07>"));
    }

    #[test]
    fn preserves_safe_newlines_and_tabs() {
        assert_eq!(sanitize_terminal_text("a\tb\nc"), "a\tb\nc");
    }

    #[test]
    fn redacts_url_and_query_passwords() {
        let redacted = redact_connection_string(
            "jdbc:postgresql://alice:secret@db.example.com/app?password=also-secret&sslmode=require",
        );

        assert!(!redacted.contains("secret"));
        assert!(redacted.contains("***"));
        assert!(redacted.starts_with("jdbc:"));
    }

    #[test]
    fn redacts_query_password_aliases_delimiters_and_encoded_keys() {
        for input in [
            "postgres://user:secret@db/app?password=one&passwd=two;pwd=three&sslmode=require",
            "jdbc:sqlserver://db;user=alice;password=secret;PWD=other",
            "postgres://db/app?%70%61%73%73%77%6f%72%64=encoded-secret",
        ] {
            let redacted = redact_connection_string(input);
            assert!(!redacted.contains("secret"));
            assert!(!redacted.contains("one"));
            assert!(!redacted.contains("two"));
            assert!(!redacted.contains("three"));
            assert!(!redacted.contains("encoded-secret"));
        }
    }
}
