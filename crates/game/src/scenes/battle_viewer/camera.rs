use super::*;

/// Which fit strategy `board_geometry` uses for this camera (spec 42
/// Decision 2/4 consolidation, b2-t1). Replaces keying dispatch on the old
/// 3-variant `AnyCamera` (which had a distinct `FreeRoam` variant) now that
/// `AnyCamera` has only 2 variants and `Sideline`/`OverShoulder`/`FreeRoam`
/// all wrap the same `Perspective` variant — `fit` is the thing that
/// actually distinguishes their geometry/dispatch behavior.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FitMode {
    /// TopDown: exact integer cell-sizing fit (today's `Orthographic` arm).
    ExactFit,
    /// Sideline / Over-the-shoulder: fit-to-viewport bbox solve.
    ViewportFit,
    /// Free-roam: no auto-fit — the user controls framing directly.
    Manual,
}

/// Wraps the engine's `AnyCamera` kind-dispatch (spec 42 Decision 2) for the
/// battle viewer, plus which fit strategy applies (spec 42 Decision 4
/// consolidation, b2-t1). Exact passthrough on every `Camera` method: each
/// delegates to `self.camera` — no recompute at this layer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BattleCamera {
    pub camera: AnyCamera,
    pub fit: FitMode,
}

impl Camera for BattleCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        self.camera.project(pos)
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        self.camera.depth_key(pos)
    }

    fn vertical_anchor_hint(&self) -> VerticalAnchor {
        self.camera.vertical_anchor_hint()
    }

    fn elevation_deg(&self) -> f32 {
        self.camera.elevation_deg()
    }

    /// Dots per world unit AT `pos` specifically (not a single near-reference
    /// value like the deleted `sprite_scale_dots`) — i.e. "how many dots wide
    /// is exactly one board cell, right where this piece actually stands."
    /// This is what per-piece sprite and shadow sizing key off so they
    /// shrink smoothly with distance instead of using one flat per-frame
    /// size for every piece regardless of how far away it actually is.
    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32 {
        self.camera.local_dots_per_world_unit(pos)
    }
}

impl BattleCamera {
    /// Grid-line prominence for `draw_board_lines`: full-strength, opaque
    /// `GRID_LINE_COLOR` when the active camera is flat/straight-down
    /// (elevation `90.0`, i.e. `TopDown`); a translucent
    /// `Rgba::new(0xFF,0xFF,0xFF,tuning.grid_dim_alpha)` for any elevated
    /// camera (Sideline 10°, OverShoulder 30°) that blends via the real
    /// alpha-blit path (b1-t3) rather than a flat dark constant.
    pub fn grid_line_color(&self, tuning: &BattleViewerTuning) -> Rgba {
        if self.camera.elevation_deg() < 89.5 {
            Rgba::new(0xFF, 0xFF, 0xFF, tuning.grid_dim_alpha)
        } else {
            GRID_LINE_COLOR
        }
    }

    /// Rebuild the active variant at `scale_dots`, preserving every other
    /// variant-specific world param AND `fit`. Scale is always area-derived
    /// (see `board_geometry`), so the incoming variant's own scale is
    /// intentionally replaced, not read.
    pub(super) fn with_scale_dots(self, scale_dots: f32) -> Self {
        BattleCamera { camera: self.camera.with_scale_dots(scale_dots), fit: self.fit }
    }

    /// Sideline preset: looks down the world-x axis (yaw 90°), mild
    /// elevation, anchored on the board's horizontal center column.
    /// `SIDELINE_CAMERA_DEPTH` sits strictly outside the occupied
    /// `[0, BOARD_COLS]` range (negative side) so every board cell projects
    /// with a comfortably positive `forward_distance` — no near-plane
    /// clamping.
    pub fn sideline_preset() -> Self {
        BattleCamera {
            camera: AnyCamera::Perspective(PerspectiveCamera {
                x: SIDELINE_CAMERA_DEPTH,
                y: BOARD_CENTER_COL,
                height: 2.5,
                yaw_deg: 90.0,
                pitch_deg: 10.0,
                fov_deg: 55.0,
                scale_dots: 0.0,
            }),
            fit: FitMode::ViewportFit,
        }
    }

    /// Over-the-shoulder preset: looks down the world-y (team-separation)
    /// axis (yaw 180°) from behind Team B's bench row. `OVER_SHOULDER_CAMERA_DEPTH`
    /// sits strictly outside the occupied `[0, BOARD_ROWS]` range on the
    /// POSITIVE side (past `TEAM_B_BENCH_ROW`, not before row 0) so the shot
    /// is genuinely from behind Team B, and `OVER_SHOULDER_CAMERA_HEIGHT` is
    /// tall enough to keep `forward_distance` positive all the way to the
    /// far edge (row 0) — see both constants' doc comments.
    pub fn over_shoulder_preset() -> Self {
        BattleCamera {
            camera: AnyCamera::Perspective(PerspectiveCamera {
                x: BOARD_CENTER_COL,
                y: OVER_SHOULDER_CAMERA_DEPTH,
                height: OVER_SHOULDER_CAMERA_HEIGHT,
                yaw_deg: 180.0,
                pitch_deg: 30.0,
                fov_deg: 55.0,
                scale_dots: 0.0,
            }),
            fit: FitMode::ViewportFit,
        }
    }

    /// Top-down preset: straight-down plan view, true orthographic projection
    /// (spec 42 Decision 0) — no tilt, no taper, no depth-anchor.
    pub fn top_down_preset() -> Self {
        BattleCamera {
            camera: AnyCamera::Orthographic(OrthographicCamera::new(0.0)),
            fit: FitMode::ExactFit,
        }
    }

    /// Free-roam preset (spec 42 Decision 5, dev-only 4th mode, key `4`):
    /// pinned starting transform, same `PerspectiveCamera` shape as the
    /// fixed presets (spec 42 Decision 4 consolidation, b2-t1) — `fit:
    /// FitMode::Manual` is what makes it behave differently (no auto-fit,
    /// movement keys enabled), not a distinct camera kind.
    pub fn free_roam_preset() -> Self {
        BattleCamera {
            camera: AnyCamera::Perspective(PerspectiveCamera {
                x: SIDELINE_CAMERA_DEPTH,
                y: BOARD_CENTER_COL,
                height: 2.5,
                yaw_deg: 90.0,
                pitch_deg: 10.0,
                fov_deg: 55.0,
                scale_dots: 40.0,
            }),
            fit: FitMode::Manual,
        }
    }
}

/// World-y the over-shoulder camera sits behind: strictly outside the
/// occupied `[0, BOARD_ROWS]` range on the POSITIVE side (past the board's
/// own far edge, beyond Team B's own active/bench rows), so the shot is
/// genuinely from behind Team B looking at Team A — not the mirror image.
/// Camera yaw 180° tells `forward_distance` which way the camera looks, so
/// `forward_distance` stays positive for ANY `height` here — no tall-camera
/// workaround needed (an earlier version of this constant paired a much
/// taller camera height with this depth specifically to route around a
/// since-fixed bug where the formula assumed a fixed facing direction; that
/// workaround produced a narrow, over-steep-looking shot and is gone now
/// that yaw carries the facing). A comfortable margin past `BOARD_ROWS` (not
/// just barely past it) matters for a different reason:
/// `sprite_base_dot_rows_width_fill` (depth-scale/sprite-size baseline)
/// probes forward_distance AT the board boundary itself — too thin a margin
/// puts that boundary almost on top of the camera, blowing up the near/far
/// size ratio until the far piece rasterizes to a near-invisible handful of
/// dots (this shipped as a real bug at margin `0.5`).
const OVER_SHOULDER_CAMERA_DEPTH: f32 = 10.0;
/// World-units-above-ground the over-shoulder camera sits at — a modest,
/// human/creature shoulder-height scale (matching Sideline's `2.5`), not
/// constrained by keeping `forward_distance` positive (the yaw-180 facing
/// already guarantees that regardless of height).
const OVER_SHOULDER_CAMERA_HEIGHT: f32 = 3.0;

/// World-x/-y the sideline camera anchors its depth on, strictly outside the
/// occupied `[0, BOARD_COLS]` board range on the negative side, so every
/// board depth sits comfortably in front of the camera.
const SIDELINE_CAMERA_DEPTH: f32 = -4.0;

#[cfg(test)]
mod battle_camera_tests {
    use super::*;

    /// Builds a standalone `PerspectiveCamera` (backs `Sideline`/`OverShoulder`/
    /// `FreeRoam` after b2-t1's consolidation) with arbitrary params — mirrors
    /// `oblique`'s role for the migrated variants. `x`/`y` stand in for the
    /// old `camera_depth`/`spread_center` pair; `yaw_deg` is fixed at 90.0
    /// (arbitrary — these tests only check exact passthrough dispatch, not
    /// any particular facing).
    fn perspective(x: f32, y: f32, pitch_deg: f32, scale: f32) -> PerspectiveCamera {
        PerspectiveCamera {
            x,
            y,
            height: 2.5,
            yaw_deg: 90.0,
            pitch_deg,
            fov_deg: 55.0,
            scale_dots: scale,
        }
    }

    /// Sideline variant must be an exact passthrough to the wrapped `PerspectiveCamera`.
    #[test]
    fn sideline_project_and_depth_key_match_wrapped_perspective() {
        let inner = perspective(-4.0, 3.5, 10.0, 4.0);
        let cam = BattleCamera { camera: AnyCamera::Perspective(inner), fit: FitMode::ViewportFit };
        let pos = WorldPos::new(1.5, 2.5);
        assert_eq!(cam.project(pos), inner.project(pos));
        assert_eq!(cam.depth_key(pos), inner.depth_key(pos));
    }

    /// TopDown variant must be an exact passthrough to the wrapped `OrthographicCamera`.
    #[test]
    fn topdown_project_and_depth_key_match_wrapped_orthographic() {
        let inner = OrthographicCamera { scale_dots: 4.0 };
        let cam = BattleCamera { camera: AnyCamera::Orthographic(inner), fit: FitMode::ExactFit };
        let pos = WorldPos::new(1.5, 2.5);
        assert_eq!(cam.project(pos), inner.project(pos));
        assert_eq!(cam.depth_key(pos), inner.depth_key(pos));
    }

    /// OverShoulder variant must be an exact passthrough to the wrapped
    /// `PerspectiveCamera`, including its construction params (`x`) —
    /// checked at a position off the camera's depth so the perspective-divide
    /// terms are actually exercised (proves forwarding, not a fresh recompute).
    #[test]
    fn overshoulder_project_and_depth_key_match_wrapped_perspective() {
        let inner = perspective(3.5, -2.0, 30.0, 4.0);
        let cam = BattleCamera { camera: AnyCamera::Perspective(inner), fit: FitMode::ViewportFit };
        let pos = WorldPos::new(1.5, 5.0); // off camera depth (-2.0)
        assert_eq!(cam.project(pos), inner.project(pos));
        assert_eq!(cam.depth_key(pos), inner.depth_key(pos));
    }

    /// `BattleCamera::elevation_deg()` returns the wrapped camera's own
    /// `pitch_deg` for `Sideline`/`OverShoulder`, and the permanent literal
    /// `90.0` for `TopDown` (`OrthographicCamera` carries no elevation field
    /// of its own) — `depth_scale_factor`/`shadow_buffers` read through this
    /// every frame.
    #[test]
    fn elevation_deg_returns_wrapped_camera_elevation_for_each_variant() {
        let side = perspective(-4.0, 3.5, 11.0, 4.0);
        let top = OrthographicCamera { scale_dots: 4.0 };
        let over = perspective(3.5, -3.0, 33.0, 4.0);
        assert_eq!(
            BattleCamera { camera: AnyCamera::Perspective(side), fit: FitMode::ViewportFit }.elevation_deg(),
            11.0
        );
        assert_eq!(
            BattleCamera { camera: AnyCamera::Orthographic(top), fit: FitMode::ExactFit }.elevation_deg(),
            90.0
        );
        assert_eq!(
            BattleCamera { camera: AnyCamera::Perspective(over), fit: FitMode::ViewportFit }.elevation_deg(),
            33.0
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: Sideline/OverShoulder -> PerspectiveCamera migration invariants
// (b3-t1 PUBLIC_SURFACE #2/#3, research.md). Property-only per the no-
// pinned-constant guardrail — never a single hardcoded "correct" preset
// constant. Framing (on-screen position) is explicitly NOT covered here;
// that is b4-t1's fit-to-viewport job (research.md's documented deferral).
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod camera_migration_tests {
    use super::*;

    /// Comfortably above the engine's private `NEAR_EPS=0.01` clamp floor —
    /// a local margin so this test never needs to reference the engine's
    /// private constant directly.
    const FORWARD_DISTANCE_MARGIN: f32 = 1.0;

    /// PUBLIC_SURFACE #2: `forward_distance` must be comfortably positive
    /// for every occupied board cell under `sideline_preset()`.
    #[test]
    fn sideline_forward_distance_positive_all_cells() {
        let AnyCamera::Perspective(cam) = BattleCamera::sideline_preset().camera else {
            unreachable!()
        };
        for col in 0..BOARD_COLS {
            for row in 0..BOARD_ROWS {
                let pos = world_pos_for_cell(col, row);
                let d = cam.forward_distance(pos);
                assert!(
                    d > FORWARD_DISTANCE_MARGIN,
                    "sideline_preset forward_distance at cell ({col},{row}) must exceed the \
                     margin ({FORWARD_DISTANCE_MARGIN}), got {d} (pos={pos:?})"
                );
            }
        }
    }

    /// PUBLIC_SURFACE #2, `over_shoulder_preset()` counterpart.
    #[test]
    fn over_shoulder_forward_distance_positive_all_cells() {
        let AnyCamera::Perspective(cam) = BattleCamera::over_shoulder_preset().camera else {
            unreachable!()
        };
        for col in 0..BOARD_COLS {
            for row in 0..BOARD_ROWS {
                let pos = world_pos_for_cell(col, row);
                let d = cam.forward_distance(pos);
                assert!(
                    d > FORWARD_DISTANCE_MARGIN,
                    "over_shoulder_preset forward_distance at cell ({col},{row}) must exceed the \
                     margin ({FORWARD_DISTANCE_MARGIN}), got {d} (pos={pos:?})"
                );
            }
        }
    }

    /// PUBLIC_SURFACE #3: each non-Top-Down preset sits strictly outside its
    /// occupied board range on the correct side, expressed on x/y (b2-t1;
    /// the deleted `camera_depth` field's replacement) — Sideline depth is
    /// `x`, Over-shoulder depth is `y`.
    #[test]
    fn non_top_down_camera_depth_outside_board_range() {
        let AnyCamera::Perspective(side) = BattleCamera::sideline_preset().camera else {
            unreachable!()
        };
        assert!(
            side.x < 0.0 || side.x > BOARD_COLS as f32,
            "sideline_preset's x ({}) must sit strictly outside [0, {}] (the occupied \
             board range along its depth axis)",
            side.x,
            BOARD_COLS
        );

        let AnyCamera::Perspective(over) = BattleCamera::over_shoulder_preset().camera else {
            unreachable!()
        };
        assert!(
            over.y < 0.0 || over.y > BOARD_ROWS as f32,
            "over_shoulder_preset's y ({}) must sit strictly outside [0, {}] (the \
             occupied board range along its depth axis)",
            over.y,
            BOARD_ROWS
        );
    }

    /// The two candidate depth-axis coordinates at the board's extremes
    /// (the first/last row-or-column's cell center), returned as `(nearer,
    /// farther)` ordered by their OWN `forward_distance` at the camera's
    /// spread coordinate (not by raw distance from the camera's depth
    /// coordinate) — a raw-distance heuristic is fooled by the `NEAR_EPS`
    /// clamp: a candidate technically "closer" by subtraction can actually
    /// be BEHIND the camera plane (negative raw forward, clamped near zero),
    /// which blows its projected spread up numerically without that being
    /// genuine convergence. Ordering by the real (clamped) `forward_distance`
    /// avoids that false positive. `depth_is_x` selects which world axis is
    /// this camera's depth axis (Sideline: x: yaw 90°; OverShoulder: y: yaw 180°).
    fn near_far_depths(cam: &PerspectiveCamera, depth_is_x: bool, board_extent: u16) -> (f32, f32) {
        let low = 0.5;
        let high = board_extent as f32 - 0.5;
        let forward_at = |d: f32| {
            let pos = if depth_is_x {
                WorldPos::new(d, spread_coord(cam, depth_is_x))
            } else {
                WorldPos::new(spread_coord(cam, depth_is_x), d)
            };
            cam.forward_distance(pos)
        };
        if forward_at(low) <= forward_at(high) {
            (low, high)
        } else {
            (high, low)
        }
    }

    /// The camera's own aim-line coordinate on the non-depth (spread) axis —
    /// Sideline's is `y`, OverShoulder's is `x` (the deleted `spread_center`
    /// field's replacement).
    fn spread_coord(cam: &PerspectiveCamera, depth_is_x: bool) -> f32 {
        if depth_is_x { cam.y } else { cam.x }
    }

    /// Projects `cam` at a fixed depth-axis coordinate `depth` across every
    /// spread-axis cell center and returns the range (`max - min`) of the
    /// resulting screen_x — a proxy for "how wide this depth slice reads on
    /// screen," used to prove/disprove real perspective convergence without
    /// needing fit-to-viewport framing (b4-t1, not yet landed).
    fn column_spread(cam: &BattleCamera, depth_is_x: bool, depth: f32) -> i32 {
        let spread_extent = if depth_is_x { BOARD_ROWS } else { BOARD_COLS };
        let xs: Vec<i32> = (0..spread_extent)
            .map(|i| {
                let spread = i as f32 + 0.5;
                let pos = if depth_is_x {
                    WorldPos::new(depth, spread)
                } else {
                    WorldPos::new(spread, depth)
                };
                cam.project(pos).0
            })
            .collect();
        xs.iter().max().unwrap() - xs.iter().min().unwrap()
    }

    /// Interim convergence-direction evidence (mirrors the retired
    /// oblique-taper-camera-based test this replaces): a real perspective camera
    /// must converge — the depth-axis coordinate nearer the camera projects
    /// a WIDER spread of screen-x across the spread axis than the farther
    /// one. Computable without fit-to-viewport framing.
    #[test]
    fn sideline_near_depth_spread_exceeds_far_depth() {
        let cam = BattleCamera::sideline_preset().with_scale_dots(8.0);
        let AnyCamera::Perspective(inner) = cam.camera else {
            unreachable!()
        };
        let (near_depth, far_depth) = near_far_depths(&inner, true, BOARD_COLS);
        let near_forward = inner.forward_distance(WorldPos::new(near_depth, inner.y));
        let far_forward = inner.forward_distance(WorldPos::new(far_depth, inner.y));
        assert!(
            near_forward > FORWARD_DISTANCE_MARGIN && far_forward > FORWARD_DISTANCE_MARGIN,
            "both candidate depths must be comfortably in front of the camera \
             (forward_distance > {FORWARD_DISTANCE_MARGIN}) before a convergence-direction \
             comparison is meaningful; got near_forward={near_forward} far_forward={far_forward} \
             (x={})",
            inner.x
        );

        let near_spread = column_spread(&cam, true, near_depth);
        let far_spread = column_spread(&cam, true, far_depth);
        assert!(
            near_spread > far_spread,
            "a depth-axis coordinate nearer the camera ({near_depth}) must project a WIDER \
             spread of screen-x than one farther away ({far_depth}): near_spread={near_spread} \
             far_spread={far_spread}"
        );
    }

    /// `over_shoulder_preset()` counterpart of
    /// `sideline_near_depth_spread_exceeds_far_depth` (depth axis y).
    #[test]
    fn over_shoulder_near_depth_spread_exceeds_far_depth() {
        let cam = BattleCamera::over_shoulder_preset().with_scale_dots(8.0);
        let AnyCamera::Perspective(inner) = cam.camera else {
            unreachable!()
        };
        let (near_depth, far_depth) = near_far_depths(&inner, false, BOARD_ROWS);
        let near_forward = inner.forward_distance(WorldPos::new(inner.x, near_depth));
        let far_forward = inner.forward_distance(WorldPos::new(inner.x, far_depth));
        assert!(
            near_forward > FORWARD_DISTANCE_MARGIN && far_forward > FORWARD_DISTANCE_MARGIN,
            "both candidate depths must be comfortably in front of the camera \
             (forward_distance > {FORWARD_DISTANCE_MARGIN}) before a convergence-direction \
             comparison is meaningful; got near_forward={near_forward} far_forward={far_forward} \
             (y={})",
            inner.y
        );

        let near_spread = column_spread(&cam, false, near_depth);
        let far_spread = column_spread(&cam, false, far_depth);
        assert!(
            near_spread > far_spread,
            "a depth-axis coordinate nearer the camera ({near_depth}) must project a WIDER \
             spread of screen-x than one farther away ({far_depth}): near_spread={near_spread} \
             far_spread={far_spread}"
        );
    }
}

#[cfg(test)]
mod camera_mode_tests {
    use super::*;
    use engine_core::scene::{EngineCtx, Scene};

    fn default_camera() -> BattleCamera {
        BattleViewer::default_camera_mode()
    }

    /// `BattleViewer::default()` must start on the pinned default camera
    /// (Sideline), matching today's pre-b5-t1 hardcoded render behavior.
    #[test]
    fn default_battle_viewer_starts_on_default_camera() {
        let scene = BattleViewer::default();
        assert_eq!(
            scene.camera_mode,
            default_camera(),
            "BattleViewer::default() must initialize camera_mode to the pinned default"
        );
    }

    /// DELIVERABLE: `enter()` resets `camera_mode` to the default even if it
    /// was previously left on a different variant (TopDown) — a prior
    /// session's/scene-visit's camera choice must never leak into the next
    /// entry.
    #[test]
    fn enter_resets_camera_mode_from_top_down_to_default() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..Default::default()
        };

        let mut ctx = EngineCtx;
        scene.enter(&mut ctx, None);

        assert_eq!(
            scene.camera_mode,
            default_camera(),
            "enter() must reset camera_mode to the default even after it was set to TopDown"
        );
    }

    /// Same reset contract, starting from OverShoulder instead of TopDown —
    /// guards against a fix that only special-cases one non-default variant.
    #[test]
    fn enter_resets_camera_mode_from_over_shoulder_to_default() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::over_shoulder_preset(),
            ..Default::default()
        };

        let mut ctx = EngineCtx;
        scene.enter(&mut ctx, None);

        assert_eq!(
            scene.camera_mode,
            default_camera(),
            "enter() must reset camera_mode to the default even after it was set to OverShoulder"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: handle_input maps keys 1/2/3 to direct camera selection (b5-t2)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod handle_input_camera_tests {
    use super::*;
    use crate::registry::GameCatalog;
    use crate::scenes::test_util::key_event;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use engine_core::scene::manager::SceneManager;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Direct, non-cycling selection: from every starting camera_mode,
    /// pressing '1'/'2'/'3' always jumps to the same fixed variant — never a
    /// step relative to the current mode.
    #[test]
    fn digit_keys_select_camera_mode_directly_from_every_start() {
        let starts: [BattleCamera; 3] = [
            BattleCamera::sideline_preset(),
            BattleCamera::over_shoulder_preset(),
            BattleCamera::top_down_preset(),
        ];

        for start in starts {
            for (digit, expect_variant) in [('1', "Sideline"), ('2', "OverShoulder"), ('3', "TopDown")] {
                let mut scene = BattleViewer {
                    camera_mode: start,
                    ..Default::default()
                };

                let transition = scene.handle_input(key_event(KeyCode::Char(digit)));

                assert!(
                    transition.is_none(),
                    "handle_input('{digit}') must never request a scene transition"
                );

                // Sideline and OverShoulder both wrap `AnyCamera::Perspective`
                // — a variant `matches!` can't tell them apart — so compare
                // against the exact preset via `PartialEq` instead.
                let matched = match expect_variant {
                    "Sideline" => scene.camera_mode == BattleCamera::sideline_preset(),
                    "OverShoulder" => scene.camera_mode == BattleCamera::over_shoulder_preset(),
                    "TopDown" => matches!(scene.camera_mode.camera, AnyCamera::Orthographic(_)),
                    _ => unreachable!(),
                };
                assert!(
                    matched,
                    "pressing '{digit}' from {:?} must select {expect_variant} directly, got {:?}",
                    start, scene.camera_mode
                );
            }
        }
    }

    /// A genuinely-unmapped key must leave camera_mode untouched and never
    /// trigger a transition. `'4'` is NOT covered here — since b6-t1 it maps
    /// to free-roam entry (see `key_4_from_fixed_presets_enters_free_roam`).
    #[test]
    fn non_digit_key_leaves_camera_mode_unchanged() {
        for code in [KeyCode::Char('x'), KeyCode::Char('5')] {
            let mut scene = BattleViewer {
                camera_mode: BattleCamera::top_down_preset(),
                ..Default::default()
            };

            let transition = scene.handle_input(key_event(code));

            assert!(transition.is_none(), "unmapped key must not request a transition");
            assert!(
                matches!(scene.camera_mode.camera, AnyCamera::Orthographic(_)),
                "unmapped key {:?} must leave camera_mode unchanged, got {:?}",
                code,
                scene.camera_mode
            );
        }
    }

    /// Pinned starting transform (spec 42 Decision 5) — reuses
    /// `SIDELINE_CAMERA_DEPTH`/`BOARD_CENTER_COL`, not new literals.
    #[test]
    fn free_roam_preset_has_pinned_starting_transform() {
        let cam = BattleCamera::free_roam_preset();
        assert_eq!(cam.fit, FitMode::Manual, "free_roam_preset() must set fit: FitMode::Manual");
        let AnyCamera::Perspective(fr) = cam.camera else {
            panic!("free_roam_preset() must wrap AnyCamera::Perspective, got {:?}", cam.camera);
        };
        assert_eq!(fr.x, SIDELINE_CAMERA_DEPTH);
        assert_eq!(fr.y, BOARD_CENTER_COL);
        assert_eq!(fr.height, 2.5);
        assert_eq!(fr.yaw_deg, 90.0);
        assert_eq!(fr.pitch_deg, 10.0);
        assert_eq!(fr.fov_deg, 55.0);
        assert_eq!(fr.scale_dots, 40.0);
    }

    /// Pressing '4' from any fixed preset (Sideline/OverShoulder/TopDown)
    /// switches directly to the pinned `free_roam_preset()` starting
    /// transform — never a no-op, never a partial/relative change.
    #[test]
    fn key_4_from_fixed_presets_enters_free_roam() {
        let starts: [BattleCamera; 3] = [
            BattleCamera::sideline_preset(),
            BattleCamera::over_shoulder_preset(),
            BattleCamera::top_down_preset(),
        ];

        for start in starts {
            let mut scene = BattleViewer {
                camera_mode: start,
                ..Default::default()
            };

            let transition = scene.handle_input(key_event(KeyCode::Char('4')));

            assert!(transition.is_none(), "'4' must never request a scene transition");
            assert_eq!(
                scene.camera_mode,
                BattleCamera::free_roam_preset(),
                "'4' from {:?} must select free_roam_preset() directly",
                start
            );
        }
    }

    /// Re-pressing '4' while already in FreeRoam (`fit: FitMode::Manual`)
    /// must NOT clobber a flown-to position/orientation back to the
    /// starting preset.
    #[test]
    fn key_4_from_nudged_free_roam_is_noop() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::free_roam_preset(),
            ..Default::default()
        };
        let AnyCamera::Perspective(ref mut fr) = scene.camera_mode.camera else {
            panic!("expected AnyCamera::Perspective");
        };
        fr.nudge(1.0, 0.0, 15.0, 0.0, 0.5);
        let nudged = scene.camera_mode;

        let transition = scene.handle_input(key_event(KeyCode::Char('4')));

        assert!(transition.is_none(), "'4' must never request a scene transition");
        assert_eq!(
            scene.camera_mode, nudged,
            "'4' from an already-Manual-fit session must not reset the flown-to transform"
        );
    }

    /// spec 42 Decision 5 movement keys (b6-t2, revised): under
    /// `fit: FitMode::Manual`, each of the 12 keys applies exactly its
    /// pinned delta to the wrapped `PerspectiveCamera` and never requests a
    /// transition. Expected values are computed by applying the same
    /// `nudge`/`scale_dots` op the prod code must use, on a copy of the
    /// starting camera — this proves the key is wired to the exact pinned
    /// args, not just "something changed". Move/height keys are the
    /// lowercase letter (case-insensitivity itself is covered separately by
    /// `free_roam_letter_keys_are_case_insensitive`); yaw/pitch are the
    /// arrow keys, never `q`/`e`/`r`/`f` (see `nudge_camera`'s doc comment
    /// for why `q` in particular can never be bound to anything here).
    #[test]
    fn free_roam_movement_keys_apply_exact_delta() {
        type NudgeFn = fn(&mut PerspectiveCamera);
        let cases: [(KeyCode, NudgeFn); 12] = [
            (KeyCode::Char('w'), |c| c.nudge(0.5, 0.0, 0.0, 0.0, 0.0)),
            (KeyCode::Char('s'), |c| c.nudge(-0.5, 0.0, 0.0, 0.0, 0.0)),
            (KeyCode::Char('d'), |c| c.nudge(0.0, 0.5, 0.0, 0.0, 0.0)),
            (KeyCode::Char('a'), |c| c.nudge(0.0, -0.5, 0.0, 0.0, 0.0)),
            (KeyCode::Right, |c| c.nudge(0.0, 0.0, 5.0, 0.0, 0.0)),
            (KeyCode::Left, |c| c.nudge(0.0, 0.0, -5.0, 0.0, 0.0)),
            (KeyCode::Up, |c| c.nudge(0.0, 0.0, 0.0, 5.0, 0.0)),
            (KeyCode::Down, |c| c.nudge(0.0, 0.0, 0.0, -5.0, 0.0)),
            (KeyCode::Char('x'), |c| c.nudge(0.0, 0.0, 0.0, 0.0, 0.5)),
            (KeyCode::Char('z'), |c| c.nudge(0.0, 0.0, 0.0, 0.0, -0.5)),
            (KeyCode::Char(']'), |c| c.scale_dots *= 1.1),
            (KeyCode::Char('['), |c| c.scale_dots *= 0.9),
        ];

        for (key, apply) in cases {
            let mut scene = BattleViewer {
                camera_mode: BattleCamera::free_roam_preset(),
                ..Default::default()
            };
            let AnyCamera::Perspective(start) = scene.camera_mode.camera else {
                panic!("expected AnyCamera::Perspective");
            };
            let mut expected = start;
            apply(&mut expected);

            let transition = scene.handle_input(key_event(key));

            assert!(transition.is_none(), "{key:?} must never request a scene transition");
            assert_eq!(
                scene.camera_mode,
                BattleCamera { camera: AnyCamera::Perspective(expected), fit: FitMode::Manual },
                "{key:?} from Manual-fit must apply its pinned delta exactly"
            );
        }
    }

    /// Case-insensitivity property (spec 42 Decision 5 revision, explicit
    /// project-owner requirement): each move/height letter key applies the
    /// IDENTICAL delta regardless of case, not merely "some mapping exists"
    /// per case in isolation. Also asserts the key actually changes the
    /// camera (guards against a vacuously-true comparison if both scenes
    /// were no-ops).
    #[test]
    fn free_roam_letter_keys_are_case_insensitive() {
        let pairs: [(char, char); 6] =
            [('w', 'W'), ('s', 'S'), ('d', 'D'), ('a', 'A'), ('x', 'X'), ('z', 'Z')];

        for (lower, upper) in pairs {
            let mut lower_scene = BattleViewer {
                camera_mode: BattleCamera::free_roam_preset(),
                ..Default::default()
            };
            lower_scene.handle_input(key_event(KeyCode::Char(lower)));

            let mut upper_scene = BattleViewer {
                camera_mode: BattleCamera::free_roam_preset(),
                ..Default::default()
            };
            upper_scene.handle_input(key_event(KeyCode::Char(upper)));

            assert_ne!(
                lower_scene.camera_mode,
                BattleCamera::free_roam_preset(),
                "'{lower}' must actually change the camera, not be a no-op"
            );
            assert_eq!(
                lower_scene.camera_mode, upper_scene.camera_mode,
                "'{lower}' and '{upper}' must apply the identical delta"
            );
        }
    }

    /// spec 42 Decision 5 movement keys (b6-t2, revised): under EACH fixed
    /// preset (Sideline/OverShoulder/TopDown), every one of the 12 movement
    /// keys must be a complete no-op — `camera_mode` byte-value unchanged
    /// and no transition requested. Guards against movement handling
    /// leaking into fixed-preset behavior.
    #[test]
    fn free_roam_movement_keys_are_noop_under_fixed_presets() {
        let keys = [
            KeyCode::Char('w'),
            KeyCode::Char('s'),
            KeyCode::Char('d'),
            KeyCode::Char('a'),
            KeyCode::Char('z'),
            KeyCode::Char('x'),
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Char('['),
            KeyCode::Char(']'),
        ];
        let starts: [BattleCamera; 3] = [
            BattleCamera::sideline_preset(),
            BattleCamera::over_shoulder_preset(),
            BattleCamera::top_down_preset(),
        ];

        for start in starts {
            for key in keys {
                let mut scene = BattleViewer {
                    camera_mode: start,
                    ..Default::default()
                };

                let transition = scene.handle_input(key_event(key));

                assert!(transition.is_none(), "{key:?} must never request a scene transition");
                assert_eq!(
                    scene.camera_mode, start,
                    "{key:?} from {:?} (non-Manual-fit) must be a complete no-op",
                    start
                );
            }
        }
    }

    /// BEHAVIORAL (b6-t1 checkpoint): the keypress must actually reach
    /// `BattleViewer::handle_input` through the real `app.rs`/
    /// `SceneManager::route_key` path (b1-t2's seam) for all 3 camera modes,
    /// AND must stay a purely local camera change — never a global scene
    /// transition (no pending transition queued, scene identity unchanged).
    #[test]
    fn route_key_switches_battle_viewer_camera_end_to_end() {
        let mut mgr = SceneManager::with_scene(Box::new(BattleViewer::default()), Box::new(GameCatalog));

        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal.draw(|f| mgr.render(f)).unwrap();
        let buf_sideline = terminal.backend().buffer().clone();

        let mut render_after = |digit: char| {
            let quit = mgr.route_key(KeyEvent::new(KeyCode::Char(digit), KeyModifiers::NONE));
            assert!(!quit, "digit key '{digit}' must not be treated as a quit key");
            assert!(
                mgr.process_pending().is_none(),
                "digit key '{digit}' must never queue a scene transition — camera switching \
                 is local to BattleViewer, not a global scene change"
            );
            assert_eq!(
                mgr.active_id(),
                SceneId::BattleViewer.into(),
                "digit key '{digit}' must leave the active scene as BattleViewer"
            );
            terminal.draw(|f| mgr.render(f)).unwrap();
            terminal.backend().buffer().clone()
        };

        let buf_over_shoulder = render_after('2');
        let buf_top_down = render_after('3');

        assert_ne!(
            buf_sideline, buf_over_shoulder,
            "routing '2' through SceneManager::route_key must reach BattleViewer::handle_input \
             and change the rendered output (Sideline -> OverShoulder)"
        );
        assert_ne!(
            buf_sideline, buf_top_down,
            "routing '3' through SceneManager::route_key must reach BattleViewer::handle_input \
             and change the rendered output (Sideline -> TopDown)"
        );
        assert_ne!(
            buf_over_shoulder, buf_top_down,
            "OverShoulder and TopDown frames must be visibly distinct from each other"
        );

        // Back to Sideline via '1' — confirms direct (non-cycling) re-selection also
        // stays local and reproduces the original default frame.
        let buf_back_to_sideline = render_after('1');
        assert_eq!(
            buf_sideline, buf_back_to_sideline,
            "routing '1' must jump directly back to Sideline, reproducing the default frame"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: BattleViewer sources 8 distinct bundled creatures (b5-t1)
// ─────────────────────────────────────────────────────────────────────────────
