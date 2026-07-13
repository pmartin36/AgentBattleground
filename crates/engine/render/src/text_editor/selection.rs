//! Selection model: anchor/caret span over logical `(line, col)` positions,
//! char-indexed to match the editor's `cursor_line`/`cursor_col` convention.
//! b1-t1.

use super::{KeyCode, Pos, TextEditor};

// Called from tests only until b1-t2 (shift-movement) / b1-t3 (drag) /
// b1-t4 (replace-on-edit) / b1-t5 (render highlight) wire these into
// handle_key/handle_mouse/render.
#[allow(dead_code)]
impl TextEditor {
    /// `true` iff a selection is currently active.
    pub(super) fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    /// Set the selection to `(anchor, caret)`, direction preserved as given.
    pub(super) fn set_selection(&mut self, anchor: Pos, caret: Pos) {
        self.selection = Some((anchor, caret));
    }

    /// Clear the selection. Caret fields (`cursor_line`/`cursor_col`) are
    /// left untouched.
    pub(super) fn collapse(&mut self) {
        self.selection = None;
    }

    /// Normalized `(start, end)` span in document order regardless of
    /// anchor/caret direction. `None` when no selection.
    pub(super) fn selection_span(&self) -> Option<(Pos, Pos)> {
        self.selection.map(|(anchor, caret)| {
            if anchor <= caret {
                (anchor, caret)
            } else {
                (caret, anchor)
            }
        })
    }

    /// Exact char-substring over the normalized span. `None` when no
    /// selection.
    pub(super) fn selected_text(&self) -> Option<String> {
        let ((sl, sc), (el, ec)) = self.selection_span()?;
        if sl == el {
            let line: Vec<char> = self.lines[sl].chars().collect();
            return Some(line[sc..ec].iter().collect());
        }
        let mut out = String::new();
        let first: Vec<char> = self.lines[sl].chars().collect();
        out.extend(&first[sc..]);
        out.push('\n');
        for line in &self.lines[sl + 1..el] {
            out.push_str(line);
            out.push('\n');
        }
        let last: Vec<char> = self.lines[el].chars().collect();
        out.extend(&last[..ec]);
        Some(out)
    }

    /// Move the caret per `code` (Left/Right/Up/Down/Home/End), updating the
    /// selection per `shift`: extends (anchoring on the first shift-move at
    /// the pre-move caret) when `shift` is `true`, or collapses then moves
    /// normally when `shift` is `false`. b1-t2.
    pub(super) fn move_and_update_selection(&mut self, code: KeyCode, shift: bool) {
        if shift {
            let anchor = self
                .selection
                .map(|(a, _)| a)
                .unwrap_or((self.cursor_line, self.cursor_col));
            self.move_cursor(code);
            let caret = (self.cursor_line, self.cursor_col);
            self.set_selection(anchor, caret);
        } else {
            self.collapse();
            self.move_cursor(code);
        }
    }

    /// Begin a text-selection mouse drag at the current caret: clears any
    /// prior selection and records the drag anchor. Called after the caret
    /// has already been placed at the `Down(Left)` cell. b1-t3.
    pub(super) fn begin_drag_selection(&mut self) {
        self.collapse();
        self.drag_anchor = Some((self.cursor_line, self.cursor_col));
    }

    /// Extend the in-progress drag selection to `(column, row)`, clamped
    /// into the content rect. No-op (`false`) if no drag is active. b1-t3.
    pub(super) fn drag_selection(&mut self, column: u16, row: u16) -> bool {
        let Some(anchor) = self.drag_anchor else {
            return false;
        };
        self.place_caret_at_cell(column, row, true);
        self.set_selection(anchor, (self.cursor_line, self.cursor_col));
        self.reset_blink();
        true
    }

    /// End the in-progress drag; leaves the selection as-is. Returns `true`
    /// iff a drag was actually active. b1-t3.
    pub(super) fn end_drag_selection(&mut self) -> bool {
        self.drag_anchor.take().is_some()
    }

    /// Delete the active selection's normalized span (if any), collapsing
    /// the caret to the span start and clearing the selection. `false`
    /// (no-op) when there is no active selection. b1-t4.
    #[allow(unused)]
    pub(super) fn delete_selection(&mut self) -> bool {
        let Some(((sl, sc), (el, ec))) = self.selection_span() else {
            return false;
        };
        self.reset_blink();
        if sl == el {
            let chars: Vec<char> = self.lines[sl].chars().collect();
            self.lines[sl] = chars[..sc].iter().chain(&chars[ec..]).collect();
        } else {
            let first: Vec<char> = self.lines[sl].chars().collect();
            let last: Vec<char> = self.lines[el].chars().collect();
            self.lines[sl] = first[..sc].iter().chain(&last[ec..]).collect();
            self.lines.drain(sl + 1..=el);
        }
        self.cursor_line = sl;
        self.cursor_col = sc;
        self.collapse();
        true
    }

    /// Delete the active selection (per `delete_selection`) then insert
    /// `with` at the resulting caret, reusing `insert_char`/`insert_newline`
    /// so multi-line payloads re-wrap naturally. b1-t4 (shared choke point
    /// b2 paste will also call).
    pub(super) fn replace_selection(&mut self, with: &str) {
        self.delete_selection();
        for c in with.chars() {
            if c == '\n' {
                self.insert_newline();
            } else {
                self.insert_char(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    use super::super::{EditorEvent, Sizing, TextEditorConfig};
    use super::*;

    fn config() -> TextEditorConfig {
        TextEditorConfig {
            sizing: Sizing::Fixed,
            submit_on_enter: false,
            placeholder: String::new(),
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    /// Same geometry as text_editor_tests.rs's `clickable_editor`: 4 wrapped
    /// display rows ("ab ", "cd ", "ef", "xy") over a rect wide enough
    /// (height 10) that every row is reachable without scrolling. b1-t3.
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
    fn fresh_editor_has_no_selection() {
        let ed = TextEditor::new(config());
        assert_eq!(ed.selection, None);
        assert!(!ed.has_selection());
    }

    #[test]
    fn set_selection_single_line_selects_exact_substring() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcdef");
        ed.set_selection((0, 1), (0, 4));
        assert!(ed.has_selection());
        assert_eq!(ed.selected_text().as_deref(), Some("bcd"));
    }

    #[test]
    fn selection_span_normalizes_regardless_of_direction() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcdef");
        ed.set_selection((0, 4), (0, 1));
        assert_eq!(ed.selection_span(), Some(((0, 1), (0, 4))));
        assert_eq!(ed.selected_text().as_deref(), Some("bcd"));
    }

    #[test]
    fn selected_text_spans_multiple_lines() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abc\ndef\nghi");
        ed.set_selection((0, 1), (2, 2));
        assert_eq!(ed.selected_text().as_deref(), Some("bc\ndef\ngh"));
    }

    #[test]
    fn collapse_clears_selection_and_leaves_caret_unchanged() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcdef");
        ed.set_selection((0, 1), (0, 4));
        let (line, col) = (ed.cursor_line, ed.cursor_col);
        ed.collapse();
        assert_eq!(ed.selection, None);
        assert_eq!((ed.cursor_line, ed.cursor_col), (line, col));
    }

    // b1-t2: shift-movement extends selection; plain movement collapses it.

    #[test]
    fn shift_right_thrice_builds_selection_spanning_three_cols() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        ed.set_text("abcdef");
        assert_eq!(ed.handle_key(key(KeyCode::Home)), EditorEvent::None);
        assert_eq!(ed.selection, None);

        assert_eq!(ed.handle_key(shift_key(KeyCode::Right)), EditorEvent::None);
        assert_eq!(ed.handle_key(shift_key(KeyCode::Right)), EditorEvent::None);
        assert_eq!(ed.handle_key(shift_key(KeyCode::Right)), EditorEvent::None);

        assert_eq!(ed.selection, Some(((0, 0), (0, 3))));
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 3));
    }

    #[test]
    fn plain_move_after_shift_selection_collapses_and_advances_caret() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        ed.set_text("abcdef");
        ed.handle_key(key(KeyCode::Home));
        ed.handle_key(shift_key(KeyCode::Right));
        ed.handle_key(shift_key(KeyCode::Right));
        ed.handle_key(shift_key(KeyCode::Right));
        assert!(ed.selection.is_some());

        assert_eq!(ed.handle_key(key(KeyCode::Right)), EditorEvent::None);

        assert_eq!(ed.selection, None);
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 4));
    }

    #[test]
    fn shift_left_after_shift_right_preserves_original_anchor() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        ed.set_text("abcdef");
        ed.handle_key(key(KeyCode::Home));

        ed.handle_key(shift_key(KeyCode::Right));
        ed.handle_key(shift_key(KeyCode::Right));
        assert_eq!(ed.selection, Some(((0, 0), (0, 2))));

        assert_eq!(ed.handle_key(shift_key(KeyCode::Left)), EditorEvent::None);

        assert_eq!(ed.selection, Some(((0, 0), (0, 1))));
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 1));
    }

    #[test]
    fn shift_down_then_shift_end_extend_over_wrapped_model() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        ed.viewport_height = 10;
        ed.set_text("abc\ndef");
        // set_text leaves the caret at the end; walk it to (0, 0).
        ed.handle_key(key(KeyCode::Home));
        ed.handle_key(key(KeyCode::Up));
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 0));
        assert_eq!(ed.selection, None);

        assert_eq!(ed.handle_key(shift_key(KeyCode::Down)), EditorEvent::None);
        assert_eq!(ed.selection_span(), Some(((0, 0), (1, 0))));

        assert_eq!(ed.handle_key(shift_key(KeyCode::End)), EditorEvent::None);
        assert_eq!(ed.selection_span(), Some(((0, 0), (1, 3))));
    }

    // b1-t3: mouse-drag selection (Down/Drag/Up).

    #[test]
    fn down_then_drag_builds_selection() {
        let mut editor = clickable_editor();
        let width = editor.viewport_width;
        let rows = editor.wrap_rows(width);

        let (a_row, a_col) = (0usize, 0usize);
        let mut probe_a = clickable_editor();
        let a_display_row = (editor.scroll_offset + a_row).min(rows.len() - 1);
        probe_a.set_from_display(&rows, a_display_row, a_col);
        let a_pos = (probe_a.cursor_line, probe_a.cursor_col);

        let (b_row, b_col) = (1usize, 2usize);
        let mut probe_b = clickable_editor();
        let b_display_row = (editor.scroll_offset + b_row).min(rows.len() - 1);
        probe_b.set_from_display(&rows, b_display_row, b_col);
        let b_pos = (probe_b.cursor_line, probe_b.cursor_col);

        let down_handled = editor.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            editor.viewport_x + a_col as u16,
            editor.viewport_y + a_row as u16,
        ));
        assert!(down_handled, "an in-content Down must report handled");
        assert_eq!(
            editor.selection, None,
            "Down alone must not start a live selection"
        );

        let drag_handled = editor.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            editor.viewport_x + b_col as u16,
            editor.viewport_y + b_row as u16,
        ));
        assert!(drag_handled, "an active drag must report handled");
        assert_eq!(editor.selection, Some((a_pos, b_pos)));
        assert_eq!((editor.cursor_line, editor.cursor_col), b_pos);
    }

    #[test]
    fn down_then_up_no_drag_leaves_no_selection() {
        let mut editor = clickable_editor();
        let width = editor.viewport_width;
        let rows = editor.wrap_rows(width);
        let (local_row, local_col) = (0usize, 1usize);
        let mut probe = clickable_editor();
        let display_row = (editor.scroll_offset + local_row).min(rows.len() - 1);
        probe.set_from_display(&rows, display_row, local_col);
        let expected = (probe.cursor_line, probe.cursor_col);

        editor.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            editor.viewport_x + local_col as u16,
            editor.viewport_y + local_row as u16,
        ));
        let up_handled = editor.handle_mouse(&mouse_at(
            MouseEventKind::Up(MouseButton::Left),
            editor.viewport_x + local_col as u16,
            editor.viewport_y + local_row as u16,
        ));

        assert!(up_handled, "Up ending an active drag must report handled");
        assert_eq!(editor.selection, None);
        assert_eq!((editor.cursor_line, editor.cursor_col), expected);
    }

    #[test]
    fn drag_out_of_rect_clamps_to_last_wrapped_row() {
        let mut editor = clickable_editor();
        let width = editor.viewport_width;
        let rows = editor.wrap_rows(width);

        let (down_row, down_col) = (0usize, 0usize);
        let mut probe_down = clickable_editor();
        let down_display_row = (editor.scroll_offset + down_row).min(rows.len() - 1);
        probe_down.set_from_display(&rows, down_display_row, down_col);
        let anchor_pos = (probe_down.cursor_line, probe_down.cursor_col);

        editor.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            editor.viewport_x + down_col as u16,
            editor.viewport_y + down_row as u16,
        ));

        let mut probe_last = clickable_editor();
        let last_display_row = rows.len() - 1;
        probe_last.set_from_display(&rows, last_display_row, 0);
        let expected_caret = (probe_last.cursor_line, probe_last.cursor_col);

        // Well below the content rect (past viewport_y + viewport_height) —
        // must clamp into the rect, not panic.
        let far_row = editor.viewport_y + editor.viewport_height as u16 + 5;
        let drag_handled = editor.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            editor.viewport_x,
            far_row,
        ));

        assert!(
            drag_handled,
            "an out-of-rect drag while dragging must still report handled (clamped)"
        );
        assert_eq!((editor.cursor_line, editor.cursor_col), expected_caret);
        assert_eq!(editor.selection, Some((anchor_pos, expected_caret)));
    }

    #[test]
    fn drag_without_down_is_noop() {
        let mut editor = clickable_editor();
        let cursor_before = (editor.cursor_line, editor.cursor_col);

        let drag_handled = editor.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            editor.viewport_x,
            editor.viewport_y,
        ));

        assert!(
            !drag_handled,
            "a Drag with no preceding Down must be a no-op"
        );
        assert_eq!(editor.selection, None);
        assert_eq!((editor.cursor_line, editor.cursor_col), cursor_before);
    }

    #[test]
    fn out_of_rect_down_does_not_start_drag() {
        let mut editor = clickable_editor();
        let cursor_before = (editor.cursor_line, editor.cursor_col);

        let down_handled = editor.handle_mouse(&mouse_at(
            MouseEventKind::Down(MouseButton::Left),
            editor.viewport_x + editor.viewport_width as u16, // right of rect
            editor.viewport_y,
        ));
        assert!(!down_handled);

        let drag_handled = editor.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            editor.viewport_x,
            editor.viewport_y,
        ));

        assert!(
            !drag_handled,
            "a Drag with no preceding in-rect Down must be a no-op"
        );
        assert_eq!(editor.selection, None);
        assert_eq!((editor.cursor_line, editor.cursor_col), cursor_before);
    }

    // b1-t4: editing over a selection replaces it.

    #[test]
    fn type_over_selection_replaces() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcd");
        ed.set_selection((0, 1), (0, 3));

        assert_eq!(ed.handle_key(key(KeyCode::Char('X'))), EditorEvent::Changed);

        assert_eq!(ed.text(), "aXd");
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 2));
        assert_eq!(ed.selection, None);
    }

    #[test]
    fn backspace_over_selection_deletes_span_only() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcd");
        ed.set_selection((0, 1), (0, 3));

        assert_eq!(ed.handle_key(key(KeyCode::Backspace)), EditorEvent::Changed);

        assert_eq!(ed.text(), "ad");
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 1));
        assert_eq!(ed.selection, None);
    }

    #[test]
    fn delete_over_selection_deletes_span_only() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcd");
        ed.set_selection((0, 1), (0, 3));

        assert_eq!(ed.handle_key(key(KeyCode::Delete)), EditorEvent::Changed);

        assert_eq!(ed.text(), "ad");
        assert_eq!(ed.selection, None);
    }

    #[test]
    fn enter_over_selection_splits_at_span_start() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcd");
        ed.set_selection((0, 1), (0, 3));

        assert_eq!(ed.handle_key(key(KeyCode::Enter)), EditorEvent::Changed);

        assert_eq!(ed.text(), "a\nd");
        assert_eq!((ed.cursor_line, ed.cursor_col), (1, 0));
        assert_eq!(ed.selection, None);
    }

    #[test]
    fn replace_selection_multiline_span_joins() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abc\ndef\nghi");
        ed.set_selection((0, 1), (2, 2));

        ed.replace_selection("XY");

        assert_eq!(ed.text(), "aXYi");
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 3));
        assert_eq!(ed.selection, None);
    }

    #[test]
    fn replace_selection_multiline_payload_inserts_newlines() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcd");
        ed.set_selection((0, 1), (0, 3));

        ed.replace_selection("X\nY");

        assert_eq!(ed.text(), "aX\nYd");
        assert_eq!((ed.cursor_line, ed.cursor_col), (1, 1));
    }

    // Regression guard: unaffected by b1-t4 wiring (delete_selection is a
    // no-op with no active selection) — passes against current code today
    // and must keep passing once the editing arms are wired.
    #[test]
    fn no_selection_typing_unchanged() {
        let mut ed = TextEditor::new(config());
        assert_eq!(ed.handle_key(key(KeyCode::Char('z'))), EditorEvent::Changed);
        assert_eq!(ed.text(), "z");
    }
}
