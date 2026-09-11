use lazydb::profile::{ConnectionUrlFormat, DatabaseKind, parse_connection_url};

#[test]
fn parses_the_supplied_oracle_service_url() {
    let parsed = parse_connection_url("jdbc:oracle:thin:@10.114.130.159:1521/SUPPORTDB").unwrap();

    assert_eq!(parsed.kind, DatabaseKind::Oracle);
    assert_eq!(parsed.format, ConnectionUrlFormat::JdbcOracle);
    assert_eq!(parsed.host.as_deref(), Some("10.114.130.159"));
    assert_eq!(parsed.port, Some(1521));
    assert_eq!(parsed.database.as_deref(), Some("SUPPORTDB"));
    assert_eq!(parsed.user, None);
    assert!(parsed.password.is_none());
}

#[test]
fn parses_oracle_credentials_without_exposing_them_in_the_endpoint() {
    let parsed = parse_connection_url(
        "jdbc:oracle:thin:@10.114.130.159:1521/SUPPORTDB?user=mfgsupport&password=mfgsupport",
    )
    .unwrap();
    assert_eq!(parsed.user.as_deref(), Some("mfgsupport"));
    assert!(parsed.password.is_some());
}
