use lazydb::model::editor_language::EditorLanguage;

#[test]
fn plain_preview_language_does_not_enable_syntax() {
    assert!(!EditorLanguage::Plain.uses_syntax_highlighting());
    assert!(EditorLanguage::Json.uses_syntax_highlighting());
}
