use lazydb::sql::embedded::mybatis::extract_units;

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
