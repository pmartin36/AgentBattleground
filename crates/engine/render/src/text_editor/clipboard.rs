//! System clipboard seam for the text editor. Copy/cut/paste (b2-t2) read
//! and write through the `Clipboard` trait; `TextEditor::new()` defaults to
//! a real backend and tests inject `FakeClipboard` via `set_clipboard`.
//! b2-t1.

use super::TextEditor;

/// Minimal text clipboard seam. Engine-reusable; tests inject a fake.
pub trait Clipboard {
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str);
}

/// Default backend. `inner` is `None` when no OS clipboard is reachable
/// (e.g. headless CI) so construction is infallible and ops no-op safely.
/// b2-t1.
pub(crate) struct SystemClipboard {
    inner: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    pub(crate) fn new() -> Self {
        Self {
            inner: arboard::Clipboard::new().ok(),
        }
    }
}

impl Clipboard for SystemClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.inner.as_mut().and_then(|c| c.get_text().ok())
    }
    fn set_text(&mut self, text: &str) {
        if let Some(c) = self.inner.as_mut() {
            let _ = c.set_text(text);
        }
    }
}

/// In-memory clipboard double for tests — never touches the real OS
/// clipboard. b2-t1.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeClipboard {
    contents: Option<String>,
}

#[cfg(test)]
impl Clipboard for FakeClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.contents.clone()
    }
    fn set_text(&mut self, text: &str) {
        self.contents = Some(text.to_string());
    }
}

impl TextEditor {
    /// Ctrl+C: write the selected text to the clipboard. No-op (clipboard
    /// left untouched) when there is no active selection. Never mutates the
    /// buffer or selection. b2-t2.
    pub(super) fn copy_selection(&mut self) {
        if let Some(text) = self.selected_text() {
            self.clipboard.set_text(&text);
        }
    }

    /// Ctrl+X: copy the selection then delete its span (via
    /// `replace_selection`, collapsing the caret to the span start and
    /// clearing the selection). Returns `true` iff it cut something; `false`
    /// (no-op, clipboard untouched) when there is no active selection.
    /// b2-t2.
    pub(super) fn cut_selection(&mut self) -> bool {
        if !self.has_selection() {
            return false;
        }
        self.copy_selection();
        self.replace_selection("");
        true
    }

    /// Ctrl+V: insert the clipboard's text at the caret, replacing any
    /// active selection (via `replace_selection`, which re-wraps multi-line
    /// payloads through `insert_char`/`insert_newline`). Returns `true` iff
    /// the clipboard had text to paste; `false` when the clipboard is empty.
    /// b2-t2.
    pub(super) fn paste(&mut self) -> bool {
        let Some(text) = self.clipboard.get_text() else {
            return false;
        };
        self.replace_selection(&text);
        true
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::super::{EditorEvent, Sizing, TextEditor, TextEditorConfig};
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

    #[test]
    fn fake_clipboard_round_trips() {
        let mut clip = FakeClipboard::default();
        clip.set_text("hello");
        assert_eq!(clip.get_text().as_deref(), Some("hello"));
    }

    #[test]
    fn fresh_fake_clipboard_is_empty() {
        let mut clip = FakeClipboard::default();
        assert_eq!(clip.get_text(), None);
    }

    #[test]
    fn editor_accepts_injected_fake_clipboard() {
        let mut ed = TextEditor::new(config());
        ed.set_clipboard(Box::new(FakeClipboard::default()));

        ed.set_text("hello world");

        assert_eq!(ed.text(), "hello world");
    }

    // b2-t2: Ctrl+C / Ctrl+X / Ctrl+V bindings.

    #[test]
    fn copy_then_paste_round_trips_at_caret() {
        let mut ed = TextEditor::new(config());
        ed.set_clipboard(Box::new(FakeClipboard::default()));
        ed.viewport_width = 80;
        ed.set_text("abcdef");
        ed.set_selection((0, 1), (0, 4)); // selects "bcd"

        assert_eq!(ed.handle_key(ctrl_key('c')), EditorEvent::None);
        assert_eq!(ed.text(), "abcdef", "copy must not mutate the buffer");
        assert_eq!(
            ed.selection,
            Some(((0, 1), (0, 4))),
            "copy must not clear the selection"
        );

        ed.handle_key(key(KeyCode::End));
        assert_eq!(ed.handle_key(ctrl_key('v')), EditorEvent::Changed);
        assert_eq!(ed.text(), "abcdefbcd");
    }

    #[test]
    fn cut_copies_and_removes_selection() {
        let mut ed = TextEditor::new(config());
        ed.set_clipboard(Box::new(FakeClipboard::default()));
        ed.viewport_width = 80;
        ed.set_text("abcdef");
        ed.set_selection((0, 1), (0, 4)); // "bcd"

        assert_eq!(ed.handle_key(ctrl_key('x')), EditorEvent::Changed);
        assert_eq!(ed.text(), "aef");
        assert_eq!(ed.selection, None);
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 1));

        // Round-trip through paste proves the cut text landed in the clipboard.
        assert_eq!(ed.handle_key(ctrl_key('v')), EditorEvent::Changed);
        assert_eq!(ed.text(), "abcdef");
    }

    #[test]
    fn multiline_paste_splits_and_rewraps() {
        let mut ed = TextEditor::new(config());
        let mut clip = FakeClipboard::default();
        clip.set_text("X\nY");
        ed.set_clipboard(Box::new(clip));
        ed.viewport_width = 80;
        ed.set_text("ab\ncd");
        ed.handle_key(key(KeyCode::Up));
        ed.handle_key(key(KeyCode::End)); // caret at end of "ab" -> (0, 2)
        assert_eq!((ed.cursor_line, ed.cursor_col), (0, 2));

        assert_eq!(ed.handle_key(ctrl_key('v')), EditorEvent::Changed);

        assert_eq!(ed.text(), "abX\nY\ncd");
        assert_eq!((ed.cursor_line, ed.cursor_col), (1, 1));
    }

    #[test]
    fn copy_with_no_selection_is_noop_clipboard_untouched() {
        let mut ed = TextEditor::new(config());
        let mut clip = FakeClipboard::default();
        clip.set_text("preexisting");
        ed.set_clipboard(Box::new(clip));
        ed.viewport_width = 80;
        ed.set_text("hello");
        assert_eq!(ed.selection, None);

        assert_eq!(ed.handle_key(ctrl_key('c')), EditorEvent::None);
        assert_eq!(ed.text(), "hello");

        ed.handle_key(key(KeyCode::End));
        assert_eq!(ed.handle_key(ctrl_key('v')), EditorEvent::Changed);
        assert_eq!(ed.text(), "hellopreexisting");
    }

    #[test]
    fn cut_with_no_selection_is_noop() {
        let mut ed = TextEditor::new(config());
        let mut clip = FakeClipboard::default();
        clip.set_text("keep");
        ed.set_clipboard(Box::new(clip));
        ed.viewport_width = 80;
        ed.set_text("hello");
        assert_eq!(ed.selection, None);

        assert_eq!(ed.handle_key(ctrl_key('x')), EditorEvent::None);
        assert_eq!(ed.text(), "hello");

        ed.handle_key(key(KeyCode::End));
        assert_eq!(ed.handle_key(ctrl_key('v')), EditorEvent::Changed);
        assert_eq!(ed.text(), "hellokeep");
    }
}
