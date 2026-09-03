//! Hatchery dev-only debug hotkey tests: force-hatch and force-create-egg.

use super::*;

use super::hatch_sequence_tests as hsq;

/// Selects the single egg at index 0, mirroring the browse-mode selection
/// that replaces tap-to-focus.
fn focus_first_egg(scene: &mut Hatchery, _w: u16, _h: u16) {
    scene.select(0);
}

/// With an `Incubating` egg focused whose still/idle/attack are all
/// unresolved, pressing the force-hatch key must not force the egg `Ready`
/// — the egg must stay `Incubating` so the ordinary generation pipeline
/// keeps advancing it.
#[test]
fn force_hatch_key_on_unresolved_egg_does_not_force_ready() {
    let dir = temp_store_dir("force-hatch-gated-state");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(now - Duration::from_secs(3600))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
    let (w, h) = (40u16, 20u16);
    focus_first_egg(&mut scene, w, h);
    assert_eq!(scene.selected, Some(0), "fixture must have the egg selected before force-hatching it");

    scene.handle_input(key_event(hatch_dev::FORCE_HATCH_KEY));

    assert!(
        matches!(scene.eggs[0].state, EggState::Incubating { .. }),
        "force-hatch on an egg whose assets are unresolved must not force it Ready"
    );
}

/// With an `Incubating` egg focused whose still/idle/attack are all
/// unresolved, pressing the force-hatch key records the request but the
/// gate holds: the sequence never launches across many ticks.
#[test]
fn force_hatch_key_on_unresolved_egg_gates_the_sequence() {
    let dir = temp_store_dir("force-hatch-gated-launch");
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
    assert_eq!(scene.pending_hatch, Some(0), "force-hatch must record a hatch request for the focused egg");

    for _ in 0..50 {
        scene.advance_hatch(Duration::from_millis(20));
    }
    assert!(
        scene.hatch.is_none(),
        "the gate must hold the sequence until still+idle+attack all resolve"
    );
}

/// While the focused egg's assets are unresolved, force-hatch shows the
/// generating wait instead of a hatch frame, once the hatch-out transition
/// has had time to run its course.
#[test]
fn force_hatch_on_unresolved_egg_renders_generating_wait() {
    let dir = temp_store_dir("force-hatch-renders-wait");
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
    scene.advance_hatch(hatch::SLIDE_DURATION);

    let area = Rect::new(0, 0, w, h);
    let buf = render_to_buffer(&scene, w, h);
    let text = crate::scenes::test_util::rect_text(&buf, area);
    assert!(text.contains("Generating"), "expected the generating wait text, got {text:?}");
}

/// Once a gated egg's still and idle/attack clips all resolve, the very
/// next `advance_hatch` tick launches the sequence, after the hatch-out
/// transition (already elapsed by this point) has run its course.
#[test]
fn force_hatch_launches_once_generation_completes_after_holding() {
    let dir = temp_store_dir("force-hatch-resolves-then-launches");
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
    scene.advance_hatch(hatch::SLIDE_DURATION);
    assert!(scene.hatch.is_none(), "fixture must start in the gated wait");

    let mut hatchling = sample_creature("Newborn");
    hatchling.idle = Some(hsq::synthetic_clip());
    hatchling.attack = Some(hsq::synthetic_clip());
    scene.eggs[0].hatchling = Some(hatchling);
    scene.eggs[0].egg_art = Some(hsq::synthetic_still());
    scene.art_cache[0] = image::open(&scene.eggs[0].egg_art.as_ref().unwrap().path).ok();

    scene.advance_hatch(Duration::from_millis(0));
    assert!(
        scene.hatch.is_some(),
        "advance_hatch must launch once still+idle+attack are all resolved"
    );
}

/// Pressing the force-hatch key with no egg selected is a no-op: no egg
/// state changes and no hatch request is recorded.
#[test]
fn force_hatch_with_no_egg_selected_is_a_no_op() {
    let dir = temp_store_dir("force-hatch-no-selection");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(now - Duration::from_secs(3600))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
    assert!(scene.selected.is_none(), "fixture must start with nothing selected");

    scene.handle_input(key_event(hatch_dev::FORCE_HATCH_KEY));

    assert!(
        matches!(scene.eggs[0].state, EggState::Incubating { .. }),
        "with no egg selected, force-hatch must not change any egg's state"
    );
    assert!(scene.pending_hatch.is_none(), "with no egg selected, force-hatch must not record a hatch request");
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
