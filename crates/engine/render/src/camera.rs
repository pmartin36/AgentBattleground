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
#[path = "camera_tests.rs"]
mod camera_tests;
