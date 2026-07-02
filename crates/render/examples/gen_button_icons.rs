//! Generates the three braille UI chrome button icons —
//! `crates/render/src/assets/icon_home.png`,
//! `crates/render/src/assets/icon_arrow_left.png`,
//! `crates/render/src/assets/icon_arrow_right.png` — used as first-party
//! icon silhouettes (spec `22-braille-ui-chrome.md`, lines 9-10: no new
//! rasterizer at runtime, no Unicode glyph substitute). First-party art;
//! this generator IS the provenance/license.
//!
//! Geometry (must match `crates/render/src/assets.rs` icon tests):
//!   - All icons 32×32 RGBA8, transparent ground `Rgba([0,0,0,0])`,
//!     opaque-white silhouette `Rgba([0xff,0xff,0xff,0xff])`, hard alpha edge.
//!   - icon_home: house silhouette (roof triangle + body rectangle).
//!     Roof rows y in 4..=13, apex at (16,4), base at row 13; half-width
//!     `hw = ((y - 4) * 12) / 9`; pixel in roof iff `|x - 16| <= hw`.
//!     Body rows y in 14..=26, columns x in 8..=23.
//!   - icon_arrow_right: right-pointing triangle. Columns x in 8..=24;
//!     half-height `hh = ((24 - x) * 10) / 16`; pixel opaque iff
//!     `8 <= x <= 24 && |y - 16| <= hh`.
//!   - icon_arrow_left: left-pointing triangle (mirror). Columns x in
//!     8..=24; half-height `hh = ((x - 8) * 10) / 16`; pixel opaque iff
//!     `8 <= x <= 24 && |y - 16| <= hh`.

use image::{ImageBuffer, Rgba};

const SIZE: u32 = 32;
const TRANSPARENT: Rgba<u8> = Rgba([0, 0, 0, 0]);
const OPAQUE_WHITE: Rgba<u8> = Rgba([0xff, 0xff, 0xff, 0xff]);

fn home_pixel(x: i32, y: i32) -> bool {
    let in_roof = (4..=13).contains(&y) && {
        let hw = ((y - 4) * 12) / 9;
        (x - 16).abs() <= hw
    };
    let in_body = (14..=26).contains(&y) && (8..=23).contains(&x);
    in_roof || in_body
}

fn arrow_right_pixel(x: i32, y: i32) -> bool {
    if !(8..=24).contains(&x) {
        return false;
    }
    let hh = ((24 - x) * 10) / 16;
    (y - 16).abs() <= hh
}

fn arrow_left_pixel(x: i32, y: i32) -> bool {
    if !(8..=24).contains(&x) {
        return false;
    }
    let hh = ((x - 8) * 10) / 16;
    (y - 16).abs() <= hh
}

fn render(name: &str, path: &str, membership: impl Fn(i32, i32) -> bool) {
    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(SIZE, SIZE, TRANSPARENT);

    for y in 0..SIZE {
        for x in 0..SIZE {
            if membership(x as i32, y as i32) {
                img.put_pixel(x, y, OPAQUE_WHITE);
            }
        }
    }

    img.save(path).unwrap();
    println!("Generated {path} (32x32 RGBA, {name} silhouette)");
}

fn main() {
    render(
        "home",
        "crates/render/src/assets/icon_home.png",
        home_pixel,
    );
    render(
        "arrow-left",
        "crates/render/src/assets/icon_arrow_left.png",
        arrow_left_pixel,
    );
    render(
        "arrow-right",
        "crates/render/src/assets/icon_arrow_right.png",
        arrow_right_pixel,
    );
}
