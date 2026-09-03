//! Hatch sequence render + scene wiring tests.

use std::sync::atomic::{AtomicU32, Ordering};

use image::{Rgba, RgbaImage};

use crate::asset_gen::types::{ClipAsset, ImageAsset};

use super::*;

static ASSET_TAG: AtomicU32 = AtomicU32::new(0);

/// Writes a synthetic opaque PNG to a unique temp path, standing in for an
/// already-resolved still or clip frame.
fn write_synthetic_png(tag: &str) -> std::path::PathBuf {
    let n = ASSET_TAG.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "game-hatchery-sequence-asset-{}-{}-{}.png",
        std::process::id(),
        tag,
        n
    ));
    let mut img = RgbaImage::from_pixel(4, 4, Rgba([200, 60, 40, 255]));
    img.put_pixel(2, 2, Rgba([0, 0, 255, 255]));
    img.save(&path).unwrap();
    path
}

/// A resolved `ImageAsset` pointing at a fresh synthetic still PNG.
pub(super) fn synthetic_still() -> ImageAsset {
    ImageAsset { path: write_synthetic_png("still") }
}

/// A resolved single-frame `ClipAsset` pointing at a fresh synthetic PNG.
pub(super) fn synthetic_clip() -> ClipAsset {
    ClipAsset { frames: vec![write_synthetic_png("clip")] }
}

/// A `Ready` egg carrying a hatchling with its still and idle/attack clips
/// all resolved, for driving the hatch sequence past the full-generation
/// gate.
pub(super) fn ready_egg_with_hatchling(mut hatchling: PersistedCreature) -> Egg {
    hatchling.idle = Some(synthetic_clip());
    hatchling.attack = Some(synthetic_clip());
    Egg {
        element: Element::Fire,
        state: EggState::Ready,
        mad_lib: None,
        egg_art: Some(synthetic_still()),
        hatchling: Some(hatchling),
    }
}

/// A `Ready` egg carrying a hatchling with no still or clips resolved at
/// all, standing in for a no-GPU environment where generation never
/// completes.
fn ready_egg_with_no_generated_assets(hatchling: PersistedCreature) -> Egg {
    Egg {
        element: Element::Fire,
        state: EggState::Ready,
        mad_lib: None,
        egg_art: None,
        hatchling: Some(hatchling),
    }
}

/// Taps the single Ready egg at index 0 (recording a pending hatch
/// request), then ticks `advance_hatch` once to launch the sequence.
pub(super) fn launch_hatch(scene: &mut Hatchery, w: u16, h: u16) {
    let area = Rect::new(0, 0, w, h);
    let _ = render_to_buffer(scene, w, h);
    let rect = tray::tray_slots(tray::tray_band(area), scene.eggs.len())[0].to_cell_rect();
    tap_at(scene, rect.x, rect.y);
    scene.advance_hatch(Duration::from_millis(0));
}

/// Whether `phase` is at or beyond the Beat phase (i.e. the color-lerp
/// reveal has completed).
fn phase_at_least_beat(phase: hatch::HatchPhase) -> bool {
    !matches!(
        phase,
        hatch::HatchPhase::Wiggle
            | hatch::HatchPhase::Crack
            | hatch::HatchPhase::Break
            | hatch::HatchPhase::RevealFlash
            | hatch::HatchPhase::RevealColor
    )
}

/// Tapping a `Ready` egg then advancing consumes `pending_hatch` and
/// launches a `HatchState` — the sequence starts inside `advance_hatch`
/// (the method `update()` calls), not synchronously inside the tap.
#[test]
fn launch_consumes_pending_hatch_in_update() {
    let dir = temp_store_dir("hatch-launch");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    let _ = render_to_buffer(&scene, w, h);
    let rect = tray::tray_slots(tray::tray_band(area), 1)[0].to_cell_rect();
    tap_at(&mut scene, rect.x, rect.y);
    assert_eq!(scene.pending_hatch, Some(0), "tapping a Ready egg must record a pending hatch request");

    scene.advance_hatch(Duration::from_millis(5));

    assert!(scene.pending_hatch.is_none(), "advance_hatch must consume pending_hatch");
    assert!(scene.hatch.is_some(), "advance_hatch must launch a HatchState for the tapped egg");
}

/// During the Crack phase, the black crack overlay is composited on top
/// of the egg: somewhere over the focused egg's rect a near-black lit
/// dot appears (neither the teal background nor the bright yellow-gold
/// egg placeholder produce one).
#[test]
fn crack_overlay_composites_over_egg() {
    let dir = temp_store_dir("hatch-crack-overlay");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    launch_hatch(&mut scene, w, h);

    while scene.hatch.as_ref().unwrap().seq.phase() != hatch::HatchPhase::Crack {
        scene.advance_hatch(Duration::from_millis(20));
    }

    let area = Rect::new(0, 0, w, h);
    let buf = render_to_buffer(&scene, w, h);
    let focus_cells = focus::focus_layout(area).0.to_cell_rect();

    let is_near_black = |r: u8, g: u8, b: u8| r < 40 && g < 40 && b < 40;
    let mut found = false;
    'scan: for y in focus_cells.y..focus_cells.y + focus_cells.height {
        for x in focus_cells.x..focus_cells.x + focus_cells.width {
            if let Some((_, color)) = engine_render::decode_braille_cell(&buf, x, y) {
                if is_near_black(color.r, color.g, color.b) {
                    found = true;
                    break 'scan;
                }
            }
        }
    }
    assert!(found, "expected a near-black crack-overlay dot composited over the egg during the Crack phase");
}

/// The crack phase's frame index advances in stutter-step bursts, not a
/// uniform sweep: within the Crack phase, some consecutive small
/// `advance_hatch` steps leave the decoded focus-rect frame unchanged
/// (a hold), and some change it (a burst boundary).
#[test]
fn crack_bursts_change_dots_holds_do_not() {
    let dir = temp_store_dir("hatch-crack-cadence");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    launch_hatch(&mut scene, w, h);

    while scene.hatch.as_ref().unwrap().seq.phase() != hatch::HatchPhase::Crack {
        scene.advance_hatch(Duration::from_millis(20));
    }

    let area = Rect::new(0, 0, w, h);
    let focus_cells = focus::focus_layout(area).0.to_cell_rect();

    let mut prev = crate::scenes::test_util::region_cells(&render_to_buffer(&scene, w, h), focus_cells);
    let mut saw_hold = false;
    let mut saw_burst = false;
    while scene.hatch.as_ref().unwrap().seq.phase() == hatch::HatchPhase::Crack {
        scene.advance_hatch(Duration::from_millis(5));
        let current = crate::scenes::test_util::region_cells(&render_to_buffer(&scene, w, h), focus_cells);
        if current == prev {
            saw_hold = true;
        } else {
            saw_burst = true;
        }
        prev = current;
        if saw_hold && saw_burst {
            break;
        }
    }

    assert!(saw_hold, "expected at least one hold window with an unchanged decoded crack frame");
    assert!(saw_burst, "expected at least one burst boundary with a changed decoded crack frame");
}

/// The hatchling's name is never drawn before the color-lerp reveal
/// completes, and is drawn from the Beat phase onward.
#[test]
fn name_absent_before_color_lerp_then_present() {
    let dir = temp_store_dir("hatch-name-reveal");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    launch_hatch(&mut scene, w, h);

    loop {
        if phase_at_least_beat(scene.hatch.as_ref().unwrap().seq.phase()) {
            break;
        }
        let before = render_to_buffer(&scene, w, h);
        let before_text = crate::scenes::test_util::rect_text(&before, area);
        assert!(
            !before_text.contains("Emberling"),
            "the hatchling's name must not appear before the Beat phase, got {before_text:?}"
        );
        scene.advance_hatch(Duration::from_millis(20));
    }

    let after = render_to_buffer(&scene, w, h);
    let after_text = crate::scenes::test_util::rect_text(&after, area);
    assert!(
        after_text.contains("Emberling"),
        "the hatchling's name must appear at/after the Beat phase, got {after_text:?}"
    );
    assert!(
        phase_at_least_beat(scene.hatch.as_ref().unwrap().seq.phase()),
        "fixture must have actually reached the Beat phase or later"
    );
}

/// The hatchling's name fades in during the Beat phase: sampled shortly
/// after Beat begins, its foreground brightness is lower than once Beat
/// has completed (Slide onward), where it is at full brightness.
#[test]
fn name_dimmer_at_beat_start_than_after_beat_completes() {
    let dir = temp_store_dir("hatch-name-fade");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    launch_hatch(&mut scene, w, h);

    while scene.hatch.as_ref().unwrap().seq.phase() != hatch::HatchPhase::Beat {
        scene.advance_hatch(Duration::from_millis(20));
    }
    let focus_dr = focus::focus_layout(area).0;
    let name_rect = hatch_render::name_rect(focus_dr);

    // `name_rect` is wider than the name text itself (room for a wrap), so a
    // plain first-non-space scan can land on background fill instead of a
    // name glyph; find the fg of the first actual letter cell instead.
    fn first_letter_fg(buf: &ratatui::buffer::Buffer, rect: Rect) -> Option<ratatui::style::Color> {
        (rect.top()..rect.bottom())
            .flat_map(|y| (rect.left()..rect.right()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let cell = buf.cell((x, y))?;
                cell.symbol().chars().next()?.is_alphabetic().then_some(cell.fg)
            })
    }

    let early_buf = render_to_buffer(&scene, w, h);
    let early_fg = first_letter_fg(&early_buf, name_rect)
        .expect("the name must paint a letter cell early in the Beat phase");

    while scene.hatch.as_ref().unwrap().seq.phase() == hatch::HatchPhase::Beat {
        scene.advance_hatch(Duration::from_millis(5));
    }
    let late_buf = render_to_buffer(&scene, w, h);
    let late_fg = first_letter_fg(&late_buf, name_rect)
        .expect("the name must still paint a letter cell once Beat has completed");

    fn luminance(c: ratatui::style::Color) -> u32 {
        match c {
            ratatui::style::Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
            other => panic!("expected an Rgb name color, got {other:?}"),
        }
    }
    assert!(
        luminance(early_fg) < luminance(late_fg),
        "expected the name dimmer near the start of Beat ({early_fg:?}) than once Beat completes ({late_fg:?})"
    );
}

/// While the sequence is active, a completed back-button click produces
/// no `Transition`, and a tap on the selected egg changes neither the
/// selection nor the edit mode.
#[test]
fn input_swallowed_mid_sequence() {
    let dir = temp_store_dir("hatch-input-swallowed");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    launch_hatch(&mut scene, w, h);
    scene.advance_hatch(Duration::from_millis(50));

    let back_rect = crate::scenes::home_button::home_dot_rect(area).to_cell_rect();
    scene.handle_input(mouse_event(MouseEventKind::Moved, back_rect.x, back_rect.y));
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), back_rect.x, back_rect.y));
    let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), back_rect.x, back_rect.y));
    assert!(t.is_none(), "a back-button click mid-hatch-sequence must not produce a Transition");

    let egg_rect = focus::focus_layout(area).0.to_cell_rect();
    tap_at(&mut scene, egg_rect.x, egg_rect.y);
    assert!(scene.selected.is_none(), "an egg tap mid-hatch-sequence must not change the selection");
    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { .. }),
        "an egg tap mid-hatch-sequence must not enter edit mode, got {:?}",
        scene.mode
    );
}

/// With no still or idle/attack clips ever resolving (a no-GPU
/// environment), the gate holds indefinitely: the sequence never launches
/// across the sampled window, and no half-generated creature is ever
/// revealed. No panic along the way.
#[test]
fn no_gpu_egg_holds_in_generating_wait_without_panic() {
    let dir = temp_store_dir("hatch-no-gpu-fallback");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![ready_egg_with_no_generated_assets(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    let _ = render_to_buffer(&scene, w, h);
    let rect = tray::tray_slots(tray::tray_band(area), scene.eggs.len())[0].to_cell_rect();
    tap_at(&mut scene, rect.x, rect.y);

    for _ in 0..300 {
        scene.advance_hatch(Duration::from_millis(20));
        let _ = render_to_buffer(&scene, w, h);
    }

    assert!(
        scene.hatch.is_none(),
        "with no generated assets ever resolving, the gate must hold indefinitely rather than reveal a half-generated creature"
    );
}
