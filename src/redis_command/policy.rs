use super::parser::RedisCommand;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisCommandKind {
    Ping,
    Type,
    Ttl,
    Pttl,
    Strlen,
    Hlen,
    Llen,
    Scard,
    Zcard,
    Getrange,
    Scan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandPolicyError {
    Empty,
    Unsupported,
    WrongArgumentCount,
    MultipleLines,
    RangeTooLarge,
}

pub fn classify(command: &RedisCommand) -> Result<RedisCommandKind, CommandPolicyError> {
    let name = command.argv.first().ok_or(CommandPolicyError::Empty)?;
    let name = name.as_slice();
    let kind = if name.eq_ignore_ascii_case(b"PING") {
        RedisCommandKind::Ping
    } else if name.eq_ignore_ascii_case(b"TYPE") {
        RedisCommandKind::Type
    } else if name.eq_ignore_ascii_case(b"TTL") {
        RedisCommandKind::Ttl
    } else if name.eq_ignore_ascii_case(b"PTTL") {
        RedisCommandKind::Pttl
    } else if name.eq_ignore_ascii_case(b"STRLEN") {
        RedisCommandKind::Strlen
    } else if name.eq_ignore_ascii_case(b"HLEN") {
        RedisCommandKind::Hlen
    } else if name.eq_ignore_ascii_case(b"LLEN") {
        RedisCommandKind::Llen
    } else if name.eq_ignore_ascii_case(b"SCARD") {
        RedisCommandKind::Scard
    } else if name.eq_ignore_ascii_case(b"ZCARD") {
        RedisCommandKind::Zcard
    } else if name.eq_ignore_ascii_case(b"GETRANGE") {
        RedisCommandKind::Getrange
    } else if name.eq_ignore_ascii_case(b"SCAN") {
        RedisCommandKind::Scan
    } else {
        return Err(CommandPolicyError::Unsupported);
    };
    validate_arguments(kind, &command.argv)?;
    Ok(kind)
}

fn validate_arguments(kind: RedisCommandKind, argv: &[Vec<u8>]) -> Result<(), CommandPolicyError> {
    let expected = match kind {
        RedisCommandKind::Ping => 1..=2,
        RedisCommandKind::Type
        | RedisCommandKind::Ttl
        | RedisCommandKind::Pttl
        | RedisCommandKind::Strlen
        | RedisCommandKind::Hlen
        | RedisCommandKind::Llen
        | RedisCommandKind::Scard
        | RedisCommandKind::Zcard => 2..=2,
        RedisCommandKind::Getrange => 4..=4,
        RedisCommandKind::Scan => 1..=5,
    };
    if !expected.contains(&argv.len()) {
        return Err(CommandPolicyError::WrongArgumentCount);
    }
    if kind == RedisCommandKind::Getrange {
        let start = std::str::from_utf8(&argv[2])
            .ok()
            .and_then(|value| value.parse::<u64>().ok());
        let end = std::str::from_utf8(&argv[3])
            .ok()
            .and_then(|value| value.parse::<u64>().ok());
        if start
            .zip(end)
            .is_none_or(|(start, end)| end < start || end - start >= 64 * 1024)
        {
            return Err(CommandPolicyError::RangeTooLarge);
        }
    }
    Ok(())
}
