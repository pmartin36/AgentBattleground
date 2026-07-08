//! Shared test-support helpers for scene test modules. Consolidates helpers
//! that were previously redefined near-verbatim across `main_hub.rs`,
//! `roster_manager.rs` (which redefined several of them multiple times
//! within itself), and `battle_viewer.rs`.

#![cfg(test)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Terminal;

use engine_core::scene::{InputEvent, Scene};

/// Render `scene` into a fresh `w`×`h` `TestBackend` and return the resulting
/// buffer. The render area is always `Rect::new(0, 0, w, h)` — construct that
/// directly rather than threading it back out of this helper.
pub(crate) fn render_to_buffer(scene: &dyn Scene, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            scene.render(f, area);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

/// A bare key-press `InputEvent` with no modifiers.
pub(crate) fn key_event(code: KeyCode) -> InputEvent {
    InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// A mouse `InputEvent` at `(column, row)` with no modifiers.
pub(crate) fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> InputEvent {
    InputEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })
}

/// True if any cell inside `rect` is non-space.
pub(crate) fn has_non_space(buf: &Buffer, rect: Rect) -> bool {
    (rect.top()..rect.bottom())
        .flat_map(|y| (rect.left()..rect.right()).map(move |x| (x, y)))
        .any(|(x, y)| buf.cell((x, y)).unwrap().symbol() != " ")
}

/// Concatenates every cell's symbol across `rect` into a single `String`
/// (row by row, no separators) so a plain-text substring assertion can be
/// made regardless of which row within `rect` the text lands on.
pub(crate) fn rect_text(buf: &Buffer, rect: Rect) -> String {
    let mut s = String::new();
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            s.push_str(buf.cell((x, y)).unwrap().symbol());
        }
    }
    s
}

/// Every cell's (symbol, fg) within `rect`, row-major — an exact-content
/// snapshot for equality comparisons restricted to one rect.
pub(crate) fn region_cells(buf: &Buffer, rect: Rect) -> Vec<(String, Color)> {
    let mut out = Vec::new();
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            let cell = buf.cell((x, y)).unwrap();
            out.push((cell.symbol().to_string(), cell.fg));
        }
    }
    out
}

/// The fg color of the first non-space cell found inside `slot`, or `None`
/// if the slot has no painted cell.
pub(crate) fn sample_fg(buf: &Buffer, slot: Rect) -> Option<Color> {
    (slot.top()..slot.bottom())
        .flat_map(|y| (slot.left()..slot.right()).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let cell = buf.cell((x, y))?;
            if cell.symbol() != " " {
                Some(cell.fg)
            } else {
                None
            }
        })
}

/// The 8-bit braille mask (bit k set = dot k, per `dots.rs`'s `DOTS` table)
/// of the glyph at `(x, y)`, or `None` if the cell is not a painted braille
/// glyph. Thin wrapper over `engine_render::decode_braille_cell`.
pub(crate) fn braille_mask(buf: &Buffer, x: u16, y: u16) -> Option<u32> {
    engine_render::decode_braille_cell(buf, x, y).map(|(mask, _color)| mask as u32)
}

/// The absolute DOT row (`cell_y * 4 + dy`, `dy` in `0..4`) of the topmost
/// LIT dot found in column `col`, scanning cell rows near `near_row`
/// (`near_row - 2 ..= near_row + 2`). Use this — never a bare `Rect`/
/// `DotRect` coordinate comparison — to check whether two braille-rendered
/// elements are actually visually aligned: two elements sharing a terminal
/// cell row are NOT proven aligned by that alone, since different drawing
/// routines can place their lit dots anywhere within a cell (this is
/// exactly how a real bug shipped once — two borders shared a cell yet
/// rendered 2 dots apart). See `CLAUDE.md`'s "`ratatui::Rect` is
/// cell-quantized" rule.
///
/// Panics if no lit dot is found in the scanned range — callers should
/// already know roughly where the element they're checking renders.
pub(crate) fn topmost_lit_dot_row(buf: &Buffer, col: u16, near_row: u16) -> i32 {
    // Bit k -> (dx, dy), per `dots.rs`'s `DOTS` table; dy is what we need.
    const DOT_DY: [u8; 8] = [0, 1, 2, 0, 1, 2, 3, 3];
    for y in near_row.saturating_sub(2)..near_row.saturating_add(3) {
        if let Some(mask) = braille_mask(buf, col, y) {
            if let Some(dy) = (0..4u8).find(|&dy| (0..8u8).any(|k| DOT_DY[k as usize] == dy && mask & (1 << k) != 0)) {
                return y as i32 * 4 + dy as i32;
            }
        }
    }
    panic!("no lit dot found near row {near_row} at column {col}");
}

// ── b7-t1: golden-fixture serialize/deserialize helpers ─────────────────────

/// Serialize every LIT braille cell of `buf`, through `decode_braille_cell`,
/// into a stable text form: header `"dims {w} {h}"` then one
/// `"{y} {x} {mask:02x} {rr}{gg}{bb}"` line per lit cell, row-major.
/// Non-braille/blank cells are omitted.
pub(crate) fn serialize_braille_buffer(buf: &Buffer) -> String {
    let (w, h) = (buf.area.width, buf.area.height);
    let mut lines = vec![format!("dims {w} {h}")];
    for y in 0..h {
        for x in 0..w {
            if let Some((mask, color)) = engine_render::decode_braille_cell(buf, x, y) {
                lines.push(format!(
                    "{y} {x} {mask:02x} {:02x}{:02x}{:02x}",
                    color.r, color.g, color.b
                ));
            }
        }
    }
    lines.join("\n")
}

/// Inverse of [`serialize_braille_buffer`]: parse `dims`, build
/// `Buffer::empty(Rect::new(0, 0, w, h))`, and seed each listed cell with
/// char `0x2800 + mask` and `fg = Color::Rgb(r, g, b)`.
pub(crate) fn deserialize_braille_buffer(s: &str) -> Buffer {
    let mut lines = s.lines();
    let header = lines.next().expect("serialized buffer must have a dims header");
    let mut parts = header.split_whitespace();
    let tag = parts.next().expect("dims header must start with 'dims'");
    assert_eq!(tag, "dims", "expected 'dims' header, got {tag:?}");
    let w: u16 = parts.next().expect("dims header missing width").parse().expect("width must be u16");
    let h: u16 = parts.next().expect("dims header missing height").parse().expect("height must be u16");

    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let y: u16 = parts.next().expect("cell line missing y").parse().expect("y must be u16");
        let x: u16 = parts.next().expect("cell line missing x").parse().expect("x must be u16");
        let mask_hex = parts.next().expect("cell line missing mask");
        let mask = u8::from_str_radix(mask_hex, 16).expect("mask must be 2-digit hex");
        let color_hex = parts.next().expect("cell line missing color");
        assert_eq!(color_hex.len(), 6, "color must be 6 hex digits, got {color_hex:?}");
        let r = u8::from_str_radix(&color_hex[0..2], 16).expect("r must be hex");
        let g = u8::from_str_radix(&color_hex[2..4], 16).expect("g must be hex");
        let b = u8::from_str_radix(&color_hex[4..6], 16).expect("b must be hex");

        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_char(char::from_u32(0x2800 + mask as u32).expect("valid braille codepoint"));
            cell.set_fg(Color::Rgb(r, g, b));
        }
    }
    buf
}

/// Full per-cell symbol grid of `buf`, rows joined by `'\n'` — the
/// human-eyeball braille-art preview artifact.
pub(crate) fn buffer_to_art(buf: &Buffer) -> String {
    let (w, h) = (buf.area.width, buf.area.height);
    let mut rows = Vec::with_capacity(h as usize);
    for y in 0..h {
        let mut row = String::with_capacity(w as usize);
        for x in 0..w {
            row.push_str(buf.cell((x, y)).unwrap().symbol());
        }
        rows.push(row);
    }
    rows.join("\n")
}

/// Load a committed fixture from `tests/fixtures/{dir}/{name}.fixture` and
/// decode it into a `Buffer` via [`deserialize_braille_buffer`].
fn load_fixture(dir: &str, name: &str) -> Buffer {
    let path = format!(
        "{}/tests/fixtures/{dir}/{name}.fixture",
        env!("CARGO_MANIFEST_DIR")
    );
    let s = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    deserialize_braille_buffer(&s)
}

/// Load a committed Roster fixture by scenario `name` (e.g. `"rest_40x20"`).
pub(crate) fn load_roster_fixture(name: &str) -> Buffer {
    load_fixture("roster", name)
}

/// Load a committed Main Hub fixture by scenario `name` from
/// `tests/fixtures/main_hub/{name}.fixture`.
pub(crate) fn load_main_hub_fixture(name: &str) -> Buffer {
    load_fixture("main_hub", name)
}

#[cfg(test)]
mod golden_fixture_helper_tests {
    use super::*;

    /// A small hand-seeded multi-cell `Buffer` must serialize then
    /// deserialize to a `Buffer` that `diff_dots` reports as an exact
    /// match — guards the serializer/loader independent of Roster.
    #[test]
    fn serialize_braille_buffer_round_trips_through_decode() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        // (0,0): mask 0xC8, orange. (2,0): mask 0x01, blue. (1,1): mask 0xFF, white.
        if let Some(cell) = buf.cell_mut((0u16, 0u16)) {
            cell.set_char(char::from_u32(0x2800 + 0xC8).unwrap());
            cell.set_fg(Color::Rgb(200, 100, 10));
        }
        if let Some(cell) = buf.cell_mut((2u16, 0u16)) {
            cell.set_char(char::from_u32(0x2800 + 0x01).unwrap());
            cell.set_fg(Color::Rgb(10, 20, 200));
        }
        if let Some(cell) = buf.cell_mut((1u16, 1u16)) {
            cell.set_char(char::from_u32(0x2800 + 0xFF).unwrap());
            cell.set_fg(Color::Rgb(255, 255, 255));
        }

        let serialized = serialize_braille_buffer(&buf);
        let round_tripped = deserialize_braille_buffer(&serialized);

        let diff = engine_render::diff_dots(&buf, &round_tripped);
        assert!(
            diff.is_match(),
            "round-tripped buffer must diff_dots-match the original exactly; mismatches: {:?}",
            diff.mismatches
        );
    }
}
