//! Hatch-out pre-reveal hand-off tests: the animation must play (egg to
//! center, panel off-right) before `hatch` launches, the dock is suppressed
//! while it plays, and input is gated through it.

use super::*;
use super::hatch_sequence_tests as hsq;

/// A completed tap on a Ready egg must not launch the reveal immediately:
/// shortly after the tap, the hatch-out transition is in progress and the
/// reveal has not started; only once it completes does `hatch` launch.
#[test]
fn ready_hatch_plays_animation_before_reveal() {
    let dir = temp_store_dir("hatch-out-plays-before-reveal");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![hsq::ready_egg_with_hatchling(sample_creature("Emberling"))],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    let _ = render_to_buffer(&scene, w, h);
    let rect = tray::tray_slots(tray::tray_band(area), 1)[0].to_cell_rect();
    tap_at(&mut scene, rect.x, rect.y);
    assert_eq!(scene.pending_hatch, Some(0), "tapping a Ready egg must record a pending hatch request");

    scene.advance_hatch(Duration::from_millis(20));

    assert!(
        scene.hatch_out.is_some() && scene.hatch.is_none(),
        "shortly after the trigger the hatch-out transition must be underway and the reveal must not have launched yet"
    );
}

/// While the hatch-out transition is playing, the stationary egg-dock chips
/// are not drawn (they render normally in an ordinary browse frame).
#[test]
fn dock_not_drawn_during_hatch_out() {
    let dir = temp_store_dir("hatch-out-suppresses-dock");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(SystemTime::now()), incubating_egg(SystemTime::now())],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    let (w, h) = (40u16, 20u16);
    let area = Rect::new(0, 0, w, h);

    let dock_cells = tray::tray_band(area);
    let any_lit_dot = |buf: &ratatui::buffer::Buffer| -> bool {
        (dock_cells.y..dock_cells.y + dock_cells.height).any(|y| {
            (dock_cells.x..dock_cells.x + dock_cells.width)
                .any(|x| engine_render::decode_braille_cell(buf, x, y).is_some())
        })
    };

    let browse_buf = render_to_buffer(&scene, w, h);
    assert!(any_lit_dot(&browse_buf), "an ordinary browse frame must draw the dock's egg chips");

    scene.hatch_out = Some(hatch_render::HatchOut { egg: 0, elapsed: Duration::from_millis(100) });
    let hatch_out_buf = render_to_buffer(&scene, w, h);
    assert!(!any_lit_dot(&hatch_out_buf), "the dock must not be drawn while the hatch-out transition is playing");
}

/// Input is gated while the hatch-out transition plays: a browse-mode
/// navigation key returns no `Transition` and does not change `mode`.
#[test]
fn input_gated_during_hatch_out() {
    let dir = temp_store_dir("hatch-out-gates-input");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(SystemTime::now()), incubating_egg(SystemTime::now())],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    scene.hatch_out = Some(hatch_render::HatchOut { egg: 0, elapsed: Duration::from_millis(100) });
    let mode_before = scene.mode;

    let t = scene.handle_input(key_event(ratatui::crossterm::event::KeyCode::Right));

    assert!(t.is_none(), "a key press during hatch-out must not produce a Transition");
    assert_eq!(
        scene.mode, mode_before,
        "a key press during hatch-out must not change the browse mode"
    );
}

/// Near the end of the hatch-out transition the panel's rect sits at or
/// past the screen's right edge; rendering that frame must not panic.
#[test]
fn hatch_out_off_right_panel_no_panic() {
    let dir = temp_store_dir("hatch-out-off-right-no-panic");
    let seed = PlayerData { roster: Vec::new(), eggs: vec![incubating_egg(SystemTime::now())] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    scene.hatch_out = Some(hatch_render::HatchOut { egg: 0, elapsed: hatch::SLIDE_DURATION });

    let (w, h) = (40u16, 20u16);
    let _ = render_to_buffer(&scene, w, h);
}
