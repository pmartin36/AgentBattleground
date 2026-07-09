//! Camera abstraction: maps world positions to screen-dot coordinates + depth.

use engine_core::Inspectable;

use crate::transform::VerticalAnchor;

/// Continuous 2D world position. Units are game-defined (spec 16).
#[derive(Clone, Copy, PartialEq, Debug, Inspectable)]
pub struct WorldPos {
    pub x: f32,
    pub y: f32,
}

impl WorldPos {
    pub fn new(x: f32, y: f32) -> Self {
        WorldPos { x, y }
    }
}

/// Maps a world position to a screen-dot coordinate and a depth sort key (spec 16).
///
/// Larger `depth_key` = nearer (drawn on top). `DotPlacement.depth` is sorted
/// ascending (far first), so a greater depth_key causes a sprite to be composited
/// last = on top.
pub trait Camera {
    /// World position → screen **dot** coordinate (2 dots/cell wide, 4 tall).
    /// May be negative / off-screen; the compositor clips.
    fn project(&self, pos: WorldPos) -> (i32, i32);

    /// Back-to-front sort key: larger = nearer (drawn on top).
    fn depth_key(&self, pos: WorldPos) -> i32;

    /// Vertical billboard-anchor hint for this camera kind (spec 42 Decision
    /// 1). Default `Center` — correct for cameras with no meaningful
    /// "ground plane" concept (`SideView`, `OrthographicCamera`); overridden
    /// by ground-relative cameras (`PerspectiveCamera`) that anchor sprites'
    /// feet to the point instead.
    fn vertical_anchor_hint(&self) -> VerticalAnchor {
        VerticalAnchor::Center
    }

    /// This camera's pitch, in degrees, for `grid_line_color`-style
    /// elevation checks (spec 42 Decision 1). Default `90.0` (flat/
    /// straight-down, no elevation) — correct for `SideView`/
    /// `OrthographicCamera`; overridden by `PerspectiveCamera`, which
    /// carries a real elevation field.
    fn elevation_deg(&self) -> f32 {
        90.0
    }

    /// Dots per world unit AT `pos` specifically — required, no sensible
    /// universal default (spec 42 Decision 1). Constant cameras
    /// (`SideView`/`OrthographicCamera`) return their fixed `scale_dots`;
    /// perspective cameras shrink this with distance from the camera.
    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32;
}

/// Side-view camera. `scale_dots` = dots per world unit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SideView {
    pub scale_dots: f32,
}

impl SideView {
    pub fn new(scale_dots: f32) -> Self {
        SideView { scale_dots }
    }
}

impl Camera for SideView {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        (
            (pos.x * self.scale_dots).round() as i32,
            (pos.y * self.scale_dots).round() as i32,
        )
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        (pos.y * self.scale_dots).round() as i32
    }

    fn local_dots_per_world_unit(&self, _pos: WorldPos) -> f32 {
        self.scale_dots
    }
}

/// Which world axis is "depth" (compressed by elevation, into the screen);
/// the other axis becomes screen-x unchanged (spec 39 Decision 1).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DepthAxis {
    Row,
    Col,
}

/// Splits a world position into `(depth, spread)` per `depth_axis`: `Row` puts
/// world-y (team-separation axis) on depth, world-x on spread (screen-x);
/// `Col` is the reverse. Module-level `pub` — reused by `PerspectiveCamera`'s
/// `cam_forward_raw`. b6-t1's `depth_scale_factor` (game crate) derives its
/// scale from `PerspectiveCamera::forward_distance` instead, not this.
pub fn axis_values(depth_axis: DepthAxis, pos: WorldPos) -> (f32, f32) {
    match depth_axis {
        DepthAxis::Row => (pos.y, pos.x),
        DepthAxis::Col => (pos.x, pos.y),
    }
}

/// True orthographic (flat, top-down) projection: `scale_dots` = dots per
/// world unit, applied identically to both axes. No tilt, no taper, no
/// depth-anchor (spec 42 Decision 0).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OrthographicCamera {
    pub scale_dots: f32,
}

impl Camera for OrthographicCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        (
            (pos.x * self.scale_dots).round() as i32,
            (pos.y * self.scale_dots).round() as i32,
        )
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        (pos.y * self.scale_dots).round() as i32
    }

    fn local_dots_per_world_unit(&self, _pos: WorldPos) -> f32 {
        self.scale_dots
    }
}

/// Small positive floor on the perspective-divide forward term: prevents
/// divide-by-zero and sign-flip when a point is at/behind the camera plane.
/// Divide-safety floor, not a visual-tuning constant (spec 41 Decision 1).
const NEAR_EPS: f32 = 0.01;

/// Real minimal pinhole camera (position + pitch + FOV) projecting a 2D
/// ground-plane `WorldPos`. No yaw — only the two `DepthAxis` assignments
/// already established. Replaces the old oblique/taper camera for Sideline/
/// Over-the-shoulder (b3-t1); Top-Down never uses this (spec 41 Decision 1).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PerspectiveCamera {
    pub depth_axis: DepthAxis,
    pub elevation_deg: f32,
    pub camera_depth: f32,
    pub camera_height: f32,
    pub spread_center: f32,
    pub fov_deg: f32,
    pub scale_dots: f32,
    /// `+1.0` if the camera looks toward INCREASING depth (`camera_depth`
    /// sits on the low side of the occupied range), `-1.0` if it looks
    /// toward DECREASING depth (`camera_depth` sits on the high side).
    /// `camera_depth` alone doesn't say which way the camera faces — both
    /// `forward_distance` and `project`'s vertical term need this to tell
    /// "farther in the direction the camera looks" apart from "behind the
    /// camera" (a plain `(depth - camera_depth).abs()` can't: it makes
    /// every point read as "in front," including ones that are genuinely
    /// behind the camera, and it also gets the near/far ordering backwards
    /// whenever `camera_depth` sits on the high side without this sign to
    /// correct it — both are real bugs this field fixes, not hypotheticals).
    pub facing_sign: f32,
}

impl PerspectiveCamera {
    /// Signed distance from the camera along its forward axis, UNCLAMPED:
    /// positive means "in front, in the direction `facing_sign` looks,"
    /// negative means "behind the camera." Single source of the forward
    /// term shared by `project`'s divide, `depth_key`'s sort key, and
    /// `forward_distance` — never re-derived.
    fn cam_forward_raw(&self, pos: WorldPos) -> f32 {
        let (depth, _) = axis_values(self.depth_axis, pos);
        let elev = self.elevation_deg.to_radians();
        let dz_facing = (depth - self.camera_depth) * self.facing_sign;
        dz_facing * elev.cos() + self.camera_height * elev.sin()
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
    /// — the same divide `project` performs, without the `spread`/`cam_right`
    /// numerator. Callers pass `forward_distance(some_reference_pos)` to get
    /// "how many dots is 1 world unit, at that reference depth."
    pub fn dots_per_world_unit(&self, forward_distance: f32) -> f32 {
        self.scale_dots / (forward_distance.max(NEAR_EPS) * self.half_fov_tan())
    }
}

impl Camera for PerspectiveCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        let (depth, spread) = axis_values(self.depth_axis, pos);
        let elev = self.elevation_deg.to_radians();
        let dz_facing = (depth - self.camera_depth) * self.facing_sign;
        // Ground-camera vertical convention: near content (small forward
        // distance) must land at the LARGER screen_y (bottom of frame, like
        // the foreground under a downward-pitched camera); far content
        // (large forward distance, toward the horizon) at the smaller/
        // negative screen_y (top of frame).
        let cam_vertical = self.camera_height * elev.cos() - dz_facing * elev.sin();
        let cam_right = spread - self.spread_center;
        let denom = self.forward_distance(pos) * self.half_fov_tan();
        let screen_x = (cam_right / denom * self.scale_dots).round() as i32;
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
        self.elevation_deg
    }
}

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

/// One value type over the engine's projection kinds — the single exhaustive
/// match on "which camera kind" for rendering behavior (spec 42 Decision 2).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AnyCamera {
    Orthographic(OrthographicCamera),
    Perspective(PerspectiveCamera),
    FreeRoam(FreeRoamCamera),
}

impl Camera for AnyCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        match self {
            AnyCamera::Orthographic(c) => c.project(pos),
            AnyCamera::Perspective(c) => c.project(pos),
            AnyCamera::FreeRoam(c) => c.project(pos),
        }
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        match self {
            AnyCamera::Orthographic(c) => c.depth_key(pos),
            AnyCamera::Perspective(c) => c.depth_key(pos),
            AnyCamera::FreeRoam(c) => c.depth_key(pos),
        }
    }

    fn vertical_anchor_hint(&self) -> VerticalAnchor {
        match self {
            AnyCamera::Orthographic(c) => c.vertical_anchor_hint(),
            AnyCamera::Perspective(c) => c.vertical_anchor_hint(),
            AnyCamera::FreeRoam(c) => c.vertical_anchor_hint(),
        }
    }

    fn elevation_deg(&self) -> f32 {
        match self {
            AnyCamera::Orthographic(c) => c.elevation_deg(),
            AnyCamera::Perspective(c) => c.elevation_deg(),
            AnyCamera::FreeRoam(c) => c.elevation_deg(),
        }
    }

    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32 {
        match self {
            AnyCamera::Orthographic(c) => c.local_dots_per_world_unit(pos),
            AnyCamera::Perspective(c) => c.local_dots_per_world_unit(pos),
            AnyCamera::FreeRoam(c) => c.local_dots_per_world_unit(pos),
        }
    }
}

impl AnyCamera {
    /// Rebuilds the active variant preserving every other field, replacing
    /// only `scale_dots` (spec 42 Decision 2).
    pub fn with_scale_dots(&self, scale_dots: f32) -> Self {
        match self {
            AnyCamera::Orthographic(_) => AnyCamera::Orthographic(OrthographicCamera { scale_dots }),
            AnyCamera::Perspective(c) => AnyCamera::Perspective(PerspectiveCamera { scale_dots, ..*c }),
            AnyCamera::FreeRoam(c) => AnyCamera::FreeRoam(FreeRoamCamera { scale_dots, ..*c }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── project ──────────────────────────────────────────────────────────────

    /// Integer world position (1.0, 2.0) at scale 4.0 → dot (4, 8).
    #[test]
    fn sideview_project_integer_pos() {
        let cam = SideView::new(4.0);
        assert_eq!(cam.project(WorldPos::new(1.0, 2.0)), (4, 8));
    }

    /// Fractional position: x=1.5 at scale 4.0 → round(6.0)=6; y=0.5 → round(2.0)=2.
    #[test]
    fn sideview_project_fractional_pos() {
        let cam = SideView::new(4.0);
        assert_eq!(cam.project(WorldPos::new(1.5, 0.5)), (6, 2));
    }

    /// Negative world position must project to negative dot coords (off-screen allowed).
    #[test]
    fn sideview_project_negative_pos() {
        let cam = SideView::new(4.0);
        let (dx, dy) = cam.project(WorldPos::new(-1.0, -2.0));
        assert_eq!(dx, -4, "negative x must produce negative dot_x");
        assert_eq!(dy, -8, "negative y must produce negative dot_y");
    }

    /// Sub-cell fractional position: x=0.1, y=0.7 at scale 10.0 → round(1.0)=1,
    /// round(7.0)=7. Verifies sub-cell precision lands in distinct dot positions.
    #[test]
    fn sideview_project_sub_cell_fractional() {
        let cam = SideView::new(10.0);
        assert_eq!(cam.project(WorldPos::new(0.1, 0.7)), (1, 7));
    }

    // ── depth_key ─────────────────────────────────────────────────────────────

    /// depth_key must equal project's y-component for the same input.
    #[test]
    fn sideview_depth_key_equals_projected_y() {
        let cam = SideView::new(4.0);
        let pos = WorldPos::new(3.0, 2.5);
        let (_, dy) = cam.project(pos);
        assert_eq!(
            cam.depth_key(pos),
            dy,
            "depth_key must equal project's y-component"
        );
    }

    /// depth_key is independent of x: two positions differing only in x yield the same depth.
    #[test]
    fn sideview_depth_key_independent_of_x() {
        let cam = SideView::new(4.0);
        let p1 = WorldPos::new(0.0, 3.0);
        let p2 = WorldPos::new(10.0, 3.0);
        assert_eq!(
            cam.depth_key(p1),
            cam.depth_key(p2),
            "depth_key must be independent of x"
        );
    }

    /// Larger-y position must yield a strictly greater depth_key (nearer sorts on top).
    #[test]
    fn sideview_depth_key_larger_y_is_nearer() {
        let cam = SideView::new(4.0);
        let far_pos = WorldPos::new(0.0, 1.0);
        let near_pos = WorldPos::new(0.0, 3.0);
        assert!(
            cam.depth_key(near_pos) > cam.depth_key(far_pos),
            "larger y must produce greater depth_key (nearer)"
        );
    }

    // ── OrthographicCamera (b1-t1, spec 42 Decision 0) ──────────────────────

    /// `project` scales both axes by `scale_dots` and rounds — exact formula,
    /// no camera_depth/taper/elevation involved.
    #[test]
    fn orthographic_project_scales_and_rounds() {
        let cam = OrthographicCamera { scale_dots: 4.0 };
        assert_eq!(cam.project(WorldPos::new(1.5, 2.5)), (6, 10));
    }

    /// Sub-cell fractional position at scale 10 lands on distinct dot coords.
    #[test]
    fn orthographic_project_sub_cell_fractional() {
        let cam = OrthographicCamera { scale_dots: 10.0 };
        assert_eq!(cam.project(WorldPos::new(0.1, 0.7)), (1, 7));
    }

    /// `depth_key` equals `round(y*scale_dots)` exactly — confirms the old
    /// `- camera_depth` term is gone, not just "close to" the old value.
    #[test]
    fn orthographic_depth_key_equals_scaled_y() {
        let cam = OrthographicCamera { scale_dots: 4.0 };
        assert_eq!(cam.depth_key(WorldPos::new(0.0, 2.5)), 10);
    }

    /// depth_key is independent of x: two positions differing only in x yield
    /// the same depth.
    #[test]
    fn orthographic_depth_key_independent_of_x() {
        let cam = OrthographicCamera { scale_dots: 4.0 };
        let p1 = WorldPos::new(0.0, 3.0);
        let p2 = WorldPos::new(10.0, 3.0);
        assert_eq!(
            cam.depth_key(p1),
            cam.depth_key(p2),
            "depth_key must be independent of x"
        );
    }

    /// Larger-y position must yield a strictly greater depth_key (nearer
    /// sorts on top) — matches today's Top-Down ordering.
    #[test]
    fn orthographic_depth_key_larger_y_is_nearer() {
        let cam = OrthographicCamera { scale_dots: 4.0 };
        let far = cam.depth_key(WorldPos::new(0.0, 1.0));
        let near = cam.depth_key(WorldPos::new(0.0, 5.0));
        assert!(
            near > far,
            "larger row (y) must yield a strictly greater depth_key"
        );
    }

    /// Negative world position must project to negative dot coords.
    #[test]
    fn orthographic_project_negative_pos() {
        let cam = OrthographicCamera { scale_dots: 4.0 };
        let (dx, dy) = cam.project(WorldPos::new(-1.0, -2.0));
        assert_eq!(dx, -4, "negative x must produce negative dot_x");
        assert_eq!(dy, -8, "negative y must produce negative dot_y");
    }

    /// `axis_values(Col, pos)` must put world-x on depth and world-y on
    /// spread — the reverse of `Row`. Still exercised directly here since
    /// `PerspectiveCamera` (not `OrthographicCamera`) is `axis_values`'s
    /// remaining consumer.
    #[test]
    fn sideline_depth_axis_is_col() {
        let pos = WorldPos::new(3.0, 5.0);
        assert_eq!(
            axis_values(DepthAxis::Col, pos),
            (3.0, 5.0),
            "Col depth axis: depth=x, spread=y"
        );
    }

    // ── PerspectiveCamera (b2-t1) ───────────────────────────────────────────

    /// Representative camera used by several tests below: camera sits at
    /// depth -5.0 (strictly outside the 0..7 occupied board range), pitched
    /// down 20°, aimed at spread_center=3.5. Not a pinned "preset" value —
    /// arbitrary well-formed config used only to exercise the formula shape.
    fn representative_cam() -> PerspectiveCamera {
        PerspectiveCamera {
            depth_axis: DepthAxis::Row,
            elevation_deg: 20.0,
            camera_depth: -5.0,
            camera_height: 3.0,
            spread_center: 3.5,
            fov_deg: 60.0,
            scale_dots: 40.0,
            facing_sign: 1.0,
        }
    }

    /// Root-cause structural fix for spec 39/41: `screen_x` and `screen_y`
    /// must divide by the *identical* `forward_distance(pos)*half_fov_tan`
    /// denominator. Recompute the expected coords independently using only
    /// the public `forward_distance` accessor (not a private helper) and
    /// assert `project` matches exactly — a hybrid formula (e.g. a taper
    /// term on only one axis) would diverge from this for at least one point.
    #[test]
    fn perspective_project_screen_x_y_share_forward_distance_denom() {
        let cam = representative_cam();
        let half_fov_tan = (cam.fov_deg.to_radians() / 2.0).tan();
        for pos in [
            WorldPos::new(3.0, 1.0),
            WorldPos::new(-2.0, 6.0),
            WorldPos::new(3.5, 0.0),
        ] {
            let (depth, spread) = axis_values(cam.depth_axis, pos);
            let elev = cam.elevation_deg.to_radians();
            let dz_abs = (depth - cam.camera_depth).abs();
            let cam_vertical = cam.camera_height * elev.cos() - dz_abs * elev.sin();
            let cam_right = spread - cam.spread_center;
            let denom = cam.forward_distance(pos) * half_fov_tan;
            let expected = (
                (cam_right / denom * cam.scale_dots).round() as i32,
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

    /// For a point comfortably on the on-screen side of `camera_depth`,
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

    /// A point exactly at `cam_forward_raw == 0` (depth == camera_depth,
    /// elevation 0 so the height term drops out) and a point behind the
    /// camera (`cam_forward_raw < 0`) must both clamp to exactly `NEAR_EPS`,
    /// never panic/produce NaN/inf, and remain safely invertible.
    #[test]
    fn perspective_forward_distance_near_eps_safety() {
        let cam = PerspectiveCamera {
            depth_axis: DepthAxis::Row,
            elevation_deg: 0.0,
            camera_depth: 5.0,
            camera_height: 3.0,
            spread_center: 0.0,
            fov_deg: 60.0,
            scale_dots: 40.0,
            facing_sign: 1.0,
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

    /// A point on the aim line (`spread == spread_center`) must project to
    /// `screen_x == 0` — no yaw, no additive shear.
    #[test]
    fn perspective_spread_center_projects_screen_x_zero() {
        let cam = representative_cam();
        let (x, _) = cam.project(WorldPos::new(cam.spread_center, 0.0));
        assert_eq!(x, 0, "point on the aim line must project to screen_x == 0");
    }

    // ── Camera trait defaults + per-kind overrides (b2-t1, spec 42 Decision 1) ─

    /// `SideView::local_dots_per_world_unit` must equal `scale_dots`,
    /// independent of `pos`.
    #[test]
    fn sideview_local_dots_per_world_unit_is_constant_scale_dots() {
        let cam = SideView::new(7.0);
        assert_eq!(cam.local_dots_per_world_unit(WorldPos::new(0.0, 0.0)), 7.0);
        assert_eq!(cam.local_dots_per_world_unit(WorldPos::new(100.0, -50.0)), 7.0);
    }

    /// `SideView` takes the trait defaults: `Center` anchor, `90.0` elevation.
    #[test]
    fn sideview_defaults_center_and_90() {
        let cam = SideView::new(7.0);
        assert_eq!(cam.vertical_anchor_hint(), VerticalAnchor::Center);
        assert_eq!(cam.elevation_deg(), 90.0);
    }

    /// `OrthographicCamera::local_dots_per_world_unit` must equal
    /// `scale_dots`, independent of `pos`.
    #[test]
    fn orthographic_local_dots_per_world_unit_is_constant_scale_dots() {
        let cam = OrthographicCamera { scale_dots: 5.0 };
        assert_eq!(cam.local_dots_per_world_unit(WorldPos::new(0.0, 0.0)), 5.0);
        assert_eq!(cam.local_dots_per_world_unit(WorldPos::new(-3.0, 9.0)), 5.0);
    }

    /// `OrthographicCamera` takes the trait defaults: `Center` anchor,
    /// `90.0` elevation.
    #[test]
    fn orthographic_defaults_center_and_90() {
        let cam = OrthographicCamera { scale_dots: 5.0 };
        assert_eq!(cam.vertical_anchor_hint(), VerticalAnchor::Center);
        assert_eq!(cam.elevation_deg(), 90.0);
    }

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
    /// `elevation_deg` field — proven with a value != 90.0 (the trait
    /// default) so the assertion can't pass by accident.
    #[test]
    fn perspective_elevation_deg_returns_field() {
        let cam = representative_cam();
        assert_ne!(cam.elevation_deg, 90.0, "test fixture must use a non-default elevation");
        assert_eq!(cam.elevation_deg(), cam.elevation_deg);
    }

    // ── FreeRoamCamera (b3-t1, spec 42 Decision 4) ──────────────────────────

    fn free_roam_representative_cam() -> FreeRoamCamera {
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

    /// Every `AnyCamera::Orthographic` trait-method call must equal calling
    /// the same method directly on the wrapped concrete camera — including
    /// the two DEFAULTED methods, confirming the delegate reproduces the
    /// trait defaults `OrthographicCamera` itself takes (Center/90.0), not
    /// some other value.
    #[test]
    fn any_camera_orthographic_delegates_to_wrapped() {
        let concrete = OrthographicCamera { scale_dots: 5.0 };
        let any = AnyCamera::Orthographic(concrete);
        for pos in [WorldPos::new(0.0, 0.0), WorldPos::new(2.0, -3.5)] {
            assert_eq!(any.project(pos), concrete.project(pos));
            assert_eq!(any.depth_key(pos), concrete.depth_key(pos));
            assert_eq!(
                any.local_dots_per_world_unit(pos),
                concrete.local_dots_per_world_unit(pos)
            );
        }
        assert_eq!(any.vertical_anchor_hint(), concrete.vertical_anchor_hint());
        assert_eq!(any.elevation_deg(), concrete.elevation_deg());
    }

    /// Every `AnyCamera::Perspective` trait-method call must equal calling
    /// the same method directly on the wrapped concrete camera — including
    /// the two DEFAULTED methods. `PerspectiveCamera` overrides both to
    /// non-default values, so a delegate that accidentally inherited the
    /// trait default (Center/90.0) instead of forwarding would diverge here.
    #[test]
    fn any_camera_perspective_delegates_to_wrapped() {
        let concrete = representative_cam();
        let any = AnyCamera::Perspective(concrete);
        for pos in [WorldPos::new(3.0, 1.0), WorldPos::new(-2.0, 6.0)] {
            assert_eq!(any.project(pos), concrete.project(pos));
            assert_eq!(any.depth_key(pos), concrete.depth_key(pos));
            assert_eq!(
                any.local_dots_per_world_unit(pos),
                concrete.local_dots_per_world_unit(pos)
            );
        }
        assert_eq!(any.vertical_anchor_hint(), concrete.vertical_anchor_hint());
        assert_eq!(any.vertical_anchor_hint(), VerticalAnchor::Bottom);
        assert_eq!(any.elevation_deg(), concrete.elevation_deg());
        assert_ne!(any.elevation_deg(), 90.0, "must forward the real override, not the trait default");
    }

    /// Every `AnyCamera::FreeRoam` trait-method call must equal calling the
    /// same method directly on the wrapped concrete camera — including the
    /// two DEFAULTED methods (mirrors the Perspective case above).
    #[test]
    fn any_camera_free_roam_delegates_to_wrapped() {
        let concrete = free_roam_representative_cam();
        let any = AnyCamera::FreeRoam(concrete);
        for pos in [WorldPos::new(2.0, 6.0), WorldPos::new(-1.0, 3.0)] {
            assert_eq!(any.project(pos), concrete.project(pos));
            assert_eq!(any.depth_key(pos), concrete.depth_key(pos));
            assert_eq!(
                any.local_dots_per_world_unit(pos),
                concrete.local_dots_per_world_unit(pos)
            );
        }
        assert_eq!(any.vertical_anchor_hint(), concrete.vertical_anchor_hint());
        assert_eq!(any.vertical_anchor_hint(), VerticalAnchor::Bottom);
        assert_eq!(any.elevation_deg(), concrete.elevation_deg());
        assert_ne!(any.elevation_deg(), 90.0, "must forward the real override, not the trait default");
    }

    /// `with_scale_dots` on an `Orthographic` variant returns the same
    /// variant with only `scale_dots` replaced.
    #[test]
    fn any_camera_with_scale_dots_orthographic_replaces_only_scale() {
        let any = AnyCamera::Orthographic(OrthographicCamera { scale_dots: 5.0 });
        let updated = any.with_scale_dots(9.0);
        assert_eq!(updated, AnyCamera::Orthographic(OrthographicCamera { scale_dots: 9.0 }));
    }

    /// `with_scale_dots` on a `Perspective` variant preserves every other
    /// field (`facing_sign`/`depth_axis`/`camera_depth`/`camera_height`/
    /// `spread_center`/`fov_deg`), replacing only `scale_dots`.
    #[test]
    fn any_camera_with_scale_dots_perspective_preserves_other_fields() {
        let cam = representative_cam();
        let any = AnyCamera::Perspective(cam);
        let updated = any.with_scale_dots(99.0);
        assert_eq!(
            updated,
            AnyCamera::Perspective(PerspectiveCamera { scale_dots: 99.0, ..cam })
        );
    }

    /// `with_scale_dots` on a `FreeRoam` variant preserves every other field
    /// (`x`/`y`/`height`/`yaw_deg`/`pitch_deg`/`fov_deg`), replacing only
    /// `scale_dots`.
    #[test]
    fn any_camera_with_scale_dots_free_roam_preserves_other_fields() {
        let cam = free_roam_representative_cam();
        let any = AnyCamera::FreeRoam(cam);
        let updated = any.with_scale_dots(123.0);
        assert_eq!(
            updated,
            AnyCamera::FreeRoam(FreeRoamCamera { scale_dots: 123.0, ..cam })
        );
    }
}
