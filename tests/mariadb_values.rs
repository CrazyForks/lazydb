mod support;

use lazydb::{
    db::{DatabaseConnection, value::CellValue},
    profile::import_connection_url,
};

#[tokio::test]
async fn mariadb_time_preserves_extended_duration_values() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB values test")).unwrap();
    let database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let result = database
        .execute("SELECT CAST('-01:02:03.123456' AS TIME(6)) AS negative, CAST('800:00:00.654321' AS TIME(6)) AS extended")
        .await
        .unwrap()
        .result_sets
        .pop()
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert!(matches!(&result.rows[0][0], CellValue::Text(value) if value.contains("-01:02:03")));
    assert!(matches!(&result.rows[0][1], CellValue::Text(value) if value.contains("800:00:00")));
    database.close().await;
}

#[tokio::test]
async fn mariadb_value_matrix_preserves_binary_json_decimal_and_empty_columns() {
    let Some(url) = support::mariadb_test_url() else {
        return;
    };
    let imported = import_connection_url(&url, Some("MariaDB values matrix")).unwrap();
    let database =
        DatabaseConnection::connect(&imported.profile, imported.transient_password.as_ref())
            .await
            .unwrap();
    let result = database
        .execute(
            "SELECT CAST('12345678901234567890.12345678901234567890' AS DECIMAL(65,20)) AS exact_decimal, \
                    CAST('ff00' AS BINARY(2)) AS bytes, \
                    JSON_OBJECT('engine', 'mariadb') AS document, \
                    CAST(18446744073709551615 AS UNSIGNED) AS maximum_unsigned",
        )
        .await
        .unwrap()
        .result_sets
        .pop()
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0][0].clipboard_text(),
        "12345678901234567890.12345678901234567890"
    );
    assert!(matches!(
        result.rows[0][1],
        CellValue::Bytes(_) | CellValue::Text(_)
    ));
    assert!(matches!(
        result.rows[0][2],
        CellValue::Text(_) | CellValue::Bytes(_)
    ));
    assert_eq!(result.rows[0][3], CellValue::Unsigned(u64::MAX));

    let empty = database
        .execute("SELECT CAST(1 AS DECIMAL(5,2)) AS amount WHERE FALSE")
        .await
        .unwrap();
    if let Some(empty_result) = empty
        .result_sets
        .iter()
        .find(|result| !result.columns.is_empty())
    {
        assert_eq!(empty_result.rows.len(), 0);
        assert_eq!(empty_result.columns[0].name, "amount");
    }
    database.close().await;
}
