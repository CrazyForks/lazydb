use std::io::Cursor;

use jaded::{Content, Parser};
use serde_json::Value;

use super::{DecodeError, DecodeStatus};

pub fn is_java_serialization(data: &[u8]) -> bool {
    data.len() >= 4 && data[..4] == [0xAC, 0xED, 0x00, 0x05]
}

pub fn parse_java_to_json(data: &[u8]) -> Result<String, DecodeError> {
    if data.len() > super::MAX_PREVIEW_INPUT_BYTES {
        return Err(DecodeError::new(
            DecodeStatus::Unsupported,
            "Java value exceeds preview input budget",
        ));
    }
    if !is_java_serialization(data) {
        return Err(DecodeError::new(
            DecodeStatus::Invalid,
            "invalid Java stream header",
        ));
    }
    let mut parser = Parser::new(Cursor::new(data))
        .map_err(|error| DecodeError::new(DecodeStatus::Invalid, error.to_string()))?;
    let content = parser
        .read()
        .map_err(|error| DecodeError::new(DecodeStatus::Invalid, error.to_string()))?;
    let value = match content {
        Content::Object(value) => serde_json::to_value(value)
            .map_err(|error| DecodeError::new(DecodeStatus::Invalid, error.to_string()))?,
        Content::Block(bytes) => serde_json::json!({
            "type": "Block",
            "bytes": bytes.len(),
            "data": bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        }),
    };
    let output = serde_json::to_string_pretty(&extract_inner_value(value))
        .map_err(|error| DecodeError::new(DecodeStatus::Invalid, error.to_string()))?;
    if output.len() > super::MAX_PREVIEW_OUTPUT_BYTES {
        return Err(DecodeError::new(
            DecodeStatus::Unsupported,
            "Java preview exceeds output budget",
        ));
    }
    Ok(output)
}

fn extract_inner_value(value: Value) -> Value {
    match value {
        Value::Object(mut object) => {
            for key in ["Object", "JavaString", "Primitive", "Array"] {
                if let Some(inner) = object.remove(key) {
                    return extract_inner_value(inner);
                }
            }
            if let Some(inner) = object.remove("Enum") {
                return inner;
            }
            Value::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key, extract_inner_value(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.into_iter().map(extract_inner_value).collect()),
        other => other,
    }
}
