//! Throwaway prototype (NOT part of the real workspace) comparing two ways
//! to render a creature name in the Roster screen:
//!   1. Today's `engine_render::label` — single centered text row.
//!   2. A hand-rolled 5x7 block-letter "banner" font (figlet/toilet style),
//!      using plain block characters (NOT braille — banner text is not a
//!      sprite/UI-chrome element under spec 13's braille-only rule; it is
//!      arguably still "text", so this prototype treats it as the toolkit's
//!      generalized answer to "can text look bigger").
//!
//! Run: `cargo run` from this directory. Prints ASCII grids to stdout.

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::Terminal;

// ---------------------------------------------------------------------
// 1. Baseline: faithful copy of engine_render::label's exact algorithm
//    (crates/engine/render/src/lib.rs), so the comparison is apples-to-apples.
// ---------------------------------------------------------------------
fn label(buf: &mut Buffer, area: Rect, text: &str, color: Color) {
    let inter = area.intersection(buf.area);
    if inter.is_empty() {
        return;
    }
    let text_w = text.chars().count() as u16;
    let y = inter.top() + inter.height / 2;
    let x = inter.left() + inter.width.saturating_sub(text_w) / 2;
    let max_width = inter.right().saturating_sub(x) as usize;
    let style = Style::default().fg(color);
    buf.set_stringn(x, y, text, max_width, style);
}

// ---------------------------------------------------------------------
// 2. Hand-rolled 5-wide x 7-tall block font, just the glyphs needed to
//    spell "EMBER WOLF", "SHADOW CAT", "STONE GOLEM".
//    '#' = filled cell (rendered as '█'), '.' = empty.
// ---------------------------------------------------------------------
fn glyph(c: char) -> [&'static str; 7] {
    match c {
        'A' => [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'B' => ["####.", "#...#", "#...#", "####.", "#...#", "#...#", "####."],
        'C' => [".####", "#....", "#....", "#....", "#....", "#....", ".####"],
        'D' => ["####.", "#...#", "#...#", "#...#", "#...#", "#...#", "####."],
        'E' => ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
        'F' => ["#####", "#....", "#....", "####.", "#....", "#....", "#...."],
        'G' => [".####", "#....", "#....", "#.###", "#...#", "#...#", ".####"],
        'H' => ["#...#", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'L' => ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
        'M' => ["#...#", "##.##", "#.#.#", "#...#", "#...#", "#...#", "#...#"],
        'N' => ["#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#"],
        'O' => [".###.", "#...#", "#...#", "#...#", "#...#", "#...#", ".###."],
        'R' => ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
        'S' => [".####", "#....", "#....", ".###.", "....#", "....#", "####."],
        'T' => ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
        'W' => ["#...#", "#...#", "#...#", "#.#.#", "#.#.#", "##.##", "#...#"],
        _ => [".....", ".....", ".....", ".....", ".....", ".....", "....."], // space / unknown
    }
}

/// Width in columns of one glyph cell (5) plus 1 column of inter-letter gap.
const GLYPH_W: u16 = 5;
const GLYPH_GAP: u16 = 1;
const GLYPH_H: u16 = 7;

/// Draws `text` (uppercased) as the 5x7 banner font, centered in `area`.
fn banner(buf: &mut Buffer, area: Rect, text: &str, color: Color) {
    let upper: Vec<char> = text.chars().map(|c| c.to_ascii_uppercase()).collect();
    let total_w: u16 = upper
        .iter()
        .map(|_| GLYPH_W + GLYPH_GAP)
        .sum::<u16>()
        .saturating_sub(GLYPH_GAP);
    let start_x = area.x + area.width.saturating_sub(total_w) / 2;
    let start_y = area.y + area.height.saturating_sub(GLYPH_H) / 2;
    let style = Style::default().fg(color);

    let mut cursor_x = start_x;
    for &c in &upper {
        let rows = glyph(c);
        for (dy, row) in rows.iter().enumerate() {
            for (dx, cell) in row.chars().enumerate() {
                if cell == '#' {
                    let x = cursor_x + dx as u16;
                    let y = start_y + dy as u16;
                    if let Some(bufcell) = buf.cell_mut((x, y)) {
                        bufcell.set_char('█');
                        bufcell.set_style(style);
                    }
                }
            }
        }
        cursor_x += GLYPH_W + GLYPH_GAP;
    }
}

fn print_buffer(buf: &Buffer, w: u16, h: u16, title: &str) {
    println!("--- {title} ({w}x{h}) ---");
    println!("+{}+", "-".repeat(w as usize));
    for y in 0..h {
        let row: String = (0..w)
            .map(|x| buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" ").to_string())
            .collect();
        println!("|{row}|");
    }
    println!("+{}+", "-".repeat(w as usize));
    println!();
}

fn render_plain(name: &str, w: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, 1)).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            label(f.buffer_mut(), area, name, Color::White);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn render_banner(name: &str, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let area = f.area();
            banner(f.buffer_mut(), area, name, Color::White);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn main() {
    let names = ["Ember Wolf", "Shadow Cat", "Stone Golem"];

    println!("========================================");
    println!("1. PLAIN LABEL BASELINE (today's approach)");
    println!("========================================\n");
    for name in names {
        for w in [60u16, 100u16] {
            let buf = render_plain(name, w);
            print_buffer(&buf, w, 1, &format!("plain label: \"{name}\" @ {w} cols"));
        }
    }

    println!("========================================");
    println!("2. BANNER TEXT (hand-rolled 5x7 block font)");
    println!("========================================\n");
    for name in names {
        // 60 and 80 col widths, 7-row-tall banner (matches GLYPH_H exactly,
        // no extra padding row).
        for w in [60u16, 80u16] {
            let buf = render_banner(name, w, GLYPH_H);
            print_buffer(&buf, w, GLYPH_H, &format!("banner: \"{name}\" @ {w}x{GLYPH_H}"));
        }
    }
}
