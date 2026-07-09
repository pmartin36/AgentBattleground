use super::*;

impl RosterManager {
    /// Width/height of the left/right arrow buttons flanking the dot row.
    /// Shrunk ~30% from the original 6×3 so the arrows read as compact chrome
    /// beside the dots rather than dominating the band. `ARROW_H` (2) is <=
    /// `DOT_H` (3), so the button still sits entirely within `dot_row`'s rows,
    /// vertically centered inline with the dot cluster.
    const ARROW_W: u16 = 4;
    const ARROW_H: u16 = 2;

    /// Width/height of the top-right home button.
    const HOME_W: u16 = 6;
    /// Horizontal blank-cell gap between each flanking arrow button and the
    /// dot-cluster group it flanks. The arrows are anchored to the actual
    /// centered dot cluster group (`dot_cluster_rects`), NOT the full-width
    /// `dot_row` band, so they stay visually associated with the dots at any
    /// screen width instead of hugging the screen edges. Widened from a
    /// 1-cell hairline to a genuine 3-cell margin so the arrows have real
    /// horizontal breathing room on either side of the dots.
    const ARROW_DOT_GAP: u16 = 3;

    /// Left/right arrow button rects flanking the centered dot-cluster group
    /// within `layout(area).dot_row`, computed in dot space by
    /// `arrow_dot_rects` (sole place button positioning is computed) — this
    /// is a thin `to_cell_rect()` wrapper over it. `render()` calls
    /// `arrow_dot_rects` directly (it also needs the sub-cell remainder for
    /// the draw offset, so it reads the `DotRect`s themselves rather than
    /// this cell-rounded view); this wrapper exists so tests can assert the
    /// cell-space hit rect without reaching into dot space. The arrows
    /// anchor to the ACTUAL dot-cluster group rect (`dot_cluster_rects`,
    /// which centers the 6 role-grouped dots as a narrower group within the
    /// band) — NOT the full-width band — so they genuinely flank the dots at
    /// any screen width instead of sitting at the screen edges while the
    /// dots float in the middle. Each arrow sits `ARROW_DOT_GAP` cells
    /// outside the group, clamped inside the band so it never underflows or
    /// runs off-screen. `#[cfg(test)]`: no non-test caller needs the
    /// cell-rounded view standalone (`render()` needs the `DotRect`s too, so
    /// it calls `arrow_dot_rects` directly instead).
    #[cfg(test)]
    pub(super) fn arrow_rects(area: Rect) -> (Rect, Rect) {
        let (l, r) = Self::arrow_dot_rects(area);
        (l.to_cell_rect(), r.to_cell_rect())
    }

    /// Dot-space arrow geometry backing `arrow_rects` (cell hit-rect) and the
    /// render site (sub-cell draw offset via `DotRect::cell_remainder`) — the
    /// single place both read from, so the offset is never a hand-maintained
    /// constant bolted on top. Vertical: the `ARROW_H`-tall arrow is
    /// dot-centered in the `DOT_H`-tall `dot_row` band via a single-child
    /// Column `flex()` with `Justify::Center` (the same 2-dot sub-cell
    /// remainder a hand-maintained constant used to bolt on now falls out
    /// of this by construction). Horizontal: the group is flanked
    /// in dot units with `ARROW_DOT_GAP` clearance, clamped inside the band
    /// by `EDGE_MARGIN` — the same formula as `dot_cluster_group_bounds`'s
    /// callers used in cell space, just doubled into dots so the whole rect
    /// rounds to cells exactly once.
    pub(super) fn arrow_dot_rects(area: Rect) -> (engine_render::DotRect, engine_render::DotRect) {
        let band = Self::layout(area).dot_row;
        let (group_left, group_right) = Self::dot_cluster_group_bounds(area);
        let band_dots = Self::cell_rect_to_dots(band);
        let arrow_w_dots = Self::ARROW_W as i32 * 2;
        let arrow_h_dots = Self::ARROW_H as i32 * 4;

        let y = engine_render::flex(
            band_dots,
            engine_render::FlexStyle {
                direction: engine_render::Direction::Column,
                justify_content: engine_render::Justify::Center,
                align_items: engine_render::Align::Start,
                gap: 0,
            },
            &[engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(arrow_h_dots),
                grow: 0.0,
                shrink: 0.0,
            }],
        )[0]
        .y;

        // Flank the group with `ARROW_DOT_GAP` of clearance, but never closer
        // to the screen edge than `EDGE_MARGIN` — at narrow widths the widened
        // label group nearly fills the band, so the flanking arrows settle at
        // that inset rather than overrunning the edge.
        let gap_dots = Self::ARROW_DOT_GAP as i32 * 2;
        let edge_dots = Self::EDGE_MARGIN as i32 * 2;
        let band_left_dots = band_dots.x;
        let band_right_dots = band_dots.x + band_dots.w;
        let left_x = (group_left as i32 * 2 - gap_dots - arrow_w_dots)
            .max(band_left_dots + edge_dots);
        let right_x = (group_right as i32 * 2 + gap_dots)
            .min(band_right_dots - edge_dots - arrow_w_dots);

        (
            engine_render::DotRect { x: left_x, y, w: arrow_w_dots, h: arrow_h_dots },
            engine_render::DotRect { x: right_x, y, w: arrow_w_dots, h: arrow_h_dots },
        )
    }

    /// Dot-space top-right geometry for the home button — sole place its
    /// position is computed; `render()` calls this directly, and `home_rect`
    /// (test-only cell-space view) wraps it. The former sub-cell upward
    /// render nudge (1 dot up) is folded into the container's top inset
    /// (`edge*4 - 1`) so it falls out of `cell_remainder` by construction
    /// rather than being a separate render-only offset constant — the same
    /// tradeoff `arrow_dot_rects` already makes for the arrow buttons.
    pub(super) fn home_dot_rect(area: Rect) -> engine_render::DotRect {
        let area_dots = Self::cell_rect_to_dots(area);
        let home_w = Self::HOME_W as i32 * 2;
        let home_h = Self::HOME_H as i32 * 4;
        let edge = Self::EDGE_MARGIN as i32;
        let inner = area_dots.inset(0, edge * 2, edge * 4 - 1, 0);
        let x = engine_render::flex(
            inner,
            engine_render::FlexStyle {
                direction: engine_render::Direction::Row,
                justify_content: engine_render::Justify::End,
                align_items: engine_render::Align::Start,
                gap: 0,
            },
            &[engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(home_w),
                grow: 0.0,
                shrink: 0.0,
            }],
        )[0]
        .x;
        engine_render::DotRect { x, y: inner.y, w: home_w, h: home_h }
    }

    /// Cell-space view of `home_dot_rect`, for tests only. `render()` needs
    /// the `DotRect` too, so it calls `home_dot_rect` directly instead.
    #[cfg(test)]
    pub(super) fn home_rect(area: Rect) -> Rect {
        Self::home_dot_rect(area).to_cell_rect()
    }

}

#[cfg(test)]
mod arrow_button_tests {
    use super::*;
    use crate::scenes::test_util::{has_non_space, mouse_event, render_to_buffer};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    /// `render()` paints the left arrow button within its own rect,
    /// immediately left of the centered dot-cluster group (spec 38 arrow
    /// flanking correction — the arrows flank the actual dots, not the
    /// screen's left edge). Its row range sits within `dot_row`'s row range
    /// and entirely outside `sprite`'s row range.
    #[test]
    fn left_button_flanks_dot_row() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (left_rect, _) = RosterManager::arrow_rects(area);
        let layout = RosterManager::layout(area);
        let (group_left, _) = RosterManager::dot_cluster_group_bounds(area);

        assert!(
            has_non_space(&buf, left_rect),
            "left arrow button must paint at least one non-space cell within its rect"
        );
        assert!(
            left_rect.right() <= group_left
                && group_left - left_rect.right() <= RosterManager::ARROW_DOT_GAP,
            "left arrow button (right={}) must sit immediately left of the dot-cluster group (group_left={group_left}), not at the screen edge",
            left_rect.right()
        );
        assert!(
            left_rect.top() >= layout.dot_row.top() && left_rect.bottom() <= layout.dot_row.bottom(),
            "left arrow button row range {:?} must lie within dot_row's row range {:?}",
            (left_rect.top(), left_rect.bottom()),
            (layout.dot_row.top(), layout.dot_row.bottom())
        );
        assert!(
            left_rect.top() >= layout.sprite.bottom() || left_rect.bottom() <= layout.sprite.top(),
            "left arrow button row range {:?} must lie entirely outside sprite's row range {:?}",
            (left_rect.top(), left_rect.bottom()),
            (layout.sprite.top(), layout.sprite.bottom())
        );
    }

    /// `render()` paints the right arrow button, immediately right of the
    /// centered dot-cluster group (spec 38 arrow flanking correction) — not
    /// the sprite, and not hugging the screen's right edge.
    #[test]
    fn right_button_flanks_dot_row() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (_, right_rect) = RosterManager::arrow_rects(area);
        let layout = RosterManager::layout(area);
        let (_, group_right) = RosterManager::dot_cluster_group_bounds(area);

        assert!(
            has_non_space(&buf, right_rect),
            "right arrow button must paint at least one non-space cell within its rect"
        );
        assert!(
            right_rect.left() >= group_right
                && right_rect.left() - group_right <= RosterManager::ARROW_DOT_GAP,
            "right arrow button (left={}) must sit immediately right of the dot-cluster group (group_right={group_right}), not at the screen edge",
            right_rect.left()
        );
        assert!(
            right_rect.top() >= layout.dot_row.top() && right_rect.bottom() <= layout.dot_row.bottom(),
            "right arrow button row range {:?} must lie within dot_row's row range {:?}",
            (right_rect.top(), right_rect.bottom()),
            (layout.dot_row.top(), layout.dot_row.bottom())
        );
        assert!(
            right_rect.top() >= layout.sprite.bottom() || right_rect.bottom() <= layout.sprite.top(),
            "right arrow button row range {:?} must lie entirely outside sprite's row range {:?}",
            (right_rect.top(), right_rect.bottom()),
            (layout.sprite.top(), layout.sprite.bottom())
        );
    }

    /// b1-t2 explicit new-test line item: at both 40-col and 80-col widths,
    /// both arrow buttons vertically overlap `layout(area).dot_row` and lie
    /// entirely outside `layout(area).sprite`'s row range.
    #[test]
    fn arrow_buttons_overlap_dot_row_not_sprite_at_multiple_widths() {
        for w in [40u16, 80u16] {
            let h = 20u16;
            let area = Rect::new(0, 0, w, h);
            let (left_rect, right_rect) = RosterManager::arrow_rects(area);
            let layout = RosterManager::layout(area);

            for (name, rect) in [("left", left_rect), ("right", right_rect)] {
                let overlaps_dot_row =
                    rect.top() < layout.dot_row.bottom() && rect.bottom() > layout.dot_row.top();
                assert!(
                    overlaps_dot_row,
                    "width={w}: {name} button rect {:?} must vertically overlap dot_row {:?}",
                    rect, layout.dot_row
                );
                let outside_sprite =
                    rect.top() >= layout.sprite.bottom() || rect.bottom() <= layout.sprite.top();
                assert!(
                    outside_sprite,
                    "width={w}: {name} button rect {:?} must lie entirely outside sprite's row range {:?}",
                    rect, layout.sprite
                );
            }
        }
    }

    /// REGRESSION (spec 38 arrow-flanking correction): the arrows must be
    /// anchored to the ACTUAL centered dot-cluster group, sitting immediately
    /// outside its bounds — not to the full-width `dot_row` band (the old
    /// bug, which left them hugging the screen edges while the dots floated
    /// in the middle). Asserted at both 40- and 80-col widths.
    #[test]
    fn arrows_immediately_flank_dot_cluster_group() {
        // Max cells an arrow may sit outside the group's own bounds — a
        // genuine "flanking" adjacency, not "somewhere in the band".
        const MAX_FLANK_GAP: u16 = RosterManager::ARROW_DOT_GAP;

        for w in [40u16, 80u16] {
            let area = Rect::new(0, 0, w, 20);
            let (left_rect, right_rect) = RosterManager::arrow_rects(area);
            // The group's true visual extent (dots + the wider role labels) —
            // the same bounds `arrow_rects` flanks. The arrows must never
            // overlap this, or they'd overwrite a label glyph.
            let (group_left, group_right) = RosterManager::dot_cluster_group_bounds(area);

            // Immediately flanking the group, no overlap.
            assert!(
                left_rect.right() <= group_left && group_left - left_rect.right() <= MAX_FLANK_GAP,
                "width={w}: left arrow (right={}) must sit within {MAX_FLANK_GAP} cell(s) left of group_left={group_left}",
                left_rect.right()
            );
            assert!(
                right_rect.left() >= group_right && right_rect.left() - group_right <= MAX_FLANK_GAP,
                "width={w}: right arrow (left={}) must sit within {MAX_FLANK_GAP} cell(s) right of group_right={group_right}",
                right_rect.left()
            );

            // Regression guard: the old bug anchored the arrows to the
            // FULL-WIDTH band, pinning them at the screen edges. At 80-col the
            // group is far narrower than the band, so a correctly group-
            // anchored arrow lands well inboard of that old edge position.
            // (At 40-col the widened label group nearly fills the band, so the
            // flanking arrows legitimately settle at the EDGE_MARGIN inset —
            // there is no inboard room to assert, so that guard is 80-col only.)
            if w >= 80 {
                assert!(
                    left_rect.x > area.x + RosterManager::EDGE_MARGIN,
                    "width={w}: left arrow x={} must be inboard of the old edge-hugging position ({})",
                    left_rect.x,
                    area.x + RosterManager::EDGE_MARGIN
                );
                assert!(
                    right_rect.right() < area.right() - RosterManager::EDGE_MARGIN,
                    "width={w}: right arrow right={} must be inboard of the old edge-hugging position ({})",
                    right_rect.right(),
                    area.right() - RosterManager::EDGE_MARGIN
                );
            }
        }
    }

    /// REGRESSION (spec 38 item 1/5): the arrows are shrunk (compact chrome)
    /// and have GENUINE horizontal breathing room beside the dot group — not
    /// a 1-cell hairline. Sizes are the shrunk values, and at 80-col (where
    /// the band has ample room) each arrow sits a real, multi-cell horizontal
    /// gap from the dot cluster group.
    #[test]
    fn arrows_are_shrunk_with_genuine_horizontal_margin() {
        // The arrows were shrunk ~30% from the original 6x3.
        assert_eq!(RosterManager::ARROW_W, 4, "arrow width must be shrunk to 4");
        assert_eq!(RosterManager::ARROW_H, 2, "arrow height must be shrunk to 2");
        assert_eq!(
            RosterManager::ARROW_DOT_GAP, 3,
            "ARROW_DOT_GAP must be a genuine 3-cell margin, not a 1-cell hairline"
        );

        // A genuine margin, not a hairline.
        const MIN_MARGIN: u16 = 3;
        let area = Rect::new(0, 0, 80, 20);
        let (left_rect, right_rect) = RosterManager::arrow_rects(area);
        let (group_left, group_right) = RosterManager::dot_cluster_group_bounds(area);

        assert_eq!(left_rect.width, RosterManager::ARROW_W);
        assert_eq!(left_rect.height, RosterManager::ARROW_H);

        let left_margin = group_left.saturating_sub(left_rect.right());
        let right_margin = right_rect.left().saturating_sub(group_right);
        assert!(
            left_margin >= MIN_MARGIN,
            "left arrow must have >= {MIN_MARGIN} cells of horizontal clearance from the dot group, got {left_margin}"
        );
        assert!(
            right_margin >= MIN_MARGIN,
            "right arrow must have >= {MIN_MARGIN} cells of horizontal clearance from the dot group, got {right_margin}"
        );
    }

    /// REGRESSION (spec 38 corrections item 2 — "the arrows moved up 1"):
    /// the arrows are floor-centered against the FULL `dot_row` band, biasing
    /// them UP onto the band's top two rows (one row higher than the prior
    /// `div_ceil` down-bias). This lands them level with the (down-nudged) dot
    /// cluster rather than dropping to the label row. At both 40- and 80-col
    /// widths.
    #[test]
    fn arrows_raised_to_top_of_dot_row_band() {
        for w in [40u16, 80u16] {
            let area = Rect::new(0, 0, w, 20);
            let (left_rect, right_rect) = RosterManager::arrow_rects(area);
            let band = RosterManager::layout(area).dot_row;

            // Exact round-half-DOWN (floor) center against the full band
            // height — one row higher than the old div_ceil position.
            let expected_top = band.y + (band.height - RosterManager::ARROW_H) / 2;
            assert_eq!(left_rect.top(), expected_top, "w={w}: left arrow top must be the floor-center of the full band");
            assert_eq!(right_rect.top(), expected_top, "w={w}: right arrow top must be the floor-center of the full band");

            // Raised: at DOT_H=3 / ARROW_H=2 the floor center pins the arrow to
            // the band's TOP row (no longer reaching the band's bottom row).
            assert_eq!(
                left_rect.top(), band.top(),
                "w={w}: left arrow must sit on the band's top row (raised up 1 from the prior down-bias)"
            );
            assert!(
                left_rect.bottom() < band.bottom(),
                "w={w}: raised arrow must no longer reach the band's bottom (label) row"
            );
            assert_eq!(right_rect.top(), band.top(), "w={w}: right arrow raised to the band's top row too");
        }
    }

    /// The flanking arrow buttons render 2 sub-cell dots below their
    /// `arrow_rects` rect, via `Button::set_dot_offset_down` fed a value
    /// derived from dot-native centering (not a hand-maintained constant).
    /// A 2-dot shift is finer than one 4-dot cell, so the drawn button spills
    /// into the cell-row directly below the rect — asserted here as that row
    /// being painted. The hit-test rect is unchanged (see
    /// `arrows_raised_to_top_of_dot_row_band`), so navigation is unaffected.
    #[test]
    fn arrows_rendered_nudged_down_into_cell_below_rect() {
        let scene = RosterManager::new();
        let (w, h) = (80u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);
        let area = Rect::new(0, 0, w, h);
        let (left_rect, right_rect) = RosterManager::arrow_rects(area);

        for (name, rect) in [("left", left_rect), ("right", right_rect)] {
            let below_y = rect.bottom(); // first cell-row below the arrow rect
            assert!(
                below_y < h,
                "{name} arrow's below-rect row ({below_y}) must be within the frame"
            );
            let painted = (rect.left()..rect.right())
                .any(|x| buf.cell((x, below_y)).unwrap().symbol() != " ");
            assert!(
                painted,
                "{name} arrow must render into the cell-row directly below its rect (y={below_y})"
            );
        }
    }

    /// A completed click on the right button drives the SAME `navigate()`
    /// as the right-arrow key (b4-t1): wraps 5 -> 0.
    #[test]
    fn mouse_click_right_button_wraps_like_right_key() {
        let mut scene = RosterManager::new();
        scene.current_index = 5;
        let (w, h) = (40u16, 20u16);
        // Render once so the buttons' rects are set to this frame's `area`
        // (handle_input hit-tests against the PREVIOUS frame's render).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (_, right_rect) = RosterManager::arrow_rects(area);
        let (cx, cy) = (right_rect.x, right_rect.y);

        let t1 = scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        let t2 = scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t3 = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        assert_eq!(scene.current_index, 0, "a completed click on the right button at index 5 must wrap current_index to 0");
        assert!(t1.is_none() && t2.is_none() && t3.is_none(), "arrow buttons must never produce a Transition");
    }

    /// A completed click on the left button drives the SAME `navigate()` as
    /// the left-arrow key (b4-t1): wraps 0 -> 5.
    #[test]
    fn mouse_click_left_button_wraps_like_left_key() {
        let mut scene = RosterManager::new();
        scene.current_index = 0;
        let (w, h) = (40u16, 20u16);
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (left_rect, _) = RosterManager::arrow_rects(area);
        let (cx, cy) = (left_rect.x, left_rect.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        assert_eq!(scene.current_index, 5, "a completed click on the left button at index 0 must wrap current_index to 5");
        assert!(t.is_none(), "arrow buttons must never produce a Transition");
    }

    /// A click sequence that completes outside both button rects leaves
    /// `current_index` unchanged.
    #[test]
    fn click_outside_buttons_is_noop() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let (w, h) = (40u16, 20u16);
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (left_rect, right_rect) = RosterManager::arrow_rects(area);
        // Horizontal midpoint of `area`, at the buttons' own row: between
        // the two edge-hugging buttons, outside both rects.
        let (ox, oy) = (area.width / 2, left_rect.y);
        assert!(!left_rect.contains(ratatui::layout::Position { x: ox, y: oy }));
        assert!(!right_rect.contains(ratatui::layout::Position { x: ox, y: oy }));

        scene.handle_input(mouse_event(MouseEventKind::Moved, ox, oy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), ox, oy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), ox, oy));

        assert_eq!(scene.current_index, 2, "a click completed outside both button rects must not change current_index");
        assert!(t.is_none());
    }
}

#[cfg(test)]
mod home_button_tests {
    use super::*;
    use crate::scenes::test_util::{has_non_space, mouse_event, render_to_buffer};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    /// `render()` paints the home button, and its rect sits top-right of
    /// `area` — inset from the right edge by `RosterManager::EDGE_MARGIN`
    /// cells (no longer flush) — distinct from the arrow buttons'
    /// beside-center position and the dot row's bottom position. The top
    /// edge floors to `area.top()`: the button's sub-cell upward nudge is
    /// now baked into its dot-space position (`home_dot_rect`) rather than
    /// bolted on at render time via a separate offset constant, so the hit
    /// rect floors one cell higher than the plain `EDGE_MARGIN` inset while
    /// the drawn glyph stays byte-identical (same tradeoff `arrow_dot_rects`
    /// already makes for the arrow buttons).
    #[test]
    fn home_button_renders_top_right() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::home_rect(area);

        assert!(
            has_non_space(&buf, rect),
            "home button must paint at least one non-space cell within its rect"
        );
        assert_eq!(
            rect.right(),
            area.right() - RosterManager::EDGE_MARGIN,
            "home button rect must be inset from the right edge of area by EDGE_MARGIN"
        );
        assert_eq!(
            rect.top(),
            area.top(),
            "home button hit rect must floor to area.top(): the former upward \
             render nudge is now absorbed into the dot-space position instead \
             of a separate render-only offset"
        );
    }

    /// A completed click (Moved+Down+Up, all inside the home button's rect)
    /// returns a `Transition` to `MainHub` with no params.
    #[test]
    fn home_click_transitions_to_main_hub() {
        let mut scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        // Render once so the button's rect is set to this frame's `area`
        // (handle_input hit-tests against the PREVIOUS frame's render).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::home_rect(area);
        let (cx, cy) = (rect.x, rect.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        let t = t.expect("a completed click on the home button must return a Transition");
        assert_eq!(t.target, SceneKey::from(SceneId::MainHub), "home button must transition to MainHub");
        assert!(t.params.is_none(), "home button transition must carry no params");
    }

    /// A click that does not complete inside the home button's rect (Down
    /// inside, Up outside) must not transition and must not touch
    /// `current_index`.
    #[test]
    fn home_click_not_completed_returns_none() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let (w, h) = (40u16, 20u16);
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::home_rect(area);
        let (cx, cy) = (rect.x, rect.y);
        // Bottom-left corner: far from the top-right home rect.
        let (ox, oy) = (0u16, h - 1);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), ox, oy));

        assert!(
            t.is_none(),
            "a click that does not complete inside the home button rect must not return a Transition"
        );
        assert_eq!(
            scene.current_index, 2,
            "an incomplete home-button click must not change current_index"
        );
    }
}

