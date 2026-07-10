use super::*;

impl RosterManager {
    /// Deliberate blank-row clearance reserved between the home button's
    /// bottom edge and the details panel's top border. The home button and
    /// the details panel share the top-right column, so without this the
    /// panel jams flush against the button. The panel's top is pushed down
    /// by this amount (shrinking its height), never the button up.
    const DETAILS_TOP_GAP: u16 = 2;

    /// Pure translate (dots) pulling the details panel's whole container
    /// (top AND bottom) up. Combines with `DETAILS_PANEL_TOP_GROWTH_DOTS`
    /// (top-only) for a total 2-dot rise of the panel's top — see
    /// `Self::layout`'s `right_col`. Confirmed by eye against the live
    /// render, not derived from the stat-bar math (matching `stat_bar`'s
    /// exact top would require reasoning in dots, not the cell-rounded
    /// `Rect.y` fields — deliberately not attempted here).
    const DETAILS_PANEL_TOP_LIFT_DOTS: i32 = 1;
    /// Height-only, non-aspect-preserving grow (dots) anchored at the
    /// details panel's (already-translated) top — only the top moves this
    /// additional distance; the bottom stays where the translate left it.
    /// See `DETAILS_PANEL_TOP_LIFT_DOTS`.
    const DETAILS_PANEL_TOP_GROWTH_DOTS: i32 = 1;

    /// Height (in rows) of the `name` band at the top of the frame.
    const NAME_H: u16 = 2;
    /// Height of the `level` band directly below `name`.
    const LEVEL_H: u16 = 1;
    /// Height of the blank row above the `name`/`level` header block — a top
    /// margin that pushes the whole header block down by this amount while the
    /// body (`stat_bar`/`sprite`/details) stays anchored (the block's height
    /// is unchanged, so nothing below it moves). `level` still sits tight
    /// under `name` with no gap between them; the blank row lives above the
    /// block, not between `level` and the body.
    const HEADER_GAP_H: u16 = 1;
    /// Height (in rows) of the `stamina` sub-region at the top of the
    /// details panel.
    const PANEL_H: u16 = 5;
    /// Cells `left_col`'s container top (and `stat_bar`/its labels with it)
    /// sits above `details_top`, with the container's height extended by the
    /// same amount so its BOTTOM — and therefore `sprite`'s pinned baseline —
    /// doesn't move. The freed space becomes `sprite`'s (the sole grow
    /// child's) to claim automatically; see `Self::layout`'s `left_col`.
    const STAT_BAR_TOP_LIFT_CELLS: u16 = 1;
    /// Height of the `dot_row` band at the bottom of the frame — the dots
    /// themselves (`DOT_H - DOT_LABEL_H` rows) plus one row of static role
    /// labels underneath (b2-t6).
    const DOT_H: u16 = 3;
    /// Height (in rows) of the role-label row at the bottom of `dot_row`
    /// (b2-t6). See `dot_bands`.
    const DOT_LABEL_H: u16 = 1;
    /// Width (in cells) of a single dot slot within a cluster (b1-t3/b2-t6).
    /// 2 cells wide × the dots band's 1-cell height = 4×4 dots per
    /// indicator — enough resolution for a recognizable filled/unfilled
    /// circle. Dividing row.width/N instead (an earlier approach) gave each
    /// slot far more width than the aspect-fit circle could ever use.
    const SLOT_W: u16 = 2;
    /// Converts a whole-cell `Rect` into dot space (2 dots wide, 4 dots tall
    /// per cell) — the sole cell->dot boundary `layout()` uses on its way
    /// into `flex()`/`DotRect::inset()`; rounding back to cells happens once,
    /// via `DotRect::to_cell_rect()`, at each field's assignment below (b8-t1).
    pub(super) fn cell_rect_to_dots(r: Rect) -> engine_render::DotRect {
        engine_render::DotRect {
            x: r.x as i32 * 2,
            y: r.y as i32 * 4,
            w: r.width as i32 * 2,
            h: r.height as i32 * 4,
        }
    }

    /// Raw, UNFLOORED dot geometry for the top-level vertical bands — the
    /// single source both `layout()` (which floors `top_bands[1]`/`[2]`/`[4]`
    /// into its `name`/`level`/`dot_row` fields), `right_col_dots` (which
    /// clamps to `top_bands[4].to_cell_rect().y`), and `render()` (which
    /// hands `top_bands[4]` straight to `render_dot_row` at dot precision)
    /// derive from. Extracted so the `flex()` call is written ONCE and the
    /// dot precision flows, unfloored, to the draw site — the same discipline
    /// as `right_col_dots`. A blank `HEADER_GAP_H` top-margin row, then
    /// `name`/`level` tight to each other, a spacer `body` child (the sole
    /// grow child, absorbing leftover main-axis space), then `dot_row` pinned
    /// to the bottom.
    pub(super) fn top_bands_dots(area: Rect) -> [engine_render::DotRect; 5] {
        let area_dots = Self::cell_rect_to_dots(area);
        let bands = engine_render::flex(
            area_dots,
            engine_render::FlexStyle {
                direction: engine_render::Direction::Column,
                justify_content: engine_render::Justify::Start,
                align_items: engine_render::Align::Stretch,
                gap: 0,
            },
            &[
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::HEADER_GAP_H as i32 * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::NAME_H as i32 * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::LEVEL_H as i32 * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::DOT_H as i32 * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
            ],
        );
        [bands[0], bands[1], bands[2], bands[3], bands[4]]
    }

    /// Raw, UNFLOORED dot geometry for the LEFT column: `[stat_bar, sprite]`,
    /// `stat_bar` directly above `sprite` (identical column range). The sole
    /// source both `layout()` (which floors each into its `stat_bar`/`sprite`
    /// fields) and `render()` (which hands `left_col_dots(area)[0]` straight
    /// to `render_stat_bars` at dot precision) derive from — the same
    /// discipline as `right_col_dots`, so the stat-bar band's dot position
    /// survives, unfloored, all the way to the draw site instead of being
    /// re-expanded from an already-cell-floored `Rect`.
    ///
    /// This container's own top sits `STAT_BAR_TOP_LIFT_CELLS` higher than
    /// `details_top`, with height extended to match, so the container's BOTTOM
    /// (still `dot_row`'s top) doesn't move. `stat_bar` (Fixed) shifts up by
    /// exactly that lift; `sprite` (the sole grow child) absorbs the freed
    /// space at its TOP. `left_w`/`details_top` stay integer-cell math and are
    /// fed in as cell-aligned `Basis::Fixed` dots.
    pub(super) fn left_col_dots(area: Rect) -> [engine_render::DotRect; 2] {
        let dot_row = Self::top_bands_dots(area)[4].to_cell_rect();
        let left_w = area.width * 2 / 3;
        let home_bottom = area
            .y
            .saturating_add(Self::EDGE_MARGIN)
            .saturating_add(Self::HOME_H);
        let details_top = home_bottom
            .saturating_add(Self::DETAILS_TOP_GAP)
            .min(dot_row.y);
        let body_h_dots = dot_row.y.saturating_sub(details_top) as i32 * 4;

        let left_col = engine_render::flex(
            engine_render::DotRect {
                x: area.x as i32 * 2,
                y: (details_top as i32 - Self::STAT_BAR_TOP_LIFT_CELLS as i32) * 4,
                w: left_w as i32 * 2,
                h: body_h_dots + Self::STAT_BAR_TOP_LIFT_CELLS as i32 * 4,
            },
            engine_render::FlexStyle {
                direction: engine_render::Direction::Column,
                justify_content: engine_render::Justify::Start,
                align_items: engine_render::Align::Stretch,
                gap: 0,
            },
            &[
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::STAT_BAR_BAND_H as i32 * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            ],
        );
        [left_col[0], left_col[1]]
    }

    /// Dot-precise geometry for the RIGHT column (stamina directly above
    /// ability_list, the details panel) — factored out of `layout()` into
    /// its own function so `details_panel_rects` can draw the panel's
    /// border at genuine dot precision, instead of re-deriving dots from
    /// `RosterLayout`'s already-cell-floored `stamina`/`ability_list`
    /// fields (which have permanently lost any sub-cell remainder the
    /// instant `layout()` calls `.to_cell_rect()` on them — re-expanding a
    /// floored cell value back into dots via `cell_rect_to_dots` cannot
    /// recover what `.to_cell_rect()` already threw away). This was the
    /// actual cause of the details panel's border rendering 2 dots off
    /// from `stat_bar`'s — not a wrong lift/growth amount.
    ///
    /// Width = `area.width - left_w`, right edge inset `EDGE_MARGIN` from
    /// `area`'s edge. Its TOP is pushed below the home button plus a
    /// deliberate `DETAILS_TOP_GAP` so the panel never jams against the
    /// home button in the shared top-right column; it still bottoms out at
    /// `dot_row`'s top. Two further, distinct adjustments compose on top of
    /// that: `DETAILS_PANEL_TOP_LIFT_DOTS` is a pure translate (shifts the
    /// whole container, top AND bottom, up by that amount), then
    /// `DETAILS_PANEL_TOP_GROWTH_DOTS` is a height-only, non-aspect-
    /// preserving grow anchored at the already-translated top, so only the
    /// top moves the extra distance and the bottom stays exactly where the
    /// translate left it.
    pub(super) fn right_col_dots(area: Rect) -> [engine_render::DotRect; 2] {
        let area_dots = Self::cell_rect_to_dots(area);

        let dot_row = Self::top_bands_dots(area)[4].to_cell_rect();

        let left_w = area.width * 2 / 3;
        let details_w = area.width.saturating_sub(left_w);
        let home_bottom = area
            .y
            .saturating_add(Self::EDGE_MARGIN)
            .saturating_add(Self::HOME_H);
        let details_top = home_bottom
            .saturating_add(Self::DETAILS_TOP_GAP)
            .min(dot_row.y);

        let details_x_dots = engine_render::flex(
            area_dots.inset(0, (Self::EDGE_MARGIN + Self::DETAILS_LEFT_SHIFT) as i32 * 2, 0, 0),
            engine_render::FlexStyle {
                direction: engine_render::Direction::Row,
                justify_content: engine_render::Justify::End,
                align_items: engine_render::Align::Start,
                gap: 0,
            },
            &[engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(details_w as i32 * 2),
                grow: 0.0,
                shrink: 0.0,
            }],
        )[0]
        .x;

        let body_h_dots = dot_row.y.saturating_sub(details_top) as i32 * 4;
        let body_style = engine_render::FlexStyle {
            direction: engine_render::Direction::Column,
            justify_content: engine_render::Justify::Start,
            align_items: engine_render::Align::Stretch,
            gap: 0,
        };

        let right_col = engine_render::flex(
            engine_render::DotRect {
                x: details_x_dots,
                y: details_top as i32 * 4
                    - Self::DETAILS_PANEL_TOP_LIFT_DOTS
                    - Self::DETAILS_PANEL_TOP_GROWTH_DOTS,
                w: details_w as i32 * 2,
                h: body_h_dots + Self::DETAILS_PANEL_TOP_GROWTH_DOTS,
            },
            body_style,
            &[
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::PANEL_H as i32 * 4),
                    grow: 0.0,
                    shrink: 0.0,
                },
                engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            ],
        );
        [right_col[0], right_col[1]]
    }

    /// Splits `area` into the 7 named panel rects (b1-t1, research.md
    /// blueprint), top to bottom: `name`, `level` (tight under `name`, no
    /// gap), a blank `HEADER_GAP_H` row, then the body, then `dot_row`. The
    /// body is a 2:1 LEFT/RIGHT column split: LEFT holds `stat_bar` directly
    /// above `sprite` (identical column range, width `area.width * 2 / 3`);
    /// RIGHT holds `stamina` directly above `ability_list` (the details
    /// panel), inset from `area`'s right edge by `EDGE_MARGIN`. Computed on
    /// dot-precision `flex()`/`DotRect` (b8-t1); `left_w`/`details_w`/
    /// `details_top` stay integer-cell math (a `flex`-grow split of the 2:1
    /// column is NOT equivalent to `width*2/3` floored in cells — see
    /// research.md's CAVEAT) and are fed into the dot-space calls below as
    /// cell-aligned `Basis::Fixed` dots. Uses saturating arithmetic
    /// throughout so small `area`s degrade to zero-height/width rects
    /// instead of panicking.
    pub(super) fn layout(area: Rect) -> RosterLayout {
        // Top-level vertical bands (`top_bands_dots`): `name`/`level` floored
        // out of the header block, `dot_row` pinned to the bottom.
        // `top_bands[0]` (the blank `HEADER_GAP_H` top margin) and
        // `top_bands[3]` (the spacer `body` grow child) are unused here — the
        // left/right columns anchor to `details_top`, not to those rects.
        let top_bands = Self::top_bands_dots(area);
        let name = top_bands[1].to_cell_rect();
        let level = top_bands[2].to_cell_rect();
        let dot_row = top_bands[4].to_cell_rect();

        // LEFT column: `sprite`, floored out of the shared, dot-precise
        // `left_col_dots`. `stat_bar` (`left_col_dots[0]`) is NOT stored here
        // — nothing in production reads a cell-floored copy (`render()` hands
        // it to `render_stat_bars` unfloored); tests call `stat_bar_rect`.
        // Mirrors the `stamina`/`ability_list` case.
        let sprite = Self::left_col_dots(area)[1].to_cell_rect();

        // RIGHT column (stamina/ability_list, the details panel): NOT
        // computed here — `RosterLayout` no longer carries cell-floored
        // copies of them (nothing in production code read them; the real
        // consumer, `details_panel_rects`, calls `Self::right_col_dots`
        // directly to get dot precision `layout()`'s own `.to_cell_rect()`
        // calls would throw away — see `right_col_dots`'s doc comment).
        // Callers needing these two rects at cell precision call
        // `Self::right_col_dots(area)` and `.to_cell_rect()` themselves.
        RosterLayout {
            name,
            level,
            sprite,
            dot_row,
        }
    }

    /// Splits `dot_row_rect` (`layout()`'s `dot_row`) into the top `dots_band`
    /// (where dot slots live) and the bottom `label_band` (one row of static
    /// role-label text), per `DOT_LABEL_H` (b2-t6). Built on `engine_render::
    /// flex()`/`DotRect` (b8-t2) — a Column `flex()` with the dots slot as
    /// the sole grow child and the label slot `Fixed(DOT_LABEL_H*4)` with
    /// `shrink: 1.0` so an undersized `row` degrades identically to the
    /// former `.min()`/`saturating_sub` clamp.
    pub(super) fn dot_bands(row: Rect) -> (Rect, Rect) {
        let out = engine_render::flex(
            Self::cell_rect_to_dots(row),
            engine_render::FlexStyle {
                direction: engine_render::Direction::Column,
                justify_content: engine_render::Justify::Start,
                align_items: engine_render::Align::Stretch,
                gap: 0,
            },
            &[
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(0),
                    grow: 1.0,
                    shrink: 0.0,
                },
                engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(Self::DOT_LABEL_H as i32 * 4),
                    grow: 0.0,
                    shrink: 1.0,
                },
            ],
        );
        (out[0].to_cell_rect(), out[1].to_cell_rect())
    }

    /// The 3 role-cluster rects within `dots_band`, in `CLUSTERS` order,
    /// centered as a group with `CLUSTER_GAP` columns between adjacent
    /// clusters (b2-t6). Built on `engine_render::flex()`/`DotRect` (b8-t2) —
    /// a Row `flex()` with `Justify::Center` and `gap: CLUSTER_GAP` — never
    /// hand-rolled x-accumulation.
    pub(super) fn dot_cluster_rects(dots_band: Rect) -> Vec<Rect> {
        let children: Vec<engine_render::FlexChild> = Self::CLUSTERS
            .iter()
            .map(|(count, _label)| engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(*count as i32 * Self::SLOT_W as i32 * 2),
                grow: 0.0,
                shrink: 0.0,
            })
            .collect();
        engine_render::flex(
            Self::cell_rect_to_dots(dots_band),
            engine_render::FlexStyle {
                direction: engine_render::Direction::Row,
                justify_content: engine_render::Justify::Center,
                align_items: engine_render::Align::Stretch,
                gap: Self::CLUSTER_GAP as i32 * 2,
            },
            &children,
        )
        .iter()
        .map(|d| d.to_cell_rect())
        .collect()
    }

    /// The visual left/right column bounds of the centered dot-cluster group
    /// within `layout(area).dot_row` — the union of the dot clusters
    /// (`dot_cluster_rects`) AND the role labels centered beneath them (the
    /// labels are wider than their clusters, so they, not the dots, set the
    /// group's true horizontal extent). `arrow_rects` flanks THIS extent — not
    /// the full-width band (the bug that left arrows at the screen edges) and
    /// not the dots-only bounds (which would let the arrows overwrite the
    /// wider labels) — so the arrows sit immediately outside the whole group
    /// without ever landing on a label glyph. Sole source of these bounds;
    /// `arrow_rects` and its tests both call it.
    pub(super) fn dot_cluster_group_bounds(area: Rect) -> (u16, u16) {
        let band = Self::layout(area).dot_row;
        let (dots_band, _label_band) = Self::dot_bands(band);
        let clusters = Self::dot_cluster_rects(dots_band);
        let mut left = band.right();
        let mut right = band.left();
        for (cluster, (_count, label)) in clusters.iter().zip(Self::CLUSTERS.iter()) {
            let label_w = label.chars().count() as u16;
            let center_x = cluster.x + cluster.width / 2;
            let label_left = center_x.saturating_sub(label_w / 2);
            let label_right = label_left + label_w;
            left = left.min(cluster.left()).min(label_left);
            right = right.max(cluster.right()).max(label_right);
        }
        (left, right)
    }

    /// The `squad_role::ROSTER_SIZE` dot slots across `row`, grouped into 3
    /// role clusters (b2-t6, per `CLUSTERS`/`dot_cluster_rects`) — indices
    /// `0..ACTIVE_SLOTS` active, then `BENCH_SLOTS` bench, then
    /// `RESERVE_SLOTS` reserve, flattened in roster-index order. Signature
    /// stays callable exactly as before (`RosterManager::dot_slots(row)`),
    /// so existing callers/tests keep working unchanged. Each cluster's slots
    /// are computed via a per-cluster Row `flex()` (b8-t3), like
    /// `dot_bands`/`dot_cluster_rects`.
    pub(super) fn dot_slots(row: Rect) -> [Rect; crate::squad_role::ROSTER_SIZE] {
        let (dots_band, _label_band) = Self::dot_bands(row);
        let clusters = Self::dot_cluster_rects(dots_band);

        let mut slots = Vec::with_capacity(crate::squad_role::ROSTER_SIZE);
        for (cluster_rect, (count, _label)) in clusters.iter().zip(Self::CLUSTERS.iter()) {
            let slot_w = Self::SLOT_W.min(cluster_rect.width.max(1));
            let children: Vec<engine_render::FlexChild> = (0..*count)
                .map(|_| engine_render::FlexChild {
                    basis: engine_render::Basis::Fixed(slot_w as i32 * 2),
                    grow: 0.0,
                    shrink: 0.0,
                })
                .collect();
            let cluster_slots = engine_render::flex(
                Self::cell_rect_to_dots(*cluster_rect),
                engine_render::FlexStyle {
                    direction: engine_render::Direction::Row,
                    justify_content: engine_render::Justify::Start,
                    align_items: engine_render::Align::Stretch,
                    gap: 0,
                },
                &children,
            );
            slots.extend(cluster_slots.iter().map(|d| d.to_cell_rect()));
        }
        slots.try_into().unwrap_or_else(|v: Vec<Rect>| {
            panic!(
                "dot_slots: expected {} slots, computed {}",
                crate::squad_role::ROSTER_SIZE,
                v.len()
            )
        })
    }

    /// Cell-space view of `left_col_dots(area)[0]` (`stat_bar`), for tests
    /// only — production code that needs dot precision calls `left_col_dots`
    /// directly instead (`render()` hands it to `render_stat_bars` unfloored;
    /// see `left_col_dots`'s doc comment). `RosterLayout` no longer carries a
    /// cell-floored copy, mirroring the `stamina`/`ability_list` case.
    #[cfg(test)]
    pub(super) fn stat_bar_rect(area: Rect) -> Rect {
        Self::left_col_dots(area)[0].to_cell_rect()
    }

    /// Cell-space view of `right_col_dots(area)[0]` (`stamina`), for
    /// tests only — production code that needs dot precision calls
    /// `right_col_dots` directly instead (see its doc comment).
    #[cfg(test)]
    pub(super) fn stamina_rect(area: Rect) -> Rect {
        Self::right_col_dots(area)[0].to_cell_rect()
    }

    /// Cell-space view of `right_col_dots(area)[1]` (`ability_list`), for
    /// tests only — production code that needs dot precision calls
    /// `right_col_dots` directly instead (see its doc comment).
    #[cfg(test)]
    pub(super) fn ability_list_rect(area: Rect) -> Rect {
        Self::right_col_dots(area)[1].to_cell_rect()
    }

    /// Details-panel geometry (b1-t5): `(border, ex_text, ability_text)`.
    /// `border` is the DOT-PRECISE (not cell-floored — see `draw_dot_border`)
    /// 1-cell-perimeter rect around the union of `right_col_dots(area)`'s
    /// two elements (they share x/width and are stacked contiguously in y,
    /// so the union is exact without needing `Rect::union`) — calls
    /// `right_col_dots` directly, NOT `layout(area).stamina`/
    /// `.ability_list`, which are already cell-floored and cannot recover
    /// the sub-cell precision `draw_dot_border` needs (see
    /// `right_col_dots`'s doc comment). The text rects are inset 1 cell off
    /// every bordered edge — there is NO border between stamina and
    /// ability_list (they share an interior boundary), so `ability_text`
    /// keeps its top edge un-inset. Sole source of this geometry; `render()`
    /// and tests both call this rather than re-deriving it.
    pub(super) fn details_panel_rects(area: Rect) -> (engine_render::DotRect, Rect, Rect) {
        let [ex_dots, ab_dots] = Self::right_col_dots(area);
        // `border` is a union (they share x/width and are y-contiguous), not
        // an inset — no negative-inset API, so build it via struct-update.
        // Deliberately NOT `.to_cell_rect()`'d here — `draw_dot_border` now
        // consumes this at dot precision directly (see its doc comment for
        // why flooring it here would silently re-introduce the bug where
        // this panel's border landed 2 dots off from stat_bar's).
        let border = engine_render::DotRect {
            h: ex_dots.h + ab_dots.h,
            ..ex_dots
        };
        // 1 cell L/R, 1 cell off the TOP border only — stamina/ability
        // share an un-bordered interior boundary, so no bottom inset.
        let ex_text = ex_dots.inset(2, 2, 4, 0).to_cell_rect();
        // 1 cell L/R, 1 cell off the BOTTOM border only — no top inset
        // (shared boundary with `ex_text` above).
        let ability_text = ab_dots.inset(2, 2, 0, 4).to_cell_rect();
        (border, ex_text, ability_text)
    }

}

/// b1-t1: `layout()`'s expanded 7-rect contract — the shared layout every
/// rendering function renders into. Header spacing: `level` sits tight under
/// `name` (no gap), then `HEADER_GAP_H` blank row, then the body. Body is a
/// 2:1 LEFT/RIGHT column split: LEFT holds `stat_bar` above `sprite`
/// (identical column range); RIGHT holds `stamina` above `ability_list`
/// (the details panel), inset from `area`'s right edge by `EDGE_MARGIN`.
#[cfg(test)]
mod layout_tests {
    use super::*;

    /// `layout(area)` must order its bands top-to-bottom: name < level <
    /// stat_bar/sprite/dot_row, with `level` tight under `name` (no blank
    /// row) and `stamina` above `ability_list`. `HEADER_GAP_H` is fully
    /// consumed as a top margin ABOVE `name` (spec 38 item 6), so `level`'s
    /// bottom edge sits FLUSH against `body_y` (no gap there) — any visible
    /// space before `stat_bar.y` is incidental spillover from `stat_bar`
    /// being independently anchored to `details_top`
    /// (`home_bottom + DETAILS_TOP_GAP`), not a deliberate header-to-body
    /// gap. Both are still asserted below since `stat_bar` must not overlap
    /// `level` regardless of which mechanism produces the separation.
    #[test]
    fn layout_rects_ordered_top_to_bottom() {
        let area = Rect::new(0, 0, 80, 30);
        let l = RosterManager::layout(area);
        let stat_bar = RosterManager::stat_bar_rect(area);

        assert!(l.name.y < l.level.y, "name.y ({}) must be above level.y ({})", l.name.y, l.level.y);
        assert_eq!(
            l.name.y + l.name.height, l.level.y,
            "level must sit directly under name with no blank row (name.y={} + name.height={} != level.y={})",
            l.name.y, l.name.height, l.level.y
        );
        assert!(
            l.level.y + l.level.height <= stat_bar.y,
            "stat_bar must not start above level's bottom edge (level {}+{} vs stat_bar.y {}); note this gap comes from \
             stat_bar's independent details_top anchoring, not from HEADER_GAP_H (which is now a top margin above name)",
            l.level.y, l.level.height, stat_bar.y
        );
        let stamina = RosterManager::stamina_rect(area);
        let ability_list = RosterManager::ability_list_rect(area);
        assert!(stamina.y < ability_list.y, "stamina.y ({}) must be above ability_list.y ({})", stamina.y, ability_list.y);
        assert!(stat_bar.y < l.sprite.y, "stat_bar.y ({}) must be above sprite.y ({})", stat_bar.y, l.sprite.y);
        assert!(l.sprite.y < l.dot_row.y, "sprite.y ({}) must be above dot_row.y ({})", l.sprite.y, l.dot_row.y);
    }

    /// spec 38 corrections (item 6): the `name`/`level` header block is
    /// shifted DOWN by one full cell (`HEADER_GAP_H`) — the blank gap row now
    /// sits ABOVE the block, not between `level` and the body — while the body
    /// (`stat_bar`/`sprite`/`dot_row`) stays put. `name` and `level` remain
    /// tight to each other (no gap between them). Asserted at several sizes.
    #[test]
    fn header_block_shifted_down_one_cell_gap_above() {
        for (w, h) in [(80u16, 30u16), (40u16, 20u16), (60u16, 24u16)] {
            let area = Rect::new(0, 0, w, h);
            let l = RosterManager::layout(area);
            let stat_bar = RosterManager::stat_bar_rect(area);
            assert_eq!(
                l.name.y,
                area.y + RosterManager::HEADER_GAP_H,
                "w={w},h={h}: name.y must be pushed down by HEADER_GAP_H (blank top-margin row above the header block)"
            );
            assert_eq!(
                l.name.y + l.name.height,
                l.level.y,
                "w={w},h={h}: level must stay tight under name (header-block shift moves them together)"
            );
            // The body is unchanged by the header shift: stat_bar and the
            // details panel top still land on the same CELL row (their
            // independent dot-level nudges — STAT_BAR_TOP_LIFT_CELLS vs.
            // DETAILS_PANEL_TOP_LIFT_DOTS/GROWTH_DOTS — aren't required to
            // be dot-identical, only close enough to round to the same
            // cell; see `stat_bar_top_and_details_panel_top_share_a_cell`).
            assert_eq!(
                stat_bar.y, RosterManager::stamina_rect(area).y,
                "w={w},h={h}: body position unchanged by the header shift"
            );
        }
    }

    /// The details panel (`stamina`/`ability_list`) width must equal
    /// `area.width - area.width * 2 / 3` (the RIGHT column of the 2:1 split).
    #[test]
    fn details_panel_width_is_one_third() {
        for width in [60u16, 90u16] {
            let area = Rect::new(0, 0, width, 30);
            let expected = width - (width * 2 / 3);
            assert_eq!(RosterManager::stamina_rect(area).width, expected, "width={width}: stamina.width");
            assert_eq!(RosterManager::ability_list_rect(area).width, expected, "width={width}: ability_list.width");
        }
    }

    /// `stat_bar` sits directly above `sprite`, spanning the identical
    /// column range (both are the LEFT column of the 2:1 split).
    #[test]
    fn stat_bar_spans_sprite_columns_and_sits_above() {
        let area = Rect::new(0, 0, 80, 30);
        let l = RosterManager::layout(area);
        let stat_bar = RosterManager::stat_bar_rect(area);
        assert_eq!(stat_bar.left(), l.sprite.left(), "stat_bar.left() must equal sprite.left()");
        assert_eq!(stat_bar.right(), l.sprite.right(), "stat_bar.right() must equal sprite.right()");
        assert!(
            stat_bar.y + stat_bar.height <= l.sprite.y,
            "stat_bar ({}+{}) must sit above sprite.y ({})",
            stat_bar.y, stat_bar.height, l.sprite.y
        );
    }

    /// Follow-up on spec 38 correction (item 4): `stat_bar`'s top
    /// (raised by `STAT_BAR_TOP_LIFT_CELLS`) and the details panel's top
    /// (raised independently by `DETAILS_PANEL_TOP_LIFT_DOTS` +
    /// `DETAILS_PANEL_TOP_GROWTH_DOTS`) must share a cell row — the coarse,
    /// necessary-but-not-sufficient check; see
    /// `stat_bar_and_details_panel_borders_visually_align_at_dot_level`
    /// (rendered-buffer test, same module) for the actual DOT-level
    /// guarantee, which is what the project owner actually cares about —
    /// two borders sharing a cell row can still be several dots apart
    /// visually if their draw routines don't use the same sub-cell
    /// convention (this is exactly the bug that shipped once already).
    #[test]
    fn stat_bar_top_and_details_panel_top_share_a_cell() {
        for (w, h) in [(80u16, 30u16), (40u16, 20u16), (60u16, 24u16)] {
            let area = Rect::new(0, 0, w, h);
            let stat_bar = RosterManager::stat_bar_rect(area);
            let stamina = RosterManager::stamina_rect(area);
            assert_eq!(
                stat_bar.y, stamina.y,
                "w={w},h={h}: stat_bar.y ({}) and the details panel top stamina.y ({}) must share a cell row",
                stat_bar.y, stamina.y
            );
        }
    }

    /// The DOT-level guarantee `stat_bar_top_and_details_panel_top_share_a_cell`
    /// can't provide: decodes the actual rendered braille buffer (not the
    /// `Rect`/`DotRect` geometry) and confirms `stat_bar`'s border and the
    /// details panel's border have their topmost LIT dot at the same
    /// absolute dot row. This is the real acceptance bar the project owner
    /// asked for — two border-drawing routines with different sub-cell
    /// conventions (`stat_bar`'s "hug cap" recess vs. `draw_dot_border`'s
    /// plain top-row line) can share a cell row while still being visibly
    /// offset; only decoding actual lit dots proves otherwise. Regression
    /// for the bug where they were 2 dots apart despite `layout()` agreeing
    /// on the cell.
    #[test]
    fn stat_bar_and_details_panel_borders_visually_align_at_dot_level() {
        use crate::scenes::test_util::{render_to_buffer, topmost_lit_dot_row};

        let scene = RosterManager::new();
        let area = Rect::new(0, 0, 80, 30);
        let stamina = RosterManager::stamina_rect(area);
        let buf = render_to_buffer(&scene, 80, 30);

        let stat_bar = RosterManager::stat_bar_rect(area);
        let stat_bar_top = topmost_lit_dot_row(&buf, stat_bar.x + 2, stat_bar.y);
        let details_top = topmost_lit_dot_row(&buf, stamina.x + 2, stamina.y);
        assert_eq!(
            stat_bar_top, details_top,
            "stat_bar's border (topmost lit dot row {stat_bar_top}) and the details panel's border \
             (topmost lit dot row {details_top}) must visually align, not just share a cell"
        );
    }

    /// spec 38 correction (item 4/5), narrowed by `STAT_BAR_TOP_LIFT_CELLS`:
    /// the stat bar band no longer starts right after the header — a real
    /// (nonzero) blank gap still separates the header (`level`) from the
    /// stat bar band, even after the band moved one cell closer to it.
    #[test]
    fn real_gap_between_header_and_stat_bar() {
        let l = RosterManager::layout(Rect::new(0, 0, 80, 30));
        let stat_bar = RosterManager::stat_bar_rect(Rect::new(0, 0, 80, 30));
        let gap = stat_bar.y.saturating_sub(l.level.y + l.level.height);
        assert!(
            gap >= 1,
            "expected a real blank gap (>=1 row) between level (bottom={}) and stat_bar (top={}), got {gap}",
            l.level.y + l.level.height, stat_bar.y
        );
    }

    /// b2-t5 layout fix, updated for the 2:1 column split (b1-t1): the
    /// sprite (LEFT column) and the ability_list (RIGHT column, inset
    /// `EDGE_MARGIN` from `area`'s edge) are forced to share exactly
    /// `EDGE_MARGIN` columns at the panel border — see research.md's "Known
    /// spec tension" — so the disjoint check tolerates that fixed overlap
    /// rather than requiring zero overlap.
    #[test]
    fn sprite_and_ability_list_columns_disjoint() {
        for width in [80u16, 60u16] {
            let area = Rect::new(0, 0, width, 30);
            let l = RosterManager::layout(area);
            let ability_list = RosterManager::ability_list_rect(area);
            // The details panel is pulled `DETAILS_LEFT_SHIFT` further left off
            // the right edge (spec 38 corrections item 4), so it now shares up
            // to `EDGE_MARGIN + DETAILS_LEFT_SHIFT` columns with the LEFT
            // column's sprite band — a nominal-rect overlap only; the actual
            // inset+centered sprite art never reaches those columns (verified
            // by render-to-buffer).
            let tolerance = RosterManager::EDGE_MARGIN + RosterManager::DETAILS_LEFT_SHIFT;
            assert!(
                l.sprite.right() <= ability_list.left() + tolerance,
                "width={}: sprite ({:?}) must not extend more than {tolerance} cells into ability_list ({:?})",
                width, l.sprite, ability_list
            );
        }
    }

    /// spec 38 corrections (item 4): the details panel is inset from `area`'s
    /// right edge by `EDGE_MARGIN + DETAILS_LEFT_SHIFT` cells — pulled a
    /// further `DETAILS_LEFT_SHIFT` (1 cell == 2 dots) LEFT off the edge than
    /// before — while its width is unchanged (only `details_x` moves).
    #[test]
    fn details_panel_pulled_left_off_right_edge() {
        for width in [60u16, 90u16] {
            let area = Rect::new(0, 0, width, 30);
            let stamina = RosterManager::stamina_rect(area);
            let ability_list = RosterManager::ability_list_rect(area);
            let expected_x = area.right()
                - (RosterManager::EDGE_MARGIN + RosterManager::DETAILS_LEFT_SHIFT + stamina.width);
            assert_eq!(
                stamina.x, expected_x,
                "width={width}: details panel x must be inset EDGE_MARGIN+DETAILS_LEFT_SHIFT from area.right()"
            );
            assert_eq!(ability_list.x, expected_x, "width={width}: ability_list shares the panel x");
            assert_eq!(
                area.right() - stamina.right(),
                RosterManager::EDGE_MARGIN + RosterManager::DETAILS_LEFT_SHIFT,
                "width={width}: panel right edge must sit EDGE_MARGIN+DETAILS_LEFT_SHIFT in from area.right()"
            );
        }
    }
}

