//! Background-removal seam: separates a subject from its solid key-color
//! screen and emits an RGBA image/frame with the background (and any baked,
//! same-hue drop-shadow) made transparent. Reserved as a swappable method
//! behind `BackgroundRemover`; `ChromaDespill` is the dependency-light
//! default. `remove_still_background` / `remove_frame_background` are the
//! sole call sites both the image and animation operations use.

use image::RgbaImage;

use super::types::KeyColor;

/// Default key-hue spill cutoff (0..255 scale) above which a pixel is
/// treated as background.
pub const DEFAULT_BG_CUT: i32 = 64;

/// The reserved background-removal seam. A still cutout and a per-frame
/// cutout are distinct entry points because the two may diverge under a
/// future method; `remove_frame` defaults to `remove_still`.
pub trait BackgroundRemover {
    fn remove_still(&self, image: &RgbaImage, key: KeyColor) -> RgbaImage;

    fn remove_frame(&self, image: &RgbaImage, key: KeyColor) -> RgbaImage {
        self.remove_still(image, key)
    }
}

/// Dependency-light chroma-key + despill default: keys out the flat field
/// (and any darkened, same-hue drop-shadow band) and despills anti-aliased
/// border pixels rather than only cutting a hard edge.
#[derive(Clone, Copy, Debug)]
pub struct ChromaDespill {
    pub bg_cut: i32,
}

impl Default for ChromaDespill {
    fn default() -> Self {
        Self {
            bg_cut: DEFAULT_BG_CUT,
        }
    }
}

impl BackgroundRemover for ChromaDespill {
    fn remove_still(&self, image: &RgbaImage, key: KeyColor) -> RgbaImage {
        let strong = strong_channels(&key);
        let mut out = image.clone();
        for px in out.pixels_mut() {
            let mut c = px.0;
            if c[3] < 128 {
                px.0 = [0, 0, 0, 0];
                continue;
            }
            let (spill, weak_max) = spill_and_weak_max(c, strong);
            if spill >= self.bg_cut {
                px.0 = [0, 0, 0, 0];
            } else if spill > 0 {
                let weak_max = weak_max as u8;
                for (i, is_strong) in strong.iter().enumerate() {
                    if *is_strong {
                        c[i] = c[i].min(weak_max);
                    }
                }
                c[3] = 255;
                px.0 = c;
            }
        }
        out
    }
}

/// Splits `key`'s RGB channels into STRONG (above the key's own mean value)
/// and WEAK (the rest): a green key yields `strong = [false, true, false]`,
/// a magenta key yields `strong = [true, false, true]`.
fn strong_channels(key: &KeyColor) -> [bool; 3] {
    let mean = (key.r as u32 + key.g as u32 + key.b as u32) / 3;
    [
        key.r as u32 > mean,
        key.g as u32 > mean,
        key.b as u32 > mean,
    ]
}

/// Computes a pixel's key-hue spill (the minimum STRONG channel minus the
/// maximum WEAK channel) alongside the WEAK max used to despill an edge
/// pixel back down to zero measurable spill.
fn spill_and_weak_max(px: [u8; 4], strong: [bool; 3]) -> (i32, i32) {
    let channels = [px[0] as i32, px[1] as i32, px[2] as i32];
    let mut strong_min = i32::MAX;
    let mut weak_max = i32::MIN;
    for (i, is_strong) in strong.iter().enumerate() {
        if *is_strong {
            strong_min = strong_min.min(channels[i]);
        } else {
            weak_max = weak_max.max(channels[i]);
        }
    }
    (strong_min - weak_max, weak_max)
}

/// Removes the background from a still image against `key`. The sole call
/// site the image-generation path uses.
pub fn remove_still_background(image: &RgbaImage, key: KeyColor) -> RgbaImage {
    ChromaDespill::default().remove_still(image, key)
}

/// Removes the background from one animation frame against `key`. The sole
/// call site the animation-generation path uses (both for the pre-clean
/// still pass and each output frame).
pub fn remove_frame_background(image: &RgbaImage, key: KeyColor) -> RgbaImage {
    ChromaDespill::default().remove_frame(image, key)
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    const GREEN: KeyColor = KeyColor { r: 0, g: 255, b: 0 };
    const MAGENTA: KeyColor = KeyColor {
        r: 255,
        g: 0,
        b: 255,
    };

    fn solid_with_center(w: u32, h: u32, field: [u8; 4], center: [u8; 4]) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, h, Rgba(field));
        img.put_pixel(w / 2, h / 2, Rgba(center));
        img
    }

    fn single_pixel(color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(1, 1, Rgba(color))
    }

    /// A solid green key field is made fully transparent; a retained
    /// subject pixel keeps its color and stays opaque.
    #[test]
    fn green_field_becomes_transparent() {
        let img = solid_with_center(4, 4, [0, 255, 0, 255], [0, 0, 255, 255]);
        let out = remove_still_background(&img, GREEN);

        let key_px = out.get_pixel(0, 0).0;
        assert_eq!(key_px[3], 0, "key-color field must be fully transparent");

        let subject_px = out.get_pixel(2, 2).0;
        assert_eq!(
            subject_px,
            [0, 0, 255, 255],
            "retained subject pixel keeps its color and opacity"
        );
    }

    /// A solid magenta key field is made fully transparent; a green-family
    /// subject retained against it keeps its color and stays opaque.
    #[test]
    fn magenta_field_becomes_transparent() {
        let img = solid_with_center(4, 4, [255, 0, 255, 255], [0, 255, 0, 255]);
        let out = remove_still_background(&img, MAGENTA);

        let key_px = out.get_pixel(0, 0).0;
        assert_eq!(key_px[3], 0, "key-color field must be fully transparent");

        let subject_px = out.get_pixel(2, 2).0;
        assert_eq!(
            subject_px,
            [0, 255, 0, 255],
            "retained green-family subject keeps its color and opacity"
        );
    }

    /// An anti-aliased border pixel (moderate key-hue spill, below the
    /// background cutoff) is despilled rather than cut: it stays opaque and
    /// its output spill is no longer measurably positive.
    #[test]
    fn border_spill_despilled() {
        let weak_max: i32 = 30;
        let spill: i32 = DEFAULT_BG_CUT / 2;
        let g = (weak_max + spill) as u8;
        let img = single_pixel([weak_max as u8, g, weak_max as u8, 255]);

        let out = remove_still_background(&img, GREEN);
        let px = out.get_pixel(0, 0).0;

        assert_eq!(px[3], 255, "edge pixel below the cutoff stays opaque");
        let out_spill = px[1] as i32 - px[0].max(px[2]) as i32;
        assert!(
            out_spill <= 0,
            "despilled pixel must carry no measurable key-color spill, got spill {out_spill}"
        );
    }

    /// A darkened-key drop-shadow band (same hue, lower value) is keyed out
    /// exactly like the bright field, without a separate shadow pass.
    #[test]
    fn baked_shadow_removed() {
        let img = single_pixel([0, 90, 0, 255]);
        let out = remove_still_background(&img, GREEN);
        let px = out.get_pixel(0, 0).0;
        assert_eq!(px[3], 0, "darkened same-hue shadow band must be keyed out");
    }

    /// A white subject pixel under a green key is not over-keyed: it has no
    /// positive key-hue spill and must be retained unchanged.
    #[test]
    fn subject_white_not_keyed() {
        let img = single_pixel([255, 255, 255, 255]);
        let out = remove_still_background(&img, GREEN);
        let px = out.get_pixel(0, 0).0;
        assert_eq!(px, [255, 255, 255, 255], "white subject must be retained unchanged");
    }

    /// A yellow subject pixel under a green key is not over-keyed: it has
    /// no positive key-hue spill and must be retained unchanged.
    #[test]
    fn subject_yellow_not_keyed() {
        let img = single_pixel([255, 255, 0, 255]);
        let out = remove_still_background(&img, GREEN);
        let px = out.get_pixel(0, 0).0;
        assert_eq!(px, [255, 255, 0, 255], "yellow subject must be retained unchanged");
    }

    /// `ChromaDespill`'s `remove_frame` uses the shared default (identical
    /// to `remove_still`), proving the seam's default wiring is real.
    #[test]
    fn remove_frame_matches_still_default() {
        let img = solid_with_center(2, 2, [0, 255, 0, 255], [0, 0, 255, 255]);
        let remover = ChromaDespill::default();
        assert_eq!(
            remover.remove_frame(&img, GREEN),
            remover.remove_still(&img, GREEN),
            "remove_frame must default to remove_still"
        );
    }
}
