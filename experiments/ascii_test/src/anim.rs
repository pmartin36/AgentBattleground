use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::{codecs::gif::GifDecoder, AnimationDecoder, DynamicImage, GenericImageView};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{
    fs::File,
    io::{self, BufReader, Write},
    time::{Duration, Instant},
};

const GIFS: &[(&str, &str)] = &[
    ("Pikachu Thunderbolt", "/tmp/pikachu_thunderbolt.gif"),
    ("Pikachu Warm-Up",     "/tmp/pikachu_warmup.gif"),
    ("Barbarian Walk",      "/tmp/barbarian.gif"),
];

// Ping-ponged so a new frame is always drawn before the previous one is deleted —
// deleting first (single id) leaves a blank gap that reads as a flicker.
const IMAGE_IDS: [u32; 2] = [1, 2];

// Braille dot layout in a 2×4 pixel block:
//   (col, row) → bit
const DOTS: [(u32, u32, u8); 8] = [
    (0,0,0),(0,1,1),(0,2,2),
    (1,0,3),(1,1,4),(1,2,5),
    (0,3,6),(1,3,7),
];

fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8
}

fn render_braille(img: &DynamicImage, cols: u32, rows: u32) -> Vec<Line<'static>> {
    let img = img.resize_exact(cols * 2, rows * 4, image::imageops::FilterType::Lanczos3);
    let mut lines = Vec::with_capacity(rows as usize);
    for ty in 0..rows {
        let mut spans = Vec::with_capacity(cols as usize);
        for tx in 0..cols {
            let (px, py) = (tx * 2, ty * 4);
            let pixels: Vec<[u8; 4]> = DOTS.iter()
                .map(|(dx, dy, _)| img.get_pixel(px + dx, py + dy).0)
                .collect();

            let is_transparent = |p: &[u8;4]| p[3] < 128;

            let lumas: Vec<Option<u8>> = pixels.iter()
                .map(|p| if is_transparent(p) { None } else { Some(luma(p[0], p[1], p[2])) })
                .collect();

            let visible: Vec<u8> = lumas.iter().filter_map(|l| *l).collect();
            if visible.is_empty() {
                spans.push(Span::raw(" "));
                continue;
            }

            let avg = visible.iter().map(|&l| l as u32).sum::<u32>() / visible.len() as u32;

            let mut mask = 0u32;
            for (i, (_, _, bit)) in DOTS.iter().enumerate() {
                if let Some(l) = lumas[i] {
                    if l as u32 > avg { mask |= 1 << bit; }
                }
            }

            // Color = average of visible pixels
            let vis_pix: Vec<&[u8;4]> = pixels.iter().enumerate()
                .filter(|(i,_)| lumas[*i].is_some()).map(|(_,p)| p).collect();
            let n = vis_pix.len() as u32;
            let (r,g,b) = vis_pix.iter().fold((0u32,0u32,0u32), |(a,b2,c),p|
                (a+p[0] as u32, b2+p[1] as u32, c+p[2] as u32));

            let ch = char::from_u32(0x2800 + mask).unwrap_or(' ');
            spans.push(Span::styled(
                ch.to_string(),
                Style::default().fg(Color::Rgb((r/n) as u8, (g/n) as u8, (b/n) as u8)),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(B64[(n >> 18 & 0x3F) as usize]);
        out.push(B64[(n >> 12 & 0x3F) as usize]);
        out.push(if chunk.len() > 1 { B64[(n >> 6 & 0x3F) as usize] } else { b'=' });
        out.push(if chunk.len() > 2 { B64[(n & 0x3F) as usize] } else { b'=' });
    }
    out
}

/// Transmits and displays one raw RGBA frame under `image_id` via the Kitty graphics
/// protocol. Does NOT delete any previous image — caller handles that (see ping-pong
/// buffering in the main loop) so the new frame is on screen before the old is gone.
fn transmit_kitty_frame(
    out: &mut impl Write,
    image_id: u32,
    rgba: &[u8],
    src_w: u32,
    src_h: u32,
    cell_col: u16,
    cell_row: u16,
    cell_cols: u16,
    cell_rows: u16,
) -> io::Result<()> {
    // Move cursor to the top-left cell where the image should be drawn (1-indexed CUP).
    write!(out, "\x1b[{};{}H", cell_row + 1, cell_col + 1)?;

    let payload = base64_encode(rgba);
    let total = payload.len();
    let chunk_size = 4096;
    let mut offset = 0;
    let mut first = true;
    while offset < total {
        let end = (offset + chunk_size).min(total);
        let chunk = &payload[offset..end];
        let more = if end < total { 1 } else { 0 };
        if first {
            write!(
                out,
                "\x1b_Ga=T,i={},f=32,t=d,s={},v={},c={},r={},q=2,m={};",
                image_id, src_w, src_h, cell_cols, cell_rows, more
            )?;
            first = false;
        } else {
            write!(out, "\x1b_Gm={};", more)?;
        }
        out.write_all(chunk)?;
        write!(out, "\x1b\\")?;
        offset = end;
    }
    out.flush()
}

fn delete_kitty_image(out: &mut impl Write, image_id: u32) -> io::Result<()> {
    write!(out, "\x1b_Ga=d,d=I,i={},q=2\x1b\\", image_id)?;
    out.flush()
}

struct Frame {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    braille: Vec<Line<'static>>,
    delay: Duration,
}

struct LoadedGif {
    name: String,
    img_col: u16,
    img_row: u16,
    img_cols: u16,
    img_rows: u16,
    frames: Vec<Frame>,
}

fn load_gif(
    path: &str,
    name: &str,
    braille_cols: u32,
    braille_rows: u32,
    left_inner: Rect,
    cell_w_px: f32,
    cell_h_px: f32,
) -> LoadedGif {
    let f = BufReader::new(File::open(path).expect("gif not found"));
    let raw_frames: Vec<_> = GifDecoder::new(f).unwrap()
        .into_frames().collect_frames().unwrap();

    let first = DynamicImage::ImageRgba8(raw_frames[0].buffer().clone());
    let src_w = first.width() as f32;
    let src_h = first.height() as f32;

    // Aspect-correct braille pane size (dots are 2x4 per cell).
    let avail_w = braille_cols as f32 * 2.0;
    let avail_h = braille_rows as f32 * 4.0;
    let scale = (avail_w / src_w).min(avail_h / src_h);
    let out_cols = ((src_w * scale) / 2.0) as u32;
    let out_rows = ((src_h * scale) / 4.0) as u32;

    // Aspect-correct Kitty image pane size, using the terminal's real cell pixel size
    // so the true-color pane isn't stretched relative to the braille pane.
    let avail_w_px = left_inner.width as f32 * cell_w_px;
    let avail_h_px = left_inner.height as f32 * cell_h_px;
    let img_scale = (avail_w_px / src_w).min(avail_h_px / src_h);
    let img_cols = (((src_w * img_scale) / cell_w_px).max(1.0) as u16).min(left_inner.width.max(1));
    let img_rows = (((src_h * img_scale) / cell_h_px).max(1.0) as u16).min(left_inner.height.max(1));
    let img_col = left_inner.x + (left_inner.width.saturating_sub(img_cols)) / 2;
    let img_row = left_inner.y + (left_inner.height.saturating_sub(img_rows)) / 2;

    let frames = raw_frames.iter().map(|frame| {
        let d = frame.delay().numer_denom_ms();
        let ms = (d.0 / d.1).max(16); // floor at ~60fps
        let delay = Duration::from_millis(ms as u64);
        let buf = frame.buffer();
        let (width, height) = buf.dimensions();
        let img = DynamicImage::ImageRgba8(buf.clone());
        Frame {
            rgba: buf.clone().into_raw(),
            width,
            height,
            braille: render_braille(&img, out_cols, out_rows),
            delay,
        }
    }).collect();

    LoadedGif { name: name.to_string(), img_col, img_row, img_cols, img_rows, frames }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut raw_out = io::stdout();

    let size = terminal.size()?;
    let full_area = Rect::new(0, 0, size.width, size.height);

    // Measure real cell pixel dimensions so the Kitty image pane isn't distorted.
    let (cell_w_px, cell_h_px) = match crossterm::terminal::window_size() {
        Ok(w) if w.width > 0 && w.height > 0 && w.columns > 0 && w.rows > 0 => {
            (w.width as f32 / w.columns as f32, w.height as f32 / w.rows as f32)
        }
        _ => (8.0, 16.0), // fallback: typical monospace cell aspect
    };

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(full_area);
    let left_inner = Block::default().borders(Borders::ALL).inner(panes[0]);
    let right_inner = Block::default().borders(Borders::ALL).inner(panes[1]);

    eprintln!("Loading GIFs...");
    let gifs: Vec<LoadedGif> = GIFS.iter()
        .filter(|(_, path)| std::path::Path::new(path).exists())
        .map(|(name, path)| load_gif(
            path, name,
            right_inner.width as u32, right_inner.height as u32,
            left_inner, cell_w_px, cell_h_px,
        ))
        .collect();

    let mut gif_idx = 0usize;
    let mut frame_idx = 0usize;
    let mut last_frame = Instant::now();
    let mut last_sent: Option<(usize, usize)> = None;
    let mut front_slot = 0usize;
    let mut has_shown_frame = false;

    loop {
        let gif = &gifs[gif_idx];
        let frame = &gif.frames[frame_idx];
        let title = format!(
            " {} — frame {}/{} — ←/→ switch GIF — q quit ",
            gif.name, frame_idx + 1, gif.frames.len()
        );

        let render_lines = frame.braille.clone();
        terminal.draw(|f| {
            let area = f.area();
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);

            let left_block = Block::default().borders(Borders::ALL).title(" ORIGINAL ");
            f.render_widget(left_block, panes[0]);

            let right_block = Block::default().borders(Borders::ALL).title(title.as_str());
            let inner = right_block.inner(panes[1]);
            f.render_widget(right_block, panes[1]);

            // Paragraph only centers horizontally, so center vertically by hand to
            // match the image pane's vertical centering.
            let content_rows = (render_lines.len() as u16).min(inner.height);
            let y_offset = (inner.height.saturating_sub(content_rows)) / 2;
            let centered = Rect {
                x: inner.x,
                y: inner.y + y_offset,
                width: inner.width,
                height: content_rows,
            };
            f.render_widget(
                Paragraph::new(render_lines).alignment(Alignment::Center),
                centered,
            );
        })?;

        let key = (gif_idx, frame_idx);
        if last_sent != Some(key) {
            let back_slot = 1 - front_slot;
            let back_id = IMAGE_IDS[back_slot];
            transmit_kitty_frame(
                &mut raw_out,
                back_id,
                &frame.rgba,
                frame.width,
                frame.height,
                gif.img_col,
                gif.img_row,
                gif.img_cols,
                gif.img_rows,
            )?;
            if has_shown_frame {
                delete_kitty_image(&mut raw_out, IMAGE_IDS[front_slot])?;
            }
            has_shown_frame = true;
            front_slot = back_slot;
            last_sent = Some(key);
        }

        if last_frame.elapsed() >= frame.delay {
            frame_idx = (frame_idx + 1) % gif.frames.len();
            last_frame = Instant::now();
        }

        if event::poll(Duration::from_millis(8))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Right => {
                        gif_idx = (gif_idx + 1) % gifs.len();
                        frame_idx = 0;
                        last_frame = Instant::now();
                    }
                    KeyCode::Left => {
                        gif_idx = (gif_idx + gifs.len() - 1) % gifs.len();
                        frame_idx = 0;
                        last_frame = Instant::now();
                    }
                    _ => {}
                }
            }
        }
    }

    write!(raw_out, "\x1b_Ga=d,d=A,q=2\x1b\\")?;
    raw_out.flush()?;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
