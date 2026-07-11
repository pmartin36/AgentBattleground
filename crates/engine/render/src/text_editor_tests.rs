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

// --- b2-t1: editing keys + Enter/submit semantics -> EditorEvent ---

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn submit_config() -> TextEditorConfig {
    TextEditorConfig {
        sizing: Sizing::Fixed,
        submit_on_enter: true,
        placeholder: String::new(),
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn typing_chars_then_backspace_mutates_buffer_and_returns_changed() {
    let mut editor = TextEditor::new(config());
    assert_eq!(editor.handle_key(key(KeyCode::Char('h'))), EditorEvent::Changed);
    assert_eq!(editor.handle_key(key(KeyCode::Char('i'))), EditorEvent::Changed);
    assert_eq!(editor.text(), "hi");
    assert_eq!(editor.handle_key(key(KeyCode::Backspace)), EditorEvent::Changed);
    assert_eq!(editor.text(), "h");
}

#[test]
fn enter_with_submit_on_enter_returns_submit_and_inserts_nothing() {
    let mut editor = TextEditor::new(submit_config());
    editor.set_text("hi");
    assert_eq!(editor.handle_key(key(KeyCode::Enter)), EditorEvent::Submit);
    assert_eq!(editor.text(), "hi");
}

#[test]
fn shift_enter_with_submit_on_enter_inserts_newline_and_returns_changed() {
    let mut editor = TextEditor::new(submit_config());
    editor.set_text("hi");
    let shift_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    assert_eq!(editor.handle_key(shift_enter), EditorEvent::Changed);
    assert_eq!(editor.text(), "hi\n");
}

#[test]
fn enter_without_submit_on_enter_inserts_newline_splitting_at_caret() {
    let mut editor = TextEditor::new(config());
    editor.set_text("hi");
    // caret at end of "hi" (col 2) after set_text.
    assert_eq!(editor.handle_key(key(KeyCode::Enter)), EditorEvent::Changed);
    assert_eq!(editor.text(), "hi\n");
}

#[test]
fn backspace_at_buffer_start_returns_none_and_leaves_text_empty() {
    let mut editor = TextEditor::new(config());
    assert_eq!(editor.handle_key(key(KeyCode::Backspace)), EditorEvent::None);
    assert_eq!(editor.text(), "");
}

#[test]
fn delete_at_end_of_non_last_line_merges_next_line_up() {
    let mut editor = TextEditor::new(config());
    editor.set_text("ab\ncd");
    editor.cursor_line = 0;
    editor.cursor_col = 2; // end of "ab"
    assert_eq!(editor.handle_key(key(KeyCode::Delete)), EditorEvent::Changed);
    assert_eq!(editor.text(), "abcd");
}

#[test]
fn delete_at_buffer_end_returns_none() {
    let mut editor = TextEditor::new(config());
    editor.set_text("ab");
    editor.cursor_line = 0;
    editor.cursor_col = 2; // end of "ab", also end of buffer
    assert_eq!(editor.handle_key(key(KeyCode::Delete)), EditorEvent::None);
    assert_eq!(editor.text(), "ab");
}

#[test]
fn non_editing_key_returns_none_and_mutates_nothing() {
    let mut editor = TextEditor::new(config());
    editor.set_text("ab");
    assert_eq!(editor.handle_key(key(KeyCode::Esc)), EditorEvent::None);
    assert_eq!(editor.text(), "ab");
}

// --- b2-t2: cursor movement keys + viewport-follows-cursor ---

#[test]
fn right_at_end_of_line_moves_to_start_of_next_logical_line() {
    let mut editor = TextEditor::new(config());
    editor.set_text("ab\ncd");
    editor.cursor_line = 0;
    editor.cursor_col = 2; // end of "ab"
    assert_eq!(editor.handle_key(key(KeyCode::Right)), EditorEvent::None);
    assert_eq!(editor.cursor_line, 1);
    assert_eq!(editor.cursor_col, 0);
    assert_eq!(editor.text(), "ab\ncd");
}

#[test]
fn left_at_start_of_line_moves_to_end_of_previous_logical_line() {
    let mut editor = TextEditor::new(config());
    editor.set_text("ab\ncd");
    editor.cursor_line = 1;
    editor.cursor_col = 0;
    assert_eq!(editor.handle_key(key(KeyCode::Left)), EditorEvent::None);
    assert_eq!(editor.cursor_line, 0);
    assert_eq!(editor.cursor_col, 2); // end of "ab"
    assert_eq!(editor.text(), "ab\ncd");
}

#[test]
fn down_moves_caret_onto_next_wrapped_display_row_of_same_logical_line() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three"); // wraps to 3 rows @ width 6
    editor.viewport_width = 6;
    editor.viewport_height = 10; // tall enough that this test isn't about scrolling
    editor.cursor_line = 0;
    editor.cursor_col = 0; // display row 0
    assert_eq!(editor.handle_key(key(KeyCode::Down)), EditorEvent::None);
    assert_eq!(editor.cursor_display_pos(6).0, 1);
    assert_eq!(editor.cursor_line, 0); // still the one logical line
}

#[test]
fn up_returns_caret_to_previous_wrapped_display_row() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three"); // wraps to 3 rows @ width 6
    editor.viewport_width = 6;
    editor.viewport_height = 10;
    editor.cursor_line = 0;
    editor.cursor_col = 5; // in the 2nd wrapped row (display row 1)
    assert_eq!(editor.cursor_display_pos(6).0, 1);
    assert_eq!(editor.handle_key(key(KeyCode::Up)), EditorEvent::None);
    assert_eq!(editor.cursor_display_pos(6).0, 0);
}

#[test]
fn up_at_top_row_is_a_clamped_no_op() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three");
    editor.viewport_width = 6;
    editor.viewport_height = 10;
    editor.cursor_line = 0;
    editor.cursor_col = 1; // display row 0
    assert_eq!(editor.handle_key(key(KeyCode::Up)), EditorEvent::None);
    assert_eq!(editor.cursor_line, 0);
    assert_eq!(editor.cursor_col, 1);
}

#[test]
fn down_at_bottom_row_is_a_clamped_no_op() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three"); // last wrapped row is display row 2
    editor.viewport_width = 6;
    editor.viewport_height = 10;
    editor.cursor_line = 0;
    editor.cursor_col = 13; // absolute end of line, display row 2
    assert_eq!(editor.cursor_display_pos(6).0, 2);
    assert_eq!(editor.handle_key(key(KeyCode::Down)), EditorEvent::None);
    assert_eq!(editor.cursor_display_pos(6).0, 2);
    assert_eq!(editor.cursor_col, 13);
}

#[test]
fn home_moves_caret_to_start_of_its_display_row() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three");
    editor.viewport_width = 6;
    editor.viewport_height = 10;
    editor.cursor_line = 0;
    editor.cursor_col = 5; // mid 2nd wrapped row (start == 4)
    assert_eq!(editor.handle_key(key(KeyCode::Home)), EditorEvent::None);
    assert_eq!(editor.cursor_col, 4);
    assert_eq!(editor.cursor_display_pos(6), (1, 0));
}

#[test]
fn end_moves_caret_to_last_visible_col_of_its_display_row() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three");
    editor.viewport_width = 6;
    editor.viewport_height = 10;
    editor.cursor_line = 0;
    editor.cursor_col = 4; // start of 2nd wrapped row ("two t", 4..8, non-last row)
    assert_eq!(editor.handle_key(key(KeyCode::End)), EditorEvent::None);
    // non-last row: last VISIBLE char, not the wrap boundary (col 8, which
    // would resolve to the next display row under cursor_display_pos).
    assert_eq!(editor.cursor_col, 7);
    assert_eq!(editor.cursor_display_pos(6), (1, 3));
}

#[test]
fn end_on_final_display_row_of_a_line_goes_to_the_line_end() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three"); // final row is "three" (8..13)
    editor.viewport_width = 6;
    editor.viewport_height = 10;
    editor.cursor_line = 0;
    editor.cursor_col = 9; // inside the final wrapped row
    assert_eq!(editor.handle_key(key(KeyCode::End)), EditorEvent::None);
    assert_eq!(editor.cursor_col, 13); // absolute end of the 13-char line
}

#[test]
fn down_past_viewport_bottom_scrolls_offset_to_keep_caret_visible() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three"); // 3 display rows @ width 6
    editor.viewport_width = 6;
    editor.viewport_height = 2; // only 2 rows visible at a time
    editor.cursor_line = 0;
    editor.cursor_col = 0; // display row 0, scroll_offset starts at 0
    assert_eq!(editor.scroll_offset, 0);

    editor.handle_key(key(KeyCode::Down)); // -> display row 1, still visible [0,2)
    assert_eq!(editor.scroll_offset, 0);

    editor.handle_key(key(KeyCode::Down)); // -> display row 2, pushes offset forward
    let row = editor.cursor_display_pos(6).0;
    assert_eq!(row, 2);
    assert_eq!(editor.scroll_offset, 1); // [1,3) now contains row 2
    assert!(row >= editor.scroll_offset && row < editor.scroll_offset + editor.viewport_height);
}

// --- b2-t3: explicit scroll input — wheel (handle_mouse) + PgUp/PgDn ---

use ratatui::crossterm::event::{MouseEvent, MouseEventKind};

fn mouse(kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }
}

/// Buffer + geometry with room to scroll: 6 short logical lines @
/// viewport_height 2 -> 6 display rows, max_scroll_offset == 4.
fn scrollable_editor() -> TextEditor {
    let mut editor = TextEditor::new(config());
    editor.set_text("a\nb\nc\nd\ne\nf");
    editor.viewport_width = 6;
    editor.viewport_height = 2;
    editor
}

#[test]
fn wheel_down_advances_offset_and_leaves_buffer_and_cursor_unchanged() {
    let mut editor = scrollable_editor();
    let text_before = editor.text();
    let (line_before, col_before) = (editor.cursor_line, editor.cursor_col);

    assert!(editor.handle_mouse(&mouse(MouseEventKind::ScrollDown)));
    assert_eq!(editor.scroll_offset, 3);
    assert_eq!(editor.text(), text_before);
    assert_eq!(editor.cursor_line, line_before);
    assert_eq!(editor.cursor_col, col_before);
}

#[test]
fn wheel_up_at_top_returns_false_and_offset_stays_zero() {
    let mut editor = scrollable_editor();
    assert_eq!(editor.scroll_offset, 0);
    assert!(!editor.handle_mouse(&mouse(MouseEventKind::ScrollUp)));
    assert_eq!(editor.scroll_offset, 0);
}

#[test]
fn wheel_down_at_bottom_clamps_and_returns_false() {
    let mut editor = scrollable_editor();
    editor.scroll_offset = editor.max_scroll_offset();
    let max = editor.scroll_offset;
    assert!(!editor.handle_mouse(&mouse(MouseEventKind::ScrollDown)));
    assert_eq!(editor.scroll_offset, max);
}

#[test]
fn non_scroll_mouse_event_returns_false_and_leaves_offset_unchanged() {
    let mut editor = scrollable_editor();
    assert!(!editor.handle_mouse(&mouse(MouseEventKind::Moved)));
    assert_eq!(editor.scroll_offset, 0);
}

#[test]
fn page_down_advances_offset_by_viewport_height_and_returns_none() {
    let mut editor = scrollable_editor();
    let text_before = editor.text();
    assert_eq!(editor.handle_key(key(KeyCode::PageDown)), EditorEvent::None);
    assert_eq!(editor.scroll_offset, 2); // + viewport_height (2)
    assert_eq!(editor.text(), text_before);
}

#[test]
fn page_down_clamps_at_max_scroll_offset() {
    let mut editor = scrollable_editor();
    let max = editor.max_scroll_offset();
    editor.handle_key(key(KeyCode::PageDown));
    editor.handle_key(key(KeyCode::PageDown));
    editor.handle_key(key(KeyCode::PageDown)); // would overshoot without clamp
    assert_eq!(editor.scroll_offset, max);
}

#[test]
fn page_up_moves_offset_back_toward_zero_clamped() {
    let mut editor = scrollable_editor();
    editor.scroll_offset = editor.max_scroll_offset();
    editor.handle_key(key(KeyCode::PageUp));
    assert_eq!(editor.scroll_offset, editor.max_scroll_offset() - 2);

    editor.handle_key(key(KeyCode::PageUp));
    editor.handle_key(key(KeyCode::PageUp));
    editor.handle_key(key(KeyCode::PageUp)); // would underflow without clamp
    assert_eq!(editor.scroll_offset, 0);
}
