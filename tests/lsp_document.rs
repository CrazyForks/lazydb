use lazydb::lsp::position::PositionIndex;
use tower_lsp_server::ls_types::{Position, Range};

fn document(text: &str) -> PositionIndex {
    PositionIndex::new(text)
}

fn position(line: u32, character: u32) -> Position {
    Position::new(line, character)
}

fn range(start_line: u32, start_character: u32, end_line: u32, end_character: u32) -> Range {
    Range::new(
        position(start_line, start_character),
        position(end_line, end_character),
    )
}

#[test]
fn utf16_positions_round_trip_without_splitting_unicode() {
    let index = document("前😀abc\r\n第二行");
    assert_eq!(index.offset(position(0, 0)), 0);
    assert_eq!(index.offset(position(0, 3)), "前😀".len());
    assert_eq!(index.position("前😀".len()), position(0, 3));
    assert_eq!(index.offset(position(1, 0)), "前😀abc\r\n".len());
    assert_eq!(index.range(0, "前😀".len()), range(0, 0, 0, 3));
}

#[test]
fn out_of_range_positions_clamp_to_document_end() {
    let index = document("abc");
    assert_eq!(index.offset(position(99, 99)), 3);
    assert_eq!(index.position(99), position(0, 3));
}
