//! Generates `crates/render/src/assets/button_panel.png` — a 64×32 RGBA8
//! rounded-rect panel background used as the braille UI chrome button base
//! (spec `22-braille-ui-chrome.md`). First-party art; this generator IS the
//! provenance/license.
//!
//! Geometry (must match `crates/render/src/assets.rs` tests):
//!   - 64×32, opaque white fill, corner radius 8, hard alpha edge.
//!   - membership: clamp (x,y) to inner rect [8,55]×[8,23]; inside iff the
//!     squared distance from (x,y) to the clamped point is <= 8^2.

use image::{ImageBuffer, Rgba};

fn main() {
    let (w, h): (u32, u32) = (64, 32);
    let r: i32 = 8;

    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(w, h, Rgba([0u8, 0, 0, 0])); // transparent ground

    let x_lo = r;
    let x_hi = w as i32 - 1 - r;
    let y_lo = r;
    let y_hi = h as i32 - 1 - r;

    for y in 0..h {
        for x in 0..w {
            let xi = x as i32;
            let yi = y as i32;
            let cx = xi.clamp(x_lo, x_hi);
            let cy = yi.clamp(y_lo, y_hi);
            let dx = xi - cx;
            let dy = yi - cy;
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x, y, Rgba([0xff, 0xff, 0xff, 0xff])); // opaque white
            }
        }
    }

    img.save("crates/render/src/assets/button_panel.png")
        .unwrap();
    println!(
        "Generated crates/render/src/assets/button_panel.png (64x32 RGBA, rounded-rect, radius 8)"
    );
}
