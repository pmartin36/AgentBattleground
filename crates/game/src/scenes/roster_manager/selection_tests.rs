//! b3-t1: selection state (Space / current-dot click sets/cancels
//! `selected_index`) and the selected dot's blink render. See research.md's
//! blueprint — `handle_input`/`render_dot_row` are not yet wired to
//! `selected_index`, so these are RED until the code-writer wires them.

use super::*;
use crate::scenes::test_util::{key_event, mouse_event, render_to_buffer, sample_fg};
use crossterm::event::KeyCode;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use engine_core::scene::EngineCtx;

/// Space with no prior selection selects the current creature.
#[test]
fn space_selects_current_when_none_selected() {
    let mut scene = RosterManager::new();
    assert_eq!(scene.selected_index, None);

    scene.handle_input(key_event(KeyCode::Char(' ')));

    assert_eq!(
        scene.selected_index,
        Some(scene.current_index),
        "Space with no prior selection must set selected_index to the current creature"
    );
}

/// A completed click (Moved/Down/Up) on the CURRENT creature's dot does
/// the same as Space.
#[test]
fn click_current_dot_selects() {
    let mut scene = RosterManager::new();
    let (w, h) = (40u16, 20u16);
    // Render once so the dot hit rects are set for this frame's area
    // (handle_input hit-tests against the PREVIOUS frame's render, per
    // the arrow/home button pattern).
    let _ = render_to_buffer(&scene, w, h);

    let area = Rect::new(0, 0, w, h);
    let dots_rect = RosterManager::layout(area).dot_row;
    let slot = RosterManager::dot_slots(dots_rect)[scene.current_index];
    let (cx, cy) = (slot.x, slot.y);

    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

    assert_eq!(
        scene.selected_index,
        Some(scene.current_index),
        "a completed click on the current creature's dot must select it"
    );
}

/// While selected, the selected dot's painted fg alternates between two
/// `BLINK_PERIOD`-separated `elapsed` samples; every other dot's fg is
/// unaffected.
#[test]
fn selected_dot_blinks_across_elapsed() {
    let mut scene = RosterManager::new();
    scene.selected_index = Some(scene.current_index);

    let (w, h) = (40u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    let dots_rect = RosterManager::layout(area).dot_row;
    let slots = RosterManager::dot_slots(dots_rect);
    let selected_slot = slots[scene.current_index];
    let other_slot = slots[(scene.current_index + 1) % slots.len()];

    let buf_a = render_to_buffer(&scene, w, h);
    let selected_fg_a = sample_fg(&buf_a, selected_slot).expect("selected dot must paint");
    let other_fg_a = sample_fg(&buf_a, other_slot).expect("other dot must paint");

    let mut ctx = EngineCtx;
    scene.update(&mut ctx, RosterManager::BLINK_PERIOD);
    let buf_b = render_to_buffer(&scene, w, h);
    let selected_fg_b = sample_fg(&buf_b, selected_slot).expect("selected dot must paint");
    let other_fg_b = sample_fg(&buf_b, other_slot).expect("other dot must paint");

    assert_ne!(
        selected_fg_a, selected_fg_b,
        "selected dot's fg must alternate (blink) across a BLINK_PERIOD-separated elapsed sample"
    );
    assert_eq!(
        other_fg_a, other_fg_b,
        "a non-selected dot's fg must be unaffected by the blink timer"
    );
}

/// Space again with `selected_index == Some(current_index)` (no
/// navigation between) cancels the selection back to `None`.
#[test]
fn space_again_cancels_selection() {
    let mut scene = RosterManager::new();
    scene.selected_index = Some(scene.current_index);

    scene.handle_input(key_event(KeyCode::Char(' ')));

    assert_eq!(
        scene.selected_index, None,
        "Space again at the same current_index (no nav between) must cancel the selection"
    );
}

/// b3-t2: select index 0, navigate to index 1, select again -> the two
/// creatures (by name/identity) are swapped and the selection clears.
#[test]
fn select_navigate_select_swaps_creatures() {
    let mut scene = RosterManager::new();
    let name_at_0_before = scene.creatures[0].name().to_string();
    let name_at_1_before = scene.creatures[1].name().to_string();

    scene.handle_input(key_event(KeyCode::Char(' '))); // select current (0)
    assert_eq!(scene.selected_index, Some(0));

    scene.handle_input(key_event(KeyCode::Right)); // navigate 0 -> 1
    assert_eq!(scene.current_index, 1);

    scene.handle_input(key_event(KeyCode::Char(' '))); // select again -> swap

    assert_eq!(
        scene.selected_index, None,
        "a completed swap must clear the selection"
    );
    assert_eq!(
        scene.creatures[0].name(),
        name_at_1_before,
        "the creature originally at index 1 must now be at index 0"
    );
    assert_eq!(
        scene.creatures[1].name(),
        name_at_0_before,
        "the creature originally at index 0 must now be at index 1"
    );
}

/// b3-t2: swapping an active-slot index with a reserve-slot index flips
/// each CREATURE's squad role (tracked by identity through the swap, not
/// by asserting on the fixed slot indices themselves — `squad_role` is a
/// pure positional lookup, so slot 0 is always Active and the last slot
/// is always Reserve; what must flip is which creature sits where).
#[test]
fn swap_active_with_reserve_flips_role_by_creature_identity() {
    use crate::squad_role::{squad_role, SquadRole, ACTIVE_SLOTS, ROSTER_SIZE};

    let mut scene = RosterManager::new();
    let active_index = 0;
    let reserve_index = ROSTER_SIZE - 1;
    assert!(active_index < ACTIVE_SLOTS, "index 0 must be an active slot");
    assert_eq!(
        squad_role(reserve_index),
        SquadRole::Reserve,
        "the last roster slot must be a reserve slot"
    );

    let active_creature_name = scene.creatures[active_index].name().to_string();
    let reserve_creature_name = scene.creatures[reserve_index].name().to_string();

    // Select the active creature, then jump the cursor directly to the
    // reserve index (bypassing navigate()'s slide-guard, which is not
    // under test here) and select again to swap.
    scene.selected_index = Some(active_index);
    scene.current_index = reserve_index;
    scene.handle_input(key_event(KeyCode::Char(' ')));

    assert_eq!(scene.selected_index, None, "swap must clear the selection");

    let new_index_of_active_creature = scene
        .creatures
        .iter()
        .position(|c| c.name() == active_creature_name)
        .expect("the originally-active creature must still be present");
    let new_index_of_reserve_creature = scene
        .creatures
        .iter()
        .position(|c| c.name() == reserve_creature_name)
        .expect("the originally-reserve creature must still be present");

    assert_eq!(new_index_of_active_creature, reserve_index);
    assert_eq!(new_index_of_reserve_creature, active_index);
    assert_eq!(
        squad_role(new_index_of_active_creature),
        SquadRole::Reserve,
        "the creature moved into the reserve slot must now report Reserve"
    );
    assert_eq!(
        squad_role(new_index_of_reserve_creature),
        SquadRole::Active,
        "the creature moved into the active slot must now report Active"
    );
}

/// b3-t2 guard: re-selecting at the SAME current_index as selected_index
/// (no navigation between) must cancel per b3-t1, NOT swap — the roster
/// order must be unchanged.
#[test]
fn reselect_same_index_cancels_without_reordering_roster() {
    let mut scene = RosterManager::new();
    let names_before: Vec<String> =
        scene.creatures.iter().map(|c| c.name().to_string()).collect();

    scene.handle_input(key_event(KeyCode::Char(' '))); // select current (0)
    assert_eq!(scene.selected_index, Some(0));

    scene.handle_input(key_event(KeyCode::Char(' '))); // same index again -> cancel

    assert_eq!(scene.selected_index, None);
    let names_after: Vec<String> =
        scene.creatures.iter().map(|c| c.name().to_string()).collect();
    assert_eq!(
        names_before, names_after,
        "cancelling a selection must never reorder the roster"
    );
}
