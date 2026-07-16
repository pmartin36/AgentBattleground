//! b1-t2: mention-popup state machine tests. `TextEditor` fields
//! (`mention_popup`, `cursor_col`, ...) and `MentionPopup`'s fields are
//! readable directly since this module is a descendant of `text_editor`,
//! matching the pattern in `text_editor_tests.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

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

fn type_str(editor: &mut TextEditor, s: &str) {
    for c in s.chars() {
        editor.handle_key(char_key(c));
    }
}

/// Deterministic test double: logs every query it receives (proving the
/// growing-query contract) and returns a hand-picked candidate list per
/// exact query string — enough to drive the specific typed sequences below,
/// without needing to mirror the real game vocabulary's filtering (b2).
struct FakeProvider {
    queries: Rc<RefCell<Vec<String>>>,
}

fn target_candidate(display: &str) -> MentionCandidate {
    MentionCandidate {
        display: display.to_string(),
        insert_text: format!("@{display}:"),
        category: "target",
        continues: true,
    }
}

fn selector_candidate(insert_text: &str) -> MentionCandidate {
    MentionCandidate {
        display: "least-hp".to_string(),
        insert_text: insert_text.to_string(),
        category: "selector",
        continues: false,
    }
}

impl MentionProvider for FakeProvider {
    fn candidates(&self, query: &str) -> Vec<MentionCandidate> {
        self.queries.borrow_mut().push(query.to_string());
        match query {
            "" => vec![target_candidate("enemy"), target_candidate("self")],
            "e" | "en" | "ene" | "enem" | "enemy" => vec![target_candidate("enemy")],
            "enemy:" | "enemy:l" | "enemy:le" | "enemy:lea" | "enemy:leas" | "enemy:least" => {
                vec![selector_candidate("@enemy:least-hp")]
            }
            "least" => vec![selector_candidate("@least-hp")],
            "most" => vec![MentionCandidate {
                display: "most-hp".to_string(),
                insert_text: "@most-hp".to_string(),
                category: "selector",
                continues: false,
            }],
            _ => vec![],
        }
    }
}

/// Attach a fresh `FakeProvider` (trigger `'@'`) and return the shared
/// query log for later inspection.
fn editor_with_provider() -> (TextEditor, Rc<RefCell<Vec<String>>>) {
    let mut editor = TextEditor::new(config());
    let log = Rc::new(RefCell::new(Vec::new()));
    editor.set_mention_provider(
        Box::new(FakeProvider {
            queries: log.clone(),
        }),
        '@',
    );
    (editor, log)
}

#[test]
fn trigger_char_opens_popup_and_queries_provider_with_growing_query() {
    let (mut editor, log) = editor_with_provider();

    editor.handle_key(char_key('@'));
    editor.handle_key(char_key('e'));
    editor.handle_key(char_key('n'));

    assert!(
        editor.mention_popup.is_some(),
        "popup should be open after typing the trigger char"
    );
    assert_eq!(
        *log.borrow(),
        vec!["".to_string(), "e".to_string(), "en".to_string()],
        "provider must be re-queried with the growing query on every char"
    );
    assert_eq!(editor.text(), "@en");
}

#[test]
fn esc_dismisses_popup_leaving_literal_query() {
    let (mut editor, _log) = editor_with_provider();
    type_str(&mut editor, "@e");

    editor.handle_key(key(KeyCode::Esc));

    assert!(editor.mention_popup.is_none(), "Esc must close the popup");
    assert_eq!(
        editor.text(),
        "@e",
        "Esc leaves the literal @query untouched"
    );
}

#[test]
fn enter_on_terminal_candidate_replaces_query_and_closes_popup() {
    let (mut editor, _log) = editor_with_provider();
    type_str(&mut editor, "@least");

    let event = editor.handle_key(key(KeyCode::Enter));

    assert_eq!(event, EditorEvent::Changed);
    assert!(
        editor.mention_popup.is_none(),
        "terminal accept must close the popup"
    );
    assert_eq!(editor.text(), "@least-hp");
    assert_eq!(editor.cursor_line, 0);
    assert_eq!(
        editor.cursor_col,
        "@least-hp".chars().count(),
        "caret lands after the inserted text"
    );
}

#[test]
fn tab_accepts_identically_to_enter() {
    let (mut editor, _log) = editor_with_provider();
    type_str(&mut editor, "@most");

    editor.handle_key(key(KeyCode::Tab));

    assert!(editor.mention_popup.is_none());
    assert_eq!(editor.text(), "@most-hp");
    assert_eq!(editor.cursor_col, "@most-hp".chars().count());
}

#[test]
fn backspace_at_empty_query_deletes_trigger_and_closes_popup() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@'));
    assert!(editor.mention_popup.is_some(), "sanity: popup opened");

    editor.handle_key(key(KeyCode::Backspace));

    assert!(
        editor.mention_popup.is_none(),
        "Backspace with an empty query closes the popup"
    );
    assert_eq!(editor.text(), "", "the trigger char itself is deleted");
}

#[test]
fn backspace_with_nonempty_query_shrinks_query_and_requeries() {
    let (mut editor, log) = editor_with_provider();
    type_str(&mut editor, "@en");

    editor.handle_key(key(KeyCode::Backspace));

    assert!(
        editor.mention_popup.is_some(),
        "Backspace with a remaining non-empty query keeps the popup open"
    );
    assert_eq!(editor.text(), "@e");
    assert_eq!(
        log.borrow().last(),
        Some(&"e".to_string()),
        "shrinking the query re-queries the provider with the shorter query"
    );
}

#[test]
fn two_stage_accept_keeps_popup_open_then_terminal_accept_yields_combined_token() {
    let (mut editor, _log) = editor_with_provider();
    type_str(&mut editor, "@enemy");

    editor.handle_key(key(KeyCode::Enter));
    assert!(
        editor.mention_popup.is_some(),
        "accepting a `continues` candidate must keep the popup open"
    );
    assert_eq!(editor.text(), "@enemy:");

    type_str(&mut editor, "least");
    editor.handle_key(key(KeyCode::Enter));

    assert!(
        editor.mention_popup.is_none(),
        "the second (terminal) accept closes the popup"
    );
    assert_eq!(
        editor.text(),
        "@enemy:least-hp",
        "two-stage accept round-trips to the single combined token"
    );
}

#[test]
fn up_down_move_highlight_without_mutating_buffer_or_caret() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@')); // query "" -> [enemy, self], highlight 0

    editor.handle_key(key(KeyCode::Down));
    assert_eq!(editor.mention_popup.as_ref().unwrap().highlight, 1);
    assert_eq!(editor.text(), "@");
    assert_eq!(editor.cursor_col, 1);

    editor.handle_key(key(KeyCode::Down));
    assert_eq!(
        editor.mention_popup.as_ref().unwrap().highlight,
        0,
        "Down wraps around past the last candidate"
    );

    editor.handle_key(key(KeyCode::Up));
    assert_eq!(
        editor.mention_popup.as_ref().unwrap().highlight,
        1,
        "Up wraps backward past the first candidate"
    );
    assert_eq!(editor.text(), "@", "nav never mutates the buffer");
}

#[test]
fn no_provider_attached_trigger_char_inserts_normally_no_popup() {
    let mut editor = TextEditor::new(config());

    editor.handle_key(char_key('@'));
    editor.handle_key(char_key('x'));

    assert_eq!(
        editor.text(),
        "@x",
        "with no provider attached, '@' is just a normal character"
    );
    assert!(editor.mention_popup.is_none());
}

// --- b1-t3: popup rendering (inline overlay at caret) + mouse-click accept ---

fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// Scan `buf` for the first row containing `needle` as a contiguous run of
/// single-char-symbol cells (candidate `display` strings render as plain
/// text, one char per cell); returns the `(x, y)` of the match's first cell.
///
/// Matches by CHAR/column index, not byte offset: a row may contain
/// multi-byte braille chrome cells (e.g. the popup's border), so `str::find`
/// (a byte offset) would misalign against the one-char-per-cell column
/// count. Each cell contributes exactly one `char` to `row_chars`, so a
/// `windows` match's position IS the column offset directly.
fn find_candidate_row(buf: &Buffer, area: Rect, needle: &str) -> Option<(u16, u16)> {
    let needle_chars: Vec<char> = needle.chars().collect();
    for y in area.top()..area.bottom() {
        let row_chars: Vec<char> = (area.left()..area.right())
            .map(|x| {
                buf.cell((x, y))
                    .unwrap()
                    .symbol()
                    .chars()
                    .next()
                    .unwrap_or(' ')
            })
            .collect();
        if let Some(idx) = row_chars
            .windows(needle_chars.len().max(1))
            .position(|w| w == needle_chars.as_slice())
        {
            return Some((area.x + idx as u16, y));
        }
    }
    None
}

fn popup_test_rect() -> Rect {
    Rect::new(0, 0, 40, 12)
}

#[test]
fn popup_renders_candidate_displays_near_caret_with_highlight() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@')); // query "" -> [enemy, self], highlight 0

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);

    let (ex, ey) = find_candidate_row(&buf, rect, "enemy")
        .expect("highlighted candidate 'enemy' must be drawn");
    let (sx, sy) = find_candidate_row(&buf, rect, "self").expect("candidate 'self' must be drawn");

    for x in ex..ex + "enemy".len() as u16 {
        assert_eq!(
            buf.cell((x, ey)).unwrap().bg,
            super::render::MENTION_HIGHLIGHT_BG,
            "highlighted row (index 0, 'enemy') must carry the highlight bg"
        );
    }
    assert_ne!(
        buf.cell((sx, sy)).unwrap().bg,
        super::render::MENTION_HIGHLIGHT_BG,
        "non-highlighted row ('self') must not carry the highlight bg"
    );
}

#[test]
fn popup_chrome_is_braille_not_raw_glyph() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@'));

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);

    let mut found_braille = false;
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            if crate::decode_braille_cell(&buf, x, y).is_some() {
                found_braille = true;
            }
            let ch = buf
                .cell((x, y))
                .unwrap()
                .symbol()
                .chars()
                .next()
                .unwrap_or(' ');
            assert!(
                !('\u{2500}'..='\u{257F}').contains(&ch),
                "no raw box-drawing glyph expected anywhere in the render, found {ch:?} at ({x},{y})"
            );
        }
    }
    assert!(
        found_braille,
        "popup chrome must come from the dot pipeline (at least one decodable braille cell)"
    );
}

#[test]
fn mouse_click_on_terminal_candidate_row_accepts_and_closes() {
    let (mut editor, _log) = editor_with_provider();
    type_str(&mut editor, "@least"); // FakeProvider -> terminal "@least-hp"

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);
    let (cx, cy) =
        find_candidate_row(&buf, rect, "least-hp").expect("candidate 'least-hp' must be drawn");

    let handled = editor.handle_mouse(&mouse_at(MouseEventKind::Down(MouseButton::Left), cx, cy));

    assert!(handled, "a click on a candidate row must be handled");
    assert_eq!(editor.text(), "@least-hp");
    assert!(
        editor.mention_popup.is_none(),
        "clicking a terminal candidate closes the popup"
    );
}

#[test]
fn mouse_click_on_continues_candidate_keeps_popup_open() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@')); // candidates [enemy, self], both continues

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);
    let (cx, cy) = find_candidate_row(&buf, rect, "self").expect("candidate 'self' must be drawn");

    editor.handle_mouse(&mouse_at(MouseEventKind::Down(MouseButton::Left), cx, cy));

    assert_eq!(editor.text(), "@self:");
    assert!(
        editor.mention_popup.is_some(),
        "clicking a continues candidate keeps the popup open"
    );
}

#[test]
fn mouse_click_outside_popup_does_not_accept() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@'));

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);

    // Far bottom-right corner of a generously sized buffer — well outside
    // the small popup anchored near the top-left caret.
    editor.handle_mouse(&mouse_at(
        MouseEventKind::Down(MouseButton::Left),
        rect.right() - 1,
        rect.bottom() - 1,
    ));

    assert_eq!(
        editor.text(),
        "@",
        "a click outside the popup must not insert a candidate"
    );
    assert!(
        editor.mention_popup.is_some(),
        "popup must remain open on an outside click"
    );
}

#[test]
fn mouse_hover_over_row_moves_highlight_without_accepting() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@')); // candidates [enemy, self], highlight 0

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);
    let (cx, cy) = find_candidate_row(&buf, rect, "self").expect("candidate 'self' must be drawn");

    let handled = editor.handle_mouse(&mouse_at(MouseEventKind::Moved, cx, cy));

    assert!(handled, "hover over a candidate row must be handled");
    assert_eq!(
        editor.mention_popup.as_ref().unwrap().highlight,
        1,
        "hover must move the highlight to the hovered row ('self')"
    );
    assert_eq!(editor.text(), "@", "hover must NOT accept — buffer unchanged");
    assert!(
        editor.mention_popup.is_some(),
        "hover must NOT close the popup"
    );
}

#[test]
fn mouse_hover_outside_popup_leaves_highlight_unchanged() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@')); // highlight 0

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);

    let handled =
        editor.handle_mouse(&mouse_at(MouseEventKind::Moved, rect.right() - 1, rect.bottom() - 1));

    assert!(!handled, "a hover outside the popup list is not handled");
    assert_eq!(
        editor.mention_popup.as_ref().unwrap().highlight,
        0,
        "highlight unchanged by an outside hover"
    );
}

#[test]
fn popup_border_renders_in_distinct_color() {
    let (mut editor, _log) = editor_with_provider();
    editor.handle_key(char_key('@'));

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);

    // The popup border must render in MENTION_BORDER (a light green),
    // distinct from the grey field-box border, so the popup reads as its own
    // element.
    let c = super::render::MENTION_BORDER;
    let want = ratatui::style::Color::Rgb(c.r, c.g, c.b);
    let found = (0..rect.height)
        .any(|y| (0..rect.width).any(|x| buf.cell((x, y)).unwrap().fg == want));
    assert!(found, "popup border must render in the distinct MENTION_BORDER color");
    assert_ne!(
        want,
        ratatui::style::Color::Rgb(0x8a, 0x8a, 0x8a),
        "popup border color must differ from the grey field-box border"
    );
}

// --- b4-t2 (iteration 2 delta): both mention-accept paths bump `revision` ---
// RED (compile-stub): `revision()` panics via the `unimplemented!()` stub
// (mod.rs) until the code-writer wires the real field.

#[test]
fn tab_accepting_a_mention_changes_revision() {
    let (mut editor, _log) = editor_with_provider();
    type_str(&mut editor, "@most");
    let r0 = editor.revision();

    editor.handle_key(key(KeyCode::Tab));

    assert_ne!(
        editor.revision(),
        r0,
        "Tab-accepting a mention candidate must change the revision"
    );
}

#[test]
fn click_accepting_a_mention_changes_revision() {
    // The case the whole finding turns on: `handle_mouse` returns `bool`, so
    // the accept's `EditorEvent::Changed` is destroyed inside the engine
    // before any caller can observe it. `revision` is the only channel.
    let (mut editor, _log) = editor_with_provider();
    type_str(&mut editor, "@least"); // FakeProvider -> terminal "@least-hp"

    let rect = popup_test_rect();
    let mut buf = Buffer::empty(rect);
    editor.render(&mut buf, rect);
    let (cx, cy) =
        find_candidate_row(&buf, rect, "least-hp").expect("candidate 'least-hp' must be drawn");
    let r0 = editor.revision();

    let handled = editor.handle_mouse(&mouse_at(MouseEventKind::Down(MouseButton::Left), cx, cy));

    assert!(handled, "sanity: the click must be handled");
    assert_eq!(editor.text(), "@least-hp", "sanity: the click must accept the candidate");
    assert_ne!(
        editor.revision(),
        r0,
        "click-accepting a mention candidate must change the revision"
    );
}

#[test]
fn no_popup_render_and_mouse_click_are_unaffected() {
    let mut editor = TextEditor::new(config());
    editor.set_text("hello world");
    let rect = Rect::new(0, 0, 20, 5);
    let mut buf = Buffer::empty(rect);

    editor.render(&mut buf, rect); // must not panic: no provider/popup attached

    assert_eq!(buf.cell((0, 0)).unwrap().symbol(), "h");

    let handled = editor.handle_mouse(&mouse_at(MouseEventKind::Down(MouseButton::Left), 2, 0));
    assert!(
        handled,
        "a normal in-rect click still places the caret when there is no popup"
    );
    assert_eq!((editor.cursor_line, editor.cursor_col), (0, 2));
}
