// mkslash — synthesize a crude grayscale "control video" of a sword slash for VACE V2V.
// A static body blob + a bright blade bar pivoting through a downward arc. This stands in for
// an LLM-drawn trajectory rendered as a control signal — the cheap probe of whether VACE can be
// driven by a synthesized (non-photoreal) control at all.
//
// Usage: mkslash <out_dir> [frames] [W] [H]

use std::env;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: mkslash <out_dir> [frames] [W] [H]");
        std::process::exit(1);
    }
    let out = &args[1];
    let frames: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(17);
    let w: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(512);
    let h: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(512);
    std::fs::create_dir_all(out).unwrap();

    // Static body blob (a centered ellipse), and a blade that pivots from up-left to down-left.
    let (cx, cy, rx, ry) = (255.0f32, 330.0, 120.0, 150.0);
    let (px, py) = (205.0f32, 330.0); // sword pivot (left paw)
    let (len, thick) = (195.0f32, 15.0);
    let (a0, a1) = (235.0f32.to_radians(), 140.0f32.to_radians()); // screen degrees: 270=up,180=left,90=down

    for f in 0..frames {
        let t = if frames > 1 { f as f32 / (frames - 1) as f32 } else { 0.0 };
        // ease-in the swing a touch so it reads as a strike, not a constant sweep
        let te = t * t * (3.0 - 2.0 * t);
        let ang = a0 + (a1 - a0) * te;
        let (tx, ty) = (px + len * ang.cos(), py + len * ang.sin());

        let mut img = image::RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32, y as f32);
                let mut v: f32 = 45.0; // background (far)
                // body ellipse (mid, static)
                let e = ((fx - cx) / rx).powi(2) + ((fy - cy) / ry).powi(2);
                if e <= 1.0 { v = v.max(140.0); }
                // blade bar (bright, moving): distance to segment [pivot,tip]
                let (dx, dy) = (tx - px, ty - py);
                let seg2 = dx * dx + dy * dy;
                let proj = (((fx - px) * dx + (fy - py) * dy) / seg2).clamp(0.0, 1.0);
                let (qx, qy) = (px + proj * dx, py + proj * dy);
                let d = ((fx - qx).powi(2) + (fy - qy).powi(2)).sqrt();
                if d <= thick * 0.5 { v = v.max(235.0); }
                let g = v as u8;
                img.put_pixel(x, y, image::Rgb([g, g, g]));
            }
        }
        let path = Path::new(out).join(format!("frame_{:03}.png", f));
        img.save(&path).unwrap();
    }
    eprintln!("mkslash: wrote {} control frames to {}", frames, out);
}
