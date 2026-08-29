//! The single owner of per-egg tray placement and the "draw one egg into a
//! target `DotRect`" helper: `Undefined` renders the bundled `?` sprite
//! untinted, `Incubating`/`Ready` render the egg's own art multiply-tinted by
//! its element color, and `Ready` additionally bobs vertically. Every
//! non-text egg visual goes through the braille dot pipeline
//! (`sprite_to_dots` -> `tint` -> `blit_dots`), with the target `DotRect`
//! threaded unfloored.

use std::time::Duration;

use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

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

/// The single owner of per-egg tray placement and hit-rects: one unfloored
/// `DotRect` per egg, a centered horizontal row, vertically centered in
/// `area`. Every returned rect is `EGG_SLOT_W_DOTS` x `EGG_SLOT_H_DOTS`.
pub(crate) fn tray_slots(area: Rect, count: usize) -> Vec<DotRect> {
    let ax = area.x as i32 * 2;
    let ay = area.y as i32 * 4;
    let aw = area.width as i32 * 2;
    let ah = area.height as i32 * 4;

    let count_i32 = count as i32;
    let total_w = count_i32 * EGG_SLOT_W_DOTS + (count_i32 - 1).max(0) * EGG_GAP_DOTS;
    let start_x = ax + (aw - total_w) / 2;
    let slot_y = ay + (ah - EGG_SLOT_H_DOTS) / 2;

    (0..count)
        .map(|i| DotRect {
            x: start_x + i as i32 * (EGG_SLOT_W_DOTS + EGG_GAP_DOTS),
            y: slot_y,
            w: EGG_SLOT_W_DOTS,
            h: EGG_SLOT_H_DOTS,
        })
        .collect()
}

/// Draws one egg into `target`. `art` is the egg's pre-decoded `egg_art`
/// (`None` if the egg has none, or it failed to decode). `Undefined` always
/// renders the bundled `EGG_UNKNOWN` sprite untinted (so its bright-yellow
/// `?` survives per-cell color averaging); `Incubating`/`Ready` render `art`
/// resized to the slot and multiply-tinted by the egg's element color,
/// falling back to an untinted `EGG_UNKNOWN` placeholder when `art` is
/// `None`. `Ready` eggs additionally bob vertically by
/// [`wiggle_offset_y`] of `elapsed`; every other state is stationary.
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

    let dots = match egg.state {
        EggState::Undefined => engine_render::asset_cache::sprite_to_dots(crate::assets::EGG_UNKNOWN, w, h),
        EggState::Incubating { .. } | EggState::Ready => match art {
            Some(img) => {
                let raw = engine_render::dots::sprite_to_dots(img, w, h);
                engine_render::dots::tint(&raw, crate::scenes::palette::element_color(egg.element))
            }
            None => engine_render::asset_cache::sprite_to_dots(crate::assets::EGG_UNKNOWN, w, h),
        },
    };

    let dy = if matches!(egg.state, EggState::Ready) { wiggle_offset_y(elapsed) } else { 0 };
    let placed = DotRect { x: target.x, y: target.y + dy, w: target.w, h: target.h };
    crate::scenes::post_battle::columns::blit_dots(buf, placed, &dots);
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
}
