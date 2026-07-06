//! Tests whether terminal-level SGR bold/italic (`ratatui::style::Modifier`)
//! does anything visible to the REGULAR (non-hand-modified) braille dot-font
//! glyphs, as opposed to hand-drawing thicker/sheared dot patterns
//! (`braille_name_bold`/`braille_name_italic`). Most terminal fonts have no
//! distinct bold/italic glyph for braille Unicode code points, so this may
//! render identically to the plain regular variant — that's a real, useful
//! finding either way. `Buffer::set_style` patches the modifier onto every
//! cell in `area` (including untouched/space cells, harmlessly) after the
//! glyphs are drawn, exactly like a caller would apply a `Style` on top of
//! any other ratatui widget.

use engine_core::color::Rgba;
use ratatui::backend::TestBackend;
use ratatui::style::{Modifier, Style};
use ratatui::Terminal;
use roster_name_banner::braille_font::{draw_banner, print_buffer, regular_matrix};

fn main() {
    let names = ["Ember Wolf", "Shadow Cat", "Stone Golem"];
    let w = 60u16;
    let h = 2u16;

    for (label, modifier) in [("SGR BOLD", Modifier::BOLD), ("SGR ITALIC", Modifier::ITALIC)] {
        for name in names {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| {
                    let area = f.area();
                    draw_banner(f.buffer_mut(), area, name, Rgba::rgb(0xff, 0xff, 0xff), 1, regular_matrix);
                    f.buffer_mut().set_style(area, Style::default().add_modifier(modifier));
                })
                .unwrap();
            let buf = terminal.backend().buffer().clone();
            print_buffer(&buf, w, h, &format!("braille dot-font + {label} (regular glyphs, terminal-level modifier): \"{name}\" @ {w}x{h} cells"));
        }
    }
}
