// mkslash3 — a structurally-proportioned 2-bone (shoulder->elbow->wrist) arm control for VACE,
// replacing mkslash2's single straight bar. Research finding: VACE control should match the
// creature's OWN topology/limb proportions (a jointed, tapered arm), not an ellipse+bar or an
// imported human skeleton (cross-morphology retargeting onto a non-human subject is unvalidated
// and risky). This stays same-species: it's just a better-shaped stand-in for the mouse's own arm.
//
// Usage: mkslash3 <ref_image> <out_dir> [frames] [W] [H] [mode: body|arm]
//   body = mouse silhouette (mid-gray) + bright tapered 2-bone arm   (identity-preserving)
//   arm  = arm only, on dark                                         (low vace-strength variant)

use std::env;
use std::path::Path;

fn main() {
    let a: Vec<String> = env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: mkslash3 <ref_image> <out_dir> [frames] [W] [H] [body|arm]");
        std::process::exit(1);
    }
    let refp = &a[1];
    let out = &a[2];
    let frames: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(25);
    let w: u32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(512);
    let h: u32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(512);
    let mode = a.get(6).map(|s| s.as_str()).unwrap_or("body");
    std::fs::create_dir_all(out).unwrap();

    let img = image::open(refp).expect("open ref").resize_exact(w, h, image::imageops::FilterType::Triangle).to_rgb8();
    let key = {
        let c = [img.get_pixel(0, 0), img.get_pixel(w - 1, 0), img.get_pixel(0, h - 1), img.get_pixel(w - 1, h - 1)];
        let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
        for p in c { r += p[0] as u32; g += p[1] as u32; b += p[2] as u32; }
        [(r / 4) as i32, (g / 4) as i32, (b / 4) as i32]
    };
    let is_fg = |x: u32, y: u32| -> bool {
        let p = img.get_pixel(x, y).0;
        let d = (p[0] as i32 - key[0]).pow(2) + (p[1] as i32 - key[1]).pow(2) + (p[2] as i32 - key[2]).pow(2);
        d > 90 * 90
    };

    // shoulder anchor + bone lengths, proportioned to where the mouse's paw actually attaches to
    // its body (short arms — this is a chunky mouse, not a human): shoulder near the lower-left
    // body edge, ~0.40w/0.62h, with short upper-arm/forearm segments so the wrist stays near-body.
    let (shx, shy) = (0.40 * w as f32, 0.62 * h as f32);
    let l1 = 0.11 * w as f32; // shoulder->elbow
    let l2 = 0.13 * w as f32; // elbow->wrist
    let (t_shoulder, t_elbow, t_wrist) = (0.040 * w as f32, 0.030 * w as f32, 0.020 * w as f32);

    // shoulder absolute angle + elbow relative bend, windup->strike (screen deg: 270=up,180=left,90=down).
    // rest = paw hanging down at the side; windup = raised up/back; strike = swung down/forward.
    let sh_rest = 100f32.to_radians();
    let sh_wind = 235f32.to_radians(); // raised up/back (anticipation)
    let sh_strike = 55f32.to_radians(); // follow-through down/forward
    let el_rest = -15f32.to_radians(); // relative bend at elbow
    let el_wind = -55f32.to_radians(); // more folded during windup
    let el_strike = -5f32.to_radians(); // extends straighter on the strike

    for f in 0..frames {
        let t = if frames > 1 { f as f32 / (frames - 1) as f32 } else { 0.0 };
        // phase 1 (0..0.4): slow windup ; phase 2 (0.4..1): fast strike (ease-out snap)
        let (sh_ang, el_ang) = if t < 0.4 {
            let u = t / 0.4;
            let e = u * u;
            (sh_rest + (sh_wind - sh_rest) * e, el_rest + (el_wind - el_rest) * e)
        } else {
            let u = (t - 0.4) / 0.6;
            let e = 1.0 - (1.0 - u).powi(3);
            (sh_wind + (sh_strike - sh_wind) * e, el_wind + (el_strike - el_wind) * e)
        };

        let (ex, ey) = (shx + l1 * sh_ang.cos(), shy + l1 * sh_ang.sin());
        let wrist_ang = sh_ang + el_ang;
        let (wx, wy) = (ex + l2 * wrist_ang.cos(), ey + l2 * wrist_ang.sin());

        let mut o = image::RgbImage::new(w, h);
        // capsule-with-taper distance: interpolated thickness along the segment's projection param.
        let seg_val = |fx: f32, fy: f32, p0: (f32, f32), p1: (f32, f32), th0: f32, th1: f32| -> Option<f32> {
            let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
            let seg2 = (dx * dx + dy * dy).max(1.0);
            let proj = (((fx - p0.0) * dx + (fy - p0.1) * dy) / seg2).clamp(0.0, 1.0);
            let (qx, qy) = (p0.0 + proj * dx, p0.1 + proj * dy);
            let d = ((fx - qx).powi(2) + (fy - qy).powi(2)).sqrt();
            let th = th0 + (th1 - th0) * proj;
            if d <= th * 0.5 { Some(1.0 - (d / (th * 0.5)).powi(2) * 0.15) } else { None }
        };

        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32, y as f32);
                let mut v: f32 = 26.0; // background (far)
                if mode == "body" && is_fg(x, y) { v = 140.0; } // creature body (mid depth, static)
                if let Some(m) = seg_val(fx, fy, (shx, shy), (ex, ey), t_shoulder, t_elbow) {
                    v = v.max(232.0 * m);
                }
                if let Some(m) = seg_val(fx, fy, (ex, ey), (wx, wy), t_elbow, t_wrist) {
                    v = v.max(250.0 * m);
                }
                // bright grip marker at the wrist (helps mark the hand/handle point)
                let dw = ((fx - wx).powi(2) + (fy - wy).powi(2)).sqrt();
                if dw <= t_wrist * 0.9 { v = 255.0; }
                let g = v.clamp(0.0, 255.0) as u8;
                o.put_pixel(x, y, image::Rgb([g, g, g]));
            }
        }
        o.save(Path::new(out).join(format!("frame_{:03}.png", f))).unwrap();
    }
    eprintln!("mkslash3: {} frames ({} mode) -> {}", frames, mode, out);
}
