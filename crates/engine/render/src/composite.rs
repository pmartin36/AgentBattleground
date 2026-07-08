//! Depth-sorted compositor: blits multiple positioned `DotBuffer`s into one.

use crate::anim::AnimatedSprite;
use crate::camera::{Camera, WorldPos};
use crate::dots::{dots_to_grid_tinted, tint, Dot, DotBuffer};
use crate::grid::Grid;
use crate::transform::{place, Transform, VerticalAnchor};
use engine_core::color::Rgba;
use std::time::Duration;

/// A dot-buffer positioned at `(dot_x, dot_y)` in the destination, with a
/// depth-sort key. Negative offsets are allowed (dots that fall outside the
/// destination are clipped).
#[derive(Clone, Copy, Debug)]
pub struct DotPlacement<'a> {
    pub dots: &'a DotBuffer,
    pub dot_x: i32,
    pub dot_y: i32,
    /// Back-to-front sort key (larger = nearer = drawn on top). Ascending =
    /// far first; equal depths keep input order (stable sort).
    pub depth: i32,
}

/// Composite dot buffers into a fresh `dot_cols`×`dot_rows` `DotBuffer`,
/// back-to-front by `depth` ascending (far first; larger depth drawn last =
/// on top).
///
/// - `Lit` dots overwrite whatever a farther placement wrote.
/// - `Transparent` dots are skipped so farther dots show through gaps.
/// - Dots landing outside `[0, dot_cols) × [0, dot_rows)` are silently clipped.
/// - Equal-depth placements keep input order (stable sort).
pub fn composite_dots(
    dot_cols: usize,
    dot_rows: usize,
    placements: &[DotPlacement],
) -> DotBuffer {
    let mut out = DotBuffer::new(dot_cols, dot_rows);
    let mut ordered: Vec<DotPlacement> = placements.to_vec();
    ordered.sort_by_key(|p| p.depth); // stable ascending: far (low depth) first
    for p in &ordered {
        for r in 0..p.dots.rows() {
            for c in 0..p.dots.cols() {
                let dot = p.dots.get(c, r);
                if let Dot::Lit(src) = dot {
                    let dx = p.dot_x + c as i32;
                    let dy = p.dot_y + r as i32;
                    if dx < 0 || dy < 0 {
                        continue;
                    }
                    if src.a == 0xFF {
                        // Byte-identical to today: hard overwrite, relying on
                        // `DotBuffer::set`'s silent upper-bound clip.
                        out.set(dx as usize, dy as usize, dot);
                    } else {
                        // Translucent: must read the destination, so this
                        // needs a full bounds check (`get` panics OOB).
                        let (ux, uy) = (dx as usize, dy as usize);
                        if ux >= out.cols() || uy >= out.rows() {
                            continue;
                        }
                        let blended = match out.get(ux, uy) {
                            Dot::Lit(dest) => src.over(dest),
                            Dot::Transparent => src,
                        };
                        out.set(ux, uy, Dot::Lit(blended));
                    }
                }
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// composite_scene (b1-t1, spec 33)
// ─────────────────────────────────────────────────────────────────────────────

/// The rasterizable content of one [`SpriteDraw`]. `Animated` mirrors
/// `piece_shape_and_color`'s inputs: sprite + elapsed + transform +
/// base_dot_rows. `Prerasterized` (b7-t1: contact shadow) carries an
/// already-built `DotBuffer` (e.g. from `rasterize_shape`) placed as-is, with
/// no sprite/transform rasterization step — additive: every existing
/// `Animated` construction site is unaffected.
pub enum SpriteContent<'a> {
    Animated {
        sprite: &'a AnimatedSprite,
        elapsed: Duration,
        transform: Transform,
        base_dot_rows: u32,
    },
    Prerasterized(&'a DotBuffer),
}

/// One sprite to composite: what to rasterize (`content`), where to place it
/// (`translate`, a world position — depth is derived from
/// `camera.depth_key(translate)` inside `composite_scene`, not stored here),
/// and an optional multiply-blend tint (`None` reuses the raw buffer for both
/// the shape and color composite passes).
pub struct SpriteDraw<'a> {
    pub content: SpriteContent<'a>,
    pub translate: WorldPos,
    pub tint: Option<Rgba>,
    pub vertical_anchor: VerticalAnchor,
}

/// Composite every `draws` entry into one `dot_cols`×`dot_rows` [`Grid`],
/// back-to-front by `camera.depth_key(translate)`.
///
/// For each draw: rasterize per `content`; if `tint` is `Some(c)`, produce a
/// separate tinted color buffer via [`crate::dots::tint`], else reuse the raw
/// buffer for both the shape and color composite passes (the glyph mask is
/// always decided from the untinted/shape buffer — tint never changes which
/// dots are lit, only their RGB). Placement borrows [`place`], so depth comes
/// from the shared `camera`, not an explicit field.
pub fn composite_scene<C: Camera>(
    dot_cols: usize,
    dot_rows: usize,
    camera: &C,
    draws: &[SpriteDraw],
) -> Grid {
    let mut raws: Vec<DotBuffer> = Vec::with_capacity(draws.len());
    let mut tints: Vec<Option<DotBuffer>> = Vec::with_capacity(draws.len());
    for d in draws {
        let raw = match &d.content {
            SpriteContent::Animated {
                sprite,
                elapsed,
                transform,
                base_dot_rows,
            } => sprite.rasterize_at(*elapsed, transform, *base_dot_rows),
            SpriteContent::Prerasterized(buf) => (*buf).clone(),
        };
        let tinted = d.tint.map(|c| tint(&raw, c));
        raws.push(raw);
        tints.push(tinted);
    }

    let mut shape_p = Vec::with_capacity(draws.len());
    let mut color_p = Vec::with_capacity(draws.len());
    for (i, d) in draws.iter().enumerate() {
        let raw = &raws[i];
        let color_buf = tints[i].as_ref().unwrap_or(raw);
        shape_p.push(place(raw, d.translate, camera, d.vertical_anchor));
        color_p.push(place(color_buf, d.translate, camera, d.vertical_anchor));
    }

    let shape = composite_dots(dot_cols, dot_rows, &shape_p);
    let color = composite_dots(dot_cols, dot_rows, &color_p);
    dots_to_grid_tinted(&shape, &color)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::SideView;
    use crate::dots::{dots_to_grid, Dot, DotBuffer};
    use crate::grid::Cell;
    use engine_core::color::Rgba;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};

    // ── composite_dots tests ──────────────────────────────────────────────────

    /// A near (higher depth) `Lit` dot over a far `Lit` dot at the same
    /// destination position must yield the near dot.
    ///
    /// Setup: 1×1 output.
    ///   far  placement: depth=0, dot_x=0, dot_y=0, Lit(color_a).
    ///   near placement: depth=1, dot_x=0, dot_y=0, Lit(color_b).
    /// Expected: out dot (0,0) == Lit(color_b).
    #[test]
    fn composite_dots_near_lit_over_far_wins() {
        let color_a = Rgba::rgb(255, 0, 0);
        let color_b = Rgba::rgb(0, 255, 0);
        let mut far_buf = DotBuffer::new(1, 1);
        far_buf.set(0, 0, Dot::Lit(color_a));
        let mut near_buf = DotBuffer::new(1, 1);
        near_buf.set(0, 0, Dot::Lit(color_b));

        let placements = [
            DotPlacement { dots: &far_buf,  dot_x: 0, dot_y: 0, depth: 0 },
            DotPlacement { dots: &near_buf, dot_x: 0, dot_y: 0, depth: 1 },
        ];
        let out = composite_dots(1, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            Dot::Lit(color_b),
            "near (depth=1) Lit dot must overwrite far (depth=0) Lit dot"
        );
    }

    /// Where the near buffer is `Transparent` at a position the far buffer
    /// has `Lit`, the far dot must show through (not be overwritten).
    ///
    /// Setup: 2×1 output.
    ///   far  placement: depth=0, both dots Lit(color_a).
    ///   near placement: depth=1, dot (0,0)=Lit(color_b), dot (1,0)=Transparent.
    /// Expected: (0,0)==Lit(color_b), (1,0)==Lit(color_a).
    #[test]
    fn composite_dots_transparent_reveals_far() {
        let color_a = Rgba::rgb(255, 0, 0);
        let color_b = Rgba::rgb(0, 255, 0);
        let mut far_buf = DotBuffer::new(2, 1);
        far_buf.set(0, 0, Dot::Lit(color_a));
        far_buf.set(1, 0, Dot::Lit(color_a));
        let mut near_buf = DotBuffer::new(2, 1);
        near_buf.set(0, 0, Dot::Lit(color_b));
        // (1, 0) stays Dot::Transparent

        let placements = [
            DotPlacement { dots: &far_buf,  dot_x: 0, dot_y: 0, depth: 0 },
            DotPlacement { dots: &near_buf, dot_x: 0, dot_y: 0, depth: 1 },
        ];
        let out = composite_dots(2, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            Dot::Lit(color_b),
            "near Lit at (0,0) must overwrite far"
        );
        assert_eq!(
            out.get(1, 0),
            Dot::Lit(color_a),
            "near Transparent at (1,0) must allow far Lit to show through"
        );
    }

    /// Swapping depths must flip the winner — depth, not input order, decides.
    ///
    /// Setup: 1×1 output.
    ///   placement A: depth=1 (near), Lit(color_a).
    ///   placement B: depth=0 (far),  Lit(color_b).
    /// Expected: out (0,0) == Lit(color_a) (A is nearer).
    #[test]
    fn composite_dots_swap_depth_flips_winner() {
        let color_a = Rgba::rgb(255, 0, 0);
        let color_b = Rgba::rgb(0, 0, 255);
        let mut buf_a = DotBuffer::new(1, 1);
        buf_a.set(0, 0, Dot::Lit(color_a));
        let mut buf_b = DotBuffer::new(1, 1);
        buf_b.set(0, 0, Dot::Lit(color_b));

        // A is near (depth=1), B is far (depth=0); A wins.
        let placements = [
            DotPlacement { dots: &buf_a, dot_x: 0, dot_y: 0, depth: 1 },
            DotPlacement { dots: &buf_b, dot_x: 0, dot_y: 0, depth: 0 },
        ];
        let out = composite_dots(1, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            Dot::Lit(color_a),
            "color_a (depth=1, near) must win over color_b (depth=0, far)"
        );
    }

    /// A 1-dot `Lit` source buffer placed at `dot_x=1` in a 2-wide output
    /// must land at dot (1,0) — the sub-cell offset point.
    /// Dot (0,0) must remain `Transparent`.
    #[test]
    fn composite_dots_subcell_offset_places_one_dot_over() {
        let color = Rgba::rgb(100, 150, 200);
        let mut src = DotBuffer::new(1, 1);
        src.set(0, 0, Dot::Lit(color));

        let placements = [
            DotPlacement { dots: &src, dot_x: 1, dot_y: 0, depth: 0 },
        ];
        let out = composite_dots(2, 1, &placements);

        assert_eq!(
            out.get(0, 0),
            Dot::Transparent,
            "dot (0,0) must be Transparent — source placed at dot_x=1"
        );
        assert_eq!(
            out.get(1, 0),
            Dot::Lit(color),
            "dot (1,0) must be Lit — the source dot lands here via +1 offset"
        );
    }

    /// Out-of-bounds placements (negative x, negative y, beyond the far edge)
    /// must not panic; only in-bounds dots are written; everything else stays
    /// `Transparent`.
    #[test]
    fn composite_dots_out_of_bounds_no_panic() {
        let color = Rgba::rgb(10, 20, 30);
        let mut src = DotBuffer::new(1, 1);
        src.set(0, 0, Dot::Lit(color));

        let placements = [
            // Negative x → dot lands at x=-1, clipped.
            DotPlacement { dots: &src, dot_x: -1, dot_y: 0, depth: 0 },
            // Beyond dot_cols (2) → lands at x=3, clipped.
            DotPlacement { dots: &src, dot_x: 3, dot_y: 0, depth: 0 },
            // Negative y → dot lands at y=-1, clipped.
            DotPlacement { dots: &src, dot_x: 0, dot_y: -1, depth: 0 },
        ];
        // Must not panic.
        let out = composite_dots(2, 2, &placements);

        // All placements are out of bounds; entire output must be Transparent.
        for row in 0..2 {
            for col in 0..2 {
                assert_eq!(
                    out.get(col, row),
                    Dot::Transparent,
                    "dot ({col},{row}) must be Transparent — all placements were OOB"
                );
            }
        }
    }

    /// A translucent (`a < 0xFF`) `Lit` dot placed over an existing far `Lit`
    /// dot must alpha-blend via `Rgba::over`, not hard-overwrite it — the
    /// load-bearing change this task (b1-t2, spec Decision 2) adds on top of
    /// b1-t1's blend helper.
    ///
    /// Setup: 1×1 output.
    ///   far  placement: depth=0, Lit(opaque red).
    ///   near placement: depth=1, Lit(translucent green, a=0x80).
    /// Expected: out dot (0,0) == Lit(near_color.over(far_color)) — the exact
    /// per-channel blend, not the raw translucent source color.
    #[test]
    fn composite_dots_translucent_blends_over_far_lit() {
        let far_color = Rgba::rgb(255, 0, 0);
        let near_color = Rgba::new(0, 255, 0, 0x80);
        let mut far_buf = DotBuffer::new(1, 1);
        far_buf.set(0, 0, Dot::Lit(far_color));
        let mut near_buf = DotBuffer::new(1, 1);
        near_buf.set(0, 0, Dot::Lit(near_color));

        let placements = [
            DotPlacement { dots: &far_buf, dot_x: 0, dot_y: 0, depth: 0 },
            DotPlacement { dots: &near_buf, dot_x: 0, dot_y: 0, depth: 1 },
        ];
        let out = composite_dots(1, 1, &placements);

        let expected = near_color.over(far_color);
        assert_ne!(
            expected, near_color,
            "sanity: a mid-alpha blend must differ from the raw translucent source color"
        );
        assert_eq!(
            out.get(0, 0),
            Dot::Lit(expected),
            "translucent near dot must alpha-blend over the far opaque dot via Rgba::over, not hard-overwrite it"
        );
    }

    // ── composite_scene tests (b1-t1, spec 33) ────────────────────────────────

    /// Build a uniform RGBA image where every pixel is `(r, g, b, a)`.
    fn solid_image(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
        let mut raw = RgbaImage::new(w, h);
        for p in raw.pixels_mut() {
            *p = PixelRgba([r, g, b, a]);
        }
        DynamicImage::from(raw)
    }

    /// An image opaque on the left half, fully transparent on the right half.
    /// Source width == `dot_cols` so the resize is an identity (mirrors
    /// `transform.rs`'s test helper of the same name).
    fn half_opaque_image(dot_cols: u32, dot_rows: u32, r: u8, g: u8, b: u8) -> DynamicImage {
        let mut raw = RgbaImage::new(dot_cols, dot_rows);
        for y in 0..dot_rows {
            for x in 0..dot_cols {
                let a = if x < dot_cols / 2 { 255u8 } else { 0u8 };
                raw.put_pixel(x, y, PixelRgba([r, g, b, a]));
            }
        }
        DynamicImage::from(raw)
    }

    /// Hand multiply-blend one channel, matching `dots::tint`'s formula:
    /// `floor(src * c / 255)`.
    fn mul(src: u8, c: u8) -> u8 {
        ((src as u16 * c as u16) / 255) as u8
    }

    /// Test-only camera that decouples `project` (fixed) from `depth_key`
    /// (`pos.x`) — needed to place two draws at the *same* screen position
    /// with *different* depths, which `SideView` cannot do (it couples both
    /// to `pos.y`).
    struct FixedProjectCamera {
        project: (i32, i32),
    }

    impl Camera for FixedProjectCamera {
        fn project(&self, _pos: WorldPos) -> (i32, i32) {
            self.project
        }
        fn depth_key(&self, pos: WorldPos) -> i32 {
            pos.x as i32
        }
    }

    /// Extract `(ch-agnostic) color` from a `Cell::Glyph`, panicking with a
    /// clear message if the cell is unexpectedly `Transparent`.
    fn glyph_color(cell: Cell) -> Rgba {
        match cell {
            Cell::Glyph { color, .. } => color,
            Cell::Transparent => panic!("expected Cell::Glyph, got Cell::Transparent"),
        }
    }

    /// Two overlapping `Animated` draws at distinct depths (via
    /// `FixedProjectCamera`, so both project to the identical screen dot)
    /// must composite nearer-wins: the higher-depth (blue) draw must be what
    /// ends up in the output, matching `composite_dots_near_lit_over_far_wins`
    /// at the `composite_scene` level.
    #[test]
    fn composite_scene_depth_orders_back_to_front() {
        let far_sprite =
            AnimatedSprite::new(vec![solid_image(2, 4, 255, 0, 0, 255)], Duration::from_millis(100));
        let near_sprite =
            AnimatedSprite::new(vec![solid_image(2, 4, 0, 0, 255, 255)], Duration::from_millis(100));
        let transform = Transform::default();
        let camera = FixedProjectCamera { project: (1, 2) };

        let draws = [
            SpriteDraw {
                content: SpriteContent::Animated {
                    sprite: &far_sprite,
                    elapsed: Duration::ZERO,
                    transform,
                    base_dot_rows: 4,
                },
                translate: WorldPos::new(0.0, 0.0), // depth 0 (far)
                tint: None,
                vertical_anchor: VerticalAnchor::Center,
            },
            SpriteDraw {
                content: SpriteContent::Animated {
                    sprite: &near_sprite,
                    elapsed: Duration::ZERO,
                    transform,
                    base_dot_rows: 4,
                },
                translate: WorldPos::new(5.0, 0.0), // depth 5 (near)
                tint: None,
                vertical_anchor: VerticalAnchor::Center,
            },
        ];

        let grid = composite_scene(2, 4, &camera, &draws);

        assert_eq!(
            glyph_color(grid.get(0, 0)),
            Rgba::rgb(0, 0, 255),
            "nearer (depth=5, blue) draw must win over farther (depth=0, red) draw"
        );
    }

    /// A `tint: Some(c)` draw over a uniform fully-opaque gray (200,200,200)
    /// source must multiply-blend every lit cell's color to the hand-derived
    /// value, and produce the identical (fully-lit) glyph topology for two
    /// different tint colors — the mask never depends on the tint value.
    /// Ports `piece_dots_tints_each_team_distinctly_via_multiply_blend` +
    /// `piece_shape_and_color_untinted_carries_raw_source_rgb`.
    #[test]
    fn composite_scene_tint_multiplies_and_mask_invariant() {
        let sprite =
            AnimatedSprite::new(vec![solid_image(4, 8, 200, 200, 200, 255)], Duration::from_millis(100));
        let transform = Transform::default();
        let camera = FixedProjectCamera { project: (2, 4) };

        let tint_a = Rgba::rgb(255, 232, 176);
        let expected_a = Rgba::rgb(mul(200, 255), mul(200, 232), mul(200, 176));
        let tint_b = Rgba::rgb(176, 255, 224);
        let expected_b = Rgba::rgb(mul(200, 176), mul(200, 255), mul(200, 224));
        assert_ne!(expected_a, expected_b, "sanity: the two hand-derived tints must differ");

        for (c, expected) in [(tint_a, expected_a), (tint_b, expected_b)] {
            let draws = [SpriteDraw {
                content: SpriteContent::Animated {
                    sprite: &sprite,
                    elapsed: Duration::ZERO,
                    transform,
                    base_dot_rows: 8,
                },
                translate: WorldPos::new(0.0, 0.0),
                tint: Some(c),
                vertical_anchor: VerticalAnchor::Center,
            }];
            let grid = composite_scene(4, 8, &camera, &draws);

            assert_eq!(grid.cols(), 2, "canvas 4 dots wide -> 2 cell columns");
            assert_eq!(grid.rows(), 2, "canvas 8 dots tall -> 2 cell rows");
            for row in 0..grid.rows() {
                for col in 0..grid.cols() {
                    assert_eq!(
                        glyph_color(grid.get(col, row)),
                        expected,
                        "cell ({col},{row}) must equal the multiply-blend of (200,200,200) with tint {c:?}"
                    );
                }
            }
        }
    }

    /// For a `tint: Some(c)` draw over a half-opaque/half-transparent source,
    /// the resulting glyph topology (which cells are `Glyph` vs
    /// `Transparent`) must be identical for two different tint values — tint
    /// changes color only, never which dots the shape buffer marks lit.
    /// Ports `piece_shape_and_color_topology_parity`.
    #[test]
    fn composite_scene_shape_color_topology_parity() {
        let camera = FixedProjectCamera { project: (2, 4) };
        let transform = Transform::default();

        let mut topologies = Vec::new();
        for c in [Rgba::rgb(255, 232, 176), Rgba::rgb(176, 255, 224)] {
            let sprite = AnimatedSprite::new(
                vec![half_opaque_image(4, 8, 10, 20, 30)],
                Duration::from_millis(100),
            );
            let draws = [SpriteDraw {
                content: SpriteContent::Animated {
                    sprite: &sprite,
                    elapsed: Duration::ZERO,
                    transform,
                    base_dot_rows: 8,
                },
                translate: WorldPos::new(0.0, 0.0),
                tint: Some(c),
                vertical_anchor: VerticalAnchor::Center,
            }];
            let grid = composite_scene(4, 8, &camera, &draws);

            let topology: Vec<bool> = (0..grid.rows())
                .flat_map(|row| (0..grid.cols()).map(move |col| (col, row)))
                .map(|(col, row)| grid.get(col, row) != Cell::Transparent)
                .collect();
            topologies.push(topology);
        }

        assert_eq!(
            topologies[0], topologies[1],
            "glyph Transparent/Glyph topology must be identical across different tint values"
        );
        // Left column (source opaque half) must be Glyph; right column
        // (source transparent half) must stay Transparent.
        assert!(topologies[0][0], "cell (0,0) over the opaque half must be Glyph");
        assert!(!topologies[0][1], "cell (1,0) over the transparent half must be Transparent");
    }

    /// Two non-overlapping draws sharing the same `content` but different
    /// per-draw `tint` values must each carry their own tinted color in their
    /// own output region — the tint is read from `SpriteDraw::tint`, not a
    /// shared/cached default. Adapts `piece_dots_reads_piece_color_field_not_team_default`'s
    /// intent (no `Piece`/`piece.color` at this layer).
    #[test]
    fn composite_scene_reads_per_draw_tint_not_shared() {
        let sprite =
            AnimatedSprite::new(vec![solid_image(4, 8, 200, 200, 200, 255)], Duration::from_millis(100));
        let transform = Transform::default();
        let camera = SideView::new(1.0);

        let tint_a = Rgba::rgb(255, 232, 176);
        let expected_a = Rgba::rgb(mul(200, 255), mul(200, 232), mul(200, 176));
        let tint_b = Rgba::rgb(176, 255, 224);
        let expected_b = Rgba::rgb(mul(200, 176), mul(200, 255), mul(200, 224));

        let draws = [
            SpriteDraw {
                content: SpriteContent::Animated {
                    sprite: &sprite,
                    elapsed: Duration::ZERO,
                    transform,
                    base_dot_rows: 8,
                },
                translate: WorldPos::new(4.0, 8.0), // buffer occupies dot cols [2,6) rows [4,12)
                tint: Some(tint_a),
                vertical_anchor: VerticalAnchor::Center,
            },
            SpriteDraw {
                content: SpriteContent::Animated {
                    sprite: &sprite,
                    elapsed: Duration::ZERO,
                    transform,
                    base_dot_rows: 8,
                },
                translate: WorldPos::new(24.0, 8.0), // buffer occupies dot cols [22,26) rows [4,12)
                tint: Some(tint_b),
                vertical_anchor: VerticalAnchor::Center,
            },
        ];

        let grid = composite_scene(32, 16, &camera, &draws);

        // Interior dot (3,6) of draw A's region -> cell (1,1).
        assert_eq!(
            glyph_color(grid.get(1, 1)),
            expected_a,
            "draw A's region must carry draw A's own tint, not draw B's"
        );
        // Interior dot (23,6) of draw B's region -> cell (11,1).
        assert_eq!(
            glyph_color(grid.get(11, 1)),
            expected_b,
            "draw B's region must carry draw B's own tint, not draw A's"
        );
    }

    /// A `tint: None` draw must composite the identical raw buffer into both
    /// the shape and color passes — i.e. `composite_scene`'s output for a
    /// single untinted draw must equal manually compositing that one raw
    /// buffer and converting via `dots_to_grid` (no separate/divergent
    /// untinted computation). Ports
    /// `piece_shape_and_color_tinted_matches_piece_dots`'s delegation-pin intent.
    #[test]
    fn composite_scene_untinted_uses_same_buffer_for_shape_and_color() {
        let sprite =
            AnimatedSprite::new(vec![solid_image(4, 8, 200, 150, 100, 255)], Duration::from_millis(100));
        let transform = Transform::default();
        let camera = FixedProjectCamera { project: (2, 4) };
        let translate = WorldPos::new(0.0, 0.0);

        let raw = sprite.rasterize_at(Duration::ZERO, &transform, 8);
        let placement = place(&raw, translate, &camera, VerticalAnchor::Center);
        let composited = composite_dots(4, 8, &[placement]);
        let expected = dots_to_grid(&composited);

        let draws = [SpriteDraw {
            content: SpriteContent::Animated {
                sprite: &sprite,
                elapsed: Duration::ZERO,
                transform,
                base_dot_rows: 8,
            },
            translate,
            tint: None,
            vertical_anchor: VerticalAnchor::Center,
        }];
        let actual = composite_scene(4, 8, &camera, &draws);

        assert_eq!(
            actual, expected,
            "untinted composite_scene output must match manually compositing the raw buffer into both shape and color"
        );
    }

    /// A `Prerasterized(&DotBuffer)` draw must composite the given buffer
    /// as-is — no sprite rasterization step, and (per `tint: None`) the same
    /// buffer feeds both the shape and color passes. Sibling `Animated` draws
    /// are unaffected (additive variant); this is the mechanism b7-t1's
    /// contact shadow relies on to place a `rasterize_shape`-built ellipse.
    #[test]
    fn composite_scene_prerasterized_places_buffer_as_is() {
        // A 2x4-dot buffer is the minimum that maps to a non-empty (1x1
        // cell) Grid (dots_to_grid_tinted floor-divides by 2/4). `project:
        // (1, 2)` makes `place`'s centering land the buffer's top-left dot
        // exactly at canvas origin: dot_x = 1 - 2/2 = 0, dot_y = 2 - 4/2 = 0.
        let mut buf = DotBuffer::new(2, 4);
        buf.set(0, 0, Dot::Lit(Rgba::rgb(10, 20, 30)));
        let camera = FixedProjectCamera { project: (1, 2) };

        let draws = [SpriteDraw {
            content: SpriteContent::Prerasterized(&buf),
            translate: WorldPos::new(0.0, 0.0),
            tint: None,
            vertical_anchor: VerticalAnchor::Center,
        }];
        let grid = composite_scene(2, 4, &camera, &draws);

        assert_eq!(grid.cols(), 1, "canvas 2 dots wide -> 1 cell column");
        assert_eq!(grid.rows(), 1, "canvas 4 dots tall -> 1 cell row");
        assert_eq!(
            glyph_color(grid.get(0, 0)),
            Rgba::rgb(10, 20, 30),
            "Prerasterized draw must place the given buffer's dot unchanged"
        );
    }

    /// `composite_scene` must place each draw via its own `vertical_anchor`
    /// (b5-t2) rather than the hardcoded `Center` it used before this task —
    /// a `Bottom`-anchored draw must match manually placing the same buffer
    /// with `place(.., VerticalAnchor::Bottom)`, and must differ from a
    /// `Center`-anchored draw of the identical buffer/position.
    #[test]
    fn composite_scene_threads_bottom_anchor() {
        // 2x8-dot buffer (even rows), one lit dot at its top-left corner.
        let mut buf = DotBuffer::new(2, 8);
        buf.set(0, 0, Dot::Lit(Rgba::rgb(10, 20, 30)));
        let camera = FixedProjectCamera { project: (8, 16) };
        let translate = WorldPos::new(0.0, 0.0);

        // Expected: composite_scene must delegate to place(.., Bottom).
        let bottom_placement = place(&buf, translate, &camera, VerticalAnchor::Bottom);
        let expected = dots_to_grid(&composite_dots(16, 16, &[bottom_placement]));

        let bottom_draws = [SpriteDraw {
            content: SpriteContent::Prerasterized(&buf),
            translate,
            tint: None,
            vertical_anchor: VerticalAnchor::Bottom,
        }];
        let actual = composite_scene(16, 16, &camera, &bottom_draws);

        assert_eq!(
            actual, expected,
            "composite_scene must place a Bottom-anchored draw via place(.., Bottom), not the hardcoded Center"
        );

        // Distinctness: a Center-anchored draw of the identical buffer/position
        // must land differently (guards against the field being read but
        // ignored).
        let center_draws = [SpriteDraw {
            content: SpriteContent::Prerasterized(&buf),
            translate,
            tint: None,
            vertical_anchor: VerticalAnchor::Center,
        }];
        let center_actual = composite_scene(16, 16, &camera, &center_draws);
        assert_ne!(
            actual, center_actual,
            "Bottom- and Center-anchored composites of the same buffer/position must differ"
        );

        // Dot-precision offset check at the place() level (composite_scene's
        // output is cell-resolution and can't expose a sub-cell shift): for
        // an even-row buffer, Center must sit exactly rows()/2 dots above
        // Bottom.
        let center_placement = place(&buf, translate, &camera, VerticalAnchor::Center);
        assert_eq!(
            center_placement.dot_y - bottom_placement.dot_y,
            (buf.rows() / 2) as i32,
            "Center anchor must sit exactly rows()/2 dots above Bottom anchor for an even-row buffer"
        );
    }
}
