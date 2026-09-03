//! Browse-mode navigation: a tray hover target moves independently of the
//! master selection, and opening the hatchery auto-selects an egg.

use super::*;
use crossterm::event::KeyCode;
use engine_core::scene::EngineCtx;

/// Three `Incubating` eggs, unselected, for hover/selection navigation.
fn three_egg_scene() -> Hatchery {
    let dir = temp_store_dir("browse-three-eggs");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg(now), incubating_egg(now), incubating_egg(now)],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    Hatchery::from_store_at(PlayerStore::with_dir(&dir), now)
}

/// A single `Incubating` egg, unselected.
fn one_egg_scene() -> Hatchery {
    let dir = temp_store_dir("browse-one-egg");
    let now = SystemTime::now();
    let seed = PlayerData { roster: Vec::new(), eggs: vec![incubating_egg(now)] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    Hatchery::from_store_at(PlayerStore::with_dir(&dir), now)
}

/// A single `Undefined` egg, unselected — the fresh-player starter-egg
/// shape.
fn undefined_egg_scene() -> Hatchery {
    let dir = temp_store_dir("browse-undefined-egg");
    let now = SystemTime::now();
    let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    Hatchery::from_store_at(PlayerStore::with_dir(&dir), now)
}

/// A single `Ready` egg, unselected.
fn ready_egg_scene() -> Hatchery {
    let dir = temp_store_dir("browse-ready-egg");
    let now = SystemTime::now();
    let egg = Egg {
        element: crate::ability::Element::Fire,
        state: EggState::Ready,
        mad_lib: None,
        egg_art: None,
        hatchling: None,
    };
    let seed = PlayerData { roster: Vec::new(), eggs: vec![egg] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    Hatchery::from_store_at(PlayerStore::with_dir(&dir), now)
}

/// Opening the hatchery with no prior selection auto-selects the first egg.
#[test]
fn enter_auto_selects_first_egg_when_none_selected() {
    let mut scene = three_egg_scene();
    assert!(scene.selected.is_none(), "fixture must start with nothing selected");

    scene.enter(&mut EngineCtx, None);

    assert_eq!(scene.selected, Some(0), "opening the hatchery must auto-select the first egg");
}

/// With a single egg, auto-select seats it as both the hover target and the
/// selection.
#[test]
fn single_egg_hover_and_selection_both_resolve_to_it() {
    let mut scene = one_egg_scene();

    scene.enter(&mut EngineCtx, None);

    assert_eq!(scene.selected, Some(0), "the only egg must become the selection");
    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { hover: 0 }),
        "the only egg must also be the hover target, got {:?}",
        scene.mode
    );
}

/// Opening the hatchery with a single `Undefined` egg (the fresh-player
/// starter shape) lands directly in edit mode for it: a selected but
/// undefined egg's blanks must be editable, not blank.
#[test]
fn enter_on_undefined_egg_enters_edit_mode() {
    let mut scene = undefined_egg_scene();

    scene.enter(&mut EngineCtx, None);

    assert_eq!(scene.selected, Some(0), "opening on an undefined egg must select it");
    assert!(
        matches!(scene.mode, HatcheryMode::Editing { active_blank: 0 }),
        "opening on an undefined egg must enter edit mode, got {:?}",
        scene.mode
    );
    assert!(
        !scene.blank_editors.is_empty(),
        "entering edit mode on open must build the blank editors"
    );
}

/// The editable body painted by opening on an undefined egg is actually
/// rendered into the reserved detail region, not left empty.
#[test]
fn enter_on_undefined_egg_renders_editable_body() {
    let mut scene = undefined_egg_scene();

    scene.enter(&mut EngineCtx, None);

    let (w, h) = (60u16, 24u16);
    let buf = render_to_buffer(&scene, w, h);
    let area = Rect::new(0, 0, w, h);
    let (_egg, body, _tray) = detail_layout::detail_layout(area);

    let underline = mad_lib_paragraph::UNDERLINE_COLOR;
    let mut found = false;
    'scan: for y in body.top()..body.bottom() {
        for x in body.left()..body.right() {
            if let Some((mask, color)) = engine_render::decode_braille_cell(&buf, x, y) {
                if mask != 0 && color.r == underline.r && color.g == underline.g && color.b == underline.b {
                    found = true;
                    break 'scan;
                }
            }
        }
    }
    assert!(
        found,
        "opening on an undefined egg must paint its mad-lib's blank underline \
         (exact color {underline:?}) into the detail body region"
    );
}

/// Opening on a `Ready` egg selects it for the large read-only view but
/// must never launch a hatch by itself — auto-open is not a tap.
#[test]
fn enter_on_ready_egg_selects_without_hatching() {
    let mut scene = ready_egg_scene();

    scene.enter(&mut EngineCtx, None);

    assert_eq!(scene.selected, Some(0), "opening on a ready egg must still select it");
    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { hover: 0 }),
        "opening on a ready egg must stay in Browsing, got {:?}",
        scene.mode
    );
    assert_eq!(
        scene.take_hatch_request(),
        None,
        "opening the hatchery must never itself launch a hatch"
    );
}

/// An arrow key moves the tray hover without changing the master selection.
#[test]
fn arrow_right_moves_hover_without_changing_selection() {
    let mut scene = three_egg_scene();
    scene.select(0);

    scene.handle_input(key_event(KeyCode::Right));

    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { hover: 1 }),
        "Right must move the hover to the next egg, got {:?}",
        scene.mode
    );
    assert_eq!(scene.selected, Some(0), "moving the hover must not change the selection");
}

/// Tab from the last egg wraps the hover back to the first.
#[test]
fn tab_from_last_egg_wraps_hover_to_first() {
    let mut scene = three_egg_scene();
    scene.select(2);

    scene.handle_input(key_event(KeyCode::Tab));

    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { hover: 0 }),
        "Tab from the last egg must wrap the hover to the first, got {:?}",
        scene.mode
    );
}

/// BackTab from the first egg wraps the hover back to the last.
#[test]
fn backtab_from_first_egg_wraps_hover_to_last() {
    let mut scene = three_egg_scene();
    scene.select(0);

    scene.handle_input(key_event(KeyCode::BackTab));

    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { hover: 2 }),
        "BackTab from the first egg must wrap the hover to the last, got {:?}",
        scene.mode
    );
}

/// Enter on the hovered egg changes the master selection to it.
#[test]
fn enter_key_on_hovered_egg_changes_selection() {
    let mut scene = three_egg_scene();
    scene.select(0);
    scene.mode = HatcheryMode::Browsing { hover: 1 };

    scene.handle_input(key_event(KeyCode::Enter));

    assert_eq!(scene.selected, Some(1), "Enter on the hovered egg must select it");
}

/// Mouse movement over a tray egg sets the hover without changing the
/// selection. Rendered unselected so the tray chips lay out via
/// `tray::tray_band` — the same band the tray uses regardless of selection
/// once every tray chip renders through one shared draw pass.
#[test]
fn mouse_move_over_a_tray_egg_sets_hover_without_changing_selection() {
    let mut scene = three_egg_scene();
    assert!(scene.selected.is_none(), "fixture must render via the unselected tray layout");
    let (w, h) = (40u16, 20u16);
    let _ = render_to_buffer(&scene, w, h);
    let area = Rect::new(0, 0, w, h);
    let rect = tray::tray_slots(tray::tray_band(area), scene.eggs.len())[2].to_cell_rect();

    scene.handle_input(mouse_event(MouseEventKind::Moved, rect.x, rect.y));

    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { hover: 2 }),
        "moving over the third tray egg must set the hover to it, got {:?}",
        scene.mode
    );
    assert!(scene.selected.is_none(), "moving the hover must not change the selection");
}
