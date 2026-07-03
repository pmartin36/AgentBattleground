// attach — composite a weapon sprite onto a creature's hand, per frame, tracking a hand anchor.
// This is the "add the sword after the fact" step: the creature clip is weaponless (so nothing melts),
// and we bind a crisp weapon sprite to the hand each frame (position + angle + scale), rotating it
// along the swing. Output = new frames with the weapon composited, ready for braille playback.
//
// Usage: attach <frames_dir> <weapon.png> <anchors.txt> <out_dir> [--handle hx,hy]
//   anchors.txt: one keyframe per line "idx x y angle_deg scale" (frame idx into the clip; x,y = where
//   the weapon HANDLE goes in the frame; angle rotates the weapon about its handle; scale sizes it).
//   Sparse keyframes are linearly interpolated across all frames.

use image::{GenericImageView, RgbaImage};
use std::{fs, path::Path};

fn load_frames(dir: &str) -> Vec<RgbaImage> {
    let mut f: Vec<_> = fs::read_dir(dir).expect("frames dir").filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("png") | Some("jpg"))).collect();
    f.sort();
    f.iter().map(|p| image::open(p).expect("frame").to_rgba8()).collect()
}
fn corner_key(img: &RgbaImage) -> (i32, i32, i32) {
    let (w, h) = (img.width(), img.height());
    let c = [img.get_pixel(0,0).0, img.get_pixel(w-1,0).0, img.get_pixel(0,h-1).0, img.get_pixel(w-1,h-1).0];
    let (mut r, mut g, mut b) = (0i32, 0i32, 0i32);
    for p in c { r += p[0] as i32; g += p[1] as i32; b += p[2] as i32; }
    (r/4, g/4, b/4)
}

#[derive(Clone, Copy)]
struct Anchor { x: f32, y: f32, ang: f32, scale: f32 }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 { eprintln!("usage: attach <frames_dir> <weapon.png> <anchors.txt> <out_dir> [--handle hx,hy]"); std::process::exit(1); }
    let frames = load_frames(&a[1]);
    let weapon = image::open(&a[2]).expect("weapon").to_rgba8();
    let (ww, wh) = (weapon.width() as i32, weapon.height() as i32);
    let key = corner_key(&weapon);
    let out = &a[4];
    // weapon handle point (where it binds to the hand). Default: bottom-center pommel.
    let mut handle = (ww as f32 / 2.0, wh as f32 * 0.92);
    let mut i = 5;
    while i < a.len() {
        if a[i] == "--handle" && i+1 < a.len() {
            let v: Vec<f32> = a[i+1].split(',').filter_map(|s| s.trim().parse().ok()).collect();
            if v.len() == 2 { handle = (v[0], v[1]); }
            i += 2;
        } else { i += 1; }
    }
    fs::create_dir_all(out).unwrap();

    // parse sparse keyframes, then interpolate to one Anchor per frame
    let mut kf: Vec<(usize, Anchor)> = Vec::new();
    for line in fs::read_to_string(&a[3]).expect("anchors").lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') { continue; }
        let p: Vec<f32> = t.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        if p.len() == 5 { kf.push((p[0] as usize, Anchor { x: p[1], y: p[2], ang: p[3].to_radians(), scale: p[4] })); }
    }
    kf.sort_by_key(|(i, _)| *i);
    if kf.is_empty() { eprintln!("no anchors"); std::process::exit(1); }
    let anchor_at = |f: usize| -> Anchor {
        if f <= kf[0].0 { return kf[0].1; }
        if f >= kf[kf.len()-1].0 { return kf[kf.len()-1].1; }
        for w in kf.windows(2) {
            let (i0, a0) = w[0]; let (i1, a1) = w[1];
            if f >= i0 && f <= i1 {
                let u = (f - i0) as f32 / (i1 - i0).max(1) as f32;
                return Anchor {
                    x: a0.x + (a1.x-a0.x)*u, y: a0.y + (a1.y-a0.y)*u,
                    ang: a0.ang + (a1.ang-a0.ang)*u, scale: a0.scale + (a1.scale-a0.scale)*u,
                };
            }
        }
        kf[kf.len()-1].1
    };

    let is_key = |p: [u8;4]| -> bool {
        if p[3] < 128 { return true; }
        let (dr,dg,db) = (p[0] as i32-key.0, p[1] as i32-key.1, p[2] as i32-key.2);
        dr*dr+dg*dg+db*db < 95*95
    };

    for (fi, frame) in frames.iter().enumerate() {
        let an = anchor_at(fi);
        let mut o = frame.clone();
        let (fw, fh) = (frame.width() as i32, frame.height() as i32);
        let (ca, sa) = (an.ang.cos(), an.ang.sin());
        // inverse-map every frame pixel back into weapon space (handle-anchored, rotated, scaled)
        for oy in 0..fh { for ox in 0..fw {
            let (fdx, fdy) = ((ox as f32 - an.x)/an.scale, (oy as f32 - an.y)/an.scale);
            let sx = handle.0 + (fdx*ca + fdy*sa);
            let sy = handle.1 + (-fdx*sa + fdy*ca);
            if sx < 0.0 || sy < 0.0 || sx >= ww as f32 || sy >= wh as f32 { continue; }
            let wp = weapon.get_pixel(sx as u32, sy as u32).0;
            if is_key(wp) { continue; }
            o.put_pixel(ox as u32, oy as u32, image::Rgba([wp[0], wp[1], wp[2], 255]));
        }}
        o.save(Path::new(out).join(format!("frame_{:03}.png", fi))).unwrap();
    }
    eprintln!("attach: composited {} frames -> {}", frames.len(), out);
}
