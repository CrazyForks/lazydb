use lazydb::lsp::completion::complete_document_with_embedded_sql;
use lazydb::lsp::diagnostics::diagnostics_for_document;
use lazydb::lsp::document::Document;
use lazydb::sql::{CompletionContext, CompletionIndex, SqlDialect};
use tower_lsp_server::ls_types::{CompletionResponse, Position, Uri};

fn xml(text: &str) -> Document {
    Document {
        uri: "file:///tmp/mapper.xml".parse::<Uri>().expect("URI"),
        language_id: "xml".into(),
        version: 1,
        text: text.into(),
    }
}

const DIALECTS: [SqlDialect; 5] = [
    SqlDialect::Postgres,
    SqlDialect::MySql,
    SqlDialect::SqlServer,
    SqlDialect::Sqlite,
    SqlDialect::Generic,
];

#[test]
fn static_parameters_do_not_produce_dialect_errors() {
    let document = xml(
        "<mapper>\n<select id=\"x\">\nSELECT * FROM users WHERE id = #{id,jdbcType=INTEGER}\nAND name = #{name} AND age &gt; #{age}\n<![CDATA[AND score < #{score}]]>\n</select></mapper>",
    );
    for dialect in DIALECTS {
        assert!(
            diagnostics_for_document(&document, dialect).is_empty(),
            "{dialect:?}"
        );
    }
}

#[test]
fn dynamic_sql_and_text_substitution_remain_untrusted() {
    for body in [
        "SELECT ${columns} FROM",
        "SELECT * FROM users <where><if test=\"id != null\">AND id = #{id}</if></where>",
        "SELECT <include refid=\"columns\"/> FROM",
        "SELECT #{unfinished",
    ] {
        for dialect in DIALECTS {
            assert!(
                diagnostics_for_document(&xml(&format!("<select>{body}</select>")), dialect)
                    .is_empty()
            );
        }
    }
}

#[test]
fn parser_errors_after_parameters_map_to_xml_not_virtual_coordinates() {
    let document = xml(
        "<mapper>\n<select>SELECT * FROM users WHERE id = #{id,jdbcType=INTEGER}\n<![CDATA[AND name = #{name} AND )]]></select></mapper>",
    );
    let positions = lazydb::lsp::position::PositionIndex::new(document.text.clone());
    let offset = document.text.find(')').unwrap();
    for dialect in DIALECTS {
        let diagnostics = diagnostics_for_document(&document, dialect);
        assert_eq!(diagnostics.len(), 1, "{dialect:?}");
        assert_eq!(diagnostics[0].range, positions.range(offset, offset + 1));
        assert!(!diagnostics[0].message.contains(" at Line:"));
    }
}

#[test]
fn eof_errors_map_to_end_of_sql_even_after_a_parameter() {
    let document =
        xml("<mapper>\n<select>SELECT * FROM users WHERE id = #{id} AND\n</select></mapper>");
    let offset = document.text.find("AND").unwrap() + 3;
    let positions = lazydb::lsp::position::PositionIndex::new(document.text.clone());
    let diagnostics = diagnostics_for_document(&document, SqlDialect::Postgres);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].range, positions.range(offset, offset));
}

#[test]
fn decoded_entity_error_highlights_the_whole_xml_entity() {
    let document = xml("<select>SELECT * FROM users WHERE id = #{id} AND &gt;</select>");
    let offset = document.text.find("&gt;").unwrap();
    let positions = lazydb::lsp::position::PositionIndex::new(document.text.clone());
    let diagnostics = diagnostics_for_document(&document, SqlDialect::Postgres);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].range, positions.range(offset, offset + 4));
}

#[test]
fn tokenizer_error_after_unicode_and_parameters_has_a_valid_xml_range() {
    let document = xml("<select>SELECT '\u{1f600}', #{id}, '</select>");
    let diagnostics = diagnostics_for_document(&document, SqlDialect::Postgres);
    assert_eq!(diagnostics.len(), 1);
    let offset = document.text.rfind('\'').unwrap();
    let positions = lazydb::lsp::position::PositionIndex::new(document.text.clone());
    assert_eq!(diagnostics[0].range, positions.range(offset, offset + 1));
    assert!(!diagnostics[0].message.contains(" at Line:"));
}

#[test]
fn completion_after_a_parameter_preserves_xml_edit_coordinates() {
    let document = xml("<select>SELECT #{id,jdbcType=INTEGER}; SEL</select>");
    let start = document.text.rfind("SEL").unwrap();
    let positions = lazydb::lsp::position::PositionIndex::new(document.text.clone());
    let CompletionResponse::List(list) = complete_document_with_embedded_sql(
        &document,
        positions.position(start + 3),
        SqlDialect::Postgres,
        &CompletionIndex::new(&[]),
        false,
        CompletionContext::default(),
    ) else {
        panic!("expected completion list")
    };
    let item = list
        .items
        .iter()
        .find(|item| item.label == "SELECT")
        .expect("SELECT completion");
    let tower_lsp_server::ls_types::CompletionTextEdit::Edit(edit) =
        item.text_edit.as_ref().unwrap()
    else {
        panic!("expected text edit")
    };
    assert_eq!(edit.range, positions.range(start, start + 3));
    assert_eq!(edit.new_text, "SELECT");
}

#[test]
fn static_mapper_errors_are_mapped_back_to_xml_ranges() {
    let document = xml("<select id=\"x\">SELECT * FROM users WHERE</select>");
    let diagnostics = diagnostics_for_document(&document, SqlDialect::Generic);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].range.start.character > 0);
}

#[test]
fn non_sql_xml_position_returns_no_sql_completion() {
    let document = xml("<mapper namespace=\"demo\"></mapper>");
    let CompletionResponse::List(list) = complete_document_with_embedded_sql(
        &document,
        Position::new(0, 20),
        SqlDialect::Generic,
        &CompletionIndex::new(&[]),
        false,
        CompletionContext::default(),
    ) else {
        panic!("expected completion list")
    };
    assert!(list.items.is_empty());
}
