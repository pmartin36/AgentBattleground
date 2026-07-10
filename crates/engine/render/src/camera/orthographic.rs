use super::*;

/// True orthographic (flat, top-down) projection: `scale_dots` = dots per
/// world unit, applied identically to both axes. No tilt, no taper, no
/// depth-anchor (spec 42 Decision 0).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct OrthographicCamera {
    pub scale_dots: f32,
}

impl OrthographicCamera {
    pub fn new(scale_dots: f32) -> Self {
        OrthographicCamera { scale_dots }
    }
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


#[cfg(test)]
mod tests {
    use super::*;

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

}
