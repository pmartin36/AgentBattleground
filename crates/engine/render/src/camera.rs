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

/// Which world axis is "depth" (compressed by elevation, into the screen);
/// the other axis becomes screen-x unchanged (spec 39 Decision 1).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DepthAxis {
    Row,
    Col,
}

/// General oblique-projection camera: one shared formula backing all three
/// `BattleCamera` presets (Top-Down, Over-the-shoulder, Sideline — spec 39
/// Decision 1), replacing `TopDownView`/`OverShoulderView`. `taper_per_world_unit`/
/// `taper_min` are copied in from the scene's live-tunable `BattleViewerTuning`
/// every frame, the same way `scale_dots` is already rebuilt every frame today —
/// not fixed once at construction.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ObliqueCamera {
    pub scale_dots: f32,
    pub depth_axis: DepthAxis,
    /// 0 = level with the ground, 90 = straight down.
    pub elevation_deg: f32,
    /// World-space depth coordinate the camera anchors on (its own "near" position).
    pub camera_depth: f32,
    pub taper_per_world_unit: f32,
    pub taper_min: f32,
}

/// Splits a world position into `(depth, spread)` per `depth_axis`: `Row` puts
/// world-y (team-separation axis) on depth, world-x on spread (screen-x);
/// `Col` is the reverse. Module-level `pub` — reused as-is by b6-t1's
/// `depth_scale_factor`, not duplicated there.
pub fn axis_values(depth_axis: DepthAxis, pos: WorldPos) -> (f32, f32) {
    match depth_axis {
        DepthAxis::Row => (pos.y, pos.x),
        DepthAxis::Col => (pos.x, pos.y),
    }
}

impl ObliqueCamera {
    /// Convergence multiplier applied to the spread axis as a point gets
    /// farther from `camera_depth`; exactly `1.0` at `elevation_deg = 90.0`
    /// for any distance (spec 39 Decision 1).
    pub fn taper_factor(&self, pos: WorldPos) -> f32 {
        let (depth, _) = axis_values(self.depth_axis, pos);
        let dist = (depth - self.camera_depth).abs();
        let k = self.elevation_deg.to_radians().sin();
        (1.0 - (1.0 - k) * dist * self.taper_per_world_unit).max(self.taper_min)
    }
}

impl Camera for ObliqueCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        let (depth, spread) = axis_values(self.depth_axis, pos);
        let k = self.elevation_deg.to_radians().sin();
        let screen_x = (spread * self.scale_dots * self.taper_factor(pos)).round() as i32;
        let screen_y = ((self.camera_depth * (1.0 - k) + depth * k) * self.scale_dots).round() as i32;
        (screen_x, screen_y)
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        let (depth, _) = axis_values(self.depth_axis, pos);
        (-(depth - self.camera_depth).abs() * self.scale_dots).round() as i32
    }
}

/// Small positive floor on the perspective-divide forward term: prevents
/// divide-by-zero and sign-flip when a point is at/behind the camera plane.
/// Divide-safety floor, not a visual-tuning constant (spec 41 Decision 1).
const NEAR_EPS: f32 = 0.01;

/// Real minimal pinhole camera (position + pitch + FOV) projecting a 2D
/// ground-plane `WorldPos`. No yaw — only the two `DepthAxis` assignments
/// already established. Replaces `ObliqueCamera` for Sideline/Over-the-
/// shoulder (b3-t1); Top-Down never uses this (spec 41 Decision 1).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PerspectiveCamera {
    pub depth_axis: DepthAxis,
    pub elevation_deg: f32,
    pub camera_depth: f32,
    pub camera_height: f32,
    pub spread_center: f32,
    pub fov_deg: f32,
    pub scale_dots: f32,
}

impl PerspectiveCamera {
    /// Signed distance from the camera along its forward axis, UNCLAMPED.
    /// Single source of the forward term shared by `project`'s divide,
    /// `depth_key`'s sort key, and `forward_distance` — never re-derived.
    fn cam_forward_raw(&self, pos: WorldPos) -> f32 {
        let (depth, _) = axis_values(self.depth_axis, pos);
        let elev = self.elevation_deg.to_radians();
        let dz = depth - self.camera_depth;
        dz * elev.cos() + self.camera_height * elev.sin()
    }

    /// Distance-from-camera term the perspective divide uses, clamped to
    /// `NEAR_EPS`. b6-t1's depth-scale reuses this (`1 / forward_distance`)
    /// rather than inventing its own falloff.
    pub fn forward_distance(&self, pos: WorldPos) -> f32 {
        self.cam_forward_raw(pos).max(NEAR_EPS)
    }
}

impl Camera for PerspectiveCamera {
    fn project(&self, pos: WorldPos) -> (i32, i32) {
        let (depth, spread) = axis_values(self.depth_axis, pos);
        let elev = self.elevation_deg.to_radians();
        let dz = depth - self.camera_depth;
        let dy = -self.camera_height;
        let cam_vertical = dz * elev.sin() + dy * elev.cos();
        let cam_right = spread - self.spread_center;
        let half_fov_tan = (self.fov_deg.to_radians() / 2.0).tan();
        let denom = self.forward_distance(pos) * half_fov_tan;
        let screen_x = (cam_right / denom * self.scale_dots).round() as i32;
        let screen_y = (cam_vertical / denom * self.scale_dots).round() as i32;
        (screen_x, screen_y)
    }

    fn depth_key(&self, pos: WorldPos) -> i32 {
        (-self.cam_forward_raw(pos) * self.scale_dots).round() as i32
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

    // ── ObliqueCamera: Top-Down preset (Row / 90° / sentinel camera_depth) ─────

    /// At elevation 90°, `taper_factor` must be exactly 1.0 for any distance —
    /// `k=sin(90°)=1.0` exactly in f32, so `(1.0-k)=0.0` cancels the whole term
    /// regardless of `dist` (spec 39 Decision 1, numerically re-verified).
    #[test]
    fn top_down_taper_factor_is_one_for_any_dist() {
        let cam = ObliqueCamera {
            scale_dots: 4.0,
            depth_axis: DepthAxis::Row,
            elevation_deg: 90.0,
            camera_depth: 1000.0,
            taper_per_world_unit: 0.06,
            taper_min: 0.4,
        };
        for y in [0.0f32, 1.0, 500.0, 998.0] {
            assert_eq!(
                cam.taper_factor(WorldPos::new(0.0, y)),
                1.0,
                "taper_factor must be exactly 1.0 at elevation_deg=90.0 (y={y})"
            );
        }
    }

    /// Top-Down reproduces today's flat `TopDownView` projection exactly:
    /// `screen_x = round(x*s)`, `screen_y = round(y*s)`, independent of
    /// `camera_depth` (the sentinel fully cancels at k=1).
    #[test]
    fn top_down_project_reproduces_scaled_xy() {
        let make = |camera_depth: f32| ObliqueCamera {
            scale_dots: 4.0,
            depth_axis: DepthAxis::Row,
            elevation_deg: 90.0,
            camera_depth,
            taper_per_world_unit: 0.06,
            taper_min: 0.4,
        };
        let pos = WorldPos::new(1.5, 2.5);
        assert_eq!(
            make(1000.0).project(pos),
            (6, 10),
            "screen coords must equal (round(x*s), round(y*s))"
        );
        assert_eq!(
            make(5.0).project(pos),
            (6, 10),
            "screen_y must be independent of camera_depth at k=1 (sentinel cancels)"
        );
    }

    /// depth_key must strictly increase with world-y (row) — nearer (larger
    /// row) sorts on top — matching today's `TopDownView` ordering.
    #[test]
    fn top_down_depth_key_orders_by_row() {
        let cam = ObliqueCamera {
            scale_dots: 4.0,
            depth_axis: DepthAxis::Row,
            elevation_deg: 90.0,
            camera_depth: 1000.0,
            taper_per_world_unit: 0.06,
            taper_min: 0.4,
        };
        let far = cam.depth_key(WorldPos::new(0.0, 1.0));
        let near = cam.depth_key(WorldPos::new(0.0, 5.0));
        assert!(
            near > far,
            "larger row (y) must yield a strictly greater depth_key"
        );
    }

    // ── ObliqueCamera: Over-the-shoulder preset (Row / 30° / 6.5) ──────────────

    /// No additive shear term: x=0 must project to screen_x=0 even far from
    /// `camera_depth` — this is what removes the old `OverShoulderView`'s
    /// depth-dependent sideways displacement bug.
    #[test]
    fn over_shoulder_no_additive_shear() {
        let cam = ObliqueCamera {
            scale_dots: 4.0,
            depth_axis: DepthAxis::Row,
            elevation_deg: 30.0,
            camera_depth: 6.5,
            taper_per_world_unit: 0.06,
            taper_min: 0.4,
        };
        let (x, _) = cam.project(WorldPos::new(0.0, 1.0)); // far row, dist=5.5
        assert_eq!(x, 0, "x=0 must project to screen_x=0 regardless of depth");

        // Nonzero x scales purely by scale_dots * taper_factor — no extra term.
        let pos = WorldPos::new(2.0, 1.0);
        let (x, _) = cam.project(pos);
        let expected = (2.0 * cam.scale_dots * cam.taper_factor(pos)).round() as i32;
        assert_eq!(x, expected, "screen_x must equal round(x*s*taper_factor(pos)) exactly");
    }

    /// Two same-row pieces 2 world-units apart in column spread wider in
    /// screen-x on the near row than on the far row (real convergence), and
    /// both spreads are strictly positive. Derived from the formula, not
    /// hardcoded — the spec's "~56 vs ~46" numbers are illustrative only.
    #[test]
    fn over_shoulder_column_spread_converges_near_gt_far() {
        let cam = ObliqueCamera {
            scale_dots: 36.0,
            depth_axis: DepthAxis::Row,
            elevation_deg: 30.0,
            camera_depth: 6.5,
            taper_per_world_unit: 0.06,
            taper_min: 0.4,
        };
        let spread_at = |y: f32| {
            let (x0, _) = cam.project(WorldPos::new(0.0, y));
            let (x1, _) = cam.project(WorldPos::new(2.0, y));
            (x1 - x0).abs()
        };
        let near_spread = spread_at(6.5); // at camera_depth, dist=0
        let far_spread = spread_at(1.0); // dist=5.5

        assert!(far_spread > 0, "far-row spread must be strictly positive");
        assert!(
            near_spread > far_spread,
            "near-row column spread ({near_spread}) must exceed far-row spread ({far_spread})"
        );
    }

    // ── ObliqueCamera: Sideline preset (Col / 10° / 3.5) ────────────────────────

    /// `axis_values(Col, pos)` must put world-x on depth and world-y on
    /// spread — the reverse of `Row`.
    #[test]
    fn sideline_depth_axis_is_col() {
        let pos = WorldPos::new(3.0, 5.0);
        assert_eq!(
            axis_values(DepthAxis::Col, pos),
            (3.0, 5.0),
            "Col depth axis: depth=x, spread=y"
        );
    }

    /// Under Sideline (`Col`), world-y (the team-separation axis) maps to
    /// screen-x, not world-x — fixing "looks down the same axis Over-the-
    /// shoulder does".
    #[test]
    fn sideline_team_axis_maps_to_screen_x() {
        let cam = ObliqueCamera {
            scale_dots: 4.0,
            depth_axis: DepthAxis::Col,
            elevation_deg: 10.0,
            camera_depth: 3.5,
            taper_per_world_unit: 0.06,
            taper_min: 0.4,
        };
        // Same x (depth/column) so taper_factor is identical; only y (team axis) differs.
        let (x_y0, _) = cam.project(WorldPos::new(1.0, 0.0));
        let (x_y2, _) = cam.project(WorldPos::new(1.0, 2.0));
        assert_ne!(
            x_y0, x_y2,
            "world-y (team axis) must map to screen-x under Sideline"
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
            let dz = depth - cam.camera_depth;
            let dy = -cam.camera_height;
            let cam_vertical = dz * elev.sin() + dy * elev.cos();
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
}
