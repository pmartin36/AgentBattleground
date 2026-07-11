use super::*;

fn config() -> TextEditorConfig {
    TextEditorConfig {
        sizing: Sizing::Fixed,
        submit_on_enter: false,
        placeholder: String::new(),
    }
}

#[test]
fn set_text_then_text_round_trips_multiline() {
    let mut editor = TextEditor::new(config());
    editor.set_text("a\nb\nc");
    assert_eq!(editor.text(), "a\nb\nc");
}

#[test]
fn new_editor_has_empty_text() {
    let editor = TextEditor::new(config());
    assert_eq!(editor.text(), "");
}

#[test]
fn set_text_round_trips_trailing_newline() {
    let mut editor = TextEditor::new(config());
    editor.set_text("a\n");
    assert_eq!(editor.text(), "a\n");
}

#[test]
fn set_text_round_trips_empty_string() {
    let mut editor = TextEditor::new(config());
    editor.set_text("");
    assert_eq!(editor.text(), "");
}

#[test]
fn sizing_variants_and_reexports_are_public() {
    let _fixed = Sizing::Fixed;
    let _grow = Sizing::Grow { max_rows: 5 };
    let _event: EditorEvent = EditorEvent::Changed;
    assert_eq!(_event, EditorEvent::Changed);
}

// --- b1-t2: soft-wrap layout model + cursor logical position ---

#[test]
fn wrap_rows_splits_long_line_into_expected_row_count() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three");
    let rows = editor.wrap_rows(6);
    assert_eq!(rows.len(), 3);
}

#[test]
fn wrap_rows_splits_overlong_token_into_char_chunks() {
    let mut editor = TextEditor::new(config());
    editor.set_text("ABCDEFGH");
    let rows = editor.wrap_rows(4);
    assert_eq!(
        rows,
        vec![
            WrapRow { line: 0, start: 0, end: 4 },
            WrapRow { line: 0, start: 4, end: 8 },
        ]
    );
}

#[test]
fn wrap_rows_empty_line_yields_single_empty_row() {
    let editor = TextEditor::new(config());
    let rows = editor.wrap_rows(10);
    assert_eq!(rows, vec![WrapRow { line: 0, start: 0, end: 0 }]);
}

#[test]
fn wrap_rows_preserves_literal_spaces_as_a_partition() {
    // "ab  cd" @ width 3: wrap_to_width's whitespace-collapsing policy would
    // strip the leading space of the continuation line; wrap_rows must not —
    // every char (including the interior double space) stays covered by
    // exactly one row, contiguous end-to-start.
    let mut editor = TextEditor::new(config());
    editor.set_text("ab  cd");
    let rows = editor.wrap_rows(3);
    assert_eq!(
        rows,
        vec![
            WrapRow { line: 0, start: 0, end: 3 },
            WrapRow { line: 0, start: 3, end: 6 },
        ]
    );
}

#[test]
fn cursor_display_pos_maps_mid_line_col_to_wrapped_row() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three");
    editor.cursor_line = 0;
    editor.cursor_col = 5; // 'w' in "two", inside the 2nd wrapped row (4..8)
    assert_eq!(editor.cursor_display_pos(6), (1, 1));
}

#[test]
fn cursor_display_pos_end_of_line_maps_to_last_row() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three");
    editor.cursor_line = 0;
    editor.cursor_col = 13; // absolute end of the 13-char line
    assert_eq!(editor.cursor_display_pos(6), (2, 5));
}

#[test]
fn cursor_display_pos_at_soft_wrap_boundary_resolves_to_continuation_row() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three");
    editor.cursor_line = 0;
    editor.cursor_col = 4; // row0.end == row1.start == 4
    assert_eq!(editor.cursor_display_pos(6), (1, 0));
}

#[test]
fn wrap_rows_and_cursor_display_pos_handle_multiline_buffer() {
    let mut editor = TextEditor::new(config());
    editor.set_text("a\none two three\nb");
    let rows = editor.wrap_rows(6);
    // line0 "a" -> 1 row, line1 "one two three" -> 3 rows, line2 "b" -> 1 row
    assert_eq!(rows.len(), 5);

    editor.cursor_line = 1;
    editor.cursor_col = 5;
    // 1 row from line0 + local row index 1 within line1 -> flat row 2
    assert_eq!(editor.cursor_display_pos(6), (2, 1));
}
