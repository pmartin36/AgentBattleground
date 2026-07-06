//! Side-by-side comparison of 3 candidate `Q` letterforms, rendered ISOLATED
//! (just the one letter, not part of a word) via the same
//! `draw_banner`/`print_buffer` pipeline every other prototype in this crate
//! uses. Two rounds of text-only description of dot-row layouts have missed
//! what the project owner actually meant, so this prints real candidates for
//! them to eyeball directly instead of a third guess.
//!
//! For reference, `O` at 7 dot-rows tall (rows 0-6) is:
//!   ·##·
//!   #··#
//!   #··#
//!   #··#
//!   #··#
//!   #··#
//!   ·##·
//!   ···· (row 7, blank inter-line spacer every letter shares)

use engine_core::color::Rgba;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use roster_name_banner::braille_font::{draw_banner, print_buffer, GlyphMatrix};

fn matrix(rows: &[&str]) -> GlyphMatrix {
    rows.iter()
        .map(|row| row.chars().map(|ch| ch == '#').collect())
        .collect()
}

/// Variant A: circle shrunk by exactly 1 row vs. `O`'s 7 (6 rows total: top
/// arc + sides + a bottom row) where the bottom 3 of those 6 circle rows
/// (rows 3-5) curve the circle closed, and row 5 (the circle's own last row)
/// is ALSO the tail's first row — the tail is 2 rows total (rows 5-6),
/// overlapping the circle's last row by exactly 1 rather than starting on a
/// fully separate row below it. Row 7 (shared inter-line spacer) untouched.
fn variant_a() -> GlyphMatrix {
    matrix(&[
        "·##··",
        "#··#·",
        "#··#·",
        "#··#·",
        "·##··",
        "·##·#",
        "····#",
        "·····",
    ])
}

/// Variant B: a smaller, rounder circle — only 4 rows total, not trying to
/// match `O`'s proportions at all — plus a tiny nub embedded in the
/// circle's own last row. Simpler than Variant A. Row 7 untouched.
fn variant_b() -> GlyphMatrix {
    matrix(&[
        "·##··",
        "#··#·",
        "#··#·",
        "·##·#",
        "·····",
        "·····",
        "·····",
        "·····",
    ])
}

/// Variant C: circle shrunk by exactly 1 row vs. `O`'s 7 (6 rows: top arc +
/// 4 sides + a STANDARD, non-shared bottom arc — no row doing double duty),
/// then a genuinely separate 2-row tail below it (rows 6-7). The most literal
/// "circle minus 1 row, plus its own separate 2-row tail" reading — this one
/// DOES touch the shared inter-line spacer row (row 7), flagged as a
/// tradeoff for comparison against A/B, which both avoid it.
fn variant_c() -> GlyphMatrix {
    matrix(&[
        "·##··",
        "#··#·",
        "#··#·",
        "#··#·",
        "#··#·",
        "·##··",
        "···##",
        "····#",
    ])
}

fn render(label: &str, m: GlyphMatrix) {
    let w = 14u16;
    let h = 2u16;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            draw_banner(f.buffer_mut(), area, "Q", Rgba::rgb(0xff, 0xff, 0xff), 0, move |_| m.clone());
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    print_buffer(&buf, w, h, label);
}

fn main() {
    render(
        "VARIANT A: shrink-by-1 circle (6 rows), 2-row tail sharing its last row with the circle",
        variant_a(),
    );
    render(
        "VARIANT B: smaller 4-row circle + nub on its own last row (simplest)",
        variant_b(),
    );
    render(
        "VARIANT C: shrink-by-1 circle (6 rows), fully separate 2-row tail below (touches spacer row)",
        variant_c(),
    );
}
