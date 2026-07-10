use super::*;

impl RosterManager {
    /// The resting name colour — full white (b2-t1).
    const NAME_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);
    /// The scene's effectively-black dark background the name fades toward
    /// mid-slide (b2-t1) — unset cells render as `Color::Reset` ≈ terminal
    /// dark (see engine render lib.rs:58-60 and this file's static-panel
    /// comments).
    const NAME_FADE_BG: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0, 0, 0);

    /// The level text colour — full white (b2-t2), matching `NAME_COLOR`.
    /// Level has no stated transition rule (spec 35's Transition
    /// Choreography never mentions it), so it always renders at this one
    /// colour, keyed off `current_index` immediately.
    const LEVEL_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);

    /// Sprite render-target inset from the left edge of `layout().sprite`.
    /// Asymmetric with `SPRITE_INSET_RIGHT` — the asymmetry IS the horizontal
    /// shift; do not add a separate shift constant. `LEFT < RIGHT` biases the
    /// centered sprite LEFT: the sprite's centre moves `(LEFT - RIGHT)` dots
    /// horizontally, so `LEFT=1`/`RIGHT=3` sits the sprite 2 dots left of its
    /// band centre — a leftward shift of 3 dots from the prior `LEFT=3`/
    /// `RIGHT=2` (which biased 1 dot right).
    const SPRITE_INSET_LEFT: u16 = 1;
    /// Sprite render-target inset from the right edge of `layout().sprite`.
    /// Kept strictly ABOVE `SPRITE_INSET_LEFT` so the left/right asymmetry
    /// shifts the sprite LEFT (see `SPRITE_INSET_LEFT`).
    const SPRITE_INSET_RIGHT: u16 = 3;
    /// Sprite render-target inset from the TOP edge of `layout().sprite`. `0`
    /// so the sprite grows UPWARD into the full band height — its baseline
    /// (bottom edge) is pinned by `SPRITE_INSET_BOTTOM`, so shrinking only the
    /// top inset makes the creature taller without moving its feet.
    const SPRITE_INSET_TOP: u16 = 0;
    /// Sprite render-target inset from the BOTTOM edge of `layout().sprite`.
    /// Guarantees a blank row above `dot_row` — do NOT drop below 1, or the
    /// sprite-gap regression test (a fully-blank row between the sprite's
    /// lowest content and `dot_row`) breaks. This is the sprite's fixed
    /// baseline: growing the sprite (via `SPRITE_INSET_TOP`) never moves it.
    const SPRITE_INSET_BOTTOM: u16 = 1;
    /// Of the one full cell (4 dots) `layout()`'s `left_col` flex frees up
    /// for the sprite band when `stat_bar` moves up one cell (see
    /// `Self::layout`'s `left_col` container), this many dots are held back
    /// as a top margin (via `DotRect::inset`) rather than handed to the
    /// creature's fit box — the remaining `4 - SPRITE_GROWTH_MARGIN_DOTS`
    /// dots are the sprite's actual net growth. A real `flex`-`grow` +
    /// `inset`-margin composition (the freed cell is claimed automatically
    /// by `sprite`'s existing `grow: 1.0`; this constant only tunes how much
    /// of it the sprite visually consumes), not a bespoke placement — see
    /// `render_sprite`.
    const SPRITE_GROWTH_MARGIN_DOTS: u16 = 2;
    /// Renders the creature at `index`'s SPRITE ONLY (b1-t3: name and dot row
    /// are static panels drawn separately, never offset) into a throwaway
    /// zero-origin buffer sized like `area`, then blits every non-space cell
    /// into `buf` shifted by `col_offset` columns — a true screen-space
    /// translation that works with `Rect`'s unsigned `x`.
    pub(super) fn render_sprite(&self, buf: &mut Buffer, area: Rect, index: usize, col_offset: i32) {
        let zero_area = Rect::new(0, 0, area.width, area.height);
        let mut tmp = Buffer::empty(zero_area);
        let base_rect = Self::layout(zero_area).sprite;
        let sprite_rect = Self::cell_rect_to_dots(base_rect).inset(
            Self::SPRITE_INSET_LEFT as i32 * 2,
            Self::SPRITE_INSET_RIGHT as i32 * 2,
            // Of the cell `layout()`'s left_col freed at the top (via
            // STAT_BAR_TOP_LIFT_CELLS + sprite's grow), hold back
            // SPRITE_GROWTH_MARGIN_DOTS as a plain top margin rather than
            // handing all of it to the fit box — the rest is real net growth.
            Self::SPRITE_INSET_TOP as i32 * 4 + Self::SPRITE_GROWTH_MARGIN_DOTS as i32,
            Self::SPRITE_INSET_BOTTOM as i32 * 4,
        );

        let creature = &self.creatures[index];
        if let Some(sprite) = creature.animation(crate::creatures::AnimationKind::Idle) {
            let frame = sprite.frame_at(self.elapsed);
            let aspect = frame.width() as f32 / (frame.height().max(1)) as f32;

            // Aspect-preserving fit within `sprite_rect`, at dot precision —
            // mirrors `fit_dot_dims`'s fit-width-then-fit-height-if-needed
            // shape, computed directly in dots. `sprite_rect`'s own size is
            // ENTIRELY the flex-grown container's (see `Self::layout`'s
            // `left_col`) plus the margin inset above — no separate growth
            // arithmetic here; this just fits within whatever `sprite_rect`
            // already is.
            let mut dot_w = sprite_rect.w;
            let mut dot_h = (dot_w as f32 / aspect).round() as i32;
            if dot_h > sprite_rect.h {
                dot_h = sprite_rect.h;
                dot_w = (dot_h as f32 * aspect).round() as i32;
            }

            if dot_w > 0 && dot_h > 0 {
                // Bottom-aligned within sprite_rect (its own bottom is the
                // sprite's pinned baseline, per SPRITE_INSET_BOTTOM), and
                // horizontally centered. Going through `.to_cell_rect()`
                // here (as the simpler cell-based `fit_dot_dims` path used
                // to) would floor `sprite_rect`'s x/y and w/h independently,
                // and `floor(a) + floor(b) != floor(a+b)` in general — that
                // silently un-pins the baseline by up to a cell whenever
                // sprite_rect isn't itself cell-aligned. Placing in dots
                // throughout, and floor-ing the FINAL target rect exactly
                // once, avoids that.
                let target_x = sprite_rect.x + (sprite_rect.w - dot_w) / 2;
                let target_y = sprite_rect.y + sprite_rect.h - dot_h;
                let target = engine_render::DotRect { x: target_x, y: target_y, w: dot_w, h: dot_h };
                let cell_rect = target.to_cell_rect();
                let (dx, dy) = target.cell_remainder();

                // Dot-precise sub-cell placement: offset the raw dots into a
                // buffer sized to include the sub-cell remainder, then
                // convert the whole thing — the same technique
                // `Button::set_dot_offset_down`'s render already uses.
                // `dots_to_grid`'s ceiling-division fix means the buffer no
                // longer needs to be a clean cell multiple beforehand.
                let content = sprite.dots_at(self.elapsed, dot_w as u32, dot_h as u32);
                let mut placed = DotBuffer::new((dot_w + dx) as usize, (dot_h + dy) as usize);
                for y in 0..dot_h {
                    for x in 0..dot_w {
                        placed.set(
                            (x + dx) as usize,
                            (y + dy) as usize,
                            content.get(x as usize, y as usize),
                        );
                    }
                }
                let grid = dots_to_grid(&placed);
                let draw_area = Rect {
                    x: cell_rect.x,
                    y: cell_rect.y,
                    width: grid.cols() as u16,
                    height: grid.rows() as u16,
                };
                engine_render::draw_grid(&mut tmp, draw_area, &grid);
            }
        }

        for y in 0..area.height {
            for x in 0..area.width {
                let cell = match tmp.cell((x, y)) {
                    Some(c) => c,
                    None => continue,
                };
                if cell.symbol() == " " {
                    continue;
                }
                let dest_x = area.x as i32 + x as i32 + col_offset;
                if dest_x < area.left() as i32 || dest_x >= area.right() as i32 {
                    continue;
                }
                let dest_y = area.y + y;
                if let Some(dest_cell) = buf.cell_mut((dest_x as u16, dest_y)) {
                    *dest_cell = cell.clone();
                }
            }
        }
    }

    /// Which creature's name to draw and at what colour, for the CURRENT
    /// frame (b2-t1). At rest (no active slide), draws `current_index` at
    /// full `NAME_COLOR`. During an active slide, cross-fades: first half
    /// draws the OUTGOING (`prev_index`) name fading from full colour toward
    /// `NAME_FADE_BG`; second half draws the INCOMING (`current_index`) name
    /// fading from `NAME_FADE_BG` back to full colour. Pure over `&self` —
    /// keys off the same `Slide`/`elapsed` window the sprite slide already
    /// uses, no second state machine.
    pub(super) fn name_display(&self) -> (usize, engine_core::color::Rgba) {
        match self.active_slide() {
            None => (self.current_index, Self::NAME_COLOR),
            Some(slide) => {
                let progress = self.elapsed.saturating_sub(slide.start);
                let p = (progress.as_secs_f32() / Self::SLIDE_DUR.as_secs_f32()).clamp(0.0, 1.0);
                if p < 0.5 {
                    (slide.prev_index, Self::NAME_COLOR.lerp(Self::NAME_FADE_BG, 2.0 * p))
                } else {
                    (self.current_index, Self::NAME_FADE_BG.lerp(Self::NAME_COLOR, 2.0 * p - 1.0))
                }
            }
        }
    }

    /// Draws `creatures[index]`'s name statically into `name_rect` at
    /// `color` — no `col_offset`, so it never travels with an in-flight
    /// sprite slide (b1-t3: name updates immediately with `current_index`
    /// regardless of slide state; b2-t1: colour cross-fades via
    /// `name_display`).
    pub(super) fn render_name(&self, buf: &mut Buffer, name_rect: Rect, index: usize, color: engine_core::color::Rgba) {
        let creature = &self.creatures[index];
        crate::braille_name::draw_name(buf, name_rect, creature.name(), color);
    }

    /// Draws `creatures[index]`'s level statically into `level_rect` as
    /// plain text (`"LVL {n}"`) — no `col_offset`, no transition (b2-t2:
    /// level has no stated transition rule, so it updates immediately with
    /// `current_index`, identical to `render_name`/`render_dot_row`).
    pub(super) fn render_level(&self, buf: &mut Buffer, level_rect: Rect, index: usize) {
        let text = format!("LVL {}", self.creatures[index].level());
        engine_render::label(buf, level_rect, &text, Self::LEVEL_COLOR);
    }

}

#[cfg(test)]
mod sprite_and_name_render_tests {
    use super::*;
    use engine_core::scene::EngineCtx;
    use crate::scenes::test_util::render_to_buffer;

    /// A fresh `RosterManager::new()` (current_index == 0) renders the name
    /// at the TOP of the frame (b1-t3 layout inversion from `24`, where the
    /// name sat below the sprite), with the sprite painting non-space cells
    /// inside `layout().sprite` — the band below the name. b2-t1 switches
    /// the name to the braille font (`crate::braille_name`), so this no
    /// longer asserts a literal ASCII substring (braille dots don't contain
    /// readable text) — only that the name rect paints and sits above the
    /// sprite.
    #[test]
    fn renders_index0_name_top_and_sprite_below() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let l = RosterManager::layout(area);

        let name_has_non_space = (l.name.top()..l.name.bottom())
            .any(|y| (l.name.left()..l.name.right()).any(|x| buf.cell((x, y)).unwrap().symbol() != " "));
        assert!(
            name_has_non_space,
            "render must paint the current creature's name (braille dots) somewhere inside the name rect"
        );
        assert!(
            l.name.y < l.sprite.y,
            "name rect (y={}) must be above the sprite rect (y={}) — name sits at the TOP band per b1-t3",
            l.name.y, l.sprite.y
        );

        let sprite_has_non_space = (l.sprite.top()..l.sprite.bottom()).any(|y| {
            (0..w).any(|x| buf.cell((x, y)).unwrap().symbol() != " ")
        });
        assert!(
            sprite_has_non_space,
            "render must paint at least one non-space cell inside the sprite rect"
        );
    }

    /// `update()` accumulating `dt` across multiple ticks past a frame
    /// boundary must change the composited idle frame — the animation
    /// genuinely progresses over `elapsed`, not a static first frame.
    #[test]
    fn idle_frame_advances_with_update() {
        let (w, h) = (40u16, 20u16);
        let mut ctx = EngineCtx;

        let still = RosterManager::new();
        let buf_at_zero = render_to_buffer(&still, w, h);

        let mut advanced = RosterManager::new();
        // Ember Wolf's idle frame_dur is 80ms; tick across multiple dt calls
        // summing well past one frame boundary.
        for _ in 0..5 {
            advanced.update(&mut ctx, Duration::from_millis(20));
        }
        let buf_after = render_to_buffer(&advanced, w, h);

        assert_ne!(
            buf_at_zero, buf_after,
            "composited sprite must change after update() crosses a frame boundary (elapsed advanced by more than one frame_dur)"
        );
    }

    /// Switching `current_index` changes which columns of the name rect are
    /// painted (a different creature's braille name). b2-t1 switches the
    /// name to the braille font, so this compares painted-column sets rather
    /// than an ASCII text substring (braille dots don't contain readable
    /// text).
    #[test]
    fn name_label_tracks_current_index() {
        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let name_rect = RosterManager::layout(area).name;

        fn painted_columns(buf: &ratatui::buffer::Buffer, rect: Rect) -> std::collections::BTreeSet<u16> {
            (rect.left()..rect.right())
                .filter(|&x| {
                    (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
                })
                .collect()
        }

        let mut scene0 = RosterManager::new();
        scene0.current_index = 0;
        let cols0 = painted_columns(&render_to_buffer(&scene0, w, h), name_rect);
        assert!(!cols0.is_empty(), "current_index == 0 must paint the name rect");

        let mut scene1 = RosterManager::new();
        scene1.current_index = 1;
        let cols1 = painted_columns(&render_to_buffer(&scene1, w, h), name_rect);
        assert!(!cols1.is_empty(), "current_index == 1 must paint the name rect");

        assert_ne!(
            cols0, cols1,
            "switching current_index must change which columns of the name rect are painted (different creature name)"
        );
    }

    /// b1-t7: the sprite must be inset within its rect so a real, non-zero
    /// gap of fully-blank cells separates it from `dot_row` — not a flush
    /// edge. Stone Golem (index 2) is tall enough that its sprite content
    /// would otherwise fill the sprite band all the way to `dot_row`.
    #[test]
    fn sprite_has_blank_gap_above_dot_row() {
        let mut scene = RosterManager::new();
        scene.current_index = 2; // Stone Golem
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let l = RosterManager::layout(area);

        // Exclude the rightmost `EDGE_MARGIN + DETAILS_LEFT_SHIFT` columns:
        // b1-t1's layout intentionally shares those columns with the
        // details-panel border (b1-t5), which always paints its bottom-left
        // corner at (details_x, dot_row.top()-1) regardless of any SPRITE_INSET
        // value — and after spec 38 item 4 the panel is pulled a further
        // `DETAILS_LEFT_SHIFT` left into the sprite band. See
        // `sprite_and_ability_list_columns_disjoint` for the tolerated overlap.
        // This test only cares about the sprite's OWN content, not that
        // unrelated, already-correct border cell.
        let sprite_content_right = l
            .sprite
            .right()
            .saturating_sub(RosterManager::EDGE_MARGIN + RosterManager::DETAILS_LEFT_SHIFT);
        let row_is_blank = |y: u16| {
            (l.sprite.left()..sprite_content_right).all(|x| buf.cell((x, y)).unwrap().symbol() == " ")
        };

        let gap_row = l.dot_row.top().saturating_sub(1);
        assert!(
            gap_row >= l.sprite.top(),
            "sprite rect (top={}) must be tall enough to contain a gap row (gap_row={})",
            l.sprite.top(), gap_row
        );
        assert!(
            row_is_blank(gap_row),
            "row directly above dot_row (y={gap_row}) must be fully blank within the sprite's \
             column range — b1-t7 requires a real, non-zero gap between the sprite and dot_row, \
             not a flush edge"
        );
    }

    /// spec 38 corrections (item 3): the sprite grows UPWARD from a fixed
    /// baseline and is shifted LEFT, encoded by the inset asymmetry.
    /// - `SPRITE_INSET_TOP < SPRITE_INSET_BOTTOM`: the top inset is minimal so
    ///   the render target extends up into the band (taller sprite) while the
    ///   bottom inset pins the baseline (fixed feet + the mandatory `dot_row`
    ///   gap). `BOTTOM >= 1` keeps that gap.
    /// - `SPRITE_INSET_LEFT < SPRITE_INSET_RIGHT`: the centred sprite is biased
    ///   left of its band centre (leftward shift).
    #[test]
    #[allow(clippy::assertions_on_constants)] // deliberate compile-time const-invariant lock
    fn sprite_insets_grow_up_and_shift_left() {
        assert!(
            RosterManager::SPRITE_INSET_TOP < RosterManager::SPRITE_INSET_BOTTOM,
            "sprite must grow upward: top inset ({}) must be smaller than the baseline-pinning bottom inset ({})",
            RosterManager::SPRITE_INSET_TOP, RosterManager::SPRITE_INSET_BOTTOM
        );
        assert!(
            RosterManager::SPRITE_INSET_BOTTOM >= 1,
            "bottom inset must stay >= 1 to preserve the blank gap above dot_row"
        );
        assert!(
            RosterManager::SPRITE_INSET_LEFT < RosterManager::SPRITE_INSET_RIGHT,
            "sprite must shift left: left inset ({}) must be smaller than right inset ({})",
            RosterManager::SPRITE_INSET_LEFT, RosterManager::SPRITE_INSET_RIGHT
        );
    }
}

/// b2-t2: plain-text `"LVL {n}"` render below the name, tracking
/// `current_index` immediately (no transition rule stated in spec 35).
#[cfg(test)]
mod level_render_tests {
    use super::*;
    use crate::scenes::test_util::{rect_text, render_to_buffer};

    #[test]
    fn level_text_renders_below_name_for_current_creature() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let l = RosterManager::layout(area);

        let text = rect_text(&buf, l.level);
        assert!(
            text.contains("LVL 5"),
            "level rect must render \"LVL {{n}}\" for the current creature's actual level \
             (Ember Wolf, demo_roster level 5); got {text:?}"
        );
    }

    #[test]
    fn level_text_tracks_current_index() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let l = RosterManager::layout(area);

        let text = rect_text(&buf, l.level);
        assert!(
            text.contains("LVL 6"),
            "level rect must track current_index (Stone Golem, demo_roster level 6); got {text:?}"
        );
        assert!(
            !text.contains("LVL 5"),
            "level rect must not still show the outgoing creature's level; got {text:?}"
        );
    }
}

