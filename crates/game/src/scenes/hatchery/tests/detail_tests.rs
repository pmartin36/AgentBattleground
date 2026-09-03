//! Read-only detail rendering for a selected, defined (`Incubating`/`Ready`)
//! egg: its completed `mad_lib` sentence renders as plain wrapped prose in
//! the detail body region, with no editable underline or caret, alongside
//! the existing HH:MM:SS countdown for an `Incubating` egg on a row distinct
//! from the prose.

use super::*;
use ratatui::style::Modifier;

/// Concatenates every cell's symbol on row `y` across `[x0, x1)`, so a
/// substring search can span the individual glyph cells a word occupies.
fn row_text(buf: &Buffer, y: u16, x0: u16, x1: u16) -> String {
    let mut s = String::new();
    for x in x0..x1 {
        if let Some(cell) = buf.cell((x, y)) {
            s.push_str(cell.symbol());
        }
    }
    s
}

/// The first row within `[y0, y1)` whose text contains `needle`, or `None`.
fn find_row_containing(buf: &Buffer, x0: u16, x1: u16, y0: u16, y1: u16, needle: &str) -> Option<u16> {
    (y0..y1).find(|&y| row_text(buf, y, x0, x1).contains(needle))
}

/// True iff any cell in `area` decodes to a lit dot in the mad-lib
/// paragraph's exact underline color.
fn area_has_underline_dot(buf: &Buffer, area: Rect) -> bool {
    let underline = mad_lib_paragraph::UNDERLINE_COLOR;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some((mask, color)) = engine_render::decode_braille_cell(buf, x, y) {
                if mask != 0 && color.r == underline.r && color.g == underline.g && color.b == underline.b {
                    return true;
                }
            }
        }
    }
    false
}

/// True iff any cell in `area` carries `Modifier::REVERSED` (a caret).
fn area_has_reversed_cell(buf: &Buffer, area: Rect) -> bool {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell((x, y)) {
                if cell.modifier.contains(Modifier::REVERSED) {
                    return true;
                }
            }
        }
    }
    false
}

fn incubating_egg_with_mad_lib(started_at: SystemTime, sentence: &str) -> Egg {
    Egg {
        element: Element::Fire,
        state: EggState::Incubating { started_at },
        mad_lib: Some(sentence.to_string()),
        egg_art: None,
        hatchling: None,
    }
}

fn ready_egg_with_mad_lib(sentence: &str) -> Egg {
    Egg { element: Element::Fire, state: EggState::Ready, mad_lib: Some(sentence.to_string()), egg_art: None, hatchling: None }
}

/// A selected `Incubating` egg with a completed `mad_lib` renders that
/// sentence as read-only prose in the detail body, on a row distinct from
/// the HH:MM:SS countdown the render path already draws for it.
#[test]
fn selected_incubating_defined_egg_renders_readonly_prose_below_countdown() {
    let dir = temp_store_dir("detail-incubating-prose");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg_with_mad_lib(now - Duration::from_secs(3600), "a big calm creature")],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
    scene.select(0);

    let (w, h) = (60u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    let (_egg_dr, body, _tray) = detail_layout::detail_layout(area);
    // The countdown's exact seconds can tick over between setup and render,
    // so both readings are accepted below.
    let before = lifecycle::remaining(&scene.eggs[0], SystemTime::now()).map(focus::format_remaining);
    let buf = render_to_buffer(&scene, w, h);
    let after = lifecycle::remaining(&scene.eggs[0], SystemTime::now()).map(focus::format_remaining);

    let word_row = find_row_containing(&buf, body.left(), body.right(), body.top(), body.bottom(), "calm")
        .expect(
            "a selected Incubating egg's completed mad_lib sentence must render as \
             read-only prose in the detail body",
        );

    let countdown_row = before
        .as_deref()
        .and_then(|s| find_row_containing(&buf, area.left(), area.right(), area.top(), area.bottom(), s))
        .or_else(|| {
            after
                .as_deref()
                .and_then(|s| find_row_containing(&buf, area.left(), area.right(), area.top(), area.bottom(), s))
        })
        .expect("the HH:MM:SS countdown must remain visible alongside the read-only prose");

    assert_ne!(word_row, countdown_row, "the read-only prose must not collide with the countdown row");
}

/// The same read-only prose carries no editable-mode decoration: no
/// braille underline dot and no `Modifier::REVERSED` caret cell.
#[test]
fn selected_incubating_defined_egg_prose_has_no_underline_or_caret() {
    let dir = temp_store_dir("detail-incubating-no-decoration");
    let now = SystemTime::now();
    let seed = PlayerData {
        roster: Vec::new(),
        eggs: vec![incubating_egg_with_mad_lib(now - Duration::from_secs(3600), "a big calm creature")],
    };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), now);
    scene.select(0);

    let (w, h) = (60u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    let (_egg_dr, body, _tray) = detail_layout::detail_layout(area);
    let buf = render_to_buffer(&scene, w, h);

    find_row_containing(&buf, body.left(), body.right(), body.top(), body.bottom(), "calm").expect(
        "a selected Incubating egg's completed mad_lib sentence must render as read-only prose \
         in the detail body",
    );
    assert!(!area_has_underline_dot(&buf, body), "read-only prose must carry no underline dots in the detail body");
    assert!(!area_has_reversed_cell(&buf, body), "read-only prose must carry no caret (REVERSED cell) in the detail body");
}

/// A selected `Ready` egg with a completed `mad_lib` renders the sentence
/// read-only in the detail body with no countdown (a `Ready` egg has no
/// `remaining` duration).
#[test]
fn selected_ready_defined_egg_renders_prose_without_countdown() {
    let dir = temp_store_dir("detail-ready-prose");
    let seed = PlayerData { roster: Vec::new(), eggs: vec![ready_egg_with_mad_lib("a swift clever hunter")] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    scene.select(0);

    assert_eq!(lifecycle::remaining(&scene.eggs[0], SystemTime::now()), None, "a Ready egg must have no countdown");

    let (w, h) = (60u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    let (_egg_dr, body, _tray) = detail_layout::detail_layout(area);
    let buf = render_to_buffer(&scene, w, h);

    find_row_containing(&buf, body.left(), body.right(), body.top(), body.bottom(), "clever").expect(
        "a selected Ready egg's completed mad_lib sentence must render as read-only prose in the \
         detail body",
    );
    assert!(!area_has_underline_dot(&buf, body), "a Ready egg's read-only prose must carry no underline dots");
}

/// Activating a selected `Ready` egg (Enter on the hovered egg) records the
/// hatch request keyed off it and leaves the selection/mode exactly as they
/// were — it never enters edit mode and never changes the selection.
#[test]
fn activating_selected_ready_egg_records_hatch_request_without_changing_selection() {
    let dir = temp_store_dir("detail-ready-activate");
    let seed = PlayerData { roster: Vec::new(), eggs: vec![ready_egg_with_mad_lib("a swift clever hunter")] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");
    let mut scene = Hatchery::from_store_at(PlayerStore::with_dir(&dir), SystemTime::now());
    scene.select(0);

    scene.handle_input(key_event(KeyCode::Enter));

    assert_eq!(scene.take_hatch_request(), Some(0), "activating a selected Ready egg must record a hatch request for it");
    assert_eq!(scene.selected, Some(0), "activating a Ready egg must not change the selection");
    assert!(
        matches!(scene.mode, HatcheryMode::Browsing { hover: 0 }),
        "activating a Ready egg must never enter edit mode, got {:?}",
        scene.mode
    );
}
