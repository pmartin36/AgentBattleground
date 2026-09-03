//! Empty-dock settled view tests: once the last owned egg hatches and its
//! Keep/Discard action dismisses, the just-hatched creature stays visible
//! read-only with full-opacity stat bars instead of leaving the scene at an
//! empty browse layout.

use ratatui::style::Color;

use crate::ability::Ability;
use crate::scenes::stat_bar::STAT_BAR_COLOR;
use crate::stamina::Stamina;
use crate::stats::Stats;

use super::hatch_sequence_tests as hsq;
use super::*;

/// A hatchling with nonzero stats, so a stat-bar fill assertion has a
/// reachable green-dominant cell to find (an all-default-stats creature
/// legitimately renders zero fill and would make such an assertion
/// unsatisfiable regardless of opacity).
fn hatchling_with_nonzero_stats(name: &str) -> PersistedCreature {
    PersistedCreature::new(
        name,
        Element::Fire,
        Stats { strength: 7, dexterity: 11, intelligence: 13, vitality: 17 },
        1,
        0,
        vec![Ability::new("Fire Breath", vec![])],
        Stamina::default(),
        None,
        None,
        None,
    )
}

/// Ticks `advance_hatch` until the sequence completes, or panics if the
/// timeline runs out first (a fixture bug, not a real timeout).
fn advance_to_complete(scene: &mut Hatchery) {
    for _ in 0..2000 {
        if scene.hatch.as_ref().unwrap().seq.is_complete() {
            return;
        }
        scene.advance_hatch(Duration::from_millis(5));
    }
    panic!("hatch sequence never completed");
}

/// Rightmost cell within `rect` whose fg is green-dominant — the shared
/// stat-bar renderer's fill blends green-dominant, its grey border chrome
/// does not.
fn rightmost_green(buf: &ratatui::buffer::Buffer, rect: Rect) -> Option<(u16, u16)> {
    (rect.left()..rect.right()).rev().find_map(|x| {
        (rect.top()..rect.bottom()).find_map(|y| match buf.cell((x, y)).unwrap().fg {
            Color::Rgb(r, g, b) if g > r && g > b => Some((x, y)),
            _ => None,
        })
    })
}

/// Any lit braille dot anywhere within `rect`.
fn any_lit_dot_in(buf: &ratatui::buffer::Buffer, rect: Rect) -> bool {
    (rect.y..rect.y + rect.height)
        .any(|y| (rect.x..rect.x + rect.width).any(|x| engine_render::decode_braille_cell(buf, x, y).is_some()))
}

/// Seeds a single Ready egg, drives its hatch to completion, and taps
/// Keep (`keep == true`) or Discard, returning the scene positioned right
/// after dismissal.
fn hatch_and_dismiss_only_egg(w: u16, h: u16, keep: bool) -> Hatchery {
    let dir = temp_store_dir(if keep { "empty-dock-keep" } else { "empty-dock-discard" });
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(hatchling_with_nonzero_stats("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    hsq::launch_hatch(&mut scene, w, h);
    advance_to_complete(&mut scene);
    scene.maybe_offer_dock_actions();

    let area = Rect::new(0, 0, w, h);
    let _ = render_to_buffer(&scene, w, h);
    let (keep_rect, discard_rect) =
        scene.dock_action_rects(area).expect("a completed hatch must offer Keep/Discard rects");
    let rect = if keep { keep_rect } else { discard_rect };
    tap_at(&mut scene, rect.x + rect.width / 2, rect.y + rect.height / 2);
    scene
}

/// Once the last egg hatches and Keep dismisses it, the empty-dock view
/// takes over: no eggs, no active hatch/roster action/selection, and the
/// hatchling is retained for the settled read-only view.
#[test]
fn keep_last_egg_enters_empty_dock() {
    let scene = hatch_and_dismiss_only_egg(90, 30, true);
    assert!(scene.eggs.is_empty(), "the last egg must be retired from the tray");
    assert!(scene.hatch.is_none(), "the hatch sub-mode must end once dismissed");
    assert!(scene.roster_action.is_none(), "the post-hatch action must clear on dismissal");
    assert!(scene.selected.is_none(), "no egg remains to select");
    assert!(scene.settled.is_some(), "the just-hatched creature must be retained for the empty-dock view");
}

/// Discard also retains the just-hatched creature for the empty-dock view
/// — the retained view does not depend on which post-hatch action was
/// taken.
#[test]
fn discard_last_egg_also_retains_creature() {
    let scene = hatch_and_dismiss_only_egg(90, 30, false);
    assert!(scene.eggs.is_empty(), "the last egg must be retired from the tray");
    assert!(
        scene.settled.is_some(),
        "Discard must still retain the just-hatched creature for the empty-dock view"
    );
}

/// The empty-dock view renders the retained creature in the settled left
/// slot and its stat bars, at full undimmed opacity, in the settled
/// stat-bar band.
#[test]
fn empty_dock_renders_creature_and_full_opacity_stat_bars() {
    let (w, h) = (90u16, 30u16);
    let scene = hatch_and_dismiss_only_egg(w, h, true);
    assert!(scene.settled.is_some(), "fixture setup must reach the empty-dock view");

    let area = Rect::new(0, 0, w, h);
    let strip = focus::focus_layout(area).1;
    let settled = hatch_layout::settled_layout(area, strip, "Emberling");

    let buf = render_to_buffer(&scene, w, h);
    assert!(
        any_lit_dot_in(&buf, settled.creature.to_cell_rect()),
        "the empty-dock view must draw the retained creature in the settled left slot"
    );

    let band = settled.stat_bars.to_cell_rect();
    let (x, y) = rightmost_green(&buf, band)
        .expect("the empty-dock stat-bar band must show at least one green-dominant fill dot");
    let expected = Color::Rgb(STAT_BAR_COLOR.r, STAT_BAR_COLOR.g, STAT_BAR_COLOR.b);
    assert_eq!(
        buf.cell((x, y)).unwrap().fg,
        expected,
        "the empty-dock view's stat bars must render at full, undimmed opacity"
    );
}

/// The empty-dock view offers no Keep/Discard action and no Submit/Hatch
/// panel action button.
#[test]
fn empty_dock_has_no_dock_action_or_panel_button() {
    let (w, h) = (90u16, 30u16);
    let scene = hatch_and_dismiss_only_egg(w, h, true);
    assert!(scene.settled.is_some(), "fixture setup must reach the empty-dock view");

    let area = Rect::new(0, 0, w, h);
    assert!(
        scene.dock_action_rects(area).is_none(),
        "the empty-dock view must offer no Keep/Discard action"
    );

    let text = crate::scenes::test_util::rect_text(&render_to_buffer(&scene, w, h), area);
    assert!(!text.contains("Submit"), "the empty-dock view must show no Submit action button, got {text:?}");
    assert!(!text.contains("Hatch"), "the empty-dock view must show no Hatch action button, got {text:?}");
}

/// The empty-dock view persists across subsequent frames/updates — it is
/// not a one-frame flash back to an ordinary (empty) browse layout.
#[test]
fn empty_dock_persists_across_frames() {
    let (w, h) = (90u16, 30u16);
    let mut scene = hatch_and_dismiss_only_egg(w, h, true);
    assert!(scene.settled.is_some(), "fixture setup must reach the empty-dock view");

    for _ in 0..3 {
        scene.advance_hatch(Duration::from_millis(50));
        let _ = render_to_buffer(&scene, w, h);
    }

    assert!(
        scene.settled.is_some(),
        "the empty-dock view must persist across subsequent frames, not reset after one render"
    );
}
