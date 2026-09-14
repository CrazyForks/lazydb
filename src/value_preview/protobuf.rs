use serde_json::{Map, Value};

use super::{DecodeError, DecodeStatus};

pub fn is_protobuf_data(data: &[u8]) -> bool {
    parse_fields(data).is_ok_and(|fields| !fields.is_empty())
}

pub fn parse_protobuf_to_json(data: &[u8]) -> Result<String, DecodeError> {
    serde_json::to_string_pretty(&Value::Object(parse_fields(data)?))
        .map_err(|error| DecodeError::new(DecodeStatus::Invalid, error.to_string()))
}

fn parse_fields(data: &[u8]) -> Result<Map<String, Value>, DecodeError> {
    let mut offset = 0;
    let mut fields = Map::new();
    while offset < data.len() {
        let (tag, next) = varint(data, offset)?;
        offset = next;
        let number = tag >> 3;
        let wire = tag & 7;
        if number == 0 || wire == 4 || wire > 5 {
            return Err(
                DecodeError::new(DecodeStatus::Invalid, "invalid protobuf field tag").at(offset),
            );
        }
        let value = match wire {
            0 => {
                let (value, next) = varint(data, offset)?;
                offset = next;
                Value::Number(value.into())
            }
            1 => {
                let bytes = take(data, offset, 8)?;
                offset += 8;
                Value::String(hex(bytes))
            }
            2 => {
                let (length, next) = varint(data, offset)?;
                offset = next;
                let length = usize::try_from(length).map_err(|_| {
                    DecodeError::new(DecodeStatus::Invalid, "protobuf length overflow").at(offset)
                })?;
                let bytes = take(data, offset, length)?;
                offset += length;
                std::str::from_utf8(bytes).map_or_else(
                    |_| Value::String(hex(bytes)),
                    |text| Value::String(text.to_owned()),
                )
            }
            5 => {
                let bytes = take(data, offset, 4)?;
                offset += 4;
                Value::String(hex(bytes))
            }
            _ => unreachable!(),
        };
        let key = format!("field_{number}");
        if let Some(previous) = fields.remove(&key) {
            fields.insert(
                key,
                match previous {
                    Value::Array(mut values) => {
                        values.push(value);
                        Value::Array(values)
                    }
                    previous => Value::Array(vec![previous, value]),
                },
            );
        } else {
            fields.insert(key, value);
        }
    }
    Ok(fields)
}

fn varint(data: &[u8], mut offset: usize) -> Result<(u64, usize), DecodeError> {
    let start = offset;
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *data.get(offset).ok_or_else(|| {
            DecodeError::new(DecodeStatus::NeedsMoreData, "truncated protobuf varint").at(offset)
        })?;
        offset += 1;
        if shift >= 64 && byte & 0x7f != 0 {
            return Err(
                DecodeError::new(DecodeStatus::Invalid, "protobuf varint overflow").at(start),
            );
        }
        value |= u64::from(byte & 0x7f) << shift.min(63);
        if byte & 0x80 == 0 {
            return Ok((value, offset));
        }
    }
    Err(DecodeError::new(DecodeStatus::Invalid, "protobuf varint overflow").at(start))
}

fn take(data: &[u8], offset: usize, length: usize) -> Result<&[u8], DecodeError> {
    let end = offset.checked_add(length).ok_or_else(|| {
        DecodeError::new(DecodeStatus::Invalid, "protobuf length overflow").at(offset)
    })?;
    data.get(offset..end).ok_or_else(|| {
        DecodeError::new(DecodeStatus::NeedsMoreData, "truncated protobuf field").at(offset)
    })
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}
