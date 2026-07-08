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
