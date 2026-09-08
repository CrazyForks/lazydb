use lazydb::sql::embedded::mybatis::extract_units;
use lazydb::sql::embedded::mybatis::extract_units_for_dialect;
use lazydb::sql::{SqlDialect, TextRange, diagnose_sql};

#[test]
fn dialect_parameters_preserve_source_spans_across_cdata_and_double_digits() {
    for dialect in [
        SqlDialect::Postgres,
        SqlDialect::MySql,
        SqlDialect::SqlServer,
        SqlDialect::Sqlite,
        SqlDialect::Generic,
    ] {
        let parameters = (1..=12)
            .map(|n| format!("#{{p{n},jdbcType=INTEGER}}"))
            .collect::<Vec<_>>();
        let source = format!(
            "<select>SELECT {}, <![CDATA[{}]]> FROM users</select>",
            parameters[..6].join(", "),
            parameters[6..].join(", ")
        );
        let unit = extract_units_for_dialect(&source, dialect).remove(0);
        assert!(unit.trusted_diagnostics);
        assert!(
            diagnose_sql(&unit.sql, dialect).is_empty(),
            "{dialect:?}: {}",
            unit.sql
        );
        let segments = unit
            .segments
            .iter()
            .filter(|s| s.kind == lazydb::sql::embedded::SourceSegmentKind::Parameter)
            .collect::<Vec<_>>();
        assert_eq!(segments.len(), 12);
        for (n, segment) in segments.iter().enumerate() {
            let expected = match dialect {
                SqlDialect::Postgres => format!("${}", n + 1),
                SqlDialect::SqlServer => format!("@p{}", n + 1),
                _ => "?".into(),
            };
            assert_eq!(
                &unit.sql[segment.generated.start..segment.generated.end],
                expected
            );
            assert_eq!(
                unit.source_diagnostic(segment.generated),
                Some(segment.source)
            );
            assert_eq!(
                &source[segment.source.start..segment.source.end],
                parameters[n]
            );
        }
        let generated = unit.sql.find("FROM").unwrap();
        let original = source.find("FROM").unwrap();
        assert_eq!(
            unit.source_edit(TextRange::new(generated, generated + 4)),
            Some(TextRange::new(original, original + 4))
        );
    }
}

#[test]
fn extracts_static_mapper_sql_and_parameters() {
    let source = r#"<mapper namespace="demo"><select id="find">SELECT id FROM users WHERE name = #{name}</select></mapper>"#;
    let units = extract_units(source);
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].sql, "SELECT id FROM users WHERE name = ?");
    assert!(units[0].trusted_diagnostics);
    assert!(units[0].segments.iter().any(|segment| matches!(
        segment.kind,
        lazydb::sql::embedded::SourceSegmentKind::Parameter
    )));
}

#[test]
fn dynamic_tags_are_extracted_but_not_trusted_for_diagnostics() {
    let source = "<select id=\"find\">SELECT * FROM users <where><if test=\"x\">AND id = #{id}</if></where></select>";
    let units = extract_units(source);
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].sql, "SELECT * FROM users AND id = ?");
    assert!(!units[0].trusted_diagnostics);
}

#[test]
fn entity_text_is_decoded_without_treating_attributes_as_sql() {
    let source = "<select id=\"x\">SELECT * FROM users WHERE a &lt; 3</select>";
    let units = extract_units(source);
    assert_eq!(units[0].sql, "SELECT * FROM users WHERE a < 3");
}

#[test]
fn cdata_delimiters_are_not_injected_into_sql() {
    let source = "<select id=\"x\"><![CDATA[SELECT * FROM users WHERE a < 3]]></select>";
    let units = extract_units(source);
    assert_eq!(units[0].sql, "SELECT * FROM users WHERE a < 3");
    assert!(units[0].trusted_diagnostics);
}

#[test]
fn include_and_fragment_tags_suppress_untrusted_diagnostics() {
    let source = "<select id=\"x\">SELECT <include refid=\"columns\"/> FROM users</select>";
    let units = extract_units(source);
    assert_eq!(units.len(), 1);
    assert!(!units[0].trusted_diagnostics);
}
