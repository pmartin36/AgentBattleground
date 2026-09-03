//! Right-panel action-button tests: the per-state Submit/Hatch button's
//! disabled-grey vs active-gold border, the disabled Hatch button's hover
//! tooltip, and the active Hatch button's click outcome.

use super::*;

use crate::scenes::test_util::rect_text;

const AREA_W: u16 = 90;
const AREA_H: u16 = 30;

fn undefined_egg() -> Egg {
    Egg { element: Element::Fire, state: EggState::Undefined, mad_lib: None, egg_art: None, hatchling: None }
}

fn incubating_egg(started_at: SystemTime) -> Egg {
    Egg { element: Element::Fire, state: EggState::Incubating { started_at }, mad_lib: None, egg_art: None, hatchling: None }
}

fn ready_egg() -> Egg {
    Egg { element: Element::Fire, state: EggState::Ready, mad_lib: Some("a swift clever hunter".to_string()), egg_art: None, hatchling: None }
}

fn scene_with_egg(tag: &str, egg: Egg) -> Hatchery {
    let dir = temp_store_dir(tag);
    let seed = PlayerData { roster: Vec::new(), eggs: vec![egg] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now())
}

/// The right panel's reserved action-button slot for `area`.
fn button_slot(area: Rect) -> Rect {
    let panel = browse_layout::browse_layout(area).panel;
    browse_panel::panel_regions(panel).button
}

/// The cell the action button's own rect always occupies at its
/// bottom-right corner, regardless of its clamped width — safe to click or
/// hover no matter how wide the button ends up being drawn.
fn bottom_right(rect: Rect) -> (u16, u16) {
    (rect.right().saturating_sub(1), rect.bottom().saturating_sub(1))
}

/// The color of the first lit braille dot found in `rect`, or `None` if
/// nothing is painted there.
fn first_lit_color(buf: &Buffer, rect: Rect) -> Option<engine_core::color::Rgba> {
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            if let Some((mask, color)) = engine_render::decode_braille_cell(buf, x, y) {
                if mask != 0 {
                    return Some(color);
                }
            }
        }
    }
    None
}

/// While editing an `Undefined` egg with a blank still empty, the panel's
/// action button (Submit) is disabled: its border renders greyscale
/// (r == g == b), never the panel's own active gold.
#[test]
fn submit_button_is_grey_while_a_blank_is_empty() {
    let mut scene = scene_with_egg("submit-grey", undefined_egg());
    scene.enter_edit(0);

    let area = Rect::new(0, 0, AREA_W, AREA_H);
    let buf = render_to_buffer(&scene, AREA_W, AREA_H);
    let slot = button_slot(area);

    let color = first_lit_color(&buf, slot)
        .expect("the disabled Submit button must still paint a lit border dot in its slot");
    assert_eq!(color.r, color.g, "a disabled Submit button's border must be greyscale, got {color:?}");
    assert_eq!(color.g, color.b, "a disabled Submit button's border must be greyscale, got {color:?}");
}

/// A selected `Incubating` egg's action button (Hatch) renders disabled:
/// its border is greyscale, the same mechanism the disabled Submit button
/// uses.
#[test]
fn incubating_hatch_button_is_disabled_grey() {
    let mut scene = scene_with_egg("hatch-grey", incubating_egg(SystemTime::now()));
    scene.select(0);

    let area = Rect::new(0, 0, AREA_W, AREA_H);
    let buf = render_to_buffer(&scene, AREA_W, AREA_H);
    let slot = button_slot(area);

    let color = first_lit_color(&buf, slot)
        .expect("an Incubating egg's disabled Hatch button must paint a lit border dot in its slot");
    assert_eq!(color.r, color.g, "a disabled Hatch button's border must be greyscale, got {color:?}");
    assert_eq!(color.g, color.b, "a disabled Hatch button's border must be greyscale, got {color:?}");
}

/// A selected `Ready` egg's action button (Hatch) renders active: its
/// border reads gold (r > b), not greyscale.
#[test]
fn ready_hatch_button_is_active_gold() {
    let mut scene = scene_with_egg("hatch-gold", ready_egg());
    scene.select(0);

    let area = Rect::new(0, 0, AREA_W, AREA_H);
    let buf = render_to_buffer(&scene, AREA_W, AREA_H);
    let slot = button_slot(area);

    let color = first_lit_color(&buf, slot)
        .expect("a Ready egg's active Hatch button must paint a lit border dot in its slot");
    assert!(color.r > color.b, "an active Hatch button's border must read gold (r > b), got {color:?}");
}

/// Hovering the disabled Hatch button on a selected `Incubating` egg draws
/// a card carrying the exact disabled-hatch tooltip text.
#[test]
fn incubating_hatch_hover_shows_verbatim_tooltip() {
    const TIP: &str = "Hatching is available once incubation is complete.";
    let mut scene = scene_with_egg("hatch-tooltip", incubating_egg(SystemTime::now()));
    scene.select(0);

    let area = Rect::new(0, 0, AREA_W, AREA_H);
    let _ = render_to_buffer(&scene, AREA_W, AREA_H);
    let (hx, hy) = bottom_right(button_slot(area));
    scene.handle_input(mouse_event(MouseEventKind::Moved, hx, hy));

    let buf = render_to_buffer(&scene, AREA_W, AREA_H);
    // The card's fill is U+2800 (braille blank), not " ", and word-wrapped
    // rows are concatenated with no separator by `rect_text` — normalize
    // both before asserting so a message that wraps across rows still
    // reads as one space-joined sentence.
    let normalized = rect_text(&buf, area)
        .replace('\u{2800}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        normalized.contains(TIP),
        "hovering the disabled Hatch button must draw a tooltip carrying the exact text {TIP:?}, got {normalized:?}"
    );
}

/// A completed click on a selected `Ready` egg's active Hatch button
/// records the hatch request the hatch hand-off consumes.
#[test]
fn ready_hatch_click_records_hatch_request() {
    let mut scene = scene_with_egg("hatch-click", ready_egg());
    scene.select(0);

    let area = Rect::new(0, 0, AREA_W, AREA_H);
    let _ = render_to_buffer(&scene, AREA_W, AREA_H);
    let (bx, by) = bottom_right(button_slot(area));
    tap_at(&mut scene, bx, by);

    assert_eq!(
        scene.take_hatch_request(),
        Some(0),
        "clicking the active Hatch button must record a hatch request for the selected egg"
    );
}
