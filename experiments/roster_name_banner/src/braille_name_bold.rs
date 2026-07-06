//! Bold variant of the braille dot-font (`braille_name.rs`): thickens every
//! stroke by smearing each lit dot one column right (`braille_font::bold_matrix`
//! in lib.rs), same sample names/widths as the regular variant for direct
//! comparison.

use engine_core::color::Rgba;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use roster_name_banner::braille_font::{bold_matrix, draw_banner, print_buffer};

fn main() {
    let names = ["Ember Wolf", "Shadow Cat", "Stone Golem"];
    for name in names {
        for w in [60u16, 80u16] {
            let h = 2u16;
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| {
                    let area = f.area();
                    draw_banner(f.buffer_mut(), area, name, Rgba::rgb(0xff, 0xff, 0xff), 1, bold_matrix);
                })
                .unwrap();
            let buf = terminal.backend().buffer().clone();
            print_buffer(&buf, w, h, &format!("braille dot-font BOLD: \"{name}\" @ {w}x{h} cells (={}x{} dots)", w as usize * 2, h as usize * 4));
        }
    }
}
