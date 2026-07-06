use std::cell::RefCell;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::Frame;
use ratatui::layout::Rect;
use engine_render::tween::Tween;
use engine_render::dots::{dots_to_grid, Dot, DotBuffer};
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::scene_id::SceneId;

#[derive(Inspectable)]
pub struct RosterManager {
    current_index: usize,
    #[inspect(hidden)]
    creatures: Vec<crate::creatures::Creature>,
    #[inspect(hidden)]
    elapsed: Duration,
    /// Mouse-driven navigation buttons beside the sprite (b4-t2). `RefCell`
    /// because `render(&self, ..)` must mutate their rect/state from an
    /// immutable receiver (see research.md b4-t2 blueprint point 1).
    #[inspect(hidden)]
    left_button: RefCell<engine_render::Button>,
    #[inspect(hidden)]
    right_button: RefCell<engine_render::Button>,
    /// Top-right button that transitions back to `MainHub` (b4-t3). `RefCell`
    /// for the same immutable-render-mutates-button-state reason as the
    /// arrows.
    #[inspect(hidden)]
    home_button: RefCell<engine_render::Button>,
    /// Transient scene-internal slide transition (b5-t1), armed by
    /// `navigate()` and driven by `elapsed`. `None` when no slide is active.
    #[inspect(hidden)]
    slide: Option<Slide>,
}

/// Transient bookkeeping for an in-flight slide transition: the group that is
/// leaving (`prev_index`), the direction of travel, and the `elapsed` value
/// at which the slide started. Scene-internal only — never a field on
/// `Creature` or any shared type.
#[derive(Clone, Copy, Debug)]
struct Slide {
    prev_index: usize,
    dir: Direction,
    start: Duration,
}

/// The 7 named panel rects `layout()` splits `area` into (b1-t3,
/// research.md). `name`/`sprite`/`dot_row` are the pre-existing bands;
/// `level`/`stat_bar`/`exhaustion`/`ability_list` are new — b2 tasks render
/// into them. Only `sprite` is offset during a slide; every other rect is
/// drawn statically at the resting column regardless of `col_offset`.
#[derive(Clone, Copy, Debug)]
struct RosterLayout {
    name: Rect,
    level: Rect,
    stat_bar: Rect,
    exhaustion: Rect,
    /// Full ability + modifier list for the current creature (b2-t5), sitting
    /// below `exhaustion` and sharing its column.
    ability_list: Rect,
    sprite: Rect,
    dot_row: Rect,
}

impl RosterManager {
    /// Width/height of the left/right arrow buttons flanking the sprite.
    const ARROW_W: u16 = 6;
    const ARROW_H: u16 = 3;

    /// Width/height of the top-right home button.
    const HOME_W: u16 = 6;
    const HOME_H: u16 = 3;

    /// Inset (in whole terminal cells) of the home/arrow buttons from the
    /// edges of `area` they anchor to (spec `Decisions (v1)`).
    const EDGE_MARGIN: u16 = 1;

    /// Duration of the slide transition between roster positions.
    const SLIDE_DUR: Duration = Duration::from_millis(300);

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

    /// v1 fill cap for the stat bars (b2-t3): a stat value >= this cap paints
    /// a full-length bar. Spec 35 explicitly defers the exact cap as an
    /// implementation detail (see research.md); this value keeps every
    /// `demo_roster()` stat (range 8..34) partially-filled with clearly
    /// distinct lengths.
    const STAT_DISPLAY_CAP: u32 = 40;
    /// Lit-dot colour for filled stat-bar segments — distinct chrome from
    /// `NAME_COLOR`/`LEVEL_COLOR` (b2-t3).
    const STAT_BAR_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0x4a, 0xd0, 0x8a);

    /// Exhaustion status text colour (b2-t4) — white, matching
    /// `LEVEL_COLOR`.
    const EXHAUSTION_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);
    /// Divisor for converting an `injured_until` remaining `Duration` into
    /// whole days-remaining (b2-t4). Not `exhaustion::RECOVERY_DURATION`,
    /// which is a recovery span, not a per-day unit.
    const SECS_PER_DAY: u64 = 24 * 60 * 60;

    /// Ability list text colour (b2-t5) — white, matching
    /// `EXHAUSTION_COLOR`/`LEVEL_COLOR` chrome.
    const ABILITY_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);

    /// Height (in rows) of the `name` band at the top of the frame.
    const NAME_H: u16 = 3;
    /// Height of the `level` band directly below `name`.
    const LEVEL_H: u16 = 1;
    /// Height of the `stat_bar`/`exhaustion` panel row below `level`.
    const PANEL_H: u16 = 5;
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
    /// Horizontal gap (in cells) between adjacent role clusters in the dot
    /// row (b2-t6). Chosen so the 3 role labels — each wider than its dot
    /// cluster — do not collide (see `research.md` DATA_FLOW for the exact
    /// column math).
    const CLUSTER_GAP: u16 = 4;
    /// Role-label text colour (b2-t6) — white, matching
    /// `LEVEL_COLOR`/`EXHAUSTION_COLOR`/`ABILITY_COLOR` chrome.
    const DOT_LABEL_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);
    /// The 3 role clusters the dot row is grouped into, in roster-index
    /// order: `(slot count, label)`. Slot counts are derived FROM
    /// `crate::squad_role`'s constants — never a hardcoded 3/1/2 — so a
    /// change to the squad-role split automatically re-groups the dot row
    /// (b2-t6).
    const CLUSTERS: [(usize, &str); 3] = [
        (crate::squad_role::ACTIVE_SLOTS, "Active"),
        (crate::squad_role::BENCH_SLOTS, "Bench"),
        (crate::squad_role::RESERVE_SLOTS, "Reserve"),
    ];

    /// Splits `area` into the 7 named panel rects (b1-t3, research.md
    /// blueprint), top to bottom: `name`, `level`, the `stat_bar`/
    /// `exhaustion` panel row (split left/right), `sprite` (left band below
    /// the stat bars — the only region that still slides), `dot_row`.
    /// `ability_list` sits below `exhaustion`, sharing its column; `sprite`
    /// and `ability_list` are carved into disjoint column ranges so neither
    /// panel paints over the other (b2-t5). Uses saturating arithmetic
    /// throughout so small `area`s degrade to zero-height/width rects
    /// instead of panicking.
    fn layout(area: Rect) -> RosterLayout {
        let name_h = Self::NAME_H.min(area.height);
        let level_h = Self::LEVEL_H.min(area.height.saturating_sub(name_h));
        let panel_h = Self::PANEL_H.min(area.height.saturating_sub(name_h + level_h));
        let dot_h = Self::DOT_H.min(area.height.saturating_sub(name_h + level_h + panel_h));
        let sprite_h = area
            .height
            .saturating_sub(name_h + level_h + panel_h + dot_h);

        let name = Rect::new(area.x, area.y, area.width, name_h);
        let level = Rect::new(area.x, area.y + name_h, area.width, level_h);

        let panel_y = area.y + name_h + level_h;
        let stat_bar_w = area.width / 2;
        let stat_bar = Rect::new(area.x, panel_y, stat_bar_w, panel_h);
        let exhaustion = Rect::new(
            area.x + stat_bar_w,
            panel_y,
            area.width.saturating_sub(stat_bar_w),
            panel_h,
        );

        let sprite_y = panel_y + panel_h;
        let sprite = Rect::new(
            area.x + Self::ARROW_W,
            sprite_y,
            stat_bar_w.saturating_sub(Self::ARROW_W),
            sprite_h,
        );

        let ability_list = Rect::new(
            exhaustion.x,
            exhaustion.y + panel_h,
            exhaustion.width.saturating_sub(Self::ARROW_W),
            sprite_h,
        );

        let dot_row = Rect::new(area.x, sprite_y + sprite_h, area.width, dot_h);

        RosterLayout {
            name,
            level,
            stat_bar,
            exhaustion,
            ability_list,
            sprite,
            dot_row,
        }
    }

    /// Splits `dot_row_rect` (`layout()`'s `dot_row`) into the top `dots_band`
    /// (where dot slots live) and the bottom `label_band` (one row of static
    /// role-label text), per `DOT_LABEL_H` (b2-t6). Saturating — never
    /// panics on a too-short `row`.
    fn dot_bands(row: Rect) -> (Rect, Rect) {
        let label_h = Self::DOT_LABEL_H.min(row.height);
        let dots_h = row.height.saturating_sub(label_h);
        let dots_band = Rect::new(row.x, row.y, row.width, dots_h);
        let label_band = Rect::new(row.x, row.y + dots_h, row.width, label_h);
        (dots_band, label_band)
    }

    /// The 3 role-cluster rects within `dots_band`, in `CLUSTERS` order,
    /// centered as a group with `CLUSTER_GAP` columns between adjacent
    /// clusters (b2-t6). Built from `engine_render`'s
    /// `26-screen-space-positioning` primitives — `anchor` (TopCenter, to
    /// center the whole group) + `stack` (Horizontal, to space the clusters)
    /// — never hand-rolled x-accumulation.
    fn dot_cluster_rects(dots_band: Rect) -> Vec<Rect> {
        let sizes: Vec<(u16, u16)> = Self::CLUSTERS
            .iter()
            .map(|(count, _label)| (*count as u16 * Self::SLOT_W, dots_band.height))
            .collect();
        let group_w = sizes.iter().map(|(w, _)| *w).sum::<u16>()
            + Self::CLUSTER_GAP * (Self::CLUSTERS.len() as u16 - 1);
        let group_rect = engine_render::anchor(
            dots_band,
            (group_w, dots_band.height),
            engine_render::Anchor::TopCenter,
        );
        engine_render::stack(group_rect, &sizes, Self::CLUSTER_GAP, engine_render::StackAxis::Horizontal)
    }

    /// The `squad_role::ROSTER_SIZE` dot slots across `row`, grouped into 3
    /// role clusters (b2-t6, per `CLUSTERS`/`dot_cluster_rects`) — indices
    /// `0..ACTIVE_SLOTS` active, then `BENCH_SLOTS` bench, then
    /// `RESERVE_SLOTS` reserve, flattened in roster-index order. Signature
    /// stays callable exactly as before (`RosterManager::dot_slots(row)`),
    /// so existing callers/tests keep working unchanged.
    fn dot_slots(row: Rect) -> [Rect; crate::squad_role::ROSTER_SIZE] {
        let (dots_band, _label_band) = Self::dot_bands(row);
        let clusters = Self::dot_cluster_rects(dots_band);

        let mut slots = Vec::with_capacity(crate::squad_role::ROSTER_SIZE);
        for (cluster_rect, (count, _label)) in clusters.iter().zip(Self::CLUSTERS.iter()) {
            let slot_w = Self::SLOT_W.min(cluster_rect.width.max(1));
            for i in 0..*count {
                slots.push(Rect::new(
                    cluster_rect.x + i as u16 * slot_w,
                    cluster_rect.y,
                    slot_w,
                    cluster_rect.height,
                ));
            }
        }
        slots.try_into().unwrap_or_else(|v: Vec<Rect>| {
            panic!(
                "dot_slots: expected {} slots, computed {}",
                crate::squad_role::ROSTER_SIZE,
                v.len()
            )
        })
    }

    pub fn new() -> Self {
        Self {
            current_index: 0,
            creatures: crate::creatures::demo_roster(),
            elapsed: Duration::ZERO,
            left_button: RefCell::new(engine_render::Button::new(
                Rect::default(),
                crate::assets::BUTTON_PANEL,
                crate::assets::ICON_ARROW_LEFT,
            )),
            right_button: RefCell::new(engine_render::Button::new(
                Rect::default(),
                crate::assets::BUTTON_PANEL,
                crate::assets::ICON_ARROW_RIGHT,
            )),
            home_button: RefCell::new(engine_render::Button::new(
                Rect::default(),
                crate::assets::BUTTON_PANEL,
                crate::assets::ICON_HOME,
            )),
            slide: None,
        }
    }

    /// Left/right arrow button rects beside the sprite for the current
    /// `area` — the sole place button positioning is computed; `render()`
    /// and tests both call this rather than re-deriving it. Both rects are
    /// vertically centered on the sprite band established by `layout()`.
    fn arrow_rects(area: Rect) -> (Rect, Rect) {
        let sprite_rect = Self::layout(area).sprite;
        let band = Rect::new(area.x, sprite_rect.y, area.width, sprite_rect.height);
        // `layout()` reserves exactly `ARROW_W` columns beside the sprite on
        // each side, flush against it. Insetting a full-`ARROW_W`-wide button
        // from `area`'s edge by `EDGE_MARGIN` would push its far edge past
        // that reservation and into the sprite band, so the button width is
        // shrunk by `EDGE_MARGIN`: its far edge (against the sprite) stays
        // exactly where the reserved space ends, while its near edge (against
        // `area`'s edge) moves inward by the margin.
        let size = (
            Self::ARROW_W.saturating_sub(Self::EDGE_MARGIN),
            Self::ARROW_H,
        );
        let left_rect = engine_render::anchor_with_margin(
            band,
            size,
            engine_render::Anchor::CenterLeft,
            (Self::EDGE_MARGIN, 0),
        );
        let right_rect = engine_render::anchor_with_margin(
            band,
            size,
            engine_render::Anchor::CenterRight,
            (Self::EDGE_MARGIN, 0),
        );
        (left_rect, right_rect)
    }

    /// Top-right rect for the home button — sole place its position is
    /// computed; `render()` and tests both call this.
    fn home_rect(area: Rect) -> Rect {
        engine_render::anchor_with_margin(
            area,
            (Self::HOME_W, Self::HOME_H),
            engine_render::Anchor::TopRight,
            (Self::EDGE_MARGIN, Self::EDGE_MARGIN),
        )
    }

    /// Advances/retreats `current_index` with wraparound. The sole place
    /// carousel index arithmetic lives — mouse (b4-t2) and slide-direction
    /// (b5-t1) paths must call this, never re-derive `(idx±1)%n`. A nav fired
    /// while a slide is already active is ignored until it settles (b5-t1's
    /// SCOPE_QUESTION default).
    fn navigate(&mut self, dir: Direction) {
        if self.active_slide().is_some() {
            return;
        }
        let prev_index = self.current_index;
        let n = self.creatures.len();
        self.current_index = match dir {
            Direction::Right => (self.current_index + 1) % n,
            Direction::Left => (self.current_index + n - 1) % n,
        };
        self.slide = Some(Slide {
            prev_index,
            dir,
            start: self.elapsed,
        });
    }

    /// The currently in-flight slide, if any — `None` once `elapsed` has
    /// reached/exceeded `SLIDE_DUR` past the slide's `start`, regardless of
    /// whether `update()` has run the settle cleanup yet.
    fn active_slide(&self) -> Option<&Slide> {
        self.slide
            .as_ref()
            .filter(|s| self.elapsed.saturating_sub(s.start) < Self::SLIDE_DUR)
    }

    /// Column offsets `(outgoing, incoming)` for an active slide `s` at the
    /// current `elapsed`, eased via `engine_render::tween::Tween`/`ease_in_out`.
    /// A right-nav exits the outgoing group LEFT and enters the incoming
    /// group from the RIGHT; a left-nav is the mirror.
    fn slide_offsets(&self, area: Rect, s: &Slide) -> (i32, i32) {
        let progress = self.elapsed.saturating_sub(s.start);
        let w = area.width as f32;
        let (out_t, in_t) = match s.dir {
            Direction::Right => (
                Tween::new(0.0, -w, Self::SLIDE_DUR).at(progress),
                Tween::new(w, 0.0, Self::SLIDE_DUR).at(progress),
            ),
            Direction::Left => (
                Tween::new(0.0, w, Self::SLIDE_DUR).at(progress),
                Tween::new(-w, 0.0, Self::SLIDE_DUR).at(progress),
            ),
        };
        (out_t.round() as i32, in_t.round() as i32)
    }

    /// Renders the creature at `index`'s SPRITE ONLY (b1-t3: name and dot row
    /// are static panels drawn separately, never offset) into a throwaway
    /// zero-origin buffer sized like `area`, then blits every non-space cell
    /// into `buf` shifted by `col_offset` columns — a true screen-space
    /// translation that works with `Rect`'s unsigned `x`.
    fn render_sprite(&self, buf: &mut Buffer, area: Rect, index: usize, col_offset: i32) {
        let zero_area = Rect::new(0, 0, area.width, area.height);
        let mut tmp = Buffer::empty(zero_area);
        let sprite_rect = Self::layout(zero_area).sprite;

        let creature = &self.creatures[index];
        if let Some(sprite) = creature.animation(crate::creatures::AnimationKind::Idle) {
            let (cols, rows) = engine_render::convert::fit_dot_dims(sprite.frame_at(self.elapsed), sprite_rect);
            if cols > 0 && rows > 0 {
                let buf = sprite.dots_at(self.elapsed, cols * 2, rows * 4);
                let grid = engine_render::dots::dots_to_grid(&buf);
                engine_render::draw_grid(&mut tmp, sprite_rect, &grid);
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
    fn name_display(&self) -> (usize, engine_core::color::Rgba) {
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
    fn render_name(&self, buf: &mut Buffer, name_rect: Rect, index: usize, color: engine_core::color::Rgba) {
        let creature = &self.creatures[index];
        crate::braille_name::draw_name(buf, name_rect, creature.name(), color);
    }

    /// Draws `creatures[index]`'s level statically into `level_rect` as
    /// plain text (`"LVL {n}"`) — no `col_offset`, no transition (b2-t2:
    /// level has no stated transition rule, so it updates immediately with
    /// `current_index`, identical to `render_name`/`render_dot_row`).
    fn render_level(&self, buf: &mut Buffer, level_rect: Rect, index: usize) {
        let text = format!("LVL {}", self.creatures[index].level());
        engine_render::label(buf, level_rect, &text, Self::LEVEL_COLOR);
    }

    /// Fill length (in dot-columns, out of `dot_cols`) for `kind`'s bar, for
    /// the CURRENT frame (b2-t3). At rest (no active slide), the current
    /// creature's stat value scaled against `STAT_DISPLAY_CAP`. During an
    /// active slide, eased-lerps from the outgoing (`prev_index`) value to
    /// the incoming (`current_index`) value via `Tween` — keyed off the SAME
    /// `Slide`/`elapsed` window the sprite slide and name cross-fade already
    /// use, no second transition state machine.
    fn stat_fill_dots(&self, kind: crate::stats::StatKind, dot_cols: usize) -> usize {
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

    /// Draws 4 horizontal braille bars (STR/DEX/INT/VIT, `StatKind::ALL`
    /// order) into `rect` — no `col_offset`, so it never travels with an
    /// in-flight sprite slide (b1-t3: static panel, b2-t3). Non-text chrome,
    /// so it renders through the engine dot pipeline (`DotBuffer`/
    /// `Dot::Lit`/`dots_to_grid`/`draw_grid`), never terminal text or
    /// `engine_render::fill` (CLAUDE.md constraint 4). Each bar's fill length
    /// is proportional to `stat_fill_dots`, growing rightward from a fixed
    /// dot-column-0 origin. The dot-rows are split into `StatKind::ALL.len()`
    /// equal bands with the last dot-row of each band left unlit as an
    /// inter-bar gap, so the 4 bars read as distinct.
    fn render_stat_bars(&self, buf: &mut Buffer, rect: Rect) {
        let dot_cols = rect.width as usize * 2;
        let dot_rows = rect.height as usize * 4;
        if dot_cols == 0 || dot_rows == 0 {
            return;
        }
        let mut dots = DotBuffer::new(dot_cols, dot_rows);

        let kinds = crate::stats::StatKind::ALL;
        let band_h = dot_rows / kinds.len();
        let gap = if band_h > 1 { 1 } else { 0 };
        let filled_rows = band_h.saturating_sub(gap);

        for (i, kind) in kinds.into_iter().enumerate() {
            let fill = self.stat_fill_dots(kind, dot_cols);
            let band_y0 = i * band_h;
            for row in band_y0..band_y0 + filled_rows {
                for col in 0..fill {
                    dots.set(col, row, Dot::Lit(Self::STAT_BAR_COLOR));
                }
            }
        }

        let grid = dots_to_grid(&dots);
        engine_render::draw_grid(buf, rect, &grid);
    }

    /// The exhaustion status line for `e` (b2-t4): `"Exhausted: {days} days
    /// remain"` when injured (days derived from `injured_until()`), else
    /// `"Exhaustion: {percent}%"`. Single source of both format strings.
    fn exhaustion_text(e: &crate::exhaustion::Exhaustion) -> String {
        match e.injured_until() {
            Some(remaining) => {
                let days = remaining.as_secs().div_ceil(Self::SECS_PER_DAY);
                format!("Exhausted: {days} days remain")
            }
            None => format!("Exhaustion: {}%", e.percent()),
        }
    }

    /// Draws `creatures[index]`'s exhaustion status statically into `rect` as
    /// plain text — no `col_offset` (b2-t4: static panel). Text, so it
    /// renders via `engine_render::label`, never the dot pipeline
    /// (CLAUDE.md constraint 4). The caller gates this on `active_slide()`
    /// so the rect is left entirely unpainted during a transition.
    fn render_exhaustion(&self, buf: &mut Buffer, rect: Rect, index: usize) {
        let text = Self::exhaustion_text(self.creatures[index].exhaustion());
        engine_render::label(buf, rect, &text, Self::EXHAUSTION_COLOR);
    }

    /// Builds the flat line list for `creatures[index].abilities()` (b2-t5):
    /// per ability, its description, then (if it has any) its modifier names
    /// joined with ", " on their own line. Single source of the line format.
    fn ability_lines(&self, index: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for ability in self.creatures[index].abilities() {
            lines.push(ability.description().to_string());
            if !ability.modifiers().is_empty() {
                let names = ability
                    .modifiers()
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(names);
            }
        }
        lines
    }

    /// Draws `creatures[index]`'s full ability list statically into `rect` as
    /// plain text — one line per row, top-down, no wrapping/scrolling
    /// (overflow degrades via `label`'s silent truncation; see research.md).
    /// Text, so it renders via `engine_render::label`, never the dot pipeline
    /// (CLAUDE.md constraint 4). The caller gates this on `active_slide()` so
    /// the rect is left entirely unpainted during a transition.
    fn render_ability_list(&self, buf: &mut Buffer, rect: Rect, index: usize) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        for (i, line) in self.ability_lines(index).into_iter().enumerate() {
            let y = rect.y + i as u16;
            if y >= rect.bottom() {
                break;
            }
            engine_render::label(buf, Rect::new(rect.x, y, rect.width, 1), &line, Self::ABILITY_COLOR);
        }
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
    fn render_dot_row(&self, buf: &mut Buffer, dot_row_rect: Rect) {
        for (i, slot) in Self::dot_slots(dot_row_rect).iter().enumerate() {
            let bytes = if i == self.current_index {
                crate::assets::DOT_FILLED
            } else {
                crate::assets::DOT_UNFILLED
            };
            let grid = engine_render::asset_cache::convert(bytes, *slot);
            engine_render::draw_grid(buf, *slot, &grid);
        }

        let (dots_band, label_band) = Self::dot_bands(dot_row_rect);
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
            .intersection(dot_row_rect);
            engine_render::label(buf, label_rect, label, Self::DOT_LABEL_COLOR);
        }
    }
}

/// Shared carousel direction — the sole type b4-t2 (mouse) and b5-t1 (slide)
/// must also consume, never re-derive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Direction {
    Left,
    Right,
}

impl Default for RosterManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for RosterManager {
    fn id(&self) -> SceneKey {
        SceneId::RosterManager.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt;
        if let Some(s) = &self.slide {
            if self.elapsed.saturating_sub(s.start) >= Self::SLIDE_DUR {
                self.slide = None;
            }
        }
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let l = Self::layout(area);

        if let Some(slide) = self.active_slide() {
            let (out_off, in_off) = self.slide_offsets(area, slide);
            self.render_sprite(frame.buffer_mut(), area, slide.prev_index, out_off);
            self.render_sprite(frame.buffer_mut(), area, self.current_index, in_off);
        } else {
            self.render_sprite(frame.buffer_mut(), area, self.current_index, 0);
        }
        // Static panels — no col_offset, so they never travel with an
        // in-flight sprite slide (b1-t3).
        let (name_idx, name_color) = self.name_display();
        self.render_name(frame.buffer_mut(), l.name, name_idx, name_color);
        self.render_level(frame.buffer_mut(), l.level, self.current_index);
        self.render_stat_bars(frame.buffer_mut(), l.stat_bar);
        if self.active_slide().is_none() {
            self.render_exhaustion(frame.buffer_mut(), l.exhaustion, self.current_index);
            self.render_ability_list(frame.buffer_mut(), l.ability_list, self.current_index);
        }
        self.render_dot_row(frame.buffer_mut(), l.dot_row);

        let (left_rect, right_rect) = Self::arrow_rects(area);
        {
            let mut left = self.left_button.borrow_mut();
            left.set_rect(left_rect);
            left.render(frame.buffer_mut());
        }
        {
            let mut right = self.right_button.borrow_mut();
            right.set_rect(right_rect);
            right.render(frame.buffer_mut());
        }

        let home_rect = Self::home_rect(area);
        {
            let mut home = self.home_button.borrow_mut();
            home.set_rect(home_rect);
            home.render(frame.buffer_mut());
        }
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use crossterm::event::KeyCode;

        match ev {
            InputEvent::Key(key) => match key.code {
                KeyCode::Left => self.navigate(Direction::Left),
                KeyCode::Right => self.navigate(Direction::Right),
                _ => {}
            },
            InputEvent::Mouse(me) => {
                let hit_left = self.left_button.get_mut().handle_mouse(&me);
                let hit_right = self.right_button.get_mut().handle_mouse(&me);
                let hit_home = self.home_button.get_mut().handle_mouse(&me);
                if hit_home {
                    return Some(Transition {
                        target: SceneId::MainHub.into(),
                        params: None,
                    });
                }
                if hit_right {
                    self.navigate(Direction::Right);
                }
                if hit_left {
                    self.navigate(Direction::Left);
                }
            }
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_seeds_full_roster_at_index_zero_with_zero_elapsed() {
        let rm = RosterManager::new();
        assert_eq!(
            rm.creatures.len(),
            6,
            "RosterManager::new() must seed all 6 creatures from crate::creatures::demo_roster()"
        );
        assert_eq!(rm.creatures[0].name(), "Ember Wolf");
        assert_eq!(rm.current_index, 0);
        assert_eq!(rm.elapsed, Duration::ZERO);
    }

    /// `RosterManager::new()` must source its roster from
    /// `crate::creatures::demo_roster()`, not `crate::creatures::all()` — the
    /// per-creature RPG fields (stats/level/abilities/exhaustion) must match
    /// `demo_roster()` element-for-element, not `Creature::new`'s defaults
    /// (level 1, `Stats::default()`, empty abilities).
    #[test]
    fn new_sources_rpg_fields_from_demo_roster() {
        let rm = RosterManager::new();
        let demo = crate::creatures::demo_roster();

        for i in [0usize, 2usize] {
            assert_eq!(
                rm.creatures[i].level(),
                demo[i].level(),
                "creature {i} level must match demo_roster()"
            );
            assert_eq!(
                rm.creatures[i].stats(),
                demo[i].stats(),
                "creature {i} stats must match demo_roster()"
            );
            assert_eq!(
                rm.creatures[i].abilities(),
                demo[i].abilities(),
                "creature {i} abilities must match demo_roster()"
            );
            assert_eq!(
                rm.creatures[i].exhaustion(),
                demo[i].exhaustion(),
                "creature {i} exhaustion must match demo_roster()"
            );
        }

        // Guard against a missed swap: `all()`'s defaults would leave Ember
        // Wolf at level 1, which demo_roster() overrides to level 5. This is
        // the assertion that actually fails if `new()` still calls `all()`.
        assert_ne!(
            rm.creatures[0].level(),
            1,
            "RosterManager::new() must use demo_roster(), not all() (which defaults to level 1)"
        );
    }

    #[test]
    fn schema_exposes_only_current_index() {
        let names: Vec<String> = <RosterManager as Inspectable>::schema()
            .children
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(
            names,
            vec!["current_index".to_string()],
            "creatures/elapsed must be #[inspect(hidden)]; only current_index is editable"
        );
    }
}

/// b1-t3: `layout()`'s expanded 7-rect contract — the shared layout every
/// b2 rendering task renders into.
#[cfg(test)]
mod layout_tests {
    use super::*;

    /// `layout(area)` must order its bands top-to-bottom: name < level <
    /// stat_bar/exhaustion row < sprite < dot_row, with `exhaustion` above
    /// `ability_list`, and `stat_bar`/`exhaustion` must not horizontally
    /// overlap (they share the same row, split left/right).
    #[test]
    fn layout_rects_ordered_top_to_bottom() {
        let area = Rect::new(0, 0, 80, 30);
        let l = RosterManager::layout(area);

        assert!(l.name.y < l.level.y, "name.y ({}) must be above level.y ({})", l.name.y, l.level.y);
        assert!(l.level.y < l.stat_bar.y, "level.y ({}) must be above stat_bar.y ({})", l.level.y, l.stat_bar.y);
        assert_eq!(l.stat_bar.y, l.exhaustion.y, "stat_bar and exhaustion must share the same row (y={} vs y={})", l.stat_bar.y, l.exhaustion.y);
        assert!(l.exhaustion.y < l.ability_list.y, "exhaustion.y ({}) must be above ability_list.y ({})", l.exhaustion.y, l.ability_list.y);
        assert!(l.stat_bar.y < l.sprite.y, "stat_bar/exhaustion row ({}) must be above sprite.y ({})", l.stat_bar.y, l.sprite.y);
        assert!(l.sprite.y < l.dot_row.y, "sprite.y ({}) must be above dot_row.y ({})", l.sprite.y, l.dot_row.y);

        assert!(
            l.stat_bar.right() <= l.exhaustion.left() || l.exhaustion.right() <= l.stat_bar.left(),
            "stat_bar ({:?}) and exhaustion ({:?}) must not horizontally overlap",
            l.stat_bar, l.exhaustion
        );
    }

    /// b2-t5 layout fix: the sprite (center/left band) and the ability_list
    /// (right band) must occupy disjoint columns — the sprite's right edge
    /// must not extend past the ability_list's left edge — so ability text
    /// never paints over the centered sprite.
    #[test]
    fn sprite_and_ability_list_columns_disjoint() {
        for width in [80u16, 60u16] {
            let area = Rect::new(0, 0, width, 30);
            let l = RosterManager::layout(area);
            assert!(
                l.sprite.right() <= l.ability_list.left(),
                "width={}: sprite ({:?}) must not extend into ability_list ({:?})",
                width, l.sprite, l.ability_list
            );
        }
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
}

/// b2-t2: plain-text `"LVL {n}"` render below the name, tracking
/// `current_index` immediately (no transition rule stated in spec 35).
#[cfg(test)]
mod level_render_tests {
    use super::*;
    use crate::scenes::test_util::render_to_buffer;

    /// Concatenates every cell's symbol across `rect` into a single `String`
    /// (row by row, no separators) so a plain-text substring assertion can
    /// be made regardless of which row within `rect` the text lands on.
    fn rect_text(buf: &ratatui::buffer::Buffer, rect: Rect) -> String {
        let mut s = String::new();
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                s.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        s
    }

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

/// b2-t3: 4 horizontal stat bars (STR/DEX/INT/VIT) rendered into
/// `layout().stat_bar`, fill length proportional to the stat value (bars
/// only, no numeric text), numeric-lerped in place (no positional slide)
/// during an active slide. Tests isolate one bar at a time via
/// `Creature::with_stats` (only the stat under test non-zero) rather than
/// pre-computing the implementation's internal per-bar row split, so they
/// don't depend on exactly how `render_stat_bars` divides `stat_bar`'s rows.
#[cfg(test)]
mod stat_bar_tests {
    use super::*;
    use crate::creatures::Creature;
    use crate::stats::{StatKind, Stats};
    use crate::scenes::test_util::{has_non_space, key_event, render_to_buffer};
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

    /// A fresh `RosterManager` with `creatures[0]`'s stats replaced by
    /// `stats`, rendered at rest (`current_index == 0`). Returns the
    /// rendered buffer and the `stat_bar` rect for the render's `area`.
    fn render_with_stats(stats: Stats) -> (ratatui::buffer::Buffer, Rect) {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test").with_stats(stats);
        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        (buf, RosterManager::layout(area).stat_bar)
    }

    /// Rows within `rect` that paint at least one non-space cell.
    fn painted_rows(buf: &ratatui::buffer::Buffer, rect: Rect) -> std::collections::BTreeSet<u16> {
        (rect.top()..rect.bottom())
            .filter(|&y| (rect.left()..rect.right()).any(|x| buf.cell((x, y)).unwrap().symbol() != " "))
            .collect()
    }

    /// Rightmost non-space column anywhere within `rect`, across all rows.
    fn rightmost_non_space(buf: &ratatui::buffer::Buffer, rect: Rect) -> Option<u16> {
        (rect.left()..rect.right()).rev().find(|&x| {
            (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
        })
    }

    /// Every cell's (symbol, fg) within `rect`, row-major — an exact-content
    /// snapshot for equality comparisons restricted to one rect (rather than
    /// the whole buffer, since `level` updates immediately with
    /// `current_index` even mid-slide and would otherwise make a whole-buffer
    /// comparison spuriously fail).
    fn region_cells(buf: &ratatui::buffer::Buffer, rect: Rect) -> Vec<(String, ratatui::style::Color)> {
        let mut out = Vec::new();
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                let cell = buf.cell((x, y)).unwrap();
                out.push((cell.symbol().to_string(), cell.fg));
            }
        }
        out
    }

    /// Concatenates every cell's symbol across `rect` into one `String`.
    fn rect_text(buf: &ratatui::buffer::Buffer, rect: Rect) -> String {
        let mut s = String::new();
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                s.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        s
    }

    /// Two creatures whose ONLY non-zero stat differs (Strength vs Vitality,
    /// same magnitude) must paint DISTINCT sets of rows within `stat_bar` —
    /// each of the 4 stats gets its own bar/band, not a shared one.
    #[test]
    fn distinct_stats_paint_distinct_rows() {
        let (buf_str, rect) = render_with_stats(only_stat(StatKind::Strength, 35));
        let rows_str = painted_rows(&buf_str, rect);
        assert!(!rows_str.is_empty(), "a bar for a non-zero Strength value must paint at least one row of stat_bar");

        let (buf_vit, _) = render_with_stats(only_stat(StatKind::Vitality, 35));
        let rows_vit = painted_rows(&buf_vit, rect);
        assert!(!rows_vit.is_empty(), "a bar for a non-zero Vitality value must paint at least one row of stat_bar");

        assert_ne!(
            rows_str, rows_vit,
            "Strength's bar and Vitality's bar must occupy different rows within stat_bar (4 distinct bars, not one shared row)"
        );
    }

    /// A higher stat value paints a strictly longer bar (farther right) than
    /// a lower one, for the same stat.
    #[test]
    fn fill_length_scales_with_stat_value() {
        let (buf_low, rect) = render_with_stats(only_stat(StatKind::Dexterity, 5));
        let low_col = rightmost_non_space(&buf_low, rect);
        assert!(low_col.is_some(), "a non-zero Dexterity value must paint at least one cell in stat_bar");

        let (buf_high, _) = render_with_stats(only_stat(StatKind::Dexterity, 35));
        let high_col = rightmost_non_space(&buf_high, rect);
        assert!(high_col.is_some(), "a higher Dexterity value must also paint at least one cell in stat_bar");

        assert!(
            high_col.unwrap() > low_col.unwrap(),
            "a higher stat value (35) must paint farther right ({high_col:?}) than a lower one (5) ({low_col:?})"
        );
    }

    /// No ASCII digit is ever painted inside `stat_bar` — bars only, no
    /// numeric text.
    #[test]
    fn no_numeric_text_in_stat_bar() {
        let scene = RosterManager::new(); // index 0: Ember Wolf, real demo_roster stats
        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&scene, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).stat_bar;

        assert!(
            has_non_space(&buf, rect),
            "stat_bar must paint the current creature's stat bars"
        );
        let text = rect_text(&buf, rect);
        assert!(
            !text.chars().any(|c| c.is_ascii_digit()),
            "stat_bar must never render a numeric digit (bars only); got {text:?}"
        );
    }

    /// At the instant a slide is triggered (nav fired, `update()` not yet
    /// called), `stat_bar` must render IDENTICALLY (same cells) to a resting
    /// render of the outgoing creature — the bars do not jump/translate;
    /// only the fill length lerps in place as `update()` progresses (at
    /// progress==0, `Tween::at(0) == from == the outgoing value`).
    #[test]
    fn stat_bars_do_not_slide_positionally_at_trigger() {
        let (w, h) = (80u16, 30u16);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).stat_bar;

        let outgoing_rest = RosterManager::new(); // index 0
        let rest_buf = render_to_buffer(&outgoing_rest, w, h);
        assert!(
            has_non_space(&rest_buf, rect),
            "stat_bar must paint the outgoing creature's bars at rest"
        );

        let mut scene = RosterManager::new();
        scene.handle_input(key_event(KeyCode::Right)); // triggers slide 0 -> 1, no update() yet
        let trigger_buf = render_to_buffer(&scene, w, h);

        assert_eq!(
            region_cells(&rest_buf, rect),
            region_cells(&trigger_buf, rect),
            "stat_bar rendering at slide trigger (elapsed==start, no update()) must be identical to a resting render of the outgoing creature"
        );
    }

    /// Mid-slide (partial progress, real differing outgoing/incoming DEX
    /// values sourced from `demo_roster()`), the fill length lies strictly
    /// between the two creatures' resting lengths — the eased numeric lerp,
    /// not a snap or a positional slide.
    #[test]
    fn fill_lerps_between_values_mid_slide() {
        let (w, h) = (80u16, 30u16);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).stat_bar;

        // index 0 (Ember Wolf, DEX 28) -> index 1 (Frost Lizard, DEX 18): a
        // real gap sourced from demo_roster(), not synthetic stats.
        let out_rest = {
            let scene = RosterManager::new();
            rightmost_non_space(&render_to_buffer(&scene, w, h), rect)
                .expect("outgoing (index 0) stat_bar must paint at rest")
        };
        let in_rest = {
            let mut scene = RosterManager::new();
            scene.current_index = 1;
            rightmost_non_space(&render_to_buffer(&scene, w, h), rect)
                .expect("incoming (index 1) stat_bar must paint at rest")
        };
        assert_ne!(
            out_rest, in_rest,
            "test fixture requires index 0 and index 1 to paint different DEX-driven fill lengths"
        );

        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        scene.handle_input(key_event(KeyCode::Right));
        scene.update(&mut ctx, Duration::from_millis(75)); // ~25% of the 300ms SLIDE_DUR
        let mid_col = rightmost_non_space(&render_to_buffer(&scene, w, h), rect)
            .expect("stat_bar must still paint mid-slide");

        let (lo, hi) = if out_rest < in_rest { (out_rest, in_rest) } else { (in_rest, out_rest) };
        assert!(
            mid_col > lo && mid_col < hi,
            "mid-slide fill length ({mid_col}) must lie strictly between the outgoing ({out_rest}) and incoming ({in_rest}) resting lengths"
        );
    }
}

/// b2-t4: exhaustion/injury status text (upper-right), disappearing entirely
/// during an active slide (no cross-fade), reappearing populated with the
/// incoming creature's data once settled.
#[cfg(test)]
mod exhaustion_render_tests {
    use super::*;
    use crate::creatures::Creature;
    use crate::exhaustion::Exhaustion;
    use crate::scenes::test_util::{has_non_space, key_event, render_to_buffer};
    use crossterm::event::KeyCode;
    use engine_core::scene::EngineCtx;

    /// Concatenates every cell's symbol across `rect` into a single `String`.
    fn rect_text(buf: &ratatui::buffer::Buffer, rect: Rect) -> String {
        let mut s = String::new();
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                s.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        s
    }

    /// A rested creature (non-injured, real percent) renders
    /// `"Exhaustion: {N}%"`, never a "days remain" form.
    #[test]
    fn exhaustion_shows_percent_for_rested_creature() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test")
            .with_exhaustion(Exhaustion::default().apply_damage_exhaustion(42));
        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).exhaustion;

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Exhaustion: 42%"),
            "expected \"Exhaustion: 42%\" for a rested creature; got {text:?}"
        );
        assert!(
            !text.contains("days remain"),
            "a rested (non-injured) creature must not show the days-remain form; got {text:?}"
        );
    }

    /// An injured creature (`injured_until` set) renders
    /// `"Exhausted: {N} days remain"` instead of a percent, N derived from
    /// `injured_until()` (24h recovery -> 1 day).
    #[test]
    fn exhaustion_shows_days_remain_for_injured_creature() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test")
            .with_exhaustion(Exhaustion::default().apply_damage_exhaustion(100));
        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).exhaustion;

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Exhausted: 1 days remain"),
            "expected \"Exhausted: 1 days remain\" for an injured creature; got {text:?}"
        );
        assert!(
            !text.contains('%'),
            "an injured creature must not show a percent form; got {text:?}"
        );
    }

    /// During an active slide (at trigger and at partial progress), the
    /// exhaustion rect paints zero non-space cells — the display is entirely
    /// absent, not cross-faded.
    #[test]
    fn exhaustion_hidden_during_slide() {
        let (w, h) = (80u16, 30u16);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).exhaustion;

        let mut scene = RosterManager::new();
        scene.handle_input(key_event(KeyCode::Right)); // trigger slide, no update() yet
        let trigger_buf = render_to_buffer(&scene, w, h);
        assert!(
            !has_non_space(&trigger_buf, rect),
            "exhaustion rect must paint nothing at the instant a slide triggers"
        );

        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(75)); // ~25% of the 300ms SLIDE_DUR
        let mid_buf = render_to_buffer(&scene, w, h);
        assert!(
            !has_non_space(&mid_buf, rect),
            "exhaustion rect must remain empty mid-slide"
        );
    }

    /// Once the slide settles (elapsed past SLIDE_DUR), the exhaustion rect
    /// shows the INCOMING (new current_index) creature's data, not the
    /// outgoing creature's.
    #[test]
    fn exhaustion_shows_incoming_after_settle() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Rested")
            .with_exhaustion(Exhaustion::default().apply_damage_exhaustion(42));
        rm.creatures[1] = Creature::new("Injured")
            .with_exhaustion(Exhaustion::default().apply_damage_exhaustion(100));

        let mut ctx = EngineCtx;
        rm.handle_input(key_event(KeyCode::Right)); // nav 0 -> 1
        rm.update(&mut ctx, Duration::from_millis(350)); // past the 300ms SLIDE_DUR

        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).exhaustion;

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Exhausted: 1 days remain"),
            "settled render must show the incoming (index 1, injured) creature's data; got {text:?}"
        );
        assert!(
            !text.contains("Exhaustion: 42%"),
            "settled render must not still show the outgoing (index 0) creature's data; got {text:?}"
        );
    }
}

/// b2-t5: full ability list (right side, below exhaustion), disappearing
/// entirely during an active slide (no cross-fade), reappearing populated
/// with the incoming creature's abilities once settled.
#[cfg(test)]
mod ability_list_render_tests {
    use super::*;
    use crate::ability::{Ability, Modifier};
    use crate::creatures::Creature;
    use crate::scenes::test_util::{key_event, render_to_buffer};
    use crossterm::event::KeyCode;
    use engine_core::scene::EngineCtx;

    /// Concatenates every cell's symbol across `rect` into a single `String`.
    fn rect_text(buf: &ratatui::buffer::Buffer, rect: Rect) -> String {
        let mut s = String::new();
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                s.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        s
    }

    /// Every ability's description AND every modifier's name must appear
    /// simultaneously inside `ability_list` — full expansion, no progressive
    /// disclosure/pagination.
    #[test]
    fn ability_list_shows_all_abilities_and_modifiers_at_rest() {
        let mut rm = RosterManager::new();
        rm.creatures[0] = Creature::new("Test").with_abilities(vec![
            Ability::new(
                "Fire Breath",
                vec![Modifier { name: "Burning".into(), requires: None }],
            ),
            Ability::new(
                "Ice Shard",
                vec![Modifier { name: "Frozen".into(), requires: None }],
            ),
        ]);
        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).ability_list;

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Fire Breath"),
            "expected \"Fire Breath\" ability description; got {text:?}"
        );
        assert!(
            text.contains("Burning"),
            "expected \"Burning\" modifier name; got {text:?}"
        );
        assert!(
            text.contains("Ice Shard"),
            "expected \"Ice Shard\" ability description; got {text:?}"
        );
        assert!(
            text.contains("Frozen"),
            "expected \"Frozen\" modifier name; got {text:?}"
        );
    }

    /// During an active slide (at trigger and at partial progress), the
    /// ability-list rect shows none of the current creature's ability text —
    /// the display is entirely absent, not cross-faded. Checked via a
    /// distinctive substring rather than whole-rect blankness so the test is
    /// robust regardless of what else may paint in/near the rect.
    #[test]
    fn ability_list_hidden_during_slide() {
        let (w, h) = (80u16, 30u16);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).ability_list;

        let mut scene = RosterManager::new();
        scene.creatures[0] =
            Creature::new("Test").with_abilities(vec![Ability::new("Fire Breath", vec![])]);

        let rest_buf = render_to_buffer(&scene, w, h);
        assert!(
            rect_text(&rest_buf, rect).contains("Fire Breath"),
            "sanity check: ability text must render at rest before triggering a slide"
        );

        scene.handle_input(key_event(KeyCode::Right)); // trigger slide, no update() yet
        let trigger_buf = render_to_buffer(&scene, w, h);
        assert!(
            !rect_text(&trigger_buf, rect).contains("Fire Breath"),
            "ability_list must show no ability text at the instant a slide triggers"
        );

        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(75)); // ~25% of the 300ms SLIDE_DUR
        let mid_buf = render_to_buffer(&scene, w, h);
        assert!(
            !rect_text(&mid_buf, rect).contains("Fire Breath"),
            "ability_list must remain free of ability text mid-slide"
        );
    }

    /// Once the slide settles (elapsed past SLIDE_DUR), the ability-list rect
    /// shows the INCOMING (new current_index) creature's abilities, not the
    /// outgoing creature's.
    #[test]
    fn ability_list_shows_incoming_after_settle() {
        let mut rm = RosterManager::new();
        rm.creatures[0] =
            Creature::new("Outgoing").with_abilities(vec![Ability::new("Outgoing Move", vec![])]);
        rm.creatures[1] =
            Creature::new("Incoming").with_abilities(vec![Ability::new("Incoming Move", vec![])]);

        let mut ctx = EngineCtx;
        rm.handle_input(key_event(KeyCode::Right)); // nav 0 -> 1
        rm.update(&mut ctx, Duration::from_millis(350)); // past the 300ms SLIDE_DUR

        let (w, h) = (80u16, 30u16);
        let buf = render_to_buffer(&rm, w, h);
        let area = Rect::new(0, 0, w, h);
        let rect = RosterManager::layout(area).ability_list;

        let text = rect_text(&buf, rect);
        assert!(
            text.contains("Incoming Move"),
            "settled render must show the incoming (index 1) creature's ability; got {text:?}"
        );
        assert!(
            !text.contains("Outgoing Move"),
            "settled render must not still show the outgoing (index 0) creature's ability; got {text:?}"
        );
    }
}

#[cfg(test)]
mod dot_row_render_tests {
    use super::*;
    use crate::scenes::test_util::render_to_buffer;
    use ratatui::style::Color;

    /// The fg color of the first non-space cell found inside `slot`, or
    /// `None` if the slot has no painted cell.
    fn sample_fg(buf: &ratatui::buffer::Buffer, slot: Rect) -> Option<Color> {
        (slot.top()..slot.bottom())
            .flat_map(|y| (slot.left()..slot.right()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let cell = buf.cell((x, y))?;
                if cell.symbol() != " " {
                    Some(cell.fg)
                } else {
                    None
                }
            })
    }

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
}

/// b2-t6: the 6-slot dot row is re-laid into 3 role clusters (3 active / 1
/// bench / 2 reserve, derived from `crate::squad_role`'s slot constants —
/// never a hardcoded 3/1/2) with a real column gap between clusters and a
/// static plain-text role label under each. The whole band (dots + labels)
/// stays detached from the slide (unchanged mechanic from b1-t3).
#[cfg(test)]
mod dot_row_cluster_tests {
    use super::*;
    use crate::scenes::test_util::render_to_buffer;
    use crate::squad_role::{ACTIVE_SLOTS, BENCH_SLOTS, ROSTER_SIZE};
    use crossterm::event::KeyCode;
    use crate::scenes::test_util::key_event;
    use engine_core::scene::EngineCtx;

    /// Concatenates every cell's symbol across `rect` into a single `String`
    /// (row by row) for a plain-text substring assertion, matching the
    /// `rect_text` pattern used by `level_render_tests` et al.
    fn rect_text(buf: &ratatui::buffer::Buffer, rect: Rect) -> String {
        let mut s = String::new();
        for y in rect.top()..rect.bottom() {
            for x in rect.left()..rect.right() {
                s.push_str(buf.cell((x, y)).unwrap().symbol());
            }
        }
        s
    }

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
}

#[cfg(test)]
mod arrow_key_navigation_tests {
    use super::*;
    use crate::scenes::test_util::key_event;
    use crossterm::event::KeyCode;

    /// Right arrow from the last index (5) wraps to 0.
    #[test]
    fn right_arrow_wraps_from_last_to_zero() {
        let mut scene = RosterManager::new();
        scene.current_index = 5;
        let transition = scene.handle_input(key_event(KeyCode::Right));
        assert_eq!(scene.current_index, 0, "right arrow at index 5 must wrap to 0");
        assert!(transition.is_none(), "arrow keys must not produce a Transition");
    }

    /// Left arrow from index 0 wraps to the last index (5).
    #[test]
    fn left_arrow_wraps_from_zero_to_last() {
        let mut scene = RosterManager::new();
        scene.current_index = 0;
        let transition = scene.handle_input(key_event(KeyCode::Left));
        assert_eq!(scene.current_index, 5, "left arrow at index 0 must wrap to 5");
        assert!(transition.is_none(), "arrow keys must not produce a Transition");
    }

    /// Right arrow from a middle index advances by 1 (non-wrap case).
    #[test]
    fn right_arrow_advances_without_wrap() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let transition = scene.handle_input(key_event(KeyCode::Right));
        assert_eq!(scene.current_index, 3, "right arrow at index 2 must advance to 3");
        assert!(transition.is_none(), "arrow keys must not produce a Transition");
    }

    /// A key unrelated to navigation leaves `current_index` unchanged.
    #[test]
    fn unrelated_key_leaves_index_unchanged() {
        let mut scene = RosterManager::new();
        scene.current_index = 2;
        let transition = scene.handle_input(key_event(KeyCode::Char('q')));
        assert_eq!(scene.current_index, 2, "unrelated key must not change current_index");
        assert!(transition.is_none(), "unrelated key must not produce a Transition");
    }
}

#[cfg(test)]
mod arrow_button_tests {
    use super::*;
    use crate::scenes::test_util::{has_non_space, mouse_event, render_to_buffer};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    /// Leftmost column (if any) within `rect` painted non-space.
    fn leftmost_non_space(buf: &ratatui::buffer::Buffer, rect: Rect) -> Option<u16> {
        (rect.left()..rect.right()).find(|&x| {
            (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
        })
    }

    /// Rightmost column (if any) within `rect` painted non-space.
    fn rightmost_non_space(buf: &ratatui::buffer::Buffer, rect: Rect) -> Option<u16> {
        (rect.left()..rect.right()).rev().find(|&x| {
            (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
        })
    }

    /// `render()` paints the left arrow button strictly to the left of the
    /// sprite's leftmost occupied column.
    #[test]
    fn left_button_renders_beside_sprite() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (left_rect, _) = RosterManager::arrow_rects(area);
        let sprite_rect = RosterManager::layout(area).sprite;

        assert!(
            has_non_space(&buf, left_rect),
            "left arrow button must paint at least one non-space cell within its rect"
        );
        let sprite_left = leftmost_non_space(&buf, sprite_rect)
            .expect("sprite must paint at least one non-space cell");
        let button_right = rightmost_non_space(&buf, left_rect)
            .expect("left button must paint at least one non-space cell");
        assert!(
            button_right < sprite_left,
            "left arrow button's rightmost painted column ({button_right}) must be strictly left of the sprite's leftmost painted column ({sprite_left})"
        );
        assert_eq!(
            left_rect.x,
            area.x + RosterManager::EDGE_MARGIN,
            "left arrow button rect must be inset from area's left edge by EDGE_MARGIN"
        );
    }

    /// `render()` paints the right arrow button strictly to the right of the
    /// sprite's rightmost occupied column.
    #[test]
    fn right_button_renders_beside_sprite() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (_, right_rect) = RosterManager::arrow_rects(area);
        let sprite_rect = RosterManager::layout(area).sprite;

        assert!(
            has_non_space(&buf, right_rect),
            "right arrow button must paint at least one non-space cell within its rect"
        );
        let sprite_right = rightmost_non_space(&buf, sprite_rect)
            .expect("sprite must paint at least one non-space cell");
        let button_left = leftmost_non_space(&buf, right_rect)
            .expect("right button must paint at least one non-space cell");
        assert!(
            button_left > sprite_right,
            "right arrow button's leftmost painted column ({button_left}) must be strictly right of the sprite's rightmost painted column ({sprite_right})"
        );
        assert_eq!(
            right_rect.right(),
            area.right() - RosterManager::EDGE_MARGIN,
            "right arrow button rect must be inset from area's right edge by EDGE_MARGIN"
        );
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
    /// `area` — inset from the top edge and the right edge by
    /// `RosterManager::EDGE_MARGIN` cells (no longer flush) — distinct from
    /// the arrow buttons' beside-center position and the dot row's bottom
    /// position.
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
            area.top() + RosterManager::EDGE_MARGIN,
            "home button rect must be inset from the top edge of area by EDGE_MARGIN"
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

/// Slide transition (b5-t1): navigating slides the outgoing creature's
/// group off-screen in the direction of travel while the incoming
/// creature's group slides in from the opposite edge, eased via
/// `engine_render::tween`. Timings below assume the blueprint's documented
/// `SLIDE_DUR = 300ms` (research.md b5-t1): 75ms/225ms/425ms total elapsed
/// land at ~25%/~75%/past-100% progress.
#[cfg(test)]
mod slide_transition_tests {
    use super::*;
    use engine_core::scene::EngineCtx;
    use crate::scenes::test_util::{key_event, render_to_buffer};
    use crossterm::event::KeyCode;
    use ratatui::buffer::Buffer;

    /// Leftmost non-space column across every row of `rect`, if any. Used to
    /// track the SPRITE region's slide position (b1-t3: only the sprite is
    /// offset during a slide; name/dot-row are static and no longer a valid
    /// signal for slide progress).
    fn leftmost_non_space_in_rect(buf: &Buffer, rect: Rect) -> Option<u16> {
        (rect.left()..rect.right()).find(|&x| {
            (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
        })
    }

    /// A right-nav slides the outgoing creature's SPRITE out to the left and
    /// the incoming creature's SPRITE in from the right, eased over time, and
    /// settles with only the incoming creature's sprite painted at its
    /// resting column. Per b1-t3, name/dot-row no longer travel with the
    /// slide (they update immediately with `current_index`, unchanged
    /// columns throughout) — only the sprite region is exercised here.
    #[test]
    fn right_nav_slide_animates_and_settles() {
        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let sprite_rect = RosterManager::layout(area).sprite;

        // Resting (no-slide) column of each creature's sprite, rendered
        // standalone.
        let out_rest_left = {
            let baseline = RosterManager::new(); // index 0: Ember Wolf
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_rect(&buf, sprite_rect)
                .expect("Ember Wolf's sprite must paint at rest")
        };
        let in_rest_left = {
            let mut baseline = RosterManager::new();
            baseline.current_index = 1; // Frost Lizard
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_rect(&buf, sprite_rect)
                .expect("Frost Lizard's sprite must paint at rest")
        };

        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        let t = scene.handle_input(key_event(KeyCode::Right));
        assert!(t.is_none(), "arrow keys must not produce a Transition");
        assert_eq!(
            scene.current_index, 1,
            "current_index must update immediately on nav (b4 contract), even though a slide starts"
        );

        // Instant of trigger (no update yet): outgoing sprite still at rest.
        let buf0 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_rect(&buf0, sprite_rect),
            Some(out_rest_left),
            "immediately after nav, the outgoing creature's sprite (Ember Wolf) must still be painted at its resting column"
        );

        // ~25% progress: outgoing sprite has slid measurably left of rest.
        scene.update(&mut ctx, Duration::from_millis(75));
        let buf1 = render_to_buffer(&scene, w, h);
        let out_mid_left = leftmost_non_space_in_rect(&buf1, sprite_rect)
            .expect("outgoing sprite must still be partially on-screen at ~25% progress");
        assert!(
            out_mid_left < out_rest_left,
            "outgoing sprite's painted column ({out_mid_left}) must have moved left of its resting column ({out_rest_left})"
        );

        // ~75% progress: incoming sprite has slid in from the right, not yet settled.
        scene.update(&mut ctx, Duration::from_millis(150)); // total elapsed 225ms
        let buf2 = render_to_buffer(&scene, w, h);
        let in_mid_left = leftmost_non_space_in_rect(&buf2, sprite_rect)
            .expect("incoming sprite must be partially visible at ~75% progress");
        assert!(
            in_mid_left > in_rest_left,
            "incoming sprite's painted column ({in_mid_left}) must still be offset right of its resting column ({in_rest_left}) mid-transition"
        );

        // Past the slide duration: only the incoming sprite remains, at rest.
        scene.update(&mut ctx, Duration::from_millis(200)); // total elapsed 425ms
        let buf3 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_rect(&buf3, sprite_rect),
            Some(in_rest_left),
            "once settled, the incoming sprite must render at its exact resting column"
        );
    }

    /// Mirror of the right-nav case: a left-nav slides the outgoing
    /// creature's sprite out to the right and the incoming creature's sprite
    /// in from the left.
    #[test]
    fn left_nav_slide_animates_and_settles() {
        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let sprite_rect = RosterManager::layout(area).sprite;

        let out_rest_left = {
            let baseline = RosterManager::new(); // index 0: Ember Wolf
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_rect(&buf, sprite_rect)
                .expect("Ember Wolf's sprite must paint at rest")
        };
        let in_rest_left = {
            let mut baseline = RosterManager::new();
            baseline.current_index = 5; // Shadow Cat (left-wrap from 0)
            let buf = render_to_buffer(&baseline, w, h);
            leftmost_non_space_in_rect(&buf, sprite_rect)
                .expect("Shadow Cat's sprite must paint at rest")
        };

        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        let t = scene.handle_input(key_event(KeyCode::Left));
        assert!(t.is_none(), "arrow keys must not produce a Transition");
        assert_eq!(
            scene.current_index, 5,
            "left nav from index 0 must wrap current_index to 5 immediately, even though a slide starts"
        );

        let buf0 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_rect(&buf0, sprite_rect),
            Some(out_rest_left),
            "immediately after nav, the outgoing creature's sprite (Ember Wolf) must still be painted at its resting column"
        );

        scene.update(&mut ctx, Duration::from_millis(75));
        let buf1 = render_to_buffer(&scene, w, h);
        let out_mid_left = leftmost_non_space_in_rect(&buf1, sprite_rect)
            .expect("outgoing sprite must still be partially on-screen at ~25% progress");
        assert!(
            out_mid_left > out_rest_left,
            "outgoing sprite's painted column ({out_mid_left}) must have moved right of its resting column ({out_rest_left}) for a left-nav exit"
        );

        scene.update(&mut ctx, Duration::from_millis(150)); // total elapsed 225ms
        let buf2 = render_to_buffer(&scene, w, h);
        let in_mid_left = leftmost_non_space_in_rect(&buf2, sprite_rect)
            .expect("incoming sprite must be partially visible at ~75% progress");
        assert!(
            in_mid_left < in_rest_left,
            "incoming sprite's painted column ({in_mid_left}) must still be offset left of its resting column ({in_rest_left}) mid-transition"
        );

        scene.update(&mut ctx, Duration::from_millis(200)); // total elapsed 425ms
        let buf3 = render_to_buffer(&scene, w, h);
        assert_eq!(
            leftmost_non_space_in_rect(&buf3, sprite_rect),
            Some(in_rest_left),
            "once settled, the incoming sprite must render at its exact resting column"
        );
    }

    /// Per b1-t3/b2-t1: during an active slide, at the SAME `current_index`,
    /// the name rect and dot-row rect paint identical COLUMNS whether or not
    /// a slide is active — only the sprite region's painted columns differ
    /// (still slides). This is the shared layout contract every b2 rendering
    /// task depends on. The name's fg COLOUR, however, DOES change during an
    /// active slide (b2-t1): it cross-fades toward the background and back,
    /// keyed off the same `Slide`/`elapsed` window (colour-only, position
    /// never moves). Sampled at 200ms (~67% progress) — unambiguously past
    /// the 150ms/50% prev/current cross-fade boundary, so `current_index`'s
    /// name is definitely the one shown, still mid-fade-in.
    #[test]
    fn name_and_dot_row_do_not_slide_but_sprite_does() {
        fn painted_columns(buf: &Buffer, rect: Rect) -> std::collections::BTreeSet<u16> {
            (rect.left()..rect.right())
                .filter(|&x| {
                    (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
                })
                .collect()
        }
        fn first_painted_fg(buf: &Buffer, rect: Rect) -> Option<ratatui::style::Color> {
            (rect.top()..rect.bottom())
                .flat_map(|y| (rect.left()..rect.right()).map(move |x| (x, y)))
                .find_map(|(x, y)| {
                    let cell = buf.cell((x, y))?;
                    if cell.symbol() != " " { Some(cell.fg) } else { None }
                })
        }
        fn channel_sum(c: ratatui::style::Color) -> u32 {
            match c {
                ratatui::style::Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
                _ => 0,
            }
        }

        let (w, h) = (80u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let l = RosterManager::layout(area);
        let mut ctx = EngineCtx;

        // Mid-slide render: nav right from index 0 -> 1, sample at ~67%
        // progress (200ms of the 300ms SLIDE_DUR).
        let mut mid_slide_scene = RosterManager::new();
        mid_slide_scene.handle_input(key_event(KeyCode::Right));
        mid_slide_scene.update(&mut ctx, Duration::from_millis(200));
        let mid_slide_buf = render_to_buffer(&mid_slide_scene, w, h);
        assert_eq!(mid_slide_scene.current_index, 1, "slide must not change current_index a second time");

        // No-slide render at the SAME resting current_index (1).
        let mut rest_scene = RosterManager::new();
        rest_scene.current_index = 1;
        let rest_buf = render_to_buffer(&rest_scene, w, h);

        assert_eq!(
            painted_columns(&mid_slide_buf, l.name),
            painted_columns(&rest_buf, l.name),
            "name rect's painted columns must be identical during an active slide vs. at rest — name no longer travels with col_offset"
        );
        assert_eq!(
            painted_columns(&mid_slide_buf, l.dot_row),
            painted_columns(&rest_buf, l.dot_row),
            "dot-row rect's painted columns must be identical during an active slide vs. at rest — dots no longer travel with col_offset"
        );
        assert_ne!(
            painted_columns(&mid_slide_buf, l.sprite),
            painted_columns(&rest_buf, l.sprite),
            "sprite rect's painted columns must differ during an active slide vs. at rest — the sprite is the only region that still slides"
        );

        let mid_name_fg = first_painted_fg(&mid_slide_buf, l.name)
            .expect("name rect must paint at least one cell mid-slide");
        let rest_name_fg = first_painted_fg(&rest_buf, l.name)
            .expect("name rect must paint at least one cell at rest");
        assert!(
            channel_sum(mid_name_fg) < channel_sum(rest_name_fg),
            "name fg during an active slide ({mid_name_fg:?}, sum={}) must be darker (closer to background) than its resting colour ({rest_name_fg:?}, sum={}) — b2-t1's colour cross-fade",
            channel_sum(mid_name_fg), channel_sum(rest_name_fg)
        );
    }

    /// A second nav fired before the first nav's slide has settled must be
    /// ignored (research.md's SCOPE_QUESTION default) — `current_index`
    /// reflects only the first nav.
    #[test]
    fn nav_ignored_during_active_slide() {
        let mut ctx = EngineCtx;
        let mut scene = RosterManager::new();
        scene.handle_input(key_event(KeyCode::Right));
        assert_eq!(scene.current_index, 1, "first nav must update current_index immediately");

        // Still well within the slide's transition window.
        scene.update(&mut ctx, Duration::from_millis(50));

        scene.handle_input(key_event(KeyCode::Right));
        assert_eq!(
            scene.current_index, 1,
            "a nav fired while a slide transition is active must be ignored until it settles"
        );
    }
}
