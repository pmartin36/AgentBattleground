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

/// Top-down (plan-view) camera. `scale_dots` = dots per world unit, applied
/// isotropically to both axes (no vertical foreshortening).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TopDownView {
    pub scale_dots: f32,
}

impl TopDownView {
    pub fn new(scale_dots: f32) -> Self {
        TopDownView { scale_dots }
    }
}

impl Camera for TopDownView {
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

/// Horizontal lean applied per world-unit of depth away from the shoulder row.
/// A fixed stylization coefficient (linear, orthographic — NOT perspective).
const OVER_SHOULDER_SHEAR: f32 = 0.5;

/// Over-the-shoulder camera: a stylized oblique (orthographic) view from
/// behind one team's back row. `scale_dots` = dots per world unit;
/// `shoulder_row` = world-`y` of the back row the camera sits behind
/// (board-agnostic — supplied by the caller, spec 16: camera never references
/// the board). No true perspective (Scope).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OverShoulderView {
    pub scale_dots: f32,
    pub shoulder_row: f32,
}

impl OverShoulderView {
    pub fn new(scale_dots: f32, shoulder_row: f32) -> Self {
        OverShoulderView {
            scale_dots,
            shoulder_row,
        }
    }
}

impl Camera for OverShoulderView {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        let depth = pos.y - self.shoulder_row;
        (
            (pos.x * self.scale_dots + depth * self.scale_dots * OVER_SHOULDER_SHEAR).round() as i32,
            (pos.y * self.scale_dots).round() as i32,
        )
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        // Nearer = closer to the shoulder row (the camera's vantage, behind one
        // back row). Proximity — NOT raw y — so pieces closest to `shoulder_row`
        // sort on top no matter which side of the board that row sits on. Linear;
        // no perspective term.
        (-(pos.y - self.shoulder_row).abs() * self.scale_dots).round() as i32
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

    // ── TopDownView ───────────────────────────────────────────────────────────

    /// Integer world position (1.0, 2.0) at scale 4.0 → dot (4, 8).
    #[test]
    fn topdownview_project_integer_pos() {
        let cam = TopDownView::new(4.0);
        assert_eq!(cam.project(WorldPos::new(1.0, 2.0)), (4, 8));
    }

    /// Fractional position: x=1.5 at scale 4.0 → round(6.0)=6; y=0.5 → round(2.0)=2.
    #[test]
    fn topdownview_project_fractional_pos() {
        let cam = TopDownView::new(4.0);
        assert_eq!(cam.project(WorldPos::new(1.5, 0.5)), (6, 2));
    }

    /// Negative world position must project to negative dot coords (off-screen allowed).
    #[test]
    fn topdownview_project_negative_pos() {
        let cam = TopDownView::new(4.0);
        let (dx, dy) = cam.project(WorldPos::new(-1.0, -2.0));
        assert_eq!(dx, -4, "negative x must produce negative dot_x");
        assert_eq!(dy, -8, "negative y must produce negative dot_y");
    }

    /// Plan-view isotropy: an equal world displacement along x and along y must
    /// yield equal-magnitude screen displacement — no vertical foreshortening.
    #[test]
    fn topdownview_project_isotropic_plan_view() {
        let cam = TopDownView::new(4.0);
        let origin = cam.project(WorldPos::new(0.0, 0.0));
        let (x_dx, x_dy) = cam.project(WorldPos::new(1.0, 0.0));
        let (y_dx, y_dy) = cam.project(WorldPos::new(0.0, 1.0));
        let x_delta = (x_dx - origin.0).abs();
        let y_delta = (y_dy - origin.1).abs();
        assert_eq!(
            x_delta, y_delta,
            "equal world displacement along x and y must produce equal-magnitude screen displacement"
        );
        assert_eq!(x_dy - origin.1, 0, "x-only displacement must not affect screen y");
        assert_eq!(y_dx - origin.0, 0, "y-only displacement must not affect screen x");
    }

    /// depth_key must equal project's y-component for the same input (shared-scale coupling).
    #[test]
    fn topdownview_depth_key_equals_projected_y() {
        let cam = TopDownView::new(4.0);
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
    fn topdownview_depth_key_independent_of_x() {
        let cam = TopDownView::new(4.0);
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
    fn topdownview_depth_key_larger_y_is_nearer() {
        let cam = TopDownView::new(4.0);
        let far_pos = WorldPos::new(0.0, 1.0);
        let near_pos = WorldPos::new(0.0, 3.0);
        assert!(
            cam.depth_key(near_pos) > cam.depth_key(far_pos),
            "larger y must produce greater depth_key (nearer)"
        );
    }

    // ── OverShoulderView ──────────────────────────────────────────────────────

    /// At the shoulder row itself, the shear term is zero: project collapses
    /// to the plain orthographic case (x*scale, y*scale).
    #[test]
    fn overshoulderview_project_at_shoulder_row_no_shear() {
        let cam = OverShoulderView::new(4.0, 2.0);
        assert_eq!(cam.project(WorldPos::new(1.0, 2.0)), (4, 8));
    }

    /// Shear is linear in depth from the shoulder row: doubling the depth must
    /// exactly double the horizontal offset from the un-sheared base.
    #[test]
    fn overshoulderview_project_shear_is_linear() {
        let cam = OverShoulderView::new(4.0, 0.0);
        let (x_at_d, _) = cam.project(WorldPos::new(0.0, 1.0));
        let (x_at_2d, _) = cam.project(WorldPos::new(0.0, 2.0));
        let base = 0.0f32 * 4.0; // pos.x * scale_dots, unsheared base at x=0
        let offset_d = x_at_d as f32 - base;
        let offset_2d = x_at_2d as f32 - base;
        assert!(offset_d != 0.0, "shear at depth d must be non-zero");
        assert_eq!(
            offset_2d,
            offset_d * 2.0,
            "shear offset must double when depth doubles (linear, no perspective)"
        );
    }

    /// A point away from the shoulder row must be visibly sheared: its screen_x
    /// differs from the un-sheared SideView-style base (proves not collapsed
    /// onto SideView).
    #[test]
    fn overshoulderview_project_shear_nonzero() {
        let cam = OverShoulderView::new(4.0, 0.0);
        let (dx, _) = cam.project(WorldPos::new(1.0, 3.0));
        let unsheared_base = (1.0f32 * 4.0).round() as i32;
        assert_ne!(
            dx, unsheared_base,
            "point off the shoulder row must be horizontally sheared vs. unsheared base"
        );
    }

    /// Negative world position must still project consistently (linear
    /// extension into negative dots; off-screen allowed).
    #[test]
    fn overshoulderview_project_negative_pos() {
        let cam = OverShoulderView::new(4.0, 0.0);
        let (dx, dy) = cam.project(WorldPos::new(-1.0, -2.0));
        assert_eq!(dy, -8, "negative y must produce negative dot_y");
        // x combines base + shear; at shoulder_row=0, depth=-2, shear=0.5:
        // dx = round(-1*4 + (-2)*4*0.5) = round(-4 + -4) = -8
        assert_eq!(dx, -8, "negative pos must project via the same linear formula");
    }

    /// depth_key must be larger for positions CLOSER to the shoulder row
    /// (proximity to the camera's vantage), regardless of which side of the
    /// board the shoulder row sits on. Uses the game's actual placement
    /// (shoulder_row ABOVE the pieces) — the config the prior masking test
    /// (shoulder_row below all pieces) never exercised.
    #[test]
    fn overshoulderview_depth_key_nearer_shoulder_row_is_nearer() {
        let cam = OverShoulderView::new(4.0, 6.5);
        let near_pos = WorldPos::new(0.0, 5.0); // distance 1.5 from shoulder_row
        let far_pos = WorldPos::new(0.0, 1.0); // distance 5.5 from shoulder_row
        assert!(
            cam.depth_key(near_pos) > cam.depth_key(far_pos),
            "position closer to shoulder_row must yield a strictly greater depth_key"
        );
    }

    /// Pins the exact regression: with the game's real construction
    /// (shoulder_row = 6.5, scale = 36), the team nearer the shoulder row
    /// must composite ON TOP of (greater depth_key than) the far team —
    /// occlusion must not invert.
    #[test]
    fn overshoulderview_depth_key_matches_composite_ordering() {
        let cam = OverShoulderView::new(36.0, 6.5);
        let team_b_near = WorldPos::new(0.0, 5.0);
        let team_a_far = WorldPos::new(0.0, 1.0);
        assert!(
            cam.depth_key(team_b_near) > cam.depth_key(team_a_far),
            "near team must get a greater depth_key than the far team (composites on top)"
        );
    }

    /// Proximity ordering is symmetric regardless of which side of the board
    /// the shoulder row sits on: with shoulder_row BELOW all pieces, smaller
    /// y (nearer the row) still yields the greater depth_key.
    #[test]
    fn overshoulderview_depth_key_side_independent() {
        let cam = OverShoulderView::new(4.0, -10.0);
        let near_pos = WorldPos::new(0.0, 1.0); // closer to shoulder_row (-10)
        let far_pos = WorldPos::new(0.0, 3.0);
        assert!(
            cam.depth_key(near_pos) > cam.depth_key(far_pos),
            "smaller y (nearer the shoulder row) must yield a strictly greater depth_key"
        );
    }

    /// depth_key is independent of x: two positions differing only in x share a depth_key.
    #[test]
    fn overshoulderview_depth_key_independent_of_x() {
        let cam = OverShoulderView::new(4.0, 0.0);
        let p1 = WorldPos::new(0.0, 3.0);
        let p2 = WorldPos::new(10.0, 3.0);
        assert_eq!(
            cam.depth_key(p1),
            cam.depth_key(p2),
            "depth_key must be independent of x"
        );
    }

    /// No perspective divide: equal world-y increments must produce equal
    /// screen-y increments (constant per-unit scale regardless of depth).
    #[test]
    fn overshoulderview_no_perspective_divide() {
        let cam = OverShoulderView::new(4.0, 0.0);
        let (_, y0) = cam.project(WorldPos::new(0.0, 0.0));
        let (_, y1) = cam.project(WorldPos::new(0.0, 1.0));
        let (_, y2) = cam.project(WorldPos::new(0.0, 2.0));
        let step1 = y1 - y0;
        let step2 = y2 - y1;
        assert_eq!(
            step1, step2,
            "equal world-y increments must produce equal screen-y increments (no perspective)"
        );
    }
}
