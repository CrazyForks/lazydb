use lazydb::profile::{DatabaseKind, SslMode, parse_connection_url};

#[test]
fn mariadb_connection_url_round_trips_supported_ssl_and_read_only_options() {
    let parsed = parse_connection_url(
        "mariadb://user:secret@example.test/app?sslMode=verify-full&readOnly=true",
    )
    .unwrap();
    assert_eq!(parsed.kind, DatabaseKind::MariaDb);
    assert_eq!(parsed.port, Some(3306));
    assert_eq!(parsed.ssl_mode, SslMode::VerifyFull);
    assert!(parsed.read_only);
}

#[test]
fn mariadb_connection_url_rejects_duplicate_ssl_options() {
    let error =
        parse_connection_url("mariadb://example.test/app?useSSL=true&sslMode=require").unwrap_err();
    assert!(error.to_string().contains("conflicting"));
}
