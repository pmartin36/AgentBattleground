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

use crate::player_data::{resolve_clip, Egg, EggState};
use crate::scenes::detail_panel;

use super::hatch::HatchPhase;
use super::hatch_layout;

/// Settled dock border color — the same grey as the roster details panel's
/// own chrome.
const DOCK_BORDER_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0x88, 0x88, 0x88);

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

impl HatchState {
    /// Decodes the shared crack overlay once and resolves the hatchling's
    /// idle/attack clips (falling back to `None`, never panicking, when a
    /// clip is unresolved), building a fresh sequence at `elapsed == 0`.
    fn launch(egg_index: usize, egg: &Egg) -> Self {
        let crack = AnimatedSprite::from_gif(crate::assets::EGG_CRACK, crate::creatures::FRAME_DUR).ok();
        let (idle, attack) = match &egg.hatchling {
            Some(hatchling) => (
                hatchling.idle.as_ref().and_then(resolve_clip),
                hatchling.attack.as_ref().and_then(resolve_clip),
            ),
            None => (None, None),
        };
        Self { egg: egg_index, seq: super::hatch::HatchSequence::new(), crack, idle, attack }
    }
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
    /// active, peeks `pending_hatch` and launches a `HatchState` only once
    /// the requested egg's assets are fully generated
    /// (`hatch_assets_ready`) — otherwise the request stays recorded and the
    /// scene sits in the generating wait while the ordinary generation loop
    /// (`poll_definition`/`advance_hatch_clips`) keeps resolving it. Every
    /// tick after launch advances the active sequence's clock. A no-op when
    /// there is neither a pending request nor an active sequence.
    pub(super) fn advance_hatch(&mut self, dt: Duration) {
        if self.hatch.is_none() {
            let Some(idx) = self.pending_hatch else {
                return;
            };
            let Some(egg) = self.eggs.get(idx) else {
                self.pending_hatch = None;
                return;
            };
            if !hatch_assets_ready(egg) {
                return;
            }
            self.pending_hatch = None;
            self.hatch = Some(HatchState::launch(idx, egg));
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
            super::tray::draw_unfilled_sentence(buf, *slot, &self.eggs[i], i);
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
            }
        }
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
