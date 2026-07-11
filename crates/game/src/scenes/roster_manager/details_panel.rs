use super::*;

impl RosterManager {
    /// Stamina status text colour (b2-t4) — white, matching
    /// `LEVEL_COLOR`.
    const STAMINA_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);
    /// Divisor for converting an `injured_until` remaining `Duration` into
    /// whole days-remaining (b2-t4). Not `stamina::RECOVERY_DURATION`,
    /// which is a recovery span, not a per-day unit.
    const SECS_PER_DAY: u64 = 24 * 60 * 60;

    /// Ability list text colour (b2-t5) — white, matching
    /// `STAMINA_COLOR`/`LEVEL_COLOR` chrome.
    const ABILITY_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);

    /// The stamina status line for `e` (b2-t4): `"Exhausted: {days} days
    /// remain"` when injured (days derived from `injured_until()`), else
    /// `"Stamina: {percent}%"`. Single source of both format strings.
    pub(super) fn stamina_text(e: &crate::stamina::Stamina) -> String {
        match e.injured_until() {
            Some(remaining) => {
                let days = remaining.as_secs().div_ceil(Self::SECS_PER_DAY);
                format!("Exhausted: {days} days remain")
            }
            None => format!("Stamina: {}%", e.percent()),
        }
    }

    /// Dot-rows tall for a header underline (spec Constants, b2-t1).
    pub(super) const HEADER_UNDERLINE_THICKNESS_DOTS: i32 = 2;
    /// Extra dot-width the underline runs past the header text's right edge.
    pub(super) const HEADER_UNDERLINE_PAD_DOTS: i32 = 2;
    /// Underline color — white, matching the header/label text color
    /// (`ABILITY_COLOR`).
    pub(super) const HEADER_UNDERLINE_COLOR: engine_core::color::Rgba =
        engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);

    /// Draws a horizontal lit-dot underline `HEADER_UNDERLINE_THICKNESS_DOTS`
    /// dot-rows tall, in the cell-row directly beneath `header`, spanning
    /// `text`'s rendered width (`text.chars().count() * 2` dots, mirroring
    /// `engine_render::label`'s width measure) plus
    /// `HEADER_UNDERLINE_PAD_DOTS`, clamped to `header.w`. Anchored to
    /// `header.to_cell_rect()` — the same cell-floored position `label` draws
    /// the header text at (CLAUDE.md rule 5) — never a raw sub-cell `header`
    /// field. Empty `text` or a zero clamped width paints nothing. Bespoke
    /// dot-pipeline chrome (spec line 36): routed through the shared
    /// `crate::scenes::post_battle::columns::blit_dots` sub-cell placer, not
    /// a hand-rolled `dots_to_grid`/`draw_grid` call.
    pub(super) fn draw_header_underline(
        buf: &mut ratatui::buffer::Buffer,
        header: engine_render::DotRect,
        text: &str,
    ) {
        if text.is_empty() {
            return;
        }
        // A collapsed header (no vertical room — e.g. a short terminal;
        // `panel_layout.rs`'s `.min(4)` clamp yields a genuine `h == 0` in
        // this case) means the header text itself already painted nothing
        // (`engine_render::label` no-ops on a zero-height cell rect); the
        // underline must match by not drawing either. Without this guard,
        // `target.y = header.y + header.h` collapses to `header.y` itself —
        // whatever row immediately follows the (nonexistent) header band,
        // which can be the details panel's own bottom border row — and the
        // underline paints directly into it (a real bug first exposed once
        // `render_instructions`/`render_abilities` were wired into
        // production `render()`, b3-t3).
        if header.h <= 0 {
            return;
        }
        let text_w = text.chars().count() as i32 * 2;
        let w = (text_w + Self::HEADER_UNDERLINE_PAD_DOTS).min(header.w).max(0);
        if w == 0 {
            return;
        }

        // Dot-precise: anchored directly to `header.x`/`header.y + header.h`,
        // never round-tripped through `header.to_cell_rect()` (CLAUDE.md rule
        // 5) — `blit_dots` floors sub-cell precision correctly on its own.
        let target = engine_render::DotRect {
            x: header.x,
            y: header.y + header.h,
            w,
            h: Self::HEADER_UNDERLINE_THICKNESS_DOTS,
        };
        let mut local = engine_render::dots::DotBuffer::new(
            target.w as usize,
            target.h as usize,
        );
        for row in 0..target.h as usize {
            for col in 0..target.w as usize {
                local.set(col, row, engine_render::dots::Dot::Lit(Self::HEADER_UNDERLINE_COLOR));
            }
        }
        crate::scenes::post_battle::columns::blit_dots(buf, target, &local);
    }

    /// Fixed dot-width of the b2-t2 stamina capsule bar (spec Constants).
    pub(super) const STAMINA_BAR_W_DOTS: i32 = 24;
    /// Dot gap between the b2-t2 stamina label slot and its bar slot.
    pub(super) const STAMINA_LABEL_BAR_GAP_DOTS: i32 = 2;

    /// Paints, into `region`, a centered (`Justify::Center`) "Stamina" label
    /// plus a `bars::draw_bar` capsule for `creatures[index]` (b2-t2). Label
    /// text is `"Stamina"` when rested, else `Self::stamina_text(stamina)`
    /// (the injured "Exhausted: {N} days remain" form). Fill fraction is
    /// `percent/100`; fill color is `stamina_fill_color(percent)`. See
    /// research.md for the flex composition.
    pub(super) fn render_stamina_row(
        &self,
        buf: &mut ratatui::buffer::Buffer,
        region: engine_render::DotRect,
        index: usize,
    ) {
        let stamina = self.creatures[index].stamina();
        let percent = stamina.percent();
        let label = if stamina.is_injured() {
            Self::stamina_text(stamina)
        } else {
            "Stamina".to_string()
        };
        let fraction = percent as f32 / 100.0;
        let fill = crate::scenes::post_battle::columns::stamina_fill_color(percent);

        let label_w = ((label.chars().count() as i32) * 2)
            .min((region.w - Self::STAMINA_BAR_W_DOTS - Self::STAMINA_LABEL_BAR_GAP_DOTS).max(0));

        let style = engine_render::FlexStyle {
            direction: engine_render::Direction::Row,
            justify_content: engine_render::Justify::Center,
            align_items: engine_render::Align::Stretch,
            gap: Self::STAMINA_LABEL_BAR_GAP_DOTS,
        };
        let children = [
            engine_render::FlexChild { basis: engine_render::Basis::Fixed(label_w), grow: 0.0, shrink: 0.0 },
            engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(Self::STAMINA_BAR_W_DOTS),
                grow: 0.0,
                shrink: 0.0,
            },
        ];
        let parts = engine_render::flex(region, style, &children);
        if parts.len() < 2 {
            return;
        }

        engine_render::label(
            buf,
            parts[0].to_cell_rect(),
            &label,
            engine_render::TextAlign::Center,
            ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
                Self::STAMINA_COLOR.r,
                Self::STAMINA_COLOR.g,
                Self::STAMINA_COLOR.b,
            )),
        );
        crate::scenes::bars::draw_bar(buf, parts[1], fraction, fill);
    }

    /// Draws a left-aligned white section header label at `header`'s cell
    /// rect, plus the b2-t1 braille underline beneath it. Shared by the
    /// Abilities (b2-t3) and Instructions (b2-t4) headers so both stay
    /// pixel-identical.
    ///
    pub(super) fn draw_section_header(
        buf: &mut ratatui::buffer::Buffer,
        header: engine_render::DotRect,
        text: &str,
    ) {
        engine_render::label(
            buf,
            header.to_cell_rect(),
            text,
            engine_render::TextAlign::Left,
            ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
                Self::HEADER_UNDERLINE_COLOR.r,
                Self::HEADER_UNDERLINE_COLOR.g,
                Self::HEADER_UNDERLINE_COLOR.b,
            )),
        );
        Self::draw_header_underline(buf, header, text);
    }

    /// Renders the "Abilities" section header (via `draw_section_header`)
    /// then `creatures[index].abilities()` into `cells` in reading order
    /// `[TL, TR, BL, BR]` — each ability's `description()` left-aligned and
    /// terminal-underlined in `ABILITY_COLOR`. A cell with no corresponding
    /// ability (fewer than `MAX_ABILITIES`) paints nothing (blank, no
    /// placeholder/border).
    ///
    pub(super) fn render_abilities(
        &self,
        buf: &mut ratatui::buffer::Buffer,
        header: engine_render::DotRect,
        cells: [engine_render::DotRect; 4],
        index: usize,
    ) {
        Self::draw_section_header(buf, header, "Abilities");

        let abilities = self.creatures[index].abilities();
        for (i, cell) in cells.iter().enumerate() {
            let Some(ability) = abilities.get(i) else {
                continue;
            };
            engine_render::label(
                buf,
                cell.to_cell_rect(),
                ability.description(),
                engine_render::TextAlign::Left,
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Rgb(
                        Self::ABILITY_COLOR.r,
                        Self::ABILITY_COLOR.g,
                        Self::ABILITY_COLOR.b,
                    ))
                    .add_modifier(ratatui::style::Modifier::UNDERLINED),
            );
        }
    }

    /// Renders the "Instructions" section header (via `draw_section_header`)
    /// then the cached `current_instructions` (raw Markdown source) into
    /// `preview`, word-wrapped/clipped/tail-ellipsized via
    /// `engine_render::wrapped_text` (b2-t4).
    pub(super) fn render_instructions(
        &self,
        buf: &mut ratatui::buffer::Buffer,
        header: engine_render::DotRect,
        preview: engine_render::DotRect,
    ) {
        Self::draw_section_header(buf, header, "Instructions");
        engine_render::wrapped_text(
            buf,
            preview.to_cell_rect(),
            &self.current_instructions,
            engine_render::TextAlign::Left,
            ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
                Self::ABILITY_COLOR.r,
                Self::ABILITY_COLOR.g,
                Self::ABILITY_COLOR.b,
            )),
            true, // tail-ellipsis on overflow (spec line 51)
        );
    }
}

/// b3-t3: `render()` integration — the redesigned panel (stamina row,
/// abilities grid, instructions preview) is wired into the REAL
/// `panel_interior_regions` geometry, replacing the retired flat
/// `render_stamina`/`render_ability_list` panel. Re-establishes the
/// suppress-during-slide / incoming-after-settle coverage the pruned
/// `stamina_render_tests`/`ability_list_render_tests` modules used to carry,
/// now against the NEW region rects.
#[cfg(test)]
mod panel_integration_tests {
    use super::*;
    use crate::ability::Ability;
    use crate::creatures::Creature;
    use crate::scenes::test_util::{key_event, rect_text, render_to_buffer};
    use crossterm::event::KeyCode;
    use engine_core::scene::EngineCtx;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp base dir, mirroring `instructions_cache_tests.rs`.
    fn temp_base_dir(tag: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "game-roster-panel-integration-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ))
    }

    fn regions() -> crate::scenes::roster_manager::panel_layout::PanelRegions {
        RosterManager::panel_interior_regions(Rect::new(0, 0, 80, 30))
    }

    /// After a nav slide settles, the panel (rendered through `render()`,
    /// not called directly) shows the INCOMING creature's ability text at
    /// the real `ability_cells` regions and its cached instructions at the
    /// real `preview` region — the OUTGOING creature's data must not still
    /// appear in either.
    #[test]
    fn panel_shows_incoming_creature_after_settle() {
        let base = temp_base_dir("settle");
        crate::instructions::write_instructions_in(&base, "Custom Zero", "zero instructions text")
            .expect("seed write should succeed");
        crate::instructions::write_instructions_in(&base, "Custom One", "one instructions text")
            .expect("seed write should succeed");

        let mut rm = RosterManager::new_with_instructions_base(base.clone());
        // Names kept <=12 chars — the ability cell is 12 cells wide, so a
        // longer name would be silently truncated by `render_abilities`
        // (see `abilities_render_tests`), which would make either assertion
        // below pass/fail for the wrong reason rather than genuinely
        // distinguishing incoming from outgoing.
        rm.creatures[0] =
            Creature::new("Custom Zero").with_abilities(vec![Ability::new("FireZero", vec![])]);
        rm.creatures[1] =
            Creature::new("Custom One").with_abilities(vec![Ability::new("IceOne", vec![])]);

        let mut ctx = EngineCtx;
        rm.handle_input(key_event(KeyCode::Right)); // nav 0 -> 1
        rm.update(&mut ctx, RosterManager::SLIDE_DUR + Duration::from_millis(1)); // past settle

        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let regions = regions();

        let ability_text: String = regions
            .ability_cells
            .iter()
            .map(|c| rect_text(&buf, c.to_cell_rect()))
            .collect();
        assert!(
            ability_text.contains("IceOne"),
            "settled render must show the incoming creature's ability at an ability_cells region; got {ability_text:?}"
        );
        assert!(
            !ability_text.contains("FireZero"),
            "settled render must not still show the outgoing creature's ability; got {ability_text:?}"
        );

        let preview_text = rect_text(&buf, regions.preview.to_cell_rect());
        assert!(
            preview_text.contains("one instructions text"),
            "settled render must show the incoming creature's cached instructions at the preview region; got {preview_text:?}"
        );
        assert!(
            !preview_text.contains("zero instructions text"),
            "settled render must not still show the outgoing creature's instructions; got {preview_text:?}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// During an active slide (at trigger and mid-progress) the panel
    /// interior is entirely suppressed at the new region rects — no
    /// ability text, no instructions preview text.
    #[test]
    fn panel_suppressed_during_slide() {
        let base = temp_base_dir("suppress");
        crate::instructions::write_instructions_in(&base, "Custom Zero", "zero instructions text")
            .expect("seed write should succeed");

        let mut rm = RosterManager::new_with_instructions_base(base.clone());
        // <=12 chars, matching `panel_shows_incoming_creature_after_settle` —
        // a longer name would be truncated by `render_abilities`, making a
        // leaked-text check against the full string pass even if suppression
        // silently broke.
        rm.creatures[0] =
            Creature::new("Custom Zero").with_abilities(vec![Ability::new("FireZero", vec![])]);
        rm.reload_instructions();

        let (w, h) = (80u16, 30u16);
        let regions = regions();
        let has_panel_text = |buf: &ratatui::buffer::Buffer| {
            let ability_text: String = regions
                .ability_cells
                .iter()
                .map(|c| rect_text(buf, c.to_cell_rect()))
                .collect();
            let preview_text = rect_text(buf, regions.preview.to_cell_rect());
            ability_text.contains("FireZero") || preview_text.contains("zero instructions text")
        };

        rm.handle_input(key_event(KeyCode::Right)); // trigger slide, no update() yet
        let trigger_buf = render_to_buffer(&rm, w, h);
        assert!(
            !has_panel_text(&trigger_buf),
            "panel regions must show no ability/instructions text at the instant a slide triggers"
        );

        let mut ctx = EngineCtx;
        rm.update(&mut ctx, Duration::from_millis(75)); // ~25% of SLIDE_DUR
        let mid_buf = render_to_buffer(&rm, w, h);
        assert!(
            !has_panel_text(&mid_buf),
            "panel regions must remain free of ability/instructions text mid-slide"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}

/// b2-t1: bespoke braille header-underline helper. Underline correctness
/// (span, thickness, sub-cell placement, color) must be verified by
/// DECODING the rendered dots (CLAUDE.md rule 5), never by comparing
/// `DotRect`/`Rect` fields.
#[cfg(test)]
mod header_underline_tests {
    use super::*;
    use crate::scenes::test_util::lit_dot_color;
    use engine_render::DotRect;

    /// Whole-cell-aligned header used by every test here: 40 dots (20
    /// cells) wide, 1 cell (4 dots) tall, at the buffer's origin. Buffer is
    /// 20 cells (40 dots) wide x 2 cells (8 dots) tall so both the header's
    /// own row and the reserved underline row beneath it fit.
    fn header() -> DotRect {
        DotRect { x: 0, y: 0, w: 40, h: 4 }
    }

    fn buf() -> ratatui::buffer::Buffer {
        ratatui::buffer::Buffer::empty(Rect::new(0, 0, 20, 2))
    }

    /// "Abilities" is 9 chars -> 18 dots wide; + `HEADER_UNDERLINE_PAD_DOTS`
    /// (2) -> a 20-dot-wide lit run starting at the header's left edge (dot
    /// col 0). A column well past that span is unlit.
    #[test]
    fn underline_spans_header_text_width() {
        let mut b = buf();
        RosterManager::draw_header_underline(&mut b, header(), "Abilities");

        for col in [0, 9, 17, 19] {
            assert_eq!(
                lit_dot_color(&b, col, 4),
                Some(RosterManager::HEADER_UNDERLINE_COLOR),
                "expected dot col {col} at underline row 4 to be lit"
            );
        }
        assert_eq!(
            lit_dot_color(&b, 24, 4),
            None,
            "dot col 24 is well past the text+pad span; must be unlit"
        );
    }

    /// The underline is exactly `HEADER_UNDERLINE_THICKNESS_DOTS` (2) dot
    /// rows tall: rows 4 and 5 lit, rows 6 and 7 unlit, at a column within
    /// the span.
    #[test]
    fn underline_is_two_dots_thick() {
        let mut b = buf();
        RosterManager::draw_header_underline(&mut b, header(), "Abilities");

        assert_eq!(lit_dot_color(&b, 8, 4), Some(RosterManager::HEADER_UNDERLINE_COLOR));
        assert_eq!(lit_dot_color(&b, 8, 5), Some(RosterManager::HEADER_UNDERLINE_COLOR));
        assert_eq!(lit_dot_color(&b, 8, 6), None, "dot row 6 must be unlit — underline is only 2 dots thick");
        assert_eq!(lit_dot_color(&b, 8, 7), None, "dot row 7 must be unlit — underline is only 2 dots thick");
    }

    /// The underline lands in the cell-row directly beneath the header
    /// (dot row 4, the first row of cell row 1) — never inside the
    /// header's own cell row (dot rows 0..4).
    #[test]
    fn underline_sits_in_row_directly_beneath_header() {
        let mut b = buf();
        RosterManager::draw_header_underline(&mut b, header(), "Abilities");

        for row in 0..4 {
            assert_eq!(
                lit_dot_color(&b, 5, row),
                None,
                "header's own cell row (dot row {row}) must carry no underline dots"
            );
        }
        assert_eq!(
            lit_dot_color(&b, 5, 4),
            Some(RosterManager::HEADER_UNDERLINE_COLOR),
            "underline must start at dot row 4, directly beneath the header"
        );
    }

    /// A lit underline dot decodes to the pinned white underline color.
    #[test]
    fn underline_color_is_white() {
        let mut b = buf();
        RosterManager::draw_header_underline(&mut b, header(), "Abilities");

        assert_eq!(lit_dot_color(&b, 10, 4), Some(RosterManager::HEADER_UNDERLINE_COLOR));
    }

    /// Empty text and a degenerate (zero-width) header both no-op cleanly:
    /// no dots painted anywhere, and no panic.
    #[test]
    fn empty_text_and_degenerate_header_draw_without_panic() {
        let mut b = buf();
        RosterManager::draw_header_underline(&mut b, header(), "");
        for col in 0..40 {
            for row in 0..8 {
                assert_eq!(lit_dot_color(&b, col, row), None, "empty text must paint no dots");
            }
        }

        let mut b2 = buf();
        RosterManager::draw_header_underline(&mut b2, DotRect { x: 0, y: 0, w: 0, h: 4 }, "Abilities");
        for col in 0..40 {
            for row in 0..8 {
                assert_eq!(lit_dot_color(&b2, col, row), None, "zero-width header must paint no dots");
            }
        }
    }
}

/// b2-t2: centered "Stamina" label + banded capsule bar. Band/fraction
/// correctness is verified by DECODING dots (CLAUDE.md rule 5), never by
/// comparing `DotRect`/`Rect` fields; label text via `rect_text`.
#[cfg(test)]
mod stamina_row_tests {
    use super::*;
    use crate::creatures::Creature;
    use crate::scenes::bars::CAP_COLOR;
    use crate::scenes::post_battle::PostBattle;
    use crate::scenes::test_util::{lit_dot_color, rect_text};
    use crate::stamina::Stamina;
    use engine_core::color::Rgba;
    use engine_render::DotRect;

    /// Wide enough (90 dots) that even the longest label text ("Exhausted: N
    /// days remain", 50 dots) fits the label slot alongside the fixed
    /// `STAMINA_BAR_W_DOTS` (24) bar + gap (2) without truncation; 1 cell
    /// (4 dots) tall, matching `draw_bar`'s expected target height.
    fn region() -> DotRect {
        DotRect { x: 0, y: 0, w: 90, h: 4 }
    }

    fn render_stamina(percent: u8) -> (ratatui::buffer::Buffer, DotRect) {
        let mut rm = RosterManager::new();
        rm.creatures[0] =
            Creature::new("Test").with_stamina(Stamina::default().drain_from_damage(100 - percent));
        let r = region();
        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 2));
        rm.render_stamina_row(&mut buf, r, 0);
        (buf, r)
    }

    fn count_color(buf: &ratatui::buffer::Buffer, region: DotRect, color: Rgba) -> usize {
        let mut n = 0;
        for row in 0..region.h {
            for col in 0..region.w {
                if lit_dot_color(buf, region.x + col, region.y + row) == Some(color) {
                    n += 1;
                }
            }
        }
        n
    }

    /// Fill color bands the same as post-battle: >50 green, 15..=50 yellow,
    /// <15 red — and ONLY that band's color appears (never a mix).
    #[test]
    fn band_color_tracks_percent() {
        let (buf, region) = render_stamina(60);
        assert!(
            count_color(&buf, region, PostBattle::STAMINA_HIGH_COLOR) > 0,
            "60% must light the green (high) band"
        );
        assert_eq!(count_color(&buf, region, PostBattle::STAMINA_MID_COLOR), 0, "60% must not light the yellow band");
        assert_eq!(count_color(&buf, region, PostBattle::STAMINA_LOW_COLOR), 0, "60% must not light the red band");

        let (buf, region) = render_stamina(30);
        assert!(
            count_color(&buf, region, PostBattle::STAMINA_MID_COLOR) > 0,
            "30% must light the yellow (mid) band"
        );
        assert_eq!(count_color(&buf, region, PostBattle::STAMINA_HIGH_COLOR), 0, "30% must not light the green band");
        assert_eq!(count_color(&buf, region, PostBattle::STAMINA_LOW_COLOR), 0, "30% must not light the red band");

        let (buf, region) = render_stamina(10);
        assert!(
            count_color(&buf, region, PostBattle::STAMINA_LOW_COLOR) > 0,
            "10% must light the red (low) band"
        );
        assert_eq!(count_color(&buf, region, PostBattle::STAMINA_HIGH_COLOR), 0, "10% must not light the green band");
        assert_eq!(count_color(&buf, region, PostBattle::STAMINA_MID_COLOR), 0, "10% must not light the yellow band");
    }

    /// Interior fill length is proportional to `percent`, held within one
    /// color band (both >50, green) so this isolates the fraction, not the
    /// banding.
    #[test]
    fn fill_fraction_tracks_percent() {
        let (buf_low, region) = render_stamina(55);
        let (buf_high, _) = render_stamina(95);
        let n_low = count_color(&buf_low, region, PostBattle::STAMINA_HIGH_COLOR);
        let n_high = count_color(&buf_high, region, PostBattle::STAMINA_HIGH_COLOR);
        assert!(n_low > 0, "55% must light at least one interior fill dot");
        assert!(n_high > n_low, "95% ({n_high}) must light more interior fill dots than 55% ({n_low})");
    }

    /// An injured (0%) creature paints an empty bar (no band-colored interior
    /// dot at all) but keeps the two white end-caps, plus "days remain" text
    /// in place of "Stamina".
    #[test]
    fn injured_renders_empty_bar_and_days_remain() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test").with_stamina(Stamina::default().drain_from_damage(100));
        let region = region();
        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 2));
        rm.render_stamina_row(&mut buf, region, 0);

        for color in [PostBattle::STAMINA_HIGH_COLOR, PostBattle::STAMINA_MID_COLOR, PostBattle::STAMINA_LOW_COLOR] {
            assert_eq!(count_color(&buf, region, color), 0, "an injured creature must paint no interior fill dots");
        }
        assert!(count_color(&buf, region, CAP_COLOR) > 0, "the two white end-caps must still render");

        let text = rect_text(&buf, region.to_cell_rect());
        assert!(text.contains("days remain"), "expected days-remain text; got {text:?}");
    }

    /// A rested creature's label is the bare word "Stamina" — never the
    /// injured "days remain" wording.
    #[test]
    fn rested_label_is_stamina() {
        let (buf, region) = render_stamina(80);
        let text = rect_text(&buf, region.to_cell_rect());
        assert!(text.contains("Stamina"), "expected \"Stamina\" label; got {text:?}");
        assert!(!text.contains("days remain"), "a rested creature must not show days-remain text; got {text:?}");
    }
}

/// b2-t3: Abilities header + 2x2 grid of underlined ability names, rendered
/// into the real `panel_interior_regions` geometry (header + `ability_cells`,
/// reading order `[TL, TR, BL, BR]`). Underline correctness is verified by
/// DECODING dots (CLAUDE.md rule 5); grid placement/blanking/underline style
/// via `rect_text`/`Modifier::UNDERLINED`, never by comparing `DotRect`
/// fields.
#[cfg(test)]
mod abilities_render_tests {
    use super::*;
    use crate::ability::Ability;
    use crate::creatures::Creature;
    use crate::scenes::test_util::{lit_dot_color, rect_text};
    use ratatui::style::Modifier;

    /// The real panel-interior header + 4 ability cells (80x30 area) — the
    /// exact geometry `render_abilities` is contracted to paint into.
    fn regions() -> crate::scenes::roster_manager::panel_layout::PanelRegions {
        RosterManager::panel_interior_regions(Rect::new(0, 0, 80, 30))
    }

    fn render(abilities: Vec<Ability>) -> (Buffer, crate::scenes::roster_manager::panel_layout::PanelRegions) {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test").with_abilities(abilities);
        let regions = regions();
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 30));
        rm.render_abilities(&mut buf, regions.abilities_header, regions.ability_cells, 0);
        (buf, regions)
    }

    /// 4 abilities land at their 4 cells in reading order `[TL, TR, BL, BR]`.
    #[test]
    fn four_abilities_fill_all_cells() {
        let descriptions = ["Fire Breath", "Ice Shard", "Rock Throw", "Wind Gust"];
        let abilities = descriptions.iter().map(|d| Ability::new(*d, vec![])).collect();
        let (buf, regions) = render(abilities);

        for (i, desc) in descriptions.iter().enumerate() {
            let text = rect_text(&buf, regions.ability_cells[i].to_cell_rect());
            assert!(
                text.contains(desc),
                "expected {desc:?} at ability cell {i}; got {text:?}"
            );
        }
    }

    /// A 3-ability creature leaves cell 3 (bottom-right) fully blank — no
    /// placeholder/border.
    #[test]
    fn three_abilities_leave_cell_three_blank() {
        let abilities = vec![
            Ability::new("Fire Breath", vec![]),
            Ability::new("Ice Shard", vec![]),
            Ability::new("Rock Throw", vec![]),
        ];
        let (buf, regions) = render(abilities);

        let text = rect_text(&buf, regions.ability_cells[3].to_cell_rect());
        assert!(
            text.trim().is_empty(),
            "cell 3 must be blank for a 3-ability creature; got {text:?}"
        );
    }

    /// A 2-ability creature leaves the whole bottom row (cells 2 and 3)
    /// blank.
    #[test]
    fn two_abilities_leave_bottom_row_blank() {
        let abilities = vec![Ability::new("Fire Breath", vec![]), Ability::new("Ice Shard", vec![])];
        let (buf, regions) = render(abilities);

        for i in [2, 3] {
            let text = rect_text(&buf, regions.ability_cells[i].to_cell_rect());
            assert!(
                text.trim().is_empty(),
                "cell {i} must be blank for a 2-ability creature; got {text:?}"
            );
        }
    }

    /// Every written ability cell's text carries `Modifier::UNDERLINED`.
    #[test]
    fn ability_text_is_underlined() {
        let abilities = vec![Ability::new("Fire Breath", vec![])];
        let (buf, regions) = render(abilities);

        let cell_rect = regions.ability_cells[0].to_cell_rect();
        let cell = buf.cell((cell_rect.x, cell_rect.y)).expect("ability cell must be in buffer bounds");
        assert!(
            cell.modifier.contains(Modifier::UNDERLINED),
            "expected ability text to carry Modifier::UNDERLINED; got {:?}",
            cell.modifier
        );
    }

    /// The "Abilities" header text renders at the header rect's left edge,
    /// and a lit dot appears in the reserved underline row directly beneath
    /// it (b2-t1 integration) — decoded, not compared via rect fields.
    #[test]
    fn abilities_header_renders_with_braille_underline() {
        let (buf, regions) = render(vec![Ability::new("Fire Breath", vec![])]);

        let header_text = rect_text(&buf, regions.abilities_header.to_cell_rect());
        assert!(
            header_text.contains("Abilities"),
            "expected \"Abilities\" header text; got {header_text:?}"
        );

        let underline_dot_row = regions.abilities_header.y + regions.abilities_header.h;
        let underline_dot_col = regions.abilities_header.x + 4;
        assert!(
            lit_dot_color(&buf, underline_dot_col, underline_dot_row).is_some(),
            "expected a lit underline dot at ({underline_dot_col}, {underline_dot_row}) beneath the Abilities header"
        );
    }
}

/// b2-t4: Instructions header + word-wrapped, tail-ellipsized preview of the
/// cached `current_instructions`. Header/underline correctness is verified
/// by DECODING dots (CLAUDE.md rule 5); preview text via `rect_text`.
#[cfg(test)]
mod instructions_render_tests {
    use super::*;
    use crate::scenes::test_util::{lit_dot_color, rect_text};

    /// The real panel-interior header + preview regions (80x30 area) — the
    /// exact geometry `render_instructions` is contracted to paint into.
    fn regions() -> crate::scenes::roster_manager::panel_layout::PanelRegions {
        RosterManager::panel_interior_regions(Rect::new(0, 0, 80, 30))
    }

    fn render(instructions: &str) -> (Buffer, crate::scenes::roster_manager::panel_layout::PanelRegions) {
        let mut rm = RosterManager::new();
        rm.current_instructions = instructions.to_string();
        let regions = regions();
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 30));
        rm.render_instructions(&mut buf, regions.instructions_header, regions.preview);
        (buf, regions)
    }

    /// The "Instructions" header text renders at the header rect's left
    /// edge, and a lit dot appears in the reserved underline row directly
    /// beneath it (b2-t1 integration) — decoded, not compared via rect
    /// fields.
    #[test]
    fn instructions_header_renders_with_braille_underline() {
        let (buf, regions) = render("hello");

        let header_text = rect_text(&buf, regions.instructions_header.to_cell_rect());
        assert!(
            header_text.contains("Instructions"),
            "expected \"Instructions\" header text; got {header_text:?}"
        );

        let underline_dot_row = regions.instructions_header.y + regions.instructions_header.h;
        let underline_dot_col = regions.instructions_header.x + 4;
        assert!(
            lit_dot_color(&buf, underline_dot_col, underline_dot_row).is_some(),
            "expected a lit underline dot at ({underline_dot_col}, {underline_dot_row}) beneath the Instructions header"
        );
    }

    /// A short, single-word instructions string fits on one row and never
    /// overflows — no trailing `…` anywhere in the preview.
    #[test]
    fn short_instructions_not_ellipsized() {
        let (buf, regions) = render("Attack");

        let text = rect_text(&buf, regions.preview.to_cell_rect());
        assert!(text.contains("Attack"), "expected preview to contain the short text; got {text:?}");
        assert!(!text.contains('…'), "a short, non-overflowing instructions string must not be ellipsized; got {text:?}");
    }

    /// A long instructions string — far more wrapped lines than the preview
    /// has rows — is clipped with a single trailing `…` somewhere in the
    /// preview.
    #[test]
    fn long_instructions_truncated_with_ellipsis() {
        let long = "word ".repeat(2000);
        let (buf, regions) = render(&long);

        let text = rect_text(&buf, regions.preview.to_cell_rect());
        assert!(
            text.contains('…'),
            "an instructions string overflowing the preview must render a trailing ellipsis; got {text:?}"
        );
    }

    /// A string wider than the preview's width word-wraps onto a second row
    /// — the first word lands on the preview's top row, a later word lands
    /// on a lower row.
    #[test]
    fn instructions_word_wrapped_to_preview_width() {
        let wrapped = "AAAAAAAAAA BBBBBBBBBB CCCCCCCCCC DDDDDDDDDD EEEEEEEEEE FFFFFFFFFF GGGGGGGGGG HHHHHHHHHH";
        let (buf, regions) = render(wrapped);
        let preview = regions.preview.to_cell_rect();

        let top_row = rect_text(&buf, Rect::new(preview.x, preview.y, preview.width, 1));
        assert!(
            top_row.contains("AAAAAAAAAA"),
            "expected the first word on the preview's top row; got {top_row:?}"
        );

        let mut found_on_lower_row = false;
        for row in 1..preview.height {
            let line = rect_text(&buf, Rect::new(preview.x, preview.y + row, preview.width, 1));
            if line.contains("HHHHHHHHHH") {
                found_on_lower_row = true;
                break;
            }
        }
        assert!(
            found_on_lower_row,
            "expected a later word to wrap onto a row below the preview's top row"
        );
    }
}

