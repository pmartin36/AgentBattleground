//! Full A-Z alphabet reference sheet for the braille dot-font, printed once
//! per style (regular, bold, italic, bold+italic) in sequence — lets the
//! project owner eyeball every letter (not just the ones needed for the 3
//! sample creature names) for each style before it gets locked into spec 35.
//!
//! Letters are split into 3 rows (A-I, J-R, S-Z) and joined with single
//! space characters so adjacent letters get a clearly visible gap (the
//! font's narrow 2-dot space glyph plus the 1-dot inter-glyph gap on each
//! side = 4 blank dot-columns between letters, vs. 1 within a real word).

use engine_core::color::Rgba;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use roster_name_banner::braille_font::{
    bold_italic_matrix, bold_matrix, draw_banner, italic_matrix, print_buffer, regular_matrix,
    GlyphMatrix,
};

const ROWS: [&str; 3] = ["A B C D E F G H I", "J K L M N O P Q R", "S T U V W X Y Z"];

fn render_style(style_label: &str, matrix_of: impl Fn(char) -> GlyphMatrix + Copy) {
    println!("======================================================================");
    println!("STYLE: {style_label}");
    println!("======================================================================\n");
    for (i, row) in ROWS.iter().enumerate() {
        let w = 72u16;
        let h = 2u16;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                draw_banner(f.buffer_mut(), area, row, Rgba::rgb(0xff, 0xff, 0xff), 1, matrix_of);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        print_buffer(&buf, w, h, &format!("{style_label} row {} : \"{row}\"", i + 1));
    }
}

fn main() {
    render_style("REGULAR", regular_matrix);
    render_style("BOLD", bold_matrix);
    render_style("ITALIC", italic_matrix);
    render_style("BOLD+ITALIC", bold_italic_matrix);
}
