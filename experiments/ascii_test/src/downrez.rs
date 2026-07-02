// downrez — the universal "nice render → battlefield braille" converter.
// Usage: downrez <image> [--width N] [--no-color]
// Prints colored Unicode braille to stdout, honoring alpha transparency.
// Both the in-app generator and the drag-n-drop import path feed through this.

use image::GenericImageView;
use std::env;

const DOTS: [(u32, u32, u8); 8] = [
    (0,0,0),(0,1,1),(0,2,2),
    (1,0,3),(1,1,4),(1,2,5),
    (0,3,6),(1,3,7),
];

fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: downrez <image> [--width N] [--no-color]");
        std::process::exit(1);
    }
    let path = &args[1];
    let mut width = 100u32;
    let mut color = true;
    let mut chroma: Option<(u8, u8, u8)> = None;
    let mut chroma_thresh = 50u32;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--width" if i + 1 < args.len() => {
                width = args[i + 1].parse().unwrap_or(100);
                i += 2;
            }
            "--no-color" => { color = false; i += 1; }
            "--chroma" if i + 1 < args.len() => {
                let v: Vec<u8> = args[i + 1].split(',').filter_map(|s| s.trim().parse().ok()).collect();
                if v.len() == 3 { chroma = Some((v[0], v[1], v[2])); }
                i += 2;
            }
            "--chroma-thresh" if i + 1 < args.len() => {
                chroma_thresh = args[i + 1].parse().unwrap_or(50); i += 2;
            }
            _ => i += 1,
        }
    }
    let chroma_thresh2 = chroma_thresh * chroma_thresh;
    // A pixel is transparent if its alpha is low OR (chroma set) it's near the key color.
    let is_transparent = |p: &[u8; 4]| -> bool {
        if p[3] < 128 { return true; }
        if let Some((kr, kg, kb)) = chroma {
            let (dr, dg, db) = (p[0] as i32 - kr as i32, p[1] as i32 - kg as i32, p[2] as i32 - kb as i32);
            if (dr*dr + dg*dg + db*db) as u32 <= chroma_thresh2 { return true; }
        }
        false
    };

    let img = match image::open(path) {
        Ok(im) => im,
        Err(e) => { eprintln!("downrez: cannot open {}: {}", path, e); std::process::exit(1); }
    };

    let (sw, sh) = (img.width() as f32, img.height() as f32);
    let aspect = sw / sh;
    let cols = width.max(1);
    // braille dots are square: out_px_w/out_px_h = aspect; out_px_w = cols*2, out_px_h = rows*4
    let rows = ((cols as f32) / (2.0 * aspect)).round().max(1.0) as u32;
    let img = img.resize_exact(cols * 2, rows * 4, image::imageops::FilterType::Lanczos3);

    let mut out = String::new();
    for ty in 0..rows {
        for tx in 0..cols {
            let (px, py) = (tx * 2, ty * 4);
            let pixels: Vec<[u8; 4]> = DOTS.iter()
                .map(|(dx, dy, _)| img.get_pixel(px + dx, py + dy).0)
                .collect();
            let lumas: Vec<Option<u8>> = pixels.iter()
                .map(|p| if is_transparent(p) { None } else { Some(luma(p[0], p[1], p[2])) })
                .collect();
            let visible: Vec<u8> = lumas.iter().filter_map(|l| *l).collect();
            if visible.is_empty() { out.push(' '); continue; }

            let avg = visible.iter().map(|&l| l as u32).sum::<u32>() / visible.len() as u32;
            let mut mask = 0u32;
            for (k, (_, _, bit)) in DOTS.iter().enumerate() {
                if let Some(l) = lumas[k] {
                    if l as u32 >= avg { mask |= 1 << bit; }
                }
            }
            if mask == 0 { out.push(' '); continue; }

            let ch = char::from_u32(0x2800 + mask).unwrap_or(' ');
            if color {
                let vp: Vec<&[u8;4]> = pixels.iter().enumerate()
                    .filter(|(k,_)| lumas[*k].is_some()).map(|(_,p)| p).collect();
                let n = vp.len() as u32;
                let (r,g,b) = vp.iter().fold((0u32,0u32,0u32), |(a,b2,c),p|
                    (a+p[0] as u32, b2+p[1] as u32, c+p[2] as u32));
                out.push_str(&format!("\x1b[38;2;{};{};{}m{}", r/n, g/n, b/n, ch));
            } else {
                out.push(ch);
            }
        }
        if color { out.push_str("\x1b[0m"); }
        out.push('\n');
    }
    print!("{}", out);
}
