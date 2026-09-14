use ratatui::text::Line;

use crate::db::redis::read::{RedisPageValue, RedisValuePage};

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
    page_lines(page)
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
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
    if value
        .iter()
        .all(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace())
    {
        String::from_utf8_lossy(value).into_owned()
    } else {
        value.iter().map(|byte| format!("\\x{byte:02x}")).collect()
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
