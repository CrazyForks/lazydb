use serde_json::{Map, Value};

use super::{DecodeError, DecodeStatus};

#[derive(Debug, Clone, PartialEq)]
enum PhpValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Vec<u8>),
    Array(Vec<(PhpValue, PhpValue)>),
    Object {
        class: Vec<u8>,
        properties: Vec<(PhpValue, PhpValue)>,
    },
    Reference(i64),
}

pub fn is_php_serialization(data: &[u8]) -> bool {
    matches!(
        data.first(),
        Some(b'a' | b'b' | b'd' | b'i' | b'N' | b's' | b'O' | b'C' | b'r' | b'R')
    ) && (data.get(1) == Some(&b':') || data.first() == Some(&b'N'))
}

pub fn parse_php_to_json(data: &[u8]) -> Result<String, DecodeError> {
    if data.len() > super::MAX_PREVIEW_INPUT_BYTES {
        return Err(DecodeError::new(
            DecodeStatus::Unsupported,
            "PHP value exceeds preview input budget",
        ));
    }
    let (value, offset) = parse_value(data, 0)?;
    if data[offset..]
        .iter()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        return Err(DecodeError::new(DecodeStatus::Invalid, "trailing PHP data").at(offset));
    }
    let output = serde_json::to_string_pretty(&to_json(value))
        .map_err(|error| DecodeError::new(DecodeStatus::Invalid, error.to_string()))?;
    if output.len() > super::MAX_PREVIEW_OUTPUT_BYTES {
        return Err(DecodeError::new(
            DecodeStatus::Unsupported,
            "PHP preview exceeds output budget",
        ));
    }
    Ok(output)
}

fn parse_value(data: &[u8], mut offset: usize) -> Result<(PhpValue, usize), DecodeError> {
    let kind = *data.get(offset).ok_or_else(|| truncated(offset))?;
    offset += 1;
    match kind {
        b'N' => expect(data, offset, b';').map(|offset| (PhpValue::Null, offset)),
        b'b' => {
            let (token, offset) = field(data, offset, b';')?;
            match token {
                b"0" => Ok((PhpValue::Bool(false), offset)),
                b"1" => Ok((PhpValue::Bool(true), offset)),
                _ => invalid(offset, "invalid PHP bool"),
            }
        }
        b'i' => {
            let (token, offset) = field(data, offset, b';')?;
            let value = std::str::from_utf8(token)
                .ok()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| invalid_error(offset, "invalid PHP integer"))?;
            Ok((PhpValue::Int(value), offset))
        }
        b'd' => {
            let (token, offset) = field(data, offset, b';')?;
            let value = std::str::from_utf8(token)
                .ok()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| invalid_error(offset, "invalid PHP float"))?;
            Ok((PhpValue::Float(value), offset))
        }
        b's' => parse_string(data, offset),
        b'a' => parse_array(data, offset),
        b'O' => parse_object(data, offset),
        b'r' | b'R' => {
            let (token, offset) = field(data, offset, b';')?;
            let value = std::str::from_utf8(token)
                .ok()
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| invalid_error(offset, "invalid PHP reference"))?;
            Ok((PhpValue::Reference(value), offset))
        }
        _ => {
            Err(DecodeError::new(DecodeStatus::Unsupported, "unsupported PHP type").at(offset - 1))
        }
    }
}

fn parse_string(data: &[u8], offset: usize) -> Result<(PhpValue, usize), DecodeError> {
    let (length, mut offset) = length_prefix(data, offset)?;
    offset = expect(data, offset, b'"')?;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid_error(offset, "PHP string length overflow"))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| truncated(offset))?
        .to_vec();
    offset = expect(data, end, b'"')?;
    offset = expect(data, offset, b';')?;
    Ok((PhpValue::String(bytes), offset))
}

fn parse_array(data: &[u8], offset: usize) -> Result<(PhpValue, usize), DecodeError> {
    let (count, mut offset) = length_prefix(data, offset)?;
    offset = expect(data, offset, b'{')?;
    let mut items = Vec::with_capacity(count.min(10_000));
    for _ in 0..count {
        let (key, next) = parse_value(data, offset)?;
        let (value, next) = parse_value(data, next)?;
        items.push((key, value));
        offset = next;
    }
    Ok((PhpValue::Array(items), expect(data, offset, b'}')?))
}

fn parse_object(data: &[u8], offset: usize) -> Result<(PhpValue, usize), DecodeError> {
    let (class, mut offset) = parse_string_payload(data, offset)?;
    offset = expect(data, offset, b':')?;
    let (count, next) = length_prefix(data, offset)?;
    offset = expect(data, next, b'{')?;
    let mut properties = Vec::with_capacity(count.min(10_000));
    for _ in 0..count {
        let (key, next) = parse_value(data, offset)?;
        let (value, next) = parse_value(data, next)?;
        properties.push((key, value));
        offset = next;
    }
    Ok((
        PhpValue::Object { class, properties },
        expect(data, offset, b'}')?,
    ))
}

fn length_prefix(data: &[u8], offset: usize) -> Result<(usize, usize), DecodeError> {
    let (token, offset) = field(data, offset, b':')?;
    let length = std::str::from_utf8(token)
        .ok()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| invalid_error(offset, "invalid PHP length"))?;
    Ok((length, offset))
}

fn parse_string_payload(data: &[u8], offset: usize) -> Result<(Vec<u8>, usize), DecodeError> {
    let (length, mut offset) = length_prefix(data, offset)?;
    offset = expect(data, offset, b'"')?;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid_error(offset, "PHP string length overflow"))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| truncated(offset))?
        .to_vec();
    offset = expect(data, end, b'"')?;
    Ok((bytes, offset))
}

fn field(data: &[u8], offset: usize, delimiter: u8) -> Result<(&[u8], usize), DecodeError> {
    let offset = expect(data, offset, b':')?;
    let end = data[offset..]
        .iter()
        .position(|byte| *byte == delimiter)
        .map(|index| offset + index)
        .ok_or_else(|| truncated(offset))?;
    Ok((&data[offset..end], end + 1))
}

fn expect(data: &[u8], offset: usize, expected: u8) -> Result<usize, DecodeError> {
    match data.get(offset) {
        Some(value) if *value == expected => Ok(offset + 1),
        Some(_) => Err(invalid_error(offset, "unexpected PHP delimiter")),
        None => Err(truncated(offset)),
    }
}

fn truncated(offset: usize) -> DecodeError {
    DecodeError::new(DecodeStatus::NeedsMoreData, "truncated PHP value").at(offset)
}
fn invalid_error(offset: usize, message: &'static str) -> DecodeError {
    DecodeError::new(DecodeStatus::Invalid, message).at(offset)
}
fn invalid<T>(offset: usize, message: &'static str) -> Result<T, DecodeError> {
    Err(invalid_error(offset, message))
}

fn to_json(value: PhpValue) -> Value {
    match value {
        PhpValue::Null => Value::Null,
        PhpValue::Bool(value) => Value::Bool(value),
        PhpValue::Int(value) => Value::Number(value.into()),
        PhpValue::Float(value) => {
            serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
        }
        PhpValue::String(value) => String::from_utf8(value)
            .map_or_else(
                |value| serde_json::json!({
                    "__php_type": "binary",
                    "length": value.as_bytes().len(),
                    "hex": value.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                }),
                Value::String,
            ),
        PhpValue::Array(items) => {
            let indexed = items.iter().enumerate().all(|(index, (key, _))| matches!(key, PhpValue::Int(value) if *value >= 0 && *value as usize == index));
            if indexed {
                Value::Array(items.into_iter().map(|(_, value)| to_json(value)).collect())
            } else {
                Value::Object(
                    items
                        .into_iter()
                        .filter_map(|(key, value)| match key {
                            PhpValue::String(key) => {
                                String::from_utf8(key).ok().map(|key| (key, to_json(value)))
                            }
                            PhpValue::Int(key) => Some((key.to_string(), to_json(value))),
                            _ => None,
                        })
                        .collect::<Map<_, _>>(),
                )
            }
        }
        PhpValue::Object { class, properties } => {
            let mut object = Map::new();
            object.insert(
                "__php_class".into(),
                String::from_utf8_lossy(&class).into_owned().into(),
            );
            object.insert(
                "properties".into(),
                Value::Array(
                    properties
                        .into_iter()
                        .map(|(key, value)| serde_json::json!({
                            "key": to_json(key),
                            "value": to_json(value),
                        }))
                        .collect(),
                ),
            );
            Value::Object(object)
        }
        PhpValue::Reference(value) => serde_json::json!({
            "__php_type": "reference",
            "id": value,
        }),
    }
}
