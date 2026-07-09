use super::*;

/// Free-roam pinhole camera: position + yaw + pitch + FOV, no `DepthAxis`
/// (spec 42 Decision 4). `yaw_deg` alone determines facing via `cam_space`;
/// no `facing_sign` field needed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FreeRoamCamera {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub fov_deg: f32,
    pub scale_dots: f32,
}

impl FreeRoamCamera {
    /// Rotates `pos` into camera space by `-yaw_deg`: `right` is the
    /// screen-x-aligned axis, `forward` is the depth axis. Single source of
    /// truth for facing — no `facing_sign` field needed (spec 42 Decision 4).
    fn cam_space(&self, pos: WorldPos) -> (f32, f32) {
        let (dx, dy) = (pos.x - self.x, pos.y - self.y);
        let yaw = self.yaw_deg.to_radians();
        (dx * yaw.cos() - dy * yaw.sin(), dx * yaw.sin() + dy * yaw.cos())
    }

    /// Signed distance from the camera along its forward axis, UNCLAMPED.
    /// Mirrors `PerspectiveCamera::cam_forward_raw`.
    fn cam_forward_raw(&self, pos: WorldPos) -> f32 {
        let (_, forward) = self.cam_space(pos);
        let pitch = self.pitch_deg.to_radians();
        forward * pitch.cos() + self.height * pitch.sin()
    }

    /// Distance-from-camera term the perspective divide uses, clamped to
    /// `NEAR_EPS`. Mirrors `PerspectiveCamera::forward_distance`.
    fn forward_distance(&self, pos: WorldPos) -> f32 {
        self.cam_forward_raw(pos).max(NEAR_EPS)
    }

    /// `tan(fov/2)`, the same divisor `project` uses. Mirrors
    /// `PerspectiveCamera::half_fov_tan`.
    fn half_fov_tan(&self) -> f32 {
        (self.fov_deg.to_radians() / 2.0).tan()
    }

    /// Moves `(x, y)` through the CURRENT (pre-delta) `yaw_deg`, then applies
    /// yaw/pitch/height deltas. `pitch_deg` clamps to `[-89.0, 89.0]`.
    pub fn nudge(&mut self, forward: f32, right: f32, yaw_delta: f32, pitch_delta: f32, height_delta: f32) {
        let yaw = self.yaw_deg.to_radians(); // CURRENT yaw, pre-delta
        self.x += forward * yaw.sin() + right * yaw.cos();
        self.y += forward * yaw.cos() - right * yaw.sin();
        self.yaw_deg += yaw_delta;
        self.pitch_deg = (self.pitch_deg + pitch_delta).clamp(-89.0, 89.0);
        self.height += height_delta;
    }
}

impl Camera for FreeRoamCamera {
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

    fn vertical_anchor_hint(&self) -> VerticalAnchor {
        VerticalAnchor::Bottom
    }

    fn elevation_deg(&self) -> f32 {
        self.pitch_deg
    }

    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32 {
        self.scale_dots / (self.forward_distance(pos) * self.half_fov_tan())
    }
}

#[cfg(test)]
pub(super) fn free_roam_representative_cam() -> FreeRoamCamera {
    FreeRoamCamera {
        x: 2.0,
        y: 3.0,
        height: 1.5,
        yaw_deg: 0.0,
        pitch_deg: 10.0,
        fov_deg: 60.0,
        scale_dots: 40.0,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A point directly ahead of the camera (along its yaw-derived forward
    /// direction) must project to `screen_x == 0` for arbitrary `yaw_deg` —
    /// proves `cam_space` is correctly signed by construction, no
    /// `facing_sign` field needed.
    #[test]
    fn free_roam_forward_axis_projects_screen_x_zero_for_arbitrary_yaw() {
        let base = free_roam_representative_cam();
        for yaw_deg in [0.0_f32, 37.0, 90.0, 200.0, -30.0] {
            let cam = FreeRoamCamera { yaw_deg, ..base };
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
    fn free_roam_depth_key_nearer_is_greater() {
        let cam = free_roam_representative_cam();
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
    fn free_roam_forward_distance_near_eps_safety() {
        let cam = FreeRoamCamera {
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

    /// `nudge` resolves `forward`/`right` through the CURRENT (pre-delta)
    /// `yaw_deg`, then applies `yaw_delta`.
    #[test]
    fn free_roam_nudge_moves_through_current_yaw_and_updates_yaw() {
        let mut cam = FreeRoamCamera {
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
    fn free_roam_nudge_clamps_pitch_deg() {
        let mut cam = FreeRoamCamera {
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

        let mut cam2 = FreeRoamCamera { pitch_deg: -85.0, ..cam };
        cam2.nudge(0.0, 0.0, 0.0, -10.0, 0.0);
        assert_eq!(cam2.pitch_deg, -89.0, "pitch_deg must clamp to -89.0");
    }

    /// `FreeRoamCamera` overrides `vertical_anchor_hint` to `Bottom` and
    /// `elevation_deg` to its own `pitch_deg` field — proven with a value
    /// != 90.0 (the trait default) so the assertion can't pass by accident.
    #[test]
    fn free_roam_vertical_anchor_hint_is_bottom_and_elevation_is_pitch() {
        let cam = free_roam_representative_cam();
        assert_ne!(cam.pitch_deg, 90.0, "test fixture must use a non-default pitch");
        assert_eq!(cam.vertical_anchor_hint(), VerticalAnchor::Bottom);
        assert_eq!(cam.elevation_deg(), cam.pitch_deg);
    }

    /// `local_dots_per_world_unit(pos)` must equal
    /// `scale_dots / (forward_distance(pos) * half_fov_tan())` exactly.
    #[test]
    fn free_roam_local_dots_per_world_unit_matches_formula() {
        let cam = FreeRoamCamera {
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

    // ── AnyCamera (b4-t1, spec 42 Decision 2) ───────────────────────────────

}
