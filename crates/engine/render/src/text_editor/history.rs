//! Undo/redo history: a bounded stack of state snapshots with kind-based
//! coalescing. b4-t1.
//!
//! Recording (snapshot before a mutating `handle_key` arm, commit after) is
//! wired into `handle_key`'s wrapper (mod.rs). `undo()`/`redo()` are
//! internal for now; wired to Ctrl+Z/Ctrl+Y/Ctrl+Shift+Z in b4-t2.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Pos, TextEditor};

/// Max entries kept in the undo stack; oldest is dropped once exceeded.
pub(super) const UNDO_STACK_DEPTH: usize = 100;

/// Captured editor state for one undo step.
#[derive(Clone)]
pub(super) struct Snapshot {
    pub(super) lines: Vec<String>,
    pub(super) cursor_line: usize,
    pub(super) cursor_col: usize,
    pub(super) selection: Option<(Pos, Pos)>,
}

/// Coalescing kind of a mutating `handle_key` arm.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EditKind {
    Insert,
    Delete,
    Other,
}

impl TextEditor {
    /// Classify a key event into the undo-coalescing kind of the mutating
    /// `handle_key` arm it will hit, or `None` for a non-mutating/movement
    /// key (Ctrl+C copy included — it never mutates). Mirrors `handle_key`'s
    /// own dispatch guard exactly so the recording wrapper and the match
    /// body cannot silently drift apart. b4-t1.
    pub(super) fn classify_key(key: &KeyEvent) -> Option<EditKind> {
        match key.code {
            KeyCode::Char(c)
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(c, 'c' | 'C' | 'x' | 'X' | 'v' | 'V') =>
            {
                match c.to_ascii_lowercase() {
                    'c' => None,
                    _ => Some(EditKind::Other), // cut / paste
                }
            }
            KeyCode::Char(c)
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(c, 'z' | 'Z' | 'y' | 'Y') =>
            {
                None
            }
            KeyCode::Char(_) => Some(EditKind::Insert),
            KeyCode::Enter => Some(EditKind::Other),
            KeyCode::Backspace | KeyCode::Delete => Some(EditKind::Delete),
            _ => None,
        }
    }

    /// Capture buffer + caret + selection as a `Snapshot`. b4-t1.
    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            cursor_line: self.cursor_line,
            cursor_col: self.cursor_col,
            selection: self.selection,
        }
    }

    /// Restore buffer + caret + selection from `snap`, then scroll the
    /// viewport to keep the caret visible. b4-t1.
    pub(super) fn restore(&mut self, snap: Snapshot) {
        self.lines = snap.lines;
        self.cursor_line = snap.cursor_line;
        self.cursor_col = snap.cursor_col;
        self.selection = snap.selection;
        self.scroll_to_cursor();
    }

    /// Record `pre` (the state captured before the just-applied edit) onto
    /// the undo stack, coalescing with the previous step when `kind` matches
    /// `last_edit_kind` and is `Insert`/`Delete`; otherwise pushes a new
    /// step. Always clears the redo stack and bounds the undo stack at
    /// `UNDO_STACK_DEPTH`. b4-t1.
    pub(super) fn commit_edit(&mut self, pre: Snapshot, kind: EditKind) {
        let coalesces = matches!(kind, EditKind::Insert | EditKind::Delete)
            && self.last_edit_kind == Some(kind);
        if !coalesces {
            self.undo_stack.push_back(pre);
            if self.undo_stack.len() > UNDO_STACK_DEPTH {
                self.undo_stack.pop_front();
            }
        }
        self.redo_stack.clear();
        self.last_edit_kind = match kind {
            EditKind::Insert | EditKind::Delete => Some(kind),
            EditKind::Other => None,
        };
    }

    /// Reset the coalescing run so the next edit starts a new undo step
    /// (called on caret movement). b4-t1.
    pub(super) fn break_coalescing(&mut self) {
        self.last_edit_kind = None;
    }

    /// Undo the most recent step: push the current state onto the redo
    /// stack and restore the popped undo snapshot. `false` (no-op) when the
    /// undo stack is empty. b4-t1, wired to Ctrl+Z in b4-t2.
    pub(super) fn undo(&mut self) -> bool {
        let Some(snap) = self.undo_stack.pop_back() else {
            return false;
        };
        let current = self.snapshot();
        self.redo_stack.push(current);
        self.restore(snap);
        self.last_edit_kind = None;
        true
    }

    /// Redo the most recently undone step: mirror of `undo`. `false` (no-op)
    /// when the redo stack is empty. b4-t1, wired to Ctrl+Y/Ctrl+Shift+Z in
    /// b4-t2.
    pub(super) fn redo(&mut self) -> bool {
        let Some(snap) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        self.undo_stack.push_back(current);
        self.restore(snap);
        self.last_edit_kind = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    use super::super::clipboard::FakeClipboard;
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

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn ctrl_shift_key(c: char) -> KeyEvent {
        KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        )
    }

    fn type_str(ed: &mut TextEditor, s: &str) {
        for c in s.chars() {
            ed.handle_key(key(KeyCode::Char(c)));
        }
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    /// Seed a viewport at the origin so `handle_mouse` can map cells to caret
    /// positions.
    fn seed_viewport(ed: &mut TextEditor) {
        ed.viewport_width = 80;
        ed.viewport_height = 10;
        ed.viewport_x = 0;
        ed.viewport_y = 0;
        ed.scroll_offset = 0;
    }

    #[test]
    fn typed_run_coalesces_into_one_undo_step() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        type_str(&mut ed, "hello");
        assert_eq!(ed.text(), "hello");

        assert!(ed.undo(), "undo of a recorded run must return true");
        assert_eq!(ed.text(), "");
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 0));
        assert!(!ed.undo(), "undo with an empty stack must return false");
    }

    #[test]
    fn newline_between_typed_runs_makes_separate_steps() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        type_str(&mut ed, "ab");
        ed.handle_key(key(KeyCode::Enter));
        type_str(&mut ed, "cd");
        assert_eq!(ed.text(), "ab\ncd");

        assert!(ed.undo());
        assert_eq!(ed.text(), "ab\n");
        assert!(ed.undo());
        assert_eq!(ed.text(), "ab");
        assert!(ed.undo());
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn caret_move_between_typed_chars_splits_undo_steps() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        ed.handle_key(key(KeyCode::Char('a')));
        ed.handle_key(key(KeyCode::Left));
        ed.handle_key(key(KeyCode::Right));
        ed.handle_key(key(KeyCode::Char('b')));
        assert_eq!(ed.text(), "ab");

        assert!(ed.undo());
        assert_eq!(ed.text(), "a", "undo must remove only the second run ('b')");
    }

    #[test]
    fn undo_then_redo_round_trips_buffer_and_caret() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        type_str(&mut ed, "ab");
        ed.handle_key(key(KeyCode::Enter));
        type_str(&mut ed, "cd");
        let full_text = ed.text();
        let full_caret = (ed.cursor_line, ed.cursor_col);

        assert!(ed.undo());
        assert!(ed.undo());
        assert!(ed.undo());
        assert_eq!(ed.text(), "");

        assert!(ed.redo());
        assert!(ed.redo());
        assert!(ed.redo());
        assert_eq!(ed.text(), full_text);
        assert_eq!((ed.cursor_line, ed.cursor_col), full_caret);
    }

    #[test]
    fn cut_and_paste_are_separate_undo_steps() {
        let mut ed = TextEditor::new(config());
        ed.set_clipboard(Box::new(FakeClipboard::default()));
        ed.viewport_width = 80;
        ed.set_text("abcdef");
        ed.set_selection((0, 1), (0, 4)); // "bcd"

        ed.handle_key(ctrl_key('x'));
        assert_eq!(ed.text(), "aef");
        ed.handle_key(key(KeyCode::End));
        ed.handle_key(ctrl_key('v'));
        assert_eq!(ed.text(), "aefbcd");

        assert!(ed.undo(), "undo of paste must be its own step");
        assert_eq!(
            ed.text(),
            "aef",
            "first undo restores the post-cut buffer only"
        );
        assert!(ed.undo(), "undo of cut must be its own step");
        assert_eq!(ed.text(), "abcdef");
        assert_eq!(ed.selection, Some(((0, 1), (0, 4))));
    }

    #[test]
    fn undo_stack_length_is_bounded_by_depth() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        for i in 0..(UNDO_STACK_DEPTH + 5) {
            let c = if i % 2 == 0 { 'a' } else { 'b' };
            ed.handle_key(key(KeyCode::Char(c)));
            ed.handle_key(key(KeyCode::Enter));
        }
        assert_eq!(
            ed.undo_stack.len(),
            UNDO_STACK_DEPTH,
            "undo stack must be capped at UNDO_STACK_DEPTH, dropping the oldest steps"
        );
    }

    #[test]
    fn backspace_at_buffer_start_records_nothing() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        assert_eq!(ed.handle_key(key(KeyCode::Backspace)), EditorEvent::None);
        assert!(ed.undo_stack.is_empty());
        assert!(!ed.undo(), "undo with nothing recorded must return false");
    }

    #[test]
    fn new_edit_after_undo_clears_redo_stack() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        ed.handle_key(key(KeyCode::Char('a')));
        assert!(ed.undo());

        ed.handle_key(key(KeyCode::Char('b')));

        assert!(
            !ed.redo(),
            "a new edit after undo must clear the redo stack"
        );
    }

    // b4-t2: Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z bindings, through handle_key.

    #[test]
    fn ctrl_z_undoes_and_ctrl_y_redoes_through_handle_key() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        type_str(&mut ed, "ab");

        let undo_event = ed.handle_key(ctrl_key('z'));
        assert_eq!(undo_event, EditorEvent::Changed);
        assert_eq!(ed.text(), "");

        let redo_event = ed.handle_key(ctrl_key('y'));
        assert_eq!(redo_event, EditorEvent::Changed);
        assert_eq!(ed.text(), "ab");
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 2));
    }

    #[test]
    fn ctrl_shift_z_redoes() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        type_str(&mut ed, "ab");
        ed.handle_key(ctrl_key('z'));
        assert_eq!(ed.text(), "");

        let event = ed.handle_key(ctrl_shift_key('z'));
        assert_eq!(event, EditorEvent::Changed);
        assert_eq!(ed.text(), "ab");
    }

    #[test]
    fn ctrl_z_restores_selection() {
        let mut ed = TextEditor::new(config());
        ed.set_clipboard(Box::new(FakeClipboard::default()));
        ed.viewport_width = 80;
        ed.set_text("abcdef");
        ed.set_selection((0, 1), (0, 4)); // "bcd"

        ed.handle_key(ctrl_key('x'));
        assert_eq!(ed.text(), "aef");

        let event = ed.handle_key(ctrl_key('z'));
        assert_eq!(event, EditorEvent::Changed);
        assert_eq!(ed.text(), "abcdef");
        assert_eq!(ed.selection, Some(((0, 1), (0, 4))));
    }

    #[test]
    fn ctrl_z_empty_stack_is_noop() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;

        let event = ed.handle_key(ctrl_key('z'));
        assert_eq!(event, EditorEvent::None);
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn ctrl_y_empty_redo_stack_is_noop() {
        let mut ed = TextEditor::new(config());
        ed.viewport_width = 80;
        type_str(&mut ed, "ab");

        let event = ed.handle_key(ctrl_key('y'));
        assert_eq!(event, EditorEvent::None);
        assert_eq!(ed.text(), "ab");
    }

    // b4-t1 seam: a MOUSE caret move must break coalescing just like keyboard.

    /// A mouse click that repositions the caret is a caret move: a typed run
    /// before the click and one after must be separate undo steps, so one undo
    /// removes only the post-click run. Regression — the mouse path did not
    /// break coalescing (only the keyboard path did), collapsing both runs.
    #[test]
    fn mouse_click_between_typed_runs_splits_undo_steps() {
        let mut ed = TextEditor::new(config());
        seed_viewport(&mut ed);
        type_str(&mut ed, "ab");
        // Click at line start to move the caret (Down places it; Up ends the
        // zero-length drag).
        ed.handle_mouse(&mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        ed.handle_mouse(&mouse(MouseEventKind::Up(MouseButton::Left), 0, 0));
        type_str(&mut ed, "cd");
        assert_eq!(ed.text(), "cdab");

        assert!(ed.undo());
        assert_eq!(
            ed.text(),
            "ab",
            "undo must remove only the post-click run ('cd')"
        );
    }

    /// A mouse drag-selection is a caret move: typing that replaces the
    /// selection is its own undo step, not merged into the prior typed run.
    #[test]
    fn mouse_drag_select_then_type_starts_new_undo_step() {
        let mut ed = TextEditor::new(config());
        seed_viewport(&mut ed);
        type_str(&mut ed, "abcd");
        // Drag-select "cd" (cols 2..4).
        ed.handle_mouse(&mouse(MouseEventKind::Down(MouseButton::Left), 2, 0));
        ed.handle_mouse(&mouse(MouseEventKind::Drag(MouseButton::Left), 4, 0));
        ed.handle_mouse(&mouse(MouseEventKind::Up(MouseButton::Left), 4, 0));
        type_str(&mut ed, "X"); // replaces "cd"
        assert_eq!(ed.text(), "abX");

        assert!(ed.undo());
        assert_eq!(
            ed.text(),
            "abcd",
            "undo removes only the selection-replacement step"
        );
    }
}
