//! Post-hatch stats panel tests.

use super::*;
use ratatui::buffer::Buffer;

use super::hatch_sequence_tests as hsq;

/// A hatchling with 4 distinct stat values, so a text-substring assertion
/// can pin a specific stat rather than passing on any placeholder number.
fn creature_with_stats(name: &str) -> PersistedCreature {
    PersistedCreature::new(
        name,
        Element::Fire,
        Stats { strength: 7, dexterity: 11, intelligence: 13, vitality: 17 },
        1,
        0,
        Vec::new(),
        Stamina::default(),
        None,
        None,
        None,
    )
}

/// Ticks `advance_hatch` until the sequence reaches `target`, or panics if
/// the timeline runs out first (a fixture bug, not a real timeout).
fn advance_until_phase(scene: &mut Hatchery, target: hatch::HatchPhase) {
    for _ in 0..2000 {
        if scene.hatch.as_ref().unwrap().seq.phase() == target {
            return;
        }
        scene.advance_hatch(Duration::from_millis(5));
    }
    panic!("hatch sequence never reached phase {target:?}");
}

/// Whether any dot in `cell_rect` decodes to (near) the roster panel's grey
/// border color (`0x88, 0x88, 0x88`).
fn has_grey_border_dot(buf: &Buffer, cell_rect: Rect) -> bool {
    let is_grey = |c: engine_core::color::Rgba| {
        (c.r as i32 - 0x88).abs() < 16 && (c.g as i32 - 0x88).abs() < 16 && (c.b as i32 - 0x88).abs() < 16
    };
    for y in cell_rect.top()..cell_rect.bottom() {
        for x in cell_rect.left()..cell_rect.right() {
            if let Some((_, color)) = engine_render::decode_braille_cell(buf, x, y) {
                if is_grey(color) {
                    return true;
                }
            }
        }
    }
    false
}

/// Before the Name phase (during the color-lerp reveal), no stats-panel
/// text has appeared yet.
#[test]
fn panel_absent_before_name_reveal() {
    let dir = temp_store_dir("hatch-stats-absent");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(creature_with_stats("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::RevealColor);

    let area = Rect::new(0, 0, w, h);
    let focus_dr = focus::focus_layout(area).0;
    let panel_cells = hatch_stats::stats_panel_rect(area, focus_dr).to_cell_rect();

    let buf = render_to_buffer(&scene, w, h);
    let text = crate::scenes::test_util::rect_text(&buf, panel_cells);
    assert!(!text.contains("STR"), "stats panel must not render before the Name phase, got {text:?}");
}

/// From the Beat phase onward, the panel's grey border chrome and the
/// hatchling's stat values are both visible.
#[test]
fn panel_present_from_name_phase() {
    let dir = temp_store_dir("hatch-stats-present");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(creature_with_stats("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Beat);

    let area = Rect::new(0, 0, w, h);
    let focus_dr = focus::focus_layout(area).0;
    let panel_cells = hatch_stats::stats_panel_rect(area, focus_dr).to_cell_rect();

    let buf = render_to_buffer(&scene, w, h);
    let text = crate::scenes::test_util::rect_text(&buf, panel_cells);
    assert!(text.contains("STR") && text.contains('7'), "expected STR value in panel text, got {text:?}");
    assert!(has_grey_border_dot(&buf, panel_cells), "expected grey border chrome in the panel region");
}

/// The panel does not wait for the attack to finish: it is still present
/// during both the Slide and the Done phase.
#[test]
fn panel_persists_through_idle_and_attack() {
    let dir = temp_store_dir("hatch-stats-persist");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(creature_with_stats("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);

    let area = Rect::new(0, 0, w, h);
    let focus_dr = focus::focus_layout(area).0;
    let panel_cells = hatch_stats::stats_panel_rect(area, focus_dr).to_cell_rect();

    advance_until_phase(&mut scene, hatch::HatchPhase::Slide);
    let idle_text = crate::scenes::test_util::rect_text(&render_to_buffer(&scene, w, h), panel_cells);
    assert!(idle_text.contains("STR"), "expected stats panel during Slide, got {idle_text:?}");

    advance_until_phase(&mut scene, hatch::HatchPhase::Done);
    let done_text = crate::scenes::test_util::rect_text(&render_to_buffer(&scene, w, h), panel_cells);
    assert!(done_text.contains("STR"), "expected stats panel during Done, got {done_text:?}");
}

/// The panel sits in the right gutter, disjoint from the centered reveal
/// rect, and during the Crack phase (before the panel ever appears) its
/// region carries no border chrome.
#[test]
fn panel_does_not_overlap_reveal() {
    let area = Rect::new(0, 0, 70, 24);
    let focus_dr = focus::focus_layout(area).0;
    let panel = hatch_stats::stats_panel_rect(area, focus_dr);
    assert!(
        panel.x >= focus_dr.x + focus_dr.w,
        "panel {panel:?} must sit to the right of the reveal rect {focus_dr:?}, not overlap it"
    );

    let dir = temp_store_dir("hatch-stats-no-overlap");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(creature_with_stats("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (70u16, 24u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Crack);

    let panel_cells = panel.to_cell_rect();
    let buf = render_to_buffer(&scene, w, h);
    assert!(
        !has_grey_border_dot(&buf, panel_cells),
        "the stats panel must carry no chrome during the Crack phase"
    );
}

/// `draw_stats_panel` never panics even when the panel has no room to fit
/// (a narrow terminal).
#[test]
fn degenerate_narrow_terminal_no_panic() {
    let area = Rect::new(0, 0, 5, 5);
    let focus_dr = focus::focus_layout(area).0;
    let mut buf = Buffer::empty(area);
    let creature = creature_with_stats("Tiny");

    hatch_stats::draw_stats_panel(&mut buf, area, focus_dr, &creature);
}
