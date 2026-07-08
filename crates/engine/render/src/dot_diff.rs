//! Deterministic dot-by-dot buffer decoding/comparison (bucket b4).
//!
//! `decode_braille_cell` is the sole engine-level decoder for a composited
//! braille cell's 8-dot lit/unlit bitmask + shared foreground color — the
//! inverse of `dots.rs::cell_from_dots_tinted`'s `char::from_u32(0x2800 +
//! mask)` encode step. Bit `k` of the returned mask corresponds to
//! `dots.rs`'s `DOTS[k]` (`DOTS[k].2 == k`, standard braille dot order), so
//! no separate table lookup is needed here: the braille codepoint's own bit
//! `k` already equals `DOTS[k]`'s dot by construction.

use ratatui::buffer::Buffer;
use ratatui::style::Color;
use engine_core::color::Rgba;

use crate::dots::DOTS;

/// Decode the braille glyph at `(x, y)` in `buf` into its 8-dot lit/unlit
/// bitmask (bit k set = dot k, same order as `dots.rs`'s `DOTS` table) plus
/// the cell's one shared foreground color.
///
/// Returns `None` for:
/// - an out-of-bounds `(x, y)`,
/// - a non-braille cell (any symbol outside U+2800..=U+28FF, e.g. a blank
///   space cell),
/// - U+2800 itself (a braille glyph with nothing lit).
///
/// A `fg` of anything other than `Color::Rgb` (including `Color::Reset`) is
/// treated as opaque black, matching `grid.rs::draw_grid`'s documented
/// convention. Alpha is always `0xFF` — composited buffers only ever store
/// post-composite RGB.
pub fn decode_braille_cell(buf: &Buffer, x: u16, y: u16) -> Option<(u8, Rgba)> {
    let cell = buf.cell((x, y))?;
    let cp = cell.symbol().chars().next()? as u32;
    if !(0x2800..=0x28FF).contains(&cp) {
        return None;
    }
    let mask = (cp - 0x2800) as u8;
    if mask == 0 {
        return None;
    }
    let color = match cell.fg {
        Color::Rgb(r, g, b) => Rgba::rgb(r, g, b),
        _ => Rgba::rgb(0, 0, 0),
    };
    Some((mask, color))
}

/// One point of divergence between two rendered buffers, at braille-dot
/// granularity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DotMismatch {
    /// `(column, row)` in the buffer.
    pub cell: (u16, u16),
    /// `(dx 0..2, dy 0..4)` within the cell's 2x4 dot block.
    pub dot: (u8, u8),
    pub expected_lit: bool,
    pub actual_lit: bool,
    /// `None` when `expected_lit` is `false`.
    pub expected_color: Option<Rgba>,
    /// `None` when `actual_lit` is `false`.
    pub actual_color: Option<Rgba>,
}

/// The full set of dot-level divergences between two buffers, per `diff_dots`.
#[derive(Clone, Debug)]
pub struct DotDiff {
    pub mismatches: Vec<DotMismatch>,
    pub dots_compared: usize,
}

impl DotDiff {
    /// `true` iff there are no mismatches.
    pub fn is_match(&self) -> bool {
        self.mismatches.is_empty()
    }
}

/// Decode every cell of both buffers via [`decode_braille_cell`] and compare
/// dot-for-dot over their shared `(min width, min height)` region; every dot
/// outside that region on the larger buffer is reported as a mismatch.
pub fn diff_dots(expected: &Buffer, actual: &Buffer) -> DotDiff {
    let (ew, eh) = (expected.area.width, expected.area.height);
    let (aw, ah) = (actual.area.width, actual.area.height);
    let (min_w, min_h) = (ew.min(aw), eh.min(ah));
    let (union_w, union_h) = (ew.max(aw), eh.max(ah));

    let mut mismatches = Vec::new();
    let mut dots_compared = 0usize;

    for cy in 0..union_h {
        for cx in 0..union_w {
            let de = decode_braille_cell(expected, cx, cy);
            let da = decode_braille_cell(actual, cx, cy);
            let in_region = cx < min_w && cy < min_h;

            for &(dx, dy, k) in DOTS.iter() {
                let bit: u8 = 1 << k;
                let e_lit = de.is_some_and(|(m, _)| m & bit != 0);
                let a_lit = da.is_some_and(|(m, _)| m & bit != 0);
                let e_color = e_lit.then(|| de.unwrap().1);
                let a_color = a_lit.then(|| da.unwrap().1);

                if in_region {
                    dots_compared += 1;
                    if e_lit != a_lit || (e_lit && a_lit && e_color != a_color) {
                        mismatches.push(DotMismatch {
                            cell: (cx, cy),
                            dot: (dx as u8, dy as u8),
                            expected_lit: e_lit,
                            actual_lit: a_lit,
                            expected_color: e_color,
                            actual_color: a_color,
                        });
                    }
                } else {
                    mismatches.push(DotMismatch {
                        cell: (cx, cy),
                        dot: (dx as u8, dy as u8),
                        expected_lit: e_lit,
                        actual_lit: a_lit,
                        expected_color: e_color,
                        actual_color: a_color,
                    });
                }
            }
        }
    }

    DotDiff {
        mismatches,
        dots_compared,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dots::DOTS;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use engine_core::color::Rgba;

    fn make_buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    /// Write a braille glyph for `mask` with fg `color` into `buf` at (0,0).
    fn seed_braille_cell(buf: &mut Buffer, mask: u8, color: Rgba) {
        let ch = char::from_u32(0x2800 + mask as u32).expect("valid braille codepoint");
        let cell = buf.cell_mut((0u16, 0u16)).expect("cell (0,0) must exist");
        cell.set_char(ch);
        cell.set_fg(Color::Rgb(color.r, color.g, color.b));
    }

    /// Write a braille glyph for `mask` with fg `color` into `buf` at `(x, y)`.
    fn seed_braille_cell_at(buf: &mut Buffer, x: u16, y: u16, mask: u8, color: Rgba) {
        let ch = char::from_u32(0x2800 + mask as u32).expect("valid braille codepoint");
        let cell = buf.cell_mut((x, y)).expect("cell must exist");
        cell.set_char(ch);
        cell.set_fg(Color::Rgb(color.r, color.g, color.b));
    }

    /// Each of the 8 single-bit masks round-trips: the returned bit equals
    /// `1 << k` and the color matches exactly what was written.
    #[test]
    fn decode_braille_cell_round_trips_each_single_bit() {
        for k in 0..8u8 {
            let mask = 1u8 << k;
            let color = Rgba::rgb(0x10 + k * 10, 0x20, 0x30);
            let mut buf = make_buf(1, 1);
            seed_braille_cell(&mut buf, mask, color);

            let decoded = decode_braille_cell(&buf, 0, 0);
            assert_eq!(
                decoded,
                Some((mask, color)),
                "bit {k} (mask {mask:#04x}) must round-trip exactly"
            );
        }
    }

    /// A combined mask (bits 3, 6, 7 set — 0xC8) round-trips exactly.
    #[test]
    fn decode_braille_cell_round_trips_combined_mask() {
        let mask = 0xC8u8; // bits 3, 6, 7
        let color = Rgba::rgb(0xAB, 0xCD, 0xEF);
        let mut buf = make_buf(1, 1);
        seed_braille_cell(&mut buf, mask, color);

        assert_eq!(decode_braille_cell(&buf, 0, 0), Some((mask, color)));
    }

    /// The fully-lit glyph (U+28FF, mask 0xFF) round-trips exactly.
    #[test]
    fn decode_braille_cell_round_trips_full_mask() {
        let mask = 0xFFu8;
        let color = Rgba::rgb(0x01, 0x02, 0x03);
        let mut buf = make_buf(1, 1);
        seed_braille_cell(&mut buf, mask, color);

        assert_eq!(decode_braille_cell(&buf, 0, 0), Some((mask, color)));
    }

    /// A blank (default space) cell decodes to `None`.
    #[test]
    fn decode_braille_cell_blank_space_is_none() {
        let buf = make_buf(1, 1);
        assert_eq!(decode_braille_cell(&buf, 0, 0), None);
    }

    /// U+2800 (braille glyph, mask 0 — nothing lit) decodes to `None`.
    #[test]
    fn decode_braille_cell_empty_braille_glyph_is_none() {
        let mut buf = make_buf(1, 1);
        let cell = buf.cell_mut((0u16, 0u16)).expect("cell (0,0) must exist");
        cell.set_char('\u{2800}');
        cell.set_fg(Color::Rgb(0x11, 0x22, 0x33));

        assert_eq!(decode_braille_cell(&buf, 0, 0), None);
    }

    /// An out-of-bounds coordinate decodes to `None` rather than panicking.
    #[test]
    fn decode_braille_cell_out_of_bounds_is_none() {
        let buf = make_buf(2, 2);
        assert_eq!(decode_braille_cell(&buf, 50, 50), None);
    }

    /// A non-`Rgb` fg (`Color::Reset`, the buffer's fresh-cell default) is
    /// treated as opaque black, per `grid.rs::draw_grid`'s convention.
    #[test]
    fn decode_braille_cell_reset_fg_treated_as_opaque_black() {
        let mut buf = make_buf(1, 1);
        let ch = char::from_u32(0x2800 + 0x01).unwrap();
        let cell = buf.cell_mut((0u16, 0u16)).expect("cell (0,0) must exist");
        cell.set_char(ch);
        // Fresh cell fg defaults to Color::Reset — not pre-seeded to Rgb.

        assert_eq!(
            decode_braille_cell(&buf, 0, 0),
            Some((0x01, Rgba::rgb(0, 0, 0))),
            "Color::Reset dest must decode as opaque black"
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // diff_dots
    // ─────────────────────────────────────────────────────────────────────

    /// Two identical 2x2-cell buffers produce zero mismatches and count
    /// every dot of every cell as compared.
    #[test]
    fn diff_dots_identical_buffers_no_mismatches() {
        let mut a = make_buf(2, 2);
        let mut b = make_buf(2, 2);
        for y in 0..2u16 {
            for x in 0..2u16 {
                let mask = 0x55u8; // fixed alternating pattern
                let color = Rgba::rgb(10, 20, 30);
                seed_braille_cell_at(&mut a, x, y, mask, color);
                seed_braille_cell_at(&mut b, x, y, mask, color);
            }
        }

        let diff = diff_dots(&a, &b);
        assert!(diff.is_match());
        assert!(diff.mismatches.is_empty());
        assert_eq!(diff.dots_compared, 2 * 2 * 8);
    }

    /// Flipping exactly one dot on one side reports exactly one mismatch, at
    /// the correct cell and dot coordinate, with the correct lit/color sides.
    #[test]
    fn diff_dots_single_altered_dot_reports_exact_cell_and_dot() {
        let color = Rgba::rgb(100, 110, 120);
        let base_mask = 0x00u8; // nothing lit
        let flipped_bit: u8 = 3;
        let altered_mask = base_mask | (1 << flipped_bit);

        let mut expected = make_buf(1, 1);
        let mut actual = make_buf(1, 1);
        seed_braille_cell_at(&mut expected, 0, 0, base_mask, color);
        seed_braille_cell_at(&mut actual, 0, 0, altered_mask, color);

        let diff = diff_dots(&expected, &actual);
        assert!(!diff.is_match());
        assert_eq!(diff.mismatches.len(), 1);

        let m = diff.mismatches[0];
        let (dx, dy, _bit) = DOTS[flipped_bit as usize];
        assert_eq!(m.cell, (0, 0));
        assert_eq!(m.dot, (dx as u8, dy as u8));
        assert!(!m.expected_lit);
        assert!(m.actual_lit);
        assert_eq!(m.expected_color, None);
        assert_eq!(m.actual_color, Some(color));
    }

    /// Same mask on both sides but a different fg color: every LIT dot in
    /// that cell is reported as a mismatch (unlit dots produce none).
    #[test]
    fn diff_dots_altered_cell_color_reports_lit_dots_only() {
        let mask = 0x07u8; // bits 0,1,2 lit — 3 dots
        let expected_color = Rgba::rgb(1, 2, 3);
        let actual_color = Rgba::rgb(9, 8, 7);

        let mut expected = make_buf(1, 1);
        let mut actual = make_buf(1, 1);
        seed_braille_cell_at(&mut expected, 0, 0, mask, expected_color);
        seed_braille_cell_at(&mut actual, 0, 0, mask, actual_color);

        let diff = diff_dots(&expected, &actual);
        assert_eq!(diff.mismatches.len(), 3, "one mismatch per lit dot");
        for m in &diff.mismatches {
            assert_eq!(m.cell, (0, 0));
            assert!(m.expected_lit);
            assert!(m.actual_lit);
            assert_eq!(m.expected_color, Some(expected_color));
            assert_eq!(m.actual_color, Some(actual_color));
        }
    }

    /// A wider `actual` buffer than `expected`: the shared region matches
    /// normally, and every dot of the out-of-region cell is reported as a
    /// mismatch (not counted in `dots_compared`).
    #[test]
    fn diff_dots_larger_buffer_reports_every_out_of_region_dot() {
        let shared_mask = 0xAAu8;
        let shared_color = Rgba::rgb(50, 60, 70);
        let extra_mask = 0xFFu8;
        let extra_color = Rgba::rgb(200, 201, 202);

        let mut expected = make_buf(1, 1);
        let mut actual = make_buf(2, 1);
        seed_braille_cell_at(&mut expected, 0, 0, shared_mask, shared_color);
        seed_braille_cell_at(&mut actual, 0, 0, shared_mask, shared_color);
        seed_braille_cell_at(&mut actual, 1, 0, extra_mask, extra_color);

        let diff = diff_dots(&expected, &actual);
        assert_eq!(diff.dots_compared, 8, "only the shared (0,0) cell is compared");
        assert_eq!(diff.mismatches.len(), 8, "every dot of the out-of-region cell mismatches");
        for m in &diff.mismatches {
            assert_eq!(m.cell, (1, 0));
            assert!(m.actual_lit, "extra_mask 0xFF has every dot lit");
            assert!(!m.expected_lit, "expected side has no cell (1,0)");
            assert_eq!(m.expected_color, None);
            assert_eq!(m.actual_color, Some(extra_color));
        }
    }

    /// `is_match()` reflects emptiness of `mismatches`: true for identical
    /// buffers, false as soon as any dot differs.
    #[test]
    fn diff_dots_is_match_true_only_when_empty() {
        let mut a = make_buf(1, 1);
        let mut b = make_buf(1, 1);
        seed_braille_cell_at(&mut a, 0, 0, 0x03, Rgba::rgb(1, 1, 1));
        seed_braille_cell_at(&mut b, 0, 0, 0x03, Rgba::rgb(1, 1, 1));
        assert!(diff_dots(&a, &b).is_match());

        seed_braille_cell_at(&mut b, 0, 0, 0x07, Rgba::rgb(1, 1, 1));
        assert!(!diff_dots(&a, &b).is_match());
    }
}
