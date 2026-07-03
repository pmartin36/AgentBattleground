//! Shared test-support helpers for scene test modules. Consolidates helpers
//! that were previously redefined near-verbatim across `main_hub.rs`,
//! `roster_manager.rs` (which redefined several of them multiple times
//! within itself), and `battle_viewer.rs`.

#![cfg(test)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;

use scene_core::scene::{InputEvent, Scene};

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

/// Row index of the first row whose text contains `needle`, if any.
pub(crate) fn row_containing(buf: &Buffer, w: u16, h: u16, needle: &str) -> Option<u16> {
    (0..h).find(|&y| {
        let row_text: String = (0..w)
            .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
            .collect();
        row_text.contains(needle)
    })
}
