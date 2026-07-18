//! `render`'s glyph-mask-invariance regression tests. The bug this guards: a
//! prior version fed the post-`ButtonState`-tint buffer into `dots_to_grid`,
//! so the adaptive-luma mask could flip per-dot between states purely from
//! `tint`'s per-channel integer rounding — the painted glyph (e.g.
//! `⣻`/`⢻`/`⣿`) shifted across Idle/Hover/Pressed even though the underlying
//! shape never changed. Split out of `button_tests.rs` (b1-t1) into its own
//! concern-partitioned sibling file, byte-for-byte unchanged.

use super::*;
use super::button_test_fixtures::*;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};
use ratatui::style::Color;

fn make_buf(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

fn ev(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// Drives any `ButtonCore`-backed widget (reachable here via
/// `Deref`/`DerefMut`) to `state` via the same mouse sequence the other
/// test modules use.
fn set_state<B: DerefMut<Target = ButtonCore>>(b: &mut B, state: ButtonState) {
    let rect = b.rect();
    let inside = (rect.x + 1, rect.y + 1);
    match state {
        ButtonState::Idle => {}
        ButtonState::Hover => {
            b.handle_mouse(&ev(MouseEventKind::Moved, inside.0, inside.1));
        }
        ButtonState::Pressed => {
            b.handle_mouse(&ev(MouseEventKind::Moved, inside.0, inside.1));
            b.handle_mouse(&ev(
                MouseEventKind::Down(MouseButton::Left),
                inside.0,
                inside.1,
            ));
        }
    }
    assert_eq!(b.state(), state, "test setup must reach the target state");
}

/// Renders into a buffer sized to fit `rect` plus margin, and returns
/// `(symbol, fg)` for every one of `cells` (in `(x, y)` buffer coords).
fn render_and_sample(
    render: impl FnOnce(&mut Buffer),
    rect: Rect,
    cells: &[(u16, u16)],
) -> Vec<(String, Color)> {
    let mut buf = make_buf(rect.x + rect.width + 4, rect.y + rect.height + 4);
    render(&mut buf);
    cells
        .iter()
        .map(|&(x, y)| {
            let cell = buf
                .cell((x, y))
                .unwrap_or_else(|| panic!("cell ({x},{y}) must exist in the buffer"));
            (cell.symbol().to_string(), cell.fg)
        })
        .collect()
}

fn button_rect() -> Rect {
    Rect::new(2, 1, 8, 4)
}

/// Every cell of the button's rect, in row-major order.
fn all_cells(rect: Rect) -> Vec<(u16, u16)> {
    (rect.y..rect.y + rect.height)
        .flat_map(|y| (rect.x..rect.x + rect.width).map(move |x| (x, y)))
        .collect()
}

/// `Button::render` (with an icon, exercising a real panel+icon
/// composite): for a fixed rect, the painted glyph at every cell must be
/// IDENTICAL across `Idle`/`Hover`/`Pressed`; at least one cell's `fg`
/// must differ between two of the states.
#[test]
fn render_glyph_mask_invariant_across_states() {
    let rect = button_rect();
    let cells = all_cells(rect);

    let mut idle = Button::new(rect, panel_bytes()).icon(icon_bytes());
    set_state(&mut idle, ButtonState::Idle);
    let idle_cells = render_and_sample(|buf| idle.render(buf), rect, &cells);

    let mut hover = Button::new(rect, panel_bytes()).icon(icon_bytes());
    set_state(&mut hover, ButtonState::Hover);
    let hover_cells = render_and_sample(|buf| hover.render(buf), rect, &cells);

    let mut pressed = Button::new(rect, panel_bytes()).icon(icon_bytes());
    set_state(&mut pressed, ButtonState::Pressed);
    let pressed_cells = render_and_sample(|buf| pressed.render(buf), rect, &cells);

    for (i, ((xy, (is, _)), hs)) in cells
        .iter()
        .zip(idle_cells.iter())
        .zip(hover_cells.iter().map(|(s, _)| s))
        .enumerate()
    {
        assert_eq!(
            is, hs,
            "cell {i} {xy:?}: glyph must be identical between Idle ({is:?}) and Hover ({hs:?})"
        );
    }
    for (i, ((xy, (is, _)), ps)) in cells
        .iter()
        .zip(idle_cells.iter())
        .zip(pressed_cells.iter().map(|(s, _)| s))
        .enumerate()
    {
        assert_eq!(
            is, ps,
            "cell {i} {xy:?}: glyph must be identical between Idle ({is:?}) and Pressed ({ps:?})"
        );
    }

    let colors_differ = idle_cells
        .iter()
        .zip(hover_cells.iter())
        .any(|((_, ic), (_, hc))| ic != hc);
    assert!(
        colors_differ,
        "at least one cell's fg must differ between Idle and Hover"
    );
}

/// Sized to actually exhibit the pre-fix mask-flip (confirmed by probing
/// `FRAME_PANEL` at a range of dims): `8x4` never flips, but `3x11` does,
/// at both Idle-vs-Hover and Idle-vs-Pressed.
fn frame_rect() -> Rect {
    Rect::new(2, 1, 3, 11)
}

/// Border-ring cells of `rect` (top row, bottom row, left/right columns),
/// excluding the label's centered row — mirrors the hollow-frame label
/// `Button`'s own painted regions (hollow frame ring; interior/label are
/// separate concerns from the dot-pipeline mask under test here).
fn border_cells(rect: Rect) -> Vec<(u16, u16)> {
    let label_row = rect.y + rect.height / 2;
    let mut cells: Vec<(u16, u16)> = (rect.x..rect.x + rect.width)
        .flat_map(|x| [(x, rect.y), (x, rect.y + rect.height - 1)])
        .collect();
    cells.extend(
        (rect.y..rect.y + rect.height)
            .filter(|&y| y != label_row)
            .flat_map(|y| [(rect.x, y), (rect.x + rect.width - 1, y)]),
    );
    cells
}

/// Label `Button` render: same glyph-mask-invariance assertion as
/// `render_glyph_mask_invariant_across_states`, over the border ring
/// cells (the frame's only opaque, `ButtonState`-tinted region).
#[test]
fn frame_button_glyph_mask_invariant_across_states() {
    let rect = frame_rect();
    let cells = border_cells(rect);

    let mut idle = Button::new(rect, frame_bytes()).label("Go");
    set_state(&mut idle, ButtonState::Idle);
    let idle_cells = render_and_sample(|buf| idle.render(buf), rect, &cells);

    let mut hover = Button::new(rect, frame_bytes()).label("Go");
    set_state(&mut hover, ButtonState::Hover);
    let hover_cells = render_and_sample(|buf| hover.render(buf), rect, &cells);

    let mut pressed = Button::new(rect, frame_bytes()).label("Go");
    set_state(&mut pressed, ButtonState::Pressed);
    let pressed_cells = render_and_sample(|buf| pressed.render(buf), rect, &cells);

    for (i, ((xy, (is, _)), hs)) in cells
        .iter()
        .zip(idle_cells.iter())
        .zip(hover_cells.iter().map(|(s, _)| s))
        .enumerate()
    {
        assert_eq!(
            is, hs,
            "border cell {i} {xy:?}: glyph must be identical between Idle ({is:?}) and Hover ({hs:?})"
        );
    }
    for (i, ((xy, (is, _)), ps)) in cells
        .iter()
        .zip(idle_cells.iter())
        .zip(pressed_cells.iter().map(|(s, _)| s))
        .enumerate()
    {
        assert_eq!(
            is, ps,
            "border cell {i} {xy:?}: glyph must be identical between Idle ({is:?}) and Pressed ({ps:?})"
        );
    }

    let colors_differ = idle_cells
        .iter()
        .zip(hover_cells.iter())
        .any(|((_, ic), (_, hc))| ic != hc);
    assert!(
        colors_differ,
        "at least one border cell's fg must differ between Idle and Hover"
    );
}
