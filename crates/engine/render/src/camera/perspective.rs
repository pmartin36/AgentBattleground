use super::*;

/// Unified pinhole camera (position + yaw + pitch + FOV) projecting a 2D
/// ground-plane `WorldPos` (spec 42 Decision 1). Folds the former
/// `DepthAxis`-branching `PerspectiveCamera` and the free-roam-only
/// `FreeRoamCamera` into one shape/formula — no per-instance axis/facing
/// discriminator. `yaw_deg` alone determines facing via `cam_space`.
///
/// DEVIATION from a literal verbatim fold (documented, forced by the goldens
/// — see feature research b2-t1): `cam_space` negates its `right` component.
/// The old `DepthAxis`-based Sideline/Over-shoulder presets differed from a
/// plain rotation by a screen-x REFLECTION, not a rotation; negating `right`
/// is the one change that reproduces that reflection handedness so both
/// fixed shots render byte-identical to before. Free-roam (dev-tool-only, no
/// golden oracle) is the sole cost: it now renders horizontally mirrored vs
/// a literal-verbatim fold.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PerspectiveCamera {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub fov_deg: f32,
    pub scale_dots: f32,
}

impl PerspectiveCamera {
    /// Rotates `pos` into camera space by `-yaw_deg`: `right` is the
    /// screen-x-aligned axis, `forward` is the depth axis. Single source of
    /// truth for facing — no `facing_sign` field needed (spec 42 Decision 4).
    /// `right` is NEGATED relative to a plain rotation — see this struct's
    /// doc comment for why (Option A, forced by the golden fixtures).
    fn cam_space(&self, pos: WorldPos) -> (f32, f32) {
        let (dx, dy) = (pos.x - self.x, pos.y - self.y);
        let yaw = self.yaw_deg.to_radians();
        (-(dx * yaw.cos() - dy * yaw.sin()), dx * yaw.sin() + dy * yaw.cos())
    }

    /// Signed distance from the camera along its forward axis, UNCLAMPED:
    /// positive means "in front," negative means "behind the camera."
    /// Single source of the forward term shared by `project`'s divide,
    /// `depth_key`'s sort key, and `forward_distance` — never re-derived.
    fn cam_forward_raw(&self, pos: WorldPos) -> f32 {
        let (_, forward) = self.cam_space(pos);
        let pitch = self.pitch_deg.to_radians();
        forward * pitch.cos() + self.height * pitch.sin()
    }

    /// Distance-from-camera term the perspective divide uses, clamped to
    /// `NEAR_EPS`. b6-t1's depth-scale reuses this (`1 / forward_distance`)
    /// rather than inventing its own falloff.
    pub fn forward_distance(&self, pos: WorldPos) -> f32 {
        self.cam_forward_raw(pos).max(NEAR_EPS)
    }

    /// `tan(fov/2)`, the same divisor `project` uses — exposed so callers
    /// needing the camera's actual world-unit-to-dots rate (e.g. sizing a
    /// sprite so it fits a cell) derive it from the identical formula
    /// `project` uses, rather than misreading `scale_dots` alone as if it
    /// were a per-world-unit rate (it is not — `scale_dots` is a raw NDC-to-
    /// dots constant solved by the viewport fit; the actual dots-per-world-
    /// unit rate at any position is `scale_dots / (forward_distance(pos) *
    /// half_fov_tan())`, and it shrinks with distance exactly the way
    /// `project`'s own divide does).
    pub fn half_fov_tan(&self) -> f32 {
        (self.fov_deg.to_radians() / 2.0).tan()
    }

    /// Dots per world unit at the given (already-computed) `forward_distance`
    /// — the same divide `project` performs, without the `right`
    /// numerator. Callers pass `forward_distance(some_reference_pos)` to get
    /// "how many dots is 1 world unit, at that reference depth."
    pub fn dots_per_world_unit(&self, forward_distance: f32) -> f32 {
        self.scale_dots / (forward_distance.max(NEAR_EPS) * self.half_fov_tan())
    }

    /// Moves `(x, y)` through the CURRENT (pre-delta) `yaw_deg`, then applies
    /// yaw/pitch/height deltas. `pitch_deg` clamps to `[-89.0, 89.0]`. Its
    /// `right` param is resolved via `yaw.sin`/`yaw.cos` directly — NOT
    /// through `cam_space` — so the struct-level `right` negation does not
    /// touch nudge.
    pub fn nudge(&mut self, forward: f32, right: f32, yaw_delta: f32, pitch_delta: f32, height_delta: f32) {
        let yaw = self.yaw_deg.to_radians(); // CURRENT yaw, pre-delta
        self.x += forward * yaw.sin() + right * yaw.cos();
        self.y += forward * yaw.cos() - right * yaw.sin();
        self.yaw_deg += yaw_delta;
        self.pitch_deg = (self.pitch_deg + pitch_delta).clamp(-89.0, 89.0);
        self.height += height_delta;
    }
}

impl Camera for PerspectiveCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        let (right, forward) = self.cam_space(pos);
        let pitch = self.pitch_deg.to_radians();
        let cam_vertical = self.height * pitch.cos() - forward * pitch.sin();
        let denom = self.forward_distance(pos) * self.half_fov_tan();
        let screen_x = (right / denom * self.scale_dots).round() as i32;
        let screen_y = (cam_vertical / denom * self.scale_dots).round() as i32;
        (screen_x, screen_y)
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        (-self.cam_forward_raw(pos) * self.scale_dots).round() as i32
    }

    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32 {
        self.dots_per_world_unit(self.forward_distance(pos))
    }

    fn vertical_anchor_hint(&self) -> VerticalAnchor {
        VerticalAnchor::Bottom
    }

    fn elevation_deg(&self) -> f32 {
        self.pitch_deg
    }
}

#[cfg(test)]
/// Representative camera used by several tests below: camera sits at
/// x=3.5 (aim-line/spread-center equivalent), y=-5.0 (strictly outside the
/// 0..7 occupied board range, on the low side), yaw 0° (facing toward
/// increasing y), pitched down 20°. Not a pinned "preset" value — arbitrary
/// well-formed config used only to exercise the formula shape (numerically
/// equivalent to the pre-consolidation `depth_axis=Row, facing_sign=1.0`
/// representative fixture).
pub(super) fn representative_cam() -> PerspectiveCamera {
    PerspectiveCamera {
        x: 3.5,
        y: -5.0,
        height: 3.0,
        yaw_deg: 0.0,
        pitch_deg: 20.0,
        fov_deg: 60.0,
        scale_dots: 40.0,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Root-cause structural fix for spec 39/41: `screen_x` and `screen_y`
    /// must divide by the *identical* `forward_distance(pos)*half_fov_tan`
    /// denominator. Recompute the expected coords independently (own
    /// cam_space/pitch math, not by calling the private methods) and assert
    /// `project` matches exactly — a hybrid formula (e.g. a taper term on
    /// only one axis) would diverge from this for at least one point.
    /// Re-expressed onto x/y/yaw (b2-t1): includes the same `right` negation
    /// `cam_space` applies (Option A) so this stays an honest independent
    /// check, not a tautology.
    #[test]
    fn perspective_project_screen_x_y_share_forward_distance_denom() {
        let cam = representative_cam();
        let half_fov_tan = (cam.fov_deg.to_radians() / 2.0).tan();
        for pos in [
            WorldPos::new(3.0, 1.0),
            WorldPos::new(-2.0, 6.0),
            WorldPos::new(3.5, 0.0),
        ] {
            let (dx, dy) = (pos.x - cam.x, pos.y - cam.y);
            let yaw = cam.yaw_deg.to_radians();
            let right = -(dx * yaw.cos() - dy * yaw.sin());
            let forward = dx * yaw.sin() + dy * yaw.cos();
            let pitch = cam.pitch_deg.to_radians();
            let cam_vertical = cam.height * pitch.cos() - forward * pitch.sin();
            let denom = cam.forward_distance(pos) * half_fov_tan;
            let expected = (
                (right / denom * cam.scale_dots).round() as i32,
                (cam_vertical / denom * cam.scale_dots).round() as i32,
            );
            assert_eq!(
                cam.project(pos),
                expected,
                "project(pos={pos:?}) must divide both axes by forward_distance(pos)*half_fov_tan"
            );
        }
    }

    /// Moving a point nearer the camera along the depth axis must strictly
    /// increase `depth_key` (nearer sorts on top).
    #[test]
    fn perspective_depth_key_nearer_is_greater() {
        let cam = representative_cam();
        let far = cam.depth_key(WorldPos::new(0.0, 6.0));
        let near = cam.depth_key(WorldPos::new(0.0, 0.0));
        assert!(
            near > far,
            "nearer point (row 0) must have a strictly greater depth_key than farther point (row 6): near={near} far={far}"
        );
    }

    /// For a point comfortably on the on-screen side of the camera,
    /// `forward_distance` must be a real positive value strictly above the
    /// `NEAR_EPS` floor (not just clamped to it).
    #[test]
    fn perspective_forward_distance_positive_on_correct_side() {
        let cam = representative_cam();
        let d = cam.forward_distance(WorldPos::new(0.0, 0.0));
        assert!(
            d > NEAR_EPS,
            "forward_distance for an on-screen point must exceed the NEAR_EPS floor, got {d}"
        );
    }

    /// A point exactly at `cam_forward_raw == 0` (on the camera plane,
    /// pitch 0 so the height term drops out) and a point behind the camera
    /// (`cam_forward_raw < 0`) must both clamp to exactly `NEAR_EPS`, never
    /// panic/produce NaN/inf, and remain safely invertible.
    #[test]
    fn perspective_forward_distance_near_eps_safety() {
        let cam = PerspectiveCamera {
            x: 0.0,
            y: 5.0,
            height: 3.0,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            fov_deg: 60.0,
            scale_dots: 40.0,
        };
        // dz == 0 → raw cam_forward == 0 exactly.
        let at_zero = cam.forward_distance(WorldPos::new(0.0, 5.0));
        assert_eq!(at_zero, NEAR_EPS, "cam_forward==0 must clamp to exactly NEAR_EPS");
        assert!((1.0 / at_zero).is_finite());

        // dz < 0 → raw cam_forward negative (behind the camera).
        let behind = cam.forward_distance(WorldPos::new(0.0, 0.0));
        assert_eq!(behind, NEAR_EPS, "cam_forward<0 must clamp to exactly NEAR_EPS");
        assert!((1.0 / behind).is_finite());

        // project() must not panic and must return finite dot coordinates.
        let (px, py) = cam.project(WorldPos::new(0.0, 5.0));
        assert!(
            px.abs() < i32::MAX / 2 && py.abs() < i32::MAX / 2,
            "project must return finite, sane dot coords at the NEAR_EPS clamp boundary, got ({px},{py})"
        );
    }

    /// Moving a point nearer the camera (comfortably above the NEAR_EPS
    /// floor, so the clamp is inert) must strictly decrease `forward_distance`.
    #[test]
    fn perspective_forward_distance_monotonic_nearer_is_smaller() {
        let cam = representative_cam();
        let near = cam.forward_distance(WorldPos::new(0.0, 0.0));
        let far = cam.forward_distance(WorldPos::new(0.0, 6.0));
        assert!(
            near < far,
            "forward_distance must strictly decrease as a point moves nearer the camera: near={near} far={far}"
        );
    }

    /// A point directly ahead in the aim-line plane (`dx == 0`, i.e. `pos.x
    /// == cam.x`) must project to `screen_x == 0` — re-expressed from the
    /// deleted `spread_center` field (b2-t1); mirror-invariant, so this holds
    /// independent of the negated-right deviation (Option A).
    #[test]
    fn perspective_aim_line_projects_screen_x_zero() {
        let cam = representative_cam();
        let (x, _) = cam.project(WorldPos::new(cam.x, 0.0));
        assert_eq!(x, 0, "a point on the aim line (pos.x == cam.x) must project to screen_x == 0");
    }

    // ── Camera trait defaults + per-kind overrides (b2-t1, spec 42 Decision 1) ─

    /// `PerspectiveCamera::local_dots_per_world_unit(pos)` must equal
    /// `dots_per_world_unit(forward_distance(pos))` exactly — the same
    /// already-validated pipeline, exposed through the trait.
    #[test]
    fn perspective_local_dots_per_world_unit_matches_forward_distance_pipeline() {
        let cam = representative_cam();
        for pos in [WorldPos::new(0.0, 0.0), WorldPos::new(2.0, 4.0)] {
            let expected = cam.dots_per_world_unit(cam.forward_distance(pos));
            assert_eq!(
                cam.local_dots_per_world_unit(pos),
                expected,
                "local_dots_per_world_unit(pos={pos:?}) must match dots_per_world_unit(forward_distance(pos))"
            );
        }
    }

    /// A nearer position must yield a strictly larger per-world-unit rate
    /// than a farther one (mirrors the shrink-with-distance semantic).
    #[test]
    fn perspective_local_dots_shrinks_with_distance() {
        let cam = representative_cam();
        let near = cam.local_dots_per_world_unit(WorldPos::new(0.0, 0.0));
        let far = cam.local_dots_per_world_unit(WorldPos::new(0.0, 6.0));
        assert!(
            near > far,
            "nearer position must yield a strictly larger local_dots_per_world_unit: near={near} far={far}"
        );
    }

    /// `PerspectiveCamera` overrides `vertical_anchor_hint` to `Bottom`
    /// (ground-relative camera; sprites' feet anchor to the point).
    #[test]
    fn perspective_vertical_anchor_hint_is_bottom() {
        let cam = representative_cam();
        assert_eq!(cam.vertical_anchor_hint(), VerticalAnchor::Bottom);
    }

    /// `PerspectiveCamera` overrides `elevation_deg` to return its own
    /// `pitch_deg` field — proven with a value != 90.0 (the trait default)
    /// so the assertion can't pass by accident.
    #[test]
    fn perspective_elevation_deg_returns_field() {
        let cam = representative_cam();
        assert_ne!(cam.pitch_deg, 90.0, "test fixture must use a non-default pitch");
        assert_eq!(cam.elevation_deg(), cam.pitch_deg);
    }

    // ── Moved from the deleted free_roam.rs (b2-t1, spec 42 Decision 4/1 fold) ──

    /// A point directly ahead of the camera (along its yaw-derived forward
    /// direction) must project to `screen_x == 0` for arbitrary `yaw_deg` —
    /// proves `cam_space` is correctly signed by construction, no
    /// `facing_sign` field needed. Mirror-invariant: holds regardless of the
    /// negated-right deviation (Option A).
    #[test]
    fn perspective_forward_axis_projects_screen_x_zero_for_arbitrary_yaw() {
        let base = representative_cam();
        for yaw_deg in [0.0_f32, 37.0, 90.0, 200.0, -30.0] {
            let cam = PerspectiveCamera { yaw_deg, ..base };
            let yaw = yaw_deg.to_radians();
            let d = 4.0;
            let pos = WorldPos::new(cam.x + d * yaw.sin(), cam.y + d * yaw.cos());
            let (screen_x, _) = cam.project(pos);
            assert_eq!(
                screen_x, 0,
                "point directly ahead at yaw_deg={yaw_deg} must project to screen_x==0, got {screen_x}"
            );
        }
    }

    /// Moving a point nearer the camera along its forward direction must
    /// strictly increase `depth_key` (nearer sorts on top).
    #[test]
    fn perspective_yaw_depth_key_nearer_is_greater() {
        let cam = representative_cam();
        let yaw = cam.yaw_deg.to_radians();
        let near = cam.depth_key(WorldPos::new(cam.x + 1.0 * yaw.sin(), cam.y + 1.0 * yaw.cos()));
        let far = cam.depth_key(WorldPos::new(cam.x + 5.0 * yaw.sin(), cam.y + 5.0 * yaw.cos()));
        assert!(
            near > far,
            "nearer point must have a strictly greater depth_key than a farther point: near={near} far={far}"
        );
    }

    /// `forward_distance` must clamp to exactly `NEAR_EPS` (never
    /// negative/zero/NaN) both at the camera plane and behind the camera —
    /// mirrors `perspective_forward_distance_near_eps_safety`.
    #[test]
    fn perspective_yaw_forward_distance_near_eps_safety() {
        let cam = PerspectiveCamera {
            x: 0.0,
            y: 5.0,
            height: 3.0,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            fov_deg: 60.0,
            scale_dots: 40.0,
        };
        // At the camera plane: raw forward == 0 exactly (pitch=0 drops the height term).
        let at_zero = cam.forward_distance(WorldPos::new(0.0, 5.0));
        assert_eq!(at_zero, NEAR_EPS, "cam_forward==0 must clamp to exactly NEAR_EPS");
        assert!((1.0 / at_zero).is_finite());

        // Behind the camera (negative forward along yaw=0).
        let behind = cam.forward_distance(WorldPos::new(0.0, 0.0));
        assert_eq!(behind, NEAR_EPS, "cam_forward<0 must clamp to exactly NEAR_EPS");
        assert!((1.0 / behind).is_finite());

        // project() must not panic and must return finite, sane dot coords.
        let (px, py) = cam.project(WorldPos::new(0.0, 5.0));
        assert!(
            px.abs() < i32::MAX / 2 && py.abs() < i32::MAX / 2,
            "project must return finite, sane dot coords at the NEAR_EPS clamp boundary, got ({px},{py})"
        );
    }

    /// `PerspectiveCamera` overrides `vertical_anchor_hint` to `Bottom` and
    /// `elevation_deg` to its own `pitch_deg` field — proven with a value
    /// != 90.0 (the trait default) so the assertion can't pass by accident.
    /// Duplicates the property `perspective_vertical_anchor_hint_is_bottom`/
    /// `perspective_elevation_deg_returns_field` already assert (both fixture
    /// families collapsed onto the same type by this task); kept as its own
    /// test since it was moved, not rewritten, from the deleted free_roam.rs.
    #[test]
    fn perspective_yaw_vertical_anchor_hint_is_bottom_and_elevation_is_pitch() {
        let cam = representative_cam();
        assert_ne!(cam.pitch_deg, 90.0, "test fixture must use a non-default pitch");
        assert_eq!(cam.vertical_anchor_hint(), VerticalAnchor::Bottom);
        assert_eq!(cam.elevation_deg(), cam.pitch_deg);
    }

    /// `nudge` resolves `forward`/`right` through the CURRENT (pre-delta)
    /// `yaw_deg`, then applies `yaw_delta`.
    #[test]
    fn perspective_nudge_moves_through_current_yaw_and_updates_yaw() {
        let mut cam = PerspectiveCamera {
            x: 0.0,
            y: 0.0,
            height: 1.0,
            yaw_deg: 90.0,
            pitch_deg: 0.0,
            fov_deg: 60.0,
            scale_dots: 40.0,
        };
        cam.nudge(0.5, 0.0, 5.0, 0.0, 0.0);
        assert!(
            (cam.x - 0.5).abs() < 1e-4,
            "x must move by ~+0.5, resolved through the pre-delta yaw=90 (not the post-delta 95), got {}",
            cam.x
        );
        assert!(
            cam.y.abs() < 1e-4,
            "y must stay ~0, resolved through the pre-delta yaw=90, got {}",
            cam.y
        );
        assert_eq!(cam.yaw_deg, 95.0, "yaw_deg must update by yaw_delta");
    }

    /// `nudge` clamps `pitch_deg` to `[-89.0, 89.0]` after applying `pitch_delta`.
    #[test]
    fn perspective_nudge_clamps_pitch_deg() {
        let mut cam = PerspectiveCamera {
            x: 0.0,
            y: 0.0,
            height: 1.0,
            yaw_deg: 0.0,
            pitch_deg: 85.0,
            fov_deg: 60.0,
            scale_dots: 40.0,
        };
        cam.nudge(0.0, 0.0, 0.0, 10.0, 0.0);
        assert_eq!(cam.pitch_deg, 89.0, "pitch_deg must clamp to 89.0");

        let mut cam2 = PerspectiveCamera { pitch_deg: -85.0, ..cam };
        cam2.nudge(0.0, 0.0, 0.0, -10.0, 0.0);
        assert_eq!(cam2.pitch_deg, -89.0, "pitch_deg must clamp to -89.0");
    }

    /// `local_dots_per_world_unit(pos)` must equal
    /// `scale_dots / (forward_distance(pos) * half_fov_tan())` exactly.
    #[test]
    fn perspective_yaw_local_dots_per_world_unit_matches_formula() {
        let cam = PerspectiveCamera {
            x: 0.0,
            y: 0.0,
            height: 1.0,
            yaw_deg: 30.0,
            pitch_deg: 15.0,
            fov_deg: 60.0,
            scale_dots: 40.0,
        };
        let pos = WorldPos::new(2.0, 4.0);
        let half_fov_tan = (cam.fov_deg.to_radians() / 2.0).tan();
        let expected = cam.scale_dots / (cam.forward_distance(pos) * half_fov_tan);
        assert_eq!(cam.local_dots_per_world_unit(pos), expected);
    }
}
