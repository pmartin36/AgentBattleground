use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::{codecs::gif::GifDecoder, AnimationDecoder, DynamicImage, GenericImageView, RgbaImage};
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

// Lit/unlit decision modes — the thing we're A/B-ing.
#[derive(Clone, Copy)]
enum Thresh {
    AdaptiveGt, // dot lit if luma  > its cell's 8-dot mean (strict — flat cells go fully blank)
    AdaptiveGe, // dot lit if luma >= its cell's 8-dot mean (flat cells fill solid — fewer holes)
    Global,     // dot lit if luma >= the whole-frame mean luma (stable per pixel value)
    Bayer,      // ordered-dither: dot lit if luma >= a fixed Bayer threshold at its pixel position
}
const MODES: [Thresh; 4] = [Thresh::AdaptiveGt, Thresh::AdaptiveGe, Thresh::Global, Thresh::Bayer];
fn mode_name(m: Thresh) -> &'static str {
    match m {
        Thresh::AdaptiveGt => "adaptive >",
        Thresh::AdaptiveGe => "adaptive >=",
        Thresh::Global => "global",
        Thresh::Bayer => "bayer",
    }
}

// 4x4 ordered-dither matrix.
const BAYER: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

// s: fractional bias applied to the AdaptiveGe threshold (luma >= cell_mean * s).
// s = 1.0 reproduces the plain AdaptiveGe behavior; s < 1.0 lights more dots (fills
// holes); s > 1.0 lights fewer (stricter, more contrast). Other modes ignore it.
fn render_braille(img: &DynamicImage, cols: u32, rows: u32, mode: Thresh, s: f32) -> Vec<Line<'static>> {
    let img = img.resize_exact(cols * 2, rows * 4, image::imageops::FilterType::Lanczos3);

    // Global mode: one threshold for the whole frame = mean luma of visible pixels.
    let frame_thresh: u32 = if matches!(mode, Thresh::Global) {
        let (mut sum, mut cnt) = (0u64, 0u64);
        for y in 0..rows * 4 {
            for x in 0..cols * 2 {
                let p = img.get_pixel(x, y).0;
                if p[3] >= 128 { sum += luma(p[0], p[1], p[2]) as u64; cnt += 1; }
            }
        }
        if cnt > 0 { (sum / cnt) as u32 } else { 128 }
    } else { 0 };

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

            let cell_avg = visible.iter().map(|&l| l as u32).sum::<u32>() / visible.len() as u32;

            let mut mask = 0u32;
            for (i, (dx, dy, bit)) in DOTS.iter().enumerate() {
                if let Some(l) = lumas[i] {
                    let lit = match mode {
                        Thresh::AdaptiveGt => l as u32 > cell_avg,
                        Thresh::AdaptiveGe => (l as f32) >= cell_avg as f32 * s,
                        Thresh::Global => l as u32 >= frame_thresh,
                        Thresh::Bayer => {
                            let (ax, ay) = (px + dx, py + dy);
                            let t = (BAYER[(ay % 4) as usize][(ax % 4) as usize] * 2 + 1) * 255 / 32;
                            l as u32 >= t
                        }
                    };
                    if lit { mask |= 1 << bit; }
                }
            }

            // Color = average of visible pixels (same across modes)
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
    braille: Vec<Vec<Line<'static>>>, // one render per Thresh mode (indexed by MODES)
    delay: Duration,
}

struct LoadedGif {
    name: String,
    img_col: u16,
    img_row: u16,
    img_cols: u16,
    img_rows: u16,
    out_cols: u32,
    out_rows: u32,
    cached_s: f32,
    frames: Vec<Frame>,
}

/// Re-renders every frame's braille cache (all modes) for the given `s`. Called lazily
/// whenever the live threshold bias changes — cheap enough at these frame counts (<=49
/// frames/gif) to just redo the whole gif rather than track per-frame dirtiness.
fn recompute_braille(gif: &mut LoadedGif, s: f32) {
    for frame in gif.frames.iter_mut() {
        let img = DynamicImage::ImageRgba8(
            RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).unwrap(),
        );
        frame.braille = MODES.iter()
            .map(|&m| render_braille(&img, gif.out_cols, gif.out_rows, m, s))
            .collect();
    }
    gif.cached_s = s;
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
            braille: MODES.iter().map(|&m| render_braille(&img, out_cols, out_rows, m, 1.0)).collect(),
            delay,
        }
    }).collect();

    LoadedGif {
        name: name.to_string(), img_col, img_row, img_cols, img_rows,
        out_cols, out_rows, cached_s: 1.0, frames,
    }
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
    let mut gifs: Vec<LoadedGif> = GIFS.iter()
        .filter(|(_, path)| std::path::Path::new(path).exists())
        .map(|(name, path)| load_gif(
            path, name,
            right_inner.width as u32, right_inner.height as u32,
            left_inner, cell_w_px, cell_h_px,
        ))
        .collect();

    let mut gif_idx = 0usize;
    let mut frame_idx = 0usize;
    let mut mode_idx = 0usize;
    let mut last_frame = Instant::now();
    let mut last_sent: Option<(usize, usize)> = None;
    let mut front_slot = 0usize;
    let mut has_shown_frame = false;

    // s_steps avoids float drift from repeated +=0.02; s = 1.0 + s_steps * 0.02.
    let mut s_steps: i32 = 0;

    loop {
        let s = 1.0 + s_steps as f32 * 0.02;
        if (gifs[gif_idx].cached_s - s).abs() > 1e-6 {
            recompute_braille(&mut gifs[gif_idx], s);
        }

        let gif = &gifs[gif_idx];
        let frame = &gif.frames[frame_idx];
        let title = format!(
            " {} — thresh: {} (s={:.2}) — frame {}/{} — ←/→ gif · t thresh · [/] bias · q quit ",
            gif.name, mode_name(MODES[mode_idx]), s, frame_idx + 1, gif.frames.len()
        );

        let render_lines = frame.braille[mode_idx].clone();
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
                    KeyCode::Char('t') => {
                        mode_idx = (mode_idx + 1) % MODES.len();
                    }
                    KeyCode::Char('[') => {
                        s_steps = (s_steps - 1).max(-25); // floor s at 0.5
                    }
                    KeyCode::Char(']') => {
                        s_steps = (s_steps + 1).min(25); // ceil s at 1.5
                    }
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
