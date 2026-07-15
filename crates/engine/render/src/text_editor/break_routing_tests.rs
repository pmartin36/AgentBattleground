//! Ctrl-C reaching a focused `TextEditor`'s copy binding through the REAL
//! app key path — `SceneManager::route_key` — rather than a direct
//! `TextEditor::handle_key` call. `route_key` intercepts Ctrl-C globally and
//! only forwards it when the active scene reports `consumes_break()`, so a
//! test that calls `handle_key` directly cannot see whether copy is actually
//! reachable in a running app. This is the only place in the workspace where
//! `route_key`, a real `TextEditor`, and the `FakeClipboard` double are all
//! in scope at once (`FakeClipboard` is `#[cfg(test)]`-gated to this crate).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use engine_core::scene::manager::SceneManager;
use engine_core::scene::{EngineCtx, InputEvent, NoInspect, Scene, Transition};
use engine_core::{FieldSchema, Inspectable, SceneCatalog, SceneKey};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::Frame;
use serde_json::Value as JsonValue;

use super::clipboard::FakeClipboard;
use super::{Sizing, TextEditor, TextEditorConfig};

/// A minimal scene that owns one `TextEditor` and reports it as the holder of
/// the break key while it has focus — the shape every text-field-owning scene
/// implements (`game`'s prompt-editor popup being the real one).
struct EditorScene {
    editor: Rc<RefCell<TextEditor>>,
    inspect: NoInspect,
}

impl Scene for EditorScene {
    fn id(&self) -> SceneKey {
        SceneKey::new("editor")
    }
    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}
    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }
    fn render(&self, _frame: &mut Frame, _area: Rect) {}
    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        if let InputEvent::Key(key) = ev {
            self.editor.borrow_mut().handle_key(key);
        }
        None
    }
    fn exit(&mut self, _ctx: &mut EngineCtx) {}
    fn inspect(&mut self) -> &mut dyn Inspectable {
        &mut self.inspect
    }
    fn consumes_break(&self) -> bool {
        self.editor.borrow().focused()
    }
}

/// `SceneManager::with_scene` never touches the catalog (it boots an
/// already-constructed scene), so these are unreachable stubs.
struct TestCatalog;

impl SceneCatalog for TestCatalog {
    fn construct(&self, _key: &SceneKey) -> Box<dyn Scene> {
        unimplemented!("with_scene boots a pre-built scene")
    }
    fn schema_for(&self, _key: &SceneKey) -> FieldSchema {
        unimplemented!("with_scene boots a pre-built scene")
    }
    fn display_name(&self, _key: &SceneKey) -> &str {
        "editor"
    }
    fn catalog_keys(&self) -> Vec<SceneKey> {
        vec![SceneKey::new("editor")]
    }
    fn is_available(&self, _key: &SceneKey) -> bool {
        true
    }
}

fn config() -> TextEditorConfig {
    TextEditorConfig {
        sizing: Sizing::Fixed,
        submit_on_enter: false,
        placeholder: String::new(),
    }
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Boots an `EditorScene` holding `editor` into a real `SceneManager`,
/// returning the manager and a handle to the editor for read-back.
fn boot(editor: TextEditor) -> (SceneManager, Rc<RefCell<TextEditor>>) {
    let editor = Rc::new(RefCell::new(editor));
    let scene = EditorScene {
        editor: Rc::clone(&editor),
        inspect: NoInspect,
    };
    let mgr = SceneManager::with_scene(Box::new(scene), Box::new(TestCatalog));
    (mgr, editor)
}

/// An editor holding "abcdef" with "bcd" selected and a fake clipboard.
fn selected_editor(focused: bool) -> TextEditor {
    let mut ed = TextEditor::new(config());
    ed.set_clipboard(Box::new(FakeClipboard::default()));
    ed.viewport_width = 80;
    ed.set_text("abcdef");
    ed.set_selection((0, 1), (0, 4)); // "bcd"
    ed.set_focused(focused);
    ed
}

/// THE deliverable: Ctrl-C routed through `route_key` into a focused editor
/// must NOT quit, and must actually copy the selection to the clipboard.
/// The clipboard write is proven by round-tripping through paste (the
/// established pattern in `clipboard.rs`'s own cut/copy tests) — every key
/// travelling the real `route_key` path.
#[test]
fn ctrl_c_through_route_key_copies_selection_without_quitting() {
    let (mut mgr, editor) = boot(selected_editor(true));

    let quit = mgr.route_key(ctrl('c'));
    assert!(
        !quit,
        "Ctrl-C with a focused editor must not quit — it is the copy binding"
    );
    assert_eq!(
        editor.borrow().text(),
        "abcdef",
        "copy must not mutate the buffer"
    );

    // Round-trip: paste at the end must reproduce the copied selection,
    // proving Ctrl-C reached `copy_selection` and wrote to the clipboard.
    mgr.route_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    mgr.route_key(ctrl('v'));

    assert_eq!(
        editor.borrow().text(),
        "abcdefbcd",
        "Ctrl-C must copy the selection to the clipboard through route_key"
    );
}

/// The same editor with focus released reports `consumes_break() == false`,
/// so Ctrl-C keeps its global quit meaning and never reaches the editor.
#[test]
fn ctrl_c_through_route_key_quits_when_editor_unfocused() {
    let (mut mgr, editor) = boot(selected_editor(false));

    let quit = mgr.route_key(ctrl('c'));

    assert!(quit, "Ctrl-C must quit when no editor holds focus");
    assert_eq!(
        editor.borrow().text(),
        "abcdef",
        "a quitting Ctrl-C must not reach the editor"
    );
}

/// Ctrl-Q is the unconditional escape hatch: it quits even while a focused
/// editor is consuming Ctrl-C, and must not reach the editor as a keystroke.
#[test]
fn ctrl_q_through_route_key_quits_past_a_focused_editor() {
    let (mut mgr, editor) = boot(selected_editor(true));

    let quit = mgr.route_key(ctrl('q'));

    assert!(
        quit,
        "Ctrl-Q must quit even while a focused editor consumes Ctrl-C"
    );
    assert_eq!(
        editor.borrow().text(),
        "abcdef",
        "Ctrl-Q must not be forwarded to the editor"
    );
}
