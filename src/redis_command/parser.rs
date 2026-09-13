#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedisCommand {
    pub argv: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandParseError {
    Empty,
    MultipleLines,
    UnterminatedQuote { byte: usize },
    InvalidEscape { byte: usize },
    TooLong,
    TooManyArguments,
}

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_ARGUMENTS: usize = 128;

pub fn parse(input: &str) -> Result<RedisCommand, CommandParseError> {
    if input.trim().is_empty() {
        return Err(CommandParseError::Empty);
    }
    if input.len() > MAX_INPUT_BYTES {
        return Err(CommandParseError::TooLong);
    }
    if input.contains('\n') || input.contains('\r') {
        return Err(CommandParseError::MultipleLines);
    }
    let bytes = input.as_bytes();
    let mut argv = Vec::new();
    let mut current = Vec::new();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            } else if byte == b'\\' {
                index += 1;
                let (value, consumed) = parse_escape(bytes, index)?;
                current.push(value);
                index += consumed;
            } else {
                let character = input[index..].chars().next().unwrap();
                current.extend_from_slice(character.to_string().as_bytes());
                index += character.len_utf8() - 1;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte.is_ascii_whitespace() {
            if !current.is_empty() {
                argv.push(std::mem::take(&mut current));
            }
        } else if byte == b'\\' {
            index += 1;
            let (value, consumed) = parse_escape(bytes, index)?;
            current.push(value);
            index += consumed;
        } else {
            current.push(byte);
        }
        index += 1;
    }
    if quote.is_some() {
        return Err(CommandParseError::UnterminatedQuote { byte: bytes.len() });
    }
    if !current.is_empty() {
        argv.push(current);
    }
    if argv.is_empty() {
        return Err(CommandParseError::Empty);
    }
    if argv.len() > MAX_ARGUMENTS {
        return Err(CommandParseError::TooManyArguments);
    }
    Ok(RedisCommand { argv })
}

fn parse_escape(bytes: &[u8], index: usize) -> Result<(u8, usize), CommandParseError> {
    let byte = *bytes
        .get(index)
        .ok_or(CommandParseError::InvalidEscape { byte: index })?;
    match byte {
        b'n' => Ok((b'\n', 0)),
        b'r' => Ok((b'\r', 0)),
        b't' => Ok((b'\t', 0)),
        b'\\' | b'\'' | b'"' => Ok((byte, 0)),
        b'x' => {
            let high = *bytes
                .get(index + 1)
                .ok_or(CommandParseError::InvalidEscape { byte: index })?;
            let low = *bytes
                .get(index + 2)
                .ok_or(CommandParseError::InvalidEscape { byte: index })?;
            let high = (high as char)
                .to_digit(16)
                .ok_or(CommandParseError::InvalidEscape { byte: index })?;
            let low = (low as char)
                .to_digit(16)
                .ok_or(CommandParseError::InvalidEscape { byte: index })?;
            Ok(((high * 16 + low) as u8, 2))
        }
        _ => Err(CommandParseError::InvalidEscape { byte: index }),
    }
}
