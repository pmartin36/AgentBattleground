//! b3-t2: hover-only hit-testing over the details panel's 4 ability cells
//! (spec 48 Testing Guidance line 86). `hovered_ability` is set from a
//! `Mouse(Moved)` event against the CURRENT frame's populated ability cell
//! rects (`panel_interior_regions(area).ability_cells`, gated on the
//! creature's real ability count) — mirrors edit_button_tests.rs's
//! render-then-hit-test pattern. RED until the code-writer wires the
//! render()-refresh + handle_input(Moved) logic (research.md's blueprint).

use super::*;
use crate::ability::Ability;
use crate::creatures::Creature;
use crate::scenes::test_util::{mouse_event, render_to_buffer};
use ratatui::crossterm::event::MouseEventKind;

fn cell_center(cells: &[engine_render::DotRect; 4], i: usize) -> (u16, u16) {
    let c = cells[i].to_cell_rect();
    (c.x + c.width / 2, c.y + c.height / 2)
}

fn four_abilities() -> Vec<Ability> {
    vec![
        Ability::new("Fire Breath", vec![]),
        Ability::new("Ice Shard", vec![]),
        Ability::new("Rock Throw", vec![]),
        Ability::new("Wind Gust", vec![]),
    ]
}

/// A `Moved` event inside a populated ability cell's rect sets
/// `hovered_ability` to that cell's index (spec Testing Guidance line 86).
#[test]
fn moved_inside_populated_ability_sets_hovered() {
    let mut scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    scene.creatures[0] = Creature::new("Test").with_abilities(four_abilities());

    // Render once so the hit rects are set for this frame (handle_input
    // hit-tests against the PREVIOUS frame's render, per
    // edit_button_tests.rs::edit_button_click_opens_prompt_editor).
    let _ = render_to_buffer(&scene, w, h);

    let cells = RosterManager::panel_interior_regions(area).ability_cells;
    let (cx, cy) = cell_center(&cells, 1);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));

    assert_eq!(
        scene.hovered_ability,
        Some(1),
        "a Moved event inside ability cell 1's rect must set hovered_ability to Some(1)"
    );
}

/// A `Moved` event outside every populated ability cell clears
/// `hovered_ability` back to `None`.
#[test]
fn moved_outside_all_abilities_clears() {
    let mut scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    scene.creatures[0] = Creature::new("Test").with_abilities(four_abilities());

    let _ = render_to_buffer(&scene, w, h);

    let cells = RosterManager::panel_interior_regions(area).ability_cells;
    let (cx, cy) = cell_center(&cells, 0);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    assert_eq!(
        scene.hovered_ability,
        Some(0),
        "setup: expected an initial hover before clearing"
    );

    // (0, 0) is the top-left corner, well outside the right-column details
    // panel's ability grid.
    scene.handle_input(mouse_event(MouseEventKind::Moved, 0, 0));

    assert_eq!(
        scene.hovered_ability, None,
        "a Moved event outside every populated ability cell must clear hovered_ability"
    );
}

/// An empty ability slot (index >= abilities().len()) is never a hover
/// target, even when the cursor sits inside its cell's geometric rect.
#[test]
fn empty_slot_has_no_hit_target() {
    let mut scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    scene.creatures[0] = Creature::new("Test").with_abilities(vec![
        Ability::new("Fire Breath", vec![]),
        Ability::new("Ice Shard", vec![]),
    ]);

    let _ = render_to_buffer(&scene, w, h);

    let cells = RosterManager::panel_interior_regions(area).ability_cells;
    let (cx, cy) = cell_center(&cells, 3);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));

    assert_eq!(
        scene.hovered_ability, None,
        "cell 3's geometric rect must not be a hover target for a 2-ability creature"
    );
}
