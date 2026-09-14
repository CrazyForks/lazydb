use ratatui::text::Line;

use crate::db::redis::read::{RedisPageValue, RedisValuePage};
use crate::value_preview::ValueView;

pub fn page_lines(page: &RedisValuePage) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(format!(
        "type={:?} ttl={} raw={}B",
        page.metadata.value_type,
        ttl_text(&page.metadata.ttl),
        page.raw_bytes
    ))];
    match &page.value {
        RedisPageValue::String(value) => lines.push(Line::from(display_bytes(value))),
        RedisPageValue::Hash(values) => lines.extend(values.iter().map(|(field, value)| {
            Line::from(format!(
                "{}\t{}",
                display_bytes(field),
                display_bytes(value)
            ))
        })),
        RedisPageValue::List(values) => lines.extend(
            values
                .iter()
                .map(|(index, value)| Line::from(format!("{index}\t{}", display_bytes(value)))),
        ),
        RedisPageValue::Set(values) => {
            lines.extend(values.iter().map(|value| Line::from(display_bytes(value))))
        }
        RedisPageValue::SortedSet(values) => lines.extend(values.iter().map(|(member, score)| {
            Line::from(format!(
                "{}\t{}",
                display_bytes(member),
                display_bytes(score)
            ))
        })),
        RedisPageValue::Stream(values) => lines.extend(values.iter().map(|(id, fields)| {
            Line::from(format!("{}\t{} fields", display_bytes(id), fields.len()))
        })),
    }
    if page.truncated {
        lines.push(Line::from("<page truncated>"));
    }
    lines
}

pub fn page_text(page: &RedisValuePage) -> String {
    let lines = match &page.value {
        RedisPageValue::String(value) => vec![display_bytes(value)],
        RedisPageValue::Hash(values) => values
            .iter()
            .map(|(field, value)| format!("{}\t{}", display_bytes(field), display_bytes(value)))
            .collect(),
        RedisPageValue::List(values) => values
            .iter()
            .map(|(index, value)| format!("{index}\t{}", display_bytes(value)))
            .collect(),
        RedisPageValue::Set(values) => values.iter().map(|value| display_bytes(value)).collect(),
        RedisPageValue::SortedSet(values) => values
            .iter()
            .map(|(member, score)| format!("{}\t{}", display_bytes(member), display_bytes(score)))
            .collect(),
        RedisPageValue::Stream(values) => values
            .iter()
            .map(|(id, fields)| format!("{}\t{} fields", display_bytes(id), fields.len()))
            .collect(),
    };
    lines.join("\n")
}

pub fn format_bytes(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return "—".into();
    };
    let value = bytes as f64;
    let (value, suffix) = if value >= 1024.0 * 1024.0 * 1024.0 {
        (value / (1024.0 * 1024.0 * 1024.0), "GB")
    } else if value >= 1024.0 * 1024.0 {
        (value / (1024.0 * 1024.0), "MB")
    } else if value >= 1024.0 {
        (value / 1024.0, "KB")
    } else {
        (value, "B")
    };
    if suffix == "B" {
        format!("{value:.0} {suffix}")
    } else {
        format!("{value:.2} {suffix}")
    }
}

pub fn format_ttl(ttl: &crate::db::redis::read::TtlState) -> String {
    match ttl {
        crate::db::redis::read::TtlState::Missing => "Missing".into(),
        crate::db::redis::read::TtlState::Persistent => "∞".into(),
        crate::db::redis::read::TtlState::Unavailable => "—".into(),
        crate::db::redis::read::TtlState::ExpiresIn { millis } => {
            if *millis < 1000 {
                return format!("{millis}ms");
            }
            let mut seconds = millis / 1000;
            let days = seconds / 86_400;
            seconds %= 86_400;
            let hours = seconds / 3_600;
            seconds %= 3_600;
            let minutes = seconds / 60;
            seconds %= 60;
            let mut result = String::new();
            if days > 0 {
                result.push_str(&format!("{days}d"));
            }
            if hours > 0 {
                result.push_str(&format!("{hours}h"));
            }
            if minutes > 0 {
                result.push_str(&format!("{minutes}m"));
            }
            if seconds > 0 || result.is_empty() {
                result.push_str(&format!("{seconds}s"));
            }
            result
        }
    }
}

pub fn format_page(page: &RedisValuePage, view: ValueView) -> Result<String, String> {
    match view {
        ValueView::Raw | ValueView::Table => Ok(page_text(page)),
        ValueView::Hex => Ok(page_bytes(page)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()),
        ValueView::Json => {
            let bytes = page_bytes(page);
            let value: serde_json::Value = serde_json::from_slice(bytes)
                .map_err(|error| format!("JSON parse error: {error}"))?;
            serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
        }
        ValueView::Yaml => {
            let value: serde_yaml::Value = serde_yaml::from_slice(page_bytes(page))
                .map_err(|error| format!("YAML parse error: {error}"))?;
            serde_yaml::to_string(&value).map_err(|error| error.to_string())
        }
    }
}

fn page_bytes(page: &RedisValuePage) -> &[u8] {
    match &page.value {
        RedisPageValue::String(value) => value,
        _ => &[],
    }
}

/// Format an individual Redis collection cell using the same byte-preserving
/// pipeline as a top-level value preview.
pub fn format_bytes_value(
    bytes: &[u8],
    format: crate::value_preview::PreviewFormat,
) -> Result<String, String> {
    use crate::value_preview::ValueEncoding;
    if !matches!(
        format.encoding,
        ValueEncoding::Text | ValueEncoding::Unknown
    ) {
        return match crate::value_preview::decode::decode(bytes, format) {
            Ok(crate::value_preview::decode::DecodedValue::Text(text)) => Ok(text),
            Ok(crate::value_preview::decode::DecodedValue::Bytes(value)) => {
                Ok(display_bytes(&value))
            }
            Err(error) => Err(error.to_string()),
        };
    }
    match format.view {
        ValueView::Raw => Ok(display_bytes(bytes)),
        ValueView::Hex => Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect()),
        ValueView::Json => {
            let value: serde_json::Value = serde_json::from_slice(bytes)
                .map_err(|error| format!("JSON parse error: {error}"))?;
            serde_json::to_string_pretty(&value).map_err(|error| error.to_string())
        }
        ValueView::Yaml => {
            let value: serde_yaml::Value = serde_yaml::from_slice(bytes)
                .map_err(|error| format!("YAML parse error: {error}"))?;
            serde_yaml::to_string(&value).map_err(|error| error.to_string())
        }
        ValueView::Table => Ok(display_bytes(bytes)),
    }
}

fn ttl_text(ttl: &crate::db::redis::read::TtlState) -> String {
    match ttl {
        crate::db::redis::read::TtlState::Missing => "missing".into(),
        crate::db::redis::read::TtlState::Persistent => "persistent".into(),
        crate::db::redis::read::TtlState::ExpiresIn { millis } => format!("{millis}ms"),
        crate::db::redis::read::TtlState::Unavailable => "unavailable".into(),
    }
}

fn display_bytes(value: &[u8]) -> String {
    match std::str::from_utf8(value) {
        Ok(text) => text.to_owned(),
        Err(_) => value.iter().map(|byte| format!("\\x{byte:02x}")).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::redis::{
        read::{RedisKeyMetadata, RedisPagePosition, RedisType, TtlState},
        types::{RedisKeyId, RedisTarget},
    };
    use uuid::Uuid;

    fn page(value: RedisPageValue) -> RedisValuePage {
        RedisValuePage {
            metadata: RedisKeyMetadata {
                key: RedisKeyId {
                    target: RedisTarget {
                        profile_id: Uuid::nil(),
                        database: 0,
                    },
                    key: b"key".to_vec(),
                },
                value_type: RedisType::Hash,
                ttl: TtlState::Unavailable,
                memory_usage_bytes: None,
                value_size: None,
            },
            position: RedisPagePosition::Complete,
            value,
            truncated: false,
            complete: true,
            raw_bytes: 5,
            formatted_bytes: 5,
        }
    }

    #[test]
    fn renders_binary_hash_values_without_loss() {
        let lines = page_lines(&page(RedisPageValue::Hash(vec![(vec![0], vec![255])])));
        assert!(lines.iter().any(|line| line.to_string().contains("\\x00")));
        assert!(lines.iter().any(|line| line.to_string().contains("\\xff")));
    }
}
