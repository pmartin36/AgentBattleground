use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::{codecs::gif::GifDecoder, AnimationDecoder, DynamicImage, GenericImageView};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{
    fs::File,
    io::{self, BufReader},
    time::{Duration, Instant},
};

const GIFS: &[(&str, &str)] = &[
    ("Pikachu Thunderbolt", "/tmp/pikachu_thunderbolt.gif"),
    ("Pikachu Warm-Up",     "/tmp/pikachu_warmup.gif"),
    ("Barbarian Walk",      "/tmp/barbarian.gif"),
];

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

struct LoadedGif {
    name: String,
    frames: Vec<(Vec<Line<'static>>, Duration)>,
}

fn load_gif(path: &str, name: &str, cols: u32, rows: u32) -> LoadedGif {
    let f = BufReader::new(File::open(path).expect("gif not found"));
    let raw_frames: Vec<_> = GifDecoder::new(f).unwrap()
        .into_frames().collect_frames().unwrap();

    let first = DynamicImage::ImageRgba8(raw_frames[0].buffer().clone());

    // Aspect-correct output size (braille dots are square)
    let src_w = first.width() as f32;
    let src_h = first.height() as f32;
    let avail_w = cols as f32 * 2.0;
    let avail_h = rows as f32 * 4.0;
    let scale = (avail_w / src_w).min(avail_h / src_h);
    let out_cols = ((src_w * scale) / 2.0) as u32;
    let out_rows = ((src_h * scale) / 4.0) as u32;

    let frames = raw_frames.iter().map(|frame| {
        let d = frame.delay().numer_denom_ms();
        let ms = (d.0 / d.1).max(16); // floor at ~60fps
        let delay = Duration::from_millis(ms as u64);
        let img = DynamicImage::ImageRgba8(frame.buffer().clone());
        (render_braille(&img, out_cols, out_rows), delay)
    }).collect();

    LoadedGif { name: name.to_string(), frames }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let size = terminal.size()?;
    let cols = size.width as u32;
    let rows = size.height.saturating_sub(2) as u32;

    eprintln!("Loading GIFs...");
    let gifs: Vec<LoadedGif> = GIFS.iter()
        .filter(|(_, path)| std::path::Path::new(path).exists())
        .map(|(name, path)| load_gif(path, name, cols, rows))
        .collect();

    let mut gif_idx = 0usize;
    let mut frame_idx = 0usize;
    let mut last_frame = Instant::now();

    loop {
        let gif = &gifs[gif_idx];
        let (lines, delay) = &gif.frames[frame_idx];
        let title = format!(
            " {} — frame {}/{} — ←/→ switch GIF — q quit ",
            gif.name, frame_idx + 1, gif.frames.len()
        );

        let render_lines = lines.clone();
        terminal.draw(move |f| {
            let area = f.area();
            let block = Block::default().title(title.as_str()).borders(Borders::ALL);
            let inner = block.inner(area);
            f.render_widget(block, area);
            f.render_widget(
                Paragraph::new(render_lines).alignment(Alignment::Center),
                inner,
            );
        })?;

        if last_frame.elapsed() >= *delay {
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

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
