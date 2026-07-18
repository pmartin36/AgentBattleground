//! Shared synthetic PNG-byte fixtures used by every `Button` test module, in
//! place of the real bundled assets that used to live in `crate::assets`
//! (now removed — see b1-t2 research.md). Each preserves the exact alpha
//! structure the moved-to-`game` real assets have, so the assertions that
//! depend on that structure (icon/panel contrast, frame glyph-mask
//! invariance) stay meaningful instead of passing vacuously against a
//! featureless fixture.
//!
//! Split out of `button_tests.rs` (b1-t1) into its own sibling file so every
//! concern-partitioned test module can share one copy instead of
//! reimplementing these fixtures.

#[cfg(test)]
pub(crate) fn encode_png(img: image::RgbaImage) -> Vec<u8> {
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("synthetic test fixture must encode to PNG");
    buf
}

/// Opaque-white rounded-rect on a transparent ground, alpha-transparent
/// corners — the exact geometry `examples/gen_button_panel.rs` uses to
/// generate the real `BUTTON_PANEL`. Load-bearing for every render test that
/// needs an opaque panel body to actually paint (`render_center_fg`,
/// `render_tints_differ_across_all_three_states`, the glyph-mask tests).
#[cfg(test)]
pub(crate) fn rounded_rect_png(w: u32, h: u32, r: i32) -> Vec<u8> {
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    let x_lo = r;
    let x_hi = w as i32 - 1 - r;
    let y_lo = r;
    let y_hi = h as i32 - 1 - r;
    for y in 0..h {
        for x in 0..w {
            let (xi, yi) = (x as i32, y as i32);
            let cx = xi.clamp(x_lo, x_hi);
            let cy = yi.clamp(y_lo, y_hi);
            let (dx, dy) = (xi - cx, yi - cy);
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
    }
    encode_png(img)
}

/// Hollow opaque ring on a transparent ground (opaque border, transparent
/// interior AND corners) — the exact geometry `examples/gen_frame_panel.rs`
/// uses to generate the real `FRAME_PANEL`. Load-bearing for
/// `frame_button_glyph_mask_invariant_across_states`: the mask-flip
/// regression this guards only has teeth against real per-dot alpha
/// structure (opaque ring + transparent interior) — a solid-fill synthetic
/// would make the invariance assertion trivially true regardless of whether
/// the underlying bug were present.
#[cfg(test)]
pub(crate) fn hollow_ring_png(w: u32, h: u32, r: i32, border: i32) -> Vec<u8> {
    let ri = r - border;
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    let x_lo = r;
    let x_hi = w as i32 - 1 - r;
    let y_lo = r;
    let y_hi = h as i32 - 1 - r;
    for y in 0..h {
        for x in 0..w {
            let (xi, yi) = (x as i32, y as i32);
            let cx = xi.clamp(x_lo, x_hi);
            let cy = yi.clamp(y_lo, y_hi);
            let (dx, dy) = (xi - cx, yi - cy);
            let dist_sq = dx * dx + dy * dy;
            if dist_sq <= r * r && dist_sq > ri * ri {
                img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
    }
    encode_png(img)
}

/// Opaque-white silhouette blob inset from every edge (transparent margin,
/// including all four corners) on a transparent ground — mirrors every real
/// bundled icon's own transparent corners. Load-bearing for
/// `icon_is_darker_than_panel_only_cell`: the icon's own alpha must NOT
/// reach its corner pixel, so a real panel-only edge cell survives at the
/// button rect's corner to contrast against (an icon that fills its whole
/// rect erases that cell and makes the contrast assertion pass trivially).
#[cfg(test)]
pub(crate) fn inset_blob_png(w: u32, h: u32, inset: u32) -> Vec<u8> {
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    for y in inset..(h - inset) {
        for x in inset..(w - inset) {
            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
    }
    encode_png(img)
}

/// Leak `bytes` to `'static`, giving each call a fresh, distinct pointer —
/// `Button::new` takes `&'static [u8]` (asset_cache is keyed on
/// `bytes.as_ptr()`), so every in-file test fixture call must yield its
/// own stable address, preserving per-test cache isolation.
#[cfg(test)]
pub(crate) fn leak_png(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

/// Synthetic stand-in for `game::assets::BUTTON_PANEL` (64x32, radius 8).
#[cfg(test)]
pub(crate) fn panel_bytes() -> &'static [u8] {
    leak_png(rounded_rect_png(64, 32, 8))
}

/// Synthetic stand-in for `game::assets::FRAME_PANEL` (64x32, radius 8,
/// border 6 — same params `gen_frame_panel.rs` uses for the real asset).
#[cfg(test)]
pub(crate) fn frame_bytes() -> &'static [u8] {
    leak_png(hollow_ring_png(64, 32, 8, 6))
}

/// Synthetic stand-in for a bundled icon (48x48, inset 12 — leaves an
/// 8px-per-side transparent corner margin, comfortably surviving the
/// Lanczos3 resize down to any of this file's tested rect sizes).
#[cfg(test)]
pub(crate) fn icon_bytes() -> &'static [u8] {
    leak_png(inset_blob_png(48, 48, 12))
}
