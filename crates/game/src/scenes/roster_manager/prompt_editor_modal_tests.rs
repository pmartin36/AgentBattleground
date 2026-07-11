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
use crate::scenes::test_util::{key_event, mouse_event, render_to_buffer};
use engine_core::scene::EngineCtx;
use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};
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
    scene.prompt_editor = Some(prompt_editor::PromptEditor::new(idx, &name, base));
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
