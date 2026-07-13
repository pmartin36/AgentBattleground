//! b3-t1: modal input routing + close (X / Esc) + cached-preview refresh
//! (spec 51 Testing Guidance lines 63,65). While `prompt_editor.is_some()`,
//! ALL input must route to the popup and normal roster bindings (nav,
//! ability hover) must not run; `Esc` or a completed X click must close the
//! popup and refresh `current_instructions` from disk. RED until the
//! code-writer wires the modal guard in `handle_input` (research.md's
//! blueprint) — mirrors edit_button_tests.rs / ability_hover_tests.rs's
//! render-then-hit-test pattern.

use super::*;
use crate::ability::Ability;
use crate::creatures::Creature;
use crate::scenes::test_util::{key_event, mouse_event, rect_text, render_to_buffer};
use engine_core::scene::EngineCtx;
use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::style::Modifier;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique per-test temp base dir (pid + monotonic counter), mirroring
/// `instructions_cache_tests.rs`'s own helper.
fn temp_base_dir(tag: &str) -> std::path::PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "game-roster-prompt-editor-modal-test-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ))
}

/// Opens the popup for the CURRENT creature directly (private-field
/// construction, per research.md's test-setup note), skipping the Edit
/// button click so each test controls the base dir independently.
fn open_popup(scene: &mut RosterManager, base: Option<&Path>) {
    let idx = scene.current_index;
    let name = scene.creatures[idx].name().to_string();
    scene.prompt_editor = Some(prompt_editor::PromptEditor::new(idx, &name, base, &scene.creatures));
}

fn four_abilities() -> Vec<Ability> {
    vec![
        Ability::new("Fire Breath", vec![]),
        Ability::new("Ice Shard", vec![]),
        Ability::new("Rock Throw", vec![]),
        Ability::new("Wind Gust", vec![]),
    ]
}

/// While the popup is open, Left/Right must not change `current_index`.
#[test]
fn modal_suppresses_left_right_nav() {
    let mut scene = RosterManager::new();
    open_popup(&mut scene, None);
    let before = scene.current_index;

    scene.handle_input(key_event(KeyCode::Left));
    scene.handle_input(key_event(KeyCode::Right));

    assert_eq!(
        scene.current_index, before,
        "Left/Right must not navigate while the prompt-editor popup is open"
    );
}

/// While the popup is open, a `Moved` mouse event over a populated ability
/// cell must not update `hovered_ability`.
#[test]
fn modal_suppresses_ability_hover() {
    let mut scene = RosterManager::new();
    scene.creatures[0] = Creature::new("Test").with_abilities(four_abilities());
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);

    // Populate ability_hit_rects while the popup is still closed.
    let _ = render_to_buffer(&scene, w, h);

    open_popup(&mut scene, None);

    let cells = RosterManager::panel_interior_regions(area).ability_cells;
    let cell = cells[1].to_cell_rect();
    let (cx, cy) = (cell.x + cell.width / 2, cell.y + cell.height / 2);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));

    assert_eq!(
        scene.hovered_ability, None,
        "ability hover must not update while the prompt-editor popup is open"
    );
}

/// `Esc` closes the popup.
#[test]
fn esc_closes_popup() {
    let mut scene = RosterManager::new();
    open_popup(&mut scene, None);
    assert!(scene.prompt_editor.is_some(), "setup: popup must be open");

    scene.handle_input(key_event(KeyCode::Esc));

    assert!(scene.prompt_editor.is_none(), "Esc must close the prompt-editor popup");
}

/// A completed click (Moved -> Down -> Up inside) on the close (X) hit-rect
/// closes the popup.
#[test]
fn x_click_closes_popup() {
    let base = temp_base_dir("x-click-closes");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);

    open_popup(&mut scene, Some(&base));
    let _ = render_to_buffer(&scene, w, h);

    let close = prompt_editor::PromptEditor::compute_layout(area, 1).close.to_cell_rect();
    let (cx, cy) = (close.x + close.width / 2, close.y + close.height / 2);

    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

    assert!(
        scene.prompt_editor.is_none(),
        "a completed click on the close (X) must close the prompt-editor popup"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Closing the popup refreshes `current_instructions` from disk.
#[test]
fn close_refreshes_cached_instructions() {
    let base = temp_base_dir("close-refreshes");
    let name = "Ember Wolf"; // demo_roster()'s first entry (instructions_cache_tests::CREATURE_0)
    crate::instructions::write_instructions_in(&base, name, "initial")
        .expect("seed write should succeed");

    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    crate::instructions::write_instructions_in(&base, name, "new body")
        .expect("edit write should succeed");

    scene.handle_input(key_event(KeyCode::Esc));

    assert_eq!(
        scene.current_instructions, "new body",
        "closing the popup must reload current_instructions from disk"
    );

    let _ = std::fs::remove_dir_all(&base);
}

// ── b3-t2: live debounced write-through + update tick ──────────────────────
// RED until the code-writer wires `PromptEditor::mark_dirty`/`update`/
// `flush_pending` (research.md's blueprint): the instructions editor's
// `Changed` event must restart a `WRITE_DEBOUNCE` countdown, the scene's
// `update(dt)` must tick it while the popup is open, and closing must flush
// any pending write BEFORE `reload_instructions` (spec 51 Testing Guidance
// line 62). Today, `handle_input`'s key branch discards the editor's
// `EditorEvent` entirely, so no write ever reaches disk.

/// Typing into the instructions editor, then advancing `update` past the
/// debounce, must flush the edited buffer to disk.
#[test]
fn debounce_write_persists_after_elapse() {
    let base = temp_base_dir("debounce-elapse");
    let name = "Ember Wolf";
    crate::instructions::write_instructions_in(&base, name, "seed")
        .expect("seed write should succeed");

    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    scene.handle_input(key_event(KeyCode::Char('h')));
    scene.handle_input(key_event(KeyCode::Char('i')));

    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(400));

    let on_disk = crate::instructions::read_instructions_in(&base, name).expect("read should succeed");
    assert_eq!(on_disk, "seedhi", "debounced write must persist after the debounce elapses");

    let _ = std::fs::remove_dir_all(&base);
}

/// Before the debounce elapses, disk must be unchanged.
#[test]
fn no_write_before_debounce_elapses() {
    let base = temp_base_dir("debounce-no-early");
    let name = "Ember Wolf";
    crate::instructions::write_instructions_in(&base, name, "seed")
        .expect("seed write should succeed");

    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    scene.handle_input(key_event(KeyCode::Char('h')));

    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(100));

    let on_disk = crate::instructions::read_instructions_in(&base, name).expect("read should succeed");
    assert_eq!(on_disk, "seed", "no write should occur before the debounce elapses");

    let _ = std::fs::remove_dir_all(&base);
}

/// A burst of keystrokes, each followed by a sub-debounce tick, then one
/// tick past the debounce, must coalesce into a single final write.
#[test]
fn keystroke_burst_coalesces() {
    let base = temp_base_dir("burst-coalesce");
    let name = "Ember Wolf";
    crate::instructions::write_instructions_in(&base, name, "seed")
        .expect("seed write should succeed");

    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    let mut ctx = EngineCtx;
    for ch in ['a', 'b', 'c'] {
        scene.handle_input(key_event(KeyCode::Char(ch)));
        scene.update(&mut ctx, Duration::from_millis(100));
    }
    // Past the debounce, measured from the final keystroke.
    scene.update(&mut ctx, Duration::from_millis(400));

    let on_disk = crate::instructions::read_instructions_in(&base, name).expect("read should succeed");
    assert_eq!(on_disk, "seedabc", "keystroke burst must coalesce into one final write");

    let _ = std::fs::remove_dir_all(&base);
}

/// Closing (Esc) with a pending edit — WITHOUT advancing `update` past the
/// debounce — must still flush the pending write before the on-close reload,
/// so `current_instructions` reflects the edit, not stale disk.
#[test]
fn close_flushes_pending_before_reload() {
    let base = temp_base_dir("close-flushes-pending");
    let name = "Ember Wolf";
    crate::instructions::write_instructions_in(&base, name, "seed")
        .expect("seed write should succeed");

    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    scene.handle_input(key_event(KeyCode::Char('h')));
    scene.handle_input(key_event(KeyCode::Char('i')));

    scene.handle_input(key_event(KeyCode::Esc));

    assert_eq!(
        scene.current_instructions, "seedhi",
        "closing with a pending edit must flush before reload, not reload stale disk"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Typing into the AGENT input (not the instructions editor) must never
/// write the instructions file — only the instructions editor's `Changed`
/// marks the popup dirty.
#[test]
fn agent_input_change_does_not_write_instructions() {
    let base = temp_base_dir("agent-input-no-write");
    let name = "Ember Wolf";
    crate::instructions::write_instructions_in(&base, name, "seed")
        .expect("seed write should succeed");

    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    scene.handle_input(key_event(KeyCode::Tab)); // focus -> AgentInput
    scene.handle_input(key_event(KeyCode::Char('h')));

    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(400));

    let on_disk = crate::instructions::read_instructions_in(&base, name).expect("read should succeed");
    assert_eq!(on_disk, "seed", "typing into the agent input must not write the instructions file");

    let _ = std::fs::remove_dir_all(&base);
}

// ── b2-t1: click-to-focus routing between the two editors ──────────────────
// RED until the code-writer wires click-driven focus in
// `PromptEditor::handle_input`'s mouse branch (research.md's blueprint):
// today a `Down(Left)` inside a field box already relocates that editor's
// OWN caret (b1-t2's `TextEditor::handle_mouse` guard), but never updates
// `PromptEditor::focus`, so which editor actually RECEIVES subsequent
// keystrokes is unaffected by where the user clicked — only `Tab` moves
// keyboard focus. These tests type a marker character right after a click
// and read back the two fields' rendered text (no accessor into
// `PromptEditor`'s private `focus` field needed, mirroring how this sibling
// test module already reads state — `scene.current_instructions`,
// `scene.prompt_editor` — rather than reaching into `PromptEditor` itself).

const AREA_W: u16 = 80;
const AREA_H: u16 = 30;

/// Center point, in cell space, of a field's actual editable content area —
/// the field's braille-box rect inset by the 1-cell border every side
/// (mirrors `PromptEditor::field_content`'s cell math), so the click lands
/// inside the `TextEditor`'s own cached viewport, not on its border.
fn field_content_center(field_box: Rect) -> (u16, u16) {
    let inner = Rect::new(
        field_box.x + 1,
        field_box.y + 1,
        field_box.width.saturating_sub(2),
        field_box.height.saturating_sub(2),
    );
    (inner.x + inner.width / 2, inner.y + inner.height / 2)
}

/// Renders `scene`, types `ch` into whichever editor currently holds
/// keyboard focus, then returns `(agent_input_text, instructions_text)` read
/// back from a fresh render — a purely observable proxy for "which editor
/// received the keystroke".
fn type_and_read_fields(scene: &mut RosterManager, ch: char) -> (String, String) {
    scene.handle_input(key_event(KeyCode::Char(ch)));

    let buf = render_to_buffer(scene, AREA_W, AREA_H);
    let layout =
        prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, AREA_W, AREA_H), 1);
    let agent_text = rect_text(&buf, layout.agent_input.to_cell_rect());
    let instructions_text =
        rect_text(&buf, layout.instructions.to_cell_rect());
    (agent_text, instructions_text)
}

/// A `Down(Left)` inside the agent-input field must focus it (default focus
/// on open is Instructions) — a keystroke right after the click must land in
/// the agent input, not the instructions editor.
#[test]
fn click_in_agent_input_focuses_it() {
    let base = temp_base_dir("click-focuses-agent");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    let _ = render_to_buffer(&scene, AREA_W, AREA_H); // caches both editors' click viewports
    let layout =
        prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, AREA_W, AREA_H), 1);
    let (cx, cy) = field_content_center(layout.agent_input.to_cell_rect());
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));

    let (agent_text, instructions_text) = type_and_read_fields(&mut scene, 'z');
    assert!(
        agent_text.contains('z'),
        "a click in the agent-input field must focus it so typing lands there, got {agent_text:?}"
    );
    assert!(
        !instructions_text.contains('z'),
        "a click in the agent-input field must not route typing to the instructions editor, got {instructions_text:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// A `Down(Left)` inside the instructions field must (re-)focus it, moving
/// keyboard focus away from a previously Tab-focused agent input.
#[test]
fn click_in_instructions_focuses_it() {
    let base = temp_base_dir("click-focuses-instructions");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));
    scene.handle_input(key_event(KeyCode::Tab)); // focus -> AgentInput

    let _ = render_to_buffer(&scene, AREA_W, AREA_H);
    let layout =
        prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, AREA_W, AREA_H), 1);
    let (cx, cy) = field_content_center(layout.instructions.to_cell_rect());
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));

    let (agent_text, instructions_text) = type_and_read_fields(&mut scene, 'z');
    assert!(
        instructions_text.contains('z'),
        "a click in the instructions field must focus it so typing lands there, got {instructions_text:?}"
    );
    assert!(
        !agent_text.contains('z'),
        "a click in the instructions field must not route typing to the agent input, got {agent_text:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// A `Down(Left)` inside the popup but outside both field boxes (the
/// file-path row) must leave keyboard focus unchanged — a keystroke right
/// after still lands in whichever field was already focused.
#[test]
fn click_outside_both_fields_leaves_focus_unchanged() {
    let base = temp_base_dir("click-outside-leaves-focus");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));
    scene.handle_input(key_event(KeyCode::Tab)); // focus -> AgentInput

    let _ = render_to_buffer(&scene, AREA_W, AREA_H);
    let layout =
        prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, AREA_W, AREA_H), 1);
    let file_path = layout.file_path.to_cell_rect();
    let (cx, cy) = (file_path.x + file_path.width / 2, file_path.y + file_path.height / 2);
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));

    let (agent_text, instructions_text) = type_and_read_fields(&mut scene, 'z');
    assert!(
        agent_text.contains('z'),
        "a click outside both fields must leave focus on the already-focused agent input, got {agent_text:?}"
    );
    assert!(
        !instructions_text.contains('z'),
        "a click outside both fields must not move focus to the instructions editor, got {instructions_text:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

// ── b2-t2: dt forwarded to both editors' tick + focus drives caret ─────────
// RED until the code-writer (1) un-gates `PromptEditor::update`'s early
// return so `TextEditor::tick` reaches both editors every frame and (2)
// `PromptEditor::render` calls `TextEditor::set_focused` per `self.focus`
// (research.md's blueprint). Today `render` never calls `set_focused`, and
// both `TextEditor`s default `focused: true`, so BOTH fields show a
// reverse-video caret regardless of which one has keyboard focus.

/// True iff any cell in `rect` carries `Modifier::REVERSED` — the caret
/// block-cursor signal `TextEditor::render` draws (mirrors
/// `text_editor_tests.rs`'s own REVERSED-cell assertions).
fn rect_has_reversed(buf: &Buffer, rect: Rect) -> bool {
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            if buf.cell((x, y)).unwrap().modifier.contains(Modifier::REVERSED) {
                return true;
            }
        }
    }
    false
}

/// A field's editable content rect inside its braille box — the box rect
/// inset by the 1-cell border every side (mirrors
/// `field_content_center`/`PromptEditor::field_content`'s cell math), so the
/// scan covers only cells `TextEditor::render` actually draws into, never the
/// border (which is never REVERSED).
fn field_inner_rect(field_box: Rect) -> Rect {
    Rect::new(
        field_box.x + 1,
        field_box.y + 1,
        field_box.width.saturating_sub(2),
        field_box.height.saturating_sub(2),
    )
}

/// A freshly opened popup (default focus is Instructions) must show a caret
/// in the instructions field only. Both fields are seeded with non-empty
/// text first — `TextEditor::render` early-returns (no caret at all) while a
/// field's text is empty, so an empty agent input could pass this check
/// vacuously without the focus gate actually working; this seeds both so the
/// assertion is meaningful for both fields.
#[test]
fn single_caret_shows_in_focused_editor_only() {
    let base = temp_base_dir("single-caret");
    let name = "Ember Wolf";
    crate::instructions::write_instructions_in(&base, name, "seed text")
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    scene.handle_input(key_event(KeyCode::Tab)); // focus -> AgentInput
    scene.handle_input(key_event(KeyCode::Char('a'))); // give it non-empty text
    scene.handle_input(key_event(KeyCode::Tab)); // focus back -> Instructions (default)

    let buf = render_to_buffer(&scene, AREA_W, AREA_H);
    let layout = prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, AREA_W, AREA_H), 1);
    let instructions_inner = field_inner_rect(layout.instructions.to_cell_rect());
    let agent_inner = field_inner_rect(layout.agent_input.to_cell_rect());

    assert!(
        rect_has_reversed(&buf, instructions_inner),
        "focus is on Instructions; it must show a caret"
    );
    assert!(
        !rect_has_reversed(&buf, agent_inner),
        "the unfocused (but non-empty) agent input must show no caret"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// After `Tab` moves keyboard focus to the agent input, the caret must
/// follow: agent input shows it, instructions no longer does (even though
/// instructions still has seeded text) — guards the per-field `set_focused`
/// wiring, not just "one of the two shows a caret".
#[test]
fn caret_follows_focus_after_tab() {
    let base = temp_base_dir("caret-follows-tab");
    let name = "Ember Wolf";
    crate::instructions::write_instructions_in(&base, name, "seed text")
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_popup(&mut scene, Some(&base));

    scene.handle_input(key_event(KeyCode::Tab)); // focus -> AgentInput
    scene.handle_input(key_event(KeyCode::Char('a'))); // give it non-empty text

    let buf = render_to_buffer(&scene, AREA_W, AREA_H);
    let layout = prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, AREA_W, AREA_H), 1);
    let instructions_inner = field_inner_rect(layout.instructions.to_cell_rect());
    let agent_inner = field_inner_rect(layout.agent_input.to_cell_rect());

    assert!(
        rect_has_reversed(&buf, agent_inner),
        "after Tab, the agent input must show the caret"
    );
    assert!(
        !rect_has_reversed(&buf, instructions_inner),
        "after Tab, the instructions editor (though it has text) must show no caret"
    );

    let _ = std::fs::remove_dir_all(&base);
}
