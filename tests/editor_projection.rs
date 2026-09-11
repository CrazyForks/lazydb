use lazydb::security::project_editor_line;

#[test]
fn projection_tracks_source_and_display_boundaries() {
    let projection = project_editor_line("a\t中\u{1b}b");

    assert_eq!(projection.text, "a   中<ESC>b");
    assert_eq!(projection.source_byte_boundaries, [0, 1, 2, 5, 6, 7]);
    assert_eq!(projection.source_to_display_bytes, [0, 1, 4, 7, 12, 13]);
    assert_eq!(projection.source_to_display_cells, [0, 1, 4, 6, 11, 12]);
}

#[test]
fn empty_projection_has_one_boundary() {
    let projection = project_editor_line("");

    assert_eq!(projection.text, "");
    assert_eq!(projection.source_byte_boundaries, [0]);
    assert_eq!(projection.source_to_display_bytes, [0]);
    assert_eq!(projection.source_to_display_cells, [0]);
}
