//! Hatchery dev-only debug hotkey tests: force-hatch and force-create-egg.

use super::*;

/// A completed tap on the single egg at tray slot 0, focusing it.
fn focus_first_egg(scene: &mut Hatchery, w: u16, h: u16) {
    let area = Rect::new(0, 0, w, h);
    let _ = render_to_buffer(scene, w, h);
    let rect = tray::tray_slots(tray::tray_band(area), scene.eggs.len())[0].to_cell_rect();
    tap_at(scene, rect.x, rect.y);
}

/// With an `Incubating` egg focused, pressing the force-hatch key sets that
/// egg `Ready` and records a hatch request; the next `advance_hatch` tick
/// launches an active `HatchSequence` for it.
#[test]
fn force_hatch_key_sets_focused_egg_ready_and_launches() {
    let dir = temp_store_dir("force-hatch-launch");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(now - Duration::from_secs(3600))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
    let (w, h) = (40u16, 20u16);
    focus_first_egg(&mut scene, w, h);
    assert_eq!(scene.focused, Some(0), "fixture must have the egg focused before force-hatching it");

    scene.handle_input(key_event(hatch_dev::FORCE_HATCH_KEY));

    assert_eq!(scene.eggs[0].state, EggState::Ready, "force-hatch must set the focused egg Ready");
    assert_eq!(scene.pending_hatch, Some(0), "force-hatch must record a hatch request for the focused egg");

    scene.advance_hatch(Duration::from_millis(0));
    assert!(scene.hatch.is_some(), "the next advance_hatch tick must launch a HatchState");
    assert!(
        scene.hatch.as_ref().unwrap().seq.is_active(),
        "the launched sequence must be active"
    );
}

/// After force-hatch launches the sequence, a rendered frame draws
/// something over the focus rect (no panic, a hatch frame is on screen).
#[test]
fn force_hatch_renders_a_sequence_frame() {
    let dir = temp_store_dir("force-hatch-renders");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(now - Duration::from_secs(3600))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
    let (w, h) = (40u16, 20u16);
    focus_first_egg(&mut scene, w, h);
    scene.handle_input(key_event(hatch_dev::FORCE_HATCH_KEY));
    scene.advance_hatch(Duration::from_millis(0));

    let area = Rect::new(0, 0, w, h);
    let buf = render_to_buffer(&scene, w, h);
    let focus_cells = focus::focus_layout(area).0.to_cell_rect();
    assert!(
        crate::scenes::test_util::has_non_space(&buf, focus_cells),
        "a hatch frame must be drawn over the focus rect after force-hatch"
    );
}

/// Pressing the force-hatch key with no egg focused is a no-op: no egg
/// state changes and no hatch request is recorded.
#[test]
fn force_hatch_with_no_egg_focused_is_a_no_op() {
    let dir = temp_store_dir("force-hatch-no-focus");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(now - Duration::from_secs(3600))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
    assert!(scene.focused.is_none(), "fixture must start with nothing focused");

    scene.handle_input(key_event(hatch_dev::FORCE_HATCH_KEY));

    assert!(
        matches!(scene.eggs[0].state, EggState::Incubating { .. }),
        "with no egg focused, force-hatch must not change any egg's state"
    );
    assert!(scene.pending_hatch.is_none(), "with no egg focused, force-hatch must not record a hatch request");
}

/// Pressing the force-create-egg key appends one `Undefined` egg to the
/// tray, and the addition survives a persist->reload round-trip without
/// disturbing the roster.
#[test]
fn force_create_egg_key_appends_undefined_egg_persisted() {
    let dir = temp_store_dir("force-create-egg");
    let seed = PlayerData {
        roster: vec![sample_creature("Emberling")],
        eggs: vec![undefined_egg()],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    assert_eq!(scene.eggs.len(), 1, "fixture must start with exactly one egg");

    scene.handle_input(key_event(hatch_dev::FORCE_CREATE_EGG_KEY));

    assert_eq!(scene.eggs.len(), 2, "force-create-egg must append exactly one egg");
    assert_eq!(scene.eggs[1].state, EggState::Undefined, "the appended egg must be Undefined");

    let reloaded = PlayerStore::with_dir(&dir)
        .load(|| panic!("must not fall back to seed"))
        .into_data();
    assert_eq!(reloaded.eggs.len(), 2, "the appended egg must be persisted");
    assert_eq!(reloaded.eggs[1].state, EggState::Undefined);
    assert_eq!(reloaded.roster.len(), 1, "force-create-egg must not touch the roster");
    assert_eq!(reloaded.roster[0].name, "Emberling");
}

/// After force-create-egg, the per-egg `art_cache`/`egg_buttons` vectors
/// stay index-aligned with `eggs` and a render over the grown tray does not
/// panic.
#[test]
fn force_create_egg_keeps_button_and_art_vectors_aligned() {
    let dir = temp_store_dir("force-create-egg-aligned");
    let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    scene.handle_input(key_event(hatch_dev::FORCE_CREATE_EGG_KEY));

    assert_eq!(scene.art_cache.len(), scene.eggs.len(), "art_cache must stay index-aligned with eggs");
    assert_eq!(
        scene.egg_buttons.borrow().len(),
        scene.eggs.len(),
        "egg_buttons must stay index-aligned with eggs"
    );

    let (w, h) = (40u16, 20u16);
    let _ = render_to_buffer(&scene, w, h);
}

/// A key with no debug-hotkey binding is not consumed: no egg state
/// changes and no hatch request is recorded.
#[test]
fn unrecognized_debug_key_is_not_consumed() {
    let dir = temp_store_dir("unrecognized-debug-key");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(now - Duration::from_secs(3600))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);

    scene.handle_input(key_event(ratatui::crossterm::event::KeyCode::Char('z')));

    assert!(
        matches!(scene.eggs[0].state, EggState::Incubating { .. }),
        "an unrelated key must not change any egg's state"
    );
    assert!(scene.pending_hatch.is_none(), "an unrelated key must not record a hatch request");
    assert_eq!(scene.eggs.len(), 1, "an unrelated key must not append an egg");
}
