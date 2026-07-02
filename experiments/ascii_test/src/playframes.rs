// playframes — braille animation player for a frame sequence.
// Input: a directory of PNG frames (frame_000.png, ...) OR a single .gif.
// Keys the background out at render time (alpha OR chroma color), so a solid-background
// video from Wan I2V animates cleanly with no intermediate files.
//
// Usage: playframes <dir|gif> [--width N] [--chroma R,G,B] [--chroma-thresh N] [--pingpong] [--fps N]

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use image::{codecs::gif::GifDecoder, AnimationDecoder, DynamicImage, GenericImageView, RgbaImage};
use ratatui::{
    backend::CrosstermBackend,
    layout::Alignment,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};
use std::{
    fs::{self, File},
    io::{self, BufReader},
    path::Path,
    time::{Duration, Instant},
};

const DOTS: [(u32, u32, u8); 8] = [
    (0,0,0),(0,1,1),(0,2,2),
    (1,0,3),(1,1,4),(1,2,5),
    (0,3,6),(1,3,7),
];

fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8
}

struct Key { rgb: (u8, u8, u8), thresh2: u32 }

fn transparent(p: [u8; 4], key: &Option<Key>) -> bool {
    if p[3] < 128 { return true; }
    if let Some(k) = key {
        let (dr, dg, db) = (p[0] as i32 - k.rgb.0 as i32, p[1] as i32 - k.rgb.1 as i32, p[2] as i32 - k.rgb.2 as i32);
        if (dr*dr + dg*dg + db*db) as u32 <= k.thresh2 { return true; }
    }
    false
}

fn render_braille(img: &DynamicImage, cols: u32, rows: u32, key: &Option<Key>) -> Vec<Line<'static>> {
    let img = img.resize_exact(cols * 2, rows * 4, image::imageops::FilterType::Lanczos3);
    let mut lines = Vec::with_capacity(rows as usize);
    for ty in 0..rows {
        let mut spans: Vec<Span> = Vec::new();
        for tx in 0..cols {
            let (px, py) = (tx * 2, ty * 4);
            let pixels: Vec<[u8; 4]> = DOTS.iter().map(|(dx, dy, _)| img.get_pixel(px + dx, py + dy).0).collect();
            let lumas: Vec<Option<u8>> = pixels.iter()
                .map(|p| if transparent(*p, key) { None } else { Some(luma(p[0], p[1], p[2])) })
                .collect();
            let visible: Vec<u8> = lumas.iter().filter_map(|l| *l).collect();
            if visible.is_empty() { spans.push(Span::raw(" ")); continue; }
            let avg = visible.iter().map(|&l| l as u32).sum::<u32>() / visible.len() as u32;
            let mut mask = 0u32;
            for (i, (_, _, bit)) in DOTS.iter().enumerate() {
                if let Some(l) = lumas[i] { if l as u32 >= avg { mask |= 1 << bit; } }
            }
            if mask == 0 { spans.push(Span::raw(" ")); continue; }
            let vis: Vec<&[u8;4]> = pixels.iter().enumerate().filter(|(i,_)| lumas[*i].is_some()).map(|(_,p)| p).collect();
            let n = vis.len() as u32;
            let (r,g,b) = vis.iter().fold((0u32,0u32,0u32), |(a,b2,c),p| (a+p[0] as u32, b2+p[1] as u32, c+p[2] as u32));
            let ch = char::from_u32(0x2800 + mask).unwrap_or(' ');
            spans.push(Span::styled(ch.to_string(), Style::default().fg(Color::Rgb((r/n) as u8,(g/n) as u8,(b/n) as u8))));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn load_frames(path: &str) -> Vec<RgbaImage> {
    let p = Path::new(path);
    if p.is_dir() {
        let mut files: Vec<_> = fs::read_dir(p).expect("read dir").filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| matches!(f.extension().and_then(|e| e.to_str()), Some("png")|Some("jpg")|Some("jpeg")))
            .collect();
        files.sort();
        files.iter().map(|f| image::open(f).expect("open frame").to_rgba8()).collect()
    } else if p.extension().and_then(|e| e.to_str()) == Some("gif") {
        let f = BufReader::new(File::open(p).expect("open gif"));
        GifDecoder::new(f).unwrap().into_frames().collect_frames().unwrap()
            .into_iter().map(|fr| fr.into_buffer()).collect()
    } else {
        eprintln!("playframes: expected a directory of frames or a .gif"); std::process::exit(1);
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: playframes <dir|gif> [--width N] [--chroma R,G,B] [--chroma-thresh N] [--pingpong] [--fps N]");
        std::process::exit(1);
    }
    let src = args[1].clone();
    let mut width: Option<u32> = None;
    let mut key: Option<Key> = None;
    let mut thresh = 60u32;
    let mut pingpong = false;
    let mut fps = 12u32;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--width" if i+1 < args.len() => { width = args[i+1].parse().ok(); i += 2; }
            "--chroma" if i+1 < args.len() => {
                let v: Vec<u8> = args[i+1].split(',').filter_map(|s| s.trim().parse().ok()).collect();
                if v.len() == 3 { key = Some(Key { rgb: (v[0],v[1],v[2]), thresh2: thresh*thresh }); }
                i += 2;
            }
            "--chroma-thresh" if i+1 < args.len() => { thresh = args[i+1].parse().unwrap_or(60); if let Some(k)=key.as_mut(){k.thresh2=thresh*thresh;} i += 2; }
            "--pingpong" => { pingpong = true; i += 1; }
            "--fps" if i+1 < args.len() => { fps = args[i+1].parse().unwrap_or(12).max(1); i += 2; }
            _ => i += 1,
        }
    }
    // chroma thresh may have been parsed after --chroma; recompute
    if let Some(k) = key.as_mut() { k.thresh2 = thresh * thresh; }

    let frames_raw = load_frames(&src);
    if frames_raw.is_empty() { eprintln!("playframes: no frames found"); std::process::exit(1); }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let size = terminal.size()?;
    let (sw, sh) = (frames_raw[0].width() as f32, frames_raw[0].height() as f32);
    let aspect = sw / sh;
    let max_cols = width.unwrap_or(size.width as u32).min(size.width as u32);
    let max_rows = size.height.saturating_sub(1) as u32;
    // fit within terminal, square braille dots
    let mut cols = max_cols;
    let mut rows = ((cols as f32) / (2.0 * aspect)).round().max(1.0) as u32;
    if rows > max_rows { rows = max_rows; cols = ((rows as f32) * 2.0 * aspect).round().max(1.0) as u32; }

    let frames: Vec<Vec<Line>> = frames_raw.iter()
        .map(|f| render_braille(&DynamicImage::ImageRgba8(f.clone()), cols, rows, &key))
        .collect();

    // playback order (ping-pong appends the reverse, minus endpoints)
    let mut order: Vec<usize> = (0..frames.len()).collect();
    if pingpong && frames.len() > 2 {
        order.extend((1..frames.len()-1).rev());
    }

    let frame_dur = Duration::from_millis((1000 / fps) as u64);
    let mut idx = 0usize;
    let mut last = Instant::now();

    loop {
        let lines = frames[order[idx]].clone();
        terminal.draw(|f| {
            f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), f.area());
        })?;
        if last.elapsed() >= frame_dur {
            idx = (idx + 1) % order.len();
            last = Instant::now();
        }
        if event::poll(Duration::from_millis(4))? {
            if let Event::Key(k) = event::read()? {
                if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) { break; }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
