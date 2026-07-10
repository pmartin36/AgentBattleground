//! Durable, API-agnostic scenario module for the button-widget-unification
//! lossless-migration gate (b1-t1 captures against it; b5-t1 verifies
//! against it). References ONLY stable public `engine_render` API
//! (`ButtonCore`, `ButtonState`, `decode_braille_cell`) — never `Button`
//! directly — so it keeps compiling across every later task.
//!
//! `#[path]`-included as a module (not compiled as its own test binary —
//! only files directly under `tests/`, not in a subdirectory, become
//! separate integration-test binaries).

#![allow(dead_code)]

use std::ops::DerefMut;

pub use engine_render::ButtonState;
use engine_render::ButtonCore;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Color;

// ── shared scenario inputs ───────────────────────────────────────────────

/// Matches roster's icon-button size class (icon `Button` call sites).
pub const ICON_RECT: Rect = Rect {
    x: 2,
    y: 1,
    width: 8,
    height: 4,
};

/// Matches battle-menu's `BUTTON_W`/`BUTTON_H` (20x3) text-button size class.
pub const TEXT_RECT: Rect = Rect {
    x: 2,
    y: 1,
    width: 20,
    height: 3,
};

/// The spec's proof-consumer label string (battle-menu's "Finish Battle").
pub const LABEL: &str = "Finish Battle";

pub const STATES: [ButtonState; 3] = [
    ButtonState::Idle,
    ButtonState::Hover,
    ButtonState::Pressed,
];

/// Human-readable tag for a state, used to build fixture file names.
pub fn state_tag(state: ButtonState) -> &'static str {
    match state {
        ButtonState::Idle => "idle",
        ButtonState::Hover => "hover",
        ButtonState::Pressed => "pressed",
    }
}

// ── synthetic asset bytes ────────────────────────────────────────────────
//
// Re-implemented from scratch (not imported) — `button_tests.rs`'s
// `rounded_rect_png`/`hollow_ring_png`/`inset_blob_png` are `#[cfg(test)]`
// -private to the lib and unreachable from an integration test, same reason
// `tests/button_shared_cache.rs` already re-built equivalents. Geometry
// copied verbatim from `button_tests.rs`.

fn encode_png(img: image::RgbaImage) -> Vec<u8> {
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("synthetic test fixture must encode to PNG");
    buf
}

/// Opaque-white rounded-rect on a transparent ground — stand-in for
/// `BUTTON_PANEL`. Mirrors `button_tests.rs::rounded_rect_png`.
fn rounded_rect_png(w: u32, h: u32, r: i32) -> Vec<u8> {
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    let x_lo = r;
    let x_hi = w as i32 - 1 - r;
    let y_lo = r;
    let y_hi = h as i32 - 1 - r;
    for y in 0..h {
        for x in 0..w {
            let (xi, yi) = (x as i32, y as i32);
            let cx = xi.clamp(x_lo, x_hi);
            let cy = yi.clamp(y_lo, y_hi);
            let (dx, dy) = (xi - cx, yi - cy);
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
    }
    encode_png(img)
}

/// Hollow opaque ring on a transparent ground (opaque border, transparent
/// interior AND corners) — stand-in for `FRAME_PANEL`. Mirrors
/// `button_tests.rs::hollow_ring_png`. The frame background MUST use this
/// hollow generator (not a solid rect) so the lossless-render oracle
/// captures the real alpha structure (research.md point 2).
fn hollow_ring_png(w: u32, h: u32, r: i32, border: i32) -> Vec<u8> {
    let ri = r - border;
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    let x_lo = r;
    let x_hi = w as i32 - 1 - r;
    let y_lo = r;
    let y_hi = h as i32 - 1 - r;
    for y in 0..h {
        for x in 0..w {
            let (xi, yi) = (x as i32, y as i32);
            let cx = xi.clamp(x_lo, x_hi);
            let cy = yi.clamp(y_lo, y_hi);
            let (dx, dy) = (xi - cx, yi - cy);
            let dist_sq = dx * dx + dy * dy;
            if dist_sq <= r * r && dist_sq > ri * ri {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
    }
    encode_png(img)
}

/// Opaque-white silhouette blob inset from every edge — stand-in for a
/// bundled icon. Mirrors `button_tests.rs::inset_blob_png`.
fn inset_blob_png(w: u32, h: u32, inset: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    for y in inset..(h - inset) {
        for x in inset..(w - inset) {
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    encode_png(img)
}

fn leak_png(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

/// Synthetic stand-in for `game::assets::BUTTON_PANEL` (64x32, radius 8).
pub fn panel_bytes() -> &'static [u8] {
    leak_png(rounded_rect_png(64, 32, 8))
}

/// Synthetic stand-in for a bundled icon (48x48, inset 12).
pub fn icon_bytes() -> &'static [u8] {
    leak_png(inset_blob_png(48, 48, 12))
}

/// Synthetic stand-in for `game::assets::FRAME_PANEL` (64x32, radius 8,
/// border 6 — hollow ring, opaque border, transparent interior).
pub fn frame_bytes() -> &'static [u8] {
    leak_png(hollow_ring_png(64, 32, 8, 6))
}

// ── state driver ─────────────────────────────────────────────────────────

fn ev(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    }
}

/// Drive any `ButtonCore`-backed widget (reached via `DerefMut`) to `state`
/// via the standard mouse sequence. Copy of `button_tests.rs`'s `set_state`.
pub fn set_state<B: DerefMut<Target = ButtonCore>>(b: &mut B, state: ButtonState) {
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
    assert_eq!(b.state(), state, "scenario state-drive must reach the target state");
}

// ── buffer + braille serialize helpers ───────────────────────────────────
//
// Braille-only, per `crates/game/src/scenes/test_util.rs:158-234` — the same
// text format so `diff_dots` and any human diff read the same shape. Text
// labels (plain terminal chars, not braille) are NOT captured here — see
// research.md point 3.

pub fn make_buf(w: u16, h: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, w, h))
}

pub fn serialize_braille_buffer(buf: &Buffer) -> String {
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

pub fn deserialize_braille_buffer(s: &str) -> Buffer {
    let mut lines = s.lines();
    let header = lines.next().expect("serialized buffer must have a dims header");
    let mut parts = header.split_whitespace();
    let tag = parts.next().expect("dims header must start with 'dims'");
    assert_eq!(tag, "dims", "expected 'dims' header, got {tag:?}");
    let w: u16 = parts
        .next()
        .expect("dims header missing width")
        .parse()
        .expect("width must be u16");
    let h: u16 = parts
        .next()
        .expect("dims header missing height")
        .parse()
        .expect("height must be u16");

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

/// Full per-cell symbol grid of `buf` — the human-eyeball braille-art
/// preview artifact (manual render-confirmation, spec line 96).
pub fn buffer_to_art(buf: &Buffer) -> String {
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

pub fn fixture_path(name: &str) -> String {
    format!(
        "{}/tests/fixtures/button_migration/{name}.fixture",
        env!("CARGO_MANIFEST_DIR")
    )
}

pub fn load_fixture(name: &str) -> Buffer {
    let path = fixture_path(name);
    let s = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"));
    deserialize_braille_buffer(&s)
}
