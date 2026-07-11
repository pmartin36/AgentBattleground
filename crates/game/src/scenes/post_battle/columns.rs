//! Creature-column geometry + portrait render (b4-t1). `column_layouts` is a
//! PURE function so downstream tasks (b4-t3's level/xp rows, b4-t4's stamina
//! row, b4-t5's glow ring) all consume the exact same rects, never
//! re-deriving them. `blit_dots` is the single sub-cell placement routine
//! (the `sprite_name.rs` dance, extracted) every column element must route
//! through — see CLAUDE.md rule 5 and this task's research.md Verdict.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use engine_core::color::Rgba;
use engine_render::dots::{dots_to_grid, DotBuffer};
use engine_render::tween::Tween;
use engine_render::{draw_grid, Align, Basis, Direction, DotRect, FlexChild, FlexStyle, Justify};

use crate::creatures::XP_TO_NEXT_LEVEL;

use super::glow;
use super::PostBattle;

/// Number of creature columns painted across the creature area.
pub(super) const N_COLUMNS: usize = 4;
/// Dot gap between adjacent columns.
pub(super) const COLUMN_GAP_DOTS: i32 = 2;
/// Dots reserved around every portrait frame for the glow ring (b4-t5).
pub(super) const GLOW_MARGIN_DOTS: i32 = 1;
/// Portrait frame border thickness, in dots.
pub(super) const FRAME_THICKNESS: usize = 1;
/// Portrait frame corner chamfer radius, in dots (single-dot chamfer, matching
/// RosterManager).
pub(super) const FRAME_RADIUS: usize = 1;
/// Height (dots) reserved for the level row (1 cell).
pub(super) const LEVEL_ROW_DOTS: i32 = 4;
/// Height (dots) reserved for the XP row — exactly 1 cell (thin capsule bar).
pub(super) const XP_ROW_DOTS: i32 = 4;
/// Height (dots) reserved for the stamina row — exactly 1 cell (thin capsule
/// bar).
pub(super) const STA_ROW_DOTS: i32 = 4;
/// Dot gap between the portrait region and the stacked text/bar rows. A whole
/// cell (4 dots) so the rows below stay cell-aligned.
pub(super) const PORTRAIT_TEXT_GAP_DOTS: i32 = 4;
/// Numerator/denominator of the portrait's share of the space it could
/// otherwise fill — deliberately compact (≈ 2/3), leaving the rest of the
/// column empty below the bars for progression UI coming later.
pub(super) const PORTRAIT_NUM: i32 = 2;
pub(super) const PORTRAIT_DEN: i32 = 3;

/// One creature column's computed sub-rects, all dot-precise — the shared
/// geometry b4-t3 (level/xp), b4-t4 (stamina), and b4-t5 (glow ring, reuses
/// `frame` verbatim) all consume with no re-derivation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ColumnLayout {
    pub frame: DotRect,
    pub inner: DotRect,
    pub level: DotRect,
    pub xp: DotRect,
    pub stamina: DotRect,
}

/// Pure column geometry: `count` evenly-sized columns across `creature_area`
/// (Row flex, gap `COLUMN_GAP_DOTS`). Each column is split top-to-bottom into a
/// portrait region (≈ `PORTRAIT_NUM`/`PORTRAIT_DEN` of the space it could take)
/// and three stacked 1-cell rows — `level` / `xp` / `stamina` — that are
/// **cell-aligned**: because `creature_area.y` is a whole-cell dot row and the
/// portrait region height is rounded to a whole number of cells, each bar row's
/// dot-Y lands on a 4-dot (cell) boundary, so a 4-dot-tall bar renders inside a
/// single terminal cell rather than smearing across two. The remaining ≈ 1/3 is
/// left empty below the bars. `frame` is the portrait region inset by
/// `GLOW_MARGIN_DOTS` (room for the selection ring); `inner` is `frame` inset by
/// `FRAME_THICKNESS` (the sprite's render target).
pub(super) fn column_layouts(creature_area: DotRect, count: usize) -> Vec<ColumnLayout> {
    let row_style = FlexStyle {
        direction: Direction::Row,
        justify_content: Justify::Start,
        align_items: Align::Stretch,
        gap: COLUMN_GAP_DOTS,
    };
    let row_children: Vec<FlexChild> =
        (0..count).map(|_| FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 }).collect();
    let columns = engine_render::flex(creature_area, row_style, &row_children);

    // The three stacked 1-cell text/bar rows, plus a 1-cell gap between the
    // portrait and them.
    let text_block = LEVEL_ROW_DOTS + XP_ROW_DOTS + STA_ROW_DOTS; // 12 dots = 3 cells
    let gap_dots = PORTRAIT_TEXT_GAP_DOTS; // 1 cell

    columns
        .into_iter()
        .map(|col| {
            // The portrait takes ≈ 2/3 of the space it *could* take (the column
            // height minus the gap and the three 1-cell rows). Round the region
            // to a WHOLE number of cells so the rows below it stay cell-aligned.
            let portrait_could = (col.h - gap_dots - text_block).max(0);
            let frame_region_h = ((portrait_could * PORTRAIT_NUM / PORTRAIT_DEN) / 4) * 4;

            let frame_region = DotRect { x: col.x, y: col.y, w: col.w, h: frame_region_h };
            let frame = frame_region.inset(
                GLOW_MARGIN_DOTS,
                GLOW_MARGIN_DOTS,
                GLOW_MARGIN_DOTS,
                GLOW_MARGIN_DOTS,
            );
            let inner = frame.inset(
                FRAME_THICKNESS as i32,
                FRAME_THICKNESS as i32,
                FRAME_THICKNESS as i32,
                FRAME_THICKNESS as i32,
            );

            // Text/bar rows start at a whole-cell boundary (`col.y` is one, and
            // both `frame_region_h` and `gap_dots` are multiples of 4). They are
            // inset horizontally by the glow margin to align with the frame; the
            // vertical alignment — the thing that keeps a bar in one cell — is
            // exact.
            let rows_x = col.x + GLOW_MARGIN_DOTS;
            let rows_w = (col.w - 2 * GLOW_MARGIN_DOTS).max(0);
            let text_top = col.y + frame_region_h + gap_dots;
            let level = DotRect { x: rows_x, y: text_top, w: rows_w, h: LEVEL_ROW_DOTS };
            let xp =
                DotRect { x: rows_x, y: text_top + LEVEL_ROW_DOTS, w: rows_w, h: XP_ROW_DOTS };
            let stamina = DotRect {
                x: rows_x,
                y: text_top + LEVEL_ROW_DOTS + XP_ROW_DOTS,
                w: rows_w,
                h: STA_ROW_DOTS,
            };

            ColumnLayout { frame, inner, level, xp, stamina }
        })
        .collect()
}

/// Renders each of `scene.creatures`' idle-sprite portraits (chamfered frame
/// via `ui_primitives::rounded_rect` + aspect-fit idle sprite, bottom-pinned/
/// horizontally-centered, per `sprite_name.rs`'s idiom) into `creature_area`,
/// sub-cell precise end to end via `blit_dots`.
pub(super) fn render(scene: &PostBattle, buf: &mut Buffer, creature_area: DotRect) {
    let layouts = column_layouts(creature_area, N_COLUMNS);

    for (i, (layout, creature)) in layouts.iter().zip(scene.creatures.iter()).enumerate() {
        let frame = layout.frame;
        if frame.w <= 0 || frame.h <= 0 {
            continue;
        }
        let frame_buf = engine_render::rounded_rect(
            frame.w as usize,
            frame.h as usize,
            FRAME_THICKNESS,
            FRAME_RADIUS,
            PostBattle::FRAME_COLOR,
            engine_render::dots::Dot::Transparent,
        );
        // The selected column's glow ring (b4-t5) must coexist with the
        // frame border in any braille cell they share — `frame_with_ring`
        // composes both into one buffer, blitted in a single call, rather
        // than two separate full-cell-replacing blits (see glow.rs).
        let (blit_rect, blit_buf) = if i == scene.selected_index {
            glow::frame_with_ring(frame, frame_buf, scene.elapsed)
        } else {
            (frame, frame_buf)
        };
        blit_dots(buf, blit_rect, &blit_buf);

        let inner = layout.inner;
        if inner.w <= 0 || inner.h <= 0 {
            continue;
        }
        if let Some(sprite) = creature.animation(crate::creatures::AnimationKind::Idle) {
            let img = sprite.frame_at(scene.elapsed);
            let aspect = img.width() as f32 / (img.height().max(1)) as f32;

            let mut dot_w = inner.w;
            let mut dot_h = (dot_w as f32 / aspect).round() as i32;
            if dot_h > inner.h {
                dot_h = inner.h;
                dot_w = (dot_h as f32 * aspect).round() as i32;
            }

            if dot_w > 0 && dot_h > 0 {
                // Bottom-pinned, horizontally centered within `inner`.
                let target_x = inner.x + (inner.w - dot_w) / 2;
                let target_y = inner.y + inner.h - dot_h;
                let target = DotRect { x: target_x, y: target_y, w: dot_w, h: dot_h };

                let content = sprite.dots_at(scene.elapsed, dot_w as u32, dot_h as u32);
                blit_dots(buf, target, &content);
            }
        }

        engine_render::label(
            buf,
            layout.level.to_cell_rect(),
            &format!("LVL {}", creature.level()),
            engine_render::TextAlign::Center,
            ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
                PostBattle::LABEL_COLOR.r,
                PostBattle::LABEL_COLOR.g,
                PostBattle::LABEL_COLOR.b,
            )),
        );

        let frac = xp_fill_fraction(creature.xp(), scene.xp_gained[i], scene.elapsed);
        crate::scenes::bars::draw_labeled_bar(
            buf,
            layout.xp,
            "XP",
            PostBattle::LABEL_COLOR,
            frac,
            PostBattle::XP_BAR_COLOR,
        );

        let stamina_percent = creature.stamina().percent();
        crate::scenes::bars::draw_labeled_bar(
            buf,
            layout.stamina,
            "STA",
            PostBattle::LABEL_COLOR,
            stamina_percent as f32 / 100.0,
            stamina_fill_color(stamina_percent),
        );
    }
}

/// The single sub-cell placement routine (the `sprite_name.rs` dance,
/// extracted): pads `dots` by `target.cell_remainder()`, converts via
/// `dots_to_grid`, and blits at `target.to_cell_rect()` sized to the
/// resulting grid's own cols/rows (an exact-size draw area, so `draw_grid`'s
/// centering never shifts it). Every column element (frame, sprite, and
/// b4-t5's glow ring) must route through this — never re-floor a `DotRect`
/// anywhere else in the column path (CLAUDE.md rule 5).
pub(crate) fn blit_dots(buf: &mut Buffer, target: DotRect, dots: &DotBuffer) {
    let (dx, dy) = target.cell_remainder();
    let cell_rect = target.to_cell_rect();

    let mut padded = DotBuffer::new(dots.cols() + dx as usize, dots.rows() + dy as usize);
    for y in 0..dots.rows() {
        for x in 0..dots.cols() {
            padded.set(x + dx as usize, y + dy as usize, dots.get(x, y));
        }
    }

    let grid = dots_to_grid(&padded);
    let draw_area = Rect {
        x: cell_rect.x,
        y: cell_rect.y,
        width: grid.cols() as u16,
        height: grid.rows() as u16,
    };
    draw_grid(buf, draw_area, &grid);
}

/// The XP bar's displayed fill fraction at `elapsed`. The animation:
/// - **holds at `start`** for `PostBattle::XP_ANIM_START_DELAY` of on-screen
///   time (the anim clock is subtracted by the delay, clamped ≥ 0), then
/// - **eases** `start -> start+gained` (`ease_in_out`) over a duration
///   *proportional to the fill amount*: `gained / XP_TO_NEXT_LEVEL *
///   XP_FILL_SECONDS_PER_BAR` — a full bar's worth eases over 5 s, so a larger
///   `gained` reaches `end` later, then
/// - **mod-wraps** by `creatures::XP_TO_NEXT_LEVEL` so a column whose eased
///   value crosses a multiple of the cap rolls the bar over (fills full, wraps
///   to empty, keeps filling) rather than clamping at 1.0.
///
/// `elapsed` is the scene-local clock (`update` clamps `dt` per frame), so a
/// scene-load spike cannot fast-forward past `start`. Sole source of the
/// fraction for both render and tests.
pub(super) fn xp_fill_fraction(start: u32, gained: u32, elapsed: Duration) -> f32 {
    let anim_elapsed = elapsed.saturating_sub(PostBattle::XP_ANIM_START_DELAY);
    let dur_secs = gained as f32 / XP_TO_NEXT_LEVEL as f32
        * PostBattle::XP_FILL_SECONDS_PER_BAR.as_secs_f32();
    let dur = Duration::from_secs_f32(dur_secs.max(0.0));
    let v = Tween::new(start as f32, (start + gained) as f32, dur).at(anim_elapsed);
    v.rem_euclid(XP_TO_NEXT_LEVEL as f32) / XP_TO_NEXT_LEVEL as f32
}

/// (b4-t4) Stamina bar fill color band: `>50` -> `STAMINA_HIGH_COLOR`
/// (green), `15..=50` -> `STAMINA_MID_COLOR` (yellow), `<15` ->
/// `STAMINA_LOW_COLOR` (red). Sole source of the band decision for both
/// render and tests — see research.md.
pub(super) fn stamina_fill_color(percent: u8) -> Rgba {
    if percent > 50 {
        PostBattle::STAMINA_HIGH_COLOR
    } else if percent >= 15 {
        PostBattle::STAMINA_MID_COLOR
    } else {
        PostBattle::STAMINA_LOW_COLOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    /// A fresh scene + a `creature_area` sized to the full render buffer
    /// (whole-cell aligned, so dot<->cell conversion is exact for the test's
    /// own bookkeeping — the column path itself must still work sub-cell,
    /// that's exercised by other tasks/regression tests, not this one).
    fn scene_and_buffer(w_cells: u16, h_cells: u16) -> (PostBattle, Buffer, DotRect) {
        let scene = PostBattle::new();
        let buf = Buffer::empty(Rect::new(0, 0, w_cells, h_cells));
        let creature_area = DotRect { x: 0, y: 0, w: w_cells as i32 * 2, h: h_cells as i32 * 4 };
        (scene, buf, creature_area)
    }

    /// Advance the live scene by `secs` of on-screen time via repeated
    /// `Scene::update` calls (`update` clamps each `dt` to `MAX_UPDATE_DT`, so
    /// a single huge `dt` cannot settle the animation — the loop mirrors real
    /// frames). Used to drive the XP animation to rest in tests.
    fn advance(scene: &mut PostBattle, secs: f32) {
        let step = PostBattle::MAX_UPDATE_DT;
        let mut ctx = engine_core::scene::EngineCtx;
        let n = (secs / step.as_secs_f32()).ceil() as u32;
        for _ in 0..n {
            engine_core::scene::Scene::update(scene, &mut ctx, step);
        }
    }

    /// `column_layouts` must produce exactly `count` layouts, each contained
    /// within `creature_area`, whose `frame` rects are pairwise x-disjoint
    /// with a real gap between adjacent columns — the split is 4 separate
    /// slots, never an overlap.
    #[test]
    fn four_frames_are_horizontally_separated() {
        let creature_area = DotRect { x: 0, y: 0, w: 200, h: 80 };
        let layouts = column_layouts(creature_area, N_COLUMNS);

        assert_eq!(layouts.len(), N_COLUMNS, "must produce exactly N_COLUMNS layouts");
        for (i, l) in layouts.iter().enumerate() {
            assert!(
                l.frame.x >= creature_area.x
                    && l.frame.x + l.frame.w <= creature_area.x + creature_area.w,
                "column {i} frame must lie within creature_area, got {:?}",
                l.frame
            );
        }
        for i in 0..layouts.len() - 1 {
            let a = layouts[i].frame;
            let b = layouts[i + 1].frame;
            assert!(
                a.x + a.w <= b.x,
                "column {i} frame (right={}) must not overlap column {} frame (left={})",
                a.x + a.w,
                i + 1,
                b.x
            );
        }
    }

    /// Rendering `PostBattle::new()`'s 4 demo creatures into `creature_area`
    /// paints at least one lit braille cell inside EACH column's portrait
    /// interior (`inner`) — every column shows a non-empty idle sprite, not a
    /// blank frame.
    #[test]
    fn four_columns_each_render_nonempty_portrait() {
        let (scene, mut buf, creature_area) = scene_and_buffer(80, 40);
        render(&scene, &mut buf, creature_area);
        let layouts = column_layouts(creature_area, N_COLUMNS);

        for (i, l) in layouts.iter().enumerate() {
            let cell_rect = l.inner.to_cell_rect();
            let has_lit = (cell_rect.top()..cell_rect.bottom())
                .flat_map(|y| (cell_rect.left()..cell_rect.right()).map(move |x| (x, y)))
                .any(|(x, y)| engine_render::decode_braille_cell(&buf, x, y).is_some());
            assert!(
                has_lit,
                "column {i} portrait interior must paint at least one lit braille cell"
            );
        }
    }

    /// Each column's `frame` paints a chamfered border: at least one cell
    /// within the frame's footprint is lit at `PostBattle::FRAME_COLOR`,
    /// while the outermost corner dot is chamfered off (no lit dot there) —
    /// proving a rounded frame, not a plain filled box.
    ///
    /// (b4-t5 iter3) Column 0 additionally composites the glow ring directly
    /// against the frame; some of the frame border's own braille cells share
    /// a cell with the ring (the cell's single reported color is then the
    /// mean of both, per `dots_to_grid`'s per-cell averaging), so a single
    /// fixed midpoint-dot probe is not portable across columns. Scan the
    /// whole frame footprint (via `blit_footprint_cell_rect`, not
    /// `to_cell_rect()` — see that helper's doc) for existence of a pure
    /// `FRAME_COLOR` cell instead — mirrors `mod.rs`'s
    /// `frame_perimeter_has_frame_color`, the same contract.
    #[test]
    fn each_column_has_visible_chamfered_frame_border() {
        let (scene, mut buf, creature_area) = scene_and_buffer(80, 40);
        render(&scene, &mut buf, creature_area);
        let layouts = column_layouts(creature_area, N_COLUMNS);

        for (i, l) in layouts.iter().enumerate() {
            let frame = l.frame;

            let r = crate::scenes::test_util::blit_footprint_cell_rect(frame);
            let has_frame_color_cell = (r.top()..r.bottom())
                .flat_map(|y| (r.left()..r.right()).map(move |x| (x, y)))
                .any(|(x, y)| {
                    engine_render::decode_braille_cell(&buf, x, y).map(|(_, c)| c)
                        == Some(PostBattle::FRAME_COLOR)
                });
            assert!(
                has_frame_color_cell,
                "column {i} must show at least one lit FRAME_COLOR cell within its frame"
            );

            let corner_color = crate::scenes::test_util::lit_dot_color(&buf, frame.x, frame.y);
            assert_eq!(
                corner_color, None,
                "column {i} frame's outermost corner dot must be chamfered off (unlit)"
            );
        }
    }

    // ---------------------------------------------------------- b4-t3 ----

    const EPS: f32 = 0.01;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < EPS
    }

    /// At `elapsed = 0` the eased `Tween` sits at `start` exactly (the anim
    /// clock is still inside the start-delay hold), so the fraction is just
    /// `start`'s own position within the current XP-to-next-level band.
    #[test]
    fn xp_fill_fraction_at_zero_elapsed_returns_start_fraction() {
        // frost_lizard-shaped column: start=30, gained=30 (never crosses).
        let frac = xp_fill_fraction(30, 30, Duration::ZERO);
        assert!(approx(frac, 0.30), "at elapsed=0 expected ~0.30, got {frac}");
    }

    /// The fill holds at `start` throughout the `XP_ANIM_START_DELAY` window —
    /// even at the very end of the delay, no easing has begun yet.
    #[test]
    fn xp_fill_fraction_holds_at_start_through_delay() {
        let frac = xp_fill_fraction(30, 30, PostBattle::XP_ANIM_START_DELAY);
        assert!(approx(frac, 0.30), "at the end of the start delay expected ~0.30, got {frac}");
    }

    /// Well past the (delay + scaled duration) the tween rests at
    /// `start + gained`; for a non-crossing column (`start + gained <
    /// XP_TO_NEXT_LEVEL`) the rest fraction is simply `(start+gained)/100`.
    #[test]
    fn xp_fill_fraction_at_rest_returns_end_fraction_non_crossing() {
        let frac = xp_fill_fraction(30, 30, Duration::from_secs(10));
        assert!(approx(frac, 0.60), "at rest expected ~0.60, got {frac}");
    }

    /// For a column whose `start + gained` crosses `XP_TO_NEXT_LEVEL` (90 +
    /// 40 = 130), the rest fraction wraps to `(130 mod 100)/100 = 0.30`, not
    /// clamping at `1.0` — proving rollover, not saturation.
    #[test]
    fn xp_fill_fraction_at_rest_wraps_past_cap() {
        let frac = xp_fill_fraction(90, 40, Duration::from_secs(10));
        assert!(approx(frac, 0.30), "rollover rest fraction expected ~0.30, got {frac}");
    }

    /// Sampling a crossing column (90, 40; delay 0.5s + 2.0s ease) either side
    /// of where its eased value crosses 100 shows near-full then
    /// reset-toward-empty: at 1000ms (before the crossing) the fraction is
    /// high (~0.96, still climbing toward 1.0); at 1500ms (after the crossing)
    /// the fraction is low (~0.10) — the bar filled, wrapped, and kept
    /// filling, rather than clamping at 1.0.
    #[test]
    fn xp_fill_fraction_rollover_shows_near_full_then_wraps_low() {
        let before = xp_fill_fraction(90, 40, Duration::from_millis(1000));
        let after = xp_fill_fraction(90, 40, Duration::from_millis(1500));

        assert!(before > 0.85, "before the crossing expected near-full (>0.85), got {before}");
        assert!(after < 0.35, "after the crossing expected wrapped-low (<0.35), got {after}");
        assert!(
            after < before,
            "the fraction must drop across the crossing (wrap), not keep climbing: before={before} after={after}"
        );
    }

    /// A non-crossing column (`start + gained < 100`) never exceeds `1.0` and
    /// is monotonically non-decreasing across the animation, since it never
    /// wraps.
    #[test]
    fn xp_fill_fraction_non_crossing_is_monotonic_and_bounded() {
        let samples: Vec<f32> = [0, 500, 1000, 1500, 2000, 3000]
            .into_iter()
            .map(|ms| xp_fill_fraction(30, 30, Duration::from_millis(ms)))
            .collect();

        let mut prev = samples[0];
        for &s in &samples[1..] {
            assert!(s <= 1.0, "fraction must never exceed 1.0, got {s}");
            assert!(s + 1e-4 >= prev, "non-crossing fraction must be non-decreasing: {samples:?}");
            prev = s;
        }
    }

    /// Duration scales with fill amount: a larger `gained` reaches its `end`
    /// LATER than a smaller one. col1 (gained 30 → 1.5s ease) and col3 (gained
    /// 25 → 1.25s ease) are both non-crossing; sampled at 0.5s delay + 1.3s
    /// (past col3's ease, inside col1's), col3 has settled to its end fraction
    /// (0.40) while col1 has NOT yet reached its end (0.60).
    #[test]
    fn xp_fill_fraction_duration_scales_with_gained() {
        let t = Duration::from_millis(1800); // 0.5s delay + 1.3s
        let col1 = xp_fill_fraction(30, 30, t); // gained 30, ease 1.5s
        let col3 = xp_fill_fraction(15, 25, t); // gained 25, ease 1.25s
        assert!(approx(col3, 0.40), "col3 (shorter ease) must have settled to 0.40, got {col3}");
        assert!(
            col1 < 0.60 - EPS,
            "col1 (longer ease) must NOT have reached its end 0.60 yet, got {col1}"
        );
    }

    /// Each column paints its level line `"LVL {creature.level()}"` into
    /// `layout.level` — checked for the stone_golem column (index 2, level
    /// 6, per `demo_roster`'s seeded data).
    #[test]
    fn level_line_shows_creature_level_text() {
        let (scene, mut buf, creature_area) = scene_and_buffer(80, 40);
        render(&scene, &mut buf, creature_area);
        let layouts = column_layouts(creature_area, N_COLUMNS);

        let level_rect = layouts[2].level.to_cell_rect();
        let text = crate::scenes::test_util::rect_text(&buf, level_rect);
        assert!(
            text.contains("LVL 6"),
            "column 2 (stone_golem, level 6) must show \"LVL 6\", got {text:?}"
        );
    }

    /// Rendering the level line for a rolled-over column (index 2: xp 90 +
    /// gained 40 crosses `XP_TO_NEXT_LEVEL`) after the XP animation has
    /// finished still shows the SAME level text — the rollover wraps the bar
    /// fraction, it never increments the displayed level number.
    #[test]
    fn xp_rollover_does_not_change_level_text() {
        let (mut scene, mut buf, creature_area) = scene_and_buffer(80, 40);
        advance(&mut scene, 4.0);

        render(&scene, &mut buf, creature_area);
        let layouts = column_layouts(creature_area, N_COLUMNS);

        let level_rect = layouts[2].level.to_cell_rect();
        let text = crate::scenes::test_util::rect_text(&buf, level_rect);
        assert!(
            text.contains("LVL 6"),
            "rollover must not change the displayed level, expected \"LVL 6\", got {text:?}"
        );
    }

    /// At rest (animation settled), the rendered XP bar's proportional
    /// fill (counted as `XP_BAR_COLOR`-lit dots inside `layout.xp`) is longer
    /// for a higher rest fraction: column 1 (frost_lizard: 30+30 -> 0.60)
    /// must show more lit fill dots than column 2 (stone_golem: 90+40 ->
    /// wraps to 0.30).
    #[test]
    fn xp_bar_fill_at_rest_is_proportional_across_columns() {
        let (mut scene, mut buf, creature_area) = scene_and_buffer(80, 40);
        advance(&mut scene, 4.0);

        render(&scene, &mut buf, creature_area);
        let layouts = column_layouts(creature_area, N_COLUMNS);

        let count_fill = |xp_rect: DotRect| -> usize {
            (xp_rect.y..xp_rect.y + xp_rect.h)
                .flat_map(|y| (xp_rect.x..xp_rect.x + xp_rect.w).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    crate::scenes::test_util::lit_dot_color(&buf, x, y)
                        == Some(PostBattle::XP_BAR_COLOR)
                })
                .count()
        };

        let col1_fill = count_fill(layouts[1].xp);
        let col2_fill = count_fill(layouts[2].xp);

        assert!(
            col1_fill > col2_fill,
            "column 1 (rest frac ~0.60) must show more XP fill dots than column 2 (rest frac ~0.30); got col1={col1_fill} col2={col2_fill}"
        );
        assert!(col1_fill > 0, "column 1 must show at least some XP fill at rest");
    }

    // ---------------------------------------------------------- b4-t4 ----

    /// `stamina_fill_color` band thresholds: `>50` -> HIGH (green), `15..=50`
    /// -> MID (yellow), `<15` -> LOW (red). Exercises both edges of each band.
    #[test]
    fn stamina_fill_color_bands() {
        for &(percent, expected) in &[
            (80u8, PostBattle::STAMINA_HIGH_COLOR),
            (60, PostBattle::STAMINA_HIGH_COLOR),
            (51, PostBattle::STAMINA_HIGH_COLOR),
            (50, PostBattle::STAMINA_MID_COLOR),
            (45, PostBattle::STAMINA_MID_COLOR),
            (15, PostBattle::STAMINA_MID_COLOR),
            (14, PostBattle::STAMINA_LOW_COLOR),
            (10, PostBattle::STAMINA_LOW_COLOR),
            (0, PostBattle::STAMINA_LOW_COLOR),
        ] {
            assert_eq!(
                stamina_fill_color(percent),
                expected,
                "percent {percent} must map to {expected:?}"
            );
        }
    }

    /// Rendering `PostBattle::new()`'s 4 demo creatures (seeded stamina
    /// percents 80/45/60/10, per `demo_roster_first_four_span_all_three_stamina_bands`)
    /// paints each column's `layout.stamina` fill in the color matching its
    /// creature's band: column 0 (80%) green/HIGH, column 1 (45%)
    /// yellow/MID, column 3 (10%) red/LOW.
    #[test]
    fn stamina_bar_fill_colors_match_creature_bands() {
        let (scene, mut buf, creature_area) = scene_and_buffer(80, 40);
        render(&scene, &mut buf, creature_area);
        let layouts = column_layouts(creature_area, N_COLUMNS);

        // The bar's white end-caps share `rect` with the fill, so a plain
        // "first lit dot" scan can hit a cap before any fill dot. Instead
        // confirm the fill region contains at least one dot of the expected
        // band color (mirroring the `count_fill` idiom in the sibling
        // proportional-length test below).
        let has_fill_color = |rect: DotRect, expected: Rgba| -> bool {
            (rect.y..rect.y + rect.h)
                .flat_map(|y| (rect.x..rect.x + rect.w).map(move |x| (x, y)))
                .any(|(x, y)| crate::scenes::test_util::lit_dot_color(&buf, x, y) == Some(expected))
        };

        assert!(
            has_fill_color(layouts[0].stamina, PostBattle::STAMINA_HIGH_COLOR),
            "column 0 (80% stamina) must show STAMINA_HIGH_COLOR fill"
        );
        assert!(
            has_fill_color(layouts[1].stamina, PostBattle::STAMINA_MID_COLOR),
            "column 1 (45% stamina) must show STAMINA_MID_COLOR fill"
        );
        assert!(
            has_fill_color(layouts[3].stamina, PostBattle::STAMINA_LOW_COLOR),
            "column 3 (10% stamina) must show STAMINA_LOW_COLOR fill"
        );
    }

    /// The stamina bar's fill length is proportional to the creature's
    /// remaining percent: column 0 (80%) shows more band-colored fill dots
    /// than column 2 (60%), which shows more than column 3 (10%).
    #[test]
    fn stamina_bar_fill_length_is_proportional_across_columns() {
        let (scene, mut buf, creature_area) = scene_and_buffer(80, 40);
        render(&scene, &mut buf, creature_area);
        let layouts = column_layouts(creature_area, N_COLUMNS);

        let count_fill = |rect: DotRect, band_color: Rgba| -> usize {
            (rect.y..rect.y + rect.h)
                .flat_map(|y| (rect.x..rect.x + rect.w).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    crate::scenes::test_util::lit_dot_color(&buf, x, y) == Some(band_color)
                })
                .count()
        };

        let col0_fill = count_fill(layouts[0].stamina, PostBattle::STAMINA_HIGH_COLOR);
        let col2_fill = count_fill(layouts[2].stamina, PostBattle::STAMINA_HIGH_COLOR);
        let col3_fill = count_fill(layouts[3].stamina, PostBattle::STAMINA_LOW_COLOR);

        assert!(
            col0_fill > col2_fill,
            "column 0 (80%) must show more stamina fill dots than column 2 (60%); got col0={col0_fill} col2={col2_fill}"
        );
        assert!(
            col2_fill > col3_fill,
            "column 2 (60%) must show more stamina fill dots than column 3 (10%); got col2={col2_fill} col3={col3_fill}"
        );
        assert!(col3_fill > 0, "column 3 must show at least some stamina fill at 10%");
    }

    /// The stamina bar is static: rendering at `elapsed = 0` and after
    /// advancing the scene by several seconds produces the identical fill color and length —
    /// unlike the XP bar, stamina never animates over time.
    #[test]
    fn stamina_bar_is_static_across_elapsed() {
        let (scene, mut buf_before, creature_area) = scene_and_buffer(80, 40);
        render(&scene, &mut buf_before, creature_area);

        let (mut scene_after, mut buf_after, _) = scene_and_buffer(80, 40);
        advance(&mut scene_after, 4.0);
        render(&scene_after, &mut buf_after, creature_area);

        let layouts = column_layouts(creature_area, N_COLUMNS);
        let count_fill = |buf: &Buffer, rect: DotRect| -> usize {
            (rect.y..rect.y + rect.h)
                .flat_map(|y| (rect.x..rect.x + rect.w).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    crate::scenes::test_util::lit_dot_color(buf, x, y)
                        == Some(PostBattle::STAMINA_HIGH_COLOR)
                })
                .count()
        };

        let before = count_fill(&buf_before, layouts[0].stamina);
        let after = count_fill(&buf_after, layouts[0].stamina);
        assert!(before > 0, "column 0 must show some stamina fill before elapsed advances");
        assert_eq!(before, after, "stamina bar fill must not change with elapsed time");
    }
}
