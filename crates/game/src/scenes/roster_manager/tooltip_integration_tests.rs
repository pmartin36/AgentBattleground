//! b3-t1: wires `tooltip::render_tooltip` into `RosterManager::render`,
//! gated on `hovered_ability.is_some()` AND `prompt_editor.is_none()`
//! (spec 49 Testing Guidance). The render-tail block is not yet added (see
//! research.md's blueprint), so the positive-path test below is RED until
//! the code-writer wires it. Row-content/pill-color/anchor-offset internals
//! are covered by `tooltip.rs`'s own tests — these assert only the
//! gating + topmost integration.

use super::*;
use crate::ability::Ability;
use crate::creatures::Creature;
use crate::scenes::test_util::{rect_text, render_to_buffer};

/// A single-field ability (Cost only) — enough to prove the card was drawn
/// with the RIGHT ability's content, without pulling in row-layout details
/// already covered by `tooltip.rs`'s own tests.
fn probe_ability() -> Ability {
    Ability::new("Fire Breath", vec![]).with_cost(3)
}

fn four_probe_abilities() -> Vec<Ability> {
    vec![probe_ability(), probe_ability(), probe_ability(), probe_ability()]
}

/// The tooltip card's cell rect for hovering ability `idx` in `area`, per
/// the same anchor math `render_tooltip` uses internally (b2-t2's
/// `layout_tooltip` — not re-tested here, only reused to locate where to
/// look).
fn card_cell_rect(area: Rect, idx: usize) -> Rect {
    let cell = RosterManager::panel_interior_regions(area).ability_cells[idx];
    tooltip::layout_tooltip(&probe_ability(), cell).card.to_cell_rect()
}

const COST_TEXT: &str = "Cost: 3 stamina";

/// With a hovered ability, no open modal, and no in-flight slide, the card
/// renders above-left of the hovered ability's cell and occludes whatever
/// the panel would otherwise show there.
#[test]
fn hovered_ability_draws_card_above_left_and_occludes_panel() {
    let mut scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    scene.creatures[0] = Creature::new("Test").with_abilities(four_probe_abilities());
    scene.hovered_ability = Some(1);

    let hovered_buf = render_to_buffer(&scene, w, h);

    let mut baseline_scene = RosterManager::new();
    baseline_scene.creatures[0] = Creature::new("Test").with_abilities(four_probe_abilities());
    let baseline_buf = render_to_buffer(&baseline_scene, w, h);

    let card = card_cell_rect(area, 1);
    let ability_cell = RosterManager::panel_interior_regions(area).ability_cells[1].to_cell_rect();

    // The card is anchored to the hovered cell's horizontal CENTER (the ability
    // name is centered in its cell), so its bottom-right sits above-left of that
    // center, not the cell's far-left edge.
    let center_x = ability_cell.x + ability_cell.width / 2;
    assert!(
        card.x + card.width <= center_x && card.y + card.height <= ability_cell.y,
        "the card's bottom-right corner must sit above-left of the hovered ability cell's center-top \
         (card {card:?}, ability cell {ability_cell:?}, center_x {center_x})"
    );
    assert!(
        rect_text(&hovered_buf, card).contains(COST_TEXT),
        "the hovered ability's card must render its Cost row"
    );
    assert!(
        !rect_text(&baseline_buf, card).contains(COST_TEXT),
        "occlusion: the same region without a hover must not already show tooltip content"
    );
}

/// `hovered_ability = None` draws no card anywhere the card would have
/// appeared.
#[test]
fn hovered_ability_none_draws_no_card() {
    let mut scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    scene.creatures[0] = Creature::new("Test").with_abilities(four_probe_abilities());
    scene.hovered_ability = None;

    let buf = render_to_buffer(&scene, w, h);
    let card = card_cell_rect(area, 1);

    assert!(
        !rect_text(&buf, card).contains(COST_TEXT),
        "no tooltip card may be drawn when hovered_ability is None"
    );
}

/// The prompt-editor modal being open suppresses the tooltip even while an
/// ability is hovered.
#[test]
fn modal_open_suppresses_tooltip_even_when_hovered() {
    let mut scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    scene.creatures[0] = Creature::new("Test").with_abilities(four_probe_abilities());
    scene.hovered_ability = Some(1);
    // Hermetic temp base so this never touches the real creature_instructions/.
    let base = std::env::temp_dir().join(format!("game-tooltip-it-modal-{}", std::process::id()));
    scene.prompt_editor = Some(prompt_editor::PromptEditor::new(0, "Test", Some(&base), &[]));

    let buf = render_to_buffer(&scene, w, h);
    let card = card_cell_rect(area, 1);

    assert!(
        !rect_text(&buf, card).contains(COST_TEXT),
        "no tooltip card may be drawn while the prompt-editor modal is open"
    );
}
