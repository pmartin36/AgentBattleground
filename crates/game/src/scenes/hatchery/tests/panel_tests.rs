//! Right-panel geometry and STATUS text: `panel_regions` carves the panel's
//! inner content into STATUS/body/button rects strictly inside the border
//! ring, and `status_text` reports each egg state's exact readout —
//! including a whole-hours/minutes Incubating remaining-time string, never
//! the `HH:MM:SS` form.

use super::*;

use engine_render::DotRect;

fn undefined_egg() -> Egg {
    Egg { element: Element::Fire, state: EggState::Undefined, mad_lib: None, egg_art: None, hatchling: None }
}

fn incubating_egg(started_at: SystemTime) -> Egg {
    Egg { element: Element::Fire, state: EggState::Incubating { started_at }, mad_lib: None, egg_art: None, hatchling: None }
}

fn ready_egg() -> Egg {
    Egg { element: Element::Fire, state: EggState::Ready, mad_lib: None, egg_art: None, hatchling: None }
}

/// An `Undefined` egg's STATUS reads exactly "Awaiting Description".
#[test]
fn status_text_undefined_is_awaiting_description() {
    let now = SystemTime::now();
    assert_eq!(browse_panel::status_text(&undefined_egg(), now), "Awaiting Description");
}

/// A `Ready` egg's STATUS reads exactly "Ready to Hatch".
#[test]
fn status_text_ready_is_ready_to_hatch() {
    let now = SystemTime::now();
    assert_eq!(browse_panel::status_text(&ready_egg(), now), "Ready to Hatch");
}

/// An `Incubating` egg's STATUS reports whole hours and minutes remaining
/// (never seconds, never the `HH:MM:SS` form) with the exact copy the panel
/// shows.
#[test]
fn status_text_incubating_is_whole_hours_minutes_not_hhmmss() {
    let now = SystemTime::now();
    let egg = incubating_egg(now - Duration::from_secs(30 * 60));
    let text = browse_panel::status_text(&egg, now);
    assert_eq!(text, "Incubating \u{2014} 23h 30m remaining");
    assert!(!text.contains(':'), "status text {text:?} must not be the HH:MM:SS form");
}

/// The STATUS, body, and button regions are each inset at least one cell
/// from the panel's border ring, and are stacked STATUS above body above
/// button — none overlaps the border and none overlaps another region.
#[test]
fn panel_regions_are_strictly_inside_the_border() {
    let panel = DotRect { x: 0, y: 0, w: 80, h: 120 };
    let regions = browse_panel::panel_regions(panel);
    let panel_cells = panel.to_cell_rect();

    assert!(regions.status.width > 0 && regions.status.height > 0, "status region {:?} must be non-degenerate", regions.status);
    assert!(regions.body.width > 0 && regions.body.height > 0, "body region {:?} must be non-degenerate", regions.body);
    assert!(regions.button.width > 0 && regions.button.height > 0, "button region {:?} must be non-degenerate", regions.button);

    assert!(regions.status.y > panel_cells.y, "status {:?} must be inset below the panel's top border", regions.status);
    assert!(regions.status.x > panel_cells.x, "status {:?} must be inset right of the panel's left border", regions.status);
    assert!(
        regions.status.x + regions.status.width < panel_cells.x + panel_cells.width,
        "status {:?} must be inset left of the panel's right border",
        regions.status
    );
    assert!(
        regions.button.y + regions.button.height < panel_cells.y + panel_cells.height,
        "button {:?} must be inset above the panel's bottom border",
        regions.button
    );

    assert!(
        regions.status.y + regions.status.height <= regions.body.y,
        "status {:?} must sit above body {:?}",
        regions.status,
        regions.body
    );
    assert!(
        regions.body.y + regions.body.height <= regions.button.y,
        "body {:?} must sit above button {:?}",
        regions.body,
        regions.button
    );
}

/// A degenerate (zero-size) panel yields regions that fit within it (never
/// wider or taller than the panel itself) rather than panicking or
/// overflowing.
#[test]
fn panel_regions_degenerate_panel_no_panic() {
    let panel = DotRect { x: 0, y: 0, w: 0, h: 0 };
    let panel_cells = panel.to_cell_rect();
    let regions = browse_panel::panel_regions(panel);
    assert!(regions.status.width <= panel_cells.width && regions.status.height <= panel_cells.height);
    assert!(regions.body.width <= panel_cells.width && regions.body.height <= panel_cells.height);
    assert!(regions.button.width <= panel_cells.width && regions.button.height <= panel_cells.height);
}

/// The right panel draws a grey braille-ring border around browse_layout's
/// panel rect. Isolated with a `Ready` egg (read-only, no mad-lib
/// underline, which shares the border's exact grey) so the only source of
/// that color in the panel is the border itself.
#[test]
fn panel_border_is_grey_braille_ring() {
    let dir = temp_store_dir("panel-border");
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![Egg { element: Element::Fire, state: EggState::Ready, mad_lib: Some("a swift clever hunter".to_string()), egg_art: None, hatchling: None }],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    scene.select(0);

    let (w, h) = (90u16, 30u16);
    let buf = render_to_buffer(&scene, w, h);
    let area = Rect::new(0, 0, w, h);
    let panel_cells = browse_layout::browse_layout(area).panel.to_cell_rect();

    let border = mad_lib_paragraph::UNDERLINE_COLOR;
    let mut found = false;
    'scan: for y in panel_cells.top()..panel_cells.bottom() {
        for x in panel_cells.left()..panel_cells.right() {
            if let Some((mask, color)) = engine_render::decode_braille_cell(&buf, x, y) {
                if mask != 0 && color.r == border.r && color.g == border.g && color.b == border.b {
                    found = true;
                    break 'scan;
                }
            }
        }
    }
    assert!(found, "expected a grey (0x888888) braille dot somewhere on the right panel's border ring");
}
