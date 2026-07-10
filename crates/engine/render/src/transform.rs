//! Sprite transform: translate (world pos) + rotation (DEGREES) + per-axis
//! scale (negative mirrors). Rotation/scale are applied by `rasterize`
//! (b1-t2); this module only stores them. Spec 16 §"Sprite Transform &
//! Placement".

use crate::camera::{Camera, WorldPos};
use crate::composite::DotPlacement;
use crate::dots::{sprite_to_dots, DotBuffer};
use image::DynamicImage;
use engine_core::Inspectable;

/// Plain 2D vector (POD). Used for per-axis scale (negative mirrors, applied
/// in rasterize).
#[derive(Clone, Copy, PartialEq, Debug, Default, Inspectable)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }

    /// Same value on both axes (e.g. uniform scale).
    pub fn splat(v: f32) -> Self {
        Vec2 { x: v, y: v }
    }
}

/// Sprite transform: translate (world pos) + rotation (DEGREES) + per-axis
/// scale (negative mirrors).
#[derive(Clone, Copy, PartialEq, Debug, Inspectable)]
pub struct Transform {
    pub translate: WorldPos,
    pub rotation: f32, // degrees
    pub scale: Vec2,
}

impl Transform {
    /// Full constructor.
    pub fn new(translate: WorldPos, rotation: f32, scale: Vec2) -> Self {
        Transform { translate, rotation, scale }
    }

    /// Identity rotation/scale at a given world position.
    pub fn from_translate(translate: WorldPos) -> Self {
        Transform { translate, rotation: 0.0, scale: Vec2::new(1.0, 1.0) }
    }
}

impl Default for Transform {
    /// Identity: origin translate, 0° rotation, unit (1,1) scale.
    fn default() -> Self {
        Transform {
            translate: WorldPos::new(0.0, 0.0),
            rotation: 0.0,
            scale: Vec2::new(1.0, 1.0),
        }
    }
}

/// Affine rasterize: apply `transform.scale` (per-axis; negative mirrors) and
/// `transform.rotation` (degrees) to `img`, producing a `DotBuffer` sized to
/// the rotated bounding box. `transform.translate` is NOT applied here
/// (placement handles it — b1-t3). `scale (1,1)`, `rotation 0` reduces to
/// `sprite_to_dots(img, base_cols, base_rows)`. Spec 16 §"Sprite Transform &
/// Placement".
///
pub fn rasterize(img: &DynamicImage, transform: &Transform, base_dot_rows: u32) -> DotBuffer {
    // 1. Base dims (matches the demo convention render_movement.rs:153-155).
    let aspect = img.width() as f32 / img.height().max(1) as f32;
    let base_dot_cols = (base_dot_rows as f32 * aspect).round() as u32;

    // 2. Scaled dims.
    let scaled_cols = (base_dot_cols as f32 * transform.scale.x.abs()).round() as u32;
    let scaled_rows = (base_dot_rows as f32 * transform.scale.y.abs()).round() as u32;

    // 3. Mirror + resample — delegate resize/alpha-threshold to sprite_to_dots.
    let mirrored;
    let source = if transform.scale.x < 0.0 || transform.scale.y < 0.0 {
        mirrored = match (transform.scale.x < 0.0, transform.scale.y < 0.0) {
            (true, true) => img.fliph().flipv(),
            (true, false) => img.fliph(),
            (false, true) => img.flipv(),
            (false, false) => unreachable!(),
        };
        &mirrored
    } else {
        img
    };
    let scaled = sprite_to_dots(source, scaled_cols, scaled_rows);

    // 4. Fast path — identity rotation is exact.
    if transform.rotation == 0.0 {
        return scaled;
    }

    // 5. Rotation bbox.
    let theta = transform.rotation.to_radians();
    let (c, s) = (theta.cos(), theta.sin());
    let out_cols = ((scaled_cols as f32) * c.abs() + (scaled_rows as f32) * s.abs())
        .round()
        .max(1.0) as usize;
    let out_rows = ((scaled_cols as f32) * s.abs() + (scaled_rows as f32) * c.abs())
        .round()
        .max(1.0) as usize;

    // 6. Inverse rotation — sample the scaled buffer for each output dot.
    let mut out = DotBuffer::new(out_cols, out_rows);
    for oy in 0..out_rows {
        for ox in 0..out_cols {
            let dx = (ox as f32 + 0.5) - out_cols as f32 / 2.0;
            let dy = (oy as f32 + 0.5) - out_rows as f32 / 2.0;
            let sx = dx * c + dy * s + scaled_cols as f32 / 2.0;
            let sy = -dx * s + dy * c + scaled_rows as f32 / 2.0;
            let (fx, fy) = (sx.floor(), sy.floor());
            if fx >= 0.0 && fy >= 0.0 && (fx as usize) < scaled_cols as usize && (fy as usize) < scaled_rows as usize {
                out.set(ox, oy, scaled.get(fx as usize, fy as usize));
            }
            // else: leave default Dot::Transparent
        }
    }

    // 7. Return the rotated buffer.
    out
}

/// Vertical placement reference for a billboard sprite relative to its
/// camera-projected point. `Center` anchors the sprite's mid-row (pre-b5-t1
/// behavior); `Bottom` anchors the sprite's bottom row (feet at the point,
/// growing upward). b5-t1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAnchor {
    Center,
    Bottom,
}

/// Anchor `dots` at `camera.project(translate)` per `anchor`, returning a
/// ready `DotPlacement`. Collapses the rasterize -> project -> placement
/// boilerplate. `translate` is the pivot's world position; rotation/scale are
/// already baked into `dots` by `rasterize`. Spec 16 §"Sprite Transform &
/// Placement"; anchor param added by b5-t1.
pub fn place<'a, C: Camera>(
    dots: &'a DotBuffer,
    translate: WorldPos,
    camera: &C,
    anchor: VerticalAnchor,
) -> DotPlacement<'a> {
    let (px, py) = camera.project(translate);
    let dot_y = match anchor {
        VerticalAnchor::Center => py - (dots.rows() / 2) as i32,
        VerticalAnchor::Bottom => py - dots.rows() as i32,
    };
    DotPlacement {
        dots,
        dot_x: px - (dots.cols() / 2) as i32,
        dot_y,
        depth: camera.depth_key(translate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_default_is_identity() {
        let t = Transform::default();
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.scale, Vec2::new(1.0, 1.0));
        assert_eq!(t.translate, WorldPos::new(0.0, 0.0));
    }

    #[test]
    fn vec2_new_roundtrips() {
        let v = Vec2::new(3.5, -2.25);
        assert_eq!(v.x, 3.5);
        assert_eq!(v.y, -2.25);
    }

    #[test]
    fn vec2_splat_sets_both() {
        let v = Vec2::splat(4.0);
        assert_eq!(v.x, 4.0);
        assert_eq!(v.y, 4.0);
    }

    #[test]
    fn transform_new_roundtrips_fields() {
        let pos = WorldPos::new(10.0, -5.0);
        let scale = Vec2::new(2.0, 0.5);
        let t = Transform::new(pos, 45.0, scale);
        assert_eq!(t.translate, pos);
        assert_eq!(t.rotation, 45.0);
        assert_eq!(t.scale, scale);
    }

    #[test]
    fn transform_from_translate_is_identity_at_pos() {
        let pos = WorldPos::new(1.0, 2.0);
        let t = Transform::from_translate(pos);
        assert_eq!(t.translate, pos);
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.scale, Vec2::new(1.0, 1.0));
    }

    // ── rasterize (b1-t2) ────────────────────────────────────────────────────

    use crate::dots::{sprite_to_dots, Dot};
    use image::{Rgba as PixelRgba, RgbaImage};

    /// Build a uniform RGBA image where every pixel is `(r, g, b, a)`.
    fn solid_image(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
        let mut raw = RgbaImage::new(w, h);
        for p in raw.pixels_mut() {
            *p = PixelRgba([r, g, b, a]);
        }
        DynamicImage::from(raw)
    }

    /// Build an image that is opaque on the left half and fully transparent on
    /// the right half. Source width == `dot_cols` so the resize is an identity
    /// and Lanczos3 ringing cannot bleed across the seam.
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

    /// Identity transform (rotation 0, scale (1,1)) must reduce to *exactly*
    /// `sprite_to_dots(img, base_cols, base_rows)` — `DotBuffer` derives
    /// `PartialEq`, so this is a byte-exact equality assert.
    #[test]
    fn rasterize_identity_equals_sprite_to_dots() {
        let img = solid_image(6, 12, 100, 150, 200, 255); // aspect 0.5 (tall)
        let base_dot_rows = 8u32;
        let aspect = img.width() as f32 / img.height().max(1) as f32;
        let base_dot_cols = (base_dot_rows as f32 * aspect).round() as u32;

        let out = rasterize(&img, &Transform::default(), base_dot_rows);
        let expected = sprite_to_dots(&img, base_dot_cols, base_dot_rows);

        assert_eq!(
            out, expected,
            "identity transform must exactly equal sprite_to_dots(img, base_cols, base_rows)"
        );
    }

    /// A 90° rotation of a non-square scaled sprite must transpose the output
    /// dot dimensions (cols<->rows) relative to the identity (unrotated) output.
    #[test]
    fn rasterize_90deg_transposes_dims() {
        let img = solid_image(6, 12, 10, 20, 30, 255); // non-square (tall)
        let base_dot_rows = 8u32;

        let identity = rasterize(&img, &Transform::default(), base_dot_rows);
        let rotated = Transform { rotation: 90.0, ..Transform::default() };
        let out = rasterize(&img, &rotated, base_dot_rows);

        assert_eq!(
            out.cols(),
            identity.rows(),
            "90° rotation: out.cols() must equal identity.rows()"
        );
        assert_eq!(
            out.rows(),
            identity.cols(),
            "90° rotation: out.rows() must equal identity.cols()"
        );
    }

    /// `scale.x < 0` mirrors horizontally: a left-weighted (opaque) source
    /// becomes right-weighted in the output (rotation 0, so bbox is unchanged).
    #[test]
    fn rasterize_negative_scale_x_mirrors() {
        let dot_cols = 8u32;
        let dot_rows = 4u32;
        // aspect == dot_cols/dot_rows == 2.0, so base_dot_cols == dot_cols exactly.
        let img = half_opaque_image(dot_cols, dot_rows, 200, 100, 50);
        let base_dot_rows = dot_rows;

        let transform = Transform { scale: Vec2::new(-1.0, 1.0), ..Transform::default() };
        let out = rasterize(&img, &transform, base_dot_rows);

        assert_eq!(out.cols(), dot_cols as usize);
        assert_eq!(out.rows(), dot_rows as usize);

        let last_col = out.cols() - 1;
        for row in 0..out.rows() {
            assert_ne!(
                out.get(last_col, row),
                Dot::Transparent,
                "rightmost column ({last_col},{row}) must be Lit after horizontal mirror"
            );
            assert_eq!(
                out.get(0, row),
                Dot::Transparent,
                "leftmost column (0,{row}) must be Transparent after horizontal mirror"
            );
        }
    }

    /// `scale = (2,2)` must ~double the output dot dims relative to identity
    /// (allow ±1 dot for rounding).
    #[test]
    fn rasterize_scale_two_doubles_dims() {
        let img = solid_image(6, 12, 5, 6, 7, 255);
        let base_dot_rows = 8u32;

        let identity = rasterize(&img, &Transform::default(), base_dot_rows);
        let doubled = Transform { scale: Vec2::new(2.0, 2.0), ..Transform::default() };
        let out = rasterize(&img, &doubled, base_dot_rows);

        let expected_cols = identity.cols() * 2;
        let expected_rows = identity.rows() * 2;
        assert!(
            (out.cols() as i64 - expected_cols as i64).abs() <= 1,
            "cols should be ~2x identity: got {}, expected ~{expected_cols}",
            out.cols()
        );
        assert!(
            (out.rows() as i64 - expected_rows as i64).abs() <= 1,
            "rows should be ~2x identity: got {}, expected ~{expected_rows}",
            out.rows()
        );
    }

    /// `translate` must have no effect on the rasterized `DotBuffer` —
    /// placement (b1-t3) is responsible for translation.
    #[test]
    fn rasterize_translate_has_no_effect() {
        let img = solid_image(6, 12, 1, 2, 3, 255);
        let base_dot_rows = 8u32;

        let t1 = Transform::default();
        let t2 = Transform { translate: WorldPos::new(500.0, -500.0), ..Transform::default() };

        let out1 = rasterize(&img, &t1, base_dot_rows);
        let out2 = rasterize(&img, &t2, base_dot_rows);

        assert_eq!(out1, out2, "translate must not affect the rasterized DotBuffer");
    }

    // ── place (b1-t3) ────────────────────────────────────────────────────────

    use crate::camera::OrthographicCamera;

    /// Even-sized buffer: dot_x/dot_y = camera.project(translate) minus the
    /// exact half-extent; depth = camera.depth_key(translate).
    #[test]
    fn place_centers_even_sized_buffer() {
        let dots = DotBuffer::new(8, 4);
        let translate = WorldPos::new(2.0, 3.0);
        let camera = OrthographicCamera::new(4.0); // project -> (8, 12)

        let placement = place(&dots, translate, &camera, VerticalAnchor::Center);

        assert_eq!(placement.dot_x, 8 - 4, "dot_x must be project.x - cols/2");
        assert_eq!(placement.dot_y, 12 - 2, "dot_y must be project.y - rows/2");
        assert_eq!(placement.depth, camera.depth_key(translate));
    }

    /// Odd-sized buffer: half-extent uses integer (floor) division, so the
    /// center rounds down (cols/2 == 2, rows/2 == 1 for a 5x3 buffer).
    #[test]
    fn place_centers_odd_sized_buffer_rounds_down() {
        let dots = DotBuffer::new(5, 3);
        let translate = WorldPos::new(0.0, 0.0);
        let camera = OrthographicCamera::new(1.0); // project -> (0, 0)

        let placement = place(&dots, translate, &camera, VerticalAnchor::Center);

        assert_eq!(placement.dot_x, 0 - 2, "5/2 must floor to 2");
        assert_eq!(placement.dot_y, 0 - 1, "3/2 must floor to 1");
    }

    /// depth must equal camera.depth_key(translate) regardless of buffer size.
    #[test]
    fn place_depth_matches_camera_depth_key() {
        let dots = DotBuffer::new(6, 6);
        let translate = WorldPos::new(-3.5, 7.25);
        let camera = OrthographicCamera::new(2.0);

        let placement = place(&dots, translate, &camera, VerticalAnchor::Center);

        assert_eq!(placement.depth, camera.depth_key(translate));
    }

    /// The returned placement must borrow the exact buffer passed in.
    #[test]
    fn place_dots_field_points_at_input_buffer() {
        let dots = DotBuffer::new(4, 4);
        let translate = WorldPos::new(1.0, 1.0);
        let camera = OrthographicCamera::new(1.0);

        let placement = place(&dots, translate, &camera, VerticalAnchor::Center);

        assert!(std::ptr::eq(placement.dots, &dots), "placement.dots must point at the passed buffer");
    }

    /// `Bottom` anchors the buffer's bottom row at the projected point:
    /// `dot_y == py - dots.rows()`, distinct from `Center`'s `py - rows/2` for
    /// the identical buffer/position. `dot_x`/`depth` are unaffected by anchor.
    #[test]
    fn place_bottom_anchors_feet_at_projected_point() {
        let dots = DotBuffer::new(8, 4);
        let translate = WorldPos::new(2.0, 3.0);
        let camera = OrthographicCamera::new(4.0); // project -> (8, 12)

        let center = place(&dots, translate, &camera, VerticalAnchor::Center);
        let bottom = place(&dots, translate, &camera, VerticalAnchor::Bottom);

        assert_eq!(bottom.dot_y, 12 - 4, "Bottom dot_y must be project.y - rows");
        assert_eq!(center.dot_y, 12 - 2, "Center dot_y must remain project.y - rows/2");
        assert_ne!(bottom.dot_y, center.dot_y, "Bottom and Center must differ for a non-zero-row buffer");
        assert_eq!(bottom.dot_x, center.dot_x, "dot_x must be identical across anchors");
        assert_eq!(bottom.depth, center.depth, "depth must be identical across anchors");
    }

    // ── Inspectable (b3-t1) ──────────────────────────────────────────────────

    use engine_core::FieldTag;

    /// `Transform::schema()` must be a `Struct` with exactly the 3 fields, and
    /// its two struct-typed children (`translate`, `scale`) must themselves
    /// expose `x`/`y` `Float` children — a real 2-level nested tree, not a
    /// flattened blob.
    #[test]
    fn transform_schema_is_nested_struct_tree() {
        let schema = Transform::schema();
        assert_eq!(schema.tag, FieldTag::Struct);
        assert_eq!(
            schema.children.len(),
            3,
            "Transform must have exactly 3 top-level fields"
        );

        let by_name = |s: &engine_core::FieldSchema, n: &str| {
            s.children
                .iter()
                .find(|c| c.name == n)
                .unwrap_or_else(|| panic!("missing child schema for field `{n}`"))
                .clone()
        };

        let translate = by_name(&schema, "translate");
        assert_eq!(translate.tag, FieldTag::Struct);
        assert_eq!(by_name(&translate, "x").tag, FieldTag::Float);
        assert_eq!(by_name(&translate, "y").tag, FieldTag::Float);

        let rotation = by_name(&schema, "rotation");
        assert_eq!(rotation.tag, FieldTag::Float);

        let scale = by_name(&schema, "scale");
        assert_eq!(scale.tag, FieldTag::Struct);
        assert_eq!(by_name(&scale, "x").tag, FieldTag::Float);
        assert_eq!(by_name(&scale, "y").tag, FieldTag::Float);
    }

    /// `apply_patch("translate.x", 3.0)` must mutate exactly that leaf,
    /// proving the derive-generated struct-arm resolves a real nested path
    /// (Transform -> WorldPos -> f32) rather than only a single-level one.
    #[test]
    fn transform_apply_patch_mutates_only_translate_x() {
        let mut t = Transform::new(WorldPos::new(1.0, 2.0), 45.0, Vec2::new(2.0, -3.0));

        t.apply_patch("translate.x", engine_core::__private::serde_json::json!(9.5))
            .expect("apply_patch on translate.x must succeed");

        assert_eq!(t.translate.x, 9.5, "translate.x must be updated");
        assert_eq!(t.translate.y, 2.0, "translate.y must be untouched");
        assert_eq!(t.rotation, 45.0, "rotation must be untouched");
        assert_eq!(t.scale, Vec2::new(2.0, -3.0), "scale must be untouched");
    }

    /// `Vec2` and `WorldPos` are both plain `{x, y}` structs: schema must be
    /// `Struct` with exactly `[x: Float, y: Float]` children for each.
    #[test]
    fn vec2_and_worldpos_schema_are_float_pair_structs() {
        for (tag, children) in [
            (Vec2::schema().tag, Vec2::schema().children),
            (WorldPos::schema().tag, WorldPos::schema().children),
        ] {
            assert_eq!(tag, FieldTag::Struct);
            assert_eq!(children.len(), 2);
            let names: Vec<&str> = children.iter().map(|c| c.name.as_str()).collect();
            assert!(names.contains(&"x"));
            assert!(names.contains(&"y"));
            assert!(children.iter().all(|c| c.tag == FieldTag::Float));
        }
    }
}
