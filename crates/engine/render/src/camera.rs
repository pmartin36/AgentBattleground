//! Camera abstraction: maps world positions to screen-dot coordinates + depth.
//! Implementation is provided by task b3-t1.

use engine_core::Inspectable;

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
}
