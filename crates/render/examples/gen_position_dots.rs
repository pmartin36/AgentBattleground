//! Generates the roster carousel's position-indicator dot assets —
//! `crates/render/src/assets/dot_filled.png`,
//! `crates/render/src/assets/dot_unfilled.png` — first-party asset-based dots
//! (spec `24-roster-carousel.md` line 10: never Unicode bullets). First-party
//! art; this generator IS the provenance/license.
//!
//! Geometry (must match `crates/render/src/assets.rs` `dot_tests`):
//!   - Both images 16×16 RGBA8, transparent ground `Rgba([0,0,0,0])`, hard
//!     alpha edge (no antialiasing).
//!   - Circle membership: center `(8,8)`, radius `r=7`; pixel opaque iff
//!     `(x-8)^2 + (y-8)^2 <= r*r`.
//!   - dot_filled: opaque white body `Rgba([0xff,0xff,0xff,0xff])`.
//!   - dot_unfilled: opaque dim-grey body `Rgba([0x66,0x66,0x66,0xff])` (same
//!     circle geometry; alpha stays 0xff).

use image::{ImageBuffer, Rgba};

const SIZE: u32 = 16;
const TRANSPARENT: Rgba<u8> = Rgba([0, 0, 0, 0]);
const CENTER: i32 = 8;
const RADIUS: i32 = 7;

fn dot_pixel(x: i32, y: i32) -> bool {
    let dx = x - CENTER;
    let dy = y - CENTER;
    dx * dx + dy * dy <= RADIUS * RADIUS
}

fn render(name: &str, path: &str, body: Rgba<u8>) {
    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(SIZE, SIZE, TRANSPARENT);

    for y in 0..SIZE {
        for x in 0..SIZE {
            if dot_pixel(x as i32, y as i32) {
                img.put_pixel(x, y, body);
            }
        }
    }

    img.save(path).unwrap();
    println!("Generated {path} (16x16 RGBA, {name} dot)");
}

fn main() {
    render(
        "filled",
        "crates/render/src/assets/dot_filled.png",
        Rgba([0xff, 0xff, 0xff, 0xff]),
    );
    render(
        "unfilled",
        "crates/render/src/assets/dot_unfilled.png",
        Rgba([0x66, 0x66, 0x66, 0xff]),
    );
}
