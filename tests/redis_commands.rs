use lazydb::redis_command::{
    classify,
    parser::{CommandParseError, parse},
    policy::{CommandPolicyError, RedisCommandKind},
};

#[test]
fn parser_preserves_binary_escaped_arguments() {
    let command = parse("SET 'key' \\x00\\xff").unwrap();
    assert_eq!(
        command.argv,
        vec![b"SET".to_vec(), b"key".to_vec(), vec![0, 255]]
    );
}

#[test]
fn parser_rejects_multiline_unclosed_quotes_and_unknown_escapes() {
    assert_eq!(
        parse("PING\nGET key"),
        Err(CommandParseError::MultipleLines)
    );
    assert!(matches!(
        parse("GET 'key"),
        Err(CommandParseError::UnterminatedQuote { .. })
    ));
    assert!(matches!(
        parse("GET \\q"),
        Err(CommandParseError::InvalidEscape { .. })
    ));
}

#[test]
fn policy_allows_only_bounded_read_commands() {
    assert_eq!(
        classify(&parse("PING").unwrap()),
        Ok(RedisCommandKind::Ping)
    );
    assert_eq!(
        classify(&parse("GET key").unwrap()),
        Err(CommandPolicyError::Unsupported)
    );
    assert_eq!(
        classify(&parse("GETRANGE key 0 10").unwrap()),
        Ok(RedisCommandKind::Getrange)
    );
    assert_eq!(
        classify(&parse("GETRANGE key 0 65536").unwrap()),
        Err(CommandPolicyError::RangeTooLarge)
    );
    assert_eq!(
        classify(&parse("MULTI").unwrap()),
        Err(CommandPolicyError::Unsupported)
    );
}
