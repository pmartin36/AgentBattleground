//! b4-t2 (iteration 2 delta, research.md): `TextEditor::revision()` — an
//! opaque, monotonic buffer-revision token that changes iff `lines` was
//! written, and by nothing else. This exists because `EditorEvent::Changed`
//! cannot reach every embedder: `handle_mouse` returns `bool`, so a mention
//! accepted by CLICK mutates the buffer with no event any caller could
//! observe. An embedder polling `revision()` at one choke point catches
//! every mutation path instead of wiring `Changed` at each call site and
//! missing the ones that structurally cannot report it.
//!
//! RED (compile-stub): `revision()` does not exist on production
//! `TextEditor` yet. The stub added at `mod.rs` (`unimplemented!()`) makes
//! this module compile; every test below panics at that stub until the
//! code-writer adds the real `revision: u64` field and the bump call in
//! each primitive that writes `lines` (`set_text`, `insert_char`,
//! `insert_newline`, `backspace`, `delete`, `selection::delete_selection`,
//! `history::restore` — and nothing else, per the field's invariant).

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use super::clipboard::{Clipboard, FakeClipboard};
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

fn char_key(c: char) -> KeyEvent {
    key(KeyCode::Char(c))
}

fn ctrl_key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn revision_changes_on_typed_chars() {
    let mut editor = TextEditor::new(config());
    let r0 = editor.revision();

    editor.handle_key(char_key('a'));
    let r1 = editor.revision();
    assert_ne!(r1, r0, "typing a char must change the revision");

    editor.handle_key(char_key('b'));
    let r2 = editor.revision();
    assert_ne!(r2, r1, "a second typed char must change the revision again");
}

#[test]
fn revision_changes_on_enter_backspace_and_delete() {
    let mut editor = TextEditor::new(config());
    editor.set_text("ab");
    editor.cursor_line = 0;
    editor.cursor_col = 1; // caret between 'a' and 'b'
    let r0 = editor.revision();

    editor.handle_key(key(KeyCode::Enter));
    let r1 = editor.revision();
    assert_ne!(r1, r0, "Enter (insert_newline) must change the revision");

    editor.handle_key(key(KeyCode::Backspace));
    let r2 = editor.revision();
    assert_ne!(r2, r1, "Backspace must change the revision");

    editor.handle_key(key(KeyCode::Delete));
    let r3 = editor.revision();
    assert_ne!(r3, r2, "Delete must change the revision");
}

#[test]
fn revision_changes_on_paste_and_cut() {
    let mut editor = TextEditor::new(config());
    let mut clip = FakeClipboard::default();
    clip.set_text("xyz");
    editor.set_clipboard(Box::new(clip));
    let r0 = editor.revision();

    assert_eq!(editor.handle_key(ctrl_key('v')), EditorEvent::Changed);
    let r1 = editor.revision();
    assert_ne!(r1, r0, "paste must change the revision");

    editor.handle_key(key(KeyCode::Home));
    editor.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT)); // select the pasted line
    assert_eq!(editor.handle_key(ctrl_key('x')), EditorEvent::Changed);
    let r2 = editor.revision();
    assert_ne!(r2, r1, "cut must change the revision");
}

#[test]
fn revision_changes_on_undo_and_redo() {
    let mut editor = TextEditor::new(config());
    editor.handle_key(char_key('X'));
    let r0 = editor.revision();

    assert_eq!(editor.handle_key(ctrl_key('z')), EditorEvent::Changed);
    let r1 = editor.revision();
    assert_ne!(r1, r0, "undo (history::restore) must change the revision");

    assert_eq!(editor.handle_key(ctrl_key('y')), EditorEvent::Changed);
    let r2 = editor.revision();
    assert_ne!(r2, r1, "redo (history::restore) must change the revision");
}

#[test]
fn revision_changes_on_set_text() {
    let mut editor = TextEditor::new(config());
    let r0 = editor.revision();

    editor.set_text("hello");
    let r1 = editor.revision();
    assert_ne!(r1, r0, "set_text must change the revision");
}

#[test]
fn revision_unchanged_by_caret_movement_and_page_scroll() {
    let mut editor = TextEditor::new(config());
    editor.set_text("one\ntwo\nthree\nfour\nfive");
    editor.viewport_width = 10;
    editor.viewport_height = 2;
    let r0 = editor.revision();

    for code in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::PageDown,
    ] {
        editor.handle_key(key(code));
    }

    assert_eq!(
        editor.revision(),
        r0,
        "caret movement and page scroll must never change the revision"
    );
}

#[test]
fn revision_unchanged_by_render_and_set_decorations() {
    // The feedback-loop guard: `relint` calls `set_decorations` from inside
    // `flush_pending`, so a bump here would re-dirty on every flush.
    let mut editor = TextEditor::new(config());
    editor.set_text("hello world");
    let r0 = editor.revision();

    let rect = Rect::new(0, 0, 20, 4);
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);
    editor.set_decorations(vec![Decoration {
        span: ((0, 0), (0, 5)),
        bg: ratatui::style::Color::Red,
    }]);

    assert_eq!(
        editor.revision(),
        r0,
        "render and set_decorations must never change the revision"
    );
}

#[test]
fn revision_unchanged_by_no_op_backspace_at_buffer_start() {
    let mut editor = TextEditor::new(config());
    editor.set_text("abc");
    editor.cursor_line = 0;
    editor.cursor_col = 0; // buffer start: backspace() returns false, no-op
    let r0 = editor.revision();

    editor.handle_key(key(KeyCode::Backspace));

    assert_eq!(
        editor.revision(),
        r0,
        "a no-op Backspace at the buffer start must not change the revision"
    );
}
