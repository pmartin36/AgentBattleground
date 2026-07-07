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
    /// The creature currently marked for a select-and-swap (b3-t1).
    /// `Some(current_index)` while blinking; `None` when nothing is
    /// selected.
    #[inspect(hidden)]
    selected_index: Option<usize>,
    /// Mouse-driven hit-test core for the CURRENT creature's dot slot
    /// (b3-t1) — reuses `engine_render::ButtonCore`'s completed-click state
    /// machine rather than hand-rolling one (see research.md's reuse
    /// check). `RefCell` for the same immutable-render-mutates-button-state
    /// reason as `left_button`/`right_button`/`home_button`.
    #[inspect(hidden)]
    current_dot: RefCell<engine_render::ButtonCore>,
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

/// The 7 named panel rects `layout()` splits `area` into (b1-t1,
/// research.md): `name` and `level` stacked tight at the top, a blank
/// `HEADER_GAP_H` row, then the body — a 2:1 LEFT/RIGHT column split with
/// `stat_bar` above `sprite` on the LEFT and `exhaustion` above
/// `ability_list` (the details panel) on the RIGHT — then `dot_row` at the
/// bottom. Only `sprite` is offset during a slide; every other rect is drawn
/// statically at the resting column regardless of `col_offset`.
#[derive(Clone, Copy, Debug)]
struct RosterLayout {
    name: Rect,
    level: Rect,
    stat_bar: Rect,
    exhaustion: Rect,
    /// Full ability + modifier list for the current creature (b2-t5), sitting
    /// directly below `exhaustion` and sharing its column (the details panel,
    /// RIGHT column of the body).
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

    /// Per-toggle interval of the selected dot's blink (b3-t1): the dot
    /// holds each of filled/unfilled for this long before flipping. A full
    /// blink cycle is 2x this. Not a spec-mandated value (spec doesn't state
    /// a period) — see research.md's ADVERSARIAL verdict.
    const BLINK_PERIOD: Duration = Duration::from_millis(400);

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

    /// Chrome color for procedural thin borders (details panel + stat-bar
    /// outlines, b1-t5/b1-t6). Not `FRAME_PANEL` — drawn via the dot
    /// pipeline (`draw_dot_border`, CLAUDE.md constraint 4).
    const BORDER_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0x88, 0x88, 0x88);

    /// Height (in rows) of the `name` band at the top of the frame.
    const NAME_H: u16 = 2;
    /// Height of the `level` band directly below `name`.
    const LEVEL_H: u16 = 1;
    /// Height of the blank row separating `level` from the body content
    /// (`stat_bar`/`exhaustion`).
    const HEADER_GAP_H: u16 = 1;
    /// Height of the `stat_bar`/`exhaustion` panel row below the header gap.
    const PANEL_H: u16 = 5;
    /// Gap (in cells) between adjacent stat-bar slices (b1-t6).
    const STAT_BAR_GAP: u16 = 1;
    /// Height (in rows) of the label row at the bottom of each stat-bar
    /// slice (b1-t6).
    const STAT_LABEL_H: u16 = 1;
    /// Sprite render-target inset from the left edge of `layout().sprite`
    /// (b1-t7). Asymmetric with `SPRITE_INSET_RIGHT` — this asymmetry IS
    /// the rightward shift; do not add a separate shift constant.
    const SPRITE_INSET_LEFT: u16 = 5;
    /// Sprite render-target inset from the right edge of `layout().sprite`
    /// (b1-t7).
    const SPRITE_INSET_RIGHT: u16 = 3;
    /// Sprite render-target inset from the top and bottom edges of
    /// `layout().sprite` (b1-t7). Guarantees a blank row above `dot_row`.
    const SPRITE_INSET_V: u16 = 1;
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
    /// row (b2-t6, widened b1-t3). `5` is spec 38's explicit, final pin —
    /// not a range to tune. It guarantees the 3 role labels — each wider
    /// than its dot cluster — stay visibly separated by a real blank-column
    /// margin (>=2 columns) between every adjacent pair, at both 40-col and
    /// 80-col widths, without the widened label group overlapping b1-t2's
    /// flanking arrow buttons (which also occupy the `dot_row` band).
    const CLUSTER_GAP: u16 = 5;
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

    /// Splits `area` into the 7 named panel rects (b1-t1, research.md
    /// blueprint), top to bottom: `name`, `level` (tight under `name`, no
    /// gap), a blank `HEADER_GAP_H` row, then the body, then `dot_row`. The
    /// body is a 2:1 LEFT/RIGHT column split: LEFT holds `stat_bar` directly
    /// above `sprite` (identical column range, width `area.width * 2 / 3`);
    /// RIGHT holds `exhaustion` directly above `ability_list` (the details
    /// panel), inset from `area`'s right edge by `EDGE_MARGIN`. Uses
    /// saturating arithmetic throughout so small `area`s degrade to
    /// zero-height/width rects instead of panicking.
    fn layout(area: Rect) -> RosterLayout {
        let name_h = Self::NAME_H.min(area.height);
        let level_h = Self::LEVEL_H.min(area.height.saturating_sub(name_h));
        let gap_h = Self::HEADER_GAP_H.min(area.height.saturating_sub(name_h + level_h));
        let dot_h = Self::DOT_H.min(area.height.saturating_sub(name_h + level_h + gap_h));
        let body_h = area
            .height
            .saturating_sub(name_h + level_h + gap_h + dot_h);

        let name = Rect::new(area.x, area.y, area.width, name_h);
        let level = Rect::new(area.x, area.y + name_h, area.width, level_h);

        let body_y = area.y + name_h + level_h + gap_h;

        // LEFT column: stat_bar directly above sprite, both spanning the
        // same column range (no ARROW_W inset — arrows move to dot_row).
        let left_w = area.width * 2 / 3;
        let stat_bar_h = Self::PANEL_H.min(body_h);
        let stat_bar = Rect::new(area.x, body_y, left_w, stat_bar_h);
        let sprite_h = body_h.saturating_sub(stat_bar_h);
        let sprite = Rect::new(area.x, body_y + stat_bar_h, left_w, sprite_h);

        // RIGHT column (details panel): width = area.width - left_w, right
        // edge inset EDGE_MARGIN from area's edge.
        let details_w = area.width.saturating_sub(left_w);
        let details_x = area.right().saturating_sub(Self::EDGE_MARGIN + details_w);
        let exhaustion_h = Self::PANEL_H.min(body_h);
        let exhaustion = Rect::new(details_x, body_y, details_w, exhaustion_h);
        let ability_list = Rect::new(
            details_x,
            body_y + exhaustion_h,
            details_w,
            body_h.saturating_sub(exhaustion_h),
        );

        let dot_row = Rect::new(area.x, body_y + body_h, area.width, dot_h);

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
            selected_index: None,
            current_dot: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
        }
    }

    /// Left/right arrow button rects flanking `layout(area).dot_row` — the
    /// sole place button positioning is computed; `render()` and tests both
    /// call this rather than re-deriving it. `ARROW_H` already matches
    /// `DOT_H`, so a full-`ARROW_W`-wide button anchored in the FULL
    /// `dot_row` band (dots + role-label row) sits entirely within that band
    /// and entirely outside the sprite band above it.
    fn arrow_rects(area: Rect) -> (Rect, Rect) {
        let band = Self::layout(area).dot_row;
        let size = (Self::ARROW_W, Self::ARROW_H);
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

    /// Details-panel geometry (b1-t5): `(border, ex_text, ability_text)`.
    /// `border` is the 1-cell-perimeter rect around the union of
    /// `layout(area).exhaustion` and `.ability_list` (they share x/width and
    /// are stacked contiguously in y, so the union is exact without needing
    /// `Rect::union`). The text rects are inset 1 cell off every bordered
    /// edge — there is NO border between exhaustion and ability_list (they
    /// share an interior boundary), so `ability_text` keeps its top edge
    /// un-inset. Sole source of this geometry; `render()` and tests both
    /// call this rather than re-deriving it.
    fn details_panel_rects(area: Rect) -> (Rect, Rect, Rect) {
        let l = Self::layout(area);
        let border = Rect::new(
            l.exhaustion.x,
            l.exhaustion.y,
            l.exhaustion.width,
            l.exhaustion.height + l.ability_list.height,
        );
        let ex_text = Rect::new(
            l.exhaustion.x + 1,
            l.exhaustion.y + 1,
            l.exhaustion.width.saturating_sub(2),
            l.exhaustion.height.saturating_sub(1),
        );
        let ability_text = Rect::new(
            l.ability_list.x + 1,
            l.ability_list.y,
            l.ability_list.width.saturating_sub(2),
            l.ability_list.height.saturating_sub(1),
        );
        (border, ex_text, ability_text)
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

    /// Whether the selected dot should currently paint as "filled" for its
    /// blink (b3-t1) — alternates every `BLINK_PERIOD` of `elapsed`. Pure
    /// over `&self`, keyed off the shared `elapsed` clock (no separate
    /// per-selection timer field).
    fn blink_on(&self) -> bool {
        (self.elapsed.as_millis() / Self::BLINK_PERIOD.as_millis()).is_multiple_of(2)
    }

    /// The single Space/current-dot-click action (b3-t1): selects the
    /// current creature if nothing is selected, cancels the selection if
    /// the current creature is already selected, and is a no-op if a
    /// DIFFERENT creature is selected (navigated-away-then-selected — b3-t2's
    /// swap fills this branch in).
    fn toggle_selection(&mut self) {
        match self.selected_index {
            None => self.selected_index = Some(self.current_index),
            Some(sel) if sel == self.current_index => self.selected_index = None,
            Some(sel) => {
                self.creatures.swap(sel, self.current_index);
                self.selected_index = None;
            }
        }
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
        let base_rect = Self::layout(zero_area).sprite;
        let sprite_rect = Rect::new(
            base_rect.x.saturating_add(Self::SPRITE_INSET_LEFT),
            base_rect.y.saturating_add(Self::SPRITE_INSET_V),
            base_rect
                .width
                .saturating_sub(Self::SPRITE_INSET_LEFT + Self::SPRITE_INSET_RIGHT),
            base_rect.height.saturating_sub(Self::SPRITE_INSET_V * 2),
        );

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

    /// The 4 stat slices across `stat_bar` (`StatKind::ALL` order,
    /// left->right), each as `(outline_rect, fill_interior_rect,
    /// label_rect)` — sole source of stat-bar geometry; `render_stat_bars`
    /// and `stat_bar_tests` both call it (research.md b1-t6 blueprint).
    /// Mirrors `dot_cluster_rects`'s `engine_render::stack` Horizontal
    /// pattern, but computes a slice width that FILLS `stat_bar.width`
    /// (rather than centering a fixed-size group). Each slice reserves its
    /// bottom `STAT_LABEL_H` row for the label; the rows above become the
    /// outline, with `fill` inset a full cell on every side of the outline
    /// so a lit fill cell never shares a cell with (and so never overwrites)
    /// the outline's border glyphs.
    fn stat_slice_parts(stat_bar: Rect) -> Vec<(Rect, Rect, Rect)> {
        let n = crate::stats::StatKind::ALL.len() as u16;
        let slice_w = stat_bar
            .width
            .saturating_sub(Self::STAT_BAR_GAP * (n - 1))
            / n;
        let sizes: Vec<(u16, u16)> = vec![(slice_w, stat_bar.height); n as usize];
        let slices = engine_render::stack(
            stat_bar,
            &sizes,
            Self::STAT_BAR_GAP,
            engine_render::StackAxis::Horizontal,
        );

        slices
            .into_iter()
            .map(|s| {
                let label_h = Self::STAT_LABEL_H.min(s.height);
                let outline_h = s.height.saturating_sub(label_h);
                let outline = Rect::new(s.x, s.y, s.width, outline_h);
                let label = Rect::new(s.x, s.y + outline_h, s.width, label_h);
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
    fn stat_label(kind: crate::stats::StatKind) -> &'static str {
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
    /// blueprint). Per slice: an unconditional `draw_dot_border` outline
    /// (visible even at zero fill), a `STAT_BAR_COLOR` fill proportional to
    /// `stat_fill_dots` drawn strictly inside the outline's interior (non-
    /// text chrome, so it renders through the dot pipeline —
    /// `DotBuffer`/`Dot::Lit`/`dots_to_grid`/`draw_grid`, never
    /// `engine_render::fill`, CLAUDE.md constraint 4), and a plain-text
    /// `stat_label(kind)` beneath its own bar.
    fn render_stat_bars(&self, buf: &mut Buffer, rect: Rect) {
        for ((outline, fill, label), kind) in
            Self::stat_slice_parts(rect).into_iter().zip(crate::stats::StatKind::ALL)
        {
            Self::draw_dot_border(buf, outline, Self::BORDER_COLOR);

            let dot_cols = fill.width as usize * 2;
            let dot_rows = fill.height as usize * 4;
            if dot_cols > 0 && dot_rows > 0 {
                let mut dots = DotBuffer::new(dot_cols, dot_rows);
                let n = self.stat_fill_dots(kind, dot_cols);
                for row in 0..dot_rows {
                    for col in 0..n {
                        dots.set(col, row, Dot::Lit(Self::STAT_BAR_COLOR));
                    }
                }
                let grid = dots_to_grid(&dots);
                engine_render::draw_grid(buf, fill, &grid);
            }

            engine_render::label(buf, label, Self::stat_label(kind), Self::DOT_LABEL_COLOR);
        }
    }

    /// Draws a 1-dot-thick rectangular border filling `rect`'s perimeter via
    /// the dot pipeline (b1-t4) — the same "light one dot position across
    /// the whole run" technique as `battle_viewer::draw_board_lines`,
    /// restricted to the 4 outer edges. Interiors are left `Transparent` by
    /// `draw_grid` (existing buffer content underneath is preserved). Clips
    /// (no-ops) on a zero-size rect. Shared by b1-t5 (details panel) and
    /// b1-t6 (stat-bar outlines) — `FRAME_PANEL` MUST NOT be used for this.
    fn draw_dot_border(buf: &mut Buffer, rect: Rect, color: engine_core::color::Rgba) {
        let dot_cols = rect.width as usize * 2;
        let dot_rows = rect.height as usize * 4;
        if dot_cols == 0 || dot_rows == 0 {
            return;
        }
        let mut dots = DotBuffer::new(dot_cols, dot_rows);
        for col in 0..dot_cols {
            dots.set(col, 0, Dot::Lit(color));
            dots.set(col, dot_rows - 1, Dot::Lit(color));
        }
        for row in 0..dot_rows {
            dots.set(0, row, Dot::Lit(color));
            dots.set(dot_cols - 1, row, Dot::Lit(color));
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
        let slots = Self::dot_slots(dot_row_rect);
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
            let (border, ex_text, ability_text) = Self::details_panel_rects(area);
            Self::draw_dot_border(frame.buffer_mut(), border, Self::BORDER_COLOR);
            self.render_exhaustion(frame.buffer_mut(), ex_text, self.current_index);
            self.render_ability_list(frame.buffer_mut(), ability_text, self.current_index);
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
                KeyCode::Char(' ') => self.toggle_selection(),
                _ => {}
            },
            InputEvent::Mouse(me) => {
                let hit_left = self.left_button.get_mut().handle_mouse(&me);
                let hit_right = self.right_button.get_mut().handle_mouse(&me);
                let hit_home = self.home_button.get_mut().handle_mouse(&me);
                let hit_dot = self.current_dot.get_mut().handle_mouse(&me);
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
                if hit_dot {
                    self.toggle_selection();
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

/// b1-t1: `layout()`'s expanded 7-rect contract — the shared layout every
/// rendering function renders into. Header spacing: `level` sits tight under
/// `name` (no gap), then `HEADER_GAP_H` blank row, then the body. Body is a
/// 2:1 LEFT/RIGHT column split: LEFT holds `stat_bar` above `sprite`
/// (identical column range); RIGHT holds `exhaustion` above `ability_list`
/// (the details panel), inset from `area`'s right edge by `EDGE_MARGIN`.
#[cfg(test)]
mod layout_tests {
    use super::*;

    /// `layout(area)` must order its bands top-to-bottom: name < level <
    /// stat_bar/sprite/dot_row, with `level` tight under `name` (no blank
    /// row) and a blank `HEADER_GAP_H` row between `level` and the body
    /// (`stat_bar`), and `exhaustion` above `ability_list`.
    #[test]
    fn layout_rects_ordered_top_to_bottom() {
        let area = Rect::new(0, 0, 80, 30);
        let l = RosterManager::layout(area);

        assert!(l.name.y < l.level.y, "name.y ({}) must be above level.y ({})", l.name.y, l.level.y);
        assert_eq!(
            l.name.y + l.name.height, l.level.y,
            "level must sit directly under name with no blank row (name.y={} + name.height={} != level.y={})",
            l.name.y, l.name.height, l.level.y
        );
        assert!(
            l.level.y + l.level.height < l.stat_bar.y,
            "a blank HEADER_GAP_H row must separate level ({}+{}) from stat_bar.y ({})",
            l.level.y, l.level.height, l.stat_bar.y
        );
        assert!(l.exhaustion.y < l.ability_list.y, "exhaustion.y ({}) must be above ability_list.y ({})", l.exhaustion.y, l.ability_list.y);
        assert!(l.stat_bar.y < l.sprite.y, "stat_bar.y ({}) must be above sprite.y ({})", l.stat_bar.y, l.sprite.y);
        assert!(l.sprite.y < l.dot_row.y, "sprite.y ({}) must be above dot_row.y ({})", l.sprite.y, l.dot_row.y);
    }

    /// The details panel (`exhaustion`/`ability_list`) width must equal
    /// `area.width - area.width * 2 / 3` (the RIGHT column of the 2:1 split).
    #[test]
    fn details_panel_width_is_one_third() {
        for width in [60u16, 90u16] {
            let area = Rect::new(0, 0, width, 30);
            let l = RosterManager::layout(area);
            let expected = width - (width * 2 / 3);
            assert_eq!(l.exhaustion.width, expected, "width={width}: exhaustion.width");
            assert_eq!(l.ability_list.width, expected, "width={width}: ability_list.width");
        }
    }

    /// `stat_bar` sits directly above `sprite`, spanning the identical
    /// column range (both are the LEFT column of the 2:1 split).
    #[test]
    fn stat_bar_spans_sprite_columns_and_sits_above() {
        let area = Rect::new(0, 0, 80, 30);
        let l = RosterManager::layout(area);
        assert_eq!(l.stat_bar.left(), l.sprite.left(), "stat_bar.left() must equal sprite.left()");
        assert_eq!(l.stat_bar.right(), l.sprite.right(), "stat_bar.right() must equal sprite.right()");
        assert!(
            l.stat_bar.y + l.stat_bar.height <= l.sprite.y,
            "stat_bar ({}+{}) must sit above sprite.y ({})",
            l.stat_bar.y, l.stat_bar.height, l.sprite.y
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
            assert!(
                l.sprite.right() <= l.ability_list.left() + RosterManager::EDGE_MARGIN,
                "width={}: sprite ({:?}) must not extend more than EDGE_MARGIN into ability_list ({:?})",
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

        // Exclude the rightmost EDGE_MARGIN column: b1-t1's layout intentionally
        // shares that column with the details-panel border (b1-t5), which always
        // paints its bottom-left corner at (sprite.right()-1, dot_row.top()-1)
        // regardless of any SPRITE_INSET value — see
        // `sprite_and_ability_list_columns_disjoint` for the tolerated overlap.
        // This test only cares about the sprite's OWN content, not that
        // unrelated, already-correct border cell.
        let sprite_content_right = l.sprite.right().saturating_sub(RosterManager::EDGE_MARGIN);
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

/// b1-t6: 4 stat bars (STR/DEX/INT/VIT, `StatKind::ALL` order) rendered as
/// side-by-side outlined+labeled column slices within `layout().stat_bar`
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

    /// DELIVERABLE 1: the 4 slices occupy 4 non-overlapping column ranges,
    /// strictly left-to-right in `StatKind::ALL` order, at both 40- and
    /// 80-wide areas.
    #[test]
    fn slices_are_four_disjoint_ordered_columns() {
        for w in [40u16, 80u16] {
            let l = RosterManager::layout(Rect::new(0, 0, w, 30));
            let slices = RosterManager::stat_slice_parts(l.stat_bar);
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

    /// DELIVERABLE 2: a slice whose stat is 0 still shows its outline — a
    /// border-only slice remains visible.
    #[test]
    fn zero_fill_slice_still_outlined() {
        let buf = render_with_stats(only_stat(StatKind::Strength, 0));
        let l = RosterManager::layout(area());
        let slices = RosterManager::stat_slice_parts(l.stat_bar);
        let (outline, _fill, _label) = slices[0]; // Strength == StatKind::ALL[0]

        for x in outline.left()..outline.right() {
            assert_ne!(
                buf.cell((x, outline.top())).unwrap().symbol(),
                " ",
                "top edge of the zero-fill slice's outline must still be painted"
            );
        }
        for y in outline.top()..outline.bottom() {
            assert_ne!(
                buf.cell((outline.left(), y)).unwrap().symbol(),
                " ",
                "left edge of the zero-fill slice's outline must still be painted"
            );
        }
    }

    /// DELIVERABLE 3: a higher stat value paints strictly farther right,
    /// measured strictly inside its OWN slice's fill-interior rect (not the
    /// whole stat_bar, since the ever-present border column would mask it).
    #[test]
    fn fill_length_scales_with_stat_value() {
        let l = RosterManager::layout(area());
        let slices = RosterManager::stat_slice_parts(l.stat_bar);
        let (_outline, fill_rect, _label) = slices[1]; // Dexterity == StatKind::ALL[1]

        let buf_low = render_with_stats(only_stat(StatKind::Dexterity, 5));
        let low_col = rightmost_non_space(&buf_low, fill_rect);
        assert!(low_col.is_some(), "a non-zero Dexterity value must paint the DEX slice's fill interior");

        let buf_high = render_with_stats(only_stat(StatKind::Dexterity, 35));
        let high_col = rightmost_non_space(&buf_high, fill_rect);
        assert!(high_col.is_some(), "a higher Dexterity value must also paint the DEX slice's fill interior");

        assert!(
            high_col.unwrap() > low_col.unwrap(),
            "a higher stat value (35) must paint farther right ({high_col:?}) than a lower one (5) ({low_col:?})"
        );
    }

    /// DELIVERABLE 4: no ASCII digit is ever painted inside `stat_bar` —
    /// bars + STR/DEX/INT/VIT labels only, never numeric text.
    #[test]
    fn no_numeric_text_in_stat_bar() {
        let scene = RosterManager::new(); // index 0: Ember Wolf, real demo_roster stats
        let buf = render_to_buffer(&scene, area().width, area().height);
        let rect = RosterManager::layout(area()).stat_bar;

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
        let l = RosterManager::layout(area());
        let slices = RosterManager::stat_slice_parts(l.stat_bar);

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
        let stat_bar = RosterManager::layout(area()).stat_bar;
        // Content region only: excludes the rightmost column, which overlaps
        // the details panel's own border and is only conditionally painted
        // (see research.md b1-t6 "Correction" -- the full `stat_bar` rect is
        // unsatisfiable here since that column differs by design, not by a
        // stat-bar regression).
        let rect = Rect::new(
            stat_bar.x,
            stat_bar.y,
            stat_bar.width.saturating_sub(RosterManager::EDGE_MARGIN),
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
        let l = RosterManager::layout(area());
        let slices = RosterManager::stat_slice_parts(l.stat_bar);
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
}

/// b1-t4: `draw_dot_border` — the shared procedural thin-border helper
/// (dot pipeline). Standalone rect tests, no `RosterManager` instance
/// needed — mirrors `battle_viewer::draw_board_lines_tests`'s pattern of
/// calling the fn directly against a throwaway `Buffer::empty`.
#[cfg(test)]
mod draw_dot_border_tests {
    use super::*;
    use ratatui::buffer::Buffer;

    /// Hand-picked rect used by every case below: origin (2,1), 10x6, inset
    /// into a comfortably larger 20x12 buffer so out-of-rect cells are
    /// distinguishable from in-rect ones.
    fn rect() -> Rect {
        Rect::new(2, 1, 10, 6)
    }

    fn render() -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 12));
        RosterManager::draw_dot_border(&mut buf, rect(), RosterManager::BORDER_COLOR);
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
        RosterManager::draw_dot_border(&mut buf, Rect::new(2, 1, 0, 6), RosterManager::BORDER_COLOR);
        assert_eq!(buf, before, "zero-width rect must leave the buffer unchanged");
    }
}

/// b1-t5: procedural bordered details panel (union of `exhaustion` +
/// `ability_list`), drawn via `draw_dot_border` (b1-t4) — the fix for the
/// "huge fat blob" regression the spec's Purpose section calls out. The
/// expected border rect is computed independently here (mirroring
/// research.md's `details_panel_rects` geometry: the union of `exhaustion`
/// and `ability_list`, which are stacked with identical x/width) rather than
/// depending on that private helper directly, so these tests assert the
/// OBSERVABLE render, not an internal implementation fn.
#[cfg(test)]
mod details_panel_border_tests {
    use super::*;
    use crate::scenes::test_util::render_to_buffer;

    /// The details-panel border rect — the union of `exhaustion` and
    /// `ability_list`.
    fn border_rect(area: Rect) -> Rect {
        let l = RosterManager::layout(area);
        Rect::new(
            l.exhaustion.x,
            l.exhaustion.y,
            l.exhaustion.width,
            l.exhaustion.height + l.ability_list.height,
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

    /// Deliverable (b): text never lands on the border's own dot-glyph
    /// cells — every perimeter cell of the details-panel border is a
    /// non-alphanumeric (braille dot) glyph, never ASCII exhaustion/ability
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

    /// `render()` paints the left arrow button within its own rect, inset
    /// from `area`'s left edge by `EDGE_MARGIN`. b1-t2: the button flanks
    /// `layout().dot_row`, not the sprite — its row range sits within
    /// `dot_row`'s row range and entirely outside `sprite`'s row range.
    #[test]
    fn left_button_flanks_dot_row() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (left_rect, _) = RosterManager::arrow_rects(area);
        let layout = RosterManager::layout(area);

        assert!(
            has_non_space(&buf, left_rect),
            "left arrow button must paint at least one non-space cell within its rect"
        );
        assert_eq!(
            left_rect.x,
            area.x + RosterManager::EDGE_MARGIN,
            "left arrow button rect must be inset from area's left edge by EDGE_MARGIN"
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

    /// `render()` paints the right arrow button, flanking `layout().dot_row`
    /// (b1-t2) — not the sprite.
    #[test]
    fn right_button_flanks_dot_row() {
        let scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let (_, right_rect) = RosterManager::arrow_rects(area);
        let layout = RosterManager::layout(area);

        assert!(
            has_non_space(&buf, right_rect),
            "right arrow button must paint at least one non-space cell within its rect"
        );
        assert_eq!(
            right_rect.right(),
            area.right() - RosterManager::EDGE_MARGIN,
            "right arrow button rect must be inset from area's right edge by EDGE_MARGIN"
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

    /// `layout(area).sprite` narrowed to exclude the static (non-sliding)
    /// arrow-button columns. As of b1-t1, `sprite` spans the full LEFT
    /// column with no `ARROW_W` inset, so the left arrow button's fixed
    /// columns now fall inside the sprite's own column range; without
    /// excluding them, `leftmost_non_space_in_rect` picks up the static
    /// button instead of the actually-sliding sprite content. b1-t2 moves
    /// the arrows off the sprite band entirely (flanking `dot_row`
    /// instead), which will make this narrowing a no-op.
    fn sprite_measure_rect(area: Rect) -> Rect {
        let sprite_rect = RosterManager::layout(area).sprite;
        let (left_arrow, right_arrow) = RosterManager::arrow_rects(area);
        let left = if left_arrow.left() < sprite_rect.right() && left_arrow.right() > sprite_rect.left() {
            sprite_rect.left().max(left_arrow.right())
        } else {
            sprite_rect.left()
        };
        let right = if right_arrow.left() < sprite_rect.right() && right_arrow.right() > sprite_rect.left() {
            sprite_rect.right().min(right_arrow.left())
        } else {
            sprite_rect.right()
        };
        Rect::new(left, sprite_rect.y, right.saturating_sub(left), sprite_rect.height)
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
        let sprite_rect = sprite_measure_rect(area);

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
        let sprite_rect = sprite_measure_rect(area);

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

/// b3-t1: selection state (Space / current-dot click sets/cancels
/// `selected_index`) and the selected dot's blink render. See research.md's
/// blueprint — `handle_input`/`render_dot_row` are not yet wired to
/// `selected_index`, so these are RED until the code-writer wires them.
#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::scenes::test_util::{key_event, mouse_event, render_to_buffer};
    use crossterm::event::KeyCode;
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::style::Color;
    use engine_core::scene::EngineCtx;

    /// The fg color of the first non-space cell found inside `slot`, or
    /// `None` if the slot has no painted cell (mirrors
    /// `dot_row_render_tests::sample_fg`).
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

    /// Space with no prior selection selects the current creature.
    #[test]
    fn space_selects_current_when_none_selected() {
        let mut scene = RosterManager::new();
        assert_eq!(scene.selected_index, None);

        scene.handle_input(key_event(KeyCode::Char(' ')));

        assert_eq!(
            scene.selected_index,
            Some(scene.current_index),
            "Space with no prior selection must set selected_index to the current creature"
        );
    }

    /// A completed click (Moved/Down/Up) on the CURRENT creature's dot does
    /// the same as Space.
    #[test]
    fn click_current_dot_selects() {
        let mut scene = RosterManager::new();
        let (w, h) = (40u16, 20u16);
        // Render once so the dot hit rects are set for this frame's area
        // (handle_input hit-tests against the PREVIOUS frame's render, per
        // the arrow/home button pattern).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let slot = RosterManager::dot_slots(dots_rect)[scene.current_index];
        let (cx, cy) = (slot.x, slot.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        assert_eq!(
            scene.selected_index,
            Some(scene.current_index),
            "a completed click on the current creature's dot must select it"
        );
    }

    /// While selected, the selected dot's painted fg alternates between two
    /// `BLINK_PERIOD`-separated `elapsed` samples; every other dot's fg is
    /// unaffected.
    #[test]
    fn selected_dot_blinks_across_elapsed() {
        let mut scene = RosterManager::new();
        scene.selected_index = Some(scene.current_index);

        let (w, h) = (40u16, 20u16);
        let area = Rect::new(0, 0, w, h);
        let dots_rect = RosterManager::layout(area).dot_row;
        let slots = RosterManager::dot_slots(dots_rect);
        let selected_slot = slots[scene.current_index];
        let other_slot = slots[(scene.current_index + 1) % slots.len()];

        let buf_a = render_to_buffer(&scene, w, h);
        let selected_fg_a = sample_fg(&buf_a, selected_slot).expect("selected dot must paint");
        let other_fg_a = sample_fg(&buf_a, other_slot).expect("other dot must paint");

        let mut ctx = EngineCtx;
        scene.update(&mut ctx, RosterManager::BLINK_PERIOD);
        let buf_b = render_to_buffer(&scene, w, h);
        let selected_fg_b = sample_fg(&buf_b, selected_slot).expect("selected dot must paint");
        let other_fg_b = sample_fg(&buf_b, other_slot).expect("other dot must paint");

        assert_ne!(
            selected_fg_a, selected_fg_b,
            "selected dot's fg must alternate (blink) across a BLINK_PERIOD-separated elapsed sample"
        );
        assert_eq!(
            other_fg_a, other_fg_b,
            "a non-selected dot's fg must be unaffected by the blink timer"
        );
    }

    /// Space again with `selected_index == Some(current_index)` (no
    /// navigation between) cancels the selection back to `None`.
    #[test]
    fn space_again_cancels_selection() {
        let mut scene = RosterManager::new();
        scene.selected_index = Some(scene.current_index);

        scene.handle_input(key_event(KeyCode::Char(' ')));

        assert_eq!(
            scene.selected_index, None,
            "Space again at the same current_index (no nav between) must cancel the selection"
        );
    }

    /// b3-t2: select index 0, navigate to index 1, select again -> the two
    /// creatures (by name/identity) are swapped and the selection clears.
    #[test]
    fn select_navigate_select_swaps_creatures() {
        let mut scene = RosterManager::new();
        let name_at_0_before = scene.creatures[0].name().to_string();
        let name_at_1_before = scene.creatures[1].name().to_string();

        scene.handle_input(key_event(KeyCode::Char(' '))); // select current (0)
        assert_eq!(scene.selected_index, Some(0));

        scene.handle_input(key_event(KeyCode::Right)); // navigate 0 -> 1
        assert_eq!(scene.current_index, 1);

        scene.handle_input(key_event(KeyCode::Char(' '))); // select again -> swap

        assert_eq!(
            scene.selected_index, None,
            "a completed swap must clear the selection"
        );
        assert_eq!(
            scene.creatures[0].name(),
            name_at_1_before,
            "the creature originally at index 1 must now be at index 0"
        );
        assert_eq!(
            scene.creatures[1].name(),
            name_at_0_before,
            "the creature originally at index 0 must now be at index 1"
        );
    }

    /// b3-t2: swapping an active-slot index with a reserve-slot index flips
    /// each CREATURE's squad role (tracked by identity through the swap, not
    /// by asserting on the fixed slot indices themselves — `squad_role` is a
    /// pure positional lookup, so slot 0 is always Active and the last slot
    /// is always Reserve; what must flip is which creature sits where).
    #[test]
    fn swap_active_with_reserve_flips_role_by_creature_identity() {
        use crate::squad_role::{squad_role, SquadRole, ACTIVE_SLOTS, ROSTER_SIZE};

        let mut scene = RosterManager::new();
        let active_index = 0;
        let reserve_index = ROSTER_SIZE - 1;
        assert!(active_index < ACTIVE_SLOTS, "index 0 must be an active slot");
        assert_eq!(
            squad_role(reserve_index),
            SquadRole::Reserve,
            "the last roster slot must be a reserve slot"
        );

        let active_creature_name = scene.creatures[active_index].name().to_string();
        let reserve_creature_name = scene.creatures[reserve_index].name().to_string();

        // Select the active creature, then jump the cursor directly to the
        // reserve index (bypassing navigate()'s slide-guard, which is not
        // under test here) and select again to swap.
        scene.selected_index = Some(active_index);
        scene.current_index = reserve_index;
        scene.handle_input(key_event(KeyCode::Char(' ')));

        assert_eq!(scene.selected_index, None, "swap must clear the selection");

        let new_index_of_active_creature = scene
            .creatures
            .iter()
            .position(|c| c.name() == active_creature_name)
            .expect("the originally-active creature must still be present");
        let new_index_of_reserve_creature = scene
            .creatures
            .iter()
            .position(|c| c.name() == reserve_creature_name)
            .expect("the originally-reserve creature must still be present");

        assert_eq!(new_index_of_active_creature, reserve_index);
        assert_eq!(new_index_of_reserve_creature, active_index);
        assert_eq!(
            squad_role(new_index_of_active_creature),
            SquadRole::Reserve,
            "the creature moved into the reserve slot must now report Reserve"
        );
        assert_eq!(
            squad_role(new_index_of_reserve_creature),
            SquadRole::Active,
            "the creature moved into the active slot must now report Active"
        );
    }

    /// b3-t2 guard: re-selecting at the SAME current_index as selected_index
    /// (no navigation between) must cancel per b3-t1, NOT swap — the roster
    /// order must be unchanged.
    #[test]
    fn reselect_same_index_cancels_without_reordering_roster() {
        let mut scene = RosterManager::new();
        let names_before: Vec<String> =
            scene.creatures.iter().map(|c| c.name().to_string()).collect();

        scene.handle_input(key_event(KeyCode::Char(' '))); // select current (0)
        assert_eq!(scene.selected_index, Some(0));

        scene.handle_input(key_event(KeyCode::Char(' '))); // same index again -> cancel

        assert_eq!(scene.selected_index, None);
        let names_after: Vec<String> =
            scene.creatures.iter().map(|c| c.name().to_string()).collect();
        assert_eq!(
            names_before, names_after,
            "cancelling a selection must never reorder the roster"
        );
    }
}
