use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::{DynamicImage, GenericImageView, Pixel};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::io;

#[derive(Clone, Copy)]
enum RightMode {
    HalfBlock,
    Posterized(u8),
    Braille,
}

const ASCII_RAMP: &[u8] = b"$@B%8&WM#*oahkbdpqwmZO0QLCJUYXzcvunxrjft/\\|()1{}[]?-_+~<>i!lI;:,\"^`'. ";

fn luma_to_ascii(luma: u8) -> char {
    let idx = (luma as usize * (ASCII_RAMP.len() - 1)) / 255;
    ASCII_RAMP[idx] as char
}

// Each terminal cell is ~2:1 (h:w), so halve rows for correct aspect ratio.
fn ascii_lines(img: &DynamicImage, w: u32, rows: u32) -> Vec<Line<'static>> {
    let img = img.resize_exact(w, rows, image::imageops::FilterType::Lanczos3);
    let mut lines = Vec::new();
    for y in 0..rows {
        let mut spans = Vec::new();
        for x in 0..w {
            let [r, g, b, _] = img.get_pixel(x, y).to_rgba().0;
            let luma = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8;
            spans.push(Span::styled(
                luma_to_ascii(luma).to_string(),
                Style::default().fg(Color::Rgb(r, g, b)),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn snap(v: u8, levels: u8) -> u8 {
    let step = 255.0 / (levels - 1) as f32;
    ((v as f32 / step).round() * step).min(255.0) as u8
}

// ▀ with fg=top pixel, bg=bottom pixel — each terminal row covers 2 image rows.
// levels=255 means no posterization; lower values snap colors to a smaller palette.
fn halfblock_lines(img: &DynamicImage, w: u32, rows: u32, levels: u8) -> Vec<Line<'static>> {
    let img = img.resize_exact(w, rows * 2, image::imageops::FilterType::Lanczos3);
    let mut lines = Vec::new();
    for row in 0..rows {
        let mut spans = Vec::new();
        for x in 0..w {
            let [tr, tg, tb, _] = img.get_pixel(x, row * 2).to_rgba().0;
            let [br, bg, bb, _] = img.get_pixel(x, row * 2 + 1).to_rgba().0;
            let (tr, tg, tb) = (snap(tr, levels), snap(tg, levels), snap(tb, levels));
            let (br, bg, bb) = (snap(br, levels), snap(bg, levels), snap(bb, levels));
            spans.push(Span::styled(
                "▀".to_string(),
                Style::default()
                    .fg(Color::Rgb(tr, tg, tb))
                    .bg(Color::Rgb(br, bg, bb)),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

// Braille dot layout within a 2×4 pixel block:
//   col 0, rows 0-2 → bits 0,1,2   col 1, rows 0-2 → bits 3,4,5
//   col 0, row 3    → bit 6         col 1, row 3    → bit 7
// Base codepoint U+2800; add bitmask to select dots.
// fg = average color of lit dots; aspect ratio is naturally square per dot.
fn braille_lines(img: &DynamicImage, w: u32, rows: u32) -> Vec<Line<'static>> {
    let img = img.resize_exact(w * 2, rows * 4, image::imageops::FilterType::Lanczos3);
    const DOTS: [(u32, u32, u8); 8] = [
        (0,0,0),(0,1,1),(0,2,2),
        (1,0,3),(1,1,4),(1,2,5),
        (0,3,6),(1,3,7),
    ];
    let mut lines = Vec::new();
    for ty in 0..rows {
        let mut spans = Vec::new();
        for tx in 0..w {
            let (px, py) = (tx * 2, ty * 4);
            let pixels: Vec<[u8; 4]> = DOTS.iter()
                .map(|(dx, dy, _)| img.get_pixel(px + dx, py + dy).to_rgba().0)
                .collect();
            let lumas: Vec<u8> = pixels.iter()
                .map(|[r,g,b,_]| (0.299 * *r as f32 + 0.587 * *g as f32 + 0.114 * *b as f32) as u8)
                .collect();
            let avg = lumas.iter().map(|&l| l as u32).sum::<u32>() / 8;
            let mut mask = 0u32;
            for (i, (_, _, bit)) in DOTS.iter().enumerate() {
                if lumas[i] as u32 > avg { mask |= 1 << bit; }
            }
            // Color from lit dots (fall back to all dots if none lit)
            let lit: Vec<&[u8;4]> = pixels.iter().enumerate()
                .filter(|(i,_)| lumas[*i] as u32 > avg).map(|(_,p)| p).collect();
            let src = if lit.is_empty() { pixels.iter().collect::<Vec<_>>() } else { lit };
            let n = src.len() as u32;
            let (r,g,b) = src.iter().fold((0u32,0u32,0u32), |(a,b2,c),p| (a+p[0] as u32, b2+p[1] as u32, c+p[2] as u32));
            let ch = char::from_u32(0x2800 + mask).unwrap_or('?');
            spans.push(Span::styled(ch.to_string(), Style::default().fg(Color::Rgb((r/n) as u8,(g/n) as u8,(b/n) as u8))));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let size = terminal.size()?;
    let panel_w = (size.width / 2).saturating_sub(1) as u32;
    let inner_h = size.height.saturating_sub(2) as u32; // subtract block borders

    let img = image::open("/tmp/lenna.png").expect("failed to open image");
    let face = img.crop_imm(150, 80, 230, 230);

    let ascii = ascii_lines(&face, panel_w, inner_h / 2);
    let raw_blocks = halfblock_lines(&face, panel_w, inner_h, 255);
    let post6 = halfblock_lines(&face, panel_w, inner_h, 6);
    let braille = braille_lines(&face, panel_w, inner_h);

    let modes: &[(RightMode, &str)] = &[
        (RightMode::Braille,       " Braille (2×4 dots/cell) — Space to cycle "),
        (RightMode::HalfBlock,     " Half-block (raw) — Space to cycle "),
        (RightMode::Posterized(6), " Half-block posterized — Space to cycle "),
    ];
    let mut mode_idx = 0usize;

    loop {
        let (mode, right_title) = modes[mode_idx];
        let right_lines = match mode {
            RightMode::Braille        => braille.clone(),
            RightMode::HalfBlock      => raw_blocks.clone(),
            RightMode::Posterized(_)  => post6.clone(),
        };

        terminal.draw(|f| {
            let area = f.area();
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);

            let ascii_block = Block::default()
                .title(" ASCII + color (q quit) ")
                .borders(Borders::ALL);
            let half_block = Block::default()
                .title(right_title)
                .borders(Borders::ALL);

            let ascii_inner = ascii_block.inner(chunks[0]);
            let half_inner = half_block.inner(chunks[1]);

            f.render_widget(ascii_block, chunks[0]);
            f.render_widget(half_block, chunks[1]);
            f.render_widget(Paragraph::new(ascii.clone()), ascii_inner);
            f.render_widget(Paragraph::new(right_lines), half_inner);
        })?;

        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char(' ') => mode_idx = (mode_idx + 1) % modes.len(),
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
