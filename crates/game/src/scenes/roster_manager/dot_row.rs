use super::*;

impl RosterManager {
    /// Draws one position-indicator circle (`bytes`) into `slot`, centered at
    /// dot precision — 2 dots lower than `draw_grid`'s default cell-granularity
    /// centering would place it. Composites the aspect-fitted circle into a
    /// slot-sized `DotBuffer` at the dot-precise offset (never a whole-cell
    /// round), then blits it 1:1. A zero-fit slot is a no-op.
    ///
    /// `(band_dx, band_dy)` is the dot-row band's sub-cell remainder (see
    /// `render_dot_row`): the slot's cell footprint drives the aspect-fit and
    /// cell-centering, while this remainder shifts the composited circle's
    /// dots uniformly inside a buffer grown to match, so the whole band's true
    /// dot position survives the eventual cell floor. Today it is `(0, 0)`, so
    /// this is byte-identical to a cell-only blit.
    pub(super) fn draw_dot_slot(buf: &mut Buffer, slot: Rect, band_dx: usize, band_dy: usize, bytes: &'static [u8]) {
        let slot_dc = slot.width as usize * 2;
        let slot_dr = slot.height as usize * 4;
        if slot_dc == 0 || slot_dr == 0 {
            return;
        }
        // Aspect-fit cell dims for the circle within the slot (same fit
        // `convert` would use), then fetch the colour-carrying dots at those
        // dims (a rasterize-cache hit after `convert`'s own call).
        let fitted = engine_render::asset_cache::convert(bytes, slot);
        let (fc, fr) = (fitted.cols(), fitted.rows());
        if fc == 0 || fr == 0 {
            return;
        }
        let circle = engine_render::asset_cache::sprite_to_dots(bytes, fc as u32 * 2, fr as u32 * 4);

        // `off_x` replicates `draw_grid`'s integer cell-centering (unchanged,
        // out of scope for this nudge). `off_y` centers at dot precision
        // instead of rounding to cells first — the same `+2` dot nudge this
        // used to apply as a separate constant now falls out of centering a
        // 4-dot-tall circle within an 8-dot-tall slot directly in dot space.
        let off_x = (slot.width.saturating_sub(fc as u16) / 2) as usize * 2;
        let off_y = slot_dr.saturating_sub(fr * 4) / 2;

        let mut target = DotBuffer::new(slot_dc + band_dx, slot_dr + band_dy);
        for r in 0..circle.rows() {
            for c in 0..circle.cols() {
                if let Dot::Lit(color) = circle.get(c, r) {
                    target.set(band_dx + off_x + c, band_dy + off_y + r, Dot::Lit(color));
                }
            }
        }
        let grid = dots_to_grid(&target);
        let draw_area = Rect {
            x: slot.x,
            y: slot.y,
            width: grid.cols() as u16,
            height: grid.rows() as u16,
        };
        engine_render::draw_grid(buf, draw_area, &grid);
    }

    /// Draws the `squad_role::ROSTER_SIZE`-slot dot row statically into
    /// `dot_row_rect`, filled at `self.current_index` — no `col_offset`, so
    /// it never travels with an in-flight sprite slide (b1-t3). Also paints
    /// each of the 3 role clusters' static "Active"/"Bench"/"Reserve" text
    /// label centered beneath it, in `dot_bands(dot_row_rect)`'s
    /// `label_band` (b2-t6). Non-text dots go through the dot pipeline
    /// (`asset_cache::convert` + `draw_grid`, unchanged mechanic); labels are
    /// plain text, so they go through `engine_render::label` (CLAUDE.md
    /// constraint 4).
    ///
    /// `dot_row_rect` (the dot-row band) is honored at DOT precision, not
    /// floored to the nearest cell first — the same sub-cell placement
    /// technique `draw_dot_border` uses. The band's whole-cell footprint
    /// (`to_cell_rect`) drives `dot_slots`/`dot_bands`/`dot_cluster_rects`'
    /// cell-granular geometry, while the band's sub-cell remainder `(dx, dy)`
    /// offsets each slot's DOT content uniformly (via `draw_dot_slot`) so the
    /// whole band's true dot position survives the floor. Labels are text, so
    /// they render at cell granularity off the floored origin. Today `(dx, dy)`
    /// is `(0, 0)` (the band is cell-aligned), so this is byte-identical to a
    /// cell-only draw.
    pub(super) fn render_dot_row(&self, buf: &mut Buffer, dot_row_rect: engine_render::DotRect) {
        let cell_rect = dot_row_rect.to_cell_rect();
        let (dxr, dyr) = dot_row_rect.cell_remainder();
        let (dx, dy) = (dxr as usize, dyr as usize);

        let slots = Self::dot_slots(cell_rect);
        self.current_dot.borrow_mut().set_rect(slots[self.current_index]);

        for (i, slot) in slots.iter().enumerate() {
            let filled = if Some(i) == self.selected_index {
                self.blink_on()
            } else {
                i == self.current_index
            };
            let bytes = if filled {
                crate::assets::DOT_FILLED
            } else {
                crate::assets::DOT_UNFILLED
            };
            Self::draw_dot_slot(buf, *slot, dx, dy, bytes);
        }

        let (dots_band, label_band) = Self::dot_bands(cell_rect);
        let clusters = Self::dot_cluster_rects(dots_band);
        for (cluster_rect, (_count, label)) in clusters.iter().zip(Self::CLUSTERS.iter()) {
            let label_w = label.chars().count() as u16;
            let center_x = cluster_rect.x + cluster_rect.width / 2;
            let label_rect = Rect::new(
                center_x.saturating_sub(label_w / 2),
                label_band.y,
                label_w,
                label_band.height,
            )
            .intersection(cell_rect);
            engine_render::label(
                buf,
                label_rect,
                label,
                engine_render::TextAlign::Center,
                ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
                    Self::DOT_LABEL_COLOR.r,
                    Self::DOT_LABEL_COLOR.g,
                    Self::DOT_LABEL_COLOR.b,
                )),
            );
        }
    }
}

#[cfg(test)]
mod dot_row_render_tests {
    use super::*;
    use crate::scenes::test_util::{render_to_buffer, sample_fg};
    use ratatui::style::Color;

    /// At `current_index == 0` (fresh `new()`), the dot row paints 6 distinct
    /// non-space dot-cell groups — one per `dot_slots` slot on `layout`'s
    /// `dots_rect` — and slot 0 (filled) paints a different fg than each of
    /// the other 5 (unfilled) slots.
    #[test]
    fn dot_row_six_groups_filled_at_index0() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let slots = RosterManager::dot_slots(dots_rect);

        let fgs: Vec<Color> = slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                sample_fg(&buf, *slot)
                    .unwrap_or_else(|| panic!("dot slot {i} must paint at least one non-space cell"))
            })
            .collect();

        for i in 1..6 {
            assert_ne!(
                fgs[0], fgs[i],
                "slot 0 (filled) fg must differ from slot {i} (unfilled) fg"
            );
        }
    }

    /// Setting `current_index = 3` moves the filled/brighter dot to slot 3;
    /// slot 0 now renders the unfilled color, distinct from slot 3.
    #[test]
    fn filled_dot_follows_current_index() {
        let mut scene = RosterManager::new();
        scene.current_index = 3;
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let slots = RosterManager::dot_slots(dots_rect);

        let fg0 = sample_fg(&buf, slots[0]).expect("slot 0 must paint at least one non-space cell");
        let fg3 = sample_fg(&buf, slots[3]).expect("slot 3 must paint at least one non-space cell");
        assert_ne!(
            fg0, fg3,
            "slot 0 (now unfilled) and slot 3 (now filled) fg must differ"
        );
    }

    /// spec 38 corrections (item 2 — "the roster dots should be moved down"):
    /// each indicator circle is centered at a genuine sub-cell, dot-precise
    /// offset (`draw_dot_slot`), so it now paints into the BOTTOM cell-row
    /// of its 2-cell slot. Before the nudge `draw_grid`'s integer cell-centering
    /// pinned the 1-cell circle to the slot's TOP row, leaving the bottom row
    /// blank — so this fails on the pre-nudge render.
    #[test]
    fn dot_circle_nudged_into_bottom_cell_row() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let slots = RosterManager::dot_slots(dots_rect);
        // Slot 0 is the current (filled) circle; its slot spans 2 cell rows.
        let slot = slots[0];
        assert!(slot.height >= 2, "dot slot must be 2 cells tall for a sub-cell nudge to be observable");
        let bottom_row = slot.bottom() - 1;
        let painted = (slot.left()..slot.right())
            .any(|x| buf.cell((x, bottom_row)).unwrap().symbol() != " ");
        assert!(
            painted,
            "the down-nudged circle must paint into its slot's bottom cell-row (y={bottom_row}); \
             a top-pinned circle would leave it blank"
        );
    }
}

/// b2-t6: the 6-slot dot row is re-laid into 3 role clusters (3 active / 1
/// bench / 2 reserve, derived from `crate::squad_role`'s slot constants —
/// never a hardcoded 3/1/2) with a real column gap between clusters and a
/// static plain-text role label under each. The whole band (dots + labels)
/// stays detached from the slide (unchanged mechanic from b1-t3).
#[cfg(test)]
mod dot_row_cluster_tests {
    use super::*;
    use crate::scenes::test_util::{key_event, rect_text, render_to_buffer};
    use crate::squad_role::{ACTIVE_SLOTS, BENCH_SLOTS, ROSTER_SIZE};
    use crossterm::event::KeyCode;
    use engine_core::scene::EngineCtx;

    /// Whether every cell in column `x` across `rect`'s full height is blank.
    fn column_is_blank(buf: &ratatui::buffer::Buffer, rect: Rect, x: u16) -> bool {
        (rect.top()..rect.bottom()).all(|y| buf.cell((x, y)).unwrap().symbol() == " ")
    }

    /// The dot row must show a real horizontal gap (at least one fully blank
    /// column, scanned across the slots' own row) between the active/bench
    /// boundary and the bench/reserve boundary — boundaries computed FROM
    /// `squad_role`'s slot constants, never hardcoded indices, so the test
    /// survives a constant change.
    #[test]
    fn dot_row_clusters_separated_by_gap_columns() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let slots = RosterManager::dot_slots(dots_rect);
        assert_eq!(slots.len(), ROSTER_SIZE);

        let active_bench_boundary = ACTIVE_SLOTS;
        let bench_reserve_boundary = ACTIVE_SLOTS + BENCH_SLOTS;

        for boundary in [active_bench_boundary, bench_reserve_boundary] {
            let left_slot = slots[boundary - 1];
            let right_slot = slots[boundary];
            let gap_cols: Vec<u16> = (left_slot.right()..right_slot.left())
                .filter(|&x| column_is_blank(&buf, dots_rect, x))
                .collect();
            assert!(
                !gap_cols.is_empty(),
                "expected at least one fully blank column between dot slot {} and {} (role cluster boundary); \
                 left_slot.right()={} right_slot.left()={} — clusters must not be contiguous",
                boundary - 1,
                boundary,
                left_slot.right(),
                right_slot.left()
            );
        }
    }

    /// Each of the 3 clusters has its role name rendered as static plain text
    /// somewhere in the dot row (below the dots).
    #[test]
    fn dot_row_labels_show_active_bench_reserve() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let text = rect_text(&buf, dots_rect);

        for label in ["Active", "Bench", "Reserve"] {
            assert!(
                text.contains(label),
                "dot row must render the role label {label:?} somewhere beneath its cluster; got {text:?}"
            );
        }
    }

    /// The dot row (dots + labels) renders identically whether or not a
    /// slide is currently active — extends b1-t3/b2-t1's
    /// `name_and_dot_row_do_not_slide_but_sprite_does` to also cover the new
    /// role labels this task adds (the prior test only covered the dots
    /// existing at all, not label text).
    #[test]
    fn dot_row_and_labels_identical_during_slide_and_at_rest() {
        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let l = RosterManager::layout(area);
        let mut ctx = EngineCtx;

        let mut mid_slide_scene = RosterManager::new();
        mid_slide_scene.handle_input(key_event(KeyCode::Right));
        mid_slide_scene.update(&mut ctx, Duration::from_millis(200));
        let mid_slide_buf = render_to_buffer(&mid_slide_scene, w, h);

        let mut rest_scene = RosterManager::new();
        rest_scene.current_index = 1;
        let rest_buf = render_to_buffer(&rest_scene, w, h);

        let mid_text = rect_text(&mid_slide_buf, l.dot_row);
        let rest_text = rect_text(&rest_buf, l.dot_row);

        assert_eq!(
            mid_text, rest_text,
            "dot row (dots + role labels) must render identically during an active slide vs. at rest"
        );
        for label in ["Active", "Bench", "Reserve"] {
            assert!(
                mid_text.contains(label),
                "role label {label:?} must still be present mid-slide (band is static chrome); got {mid_text:?}"
            );
        }
    }

    /// b1-t3: `CLUSTER_GAP` widens the group so adjacent clusters' LABEL text
    /// never occupies the same column and is separated by a real, visible
    /// blank-column margin (not just a single incidental column), at both
    /// 40-col and 80-col widths — stricter than
    /// `dot_row_clusters_separated_by_gap_columns` (which only checks the
    /// dot-slot gap, not the label text itself).
    ///
    /// Measures the blank-column run strictly BETWEEN each adjacent pair of
    /// clusters' own rendered label spans (bounded by
    /// `clusters[i].left()..clusters[i+1].right()`), never the full
    /// `label_band` width — since b1-t2, the flanking arrow buttons also
    /// paint within `dot_row`'s row range, so a full-row blank/non-blank
    /// scan would spuriously count their glyphs as label content.
    #[test]
    fn dot_row_cluster_labels_never_share_a_column() {
        // MIN_LABEL_GAP: the minimum acceptable blank-column margin between
        // two adjacent labels' rendered text. At `CLUSTER_GAP=4` the
        // "Bench"/"Reserve" pair (label text wider than its dot cluster)
        // is separated by exactly 1 blank column — a hairline gap, not the
        // "visibly separated" margin the spec requires. `CLUSTER_GAP=5`
        // widens every adjacent pair's margin past this threshold while
        // still comfortably clearing b1-t2's flanking arrows at 40-col
        // (unlike `8`, which does not — see spec 38's Decisions).
        const MIN_LABEL_GAP: u16 = 2;

        // Spec pins the exact number ("a modest increase... `5`") — assert
        // the literal value, not merely a margin a different value could
        // also satisfy.
        assert_eq!(
            RosterManager::CLUSTER_GAP,
            5,
            "CLUSTER_GAP must be exactly 5 per spec — not a range-tuned value"
        );

        for w in [40u16, 80u16] {
            let scene = RosterManager::new();
            let h = 20u16;
            let buf = render_to_buffer(&scene, w, h);

            let area = Rect::new(0, 0, w, h);
            let dot_row = RosterManager::layout(area).dot_row;
            let (dots_band, label_band) = RosterManager::dot_bands(dot_row);
            let clusters = RosterManager::dot_cluster_rects(dots_band);
            assert_eq!(clusters.len(), RosterManager::CLUSTERS.len());

            for pair in clusters.windows(2) {
                let (left_cluster, right_cluster) = (pair[0], pair[1]);
                let scan_left = left_cluster.left();
                let scan_right = right_cluster.right();

                // Walk the bounded sub-range, recording the end column of
                // the first non-blank run and the start column of the next
                // non-blank run after it.
                let mut first_run_end: Option<u16> = None;
                let mut second_run_start: Option<u16> = None;
                let mut in_run = false;
                for x in scan_left..scan_right {
                    let blank = column_is_blank(&buf, label_band, x);
                    if !blank && !in_run {
                        in_run = true;
                        if first_run_end.is_some() && second_run_start.is_none() {
                            second_run_start = Some(x);
                        }
                    } else if blank && in_run {
                        in_run = false;
                        if first_run_end.is_none() {
                            first_run_end = Some(x);
                        }
                    }
                }

                let first_run_end = first_run_end.unwrap_or_else(|| {
                    panic!("width={w}: expected a painted label run in [{scan_left},{scan_right}) for cluster pair {left_cluster:?}/{right_cluster:?}")
                });
                let second_run_start = second_run_start.unwrap_or_else(|| {
                    panic!("width={w}: expected a second painted label run after column {first_run_end} in [{scan_left},{scan_right}) for cluster pair {left_cluster:?}/{right_cluster:?}")
                });

                let gap = second_run_start - first_run_end;
                assert!(
                    gap >= MIN_LABEL_GAP,
                    "width={w}: adjacent cluster labels {left_cluster:?}/{right_cluster:?} are only \
                     separated by {gap} blank column(s) (need >= {MIN_LABEL_GAP}) — labels blend together"
                );
            }
        }
    }
}

