//! Camera abstraction: maps world positions to screen-dot coordinates + depth.

use engine_core::Inspectable;

use crate::transform::VerticalAnchor;

mod orthographic;
mod perspective;

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
    /// carries a real pitch field.
    fn elevation_deg(&self) -> f32 {
        90.0
    }

    /// Dots per world unit AT `pos` specifically — required, no sensible
    /// universal default (spec 42 Decision 1). Constant cameras
    /// (`OrthographicCamera`) return their fixed `scale_dots`;
    /// perspective cameras shrink this with distance from the camera.
    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32;
}

/// Small positive floor on the perspective-divide forward term: prevents
/// divide-by-zero and sign-flip when a point is at/behind the camera plane.
/// Divide-safety floor, not a visual-tuning constant (spec 41 Decision 1).
const NEAR_EPS: f32 = 0.01;

/// One value type over the engine's projection kinds — the single exhaustive
/// match on "which camera kind" for rendering behavior (spec 42 Decision 2).
/// Exactly 2 variants (spec 42 Decision 4 consolidation, b2-t1): the former
/// dev-only `FreeRoam` kind folded into `Perspective` — same shape, same
/// formula, no separate variant.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AnyCamera {
    Orthographic(OrthographicCamera),
    Perspective(PerspectiveCamera),
}

impl Camera for AnyCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        match self {
            AnyCamera::Orthographic(c) => c.project(pos),
            AnyCamera::Perspective(c) => c.project(pos),
        }
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        match self {
            AnyCamera::Orthographic(c) => c.depth_key(pos),
            AnyCamera::Perspective(c) => c.depth_key(pos),
        }
    }

    fn vertical_anchor_hint(&self) -> VerticalAnchor {
        match self {
            AnyCamera::Orthographic(c) => c.vertical_anchor_hint(),
            AnyCamera::Perspective(c) => c.vertical_anchor_hint(),
        }
    }

    fn elevation_deg(&self) -> f32 {
        match self {
            AnyCamera::Orthographic(c) => c.elevation_deg(),
            AnyCamera::Perspective(c) => c.elevation_deg(),
        }
    }

    fn local_dots_per_world_unit(&self, pos: WorldPos) -> f32 {
        match self {
            AnyCamera::Orthographic(c) => c.local_dots_per_world_unit(pos),
            AnyCamera::Perspective(c) => c.local_dots_per_world_unit(pos),
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::perspective::representative_cam;

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

    /// `with_scale_dots` on an `Orthographic` variant returns the same
    /// variant with only `scale_dots` replaced.
    #[test]
    fn any_camera_with_scale_dots_orthographic_replaces_only_scale() {
        let any = AnyCamera::Orthographic(OrthographicCamera { scale_dots: 5.0 });
        let updated = any.with_scale_dots(9.0);
        assert_eq!(updated, AnyCamera::Orthographic(OrthographicCamera { scale_dots: 9.0 }));
    }

    /// `with_scale_dots` on a `Perspective` variant preserves every other
    /// field (`x`/`y`/`height`/`yaw_deg`/`pitch_deg`/`fov_deg`), replacing
    /// only `scale_dots`.
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
}
