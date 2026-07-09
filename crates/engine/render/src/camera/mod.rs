//! Camera abstraction: maps world positions to screen-dot coordinates + depth.

use engine_core::Inspectable;

use crate::transform::VerticalAnchor;

mod free_roam;
mod orthographic;
mod perspective;

pub use free_roam::FreeRoamCamera;
pub use orthographic::OrthographicCamera;
pub use perspective::PerspectiveCamera;

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
    /// "ground plane" concept (`OrthographicCamera`); overridden
    /// by ground-relative cameras (`PerspectiveCamera`) that anchor sprites'
    /// feet to the point instead.
    fn vertical_anchor_hint(&self) -> VerticalAnchor {
        VerticalAnchor::Center
    }

    /// This camera's pitch, in degrees, for `grid_line_color`-style
    /// elevation checks (spec 42 Decision 1). Default `90.0` (flat/
    /// straight-down, no elevation) — correct for
    /// `OrthographicCamera`; overridden by `PerspectiveCamera`, which
    /// carries a real elevation field.
    fn elevation_deg(&self) -> f32 {
        90.0
    }

    /// Dots per world unit AT `pos` specifically — required, no sensible
    /// universal default (spec 42 Decision 1). Constant cameras
    /// (`OrthographicCamera`) return their fixed `scale_dots`;
    /// perspective cameras shrink this with distance from the camera.
    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32;
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

/// Small positive floor on the perspective-divide forward term: prevents
/// divide-by-zero and sign-flip when a point is at/behind the camera plane.
/// Divide-safety floor, not a visual-tuning constant (spec 41 Decision 1).
const NEAR_EPS: f32 = 0.01;

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
    use super::free_roam::free_roam_representative_cam;
    use super::perspective::representative_cam;

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
