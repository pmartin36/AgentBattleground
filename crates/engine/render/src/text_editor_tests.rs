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

use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

fn mouse(kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }
}

fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
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

// --- b3-t1: render text + reverse-video block cursor + placeholder ---

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

fn make_buf(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

/// Collect the visible symbol of each cell in row `y` of `rect`, trailing
/// spaces stripped, for row-by-row wrap assertions.
fn row_text(buf: &Buffer, rect: Rect, y: u16) -> String {
    (rect.left()..rect.right())
        .map(|x| buf.cell((x, y)).unwrap().symbol().chars().next().unwrap_or(' '))
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn placeholder_config(placeholder: &str) -> TextEditorConfig {
    TextEditorConfig {
        sizing: Sizing::Fixed,
        submit_on_enter: false,
        placeholder: placeholder.to_string(),
    }
}

#[test]
fn caret_cell_is_reverse_video_and_preserves_underlying_char() {
    let mut editor = TextEditor::new(config());
    editor.set_text("hi"); // caret lands at end (col 2) after set_text
    let rect = Rect::new(0, 0, 10, 3);
    let mut buf = make_buf(10, 3);

    editor.render(&mut buf, rect);

    // Caret is one past "hi" (row 0, col 2) — a reverse-video blank cell.
    let caret = buf.cell((2, 0)).unwrap();
    assert!(
        caret.modifier.contains(Modifier::REVERSED),
        "caret cell must carry REVERSED for the block-cursor effect"
    );
    assert_eq!(caret.symbol(), " ", "caret must not overwrite/blank an existing char");

    // The written text cells themselves are not reverse-video.
    let h_cell = buf.cell((0, 0)).unwrap();
    assert_eq!(h_cell.symbol(), "h");
    assert!(!h_cell.modifier.contains(Modifier::REVERSED));
}

#[test]
fn long_line_occupies_expected_display_rows() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one two three"); // wraps to 3 rows @ width 6
    // Height (5) exceeds the 3 wrapped rows so content fits vertically —
    // no scrollbar/gutter, full width available (stable across b3-t2).
    let rect = Rect::new(0, 0, 6, 5);
    let mut buf = make_buf(6, 5);

    editor.render(&mut buf, rect);

    assert_eq!(row_text(&buf, rect, 0), "one");
    assert_eq!(row_text(&buf, rect, 1), "two");
    assert_eq!(row_text(&buf, rect, 2), "three");
    assert_eq!(row_text(&buf, rect, 3), "", "row past the last wrapped row must be blank");
}

#[test]
fn empty_focused_editor_renders_placeholder_and_caret_at_origin() {
    let mut editor = TextEditor::new(placeholder_config("type here"));
    let rect = Rect::new(0, 0, 20, 3);
    let mut buf = make_buf(20, 3);

    editor.render(&mut buf, rect);

    assert_eq!(row_text(&buf, rect, 0), "type here");
    assert!(
        buf.cell((0, 0)).unwrap().modifier.contains(Modifier::DIM),
        "placeholder text must render dimmed"
    );
    // A focused empty editor draws the caret at position 0 — so clicking into
    // an empty box shows a visible cursor rather than nothing.
    assert!(
        buf.cell((0, 0)).unwrap().modifier.contains(Modifier::REVERSED),
        "a focused empty editor must draw the caret at the origin"
    );
    for x in rect.left()..rect.right() {
        for y in rect.top()..rect.bottom() {
            if (x, y) == (0, 0) {
                continue;
            }
            assert!(
                !buf.cell((x, y)).unwrap().modifier.contains(Modifier::REVERSED),
                "the caret must only appear at the origin"
            );
        }
    }
}

#[test]
fn empty_unfocused_editor_draws_no_caret() {
    let mut editor = TextEditor::new(placeholder_config("type here"));
    editor.set_focused(false);
    let rect = Rect::new(0, 0, 20, 3);
    let mut buf = make_buf(20, 3);

    editor.render(&mut buf, rect);

    for x in rect.left()..rect.right() {
        for y in rect.top()..rect.bottom() {
            assert!(
                !buf.cell((x, y)).unwrap().modifier.contains(Modifier::REVERSED),
                "an unfocused empty editor must not draw a caret"
            );
        }
    }
}

#[test]
fn render_respects_scroll_offset_showing_rows_from_the_offset_down() {
    let mut editor = TextEditor::new(config());
    editor.set_text("a\nb\nc\nd\ne"); // 5 single-char logical lines
    editor.scroll_offset = 2; // skip past "a", "b"
    let rect = Rect::new(0, 0, 6, 2);
    let mut buf = make_buf(6, 2);

    editor.render(&mut buf, rect);

    // 5 display rows > viewport height 2, so this scenario overflows and
    // (as of b3-t2) the right-most column carries scrollbar ink, not text —
    // assert column 0 directly rather than the full-row string.
    assert_eq!(buf.cell((0, 0)).unwrap().symbol(), "c");
    assert_eq!(buf.cell((0, 1)).unwrap().symbol(), "d");
}

// --- b3-t2: scrollbar fixture through the dot pipeline ---

use crate::decode_braille_cell;

/// 6 single-char logical lines rendered at width 6 -> 6 display rows;
/// viewport height 2 -> overflow (`max_scroll_offset() == 4`). Viewport
/// dims come from `render()`'s rect, not hand-seeded fields, so the
/// scrollbar math matches exactly what render() itself computes.
fn overflowing_editor_and_rect() -> (TextEditor, Rect) {
    let mut editor = TextEditor::new(config());
    editor.set_text("a\nb\nc\nd\ne\nf");
    (editor, Rect::new(0, 0, 6, 2))
}

#[test]
fn overflowing_content_draws_thumb_only_in_rightmost_column() {
    let (mut editor, rect) = overflowing_editor_and_rect();
    let mut buf = make_buf(rect.width, rect.height);

    editor.render(&mut buf, rect);

    // total=6 rows, view=2 cells -> thumb is exactly 1 cell tall and sits at
    // the top cell when scroll_offset == 0. Thumb-only: it fills the top cell
    // (both dot columns), and there is NO full-height track, so the cell below
    // it is empty.
    let x = rect.right() - 1;
    let (top_mask, _) = decode_braille_cell(&buf, x, 0).expect("top cell must carry the thumb");
    assert_eq!(top_mask, 0xFF, "thumb fills the top cell (both dot columns) at offset 0");
    assert!(
        decode_braille_cell(&buf, x, 1).is_none(),
        "no full-height track: the cell below the thumb is empty at offset 0"
    );
}

#[test]
fn thumb_moves_down_as_scroll_offset_increases() {
    let (mut editor, rect) = overflowing_editor_and_rect();
    let mut buf = make_buf(rect.width, rect.height);
    editor.render(&mut buf, rect); // establishes viewport dims / max_scroll_offset

    editor.scroll_offset = editor.max_scroll_offset();
    let mut buf2 = make_buf(rect.width, rect.height);
    editor.render(&mut buf2, rect);

    let x = rect.right() - 1;
    assert!(
        decode_braille_cell(&buf2, x, 0).is_none(),
        "thumb has moved off the top cell at max scroll offset"
    );
    let (bottom_mask, _) =
        decode_braille_cell(&buf2, x, 1).expect("bottom cell must carry the thumb");
    assert_eq!(bottom_mask, 0xFF, "thumb is flush to the bottom cell at max scroll offset");
}

#[test]
fn content_that_fits_draws_no_scrollbar() {
    let mut editor = TextEditor::new(config());
    editor.set_text("a\nb");
    let rect = Rect::new(0, 0, 6, 3); // 2 display rows <= 3 visible rows
    let mut buf = make_buf(6, 3);

    editor.render(&mut buf, rect);

    let x = rect.right() - 1;
    for y in rect.top()..rect.bottom() {
        assert!(
            decode_braille_cell(&buf, x, y).is_none(),
            "no scrollbar dots when content fits the viewport"
        );
    }
}

// --- b3-t3: Grow sizing — desired_rows(width) clamp + Fixed-at-max behavior ---

fn grow_config(max_rows: u16) -> TextEditorConfig {
    TextEditorConfig {
        sizing: Sizing::Grow { max_rows },
        submit_on_enter: false,
        placeholder: String::new(),
    }
}

#[test]
fn grow_desired_rows_tracks_wrap_count_as_width_narrows() {
    let mut editor = TextEditor::new(grow_config(10));
    editor.set_text("one two three");

    let wide = editor.desired_rows(100);
    let narrow = editor.desired_rows(6);

    assert_eq!(wide, 1, "fits on one row at a wide width");
    assert_eq!(narrow, 3, "wraps to 3 rows at width 6, matching wrap_rows(6)");
    assert!(narrow > wide, "narrower width wraps to more rows");
}

#[test]
fn grow_desired_rows_clamps_to_max_rows() {
    let mut editor = TextEditor::new(grow_config(3));
    editor.set_text("a\nb\nc\nd\ne\nf"); // 6 single-char logical lines -> 6 display rows at width 6

    assert_eq!(editor.desired_rows(6), 3, "clamped to max_rows even though content wraps to 6 rows");
}

#[test]
fn grow_at_max_rows_still_allows_scrolling_past_cap() {
    let mut editor = TextEditor::new(grow_config(3));
    editor.set_text("a\nb\nc\nd\ne\nf");
    editor.viewport_width = 6;
    editor.viewport_height = 3; // caller sized the rect to desired_rows(6) == 3

    assert_eq!(editor.desired_rows(6), 3, "reporting stays capped at max_rows");
    assert_eq!(
        editor.max_scroll_offset(),
        editor.total_display_rows() - 3,
        "content beyond max_rows is still reachable by scrolling"
    );
    assert!(editor.max_scroll_offset() > 0, "there is scrollable overflow beyond the cap");
}

#[test]
fn desired_rows_never_returns_zero_for_empty_editor() {
    let grow_empty = TextEditor::new(grow_config(5));
    assert_eq!(grow_empty.desired_rows(10), 1, "empty Grow editor floors to 1");

    let fixed_empty = TextEditor::new(config());
    assert_eq!(fixed_empty.desired_rows(10), 1, "empty Fixed editor floors to 1");
}

#[test]
fn fixed_desired_rows_is_uncapped_natural_wrap_count() {
    let mut editor = TextEditor::new(config()); // Sizing::Fixed
    editor.set_text("a\nb\nc\nd\ne\nf"); // 6 display rows at width 6

    assert_eq!(editor.desired_rows(6), 6, "Fixed reports the natural wrapped row count, no cap");
}

// --- b1-t1: slow cursor blink — accumulator, tick, focus flag, render gating ---

#[test]
fn caret_visible_at_phase_zero_when_focused_and_render_shows_reversed_cell() {
    let mut editor = TextEditor::new(config());
    editor.set_text("hi"); // caret lands at end (col 2)

    assert!(editor.caret_visible(), "caret must be visible at phase 0 on a focused editor");

    let rect = Rect::new(0, 0, 10, 3);
    let mut buf = make_buf(10, 3);
    editor.render(&mut buf, rect);
    let caret = buf.cell((2, 0)).unwrap();
    assert!(caret.modifier.contains(Modifier::REVERSED), "caret cell must be reverse-video at phase 0");
}

#[test]
fn tick_half_period_hides_caret_and_render_omits_reversed_cell() {
    let mut editor = TextEditor::new(config());
    editor.set_text("hi");

    editor.tick(BLINK_PERIOD);
    assert!(!editor.caret_visible(), "caret must be hidden once accumulator reaches the second half");

    let rect = Rect::new(0, 0, 10, 3);
    let mut buf = make_buf(10, 3);
    editor.render(&mut buf, rect);
    let caret = buf.cell((2, 0)).unwrap();
    assert!(
        !caret.modifier.contains(Modifier::REVERSED),
        "no reverse-video caret cell while hidden"
    );
}

#[test]
fn edit_during_hidden_phase_resets_caret_to_visible() {
    let mut editor = TextEditor::new(config());
    editor.set_text("hi");
    editor.tick(BLINK_PERIOD);
    assert!(!editor.caret_visible(), "precondition: caret is hidden before the edit");

    editor.handle_key(key(KeyCode::Char('x')));

    assert!(editor.caret_visible(), "any edit must reset the blink phase back to visible");
}

#[test]
fn unfocused_editor_renders_no_caret_while_focused_one_does() {
    let mut focused = TextEditor::new(config());
    focused.set_text("hi");
    let rect = Rect::new(0, 0, 10, 3);
    let mut focused_buf = make_buf(10, 3);
    focused.render(&mut focused_buf, rect);
    assert!(
        focused_buf.cell((2, 0)).unwrap().modifier.contains(Modifier::REVERSED),
        "a focused editor at phase 0 must render a caret"
    );

    let mut unfocused = TextEditor::new(config());
    unfocused.set_text("hi");
    unfocused.set_focused(false);
    assert!(!unfocused.caret_visible(), "an unfocused editor must never report the caret visible");

    let mut unfocused_buf = make_buf(10, 3);
    unfocused.render(&mut unfocused_buf, rect);
    for x in rect.left()..rect.right() {
        for y in rect.top()..rect.bottom() {
            assert!(
                !unfocused_buf.cell((x, y)).unwrap().modifier.contains(Modifier::REVERSED),
                "an unfocused editor must render NO caret anywhere in its rect"
            );
        }
    }
}

#[test]
fn desired_rows_does_not_mutate_editor_state() {
    let mut editor = TextEditor::new(grow_config(3));
    editor.set_text("one two three");
    editor.viewport_width = 6;
    editor.viewport_height = 2;
    editor.scroll_offset = 1;
    let before_text = editor.text();
    let before_cursor = (editor.cursor_line, editor.cursor_col);
    let before_scroll = editor.scroll_offset;
    let before_vw = editor.viewport_width;
    let before_vh = editor.viewport_height;

    let _ = editor.desired_rows(6);

    assert_eq!(editor.text(), before_text);
    assert_eq!((editor.cursor_line, editor.cursor_col), before_cursor);
    assert_eq!(editor.scroll_offset, before_scroll);
    assert_eq!(editor.viewport_width, before_vw);
    assert_eq!(editor.viewport_height, before_vh);
}

// --- b1-t2: click-to-place caret ---

/// Multi-line wrapped content at a cached render origin away from (0,0), with
/// a wrapped row shorter than `viewport_width` (`"ab "` at width 4) so a click
/// past the row's own width exercises `set_from_display`'s wrap-boundary
/// clamp. `wrap_rows(4)` on `"ab cd ef\nxy"` yields 4 display rows: "ab "
/// (non-last), "cd " (non-last), "ef" (last of line 0), "xy" (last of line 1)
/// — enough rows that `viewport_height` (10) exceeds them, for the
/// clamp-to-last-row case.
fn clickable_editor() -> TextEditor {
    let mut editor = TextEditor::new(config());
    editor.set_text("ab cd ef\nxy");
    editor.viewport_width = 4;
    editor.viewport_height = 10;
    editor.viewport_x = 2;
    editor.viewport_y = 1;
    editor.scroll_offset = 0;
    editor
}

#[test]
fn click_places_caret_matching_wrap_rows_and_set_from_display() {
    let mut editor = clickable_editor();
    let width = editor.viewport_width;
    let local_row = 0usize;
    let local_col = 3usize; // at the row's own width -> exercises the wrap-boundary clamp rule

    let rows = editor.wrap_rows(width);
    let mut probe = clickable_editor();
    let display_row = (editor.scroll_offset + local_row).min(rows.len() - 1);
    probe.set_from_display(&rows, display_row, local_col);
    let expected = (probe.cursor_line, probe.cursor_col);

    let handled = editor.handle_mouse(&mouse_at(
        MouseEventKind::Down(MouseButton::Left),
        editor.viewport_x + local_col as u16,
        editor.viewport_y + local_row as u16,
    ));

    assert!(handled, "an in-content click must report handled");
    assert_eq!((editor.cursor_line, editor.cursor_col), expected);
}

#[test]
fn click_row_past_content_clamps_to_last_wrapped_row() {
    let mut editor = clickable_editor();
    let width = editor.viewport_width;
    let rows = editor.wrap_rows(width);
    let local_row = editor.viewport_height - 1; // far past the 4 real wrapped rows, still inside the rect
    let local_col = 0usize;

    let mut probe = clickable_editor();
    let display_row = rows.len() - 1;
    probe.set_from_display(&rows, display_row, local_col);
    let expected = (probe.cursor_line, probe.cursor_col);

    let handled = editor.handle_mouse(&mouse_at(
        MouseEventKind::Down(MouseButton::Left),
        editor.viewport_x + local_col as u16,
        editor.viewport_y + local_row as u16,
    ));

    assert!(handled, "a click below content but inside the rect must still report handled");
    assert_eq!((editor.cursor_line, editor.cursor_col), expected);
}

#[test]
fn click_outside_content_rect_is_ignored() {
    let mut editor = clickable_editor();
    let text_before = editor.text();
    let cursor_before = (editor.cursor_line, editor.cursor_col);

    // right of the content rect
    assert!(!editor.handle_mouse(&mouse_at(
        MouseEventKind::Down(MouseButton::Left),
        editor.viewport_x + editor.viewport_width as u16,
        editor.viewport_y,
    )));
    // below the content rect
    assert!(!editor.handle_mouse(&mouse_at(
        MouseEventKind::Down(MouseButton::Left),
        editor.viewport_x,
        editor.viewport_y + editor.viewport_height as u16,
    )));
    // above the content rect
    assert!(!editor.handle_mouse(&mouse_at(
        MouseEventKind::Down(MouseButton::Left),
        editor.viewport_x,
        editor.viewport_y - 1,
    )));
    // left of the content rect
    assert!(!editor.handle_mouse(&mouse_at(
        MouseEventKind::Down(MouseButton::Left),
        editor.viewport_x - 1,
        editor.viewport_y,
    )));

    assert_eq!(editor.text(), text_before, "an out-of-rect click must not mutate the buffer");
    assert_eq!(
        (editor.cursor_line, editor.cursor_col),
        cursor_before,
        "an out-of-rect click must not move the caret"
    );
}

#[test]
fn click_to_place_resets_blink_to_visible() {
    let mut editor = clickable_editor();
    editor.tick(BLINK_PERIOD);
    assert!(!editor.caret_visible(), "precondition: caret hidden before click");

    let handled = editor.handle_mouse(&mouse_at(
        MouseEventKind::Down(MouseButton::Left),
        editor.viewport_x,
        editor.viewport_y,
    ));

    assert!(handled, "an in-content click must report handled");
    assert!(editor.caret_visible(), "an in-content click must reset the blink phase to visible");
}
