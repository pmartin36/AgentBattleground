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
use engine_render::{AnimatedSprite, DotRect, TextAlign};

use crate::player_data::{resolve_clip, Egg, EggState};

use super::hatch::HatchPhase;

/// The launched-and-in-progress hatch sequence for one egg: the pure
/// timeline plus the sprites its render needs, decoded once at launch.
pub(crate) struct HatchState {
    pub(super) egg: usize,
    /// Read by `handle_input`'s non-interruptible guard.
    pub(super) seq: super::hatch::HatchSequence,
    crack: Option<AnimatedSprite>,
    idle: Option<AnimatedSprite>,
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

/// Rasterizes `art` (the egg's decoded still), falling back to the bundled
/// `EGG_UNKNOWN` placeholder when there is no still — the same fallback
/// `tray::draw_egg` uses for art-less eggs.
fn still_dots(art: Option<&DynamicImage>, w: u32, h: u32) -> DotBuffer {
    match art {
        Some(img) => engine_render::dots::sprite_to_dots(img, w, h),
        None => engine_render::asset_cache::sprite_to_dots(crate::assets::EGG_UNKNOWN, w, h),
    }
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
    /// active, consumes `pending_hatch` and launches a `HatchState`; every
    /// tick after that advances the active sequence's clock. A no-op when
    /// there is neither a pending request nor an active sequence.
    pub(super) fn advance_hatch(&mut self, dt: Duration) {
        if self.hatch.is_none() {
            let Some(idx) = self.take_hatch_request() else {
                return;
            };
            let Some(egg) = self.eggs.get(idx) else {
                return;
            };
            self.hatch = Some(HatchState::launch(idx, egg));
        }
        if let Some(h) = self.hatch.as_mut() {
            h.seq.advance(dt);
        }
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
                let raw = match art {
                    Some(img) => engine_render::dots::sprite_to_dots(img, w, hh),
                    None => engine_render::asset_cache::sprite_to_dots(crate::assets::EGG_UNKNOWN, w, hh),
                };
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
                let t = if phase == HatchPhase::RevealFlash { 0.0 } else { h.seq.phase_progress() };
                let dots = reveal_recolor(&still_dots(art, w, hh), t);
                crate::scenes::post_battle::columns::blit_dots(buf, focus_dr, &dots);
            }
            phase @ (HatchPhase::Name | HatchPhase::Idle | HatchPhase::Attack | HatchPhase::Done) => {
                let dots = match phase {
                    HatchPhase::Attack => h.attack.as_ref().map(|s| s.dots_at(self.elapsed, w, hh)),
                    HatchPhase::Idle | HatchPhase::Done => h.idle.as_ref().map(|s| s.dots_at(self.elapsed, w, hh)),
                    _ => None,
                }
                .unwrap_or_else(|| still_dots(art, w, hh));
                crate::scenes::post_battle::columns::blit_dots(buf, focus_dr, &dots);

                if let Some(hatchling) = &egg.hatchling {
                    let cell = focus_dr.to_cell_rect();
                    let width = cell.width.saturating_add(6);
                    let height = 3;
                    let x = (cell.x + cell.width / 2).saturating_sub(width / 2);
                    let y = cell.y + cell.height;
                    engine_render::wrapped_text(
                        buf,
                        Rect { x, y, width, height },
                        &hatchling.name,
                        TextAlign::Center,
                        Style::default().fg(Color::Rgb(0xff, 0xff, 0xff)),
                        true,
                    );

                    super::hatch_stats::draw_stats_panel(buf, area, focus_dr, hatchling);
                }
            }
        }
    }
}
