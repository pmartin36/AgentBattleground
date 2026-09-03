//! Settled-placement (Done phase) render tests: the name zone never
//! overlaps the creature or the dock, and the dock body renders through
//! exactly the same shared component the roster detail screen uses.

use ratatui::buffer::Buffer;
use ratatui::style::Color;

use crate::ability::Ability;
use crate::scenes::detail_panel;
use crate::scenes::stat_bar::STAT_BAR_COLOR;
use crate::scenes::test_util::region_cells;
use crate::stamina::Stamina;
use crate::stats::Stats;

use super::*;
use super::hatch_sequence_tests as hsq;

/// Rightmost cell within `rect` whose fg is green-dominant — the shared
/// stat-bar renderer's fill blends green-dominant, its grey border chrome
/// does not.
fn rightmost_green(buf: &Buffer, rect: Rect) -> Option<(u16, u16)> {
    (rect.left()..rect.right()).rev().find_map(|x| {
        (rect.top()..rect.bottom()).find_map(|y| match buf.cell((x, y)).unwrap().fg {
            Color::Rgb(r, g, b) if g > r && g > b => Some((x, y)),
            _ => None,
        })
    })
}

/// Sum of a green-dominant cell's RGB channels within `rect`, or 0 when no
/// such cell is found — a coarse proxy for the fill's rendered opacity.
fn green_channel_sum(buf: &Buffer, rect: Rect) -> u32 {
    rightmost_green(buf, rect)
        .map(|(x, y)| match buf.cell((x, y)).unwrap().fg {
            Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
            _ => 0,
        })
        .unwrap_or(0)
}

/// A hatchling with a distinct stamina + one ability, so the never-drift
/// comparison exercises real (non-default) data.
fn creature_with_stamina_and_ability(name: &str) -> PersistedCreature {
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

/// At the settled (Done) state, the name zone sits entirely above the
/// creature zone and does not reach into the dock's left edge — the
/// anti-overlap invariant `settled_layout` must uphold regardless of the
/// hatchling's name length.
#[test]
fn settled_name_above_creature_no_overlap() {
    let dir = temp_store_dir("hatch-settled-no-overlap");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(creature_with_stamina_and_ability("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (80u16, 30u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Done);

    let area = Rect::new(0, 0, w, h);
    let strip = focus::focus_layout(area).1;
    let settled = hatch_layout::settled_layout(area, strip, "Emberling");

    assert!(
        settled.name_zone.y + settled.name_zone.h <= settled.creature.y,
        "name zone {:?} must sit above the creature zone {:?}",
        settled.name_zone,
        settled.creature
    );
    assert!(
        settled.name_zone.x + settled.name_zone.w <= settled.dock_border.x,
        "name zone {:?} must not reach into the dock {:?}",
        settled.name_zone,
        settled.dock_border
    );

    let buf = render_to_buffer(&scene, w, h);
    let name_text = crate::scenes::test_util::rect_text(&buf, settled.name_zone.to_cell_rect());
    assert!(name_text.contains("Emberling"), "expected the name inside its own zone, got {name_text:?}");
}

/// The settled dock body (stamina row + abilities) renders through the
/// exact shared component the roster detail screen uses: rendering the same
/// stamina/abilities via `detail_panel`'s free functions at the same
/// regions decodes byte-for-byte identical to what the hatch scene draws.
#[test]
fn dock_matches_shared_component_never_drift() {
    let dir = temp_store_dir("hatch-settled-never-drift");
    let hatchling = creature_with_stamina_and_ability("Emberling");
    let seed =
        PlayerData { roster: Vec::new(), eggs: vec![hsq::ready_egg_with_hatchling(hatchling.clone())] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (80u16, 30u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Done);

    let area = Rect::new(0, 0, w, h);
    let strip = focus::focus_layout(area).1;
    let settled = hatch_layout::settled_layout(area, strip, &hatchling.name);
    let regions = detail_panel::interior_regions(settled.dock_border);

    let scene_buf = render_to_buffer(&scene, w, h);

    let mut shared_buf = Buffer::empty(Rect::new(0, 0, w, h));
    detail_panel::render_stamina_row(&mut shared_buf, regions.stamina, &hatchling.stamina);
    detail_panel::render_abilities(
        &mut shared_buf,
        regions.abilities_header,
        regions.ability_cells,
        &hatchling.abilities,
    );

    let stamina_rect = regions.stamina.to_cell_rect();
    assert_eq!(
        region_cells(&scene_buf, stamina_rect),
        region_cells(&shared_buf, stamina_rect),
        "the settled dock's stamina row must decode byte-identical to the shared component's own render"
    );
    for (i, cell) in regions.ability_cells.iter().enumerate() {
        let rect = cell.to_cell_rect();
        assert_eq!(
            region_cells(&scene_buf, rect),
            region_cells(&shared_buf, rect),
            "the settled dock's ability cell {i} must decode byte-identical to the shared component's own render"
        );
    }
}

/// At the settled (Done) state, the hatchling's stat bars render in the
/// settled stat-bar band at full, undimmed opacity — the exact
/// `STAT_BAR_COLOR`, not a blended/dimmed shade.
#[test]
fn stat_bars_full_opacity_at_done() {
    let dir = temp_store_dir("hatch-settled-stat-bars-done");
    let hatchling = creature_with_stamina_and_ability("Emberling");
    let seed =
        PlayerData { roster: Vec::new(), eggs: vec![hsq::ready_egg_with_hatchling(hatchling.clone())] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (80u16, 30u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Done);

    let area = Rect::new(0, 0, w, h);
    let strip = focus::focus_layout(area).1;
    let settled = hatch_layout::settled_layout(area, strip, &hatchling.name);
    let band = settled.stat_bars.to_cell_rect();

    let buf = render_to_buffer(&scene, w, h);
    let (x, y) = rightmost_green(&buf, band)
        .expect("the settled stat-bar band must show at least one green-dominant fill dot at Done");
    let expected = Color::Rgb(STAT_BAR_COLOR.r, STAT_BAR_COLOR.g, STAT_BAR_COLOR.b);
    assert_eq!(
        buf.cell((x, y)).unwrap().fg,
        expected,
        "at Done the stat bars must render at full, undimmed opacity"
    );
}

/// Across the Slide phase, the settled stat-bar band's fill opacity rises:
/// a sample late in Slide reads a stronger green-channel intensity than a
/// sample taken early in Slide.
#[test]
fn stat_bars_fade_in_across_slide() {
    let dir = temp_store_dir("hatch-settled-stat-bars-fade");
    let hatchling = creature_with_stamina_and_ability("Emberling");
    let seed =
        PlayerData { roster: Vec::new(), eggs: vec![hsq::ready_egg_with_hatchling(hatchling.clone())] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (80u16, 30u16);
    hsq::launch_hatch(&mut scene, w, h);
    advance_until_phase(&mut scene, hatch::HatchPhase::Slide);

    let area = Rect::new(0, 0, w, h);
    let strip = focus::focus_layout(area).1;
    let settled = hatch_layout::settled_layout(area, strip, &hatchling.name);
    let band = settled.stat_bars.to_cell_rect();

    let early_buf = render_to_buffer(&scene, w, h);
    let early_sum = green_channel_sum(&early_buf, band);

    scene.advance_hatch(Duration::from_millis(450));
    assert_eq!(
        scene.hatch.as_ref().unwrap().seq.phase(),
        hatch::HatchPhase::Slide,
        "fixture must still be inside Slide, not yet Done, for this to test the fade-in ramp"
    );
    let late_buf = render_to_buffer(&scene, w, h);
    let late_sum = green_channel_sum(&late_buf, band);

    assert!(
        late_sum > early_sum,
        "the stat-bar band's green-channel intensity must rise across Slide: early {early_sum}, late {late_sum}"
    );
}
