//! Bold+italic combined variant of the braille dot-font: composes the bold
//! stroke-thickening transform with the italic shear on the same letterform
//! (`braille_font::bold_italic_matrix` in lib.rs — bold first, then shear).
//! Same sample names/widths as `braille_name_bold`/`braille_name_italic` for
//! direct comparison against bold-alone and italic-alone.

use engine_core::color::Rgba;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use roster_name_banner::braille_font::{bold_italic_matrix, draw_banner, print_buffer};

fn main() {
    let names = ["Ember Wolf", "Shadow Cat", "Stone Golem"];
    for name in names {
        for w in [60u16, 80u16] {
            let h = 2u16;
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| {
                    let area = f.area();
                    draw_banner(f.buffer_mut(), area, name, Rgba::rgb(0xff, 0xff, 0xff), 1, bold_italic_matrix);
                })
                .unwrap();
            let buf = terminal.backend().buffer().clone();
            print_buffer(&buf, w, h, &format!("braille dot-font BOLD+ITALIC: \"{name}\" @ {w}x{h} cells (={}x{} dots)", w as usize * 2, h as usize * 4));
        }
    }
}
