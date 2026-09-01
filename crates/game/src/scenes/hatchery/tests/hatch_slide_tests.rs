//! Slide-phase choreography: the creature slides left, the stats dock
//! slides in from the right, and the egg-dock strip does not move.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::scenes::detail_panel;
use crate::scenes::test_util::{rect_text, render_to_buffer};

use super::*;
use super::hatch_sequence_tests as hsq;

/// The scene's background fill color (`Hatchery::COLOR`) — every cell not
/// drawn over by a sprite is painted with a fully-lit braille glyph in this
/// color, so column scans must exclude it to find the creature's own dots.
const BG_TEAL: (u8, u8, u8) = (0x1a, 0x66, 0x66);

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

/// Whether cell `(x, y)` is a painted braille glyph in a color other than
/// the background fill's teal — i.e. a sprite's own lit dot, not the empty
/// background or a text glyph (text is exempt from the braille pipeline).
fn is_sprite_lit(buf: &Buffer, x: u16, y: u16) -> bool {
    let Some(cell) = buf.cell((x, y)) else { return false };
    let Some(ch) = cell.symbol().chars().next() else { return false };
    let cp = ch as u32;
    if !(0x2800..=0x28FF).contains(&cp) || cp == 0x2800 {
        return false;
    }
    match cell.fg {
        Color::Rgb(r, g, b) => (r, g, b) != BG_TEAL,
        _ => true,
    }
}

/// The leftmost cell column, over `rows`, containing a sprite's own lit
/// braille dot (excluding the background fill) — the left edge of the
/// rendered sprite.
fn leftmost_lit_column(buf: &Buffer, w: u16, rows: std::ops::Range<u16>) -> Option<u16> {
    (0..w).find(|&x| rows.clone().any(|y| is_sprite_lit(buf, x, y)))
}

/// Across the Slide phase, the creature's leftmost lit column moves left
/// (never right, never stays put): its rendered sprite tracks the eased
/// slide from the centered Beat pose toward the settled left column.
#[test]
fn creature_slides_left_across_slide_phase_frames() {
    let dir = temp_store_dir("hatch-slide-creature-left");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Slide);

    let strip = focus::focus_layout(area).1;
    let rows = 0..strip.y;

    let early_buf = render_to_buffer(&scene, w, h);
    let early_col = leftmost_lit_column(&early_buf, w, rows.clone())
        .expect("the creature must render a lit sprite in the content band early in Slide");

    scene.advance_hatch(Duration::from_millis(400));
    assert_eq!(
        scene.hatch.as_ref().unwrap().seq.phase(),
        hatch::HatchPhase::Slide,
        "fixture must still be inside Slide for this comparison to be meaningful"
    );
    let late_buf = render_to_buffer(&scene, w, h);
    let late_col = leftmost_lit_column(&late_buf, w, rows)
        .expect("the creature must still render a lit sprite in the content band late in Slide");

    assert!(
        late_col < early_col,
        "the creature's leftmost lit column must move left across the Slide phase: early {early_col}, late {late_col}"
    );
}

/// The stats dock (border + shared stamina/abilities body) becomes visible
/// before the Slide phase completes, having slid in from the right edge.
#[test]
fn stats_dock_becomes_visible_before_slide_completes() {
    let dir = temp_store_dir("hatch-slide-dock-visible");
    let hatchling = sample_creature("Emberling");
    let seed = PlayerData { roster: Vec::new(), eggs: vec![hsq::ready_egg_with_hatchling(hatchling)] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Slide);
    scene.advance_hatch(Duration::from_millis(450));
    assert_eq!(
        scene.hatch.as_ref().unwrap().seq.phase(),
        hatch::HatchPhase::Slide,
        "fixture must still be inside Slide, not yet Done, for this to test the sliding-in dock"
    );

    let strip = focus::focus_layout(area).1;
    let settled = hatch_layout::settled_layout(area, strip, "Emberling");
    let regions = detail_panel::interior_regions(settled.dock_border);

    let buf = render_to_buffer(&scene, w, h);
    let stamina_text = rect_text(&buf, regions.stamina.to_cell_rect());
    assert!(
        stamina_text.contains("Stamina"),
        "expected the stats dock's stamina row visible before Slide completes, got {stamina_text:?}"
    );
}

/// The egg-dock strip's row is unaffected by the Slide-phase motion: its
/// decoded cells are identical between two distinct Slide frames even
/// though the creature and stats dock are moving above it.
#[test]
fn egg_dock_strip_row_unchanged_during_slide() {
    let dir = temp_store_dir("hatch-slide-strip-stationary");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![
            hsq::ready_egg_with_hatchling(sample_creature("Emberling")),
            incubating_egg(now - Duration::from_secs(3600)),
        ],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Slide);

    let strip = focus::focus_layout(area).1;

    let early_buf = render_to_buffer(&scene, w, h);
    let early_strip = crate::scenes::test_util::region_cells(&early_buf, strip);

    scene.advance_hatch(Duration::from_millis(300));
    let late_buf = render_to_buffer(&scene, w, h);
    let late_strip = crate::scenes::test_util::region_cells(&late_buf, strip);

    assert_eq!(
        early_strip, late_strip,
        "the egg-dock strip must not change while the creature and stats dock slide above it"
    );
}
