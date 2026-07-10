use super::*;

/// Fraction of a cell's width, AT the piece's own position, that its contact
/// shadow's own outer diameter targets — deliberately smaller than
/// `WIDTH_FILL_RATIO` so the shadow reads as a mark under the creature's
/// feet, not another shape competing with it for the same footprint.
const SHADOW_WIDTH_RATIO: f32 = 0.55;

impl BattleViewer {
    /// b7-t1: pure fade scalar in `[0,1]` for `piece_index`'s contact shadow,
    /// derived from `self.elapsed`/`self.events`/`self.tuning.shadow_fade_ms`
    /// (spec Decision 4). Standing still (no relevant event, or outside every
    /// event's window) => `1.0`. When multiple events target this piece, the
    /// MIN across them wins.
    fn shadow_alpha(&self, piece_index: usize) -> f32 {
        let fade = self.tuning.shadow_fade_ms as f32 / 1000.0;
        let mut alpha = 1.0_f32;
        for ev in &self.events {
            let target = match ev.kind {
                EventKind::Move { piece_index, .. } => piece_index,
                EventKind::Die { piece_index } => piece_index,
            };
            if target != piece_index {
                continue;
            }
            let t_start = ev.start_time;
            let t_end = ev.start_time + ev.duration;
            let a = if self.elapsed < t_start {
                1.0
            } else if fade > 0.0 && self.elapsed < t_start + fade {
                1.0 - (self.elapsed - t_start) / fade
            } else if self.elapsed < t_end {
                0.0
            } else if fade > 0.0 && self.elapsed < t_end + fade {
                (self.elapsed - t_end) / fade
            } else {
                1.0
            };
            alpha = alpha.min(a);
        }
        alpha
    }

    /// b7-t1/b1-t4: one team-colored `rasterize_shape(ShapeKind::Ring, ...)`
    /// buffer per drawable piece, same iteration order `build_draws` shares
    /// (`drawable_pieces`). Width is `SHADOW_WIDTH_RATIO` of a cell-width AT
    /// EACH PIECE'S OWN POSITION (`BattleCamera::local_dots_per_world_unit`)
    /// — sized per piece, not once per frame off a single flat cell metric,
    /// so a far piece's shadow is visibly smaller than a near piece's, the
    /// same way its sprite is. Height squashes by the camera's elevation
    /// sine (`k`) so a low, oblique camera flattens the annulus while
    /// Top-Down (`k=1`) keeps it round. The Ring's exact center is the
    /// alpha-0 hole; alpha peaks in a thin band near the outer edge.
    pub(super) fn shadow_buffers(&self, geom: &BoardGeometry) -> Vec<DotBuffer> {
        let k = geom.camera.elevation_deg().to_radians().sin();
        self.drawable_pieces()
            .map(|(p, _)| {
                let rate = geom.camera.local_dots_per_world_unit(p.transform.translate);
                // `| 1` forces width (and, from it, height) odd so
                // `rasterize_shape`'s center (`(size-1)/2`) always lands on a
                // real dot — for `Ring` that center dot is the alpha-0 hole.
                // `.min(MAX_SPRITE_DOT_DIMENSION)` — see that constant's doc
                // comment: `rate` is unbounded near the camera, and this
                // allocates+rasterizes a `w*h`-dot buffer.
                let w = ((rate * SHADOW_WIDTH_RATIO).round().max(1.0) as usize)
                    .min(MAX_SPRITE_DOT_DIMENSION as usize) | 1;
                let h = ((((w as f32) * k).round().max(1.0)) as usize)
                    .min(MAX_SPRITE_DOT_DIMENSION as usize) | 1;
                let alpha = self.shadow_alpha(p.index);
                let col = Rgba::new(p.color.r, p.color.g, p.color.b, (255.0 * alpha).round() as u8);
                rasterize_shape(ShapeKind::Ring, w, h, col)
            })
            .collect()
    }

}

#[cfg(test)]
mod contact_shadow_tests {
    use super::*;

    /// PUBLIC_SURFACE case 1 (unit on `shadow_alpha`): a standing-still
    /// piece — `elapsed == 0.0`, before `demo_events`' `start_time`s — has no
    /// active event, so `shadow_alpha` must be exactly `1.0`.
    #[test]
    fn shadow_alpha_full_when_standing_still() {
        let scene = BattleViewer::default();
        assert_eq!(
            scene.shadow_alpha(0),
            1.0,
            "a piece with no active event (elapsed before every event's start_time) must be full shadow alpha"
        );
    }

    /// PUBLIC_SURFACE case 2: sampled partway through a `Move` longer than
    /// the fade window (`demo_events`' piece-0 Move: [1.0, 2.2), fade =
    /// `shadow_fade_ms/1000 = 0.15`), `shadow_alpha` must be exactly `0.0`.
    #[test]
    fn shadow_alpha_zero_mid_move_past_fade() {
        let scene = BattleViewer { elapsed: 1.5, ..BattleViewer::default() }; // 1.0+0.15=1.15 <= 1.5 < 2.2
        assert_eq!(
            scene.shadow_alpha(0),
            0.0,
            "sampled well inside the move's window, past the fade-out, alpha must be exactly 0.0"
        );
    }

    /// PUBLIC_SURFACE case 3: immediately after the move's window closes
    /// (`t_end = 2.2`, fade window `[2.2, 2.35)`), `shadow_alpha` must be
    /// partway back up from 0 toward 1, matching the fade-in formula exactly.
    #[test]
    fn shadow_alpha_fades_back_in_after_move_window() {
        let scene = BattleViewer { elapsed: 2.3, ..BattleViewer::default() }; // (2.3 - 2.2) / 0.15 = 0.6666...
        let alpha = scene.shadow_alpha(0);
        assert!(
            alpha > 0.0 && alpha < 1.0,
            "expected a partial fade-in value in (0,1), got {alpha}"
        );
        let expected = (2.3_f32 - 2.2_f32) / 0.15_f32;
        assert!(
            (alpha - expected).abs() < 1e-3,
            "expected shadow_alpha ~= {expected}, got {alpha}"
        );
    }

    /// Helper (b1-t4): scan `buf` for its highest-alpha `Lit` dot, returning
    /// `(col, row, color)`. Used to locate `ShapeKind::Ring`'s off-center
    /// peak band without pinning `RING_PEAK` or any exact alpha value.
    fn max_alpha_dot(buf: &DotBuffer) -> (usize, usize, Rgba) {
        let mut best: Option<(usize, usize, Rgba)> = None;
        for row in 0..buf.rows() {
            for col in 0..buf.cols() {
                if let Dot::Lit(c) = buf.get(col, row) {
                    if best.is_none_or(|(_, _, b)| c.a > b.a) {
                        best = Some((col, row, c));
                    }
                }
            }
        }
        best.expect("expected at least one Lit dot in the shadow buffer")
    }

    /// PUBLIC_SURFACE case 1 (shadow_buffers, all presets — b1-t4): the
    /// shadow buffer is a team-colored `ShapeKind::Ring` — the exact
    /// geometric-center dot is the transparent hole (never `Lit`), and the
    /// buffer's max-alpha dot sits off-center in the mid band. Replaces
    /// `shadow_center_dot_full_alpha_when_standing_still`'s Ellipse-shaped
    /// contract (full alpha at dead center), which no longer holds once the
    /// shadow's primitive swaps from `Ellipse` to `Ring`.
    #[test]
    fn shadow_is_ring_signature_all_presets() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        for camera in [
            BattleCamera::top_down_preset(),
            BattleCamera::sideline_preset(),
            BattleCamera::over_shoulder_preset(),
        ] {
            let geom = board_geometry(area, camera, BattleViewerTuning::default());
            let bufs = scene.shadow_buffers(&geom);
            assert!(!bufs.is_empty(), "expected at least one shadow buffer for a drawable piece");
            let buf = &bufs[0];
            let (cx, cy) = (buf.cols() / 2, buf.rows() / 2);
            assert_eq!(
                buf.get(cx, cy),
                Dot::Transparent,
                "Ring's exact geometric center must be the transparent hole, not Lit"
            );
            let (peak_col, peak_row, peak) = max_alpha_dot(buf);
            assert!(peak.a > 0, "expected the shadow's peak dot to be Lit with alpha > 0");
            assert_ne!(
                (peak_col, peak_row),
                (cx, cy),
                "the max-alpha dot must be off-center (the mid band), not the hole"
            );
        }
    }

    /// Replaces `render_reflects_mutated_stored_piece_color` (b3-t2), rewritten
    /// for `ShapeKind::Ring` (b1-t4): after this task detints the piece
    /// sprite, team color lives ONLY on the shadow — mutating a stored
    /// `piece.color` must change what `shadow_buffers` bakes into the Ring's
    /// off-center peak dot RGB (the center is now the transparent hole and
    /// can carry no RGB), proving the shadow reads live stored state instead
    /// of a fixed team default.
    #[test]
    fn shadow_buffers_reflect_mutated_stored_piece_color() {
        let mut scene = BattleViewer::default();
        for p in &mut scene.pieces {
            p.color = Rgba::rgb(11, 22, 33);
        }
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::sideline_preset(), BattleViewerTuning::default());

        let bufs = scene.shadow_buffers(&geom);
        assert!(!bufs.is_empty(), "expected at least one shadow buffer for a drawable piece");
        let buf = &bufs[0];
        let (cx, cy) = (buf.cols() / 2, buf.rows() / 2);
        assert_eq!(
            buf.get(cx, cy),
            Dot::Transparent,
            "Ring's exact geometric center must be the transparent hole and can carry no RGB"
        );
        let (_, _, peak) = max_alpha_dot(buf);
        assert_eq!(
            (peak.r, peak.g, peak.b),
            (11, 22, 33),
            "shadow's off-center peak dot RGB must reflect the mutated stored piece.color, not a team default"
        );
    }

    /// PUBLIC_SURFACE case 4: the piece's own `SpriteDraw.tint` must be
    /// `None` — team color no longer multiply-tints the piece sprite
    /// (b7-t1 detint; carried only by the shadow).
    #[test]
    fn piece_own_sprite_tint_is_none() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::sideline_preset(), BattleViewerTuning::default());
        let shadow_bufs = scene.shadow_buffers(&geom);
        let draws = scene.build_draws(&geom, &shadow_bufs, Duration::ZERO);

        assert!(draws.len() >= 2, "expected at least one shadow+piece pair");
        for pair in draws.chunks(2) {
            let piece = &pair[1];
            assert!(
                matches!(piece.content, SpriteContent::Animated { .. }),
                "the second entry of each pair must be the piece's own Animated draw"
            );
            assert_eq!(piece.tint, None, "the piece's own SpriteDraw.tint must be None");
        }
    }

    /// Regression: a free-roam-style camera positioned essentially AT a
    /// piece's own world position drives `forward_distance` down to its
    /// `NEAR_EPS` floor, spiking `local_dots_per_world_unit` into the
    /// thousands. Before the `MAX_SPRITE_DOT_DIMENSION` cap, this allocated
    /// and rasterized a tens-of-millions-of-dots shadow buffer every frame —
    /// a real shipped hang (the game freezes whenever the camera passes near
    /// the board's midpoint, where the demo pieces stand). Both dimensions
    /// must stay within the documented ceiling regardless.
    #[test]
    fn shadow_buffers_stay_capped_when_camera_is_at_piece_position() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let piece_pos = scene.pieces[0].transform.translate;
        let camera = BattleCamera {
            camera: AnyCamera::Perspective(PerspectiveCamera {
                x: piece_pos.x,
                y: piece_pos.y,
                height: 0.0,
                yaw_deg: 0.0,
                pitch_deg: 0.0,
                fov_deg: 55.0,
                scale_dots: 40.0,
            }),
            fit: FitMode::Manual,
        };
        let geom = board_geometry(area, camera, BattleViewerTuning::default());

        let bufs = scene.shadow_buffers(&geom);
        assert!(!bufs.is_empty(), "expected at least one shadow buffer for a drawable piece");
        for buf in &bufs {
            assert!(
                buf.cols() as u32 <= MAX_SPRITE_DOT_DIMENSION,
                "shadow buffer width {} exceeds MAX_SPRITE_DOT_DIMENSION ({}) — the near-camera cap regressed",
                buf.cols(),
                MAX_SPRITE_DOT_DIMENSION
            );
            assert!(
                buf.rows() as u32 <= MAX_SPRITE_DOT_DIMENSION,
                "shadow buffer height {} exceeds MAX_SPRITE_DOT_DIMENSION ({}) — the near-camera cap regressed",
                buf.rows(),
                MAX_SPRITE_DOT_DIMENSION
            );
        }
    }

    /// PUBLIC_SURFACE case 5: each shadow entry in `build_draws`' output is
    /// `Prerasterized`, carries `tint: None`, shares `translate` with the
    /// following piece entry, and sits immediately before it (order =
    /// shadow, piece, shadow, piece, ...).
    #[test]
    fn shadow_precedes_own_piece_and_shares_translate() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::sideline_preset(), BattleViewerTuning::default());
        let shadow_bufs = scene.shadow_buffers(&geom);
        let draws = scene.build_draws(&geom, &shadow_bufs, Duration::ZERO);

        assert_eq!(
            draws.len(),
            shadow_bufs.len() * 2,
            "expected exactly one shadow draw + one piece draw per drawable piece"
        );
        assert!(draws.len() >= 2, "expected at least one drawable piece in the default scene");

        for pair in draws.chunks(2) {
            let (shadow, piece) = (&pair[0], &pair[1]);
            assert!(
                matches!(shadow.content, SpriteContent::Prerasterized(_)),
                "shadow entry must be SpriteContent::Prerasterized"
            );
            assert_eq!(shadow.tint, None, "shadow entry's tint must be None (color is baked into the shadow shape)");
            assert_eq!(
                shadow.translate, piece.translate,
                "shadow must share the very next entry's (its own piece's) translate"
            );
        }
    }

    /// PUBLIC_SURFACE case 6: shadow vertical squash differs by camera
    /// elevation. Top-Down (`elevation_deg = 90.0`, `k = sin(90°) = 1.0`)
    /// shadows are round (`rows() == cols()`); Sideline
    /// (`elevation_deg = 10.0`, `k = sin(10°) ≈ 0.17`) shadows are
    /// flattened (`rows()` much smaller than Top-Down's for the same width).
    #[test]
    fn shadow_squash_round_topdown_flat_sideline() {
        let scene = BattleViewer::default();
        let area = Rect::new(0, 0, 100, 50);

        let geom_top = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());
        let geom_side = board_geometry(area, BattleCamera::sideline_preset(), BattleViewerTuning::default());
        // Shadow width is now per-piece-position (`local_dots_per_world_unit`),
        // not a single camera-independent `cell_width_cols`-derived constant
        // — so TopDown's and Sideline's `cell_width_cols` are no longer
        // expected to match for the same area (TopDown also reserves an
        // extra column for `BENCH_COL`, Sideline doesn't need to). The
        // property this test actually cares about — round vs. flattened
        // shape — doesn't depend on that equality.

        let top_bufs = scene.shadow_buffers(&geom_top);
        let side_bufs = scene.shadow_buffers(&geom_side);
        assert!(!top_bufs.is_empty() && !side_bufs.is_empty(), "expected at least one shadow buffer each");

        let (topdown, sideline) = (&top_bufs[0], &side_bufs[0]);
        assert_eq!(
            topdown.cols(), topdown.rows(),
            "Top-Down (k=1) shadow must be round: rows() == cols()"
        );
        assert!(
            sideline.rows() < topdown.rows(),
            "Sideline (k~=0.17) shadow must be flattened relative to Top-Down: sideline.rows()={} topdown.rows()={}",
            sideline.rows(), topdown.rows()
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: billboarding invariant regression (b8-t1) — every piece's own
// Animated transform is never camera-rotated and never sheared non-uniformly,
// across all 3 camera presets, standing still and with active Move/Die events.
// ─────────────────────────────────────────────────────────────────────────────

