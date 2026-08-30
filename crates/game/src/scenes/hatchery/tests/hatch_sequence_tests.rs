//! Hatch sequence render + scene wiring tests.

use super::*;

/// A `Ready` egg carrying a hatchling, for driving the hatch sequence.
fn ready_egg_with_hatchling(hatchling: PersistedCreature) -> Egg {
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
fn launch_hatch(scene: &mut Hatchery, w: u16, h: u16) {
    let area = Rect::new(0, 0, w, h);
    let _ = render_to_buffer(scene, w, h);
    let rect = tray::tray_slots(tray::tray_band(area), scene.eggs.len())[0].to_cell_rect();
    tap_at(scene, rect.x, rect.y);
    scene.advance_hatch(Duration::from_millis(0));
}

/// Whether `phase` is at or beyond the Name phase (i.e. the color-lerp
/// reveal has completed).
fn phase_at_least_name(phase: hatch::HatchPhase) -> bool {
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
/// completes, and is drawn from the Name phase onward.
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
        if phase_at_least_name(scene.hatch.as_ref().unwrap().seq.phase()) {
            break;
        }
        let before = render_to_buffer(&scene, w, h);
        let before_text = crate::scenes::test_util::rect_text(&before, area);
        assert!(
            !before_text.contains("Emberling"),
            "the hatchling's name must not appear before the Name phase, got {before_text:?}"
        );
        scene.advance_hatch(Duration::from_millis(20));
    }

    let after = render_to_buffer(&scene, w, h);
    let after_text = crate::scenes::test_util::rect_text(&after, area);
    assert!(
        after_text.contains("Emberling"),
        "the hatchling's name must appear at/after the Name phase, got {after_text:?}"
    );
    assert!(
        phase_at_least_name(scene.hatch.as_ref().unwrap().seq.phase()),
        "fixture must have actually reached the Name phase or later"
    );
}

/// While the sequence is active, a completed back-button click produces
/// no `Transition`, and a tap on the focused egg changes neither focus
/// nor the define modal.
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
    assert!(scene.focused.is_none(), "an egg tap mid-hatch-sequence must not change focus");
    assert!(scene.define_modal.is_none(), "an egg tap mid-hatch-sequence must not open the define modal");
}

/// With no idle/attack clips resolved (the no-GPU/dev-force-hatch case),
/// the sequence still reaches completion within the sampled window,
/// with no panic along the way.
#[test]
fn no_gpu_fallback_completes_without_panic() {
    let dir = temp_store_dir("hatch-no-gpu-fallback");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    launch_hatch(&mut scene, w, h);

    for _ in 0..300 {
        scene.advance_hatch(Duration::from_millis(20));
        let _ = render_to_buffer(&scene, w, h);
        if scene.hatch.as_ref().unwrap().seq.is_complete() {
            break;
        }
    }

    assert!(
        scene.hatch.as_ref().unwrap().seq.is_complete(),
        "the sequence must reach completion within the sampled window without panicking"
    );
}
