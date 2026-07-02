// mkslash2 — a *structurally faithful, lively* slash control for VACE.
// Derives the creature's actual silhouette from its (green-screen) image so the control's body
// MATCHES the creature (identity survives high vace-strength), then animates the sword with a real
// windup + snappy ease-out. This is "author a lively motion once, retarget to any creature."
//
// Usage: mkslash2 <ref_image> <out_dir> [frames] [W] [H] [mode: body|sword]
//   body  = mouse silhouette (mid-gray) + bright arcing sword   (identity-preserving)
//   sword = bright arcing sword only, on dark                    (energy-slash / low-strength)

use image::GenericImageView;
use std::env;
use std::path::Path;

fn main() {
    let a: Vec<String> = env::args().collect();
    if a.len() < 3 { eprintln!("usage: mkslash2 <ref_image> <out_dir> [frames] [W] [H] [body|sword]"); std::process::exit(1); }
    let refp = &a[1];
    let out = &a[2];
    let frames: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(25);
    let w: u32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(512);
    let h: u32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(512);
    let mode = a.get(6).map(|s| s.as_str()).unwrap_or("body");
    std::fs::create_dir_all(out).unwrap();

    // Load ref, resize to control resolution, build a foreground mask by keying the corner color.
    let img = image::open(refp).expect("open ref").resize_exact(w, h, image::imageops::FilterType::Triangle).to_rgb8();
    let key = {
        let c = [img.get_pixel(0,0), img.get_pixel(w-1,0), img.get_pixel(0,h-1), img.get_pixel(w-1,h-1)];
        let (mut r,mut g,mut b)=(0u32,0u32,0u32);
        for p in c { r+=p[0] as u32; g+=p[1] as u32; b+=p[2] as u32; }
        [(r/4) as i32,(g/4) as i32,(b/4) as i32]
    };
    let is_fg = |x: u32, y: u32| -> bool {
        let p = img.get_pixel(x, y).0;
        let d = (p[0] as i32-key[0]).pow(2)+(p[1] as i32-key[1]).pow(2)+(p[2] as i32-key[2]).pow(2);
        d > 90*90 // far from background color => foreground (mouse)
    };

    // Sword pivot ~ where the paws hold it, and a windup→strike arc (screen deg: 270=up,180=left,90=down).
    let (px, py) = (0.37 * w as f32, 0.60 * h as f32);
    let (len, thick) = (0.42 * w as f32, 0.032 * w as f32);
    let a_rest = 212f32.to_radians();
    let a_wind = 250f32.to_radians();   // pulled up/back (anticipation)
    let a_strike = 150f32.to_radians(); // follow-through down

    for f in 0..frames {
        let t = if frames > 1 { f as f32 / (frames - 1) as f32 } else { 0.0 };
        // phase 1 (0..0.4): slow windup rest->wind ; phase 2 (0.4..1): fast strike wind->strike (ease-out)
        let ang = if t < 0.4 {
            let u = t / 0.4;
            let e = u * u; // ease-in (slow start) for anticipation
            a_rest + (a_wind - a_rest) * e
        } else {
            let u = (t - 0.4) / 0.6;
            let e = 1.0 - (1.0 - u).powi(3); // ease-out (fast snap, settle)
            a_wind + (a_strike - a_wind) * e
        };
        let (tx, ty) = (px + len * ang.cos(), py + len * ang.sin());

        let mut o = image::RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut v: f32 = 28.0; // background (far)
                if mode == "body" && is_fg(x, y) { v = 145.0; } // creature body (mid depth, static)
                // sword bar (bright, moving)
                let (fx, fy) = (x as f32, y as f32);
                let (dx, dy) = (tx - px, ty - py);
                let seg2 = dx*dx + dy*dy;
                let proj = (((fx-px)*dx + (fy-py)*dy)/seg2).clamp(0.0,1.0);
                let (qx, qy) = (px+proj*dx, py+proj*dy);
                let d = ((fx-qx).powi(2)+(fy-qy).powi(2)).sqrt();
                if d <= thick*0.5 { v = v.max(238.0); }
                let g = v as u8;
                o.put_pixel(x, y, image::Rgb([g,g,g]));
            }
        }
        o.save(Path::new(out).join(format!("frame_{:03}.png", f))).unwrap();
    }
    eprintln!("mkslash2: {} frames ({} mode) -> {}", frames, mode, out);
}
