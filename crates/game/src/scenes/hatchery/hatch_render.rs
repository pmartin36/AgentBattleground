//! Hatch sequence render + scene wiring: consumes `pending_hatch` from
//! `update()` to launch a [`super::hatch::HatchSequence`] on the tapped egg,
//! ticks it, renders every phase over the focused egg's rect, and gates
//! input while the sequence is active.

use std::time::{Duration, SystemTime};

use image::DynamicImage;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::Frame;

use engine_render::dots::{Dot, DotBuffer};
use engine_render::{ui_primitives, AnimatedSprite, DotRect, TextAlign};

use crate::player_data::{resolve_clip, Egg, EggState, PersistedCreature};
use crate::scenes::detail_panel;
use crate::scenes::stat_bar::StatBarChrome;

use super::hatch::HatchPhase;
use super::hatch_layout;

/// Settled dock border color — the same grey as the roster details panel's
/// own chrome.
const DOCK_BORDER_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0x88, 0x88, 0x88);

/// Chrome for the settled stat-bar band — the same grey border as the dock,
/// white labels, a single-dot border thickness and chamfer (mirrors the
/// roster's own stat-bar chrome shape).
const HATCH_STAT_CHROME: StatBarChrome = StatBarChrome {
    border_color: DOCK_BORDER_COLOR,
    label_color: engine_core::color::Rgba::rgb(0xff, 0xff, 0xff),
    h_thickness: 1,
    chamfer: 1,
};

/// The launched-and-in-progress hatch sequence for one egg: the pure
/// timeline plus the sprites its render needs, decoded once at launch.
pub(crate) struct HatchState {
    pub(super) egg: usize,
    /// Read by `handle_input`'s non-interruptible guard.
    pub(super) seq: super::hatch::HatchSequence,
    crack: Option<AnimatedSprite>,
    idle: Option<AnimatedSprite>,
    /// Decoded for hatch-readiness parity with `hatch_assets_ready` (the
    /// hatchling's attack clip must resolve before the reveal launches);
    /// never rasterized during the hatch — the starting-attack clip does
    /// not play in the reveal.
    #[allow(dead_code)]
    attack: Option<AnimatedSprite>,
}

/// The pre-reveal hatch-out transition for one egg: the egg flies from its
/// browse left-slot to screen-center while the right panel slides off,
/// before control passes to the crack/break/reveal sequence. Pure timer;
/// poses are computed at render time from `browse_layout`'s unfloored
/// resting rects via `hatch_layout::hatch_out_pose`.
pub(super) struct HatchOut {
    pub(super) egg: usize,
    pub(super) elapsed: Duration,
}

impl HatchState {
    /// Decodes the shared crack overlay once and resolves the hatchling's
    /// idle/attack clips (falling back to `None`, never panicking, when a
    /// clip is unresolved), building a fresh sequence at `elapsed == 0`.
    fn launch(egg_index: usize, egg: &Egg) -> Self {
        let crack = AnimatedSprite::from_gif(crate::assets::EGG_CRACK, crate::creatures::FRAME_DUR).ok();
        let (idle, attack) = match &egg.hatchling {
            Some(hatchling) => (resolve_idle(hatchling), hatchling.attack.as_ref().and_then(resolve_clip)),
            None => (None, None),
        };
        Self { egg: egg_index, seq: super::hatch::HatchSequence::new(), crack, idle, attack }
    }
}

/// The just-hatched creature retained after the last egg leaves the dock:
/// its persisted data plus its idle clip, resolved once at retention so the
/// empty-dock view renders it read-only every frame with no active
/// `HatchState`.
pub(super) struct SettledCreature {
    pub creature: PersistedCreature,
    pub idle: Option<AnimatedSprite>,
}

/// The sole idle-clip resolve site for a hatchling: `HatchState::launch` and
/// the empty-dock retention both call this, so idle resolution never forks
/// into a second expression.
pub(super) fn resolve_idle(hatchling: &PersistedCreature) -> Option<AnimatedSprite> {
    hatchling.idle.as_ref().and_then(resolve_clip)
}

/// Rasterizes `art` (the egg's decoded still) at `w × h` dots.
fn still_to_dots(art: &DynamicImage, w: u32, h: u32) -> DotBuffer {
    engine_render::dots::sprite_to_dots(art, w, h)
}

/// The hatchling's name-label rect: centered under `focus_dr`, widened a
/// few columns beyond the focus rect's width to admit a short wrap, three
/// rows tall.
pub(super) fn name_rect(focus_dr: DotRect) -> Rect {
    let cell = focus_dr.to_cell_rect();
    let width = cell.width.saturating_add(6);
    let height = 3;
    let x = (cell.x + cell.width / 2).saturating_sub(width / 2);
    let y = cell.y + cell.height;
    Rect { x, y, width, height }
}

/// The single readiness predicate consulted at the one hatch-launch site:
/// an egg is ready to reveal only once its still and both hatchling clips
/// have all resolved. Any hatch entry path (tap or dev force-hatch) that
/// records a `pending_hatch` request before this is `true` waits in the
/// generating state until it becomes so.
fn hatch_assets_ready(egg: &Egg) -> bool {
    egg.egg_art.is_some() && egg.hatchling.as_ref().is_some_and(|h| h.idle.is_some() && h.attack.is_some())
}

/// Recolors every lit dot toward its true color by linearly interpolating
/// from white (`t == 0.0`, pure white silhouette) to the dot's own source
/// color (`t == 1.0`, true color). Transparent/occluded dots pass through
/// untouched.
fn reveal_recolor(dots: &DotBuffer, t: f32) -> DotBuffer {
    let white = engine_core::color::Rgba::rgb(255, 255, 255);
    let mut out = DotBuffer::new(dots.cols(), dots.rows());
    for row in 0..dots.rows() {
        for col in 0..dots.cols() {
            let dot = match dots.get(col, row) {
                Dot::Lit(src) => Dot::Lit(white.lerp(src, t)),
                other => other,
            };
            out.set(col, row, dot);
        }
    }
    out
}

/// Rasterizes crack frame `i` at `w × h` dots. `AnimatedSprite` exposes no
/// direct frame-index accessor, but at `speed == 1.0` it resolves the active
/// frame by exact integer division of `elapsed / frame_dur`
/// (`AnimatedSprite::frame_index_at`), so sampling at `frame_dur() * i`
/// selects frame `i` exactly (for `i < frame_count`) while still hitting the
/// shared rasterization cache.
fn crack_frame_dots(sprite: &AnimatedSprite, i: usize, w: u32, h: u32) -> DotBuffer {
    sprite.dots_at(sprite.frame_dur() * i as u32, w, h)
}

/// Splits `dots` into two same-sized buffers: the top half's rows (`0..mid`)
/// with the bottom half blanked out, and vice versa, so each half can be
/// drawn at the same target rect but nudged apart vertically without
/// recomputing sub-rect placement.
fn split_top_bottom(dots: &DotBuffer) -> (DotBuffer, DotBuffer) {
    let mid = dots.rows() / 2;
    let mut top = DotBuffer::new(dots.cols(), dots.rows());
    let mut bottom = DotBuffer::new(dots.cols(), dots.rows());
    for row in 0..dots.rows() {
        for col in 0..dots.cols() {
            let dot = dots.get(col, row);
            if row < mid {
                top.set(col, row, dot);
            } else {
                bottom.set(col, row, dot);
            }
        }
    }
    (top, bottom)
}

/// A stationary copy of `egg` for phases that draw the base egg sprite
/// without the tray's built-in `Ready` bob (the hatch sequence drives its
/// own wiggle/positioning instead).
fn stationary_copy(egg: &Egg) -> Egg {
    let mut e = egg.clone();
    e.state = EggState::Incubating { started_at: SystemTime::now() };
    e
}

impl super::Hatchery {
    /// Single per-`update()` entry point: on the first tick with no sequence
    /// active, peeks `pending_hatch` and plays the `hatch_out` pre-reveal
    /// transition to completion, then launches a `HatchState` only once the
    /// requested egg's assets are fully generated (`hatch_assets_ready`) —
    /// otherwise the request stays recorded and the scene sits in the
    /// generating wait (deferred behind the animation) while the ordinary
    /// generation loop (`poll_definition`/`advance_hatch_clips`) keeps
    /// resolving it. Every tick after launch advances the active sequence's
    /// clock. A no-op when there is neither a pending request nor an active
    /// sequence.
    pub(super) fn advance_hatch(&mut self, dt: Duration) {
        if self.hatch.is_none() {
            let Some(idx) = self.pending_hatch else {
                return;
            };
            let Some(egg) = self.eggs.get(idx) else {
                self.pending_hatch = None;
                self.hatch_out = None;
                return;
            };

            // Play the pre-reveal hand-off animation exactly once, keyed to
            // the tapped egg.
            let ho = self.hatch_out.get_or_insert(HatchOut { egg: idx, elapsed: Duration::ZERO });
            if ho.egg != idx {
                *ho = HatchOut { egg: idx, elapsed: Duration::ZERO };
            }
            ho.elapsed += dt;
            if ho.elapsed < super::hatch::SLIDE_DURATION {
                return;
            }

            if !hatch_assets_ready(egg) {
                return;
            }
            self.pending_hatch = None;
            self.hatch = Some(HatchState::launch(idx, egg));
            self.hatch_out = None;
        }
        if let Some(h) = self.hatch.as_mut() {
            h.seq.advance(dt);
        }
    }

    /// Renders the wait shown while a `pending_hatch` request's egg has not
    /// yet fully generated: the back button (so a no-GPU wait is always
    /// escapable) plus a single centered "Generating..." line. No creature
    /// or egg pixels are drawn — the reveal begins only once
    /// `hatch_assets_ready` gates it open.
    pub(super) fn draw_hatch_generating(&self, frame: &mut Frame, area: Rect) {
        let dr = Self::back_dot_rect(area);
        let mut b = self.back_button.borrow_mut();
        b.set_rect(dr.to_cell_rect());
        crate::scenes::home_button::draw_badge_button(
            frame.buffer_mut(),
            dr,
            b.state(),
            crate::assets::ICON_ARROW_LEFT,
        );

        engine_render::label(
            frame.buffer_mut(),
            area,
            "Generating...",
            TextAlign::Center,
            Style::default().fg(Color::Rgb(0xff, 0xff, 0xff)),
        );
    }

    /// Renders the hatch-out pre-reveal transition (egg to center, panel
    /// off-right, dock suppressed) at `self.hatch_out`'s current elapsed
    /// time; once the transition's duration has elapsed but the egg's
    /// assets are still generating, falls back to the deferred generating
    /// wait. Only called once `self.hatch_out` is `Some`.
    pub(super) fn draw_hatch_out(&self, frame: &mut Frame, area: Rect) {
        let Some(ho) = self.hatch_out.as_ref() else { return };
        if ho.elapsed >= super::hatch::SLIDE_DURATION {
            self.draw_hatch_generating(frame, area);
            return;
        }
        let Some(egg) = self.eggs.get(ho.egg) else { return };
        let layout = super::browse_layout::browse_layout(area);
        let p = ho.elapsed.as_secs_f32() / super::hatch::SLIDE_DURATION.as_secs_f32();
        let pose = hatch_layout::hatch_out_pose(area, layout.egg, layout.panel, p);
        let buf = frame.buffer_mut();

        self.draw_browse_panel(buf, pose.panel, ho.egg);
        let art = self.art_cache.get(ho.egg).and_then(|a| a.as_ref());
        super::tray::draw_egg(buf, pose.egg, &stationary_copy(egg), art, Duration::ZERO);
    }

    /// Renders every hatch phase over the focused egg's rect. Only called
    /// once `self.hatch` is `Some`.
    pub(super) fn draw_hatch(&self, frame: &mut Frame, area: Rect) {
        let Some(h) = self.hatch.as_ref() else { return };
        let Some(egg) = self.eggs.get(h.egg) else { return };
        let buf = frame.buffer_mut();

        let (focus_dr, strip) = super::focus::focus_layout(area);
        let slots = super::tray::tray_slots(strip, self.eggs.len());
        for (i, slot) in slots.iter().enumerate() {
            if i == h.egg {
                continue;
            }
            super::tray::draw_egg(
                buf,
                *slot,
                &self.eggs[i],
                self.art_cache.get(i).and_then(|a| a.as_ref()),
                self.elapsed,
            );
        }

        let art = self.art_cache.get(h.egg).and_then(|a| a.as_ref());
        let w = focus_dr.w.max(0) as u32;
        let hh = focus_dr.h.max(0) as u32;

        match h.seq.phase() {
            HatchPhase::Wiggle => {
                let offset = h.seq.wiggle_offset_y();
                let target = DotRect { x: focus_dr.x, y: focus_dr.y + offset, w: focus_dr.w, h: focus_dr.h };
                super::tray::draw_egg(buf, target, &stationary_copy(egg), art, Duration::ZERO);
            }
            HatchPhase::Crack => {
                super::tray::draw_egg(buf, focus_dr, &stationary_copy(egg), art, Duration::ZERO);
                if let Some(crack) = &h.crack {
                    let n = crack.frame_count();
                    let i = h.seq.crack_frame(n);
                    let dots = crack_frame_dots(crack, i, w, hh);
                    crate::scenes::post_battle::columns::blit_dots(buf, focus_dr, &dots);
                }
            }
            HatchPhase::Break => {
                let Some(art) = art else { return };
                let raw = still_to_dots(art, w, hh);
                let dots = engine_render::dots::tint(&raw, crate::scenes::palette::element_color(egg.element));
                let gap = ((focus_dr.h as f32) * 0.2 * h.seq.phase_progress()).round() as i32;
                let (top, bottom) = split_top_bottom(&dots);
                let top_rect = DotRect { x: focus_dr.x, y: focus_dr.y - gap, w: focus_dr.w, h: focus_dr.h };
                let bottom_rect = DotRect { x: focus_dr.x, y: focus_dr.y + gap, w: focus_dr.w, h: focus_dr.h };
                crate::scenes::post_battle::columns::blit_dots(buf, top_rect, &top);
                crate::scenes::post_battle::columns::blit_dots(buf, bottom_rect, &bottom);
                if let Some(crack) = &h.crack {
                    let last = crack.frame_count().saturating_sub(1);
                    let dots = crack_frame_dots(crack, last, w, hh);
                    crate::scenes::post_battle::columns::blit_dots(buf, focus_dr, &dots);
                }
            }
            phase @ (HatchPhase::RevealFlash | HatchPhase::RevealColor) => {
                let Some(art) = art else { return };
                let t = if phase == HatchPhase::RevealFlash { 0.0 } else { h.seq.phase_progress() };
                let dots = reveal_recolor(&still_to_dots(art, w, hh), t);
                crate::scenes::post_battle::columns::blit_dots(buf, focus_dr, &dots);
            }
            HatchPhase::Beat => {
                if let Some(idle) = h.idle.as_ref() {
                    let dots = idle.dots_at(self.elapsed, w, hh);
                    crate::scenes::post_battle::columns::blit_dots(buf, focus_dr, &dots);
                }

                if let Some(hatchling) = &egg.hatchling {
                    let t = h.seq.phase_progress();
                    let l = (t * 255.0).round().clamp(0.0, 255.0) as u8;
                    engine_render::wrapped_text(
                        buf,
                        name_rect(focus_dr),
                        &hatchling.name,
                        TextAlign::Center,
                        Style::default().fg(Color::Rgb(l, l, l)),
                        true,
                    );
                }
            }
            phase @ (HatchPhase::Slide | HatchPhase::Done) => {
                let Some(hatchling) = &egg.hatchling else { return };
                let p = if phase == HatchPhase::Done { 1.0 } else { h.seq.phase_progress() };

                let nr = name_rect(focus_dr);
                let name_start = DotRect {
                    x: nr.x as i32 * 2,
                    y: nr.y as i32 * 4,
                    w: nr.width as i32 * 2,
                    h: nr.height as i32 * 4,
                };
                let pose = hatch_layout::slide_pose(area, strip, &hatchling.name, focus_dr, name_start, p);

                if let Some(idle) = h.idle.as_ref() {
                    let cw = pose.creature.w.max(0) as u32;
                    let ch = pose.creature.h.max(0) as u32;
                    let dots = idle.dots_at(self.elapsed, cw, ch);
                    crate::scenes::post_battle::columns::blit_dots(buf, pose.creature, &dots);
                }

                engine_render::wrapped_text(
                    buf,
                    pose.name_zone.to_cell_rect(),
                    &hatchling.name,
                    TextAlign::Center,
                    Style::default().fg(Color::Rgb(0xff, 0xff, 0xff)),
                    true,
                );

                draw_stats_dock(buf, pose.dock_border, hatchling);
                draw_hatch_stat_bars(buf, pose.stat_bars, hatchling, p);
            }
        }
    }

    /// Renders the empty-dock settled view: back button, the retained
    /// creature idling read-only in the settled left slot, its name, its
    /// stat bars at full undimmed opacity, and the shared stamina/abilities
    /// dock — the settled Done frame held indefinitely, with no Keep/
    /// Discard and no action button. Only called once `self.settled` is
    /// `Some`.
    pub(super) fn draw_empty_dock(&self, frame: &mut Frame, area: Rect) {
        let Some(settled) = self.settled.as_ref() else { return };
        let dr = Self::back_dot_rect(area);
        let mut b = self.back_button.borrow_mut();
        b.set_rect(dr.to_cell_rect());
        crate::scenes::home_button::draw_badge_button(
            frame.buffer_mut(),
            dr,
            b.state(),
            crate::assets::ICON_ARROW_LEFT,
        );

        let (_focus_dr, strip) = super::focus::focus_layout(area);
        let l = hatch_layout::settled_layout(area, strip, &settled.creature.name);
        let buf = frame.buffer_mut();

        if let Some(idle) = settled.idle.as_ref() {
            let w = l.creature.w.max(0) as u32;
            let h = l.creature.h.max(0) as u32;
            let dots = idle.dots_at(self.elapsed, w, h);
            crate::scenes::post_battle::columns::blit_dots(buf, l.creature, &dots);
        }

        engine_render::wrapped_text(
            buf,
            l.name_zone.to_cell_rect(),
            &settled.creature.name,
            TextAlign::Center,
            Style::default().fg(Color::Rgb(0xff, 0xff, 0xff)),
            true,
        );

        draw_stats_dock(buf, l.dock_border, &settled.creature);
        draw_hatch_stat_bars(buf, l.stat_bars, &settled.creature, 1.0);
    }
}

/// Paints the settled-placement stats dock (border + shared stamina/
/// abilities body) at `border`: clears the dock's cells first so the
/// scene's background fill never bleeds through the border chrome or the
/// gaps the shared body leaves unpainted between its label/bar and ability
/// cells, then draws the rounded border and the shared `detail_panel` body
/// — the sole dock-render site, called for both the sliding-in dock and its
/// settled resting frame.
fn draw_stats_dock(buf: &mut ratatui::buffer::Buffer, border: DotRect, hatchling: &crate::player_data::PersistedCreature) {
    // `border` slides in from fully off the right edge during the Slide
    // phase, so its cell rect can extend past the buffer's bounds; `Clear`
    // indexes the buffer directly and panics on an out-of-bounds cell, so
    // clip to the buffer's own area first (a zero-area clip is a no-op).
    let clip = border.to_cell_rect().intersection(buf.area);
    ratatui::widgets::Widget::render(ratatui::widgets::Clear, clip, buf);

    let dock_dots = ui_primitives::rounded_rect(
        border.w.max(0) as usize,
        border.h.max(0) as usize,
        1,
        1,
        DOCK_BORDER_COLOR,
        Dot::Transparent,
    );
    crate::scenes::post_battle::columns::blit_dots(buf, border, &dock_dots);

    let regions = detail_panel::interior_regions(border);
    detail_panel::render_stamina_row(buf, regions.stamina, &hatchling.stamina);
    detail_panel::render_abilities(buf, regions.abilities_header, regions.ability_cells, &hatchling.abilities);
}

/// Paints `hatchling`'s 4 stat bars into `band` at `opacity` — the sole
/// hatchery stat-bar render site, forwarding to the shared
/// `stat_bar::draw_stat_bars` renderer with the hatchling's own stats (via
/// the shared `stat_bar::stat_fill_scaled` scale) and the dock's grey chrome.
/// Called from the Slide/Done arm at `opacity = p` (the settle's fade
/// progress); the empty-dock view reuses this same helper at opacity 1.0
/// rather than invoking `draw_stat_bars` a second time. A no-op at
/// `opacity <= 0.0`: `draw_stat_bars` paints its border/fill GLYPH shape
/// unconditionally regardless of alpha (only the color fades), which would
/// otherwise stamp a fully-transparent ghost outline over cells this band
/// has never touched before the fade begins.
fn draw_hatch_stat_bars(buf: &mut ratatui::buffer::Buffer, band: DotRect, hatchling: &PersistedCreature, opacity: f32) {
    if opacity <= 0.0 {
        return;
    }
    crate::scenes::stat_bar::draw_stat_bars(
        buf,
        band,
        |kind, cols| crate::scenes::stat_bar::stat_fill_scaled(hatchling.stats.value(kind), cols).round() as usize,
        opacity,
        HATCH_STAT_CHROME,
    );
}
