use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::{codecs::gif::GifDecoder, AnimationDecoder, DynamicImage, GenericImageView};
use ratatui::{
    backend::CrosstermBackend,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};
use std::{
    fs::File,
    io::{self, BufReader},
    time::{Duration, Instant},
};

const GIF_PATH: &str = "/tmp/barbarian.gif";

const DOTS: [(u32, u32, u8); 8] = [
    (0,0,0),(0,1,1),(0,2,2),
    (1,0,3),(1,1,4),(1,2,5),
    (0,3,6),(1,3,7),
];

// A rendered braille cell: the char + its color. None = transparent (show background).
type Cell = Option<(char, Color)>;
type Grid = Vec<Vec<Cell>>; // [row][col]

fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8
}

// 20% chance to nudge a cell up or down one "level" (±1 lit dot).
// Driven by the passed RNG so the result is baked into the frame, not re-rolled per wall-clock frame.
fn apply_noise(mask: u32, rng: &mut Rng) -> u32 {
    if rng.f32() >= 0.20 { return mask; }
    let lit: Vec<u8> = (0..8).filter(|b| mask & (1 << b) != 0).collect();
    let unlit: Vec<u8> = (0..8).filter(|b| mask & (1 << b) == 0).collect();
    let up = rng.f32() < 0.5;
    if up && !unlit.is_empty() {
        mask | (1 << unlit[(rng.f32() * unlit.len() as f32) as usize])
    } else if !up && !lit.is_empty() {
        mask & !(1 << lit[(rng.f32() * lit.len() as f32) as usize])
    } else {
        mask
    }
}

// Render an image to a grid of braille cells, honoring alpha. `bright` dims color (depth cue).
// `noise_seed` Some => bake per-cell ±1-dot grain seeded deterministically (stable per frame).
fn render_grid(img: &DynamicImage, cols: u32, rows: u32, bright: f32, noise_seed: Option<u32>) -> Grid {
    let img = img.resize_exact(cols * 2, rows * 4, image::imageops::FilterType::Lanczos3);
    let mut grid = vec![vec![None; cols as usize]; rows as usize];
    let mut rng = Rng(noise_seed.unwrap_or(1).max(1));
    for ty in 0..rows {
        for tx in 0..cols {
            let (px, py) = (tx * 2, ty * 4);
            let pixels: Vec<[u8; 4]> = DOTS.iter()
                .map(|(dx, dy, _)| img.get_pixel(px + dx, py + dy).0)
                .collect();
            let lumas: Vec<Option<u8>> = pixels.iter()
                .map(|p| if p[3] < 128 { None } else { Some(luma(p[0], p[1], p[2])) })
                .collect();
            let visible: Vec<u8> = lumas.iter().filter_map(|l| *l).collect();
            if visible.is_empty() { continue; }

            let avg = visible.iter().map(|&l| l as u32).sum::<u32>() / visible.len() as u32;
            let mut mask = 0u32;
            for (i, (_, _, bit)) in DOTS.iter().enumerate() {
                if let Some(l) = lumas[i] {
                    if l as u32 >= avg { mask |= 1 << bit; }
                }
            }
            if mask == 0 { continue; }
            if noise_seed.is_some() { mask = apply_noise(mask, &mut rng); }
            if mask == 0 { continue; }

            let vis_pix: Vec<&[u8;4]> = pixels.iter().enumerate()
                .filter(|(i,_)| lumas[*i].is_some()).map(|(_,p)| p).collect();
            let n = vis_pix.len() as u32;
            let (r,g,b) = vis_pix.iter().fold((0u32,0u32,0u32), |(a,b2,c),p|
                (a+p[0] as u32, b2+p[1] as u32, c+p[2] as u32));
            let (r,g,b) = (
                ((r/n) as f32 * bright) as u8,
                ((g/n) as f32 * bright) as u8,
                ((b/n) as f32 * bright) as u8,
            );
            let ch = char::from_u32(0x2800 + mask).unwrap_or(' ');
            grid[ty as usize][tx as usize] = Some((ch, Color::Rgb(r, g, b)));
        }
    }
    grid
}

// Tiny deterministic PRNG so the layout is stable across frames.
struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13; x ^= x >> 17; x ^= x << 5;
        self.0 = x; x
    }
    fn f32(&mut self) -> f32 { (self.next() >> 8) as f32 / (1u32 << 24) as f32 }
    fn range(&mut self, lo: f32, hi: f32) -> f32 { lo + self.f32() * (hi - lo) }
}

struct Layer {
    frames_clean: Vec<Grid>, // one grid per gif frame, at this layer's scale
    frames_noisy: Vec<Grid>, // same, with baked ±1-dot grain
    w: usize,
    h: usize,
    speed: f32, // cols/sec
}

struct Sprite {
    layer: usize,
    x: f32,
    y: f32,        // base top row
    frame_off: usize,
    bob_amp: f32,
    bob_phase: f32,
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let size = terminal.size()?;
    let cols = size.width as usize;
    let rows = size.height as usize;

    // Load barbarian frames
    let f = BufReader::new(File::open(GIF_PATH).expect("gif not found"));
    let raw: Vec<_> = GifDecoder::new(f).unwrap().into_frames().collect_frames().unwrap();
    let src_w = raw[0].buffer().width() as f32;
    let src_h = raw[0].buffer().height() as f32;
    let aspect = src_w / src_h; // ~0.44 for barbarian
    let imgs: Vec<DynamicImage> = raw.iter()
        .map(|fr| DynamicImage::ImageRgba8(fr.buffer().clone()))
        .collect();
    let nframes = imgs.len();

    // Depth layers: far (small/slow/dim, high on screen) → near (big/fast/bright, low)
    // sprite height in rows drives size; width derived from aspect (braille dots are square)
    let layer_specs = [
        (rows as f32 * 0.34, 0.45f32, 7.0f32),   // far
        (rows as f32 * 0.50, 0.72f32, 12.0f32),  // mid
        (rows as f32 * 0.70, 1.00f32, 18.0f32),  // near
    ];
    let layers: Vec<Layer> = layer_specs.iter().map(|&(h_rows, bright, speed)| {
        let h = h_rows.max(4.0) as u32;
        let w = (h as f32 * 2.0 * aspect).max(2.0) as u32;
        let frames_clean: Vec<Grid> = imgs.iter()
            .map(|im| render_grid(im, w, h, bright, None)).collect();
        let frames_noisy: Vec<Grid> = imgs.iter().enumerate()
            .map(|(fi, im)| render_grid(im, w, h, bright, Some(0x1000 + fi as u32 * 2654435761)))
            .collect();
        Layer { frames_clean, frames_noisy, w: w as usize, h: h as usize, speed }
    }).collect();

    // Spawn sprites: more in back, fewer in front (density falls off with depth)
    let counts = [34usize, 22, 13];
    let y_bands = [
        (1.0f32, rows as f32 * 0.40),
        (rows as f32 * 0.30, rows as f32 * 0.66),
        (rows as f32 * 0.50, rows as f32 - layers[2].h as f32 - 1.0),
    ];
    let mut rng = Rng(0x9e3779b9);
    let mut sprites: Vec<Sprite> = Vec::new();
    for (li, &count) in counts.iter().enumerate() {
        let (ylo, yhi) = y_bands[li];
        for _ in 0..count {
            sprites.push(Sprite {
                layer: li,
                x: rng.range(-(layers[li].w as f32), cols as f32),
                y: rng.range(ylo, yhi.max(ylo + 1.0)),
                frame_off: (rng.f32() * nframes as f32) as usize,
                bob_amp: rng.range(0.4, 1.4),
                bob_phase: rng.range(0.0, 6.28),
            });
        }
    }
    // Draw back-to-front so near sprites occlude far ones
    sprites.sort_by_key(|s| s.layer);

    let frame_period = 0.05f32; // walk-cycle speed (~20fps for the gait itself)
    let start = Instant::now();
    let mut last = Instant::now();
    let mut paused = false;
    let mut noise = false;

    loop {
        let now = Instant::now();
        let dt = (now - last).as_secs_f32();
        last = now;
        let t = start.elapsed().as_secs_f32();
        let base_frame = (t / frame_period) as usize;

        if !paused {
            for s in &mut sprites {
                s.x += layers[s.layer].speed * dt;
                let w = layers[s.layer].w as f32;
                if s.x > cols as f32 { s.x = -w; }
            }
        }

        // Composite into a cell buffer
        let mut buf: Vec<Vec<Cell>> = vec![vec![None; cols]; rows];
        for s in &sprites {
            let layer = &layers[s.layer];
            let frames = if noise { &layer.frames_noisy } else { &layer.frames_clean };
            let grid = &frames[(base_frame + s.frame_off) % nframes];
            let bob = (t * 1.6 + s.bob_phase).sin() * s.bob_amp;
            let sx = s.x.round() as isize;
            let sy = (s.y + bob).round() as isize;
            for (r, row) in grid.iter().enumerate() {
                let by = sy + r as isize;
                if by < 0 || by >= rows as isize { continue; }
                let bufrow = &mut buf[by as usize];
                for (c, cell) in row.iter().enumerate() {
                    if cell.is_none() { continue; }
                    let bx = sx + c as isize;
                    if bx < 0 || bx >= cols as isize { continue; }
                    bufrow[bx as usize] = *cell;
                }
            }
        }

        // Convert buffer to Lines, coalescing runs of identical color
        let mut lines: Vec<Line> = Vec::with_capacity(rows);
        for row in &buf {
            let mut spans: Vec<Span> = Vec::new();
            let mut i = 0;
            while i < cols {
                match row[i] {
                    None => {
                        let mut k = i;
                        while k < cols && row[k].is_none() { k += 1; }
                        spans.push(Span::raw(" ".repeat(k - i)));
                        i = k;
                    }
                    Some((_, color)) => {
                        let mut s = String::new();
                        while i < cols {
                            match row[i] {
                                Some((ch, col)) if col == color => { s.push(ch); i += 1; }
                                _ => break,
                            }
                        }
                        spans.push(Span::styled(s, Style::default().fg(color)));
                    }
                }
            }
            lines.push(Line::from(spans));
        }

        let total: usize = sprites.len();
        let hud = format!(
            " tidal wave — {} sprites — noise: {} (n) — space pause — q quit ",
            total, if noise { "ON" } else { "off" }
        );
        // overlay hud on top row
        if let Some(first) = lines.first_mut() {
            *first = Line::from(Span::styled(hud, Style::default().fg(Color::White)));
        }

        terminal.draw(|f| {
            f.render_widget(Paragraph::new(lines.clone()), f.area());
        })?;

        if event::poll(Duration::from_millis(8))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char(' ') => paused = !paused,
                    KeyCode::Char('n') => noise = !noise,
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
