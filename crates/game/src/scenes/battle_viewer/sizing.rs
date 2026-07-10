use super::*;

/// Per-index idle-animation desync offset (spec 05: pieces must not all
/// animate in lockstep).
pub const PIECE_STAGGER: std::time::Duration = std::time::Duration::from_millis(37);
/// `sprite_base_dot_rows` = `camera.scale_dots * SPRITE_DOT_RATIO`, rounded.
/// Kept well under `1.0` because the binding constraint is sprite WIDTH, not
/// height: idle GIFs are not square (widest real asset, frost_lizard, is
/// ~1.58:1 width:height), and width is derived downstream from
/// `base_dot_rows` by the raster aspect formula. `0.6` keeps both the height
/// and the width of every idle sprite inside its own square-in-dots cell.
pub const SPRITE_DOT_RATIO: f32 = 0.6;
/// Per-index animation offset so the 8 idle loops don't play in lockstep:
/// `elapsed + PIECE_STAGGER * index`.
pub fn piece_elapsed(elapsed: Duration, index: usize) -> Duration {
    elapsed + PIECE_STAGGER * index as u32
}

/// Sprite height in dots, sized off the shared camera's per-world-unit dot
/// scale at the world origin:
/// `(camera.local_dots_per_world_unit(WorldPos::new(0.0, 0.0)) * SPRITE_DOT_RATIO).round() as u32`.
/// `TopDown`-only in practice now (see `sprite_base_dot_rows_width_fill` for
/// Sideline/OverShoulder) — kept pos-independent since
/// `local_dots_per_world_unit` is constant everywhere for the orthographic
/// Top-Down camera (Orthographic ignores `pos`).
pub fn sprite_base_dot_rows(camera: &BattleCamera) -> u32 {
    (camera.local_dots_per_world_unit(WorldPos::new(0.0, 0.0)) * SPRITE_DOT_RATIO).round() as u32
}

/// Fraction of a cell's width, AT the piece's own position, that a creature's
/// rendered WIDTH targets under Sideline/OverShoulder — the binding
/// constraint there is width filling the base of the cell the piece stands
/// on (project owner's explicit ask), unlike Top-Down's height-ratio
/// approach (`SPRITE_DOT_RATIO`), which exists to keep sprites from
/// overflowing a fixed-size square cell from directly above.
const WIDTH_FILL_RATIO: f32 = 0.92;

/// Per-piece sprite height in dots for Sideline/OverShoulder: sized so the
/// creature's rendered WIDTH is `WIDTH_FILL_RATIO` of a cell-width AT `pos`
/// (via `BattleCamera::local_dots_per_world_unit`, which already shrinks
/// with distance from the camera) — `aspect` (source image width/height)
/// converts that width target into the `base_dot_rows` the rasterizer
/// actually takes. Because this already varies with `pos`, no SEPARATE
/// depth-scale multiplier is applied on top (see `depth_scale_factor`) —
/// one distance term, not two stacked ones.
fn sprite_base_dot_rows_width_fill(camera: &BattleCamera, pos: WorldPos, aspect: f32) -> u32 {
    let target_width_dots = camera.local_dots_per_world_unit(pos) * WIDTH_FILL_RATIO;
    (target_width_dots / aspect.max(0.01)).round().max(1.0) as u32
}

/// Per-piece depth-scale multiplier (spec 41 Decision 4): always `1.0` now.
/// `TopDown` never had a `forward_distance` term to derive one from.
/// Sideline/OverShoulder used to apply a second, ratio-based shrink here on
/// top of `sprite_base_dot_rows_width_fill`'s own already-distance-dependent
/// sizing — that double-applied the same falloff (once as an absolute
/// per-position rate, once again as a relative ratio), over-shrinking far
/// pieces. Kept as a named seam (not deleted outright) since
/// `depth_scaled_transform` below still has a real job: preserving
/// `transform`'s own existing scale (team mirror, Die-tween shrink).
fn depth_scale_factor(_camera: &BattleCamera, _pos: WorldPos) -> f32 {
    1.0
}

/// Applies `depth_scale_factor` on top of `transform`'s OWN existing scale
/// (which may already carry a Die-event shrink tween) — multiplies, never
/// overwrites from `Team::scale_x()`/`1.0`. `translate`/`rotation` pass
/// through unchanged. The render loop's per-piece draw-construction calls
/// this same helper (not a second, duplicated inline formula).
fn depth_scaled_transform(transform: &Transform, camera: &BattleCamera) -> Transform {
    let factor = depth_scale_factor(camera, transform.translate);
    Transform {
        scale: Vec2::new(transform.scale.x * factor, transform.scale.y * factor),
        ..*transform
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Playback event data model (b1-t2). Data shape only — not yet wired into
// `BattleViewer`/`update()`/`render()` (that starts at b2-t1).
// ─────────────────────────────────────────────────────────────────────────────

impl BattleViewer {
    /// b7-t1: per drawable piece, emits the shadow `SpriteDraw`
    /// (`Prerasterized`, `tint: None`) immediately followed by the piece's
    /// own `SpriteDraw` (`tint: None` — b7-t1 detint), sharing `translate`.
    /// `shadow_bufs` must come from `self.shadow_buffers(geom)` and align
    /// index-for-index with `drawable_pieces()`'s order.
    pub(super) fn build_draws<'a>(
        &'a self,
        geom: &BoardGeometry,
        shadow_bufs: &'a [DotBuffer],
        elapsed: Duration,
    ) -> Vec<SpriteDraw<'a>> {
        let anchor = geom.camera.vertical_anchor_hint();
        let is_top_down = matches!(geom.camera.camera, AnyCamera::Orthographic(_));
        // Top-Down's base size is position-independent (its `scale_dots` IS
        // a constant per-world-unit rate everywhere), so it's still fine to
        // compute once per frame; Sideline/OverShoulder need it per piece
        // (see the loop body) since it depends on that piece's own distance
        // from the camera AND its own sprite's aspect ratio.
        let top_down_base_dot_rows = sprite_base_dot_rows(&geom.camera);
        let mut draws = Vec::with_capacity(shadow_bufs.len() * 2);
        for (i, (p, sprite)) in self.drawable_pieces().enumerate() {
            let transform = depth_scaled_transform(&p.transform, &geom.camera);
            draws.push(SpriteDraw {
                content: SpriteContent::Prerasterized(&shadow_bufs[i]),
                translate: p.transform.translate,
                tint: None,
                // A ground-plane decal is centered ON the contact point
                // (spreads symmetrically around it), unlike a standing
                // creature (whose FEET are at the point, body extending
                // upward) — using the same per-camera Bottom anchor as the
                // creature here shifted the shadow's visible center away
                // from the piece's actual ground position under any camera
                // with a non-Center anchor.
                vertical_anchor: VerticalAnchor::Center,
            });
            let base_dot_rows = if is_top_down {
                top_down_base_dot_rows
            } else {
                let frame = sprite.frame_at(piece_elapsed(elapsed, p.index));
                let aspect = frame.width() as f32 / frame.height().max(1) as f32;
                sprite_base_dot_rows_width_fill(&geom.camera, p.transform.translate, aspect)
            };
            draws.push(SpriteDraw {
                content: SpriteContent::Animated {
                    sprite,
                    elapsed: piece_elapsed(elapsed, p.index),
                    transform,
                    base_dot_rows,
                },
                translate: p.transform.translate,
                tint: None,
                vertical_anchor: anchor,
            });
        }
        draws
    }

}

#[cfg(test)]
mod piece_render_tests {
    use super::*;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};

    /// A `BattleCamera` variant rebuilt at an arbitrary test `scale` — only
    /// `scale_dots` matters for the tests using this helper.
    fn at_scale(make: fn() -> BattleCamera, scale: f32) -> BattleCamera {
        make().with_scale_dots(scale)
    }

    /// A uniform fully-opaque RGBA image (source for the synthetic sprites
    /// below) — deterministic, unlike the real GIF asset.
    fn opaque_image(w: u32, h: u32) -> DynamicImage {
        let mut raw = RgbaImage::new(w, h);
        for p in raw.pixels_mut() {
            *p = PixelRgba([200, 200, 200, 255]);
        }
        DynamicImage::from(raw)
    }

    /// Direct-value pin (b1-t1): the two team tint constants must be the
    /// pale-gold / pale-mint defaults, not the old saturated blue/red.
    #[test]
    fn team_colors_are_pale_gold_and_pale_mint() {
        assert_eq!(TEAM_A_COLOR, Rgba::rgb(0xff, 0xe8, 0xb0), "TEAM_A_COLOR must be pale gold");
        assert_eq!(TEAM_B_COLOR, Rgba::rgb(0xb0, 0xff, 0xe0), "TEAM_B_COLOR must be pale mint");
    }

    /// Piece::new's stored, seeded `transform` mirrors Team B (`scale.x == -1.0`)
    /// and leaves Team A unmirrored (`== 1.0`).
    #[test]
    fn piece_transform_scale_x_mirrors_team_b_only() {
        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 3);

        assert_eq!(piece_a.transform.scale.x, 1.0, "Team A stored transform unmirrored");
        assert_eq!(piece_b.transform.scale.x, -1.0, "Team B stored transform mirrored");
    }

    /// b2-t1 DELIVERABLE: `Piece::new`'s seeded `transform` field must match
    /// the hand-derived layout formula for the same `(col, row, team)`:
    /// `translate = world_pos_for_cell(col, row)`, `rotation = 0.0`, `scale =
    /// (team.scale_x(), 1.0)`. Covers both teams so the mirror (`scale.x`) is
    /// pinned for Team B too.
    #[test]
    fn piece_new_seeds_transform_from_layout_math() {
        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        assert_eq!(
            piece_a.transform,
            Transform {
                translate: world_pos_for_cell(1, TEAM_A_ROW),
                rotation: 0.0,
                scale: Vec2::new(1.0, 1.0),
            },
            "Team A: Piece::new's seeded transform must match the hand-derived layout formula"
        );

        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 3);
        assert_eq!(
            piece_b.transform,
            Transform {
                translate: world_pos_for_cell(1, TEAM_B_ROW),
                rotation: 0.0,
                scale: Vec2::new(-1.0, 1.0),
            },
            "Team B: Piece::new's seeded transform must match the hand-derived layout formula (mirrored)"
        );
    }

    /// b2-t1 DELIVERABLE: `Piece::new`'s seeded `color` field must be the
    /// piece's team default (the pastel `TEAM_A_COLOR`/`TEAM_B_COLOR`
    /// constants), not some other placeholder.
    #[test]
    fn piece_new_seeds_color_from_team_default() {
        let piece_a = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        assert_eq!(piece_a.color, TEAM_A_COLOR, "Team A piece must seed color = TEAM_A_COLOR");

        let piece_b = Piece::new(1, TEAM_B_ROW, Team::B, 0);
        assert_eq!(piece_b.color, TEAM_B_COLOR, "Team B piece must seed color = TEAM_B_COLOR");
    }

    /// b1-t1 DELIVERABLE: `Piece::new` seeds `alive = true` — a freshly
    /// constructed piece starts alive.
    #[test]
    fn piece_new_defaults_alive_true() {
        let piece = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        assert!(piece.alive, "Piece::new must seed alive = true");
    }

    /// DELIVERABLE (3): using a synthetic multi-frame `AnimatedSprite`, two
    /// pieces with different `index` at the same `elapsed` must resolve to
    /// different frame indices for at least one tested `elapsed` — proves the
    /// per-piece stagger actually desyncs idle playback.
    #[test]
    fn piece_elapsed_desyncs_frame_selection_across_indices() {
        let sprite = AnimatedSprite::new(
            vec![opaque_image(1, 1), opaque_image(1, 1), opaque_image(1, 1)],
            Duration::from_millis(20),
        );
        let elapsed = Duration::from_millis(15);

        let idx0 = sprite.frame_index_at(piece_elapsed(elapsed, 0));
        let idx5 = sprite.frame_index_at(piece_elapsed(elapsed, 5));

        assert_ne!(
            idx0, idx5,
            "pieces with index 0 vs 5 at the same elapsed time must resolve to different frames"
        );
    }

    /// DELIVERABLE (4): `sprite_base_dot_rows` is a fixed, documented ratio of
    /// the active `BattleCamera`'s dot scale, pinned against the
    /// `SPRITE_DOT_RATIO` constant. `sprite_base_dot_rows` is `TopDown`-only
    /// in practice (see `sprite_base_dot_rows_width_fill` for Sideline/
    /// OverShoulder).
    #[test]
    fn sprite_base_dot_rows_matches_ratio_constant() {
        // TopDown is orthographic — `local_dots_per_world_unit` IS
        // `scale_dots` directly, so `sprite_base_dot_rows` is a fixed ratio
        // of `scale`.
        for scale in [8.0f32, 32.0f32, 5.0f32] {
            let camera = at_scale(BattleCamera::top_down_preset, scale);
            let expected = (scale * SPRITE_DOT_RATIO).round() as u32;
            assert_eq!(
                sprite_base_dot_rows(&camera),
                expected,
                "sprite_base_dot_rows must equal \
                 (local_dots_per_world_unit(origin) * SPRITE_DOT_RATIO).round() \
                 for scale {scale} and camera {camera:?}"
            );
        }
    }

    /// b1-t3 DELIVERABLE: a Top-Down piece sprite's rasterized dot footprint
    /// (height AND width) must be contained within its own cell's dot bounds.
    /// At today's `SPRITE_DOT_RATIO = 1.2` this overflows (height already
    /// exceeds `cell_height_rows*4`, and width overflows further for
    /// non-square idle GIFs, e.g. frost_lizard's ~1.58 aspect) — this is an
    /// invariant on containment, never a pinned "ratio is now X" value.
    #[test]
    fn top_down_sprite_footprint_fits_cell() {
        let area = Rect::new(0, 0, 80, 40);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());
        let cell_h = geom.cell_height_rows as u32 * 4;
        let cell_w = geom.cell_width_cols as u32 * 2;
        let base = sprite_base_dot_rows(&geom.camera);

        assert!(
            base <= cell_h,
            "sprite_base_dot_rows ({base}) must fit within the cell's dot height ({cell_h}) \
             at SPRITE_DOT_RATIO={SPRITE_DOT_RATIO}"
        );

        for creature in crate::creatures::all() {
            let sprite = creature
                .animation(AnimationKind::Idle)
                .expect("every creature must have an Idle animation registered");
            let buf = sprite.rasterize_at(Duration::ZERO, &Transform::default(), base);
            assert!(
                buf.rows() as u32 <= cell_h && buf.cols() as u32 <= cell_w,
                "{}'s idle sprite footprint ({}x{} dots) must fit within the cell's dot bounds \
                 ({cell_w}x{cell_h}) at SPRITE_DOT_RATIO={SPRITE_DOT_RATIO} (base_dot_rows={base})",
                creature.name(),
                buf.cols(),
                buf.rows(),
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: depth-scale factor + per-piece scale wiring (b6-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod depth_scale_tests {
    use super::*;
    use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};

    /// Synthetic square sprite — only relative rasterized dimensions matter
    /// for this module's size-comparison assertions, not pixel content.
    fn solid_sprite(size: u32) -> AnimatedSprite {
        let mut raw = RgbaImage::new(size, size);
        for p in raw.pixels_mut() {
            *p = PixelRgba([200, 200, 200, 255]);
        }
        AnimatedSprite::new(vec![DynamicImage::from(raw)], Duration::from_millis(100))
    }

    /// Top-Down has no `PerspectiveCamera` backing it, so `depth_scale_factor`
    /// is exactly `1.0` for any `pos`, in-board or not.
    #[test]
    fn top_down_factor_always_one() {
        let mode = BattleCamera::top_down_preset();
        for pos in [
            WorldPos::new(0.5, 0.5),
            WorldPos::new(3.5, 3.5),
            WorldPos::new(6.5, 6.5),
            WorldPos::new(-10.0, 40.0),
        ] {
            assert_eq!(
                depth_scale_factor(&mode, pos),
                1.0,
                "Top-Down depth_scale_factor must be exactly 1.0 at pos {pos:?}"
            );
        }
    }

    /// Distance-based size now lives entirely in `sprite_base_dot_rows_width_fill`
    /// (spec 41 Decision 4's single distance term) — `depth_scale_factor` was
    /// deprecated to a constant `1.0` once that became position-dependent in
    /// its own right, to avoid double-applying the same falloff. Under
    /// Over-the-shoulder (depth axis = Row), the camera sits behind Team B
    /// (past Row `BOARD_ROWS`, the high side), so the near edge is Row
    /// `BOARD_ROWS`, not Row 0.0 — width-fill sizing strictly decreases
    /// moving away from it.
    #[test]
    fn over_shoulder_width_fill_decreases_with_distance() {
        // The bare preset's `scale_dots` is a `0.0` placeholder (only
        // `board_geometry`'s fit-to-viewport ever sets a real value) — with
        // it at 0.0, `local_dots_per_world_unit` is 0.0 everywhere and every
        // width-fill result clamps to the same `1` floor, hiding the
        // distance falloff this test exists to check. Rebuild at an
        // arbitrary nonzero scale first, the same way `piece_render_tests`'s
        // `at_scale` helper does.
        let mode = BattleCamera::over_shoulder_preset().with_scale_dots(40.0);
        let spread = 3.5;
        let aspect = 1.0;

        let near = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(spread, BOARD_ROWS as f32 - 0.5), aspect);
        let mid = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(spread, 3.5), aspect);
        let far = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(spread, 0.0), aspect);

        assert!(near > mid, "near ({near}) must exceed mid ({mid})");
        assert!(mid > far, "mid ({mid}) must exceed far ({far})");
        for v in [near, mid, far] {
            assert!(v > 0, "width-fill base_dot_rows must stay positive: got {v}");
        }
    }

    /// Sideline analog of `over_shoulder_width_fill_decreases_with_distance`
    /// (depth axis = Col) — the "farther = strictly smaller" property must
    /// hold per non-Top-Down preset, not just one.
    #[test]
    fn sideline_width_fill_decreases_with_distance() {
        let mode = BattleCamera::sideline_preset().with_scale_dots(40.0);
        let spread = 3.5;
        let aspect = 1.0;

        let near = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(0.5, spread), aspect);
        let mid = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(3.5, spread), aspect);
        let far = sprite_base_dot_rows_width_fill(&mode, WorldPos::new(6.5, spread), aspect);

        assert!(near > mid, "near ({near}) must exceed mid ({mid})");
        assert!(mid > far, "mid ({mid}) must exceed far ({far})");
        for v in [near, mid, far] {
            assert!(v > 0, "width-fill base_dot_rows must stay positive: got {v}");
        }
    }

    /// A farther piece's rasterized sprite must be smaller than a nearer
    /// piece's under Over-the-shoulder — the visible depth cue this task
    /// adds. Only `translate`'s depth coordinate (world-y, since
    /// Over-the-shoulder's depth axis is Row) differs between the two;
    /// both are in-board positions. `base_dot_rows` now comes from
    /// `sprite_base_dot_rows_width_fill` (per-piece), not a single
    /// `depth_scaled_transform`-applied ratio.
    #[test]
    fn width_fill_sizing_shrinks_farther_piece_under_over_shoulder() {
        let mode = BattleCamera::over_shoulder_preset().with_scale_dots(40.0);
        let sprite = solid_sprite(20); // square synthetic sprite: aspect 1.0

        // Over-the-shoulder's camera sits behind Team B (past Row
        // BOARD_ROWS, the high side) — so Row 6.5 is NEAR the camera and
        // Row 0.5 is FAR from it, the opposite of the low-side presets.
        let near_pos = WorldPos::new(3.5, 6.5);
        let far_pos = WorldPos::new(3.5, 0.5);
        let near_transform = Transform { translate: near_pos, ..Transform::default() };
        let far_transform = Transform { translate: far_pos, ..Transform::default() };

        let near_rows = sprite_base_dot_rows_width_fill(&mode, near_pos, 1.0);
        let far_rows = sprite_base_dot_rows_width_fill(&mode, far_pos, 1.0);

        let near_buf = sprite.rasterize_at(Duration::ZERO, &near_transform, near_rows);
        let far_buf = sprite.rasterize_at(Duration::ZERO, &far_transform, far_rows);

        assert!(
            far_buf.cols() < near_buf.cols() && far_buf.rows() < near_buf.rows(),
            "a farther piece's rasterized sprite must be smaller: near={}x{}, far={}x{}",
            near_buf.cols(),
            near_buf.rows(),
            far_buf.cols(),
            far_buf.rows()
        );
    }

    /// Under Top-Down, no piece is ever depth-scaled smaller than another —
    /// two pieces at very different depths rasterize to identical sizes.
    #[test]
    fn depth_scaled_transform_equal_size_under_top_down() {
        let mode = BattleCamera::top_down_preset();
        let sprite = solid_sprite(20);

        let near_transform = Transform { translate: WorldPos::new(3.5, 3.5), ..Transform::default() };
        let far_transform = Transform { translate: WorldPos::new(3.5, 30.0), ..Transform::default() };

        let near_scaled = depth_scaled_transform(&near_transform, &mode);
        let far_scaled = depth_scaled_transform(&far_transform, &mode);

        let near_buf = sprite.rasterize_at(Duration::ZERO, &near_scaled, 20);
        let far_buf = sprite.rasterize_at(Duration::ZERO, &far_scaled, 20);

        assert_eq!(
            (near_buf.cols(), near_buf.rows()),
            (far_buf.cols(), far_buf.rows()),
            "Top-Down must not apply any depth-based size difference"
        );
    }

    /// Load-bearing regression (research.md refinement): `depth_scaled_transform`
    /// must MULTIPLY the depth factor into `transform`'s OWN existing scale
    /// (which may already carry a partial Die-shrink tween written by
    /// `drive_events`), never overwrite it from `Team::scale_x()`/`1.0`. A
    /// transform with an already-halved scale (simulating a mid-Die tween)
    /// must come out at exactly half of the untweened result — not silently
    /// popping back to full size.
    #[test]
    fn depth_scaled_transform_preserves_existing_tweened_scale() {
        let mode = BattleCamera::over_shoulder_preset();
        let pos = WorldPos::new(3.5, 6.5);

        let full_scale_transform = Transform {
            translate: pos,
            scale: Vec2::new(1.0, 1.0),
            ..Transform::default()
        };
        let half_scale_transform = Transform {
            translate: pos,
            scale: Vec2::new(0.5, 0.5),
            ..Transform::default()
        };

        let full_scaled = depth_scaled_transform(&full_scale_transform, &mode);
        let half_scaled = depth_scaled_transform(&half_scale_transform, &mode);

        assert_eq!(
            half_scaled.scale.x,
            full_scaled.scale.x * 0.5,
            "a half-tweened scale.x must remain exactly half the untweened result, not be overwritten"
        );
        assert_eq!(
            half_scaled.scale.y,
            full_scaled.scale.y * 0.5,
            "a half-tweened scale.y must remain exactly half the untweened result, not be overwritten"
        );
        assert_eq!(half_scaled.translate, pos, "translate must pass through unchanged");
        assert_eq!(half_scaled.rotation, 0.0, "rotation must pass through unchanged (billboarding invariant)");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: playback event data model (b1-t2)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod billboarding_invariant_tests {
    use super::*;
    use engine_core::scene::{EngineCtx, Scene};

    const EPS: f32 = 1e-5;

    fn all_presets() -> [BattleCamera; 3] {
        [
            BattleCamera::sideline_preset(),
            BattleCamera::over_shoulder_preset(),
            BattleCamera::top_down_preset(),
        ]
    }

    /// Asserts the billboarding invariant on every draw pair produced by
    /// `build_draws` for one scene/preset combination: the piece's own
    /// `Animated` transform carries `rotation == 0.0` and
    /// `|scale.x| == |scale.y|` (within EPS); the shadow entry is always
    /// `Prerasterized` (carries no `Transform` at all, so its squash can
    /// never be a sprite shear/rotation). Returns whether any `Animated`
    /// entry had a negative `scale.x` (team-mirror sign), so callers can
    /// sanity-check the abs-comparison is non-vacuous.
    fn assert_billboarding_invariant(scene: &BattleViewer, geom: &BoardGeometry, elapsed: Duration) -> bool {
        let shadow_bufs = scene.shadow_buffers(geom);
        let draws = scene.build_draws(geom, &shadow_bufs, elapsed);
        assert!(draws.len() >= 2, "expected at least one shadow+piece pair");

        let mut saw_negative_scale_x = false;
        for pair in draws.chunks(2) {
            let (shadow, piece) = (&pair[0], &pair[1]);
            assert!(
                matches!(shadow.content, SpriteContent::Prerasterized(_)),
                "shadow entry must be SpriteContent::Prerasterized (never carries a Transform)"
            );
            match &piece.content {
                SpriteContent::Animated { transform, .. } => {
                    assert_eq!(
                        transform.rotation, 0.0,
                        "piece's own sprite must never be camera-rotated; got rotation={}",
                        transform.rotation
                    );
                    let (sx, sy) = (transform.scale.x.abs(), transform.scale.y.abs());
                    assert!(
                        (sx - sy).abs() < EPS,
                        "piece's own sprite scale must be uniform in magnitude: |scale.x|={} |scale.y|={}",
                        sx, sy
                    );
                    if transform.scale.x < 0.0 {
                        saw_negative_scale_x = true;
                    }
                }
                _ => panic!("expected the piece's own draw to be SpriteContent::Animated"),
            }
        }
        saw_negative_scale_x
    }

    /// PUBLIC_SURFACE: fresh default scene, standing still (`elapsed == 0`),
    /// across all 3 camera presets — no rotation, uniform scale magnitude.
    #[test]
    fn billboard_no_rotation_uniform_scale_all_presets_standing_still() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);

        let mut saw_negative_scale_x = false;
        for camera in all_presets() {
            let geom = board_geometry(area, camera, BattleViewerTuning::default());
            saw_negative_scale_x |= assert_billboarding_invariant(&scene, &geom, Duration::ZERO);
        }
        assert!(
            saw_negative_scale_x,
            "expected at least one Team-B piece with scale.x < 0.0 across presets, \
             otherwise the abs-magnitude comparison is vacuous"
        );
    }

    /// PUBLIC_SURFACE: the invariant must keep holding while events are
    /// actively driving pieces — `demo_events()`'s `Move` (piece 0, window
    /// `[1.0, 2.2)`) and `Die` (piece 6, window `[1.6, 2.6)`) are both active
    /// at `elapsed = 2.0`, so this exercises both a mid-glide translate tween
    /// and a mid-shrink scale tween, across all 3 camera presets.
    #[test]
    fn billboard_invariant_holds_through_active_move_and_die() {
        let mut ctx = EngineCtx;
        let mut scene = BattleViewer::default();
        scene.update(&mut ctx, Duration::from_secs_f32(2.0));

        let area = Rect::new(0, 0, 100, 50);
        for camera in all_presets() {
            let geom = board_geometry(area, camera, BattleViewerTuning::default());
            assert_billboarding_invariant(&scene, &geom, Duration::from_secs_f32(2.0));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: per-preset vertical anchor / feet-in-the-box placement (b5-t4)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod vertical_anchor_choice_tests {
    use super::*;

    /// PUBLIC_SURFACE (research.md b5-t4): every `SpriteDraw` `build_draws`
    /// emits (both the shadow and the piece of every pair) must carry
    /// `VerticalAnchor::Bottom` for Sideline/OverShoulder (feet-in-the-box)
    /// and `VerticalAnchor::Center` for TopDown (no verticality to anchor).
    /// Exercises the full call path (not a standalone accessor), so a
    /// mis-wired or reverted call site in `build_draws` is caught directly.
    #[test]
    fn build_draws_anchor_is_bottom_for_oblique_center_for_top_down() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);

        let cases: [(BattleCamera, VerticalAnchor); 3] = [
            (BattleCamera::sideline_preset(), VerticalAnchor::Bottom),
            (BattleCamera::over_shoulder_preset(), VerticalAnchor::Bottom),
            (BattleCamera::top_down_preset(), VerticalAnchor::Center),
        ];

        for (camera, expected_sprite_anchor) in cases {
            let geom = board_geometry(area, camera, BattleViewerTuning::default());
            let shadow_bufs = scene.shadow_buffers(&geom);
            let draws = scene.build_draws(&geom, &shadow_bufs, Duration::ZERO);
            assert!(!draws.is_empty(), "expected at least one draw for preset {camera:?}");
            // build_draws emits (shadow, sprite) pairs per piece: the shadow
            // is ALWAYS Center (a ground decal spreads symmetrically around
            // its contact point, regardless of camera), only the creature
            // sprite's anchor varies per camera.
            for (i, draw) in draws.iter().enumerate() {
                let expected = if i % 2 == 0 { VerticalAnchor::Center } else { expected_sprite_anchor };
                assert_eq!(
                    draw.vertical_anchor, expected,
                    "preset {camera:?}, draw {i} ({}): vertical_anchor must be {expected:?}",
                    if i % 2 == 0 { "shadow" } else { "sprite" }
                );
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: feet-anchored placement lands sprites at the ground point (b5-t4)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod feet_anchored_placement_tests {
    use super::*;
    use crate::scenes::test_util::{lit_dot_color, render_to_buffer};

    /// Rounding tolerance for the folded feet-vs-rendered comparison
    /// (research.md b5-t4): covers `place`'s Bottom-anchor integer dot
    /// rounding — the shadow's own outer alpha-0 padding is folded out below,
    /// so it never needs to be absorbed by this margin. Widened from `2` to
    /// `5` for spec 41's corrected Over-the-shoulder placement (Decision 1's
    /// bug fix): that preset's much taller/farther camera (needed to keep
    /// `forward_distance` positive from the correct side, behind Team B)
    /// produces a wider near/far depth-scale spread than before, so integer
    /// rounding on the far team's already-small sprite accumulates a few
    /// more dots than the old, gentler camera did.
    const FEET_TOLERANCE_DOTS: i32 = 5;

    /// True for any rendered dot color that is NOT a shade of the board's
    /// grid-line gray. `GRID_LINE_COLOR` (and its translucent Sideline/
    /// OverShoulder blends, always blended toward black) keeps R≈G≈B; both
    /// team colors (`TEAM_A_COLOR` pale gold, `TEAM_B_COLOR` pale mint) are
    /// chromatic at every alpha. Used to keep the feet-probe's bottom-up scan
    /// from landing on a board grid-line dot instead of the target piece's
    /// team-colored shadow: `draw_board_lines` draws a line through every
    /// integer world column/row, and a piece's world position (`col + 0.5`)
    /// is always within half a unit of one — under Sideline (depth axis:
    /// world-x) that line lands at nearly the piece's own screen depth;
    /// under OverShoulder (depth axis: world-y) it runs the full screen
    /// column. Either way it can land in the exact scanned cell, and only a
    /// color-based filter (not a tighter cell window) reliably excludes it.
    pub(super) fn is_chromatic(color: Rgba) -> bool {
        let (max, min) = (
            color.r.max(color.g).max(color.b),
            color.r.min(color.g).min(color.b),
        );
        max.saturating_sub(min) > 12
    }

    /// For a non-Top-Down preset, one Team A piece from the demo roster
    /// (rendered alone — see the isolation comment below for why): the
    /// BOTTOMMOST chromatic (team/creature-colored, never board-grid-gray)
    /// rendered dot in its exact projected dot column must land within
    /// `FEET_TOLERANCE_DOTS` of the camera-projected ground point, once the
    /// shadow buffer's own bottom transparent padding (the Ring's alpha-0
    /// outer edge) is folded out. Projects through `geom.framed_camera()`
    /// (not bare `geom.camera`) since `render()` composites through the
    /// offset-baked camera (fit-to-viewport, b4-t1) — see research.md's
    /// correction #1. Verified via decoded dots (`lit_dot_color`, checked at
    /// the exact dot column rather than a cell-floored one — see
    /// `lit_dot_color`'s docs for why a coarser scan can find an unrelated
    /// element instead of the shadow — and restricted to chromatic dots via
    /// `is_chromatic` so a board grid-line dot is never mistaken for the
    /// shadow), never a raw `Rect`/`DotRect` field comparison, per this
    /// project's alignment-verification convention.
    fn assert_piece_feet_land_at_ground_point(camera: BattleCamera, col: u16, row: u16) {
        let mut scene = BattleViewer::default();
        // Isolate to just the target piece: the demo roster's other 7 pieces
        // (and their own shadows) are close enough in screen-space under a
        // perspective camera's spread/depth divide that a neighbor's WIDE
        // shadow can bleed into this piece's scanned column even from a
        // different board column/row — not just an exact-column collision.
        // Keeping only the piece under test removes every such cross-piece
        // ambiguity; the board grid chrome (excluded via `is_chromatic`) is
        // the only other rendered element left to guard against.
        scene.pieces.retain(|p| p.col == col && p.row == row);
        // `render()` (battle_viewer.rs's `Scene` impl) builds its geometry
        // from `self.camera_mode`, NOT from a parameter — so the scene's own
        // camera must actually be switched to `camera` or `render_to_buffer`
        // below renders under whatever `BattleViewer::default()` starts on
        // (Sideline) regardless of which preset this assertion is computing
        // `expected` for.
        scene.camera_mode = camera;
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, camera, BattleViewerTuning::default());

        let piece = scene
            .pieces
            .first()
            .expect("demo pieces() must include a Team A piece at (col, row)");

        // Fold against the CREATURE's own rasterized buffer, not the
        // shadow's: the shadow is now ALWAYS Center-anchored (a ground decal
        // spreads symmetrically around its contact point, regardless of
        // camera — see `build_draws`), so it deliberately hangs below the
        // ground point under every camera. Only the creature sprite itself
        // carries the per-camera anchor this test is actually checking.
        let sprite = scene
            .piece_sprite(piece.index)
            .expect("Team A piece at (col, row) must have a sprite");
        let anchor = geom.camera.vertical_anchor_hint();
        let transform = depth_scaled_transform(&piece.transform, &geom.camera);
        // This helper is only ever called with a perspective preset
        // (Sideline/OverShoulder) — mirror `build_draws`'s own non-Top-Down
        // path (`sprite_base_dot_rows_width_fill`), not the Top-Down-only
        // `sprite_base_dot_rows`, so the reference buffer size matches what
        // actually renders.
        let frame = sprite.frame_at(Duration::ZERO);
        let aspect = frame.width() as f32 / frame.height().max(1) as f32;
        let base_dot_rows = sprite_base_dot_rows_width_fill(&geom.camera, piece.transform.translate, aspect);
        let creature_buf = sprite.rasterize_at(Duration::ZERO, &transform, base_dot_rows);

        let cam = geom.framed_camera();
        let (fx, fy) = cam.project(piece.transform.translate);
        let feet_dot_col = geom.board_rect.left() as i32 * 2 + fx;
        let feet_dot_row = geom.board_rect.top() as i32 * 4 + fy;

        // Fold out the creature buffer's own bottom-lit padding: the buffer-
        // local lowest LIT row (0-indexed from the top), and its distance
        // from the buffer's own last row.
        let lowest_local = (0..creature_buf.rows())
            .rev()
            .find(|&r| (0..creature_buf.cols()).any(|c| !matches!(creature_buf.get(c, r), Dot::Transparent)))
            .expect("creature buffer must have at least one lit dot");
        let bottom_padding = (creature_buf.rows() - 1 - lowest_local) as i32;

        // `place`'s Bottom anchor sets the buffer's own lowest row at grid-y
        // `fy - 1` (dot_y = py - rows, so the last row is dot_y + rows - 1 ==
        // py - 1); `place`'s Center anchor centers it on `fy` instead, so the
        // sprite's own bottom sits roughly `rows/2` below `fy`. Either way,
        // the folded padding removes the rest of the deterministic offset,
        // leaving only rounding for FEET_TOLERANCE_DOTS to absorb.
        let expected = match anchor {
            VerticalAnchor::Bottom => feet_dot_row - 1 - bottom_padding,
            VerticalAnchor::Center => {
                feet_dot_row + (creature_buf.rows() as i32) / 2 - 1 - bottom_padding
            }
        };

        let buf = render_to_buffer(&scene, area.width, area.height);
        // Find the actual BOTTOMMOST chromatic (team/creature-colored, never
        // board-grid-gray) dot in the exact projected dot COLUMN (not floored
        // to a terminal cell). The scan is capped at `expected + tolerance`
        // (never below it): the creature's own sprite, per its anchor, never
        // renders below that line — the only thing that legitimately would
        // is the shadow's below-ground overhang (it's Center-anchored and
        // therefore spreads past the ground point on purpose), which this
        // test is not checking and must not let bleed into "the creature's
        // own bottom." The isolated single-piece scene + exact dot column +
        // chromatic filter together rule out every OTHER non-anchor source
        // of a lower dot (a neighboring piece, a board grid line).
        // `ShapeKind::Ring`'s alpha profile is a genuine annulus (spec 41
        // Decision 5) — its exact geometric center (and a dot or two either
        // side of it) is deliberately hollow (`Dot::Transparent`), not lit.
        // Scanning only the single exact `feet_dot_col` can land inside that
        // hole and find nothing even though the ring is correctly rendered
        // and correctly anchored a couple of columns either side. A small
        // horizontal window is safe here (unlike a general-purpose neighbor
        // search): the scene above was already isolated to this one piece,
        // so there is no other piece/shadow left to produce a false
        // positive — only the ring's own hollow center to route around.
        const RING_HOLE_TOLERANCE_DOTS: i32 = 3;
        let scan_floor = expected + FEET_TOLERANCE_DOTS;
        let bottommost_chromatic = (feet_dot_col - RING_HOLE_TOLERANCE_DOTS
            ..=feet_dot_col + RING_HOLE_TOLERANCE_DOTS)
            .find_map(|probe_col| {
                (0..=scan_floor.min(area.height as i32 * 4 - 1))
                    .rev()
                    .find(|&dot_row| lit_dot_color(&buf, probe_col, dot_row).is_some_and(is_chromatic))
            });

        assert_eq!(
            bottommost_chromatic.map(|observed| (observed - expected).abs() <= FEET_TOLERANCE_DOTS),
            Some(true),
            "preset {camera:?}: bottommost chromatic (team/creature-colored) dot near dot column \
             {feet_dot_col} (±{RING_HOLE_TOLERANCE_DOTS}) is {bottommost_chromatic:?}, must land \
             within {FEET_TOLERANCE_DOTS} dots of the projected ground point (row {expected}) — \
             the creature must stand in its cell, not float centered on it"
        );
    }

    /// Team A's bench piece, under Sideline.
    #[test]
    fn sideline_bench_piece_feet_land_at_ground_point() {
        assert_piece_feet_land_at_ground_point(BattleCamera::sideline_preset(), BENCH_COL, TEAM_A_BENCH_ROW);
    }

    /// Team A's bench piece, under OverShoulder.
    #[test]
    fn over_shoulder_bench_piece_feet_land_at_ground_point() {
        assert_piece_feet_land_at_ground_point(BattleCamera::over_shoulder_preset(), BENCH_COL, TEAM_A_BENCH_ROW);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: bench-piece visibility regression, all three presets (b7-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod bench_visibility_tests {
    use super::*;
    use crate::scenes::test_util::render_to_buffer;
    use super::feet_anchored_placement_tests::is_chromatic;

    /// Renders the demo `pieces()` layout under `camera` both with and
    /// without the piece at `(BENCH_COL, bench_row)`, then asserts the
    /// differential (`diff_dots`, control vs. full) contains at least one
    /// chromatic (team-colored, never board-grid-gray) lit dot that only
    /// exists in the full render. `board_geometry` depends only on
    /// `area`+`camera`+`tuning` (never `pieces`), so grid lines/board chrome
    /// are byte-identical in both renders and can never produce a false
    /// positive here — the only possible dot-level differences are the
    /// removed bench piece's own sprite+shadow dots (and anything they
    /// occluded).
    fn assert_bench_piece_visible(camera: BattleCamera, bench_row: u16) {
        let full = BattleViewer {
            camera_mode: camera,
            ..BattleViewer::default()
        };

        let mut control = BattleViewer {
            camera_mode: camera,
            ..BattleViewer::default()
        };
        control.pieces.retain(|p| !(p.col == BENCH_COL && p.row == bench_row));

        assert_eq!(
            full.pieces.len(),
            control.pieces.len() + 1,
            "no bench piece matched (BENCH_COL, {bench_row}) — layout drifted, test is not \
             exercising anything"
        );

        let buf_full = render_to_buffer(&full, 100, 50);
        let buf_ctrl = render_to_buffer(&control, 100, 50);

        let diff = engine_render::diff_dots(&buf_ctrl, &buf_full);
        let visible = diff
            .mismatches
            .iter()
            .any(|m| m.actual_lit && m.actual_color.is_some_and(is_chromatic));

        assert!(
            visible,
            "preset {camera:?}: bench piece at row {bench_row} produced no distinguishable \
             chromatic dot — it is invisible/off-screen (this is exactly spec 39's 'bench not \
             visible' bug)"
        );
    }

    #[test]
    fn bench_visible_sideline() {
        assert_bench_piece_visible(BattleCamera::sideline_preset(), TEAM_A_BENCH_ROW);
        assert_bench_piece_visible(BattleCamera::sideline_preset(), TEAM_B_BENCH_ROW);
    }

    #[test]
    fn bench_visible_over_shoulder() {
        assert_bench_piece_visible(BattleCamera::over_shoulder_preset(), TEAM_A_BENCH_ROW);
        assert_bench_piece_visible(BattleCamera::over_shoulder_preset(), TEAM_B_BENCH_ROW);
    }

    #[test]
    fn bench_visible_top_down() {
        assert_bench_piece_visible(BattleCamera::top_down_preset(), TEAM_A_BENCH_ROW);
        assert_bench_piece_visible(BattleCamera::top_down_preset(), TEAM_B_BENCH_ROW);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: camera_mode default + enter()-reset contract (b5-t1)
// ─────────────────────────────────────────────────────────────────────────────

