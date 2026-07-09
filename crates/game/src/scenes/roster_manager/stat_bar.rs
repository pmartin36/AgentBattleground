use super::*;

impl RosterManager {
    /// v1 fill cap for the stat bars (b2-t3): a stat value >= this cap paints
    /// a full-length bar. Spec 35 explicitly defers the exact cap as an
    /// implementation detail (see research.md); this value keeps every
    /// `demo_roster()` stat (range 8..34) partially-filled with clearly
    /// distinct lengths.
    const STAT_DISPLAY_CAP: u32 = 40;
    /// Lit-dot colour for filled stat-bar segments — distinct chrome from
    /// `NAME_COLOR`/`LEVEL_COLOR` (b2-t3).
    const STAT_BAR_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0x4a, 0xd0, 0x8a);

    /// Thickness (in dots) of the grey cap directly above/below the fill —
    /// see `STAT_BAR_OUTLINE_H`.
    const STAT_BAR_HUG_CAP_DOTS: usize = 2;
    /// Reserved blank cells on the RIGHT of `stat_bar` that the 4 slices never
    /// occupy, so the rightmost bar keeps genuine horizontal clearance from
    /// the details panel's left border. The panel's left edge sits
    /// `EDGE_MARGIN + DETAILS_LEFT_SHIFT` (2) cells inside `stat_bar.right()`
    /// after spec 38 item 4 pulled the panel left, so reserving 4 keeps a real
    /// two-cell-or-wider gap between the last bar and the (now closer) panel at
    /// every tested width.
    const STAT_BAR_DETAILS_MARGIN: u16 = 4;
    /// Reserved blank cells on the LEFT of `stat_bar`, before the first bar
    /// slice, so the leftmost bar has a deliberate left margin off the screen
    /// edge rather than starting flush at `area.x`. Mirrors how
    /// `STAT_BAR_DETAILS_MARGIN` reserves space on the right; the 4 slices
    /// divide the width remaining between the two margins, so adding this
    /// narrows each bar by a few dots rather than widening the group.
    const STAT_BAR_LEFT_MARGIN: u16 = 2;
    /// Gap (in cells) between adjacent stat-bar slices (b1-t6).
    const STAT_BAR_GAP: u16 = 1;
    /// Fill length (in dot-columns, out of `dot_cols`) for `kind`'s bar, for
    /// the CURRENT frame (b2-t3). At rest (no active slide), the current
    /// creature's stat value scaled against `STAT_DISPLAY_CAP`. During an
    /// active slide, eased-lerps from the outgoing (`prev_index`) value to
    /// the incoming (`current_index`) value via `Tween` — keyed off the SAME
    /// `Slide`/`elapsed` window the sprite slide and name cross-fade already
    /// use, no second transition state machine.
    pub(super) fn stat_fill_dots(&self, kind: crate::stats::StatKind, dot_cols: usize) -> usize {
        let to_dots = |v: u32| {
            (v as f32 / Self::STAT_DISPLAY_CAP as f32).clamp(0.0, 1.0) * dot_cols as f32
        };
        let fill = match self.active_slide() {
            None => to_dots(self.creatures[self.current_index].stats().value(kind)),
            Some(s) => {
                let progress = self.elapsed.saturating_sub(s.start);
                let from = to_dots(self.creatures[s.prev_index].stats().value(kind));
                let to = to_dots(self.creatures[self.current_index].stats().value(kind));
                Tween::new(from, to, Self::SLIDE_DUR).at(progress)
            }
        };
        fill.round() as usize
    }

    /// The 4 stat slices across `stat_bar` (`StatKind::ALL` order,
    /// left->right), each as `(outline_rect, fill_interior_rect,
    /// label_rect)` — sole source of stat-bar geometry; `render_stat_bars`
    /// and `stat_bar_tests` both call it (research.md b1-t6 blueprint).
    /// Built on `engine_render::flex()`/`DotRect` (b8-t7) — a Row `flex()`
    /// with `Justify::Start`/`Align::Stretch` over cell-floored `Basis::Fixed`
    /// slices, never hand-rolled x-accumulation — computing a slice width
    /// that FILLS the width remaining between the left/right margins (rather
    /// than centering a fixed-size group). Each slice reserves its bottom
    /// `STAT_LABEL_H` row for the label (immediately below the outline, no
    /// gap row); the rows above become the outline. `fill` is the middle
    /// interior cell of the outline (inset a full cell on every side);
    /// `render_stat_bars` draws the border box around the full outline and
    /// lights THIS `fill` cell, so the top/bottom border and the fill land in
    /// separate braille cells and neither overwrites the other.
    pub(super) fn stat_slice_parts(stat_bar: Rect) -> Vec<(Rect, Rect, Rect)> {
        let n = crate::stats::StatKind::ALL.len();
        // Reserve `STAT_BAR_LEFT_MARGIN` cells before the first slice and
        // `STAT_BAR_DETAILS_MARGIN` cells after the last, so the leftmost bar
        // clears the screen edge and the rightmost bar clears the details
        // panel's left border. The 4 slices divide the width remaining between
        // those two margins, so both margins narrow each bar rather than
        // widening the group past `stat_bar`.
        let container = Self::cell_rect_to_dots(stat_bar).inset(
            Self::STAT_BAR_LEFT_MARGIN as i32 * 2,
            Self::STAT_BAR_DETAILS_MARGIN as i32 * 2,
            0,
            0,
        );
        // Slice width MUST stay floored at CELL granularity before ×2 to
        // dots — computing `(container.w - gap_dots*(n-1))/n` directly in
        // dots rounds differently and would silently change the layout.
        let usable_cells = container.w / 2;
        let slice_w_cells =
            (usable_cells - Self::STAT_BAR_GAP as i32 * (n as i32 - 1)).max(0) / n as i32;
        let children: Vec<engine_render::FlexChild> = (0..n)
            .map(|_| engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(slice_w_cells * 2),
                grow: 0.0,
                shrink: 0.0,
            })
            .collect();
        let slices = engine_render::flex(
            container,
            engine_render::FlexStyle {
                direction: engine_render::Direction::Row,
                justify_content: engine_render::Justify::Start,
                align_items: engine_render::Align::Stretch,
                gap: Self::STAT_BAR_GAP as i32 * 2,
            },
            &children,
        );

        slices
            .into_iter()
            .map(|s| s.to_cell_rect())
            .map(|s| {
                // Fixed, compact outline height at the TOP of the slice (so the
                // bar sits level with the details box top), then the label on
                // the row immediately below it (no gap row). Any slice height
                // beyond that is deliberate bottom breathing room. All
                // saturating/clamped so a too-short slice degrades gracefully.
                let outline_h = Self::STAT_BAR_OUTLINE_H.min(s.height);
                let outline = Rect::new(s.x, s.y, s.width, outline_h);
                let label_h = Self::STAT_LABEL_H.min(s.height.saturating_sub(outline_h));
                // Label sits on the row IMMEDIATELY below the outline — no
                // blank gap row between the bar and its label.
                let label_y = (s.y + outline_h).min(s.bottom().saturating_sub(label_h));
                let label = Rect::new(s.x, label_y, s.width, label_h);
                // `fill` is exactly the outline's MIDDLE cell — inset one full
                // cell on every side — so it never shares a braille cell with
                // the hug caps drawn directly above/below it (see
                // `STAT_BAR_OUTLINE_H`).
                let fill = Rect::new(
                    outline.x.saturating_add(1),
                    outline.y.saturating_add(1),
                    outline.width.saturating_sub(2),
                    outline.height.saturating_sub(2),
                );
                (outline, fill, label)
            })
            .collect()
    }

    /// Display label for `kind`'s slice (b1-t6) — an exhaustive `match`
    /// over `StatKind`, mirroring `Stats::value`'s discipline (single stat
    /// list, no second enumeration to drift out of sync).
    pub(super) fn stat_label(kind: crate::stats::StatKind) -> &'static str {
        match kind {
            crate::stats::StatKind::Strength => "STR",
            crate::stats::StatKind::Dexterity => "DEX",
            crate::stats::StatKind::Intelligence => "INT",
            crate::stats::StatKind::Vitality => "VIT",
        }
    }

    /// Draws 4 side-by-side outlined, labeled stat bars (STR/DEX/INT/VIT,
    /// `StatKind::ALL` order) into `rect` — no `col_offset`, so it never
    /// travels with an in-flight sprite slide (b1-t3: static panel, b2-t3).
    /// Geometry comes solely from `stat_slice_parts` (research.md b1-t6
    /// blueprint). Per slice the border box AND the proportional
    /// `STAT_BAR_COLOR` fill are built into ONE `DotBuffer` spanning the
    /// `outline` rect and drawn with a single `draw_grid` (non-text chrome, so
    /// they render through the dot pipeline — never `engine_render::fill`,
    /// CLAUDE.md constraint 4): a rounded "hug" bracket (via `draw_dot_cap_box`)
    /// wraps just the fill's own cell — `STAT_BAR_HUG_CAP_DOTS`-thick grey caps
    /// directly above/below it, 1-dot left/right sides connecting them, with
    /// the same chamfered corner used everywhere else on this screen — and the
    /// fill lights `fill` — the outline's own middle cell — proportionally to
    /// `stat_fill_dots`. Because `fill` never shares a braille cell with the
    /// caps (see `STAT_BAR_OUTLINE_H`), the border always renders as a
    /// complete, crisp bracket at any fill amount, including zero. A
    /// plain-text `stat_label(kind)` sits on the row immediately beneath.
    ///
    /// `rect` (the stat-bar band) is honored at DOT precision, not floored to
    /// the nearest cell first — the same sub-cell placement technique
    /// `draw_dot_border` uses. The band's whole-cell footprint (`to_cell_rect`)
    /// drives `stat_slice_parts`' cell-granular slice/outline/label geometry,
    /// while the band's sub-cell remainder `(dx, dy)` offsets every slice's
    /// DOT content uniformly inside its buffer before the single per-slice
    /// `draw_grid`, so the whole band's true dot position survives the floor.
    /// Labels are text, so they render at cell granularity (offset by the
    /// same floored origin) — text can't be placed sub-cell. Today `(dx, dy)`
    /// is `(0, 0)` (the band is cell-aligned), so this is byte-identical to a
    /// cell-only draw; the offset only bites once an upstream sub-cell dot
    /// value is introduced, exactly as it did for `draw_dot_border`.
    pub(super) fn render_stat_bars(&self, buf: &mut Buffer, rect: engine_render::DotRect) {
        let cell_rect = rect.to_cell_rect();
        let (dxr, dyr) = rect.cell_remainder();
        let (dx, dy) = (dxr as usize, dyr as usize);
        for ((outline, fill, label), kind) in
            Self::stat_slice_parts(cell_rect).into_iter().zip(crate::stats::StatKind::ALL)
        {
            let dot_cols = outline.width as usize * 2;
            let dot_rows = outline.height as usize * 4;
            if dot_cols > 0 && dot_rows > 0 && fill.width > 0 && fill.height > 0 {
                // Buffer sized to include the sub-cell remainder; all content
                // is drawn OFFSET by `(dx, dy)` within it, so the outline's
                // true position survives the eventual cell floor.
                let mut dots = DotBuffer::new(dot_cols + dx, dot_rows + dy);

                // Fill-cell dot bounds within the outline's dot grid (the
                // middle cell — see `stat_slice_parts`), shifted by `(dx, dy)`.
                let fx0 = dx + (fill.x - outline.x) as usize * 2;
                let fy0 = dy + (fill.y - outline.y) as usize * 4;
                let fill_dot_cols = fill.width as usize * 2;
                let fill_dot_rows = fill.height as usize * 4;

                // Rounded hug bracket: `STAT_BAR_HUG_CAP_DOTS`-thick caps
                // directly above/below the fill's own cell, 1-dot left/right
                // sides spanning that same hugged range, chamfered corners.
                let hug_top = fy0.saturating_sub(Self::STAT_BAR_HUG_CAP_DOTS);
                let hug_bottom = (fy0 + fill_dot_rows + Self::STAT_BAR_HUG_CAP_DOTS)
                    .min(dot_rows + dy)
                    .saturating_sub(1);
                Self::draw_dot_cap_box(
                    &mut dots,
                    dx,
                    hug_top,
                    dx + dot_cols - 1,
                    hug_bottom,
                    Self::STAT_BAR_HUG_CAP_DOTS,
                    Self::BORDER_COLOR,
                );

                let n = self.stat_fill_dots(kind, fill_dot_cols);
                for row in fy0..fy0 + fill_dot_rows {
                    for col in 0..n {
                        dots.set(fx0 + col, row, Dot::Lit(Self::STAT_BAR_COLOR));
                    }
                }

                let grid = dots_to_grid(&dots);
                let draw_area = Rect {
                    x: outline.x,
                    y: outline.y,
                    width: grid.cols() as u16,
                    height: grid.rows() as u16,
                };
                engine_render::draw_grid(buf, draw_area, &grid);
            }

            engine_render::label(buf, label, Self::stat_label(kind), Self::DOT_LABEL_COLOR);
        }
    }

}

/// b1-t6: 4 stat bars (STR/DEX/INT/VIT, `StatKind::ALL` order) rendered as
/// side-by-side outlined+labeled column slices within `stat_bar_rect(area)`
/// (spec 38 "Stat bars layout"). `stat_slice_parts` is the SOLE geometry
/// source both `render_stat_bars` and these tests call — no re-derived
/// per-test slice math (research.md CLEANLINESS). Supersedes the b2-t3
/// stacked-horizontal-bands module (`distinct_stats_paint_distinct_rows`
/// etc. — that row-band geometry no longer exists once slices land).
#[cfg(test)]
mod stat_bar_tests {
    use super::*;
    use crate::creatures::Creature;
    use crate::stats::{StatKind, Stats};
    use crate::scenes::test_util::{
        braille_mask, has_non_space, key_event, region_cells, rect_text, render_to_buffer,
    };
    use crossterm::event::KeyCode;
    use engine_core::scene::EngineCtx;

    /// `Stats` with only `kind` set to `value`, every other stat zero.
    fn only_stat(kind: StatKind, value: u32) -> Stats {
        let mut s = Stats::default();
        match kind {
            StatKind::Strength => s.strength = value,
            StatKind::Dexterity => s.dexterity = value,
            StatKind::Intelligence => s.intelligence = value,
            StatKind::Vitality => s.vitality = value,
        }
        s
    }

    /// The render area every case below uses.
    fn area() -> Rect {
        Rect::new(0, 0, 80, 30)
    }

    /// A fresh `RosterManager` with `creatures[0]`'s stats replaced by
    /// `stats`, rendered at rest (`current_index == 0`) at `area()`.
    fn render_with_stats(stats: Stats) -> ratatui::buffer::Buffer {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test").with_stats(stats);
        render_to_buffer(&rm, area().width, area().height)
    }

    /// REGRESSION (spec 38 item 3a): the rightmost stat bar must keep a real
    /// horizontal gap from the details panel's left border — the bars must
    /// never touch or overlap the panel. Asserted against the panel border's
    /// own left column (`exhaustion.x`, which is where `draw_dot_border`
    /// paints the panel's left edge) at multiple widths.
    #[test]
    fn rightmost_stat_bar_clears_details_panel() {
        // A concrete, visible minimum — not a technically-non-zero hairline.
        const MIN_GAP: u16 = 2;

        for (w, h) in [(80u16, 30u16), (40u16, 20u16), (60u16, 24u16)] {
            let area = Rect::new(0, 0, w, h);
            let stat_bar = RosterManager::stat_bar_rect(area);
            let slices = RosterManager::stat_slice_parts(stat_bar);
            let (last_outline, _fill, _label) = *slices.last().unwrap();
            let panel_left = RosterManager::exhaustion_rect(area).x; // details panel border's left column

            assert!(
                last_outline.right() <= panel_left,
                "w={w},h={h}: rightmost stat bar (right={}) must not reach the details panel border (left={panel_left})",
                last_outline.right()
            );
            let gap = panel_left.saturating_sub(last_outline.right());
            assert!(
                gap >= MIN_GAP,
                "w={w},h={h}: only {gap} blank column(s) between the rightmost stat bar (right={}) and the details panel border (left={panel_left}) — need >= {MIN_GAP}",
                last_outline.right()
            );
        }
    }

    /// Each bar is the compact 2-cell `STAT_BAR_OUTLINE_H`, and the stat_bar
    /// band is exactly `outline + label` tall, reserving no padding cell (see
    /// `stat_bar_band_is_tight_and_sprite_grows`).
    #[test]
    fn stat_bar_fill_is_short_and_band_is_tight() {
        let stat_bar = RosterManager::stat_bar_rect(area());
        let slices = RosterManager::stat_slice_parts(stat_bar);
        for (i, (outline, _fill, label)) in slices.iter().enumerate() {
            assert_eq!(
                outline.height, RosterManager::STAT_BAR_OUTLINE_H,
                "slice {i}: outline must be the fixed compact height, not stretched to fill the band"
            );
            // The band is exactly outline + label — no padding cell.
            let content_bottom = label.y + label.height;
            assert_eq!(
                content_bottom,
                stat_bar.bottom(),
                "slice {i}: outline+label ({content_bottom}) must fill the stat_bar band exactly (bottom={})",
                stat_bar.bottom()
            );
            assert_eq!(
                stat_bar.height,
                outline.height + label.height,
                "slice {i}: stat_bar band ({}) must be exactly outline+label ({}+{})",
                stat_bar.height, outline.height, label.height
            );
        }
    }

    /// The stat_bar band is exactly `STAT_BAR_OUTLINE_H + STAT_LABEL_H`, and the
    /// `sprite` band fills every remaining row between the band's bottom and its
    /// baseline pinned at `dot_row.top()` — so all vertical space not needed by
    /// the tight bars belongs to the sprite.
    #[test]
    fn stat_bar_band_is_tight_and_sprite_grows() {
        assert_eq!(
            RosterManager::STAT_BAR_BAND_H,
            RosterManager::STAT_BAR_OUTLINE_H + RosterManager::STAT_LABEL_H,
            "STAT_BAR_BAND_H must be exactly outline+label, with no padding cell"
        );

        for (w, h) in [(80u16, 30u16), (60u16, 24u16), (40u16, 20u16)] {
            let area = Rect::new(0, 0, w, h);
            let l = RosterManager::layout(area);
            let stat_bar = RosterManager::stat_bar_rect(area);

            // The sprite opens directly below the stat_bar band...
            assert_eq!(
                l.sprite.y,
                stat_bar.y + stat_bar.height,
                "w={w},h={h}: sprite must open directly below the stat_bar band"
            );
            // ...and bottoms out flush against dot_row (baseline pinned).
            assert_eq!(
                l.sprite.y + l.sprite.height,
                l.dot_row.y,
                "w={w},h={h}: sprite baseline must sit at dot_row.top()"
            );

            // Every row between the band bottom and the baseline is sprite; a
            // band carrying one extra padding cell would yield a shorter sprite.
            let sprite_if_band_padded = l
                .dot_row
                .y
                .saturating_sub(stat_bar.y + stat_bar.height + 1);
            assert!(
                l.sprite.height > sprite_if_band_padded,
                "w={w},h={h}: sprite ({}) must claim the row a padding cell would otherwise take ({sprite_if_band_padded})",
                l.sprite.height
            );
        }
    }

    /// DELIVERABLE 1: the 4 slices occupy 4 non-overlapping column ranges,
    /// strictly left-to-right in `StatKind::ALL` order, at both 40- and
    /// 80-wide areas.
    #[test]
    fn slices_are_four_disjoint_ordered_columns() {
        for w in [40u16, 80u16] {
            let stat_bar = RosterManager::stat_bar_rect(Rect::new(0, 0, w, 30));
            let slices = RosterManager::stat_slice_parts(stat_bar);
            assert_eq!(slices.len(), 4, "expected 4 stat slices at width {w}");

            for i in 0..slices.len() - 1 {
                let (a, _, _) = slices[i];
                let (b, _, _) = slices[i + 1];
                assert!(
                    a.right() <= b.left(),
                    "slice {i} (right={}) must not overlap slice {} (left={}) at width {w}",
                    a.right(), i + 1, b.left()
                );
                assert!(
                    a.left() < b.left(),
                    "slices must be strictly left-to-right ordered (StatKind::ALL order) at width {w}"
                );
            }
        }
    }

    /// DELIVERABLE 2: a slice whose stat is 0 still shows its outline — the
    /// border box remains visible on all four sides of the 2-cell bar (top row,
    /// bottom row, and left/right edges).
    #[test]
    fn zero_fill_slice_still_outlined() {
        let buf = render_with_stats(only_stat(StatKind::Strength, 0));
        let stat_bar = RosterManager::stat_bar_rect(area());
        let slices = RosterManager::stat_slice_parts(stat_bar);
        let (outline, _fill, _label) = slices[0]; // Strength == StatKind::ALL[0]

        // Top and bottom edges of the border box, across the bar's width.
        for x in outline.left()..outline.right() {
            assert_ne!(
                buf.cell((x, outline.top())).unwrap().symbol(),
                " ",
                "top edge of the zero-fill slice's border box must still be painted at ({x},{})",
                outline.top()
            );
            assert_ne!(
                buf.cell((x, outline.bottom() - 1)).unwrap().symbol(),
                " ",
                "bottom edge of the zero-fill slice's border box must still be painted at ({x},{})",
                outline.bottom() - 1
            );
        }
        // Left and right edges of the border box, down the bar's height.
        for y in outline.top()..outline.bottom() {
            assert_ne!(
                buf.cell((outline.left(), y)).unwrap().symbol(),
                " ",
                "left edge of the zero-fill slice's border box must still be painted at ({},{y})",
                outline.left()
            );
            assert_ne!(
                buf.cell((outline.right() - 1, y)).unwrap().symbol(),
                " ",
                "right edge of the zero-fill slice's border box must still be painted at ({},{y})",
                outline.right() - 1
            );
        }
    }

    /// DELIVERABLE 3: a higher stat value paints its green fill strictly farther
    /// right. Measured by the rightmost GREEN-dominant cell (the hollow, unfilled
    /// remainder of the bar shows only the grey border, so a plain "rightmost
    /// non-space" would always hit the border edge and never scale).
    #[test]
    fn fill_length_scales_with_stat_value() {
        let stat_bar = RosterManager::stat_bar_rect(area());
        let slices = RosterManager::stat_slice_parts(stat_bar);
        let (outline, _fill, _label) = slices[1]; // Dexterity == StatKind::ALL[1]

        // Rightmost column within `outline` whose fg is green-dominant (the
        // `STAT_BAR_COLOR` fill blends green-dominant; the grey border does not).
        fn rightmost_green(buf: &ratatui::buffer::Buffer, rect: Rect) -> Option<u16> {
            (rect.left()..rect.right()).rev().find(|&x| {
                (rect.top()..rect.bottom()).any(|y| {
                    matches!(buf.cell((x, y)).unwrap().fg,
                        ratatui::style::Color::Rgb(r, g, b) if g > r && g > b)
                })
            })
        }

        let buf_low = render_with_stats(only_stat(StatKind::Dexterity, 5));
        let low_col = rightmost_green(&buf_low, outline);
        assert!(low_col.is_some(), "a non-zero Dexterity value must paint green in the DEX bar");

        let buf_high = render_with_stats(only_stat(StatKind::Dexterity, 35));
        let high_col = rightmost_green(&buf_high, outline);
        assert!(high_col.is_some(), "a higher Dexterity value must also paint green in the DEX bar");

        assert!(
            high_col.unwrap() > low_col.unwrap(),
            "a higher stat value (35) must fill green farther right ({high_col:?}) than a lower one (5) ({low_col:?})"
        );
    }

    /// DELIVERABLE 4: no ASCII digit is ever painted inside `stat_bar` —
    /// bars + STR/DEX/INT/VIT labels only, never numeric text.
    #[test]
    fn no_numeric_text_in_stat_bar() {
        let scene = RosterManager::new(); // index 0: Ember Wolf, real demo_roster stats
        let buf = render_to_buffer(&scene, area().width, area().height);
        let rect = RosterManager::stat_bar_rect(area());

        assert!(
            has_non_space(&buf, rect),
            "stat_bar must paint the current creature's stat bars"
        );
        let text = rect_text(&buf, rect);
        assert!(
            !text.chars().any(|c| c.is_ascii_digit()),
            "stat_bar must never render a numeric digit (bars + labels only); got {text:?}"
        );
    }

    /// DELIVERABLE 5: each slice's label text is its own `StatKind`'s name,
    /// rendered at/below that slice's own outline (label beneath its bar,
    /// not some other slice's).
    #[test]
    fn label_renders_beneath_its_own_bar() {
        let buf = render_to_buffer(&RosterManager::new(), area().width, area().height);
        let stat_bar = RosterManager::stat_bar_rect(area());
        let slices = RosterManager::stat_slice_parts(stat_bar);

        for (i, kind) in StatKind::ALL.into_iter().enumerate() {
            let (outline, _fill, label_rect) = slices[i];
            assert!(
                label_rect.y >= outline.bottom(),
                "slice {i}'s label must sit at/below its own outline's bottom edge"
            );
            let expected = RosterManager::stat_label(kind);
            let text = rect_text(&buf, label_rect);
            assert!(
                text.contains(expected),
                "slice {i}'s label rect must render {expected:?}, got {text:?}"
            );
        }
    }

    /// DELIVERABLE 6a: at the instant a slide is triggered (nav fired,
    /// `update()` not yet called), `stat_bar`'s CONTENT region (the 4 slices
    /// -- excludes `stat_bar`'s rightmost column, which is shared with the
    /// details panel's own conditionally-drawn border per b1-t1's documented
    /// "Known spec tension" and is NOT stat-bar content) renders IDENTICALLY
    /// to a resting render of the outgoing creature — outlines are static,
    /// fill at progress==0 equals the outgoing value (no positional slide).
    #[test]
    fn stat_bars_do_not_slide_positionally_at_trigger() {
        let stat_bar = RosterManager::stat_bar_rect(area());
        // Content region only: excludes the rightmost `EDGE_MARGIN +
        // DETAILS_LEFT_SHIFT` columns, which overlap the details panel's own
        // border and are only conditionally painted (the panel is hidden
        // mid-slide). After spec 38 item 4 the panel reaches
        // `DETAILS_LEFT_SHIFT` further left into these columns, so the
        // exclusion widened to match -- those columns differ by design, not by
        // a stat-bar regression.
        let rect = Rect::new(
            stat_bar.x,
            stat_bar.y,
            stat_bar
                .width
                .saturating_sub(RosterManager::EDGE_MARGIN + RosterManager::DETAILS_LEFT_SHIFT),
            stat_bar.height,
        );

        let rest_buf = render_to_buffer(&RosterManager::new(), area().width, area().height);
        assert!(
            has_non_space(&rest_buf, rect),
            "stat_bar content must paint the outgoing creature's bars at rest"
        );

        let mut scene = RosterManager::new();
        scene.handle_input(key_event(KeyCode::Right)); // triggers slide 0 -> 1, no update() yet
        let trigger_buf = render_to_buffer(&scene, area().width, area().height);

        assert_eq!(
            region_cells(&rest_buf, rect),
            region_cells(&trigger_buf, rect),
            "stat_bar content rendering at slide trigger (elapsed==start, no update()) must be identical to a resting render of the outgoing creature"
        );
    }

    /// DELIVERABLE 6b: mid-slide (real differing outgoing/incoming DEX
    /// values sourced from `demo_roster()`), the DEX slice's fill lies
    /// strictly between the two resting lengths — an eased numeric lerp,
    /// not a snap or a positional slide.
    #[test]
    fn fill_lerps_between_values_mid_slide() {
        // Measured in raw `stat_fill_dots` dot counts, not rendered buffer
        // cells: braille packs 2 dots per cell, so a 1-dot eased delta near
        // a cell boundary can quantize to the SAME rendered column as one of
        // the resting endpoints even though the underlying interpolation is
        // correct (see research.md b1-t6 iteration-2 "Correction" fallout /
        // validator TEST_ISSUE — the prior render-column form of this test
        // was a false negative caused by that quantization, not a code bug).
        // `stat_fill_dots` is the sole source of fill length for both the
        // render path and this assertion, so this still exercises the real
        // lerp logic end-to-end via a real `RosterManager` slide.
        let stat_bar = RosterManager::stat_bar_rect(area());
        let slices = RosterManager::stat_slice_parts(stat_bar);
        let (_outline, dex_fill, _label) = slices[1]; // Dexterity == StatKind::ALL[1]
        let dot_cols = dex_fill.width as usize * 2;

        // index 0 (Ember Wolf, DEX 28) -> index 1 (Frost Lizard, DEX 18): a
        // real gap sourced from demo_roster(), not synthetic stats.
        let out_rest = RosterManager::new().stat_fill_dots(StatKind::Dexterity, dot_cols);
        let in_rest = {
            let mut scene = RosterManager::new();
            scene.current_index = 1;
            scene.stat_fill_dots(StatKind::Dexterity, dot_cols)
        };
        assert_ne!(
            out_rest, in_rest,
            "test fixture requires index 0 and index 1 to have different DEX-driven fill dot counts"
        );

        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        scene.handle_input(key_event(KeyCode::Right));
        scene.update(&mut ctx, Duration::from_millis(75)); // ~25% of the 300ms SLIDE_DUR
        let mid_dots = scene.stat_fill_dots(StatKind::Dexterity, dot_cols);

        let (lo, hi) = if out_rest < in_rest { (out_rest, in_rest) } else { (in_rest, out_rest) };
        assert!(
            mid_dots > lo && mid_dots < hi,
            "mid-slide fill ({mid_dots} dots) must lie strictly between the outgoing ({out_rest}) and incoming ({in_rest}) resting dot counts"
        );
    }

    /// The 3-cell bar box's OUTLINE rect is top-aligned (level with the
    /// details-panel border) and snug to the label — but the visible hug
    /// bracket is recessed within it: the outline's TOP cell only lights its
    /// own BOTTOM `STAT_BAR_HUG_CAP_DOTS` dots (the cap directly above the
    /// fill), and its BOTTOM cell only lights its own TOP
    /// `STAT_BAR_HUG_CAP_DOTS` dots (the cap directly below the fill) — the
    /// outline's outermost dot-rows on each end stay genuinely empty. Proven
    /// at the dot level via the mid-span cells' glyph masks.
    #[test]
    fn stat_bar_box_is_top_aligned_and_snug_to_label() {
        // A zero stat so the border is fully visible everywhere (no green fill
        // to luma-blend over the border rows).
        let buf = render_with_stats(only_stat(StatKind::Strength, 0));
        let stat_bar = RosterManager::stat_bar_rect(area());
        let slices = RosterManager::stat_slice_parts(stat_bar);
        let (outline, _fill, label) = slices[0]; // Strength == StatKind::ALL[0]

        const CELL_TOP_HALF: u32 = (1 << 0) | (1 << 3) | (1 << 1) | (1 << 4);
        const CELL_BOTTOM_HALF: u32 = (1 << 2) | (1 << 5) | (1 << 6) | (1 << 7);

        // A cell mid-way along each border edge (avoid the chamfered corners).
        let cx = outline.left() + outline.width / 2;

        // The outline's TOP cell only lights its own bottom half (the hug cap
        // directly above the fill's cell) — its top half stays empty space.
        let top_mask = braille_mask(&buf, cx, outline.top())
            .expect("mid top-cap cell must be a painted braille glyph");
        assert!(
            top_mask & CELL_BOTTOM_HALF != 0 && top_mask & CELL_TOP_HALF == 0,
            "top cap cell (mask={top_mask:#04x}) must light only its BOTTOM half, hugging the fill below it"
        );

        // The outline's BOTTOM cell only lights its own top half (the hug cap
        // directly below the fill's cell), snug against the label below it.
        let bottom_cell_y = outline.bottom() - 1;
        let bottom_mask = braille_mask(&buf, cx, bottom_cell_y)
            .expect("mid bottom-cap cell must be a painted braille glyph");
        assert!(
            bottom_mask & CELL_TOP_HALF != 0 && bottom_mask & CELL_BOTTOM_HALF == 0,
            "bottom cap cell (mask={bottom_mask:#04x}) must light only its TOP half, hugging the fill above it"
        );

        // The bar box top shares a cell row with the details-panel border
        // top (see `stat_bar_top_and_details_panel_top_share_a_cell`).
        assert_eq!(
            outline.top(),
            RosterManager::exhaustion_rect(area()).y,
            "the stat-bar box top must share a cell row with the details-panel border top"
        );

        // The label cell sits immediately below the outline — no gap between the
        // bar box and its text.
        assert_eq!(
            label.y,
            outline.bottom(),
            "label cell must be directly below the outline (no gap between the box and its text)"
        );
    }

    /// REGRESSION (spec 38 refinement — "margin left on the leftmost bar"):
    /// the first slice starts a deliberate `STAT_BAR_LEFT_MARGIN` in from
    /// `stat_bar.x`, not flush at the screen's left edge, at multiple widths.
    #[test]
    fn first_bar_has_left_margin() {
        for w in [40u16, 60u16, 80u16] {
            let stat_bar = RosterManager::stat_bar_rect(Rect::new(0, 0, w, 30));
            let slices = RosterManager::stat_slice_parts(stat_bar);
            let (first_outline, _fill, _label) = slices[0];
            assert_eq!(
                first_outline.left(),
                stat_bar.x + RosterManager::STAT_BAR_LEFT_MARGIN,
                "w={w}: first bar must start STAT_BAR_LEFT_MARGIN in from stat_bar.x, not flush at the edge"
            );
            assert!(
                first_outline.left() > stat_bar.x,
                "w={w}: first bar must have a real, non-zero left margin off the stat_bar edge"
            );
        }
    }

    /// REGRESSION (spec 38 refinement — "labels one line below, no space
    /// between"): each label sits on the row IMMEDIATELY below its outline,
    /// with zero blank spacer rows between the bar and its label.
    #[test]
    fn label_sits_immediately_below_bar() {
        let stat_bar = RosterManager::stat_bar_rect(area());
        let slices = RosterManager::stat_slice_parts(stat_bar);
        for (i, (outline, _fill, label)) in slices.iter().enumerate() {
            assert_eq!(
                label.y,
                outline.bottom(),
                "slice {i}: label must sit on the row immediately below the bar outline (zero blank rows between)"
            );
        }
    }
}

