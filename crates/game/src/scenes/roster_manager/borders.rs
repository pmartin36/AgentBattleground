use super::*;

impl RosterManager {
    /// Border thickness in dots per edge — shared by every procedurally drawn
    /// border on this screen (details panel + stat-bar outlines). `pub(super)`
    /// so the sibling `stat_bar` module can pass it into the shared
    /// `stat_bar::StatBarChrome` as this screen's chrome.
    pub(super) const BORDER_THICKNESS: usize = 1;
    /// Corner chamfer in dots — the outermost `CHAMFER` dots at each of the 4
    /// corners are clipped along a 45° diagonal so the corner reads as
    /// rounded rather than a hard square. At `BORDER_THICKNESS == 1` this
    /// clips exactly the single corner dot per corner (visually confirmed as
    /// "rounded enough" against a live prototype; a 2-dot chamfer read as too
    /// heavy). `pub(super)` for the same reason as `BORDER_THICKNESS`.
    pub(super) const CHAMFER: usize = 1;

    /// Draws a chamfered-corner rectangular border filling `rect`'s perimeter
    /// via the dot pipeline — a thin delegation to
    /// `engine_render::draw_dot_border`, which composes `rounded_rect` and
    /// places it at DOT precision via `draw_dots_at`. Interiors are left
    /// `Transparent` (existing buffer content underneath is preserved).
    /// Clips (no-ops) on a zero-size rect. Used by every bordered element on
    /// this screen that needs a uniform thickness (the details panel); the
    /// asymmetric-thickness stat-bar outlines use `draw_dot_cap_box` instead.
    /// `FRAME_PANEL` MUST NOT be used for this.
    pub(super) fn draw_dot_border(buf: &mut Buffer, rect: engine_render::DotRect, color: engine_core::color::Rgba) {
        engine_render::draw_dot_border(buf, rect, Self::BORDER_THICKNESS, Self::CHAMFER, color);
    }
}

/// b1-t4: `draw_dot_border` — the shared procedural thin-border helper
/// (dot pipeline). Standalone rect tests, no `RosterManager` instance
/// needed — mirrors `battle_viewer::draw_board_lines_tests`'s pattern of
/// calling the fn directly against a throwaway `Buffer::empty`.
#[cfg(test)]
mod draw_dot_border_tests {
    use super::*;
    use crate::scenes::test_util::braille_mask;
    use ratatui::buffer::Buffer;

    /// Hand-picked rect used by every case below: origin (2,1), 10x6, inset
    /// into a comfortably larger 20x12 buffer so out-of-rect cells are
    /// distinguishable from in-rect ones.
    fn rect() -> Rect {
        Rect::new(2, 1, 10, 6)
    }

    fn render() -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 12));
        RosterManager::draw_dot_border(&mut buf, RosterManager::cell_rect_to_dots(rect()), RosterManager::BORDER_COLOR);
        buf
    }

    /// Spec assertion (a): every cell along the rect's 4 edges is painted.
    #[test]
    fn every_edge_cell_is_painted() {
        let buf = render();
        let r = rect();
        for x in r.left()..r.right() {
            assert_ne!(
                buf.cell((x, r.top())).unwrap().symbol(),
                " ",
                "top edge cell ({x},{}) must be painted",
                r.top()
            );
            assert_ne!(
                buf.cell((x, r.bottom() - 1)).unwrap().symbol(),
                " ",
                "bottom edge cell ({x},{}) must be painted",
                r.bottom() - 1
            );
        }
        for y in r.top()..r.bottom() {
            assert_ne!(
                buf.cell((r.left(), y)).unwrap().symbol(),
                " ",
                "left edge cell ({},{y}) must be painted",
                r.left()
            );
            assert_ne!(
                buf.cell((r.right() - 1, y)).unwrap().symbol(),
                " ",
                "right edge cell ({},{y}) must be painted",
                r.right() - 1
            );
        }
    }

    /// Spec assertion (b): the border is thin, not a filled blob — the
    /// interior (strictly inside a 1-cell margin) contains at least one
    /// unpainted cell.
    #[test]
    fn interior_is_not_filled() {
        let buf = render();
        let cell = buf.cell((4, 3)).unwrap(); // rect (2,1,10,6): well inside the 1-cell margin
        assert_eq!(
            cell.symbol(),
            " ",
            "interior cell must stay unpainted — a thin border, not a filled blob"
        );
    }

    /// Guard-branch coverage: a zero-size rect is a no-op, not a panic.
    #[test]
    fn zero_size_rect_is_noop() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 12));
        let before = buf.clone();
        RosterManager::draw_dot_border(&mut buf, RosterManager::cell_rect_to_dots(Rect::new(2, 1, 0, 6)), RosterManager::BORDER_COLOR);
        assert_eq!(buf, before, "zero-width rect must leave the buffer unchanged");
    }

    /// spec 38 correction (item 3b): the shared border helper rounds its
    /// corners via a 1-dot chamfer — the single outermost corner dot of each
    /// corner is NOT lit, while the edge dots immediately alongside it ARE.
    /// Proven at the dot level: the top-left CORNER cell's top-left dot (mask
    /// bit 0) must be CLEAR, whereas a mid-top-edge cell's top-left dot (bit
    /// 0, an ordinary lit edge dot) must be SET. This is what makes the corner
    /// read as rounded rather than a hard square — and because it lives in the
    /// one shared `draw_dot_border`, every bordered element on the screen
    /// (stat bars + details panel) inherits it.
    #[test]
    fn corners_are_chamfered_not_square() {
        let buf = render();
        let r = rect(); // (2,1,10,6)

        let corner = braille_mask(&buf, r.left(), r.top())
            .expect("top-left corner cell must still be a painted braille glyph");
        assert_eq!(
            corner & 0x01,
            0,
            "top-left corner cell (mask={corner:#04x}) must have its outermost corner dot (bit 0) CLIPPED — a chamfered, rounded corner"
        );
        // The corner cell is NOT blanked wholesale — the chamfer only removes
        // the single corner dot; the rest of the two edges meeting there stay
        // lit.
        assert_ne!(corner, 0, "corner cell must still be painted (only the corner dot is clipped, not the whole cell)");

        // A cell mid-way along the top edge (3 cells inboard of the corner)
        // keeps its top-left dot lit — proving the clip is corner-local, not a
        // blanket "never light bit 0".
        let mid = braille_mask(&buf, r.left() + 3, r.top())
            .expect("mid-top-edge cell must be a painted braille glyph");
        assert_eq!(
            mid & 0x01,
            0x01,
            "a mid-top-edge cell (mask={mid:#04x}) must keep its top-left dot (bit 0) lit — the chamfer is corner-local only"
        );
    }
}

/// b1-t5: procedural bordered details panel (union of `stamina` +
/// `ability_list`), drawn via `draw_dot_border` (b1-t4) — the fix for the
/// "huge fat blob" regression the spec's Purpose section calls out. The
/// expected border rect is computed independently here (mirroring
/// research.md's `details_panel_rects` geometry: the union of `stamina`
/// and `ability_list`, which are stacked with identical x/width) rather than
/// depending on that private helper directly, so these tests assert the
/// OBSERVABLE render, not an internal implementation fn.
#[cfg(test)]
mod details_panel_border_tests {
    use super::*;
    use crate::scenes::test_util::render_to_buffer;

    /// The details-panel border's OBSERVABLE cell footprint — the union of
    /// `stamina` and `ability_list`, computed at dot precision (matching
    /// `RosterManager::details_panel_rects`) and THEN converted to the cell
    /// span `draw_dot_border` actually paints. Using `.to_cell_rect()` alone
    /// (which floors the origin and size independently) understates the
    /// footprint whenever the union's dot extent isn't itself cell-aligned:
    /// `draw_dot_border` paints every cell touched by any dot in
    /// `[y, y+h)`, i.e. `ceil((y+h)/4) - floor(y/4)` rows, not
    /// `floor(h/4)` — the same `floor(a)+floor(b) != floor(a+b)` gap that
    /// caused this panel's border to render 2 dots off `stat_bar`'s in the
    /// first place.
    fn border_rect(area: Rect) -> Rect {
        let [ex_dots, ab_dots] = RosterManager::right_col_dots(area);
        let union = engine_render::DotRect { h: ex_dots.h + ab_dots.h, ..ex_dots };
        let top_cell = union.y.div_euclid(4);
        let bottom_cell_exclusive = (union.y + union.h + 3).div_euclid(4);
        let cell = union.to_cell_rect();
        Rect::new(
            cell.x,
            top_cell.max(0) as u16,
            cell.width,
            (bottom_cell_exclusive - top_cell).max(0) as u16,
        )
    }

    /// Deliverable (a): the border perimeter of the details-panel rect is
    /// painted (b1-t4's edge assertion, applied to the concrete panel this
    /// task draws it around). Also folds in deliverable (c): the panel's
    /// right edge sits strictly left of `area.right()` — a visible margin,
    /// not flush (holds today via `EDGE_MARGIN`; asserted here so it's
    /// locked to the same rect the border is drawn around).
    #[test]
    fn panel_border_perimeter_painted() {
        let (w, h) = (80u16, 30u16);
        let area = Rect::new(0, 0, w, h);
        let rm = RosterManager::new();
        let buf = render_to_buffer(&rm, w, h);
        let r = border_rect(area);

        assert!(
            r.right() < area.right(),
            "details panel border must sit left of area's right edge (margin, not flush)"
        );

        for x in r.left()..r.right() {
            assert_ne!(
                buf.cell((x, r.top())).unwrap().symbol(),
                " ",
                "top edge cell ({x},{}) of details panel border must be painted",
                r.top()
            );
            assert_ne!(
                buf.cell((x, r.bottom() - 1)).unwrap().symbol(),
                " ",
                "bottom edge cell ({x},{}) of details panel border must be painted",
                r.bottom() - 1
            );
        }
        for y in r.top()..r.bottom() {
            assert_ne!(
                buf.cell((r.left(), y)).unwrap().symbol(),
                " ",
                "left edge cell ({},{y}) of details panel border must be painted",
                r.left()
            );
            assert_ne!(
                buf.cell((r.right() - 1, y)).unwrap().symbol(),
                " ",
                "right edge cell ({},{y}) of details panel border must be painted",
                r.right() - 1
            );
        }
    }

    /// REGRESSION (spec 38 margin correction): the details-panel border must
    /// sit a real, generously-sized blank gap below the home button, not
    /// jammed flush against it (the two share the top-right column). A
    /// 1-row technically-non-zero gap would NOT read as separated; require a
    /// concrete, visible minimum. Asserted at multiple sizes.
    #[test]
    fn details_panel_has_generous_gap_below_home_button() {
        // A gap this size or larger reads as deliberate breathing room, not a
        // hairline. `DETAILS_TOP_GAP` is exactly this value by construction.
        const MIN_GAP: u16 = 2;

        for (w, h) in [(80u16, 30u16), (40u16, 20u16), (60u16, 24u16)] {
            let area = Rect::new(0, 0, w, h);
            let home = RosterManager::home_rect(area);
            let panel = border_rect(area);

            let gap = panel.top().saturating_sub(home.bottom());
            assert!(
                panel.top() >= home.bottom(),
                "w={w},h={h}: details panel border top ({}) must not sit above/inside the home button (bottom={})",
                panel.top(), home.bottom()
            );
            assert!(
                gap >= MIN_GAP,
                "w={w},h={h}: only {gap} blank row(s) between the home button (bottom={}) and the \
                 details panel border (top={}) — need >= {MIN_GAP} for a visible margin, not a jam",
                home.bottom(), panel.top()
            );
        }
    }

    /// Deliverable (b): text never lands on the border's own dot-glyph
    /// cells — every perimeter cell of the details-panel border is a
    /// non-alphanumeric (braille dot) glyph, never ASCII stamina/ability
    /// text. Uses an ability description long enough to fill the full
    /// details-panel width (so `label` left-aligns and truncates it flush
    /// against both the left and right edges of `ability_list`, which today
    /// — pre-inset — are the SAME columns as the border) so this actually
    /// exercises the inset, rather than trivially passing because ordinary
    /// short text never reaches the edge columns.
    #[test]
    fn text_never_lands_on_border() {
        let (w, h) = (80u16, 30u16);
        let area = Rect::new(0, 0, w, h);
        let mut rm = RosterManager::new();
        rm.creatures[0] = crate::creatures::Creature::new("Test").with_abilities(vec![
            crate::ability::Ability::new(
                "A Very Long Ability Description That Fills The Whole Panel Width",
                vec![],
            ),
        ]);
        let buf = render_to_buffer(&rm, w, h);
        let r = border_rect(area);

        let is_alnum = |x: u16, y: u16| {
            buf.cell((x, y))
                .unwrap()
                .symbol()
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric())
        };
        for x in r.left()..r.right() {
            assert!(!is_alnum(x, r.top()), "top edge cell ({x},{}) must not contain text", r.top());
            assert!(
                !is_alnum(x, r.bottom() - 1),
                "bottom edge cell ({x},{}) must not contain text",
                r.bottom() - 1
            );
        }
        for y in r.top()..r.bottom() {
            assert!(!is_alnum(r.left(), y), "left edge cell ({},{y}) must not contain text", r.left());
            assert!(
                !is_alnum(r.right() - 1, y),
                "right edge cell ({},{y}) must not contain text",
                r.right() - 1
            );
        }
    }
}

