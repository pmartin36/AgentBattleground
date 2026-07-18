//! b3-t1: unified `Button` builder + per-state 3-layer render. Covers the
//! new `.icon()`/`.label()`/`.colors()` builder wiring and the render
//! contract (background always painted; icon iff `.icon()` set; label iff
//! `.label()` set; each layer recolors per `ButtonColors::for_state` across
//! Idle/Hover/Pressed). Verified by DECODING the rendered `Buffer` (per
//! CLAUDE.md), never by reading the builder's private fields. Split out of
//! `button_tests.rs` (b1-t1) into its own concern-partitioned sibling file,
//! byte-for-byte unchanged.

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

/// Drives a `Button` to `state` via the same mouse sequence every other
/// test module in this file uses.
fn set_state(b: &mut Button, state: ButtonState) {
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

/// Three distinct, easily-told-apart `StateColors` per state, on all
/// three layers, for asserting per-state recolor without relying on the
/// (lossless) default scheme.
fn custom_colors() -> ButtonColors {
    ButtonColors {
        idle: StateColors {
            background: Rgba::rgb(0x10, 0x20, 0x30),
            icon: Rgba::rgb(0x40, 0x50, 0x60),
            label: Rgba::rgb(0x70, 0x80, 0x90),
        },
        hover: StateColors {
            background: Rgba::rgb(0x11, 0x22, 0x33),
            icon: Rgba::rgb(0x44, 0x55, 0x66),
            label: Rgba::rgb(0x77, 0x88, 0x99),
        },
        pressed: StateColors {
            background: Rgba::rgb(0x01, 0x02, 0x03),
            icon: Rgba::rgb(0x04, 0x05, 0x06),
            label: Rgba::rgb(0x07, 0x08, 0x09),
        },
    }
}

/// Rect matching `icon_contrast_tests`': center cell (icon) vs. (0,0)
/// corner cell (background-only, since the icon fixture has a
/// transparent inset margin) are both known-painted, distinguishable
/// samples for this panel/icon fixture pair.
fn rect() -> Rect {
    Rect::new(0, 0, 8, 4)
}

/// `ButtonColors::for_state` returns the exact `StateColors` for each
/// state.
#[test]
fn for_state_returns_matching_state_colors() {
    let colors = custom_colors();
    assert_eq!(colors.for_state(ButtonState::Idle), colors.idle);
    assert_eq!(colors.for_state(ButtonState::Hover), colors.hover);
    assert_eq!(colors.for_state(ButtonState::Pressed), colors.pressed);
}

/// No `.icon()` -> only the background layer paints; the center cell's
/// color must differ once an icon IS composited over that same
/// background, proving `.icon()` gates whether the icon layer paints at
/// all.
#[test]
fn render_paints_icon_layer_only_when_icon_is_set() {
    let rect = rect();

    let without_icon = Button::new(rect, panel_bytes());
    let mut buf_without = make_buf(8, 4);
    without_icon.render(&mut buf_without);

    let with_icon = Button::new(rect, panel_bytes()).icon(icon_bytes());
    let mut buf_with = make_buf(8, 4);
    with_icon.render(&mut buf_with);

    let center_without = buf_without
        .cell((4, 2))
        .expect("center cell must exist")
        .fg;
    let center_with = buf_with.cell((4, 2)).expect("center cell must exist").fg;
    assert_ne!(
        center_without, center_with,
        "center cell color must change once an icon is composited in over the background"
    );
}

/// No `.label()` -> no label glyph is drawn at the label's centered
/// position; `.label("Go")` draws "Go" there — proving `.label()` gates
/// whether the label layer paints at all.
#[test]
fn render_draws_label_only_when_label_is_set() {
    let rect = Rect::new(2, 1, 8, 4);
    let expected_y = rect.y + rect.height / 2;
    let expected_x = rect.x + (rect.width - 2) / 2; // "Go" is 2 chars

    let without_label = Button::new(rect, panel_bytes());
    let mut buf_without = make_buf(16, 8);
    without_label.render(&mut buf_without);
    let cell_without = buf_without
        .cell((expected_x, expected_y))
        .expect("cell must exist");
    assert_ne!(
        cell_without.symbol(),
        "G",
        "no label glyph must be drawn when .label() is unset"
    );

    let with_label = Button::new(rect, panel_bytes()).label("Go");
    let mut buf_with = make_buf(16, 8);
    with_label.render(&mut buf_with);
    let first = buf_with
        .cell((expected_x, expected_y))
        .expect("cell must exist");
    assert_eq!(
        first.symbol(),
        "G",
        "'G' of label 'Go' must be drawn at the centered position when .label() is set"
    );
    let second = buf_with
        .cell((expected_x + 1, expected_y))
        .expect("cell must exist");
    assert_eq!(second.symbol(), "o");
}

/// A `Button` with BOTH `.icon()` and `.label()` set must paint all
/// three layers without panicking: a background-only edge cell, a
/// painted icon-composited center, AND the centered label glyphs.
#[test]
fn render_supports_background_icon_and_label_together() {
    let rect = Rect::new(2, 1, 8, 4);
    let b = Button::new(rect, panel_bytes())
        .icon(icon_bytes())
        .label("Go");
    let mut buf = make_buf(16, 8);
    b.render(&mut buf); // must not panic

    let expected_y = rect.y + rect.height / 2;
    let expected_x = rect.x + (rect.width - 2) / 2;
    let label_cell = buf
        .cell((expected_x, expected_y))
        .expect("label cell must exist");
    assert_eq!(
        label_cell.symbol(),
        "G",
        "label must still be drawn when an icon is also present"
    );

    let corner = buf
        .cell((rect.x, rect.y))
        .expect("corner cell must exist");
    assert_ne!(
        corner.symbol(),
        " ",
        "background must still paint a corner cell when icon+label are both present"
    );
}

/// Driving Idle->Hover->Pressed on a `Button` built with `.icon()` and a
/// custom `.colors()` must recolor BOTH the background layer (sampled at
/// a background-only corner) AND the icon layer (sampled at the icon's
/// center), each to that state's `StateColors` value — all three states
/// pairwise distinct on both samples.
#[test]
fn render_background_and_icon_recolor_per_state_with_custom_colors() {
    let rect = rect();
    let colors = custom_colors();

    let sample = |state: ButtonState| -> (Color, Color) {
        let mut b = Button::new(rect, panel_bytes())
            .icon(icon_bytes())
            .colors(colors);
        set_state(&mut b, state);
        let mut buf = make_buf(8, 4);
        b.render(&mut buf);
        let bg = buf.cell((0, 0)).expect("corner cell must exist").fg;
        let icon = buf.cell((4, 2)).expect("center cell must exist").fg;
        (bg, icon)
    };

    let (idle_bg, idle_icon) = sample(ButtonState::Idle);
    let (hover_bg, hover_icon) = sample(ButtonState::Hover);
    let (pressed_bg, pressed_icon) = sample(ButtonState::Pressed);

    assert_ne!(idle_bg, hover_bg, "background: Idle vs Hover must differ");
    assert_ne!(idle_bg, pressed_bg, "background: Idle vs Pressed must differ");
    assert_ne!(hover_bg, pressed_bg, "background: Hover vs Pressed must differ");

    assert_ne!(idle_icon, hover_icon, "icon: Idle vs Hover must differ");
    assert_ne!(idle_icon, pressed_icon, "icon: Idle vs Pressed must differ");
    assert_ne!(hover_icon, pressed_icon, "icon: Hover vs Pressed must differ");
}

/// Driving Idle->Hover->Pressed on a `Button` built with `.label()` and a
/// custom `.colors()` must recolor the label text itself, pairwise
/// distinct across all three states.
#[test]
fn render_label_recolors_per_state_with_custom_colors() {
    let rect = Rect::new(2, 1, 8, 4);
    let colors = custom_colors();
    let expected_y = rect.y + rect.height / 2;
    let expected_x = rect.x + (rect.width - 2) / 2;

    let sample = |state: ButtonState| -> Color {
        let mut b = Button::new(rect, panel_bytes()).label("Go").colors(colors);
        set_state(&mut b, state);
        let mut buf = make_buf(16, 8);
        b.render(&mut buf);
        buf.cell((expected_x, expected_y))
            .expect("label cell must exist")
            .fg
    };

    let idle = sample(ButtonState::Idle);
    let hover = sample(ButtonState::Hover);
    let pressed = sample(ButtonState::Pressed);

    assert_ne!(idle, hover, "label color: Idle vs Hover must differ");
    assert_ne!(idle, pressed, "label color: Idle vs Pressed must differ");
    assert_ne!(hover, pressed, "label color: Hover vs Pressed must differ");
}
