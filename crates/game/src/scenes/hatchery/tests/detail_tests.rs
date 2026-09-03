//! Read-only detail rendering for a selected, defined (`Incubating`/`Ready`)
//! egg: its completed `mad_lib` sentence renders as plain wrapped prose in
//! the right panel's body, with no editable underline or caret, alongside
//! the panel's whole-hours/minutes remaining-time STATUS row for an
//! `Incubating` egg, on a row distinct from the prose.

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

/// True iff `line` contains a `DD:DD:DD` digit-colon-digit-colon-digit
/// pattern (an `HH:MM:SS` countdown), scanning every 8-character window.
fn contains_hhmmss(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let is_digit_at = |i: usize| chars.get(i).is_some_and(|c| c.is_ascii_digit());
    (0..chars.len().saturating_sub(7)).any(|i| {
        is_digit_at(i)
            && is_digit_at(i + 1)
            && chars.get(i + 2) == Some(&':')
            && is_digit_at(i + 3)
            && is_digit_at(i + 4)
            && chars.get(i + 5) == Some(&':')
            && is_digit_at(i + 6)
            && is_digit_at(i + 7)
    })
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
/// sentence as read-only prose in the panel body, on a row distinct from the
/// panel's whole-hours/minutes remaining-time STATUS row.
#[test]
fn selected_incubating_defined_egg_renders_readonly_prose_and_whole_hm_status() {
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
    let panel = browse_layout::browse_layout(area).panel;
    let regions = browse_panel::panel_regions(panel);
    let buf = render_to_buffer(&scene, w, h);

    let word_row = find_row_containing(&buf, regions.body.left(), regions.body.right(), regions.body.top(), regions.body.bottom(), "calm")
        .expect(
            "a selected Incubating egg's completed mad_lib sentence must render as \
             read-only prose in the panel body",
        );

    let status_row = find_row_containing(&buf, regions.status.left(), regions.status.right(), regions.status.top(), regions.status.bottom(), "remaining")
        .expect("the panel STATUS row must show a whole-hours/minutes remaining readout");
    let status_line = row_text(&buf, status_row, regions.status.left(), regions.status.right());
    assert!(!status_line.contains(':'), "STATUS row {status_line:?} must not be the HH:MM:SS form");

    assert_ne!(word_row, status_row, "the read-only prose must not collide with the STATUS row");
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
    let panel = browse_layout::browse_layout(area).panel;
    let body = browse_panel::panel_regions(panel).body;
    let buf = render_to_buffer(&scene, w, h);

    find_row_containing(&buf, body.left(), body.right(), body.top(), body.bottom(), "calm").expect(
        "a selected Incubating egg's completed mad_lib sentence must render as read-only prose \
         in the panel body",
    );
    assert!(!area_has_underline_dot(&buf, body), "read-only prose must carry no underline dots in the panel body");
    assert!(!area_has_reversed_cell(&buf, body), "read-only prose must carry no caret (REVERSED cell) in the panel body");
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
    let panel = browse_layout::browse_layout(area).panel;
    let body = browse_panel::panel_regions(panel).body;
    let buf = render_to_buffer(&scene, w, h);

    find_row_containing(&buf, body.left(), body.right(), body.top(), body.bottom(), "clever").expect(
        "a selected Ready egg's completed mad_lib sentence must render as read-only prose in the \
         panel body",
    );
    assert!(!area_has_underline_dot(&buf, body), "a Ready egg's read-only prose must carry no underline dots");

    for y in area.top()..area.bottom() {
        let line = row_text(&buf, y, area.left(), area.right());
        assert!(
            !contains_hhmmss(&line),
            "a Ready egg's render must show no HH:MM:SS-style countdown anywhere, found {line:?}"
        );
    }
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
