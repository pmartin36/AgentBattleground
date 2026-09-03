//! The single owner of per-egg tray placement and the "draw one egg into a
//! target `DotRect`" helper: `Undefined` renders the bundled `?` sprite
//! untinted, `Incubating`/`Ready` render the egg's own art multiply-tinted by
//! its element color, and `Ready` additionally bobs vertically. Every
//! non-text egg visual goes through the braille dot pipeline
//! (`sprite_to_dots` -> `tint` -> `blit_dots`), with the target `DotRect`
//! threaded unfloored.

use std::time::Duration;

use engine_core::color::Rgba;
use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use engine_render::composite::{composite_dots, DotPlacement};
use engine_render::dots::{Dot, DotBuffer};
use engine_render::DotRect;

use crate::player_data::{Egg, EggState};

/// Egg slot width in dots — aspect matches `egg_unknown.png` (220x280, 11:14).
pub(crate) const EGG_SLOT_W_DOTS: i32 = 22;
/// Egg slot height in dots — see [`EGG_SLOT_W_DOTS`].
pub(crate) const EGG_SLOT_H_DOTS: i32 = 28;
/// Horizontal gap between adjacent tray slots, in dots.
const EGG_GAP_DOTS: i32 = 8;
/// Full period of the `Ready` egg's idle vertical bob.
const WIGGLE_PERIOD: Duration = Duration::from_millis(500);
/// Peak vertical bob amplitude, in dots.
const WIGGLE_AMP_DOTS: i32 = 2;

/// The resting tray band: a strip along the bottom of `area` ~15% of its
/// height tall, where the tray lays its eggs out. Anchoring the eggs to a
/// short bottom band (rather than the vertical center) keeps the play area
/// above it clear.
pub(crate) fn tray_band(area: Rect) -> Rect {
    let band_h = (area.height * 3 / 20).max(3);
    let margin = 1;
    let bottom = area.y + area.height.saturating_sub(margin);
    Rect {
        x: area.x,
        y: bottom.saturating_sub(band_h),
        width: area.width,
        height: band_h,
    }
}

/// The single owner of per-egg tray placement and hit-rects: one unfloored
/// `DotRect` per egg, a centered horizontal row that fills the given `area`
/// band vertically (small padding), preserving the egg sprite's 11:14 aspect.
/// Callers pass a short band (`tray_band` for the resting tray, the focus
/// strip while an egg is focused), so the eggs size themselves to it.
pub(crate) fn tray_slots(area: Rect, count: usize) -> Vec<DotRect> {
    let ax = area.x as i32 * 2;
    let ay = area.y as i32 * 4;
    let aw = area.width as i32 * 2;
    let ah = area.height as i32 * 4;

    let pad = (ah / 14).max(1);
    let slot_h = (ah - 2 * pad).max(1);
    let slot_w = (slot_h * EGG_SLOT_W_DOTS / EGG_SLOT_H_DOTS).max(1);
    let slot_y = ay + (ah - slot_h) / 2;

    let count_i32 = count as i32;
    let total_w = count_i32 * slot_w + (count_i32 - 1).max(0) * EGG_GAP_DOTS;
    let start_x = ax + (aw - total_w) / 2;

    (0..count)
        .map(|i| DotRect {
            x: start_x + i as i32 * (slot_w + EGG_GAP_DOTS),
            y: slot_y,
            w: slot_w,
            h: slot_h,
        })
        .collect()
}

/// Builds the raw egg-dots for a `w`×`h` render: `Undefined` always the
/// bundled `EGG_UNKNOWN` sprite untinted (so its bright-yellow `?` survives
/// per-cell color averaging); `Incubating`/`Ready` render `art` resized to
/// the target and multiply-tinted by the egg's element color, falling back
/// to an untinted `EGG_UNKNOWN` placeholder when `art` is `None`. The sole
/// owner of egg-sprite construction, shared by [`draw_egg`] and
/// [`draw_egg_with_highlight`] so neither re-derives the sprite/tint logic.
fn egg_dots(egg: &Egg, w: u32, h: u32, art: Option<&DynamicImage>) -> DotBuffer {
    match egg.state {
        EggState::Undefined => engine_render::asset_cache::sprite_to_dots(crate::assets::EGG_UNKNOWN, w, h),
        EggState::Incubating { .. } | EggState::Ready => match art {
            Some(img) => {
                let raw = engine_render::dots::sprite_to_dots(img, w, h);
                engine_render::dots::tint(&raw, crate::scenes::palette::element_color(egg.element))
            }
            None => engine_render::asset_cache::sprite_to_dots(crate::assets::EGG_UNKNOWN, w, h),
        },
    }
}

/// Draws one egg into `target`. `art` is the egg's pre-decoded `egg_art`
/// (`None` if the egg has none, or it failed to decode). See [`egg_dots`]
/// for the per-state sprite/tint rules. `Ready` eggs additionally bob
/// vertically by [`wiggle_offset_y`] of `elapsed`; every other state is
/// stationary.
pub(crate) fn draw_egg(
    buf: &mut Buffer,
    target: DotRect,
    egg: &Egg,
    art: Option<&DynamicImage>,
    elapsed: Duration,
) {
    let (w, h) = (target.w.max(0) as u32, target.h.max(0) as u32);
    if w == 0 || h == 0 {
        return;
    }

    let dots = egg_dots(egg, w, h, art);

    let dy = if matches!(egg.state, EggState::Ready) { wiggle_offset_y(elapsed) } else { 0 };
    let placed = DotRect { x: target.x, y: target.y + dy, w: target.w, h: target.h };
    crate::scenes::post_battle::columns::blit_dots(buf, placed, &dots);
}


/// Visual treatment for a tray egg slot. `Idle` is the plain render;
/// `Hovered`/`Selected` must each decode to a treatment distinguishable from
/// `Idle` and from each other — see `draw_egg_with_highlight`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayHighlight {
    Idle,
    Hovered,
    Selected,
}

/// Dot margin the highlight ring is outset beyond the egg's own rect.
const HILITE_MARGIN_DOTS: i32 = 3;
/// Chamfer radius of the highlight ring's corners, in dots.
const HILITE_RADIUS: usize = 2;

/// The ring color + thickness for `hl`. The ONE source of ring
/// color/thickness; `Idle` never reaches here (see
/// [`draw_egg_with_highlight`]'s early return).
fn highlight_style(hl: TrayHighlight) -> (Rgba, usize) {
    match hl {
        TrayHighlight::Idle => (Rgba::rgb(0, 0, 0), 0),
        TrayHighlight::Hovered => (Rgba::rgb(0xC8, 0xD0, 0xE0), 1),
        TrayHighlight::Selected => (Rgba::rgb(0xFF, 0xC8, 0x40), 2),
    }
}

/// Draws one tray egg with `hl`'s highlight treatment through the braille
/// dot pipeline. `Idle` renders identically to [`draw_egg`]; `Hovered` and
/// `Selected` each rasterize a [`highlight_style`]-colored ring
/// ([`engine_render::rounded_rect`]) outset by [`HILITE_MARGIN_DOTS`],
/// composite it under the egg's own dots into one buffer (the
/// `post_battle/glow.rs` ring-under-content pattern, so ring and egg survive
/// in any braille cell they share), and blit that single buffer once.
/// Highlighted eggs do not bob — the ring marks position, and compositing
/// against a moving egg would complicate the ring math for no visible gain.
pub(crate) fn draw_egg_with_highlight(
    buf: &mut Buffer,
    target: DotRect,
    egg: &Egg,
    art: Option<&DynamicImage>,
    elapsed: Duration,
    hl: TrayHighlight,
) {
    if hl == TrayHighlight::Idle {
        draw_egg(buf, target, egg, art, elapsed);
        return;
    }

    let (w, h) = (target.w.max(0) as u32, target.h.max(0) as u32);
    if w == 0 || h == 0 {
        return;
    }

    let egg_buf = egg_dots(egg, w, h, art);
    let m = HILITE_MARGIN_DOTS;
    let (color, thickness) = highlight_style(hl);
    let ring_w = (w as i32 + 2 * m).max(0) as usize;
    let ring_h = (h as i32 + 2 * m).max(0) as usize;
    let ring_buf = engine_render::rounded_rect(ring_w, ring_h, thickness, HILITE_RADIUS, color, Dot::Transparent);
    let combined = composite_dots(
        ring_w,
        ring_h,
        &[
            DotPlacement { dots: &ring_buf, dot_x: 0, dot_y: 0, depth: 0 },
            DotPlacement { dots: &egg_buf, dot_x: m, dot_y: m, depth: 1 },
        ],
    );
    let outer = target.inset(-m, -m, -m, -m);
    crate::scenes::post_battle::columns::blit_dots(buf, outer, &combined);
}

/// Vertical dot offset of a `Ready` egg's idle bob at `elapsed`:
/// `round(WIGGLE_AMP_DOTS * sin(TAU * elapsed / WIGGLE_PERIOD))`. Zero at
/// `elapsed == Duration::ZERO`.
pub(crate) fn wiggle_offset_y(elapsed: Duration) -> i32 {
    let phase = elapsed.as_secs_f64() / WIGGLE_PERIOD.as_secs_f64() * std::f64::consts::TAU;
    (WIGGLE_AMP_DOTS as f64 * phase.sin()).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::Element;
    use crate::player_data::EggState;
    use engine_render::decode_braille_cell;
    use image::{Rgba as ImageRgba, RgbaImage};

    fn undefined_egg() -> Egg {
        Egg {
            element: Element::Fire,
            state: EggState::Undefined,
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        }
    }

    fn incubating_egg(element: Element) -> Egg {
        Egg {
            element,
            state: EggState::Incubating { started_at: std::time::SystemTime::now() },
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        }
    }

    fn ready_egg() -> Egg {
        Egg {
            element: Element::Fire,
            state: EggState::Ready,
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        }
    }

    fn white_art() -> DynamicImage {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, ImageRgba([255, 255, 255, 255])))
    }

    /// A dot lit anywhere in `buf` whose blended color satisfies `pred`.
    fn any_lit_dot_matching(buf: &Buffer, area: Rect, pred: impl Fn(u8, u8, u8) -> bool) -> bool {
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                if let Some((_, color)) = decode_braille_cell(buf, x, y) {
                    if pred(color.r, color.g, color.b) {
                        return true;
                    }
                }
            }
        }
        false
    }

    const SLOT: Rect = Rect { x: 0, y: 0, width: 12, height: 8 };
    fn slot_dot_rect() -> DotRect {
        DotRect { x: 0, y: 0, w: EGG_SLOT_W_DOTS, h: EGG_SLOT_H_DOTS }
    }

    /// The tray lays out one slot per egg.
    #[test]
    fn tray_slots_returns_one_rect_per_egg() {
        let area = Rect::new(0, 0, 60, 20);
        let slots = tray_slots(area, 3);
        assert_eq!(slots.len(), 3);
    }

    /// Slots are ordered left-to-right and never overlap.
    #[test]
    fn tray_slots_are_ordered_and_non_overlapping() {
        let area = Rect::new(0, 0, 60, 20);
        let slots = tray_slots(area, 3);
        for pair in slots.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(a.x < b.x, "slots must be ordered left-to-right: {a:?} then {b:?}");
            assert!(a.x + a.w <= b.x, "slots must not overlap: {a:?} and {b:?}");
        }
    }

    /// The bob is zero at `elapsed == 0`.
    #[test]
    fn wiggle_offset_y_is_zero_at_start() {
        assert_eq!(wiggle_offset_y(Duration::ZERO), 0);
    }

    /// A quarter period in, the bob is at its peak amplitude.
    #[test]
    fn wiggle_offset_y_peaks_at_quarter_period() {
        assert_eq!(wiggle_offset_y(WIGGLE_PERIOD / 4), WIGGLE_AMP_DOTS);
    }

    /// An `Undefined` egg renders the bundled `?` sprite's bright yellow,
    /// untinted (multiplying it by any element color would darken it away
    /// from this exact hue).
    #[test]
    fn draw_egg_undefined_renders_untinted_bright_yellow() {
        let mut buf = Buffer::empty(SLOT);
        draw_egg(&mut buf, slot_dot_rect(), &undefined_egg(), None, Duration::ZERO);

        let is_bright_yellow = |r: u8, g: u8, b: u8| r > 200 && g > 150 && b < 50;
        assert!(
            any_lit_dot_matching(&buf, SLOT, is_bright_yellow),
            "expected a bright-yellow lit dot for the undefined egg's `?`"
        );
    }

    /// A multiply-tint of fully-white art against an element color reproduces
    /// that element color exactly (255 * c / 255 == c) — the Fire egg must
    /// carry the Fire hue and must NOT carry the Normal (grey) hue.
    #[test]
    fn draw_egg_incubating_tints_white_art_by_element_color() {
        let art = white_art();

        let mut fire_buf = Buffer::empty(SLOT);
        draw_egg(&mut fire_buf, slot_dot_rect(), &incubating_egg(Element::Fire), Some(&art), Duration::ZERO);

        let fire = crate::scenes::palette::element_color(Element::Fire);
        let is_fire = |r: u8, g: u8, b: u8| r == fire.r && g == fire.g && b == fire.b;
        assert!(
            any_lit_dot_matching(&fire_buf, SLOT, is_fire),
            "expected the Fire egg's tinted dots to carry element_color(Fire) exactly"
        );

        let mut normal_buf = Buffer::empty(SLOT);
        draw_egg(&mut normal_buf, slot_dot_rect(), &incubating_egg(Element::Normal), Some(&art), Duration::ZERO);
        assert!(
            !any_lit_dot_matching(&normal_buf, SLOT, is_fire),
            "a Normal egg must not carry the Fire hue"
        );
    }

    /// A `Ready` egg's render changes between two `elapsed` values a quarter
    /// period apart (the idle bob); an `Incubating` egg's render at the same
    /// two `elapsed` values is identical (stationary).
    #[test]
    fn draw_egg_ready_bobs_while_incubating_stays_stationary() {
        let render_at = |egg: &Egg, elapsed: Duration| -> Buffer {
            let mut buf = Buffer::empty(SLOT);
            draw_egg(&mut buf, slot_dot_rect(), egg, None, elapsed);
            buf
        };
        let serialize = crate::scenes::test_util::serialize_braille_buffer;

        let ready = ready_egg();
        let ready_a = serialize(&render_at(&ready, Duration::ZERO));
        let ready_b = serialize(&render_at(&ready, WIGGLE_PERIOD / 4));
        assert_ne!(ready_a, ready_b, "a Ready egg's render must change across elapsed (bob)");

        let incubating = incubating_egg(Element::Fire);
        let incubating_a = serialize(&render_at(&incubating, Duration::ZERO));
        let incubating_b = serialize(&render_at(&incubating, WIGGLE_PERIOD / 4));
        assert_eq!(incubating_a, incubating_b, "an Incubating egg's render must be stationary across elapsed");
    }

    /// Renders `egg` with `hl` into a fresh buffer and serializes it, for
    /// comparing highlight treatments dot-for-dot.
    fn render_highlight(hl: TrayHighlight) -> String {
        let mut buf = Buffer::empty(SLOT);
        draw_egg_with_highlight(&mut buf, slot_dot_rect(), &incubating_egg(Element::Fire), None, Duration::ZERO, hl);
        crate::scenes::test_util::serialize_braille_buffer(&buf)
    }

    /// A hovered tray egg must decode differently from an idle one.
    #[test]
    fn hovered_highlight_renders_differently_from_idle() {
        assert_ne!(
            render_highlight(TrayHighlight::Idle),
            render_highlight(TrayHighlight::Hovered),
            "a hovered tray egg must render differently from an idle one"
        );
    }

    /// A selected tray egg must decode differently from an idle one.
    #[test]
    fn selected_highlight_renders_differently_from_idle() {
        assert_ne!(
            render_highlight(TrayHighlight::Idle),
            render_highlight(TrayHighlight::Selected),
            "a selected tray egg must render differently from an idle one"
        );
    }

    /// Hovered and selected must decode differently from each other, so a
    /// player can tell the two apart on the same tray.
    #[test]
    fn hovered_and_selected_highlights_render_differently_from_each_other() {
        assert_ne!(
            render_highlight(TrayHighlight::Hovered),
            render_highlight(TrayHighlight::Selected),
            "hovered and selected must render differently from each other"
        );
    }
}
