//! Quick check: does the `tui-big-text` crate (a figlet-style widget already
//! on crates.io, not in this workspace) render usably at a compact size?
use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style};
use ratatui_core::widgets::Widget;
use tui_big_text::{BigTextBuilder, PixelSize};

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

fn main() {
    for (size_name, size) in [("Full (8px font)", PixelSize::Full), ("Quadrant (half-block)", PixelSize::Quadrant)] {
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        let big = BigTextBuilder::default()
            .pixel_size(size)
            .style(Style::default().fg(Color::White))
            .lines(vec!["Ember Wolf".into()])
            .build();
        big.render(area, &mut buf);
        print_buffer(&buf, 80, 10, &format!("tui-big-text {size_name}: \"Ember Wolf\""));
    }
}
