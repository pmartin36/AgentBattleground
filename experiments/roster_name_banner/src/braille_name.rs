//! Third variant: a hand-authored braille DOT-MATRIX font for creature names,
//! using the REAL engine-render low-level pipeline (`DotBuffer`, `Dot::Lit`,
//! `dots_to_grid`) the exact same way `battle_viewer.rs`'s `draw_board_lines`
//! draws grid lines — bypassing `AnimatedSprite`/image-conversion entirely.
//! This is NOT the adaptive-luma photo-conversion path (that one is known to
//! turn "busy detail... to mush" per experiments/creature_lab's findings);
//! here every dot is set directly from a hand-drawn letterform, so the
//! "images don't survive braille downrez" finding doesn't apply.
//!
//! Letterform data + drawing helpers live in `lib.rs`'s `braille_font`
//! module, shared with the bold/italic/SGR-style sibling binaries.

use engine_core::color::Rgba;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use roster_name_banner::braille_font::{draw_banner, print_buffer, regular_matrix};

fn main() {
    let names = ["Ember Wolf", "Shadow Cat", "Stone Golem"];
    // 2 braille-cell rows tall (= 8 dot rows) — same "banner block height" as
    // the hand-rolled 5x7 variant's 7 rows, for a fair side-by-side.
    for name in names {
        for w in [60u16, 80u16] {
            let h = 2u16;
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| {
                    let area = f.area();
                    draw_banner(f.buffer_mut(), area, name, Rgba::rgb(0xff, 0xff, 0xff), 1, regular_matrix);
                })
                .unwrap();
            let buf = terminal.backend().buffer().clone();
            print_buffer(&buf, w, h, &format!("braille dot-font: \"{name}\" @ {w}x{h} cells (={}x{} dots)", w as usize * 2, h as usize * 4));
        }
    }
}
