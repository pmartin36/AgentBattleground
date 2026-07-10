//! Selection glow ring (b4-t5): a 1-dot pulsing ring drawn one dot outside
//! the selected column's portrait frame perimeter. `glow_color` is the pure,
//! quantized source of the pulse color for both `frame_with_ring` and its
//! tests (the `columns.rs` "pure helper is the sole source" convention).
//! Only `scene.selected_index` gets a ring; geometry reuses `columns::
//! column_layouts`'s `frame` verbatim — see research.md.
//!
//! `frame_with_ring` composes the ring with the caller's already-built frame
//! `DotBuffer` into ONE buffer (via `engine_render::composite::composite_dots`)
//! so both sets of lit dots survive in braille cells the ring and frame
//! share — a separate `blit_dots` call per layer would full-cell-replace
//! whichever drew second (see research.md's iteration-2 diagnosis).

use std::time::Duration;

use engine_core::color::Rgba;
use engine_render::composite::{composite_dots, DotPlacement};
use engine_render::dots::DotBuffer;
use engine_render::DotRect;

use super::columns;
use super::PostBattle;

/// (b4-t5) Pure, quantized pulse color at `elapsed`: `PostBattle::
/// GLOW_PERIOD`-periodic, stepped into `PostBattle::GLOW_STEPS` discrete
/// levels so it reads as stepped rather than perfectly smooth. Sole source of
/// the ring color for both `render` and tests.
pub(super) fn glow_color(elapsed: Duration) -> Rgba {
    let period = PostBattle::GLOW_PERIOD.as_secs_f32();
    let steps = PostBattle::GLOW_STEPS;
    let phase = elapsed.as_secs_f32().rem_euclid(period) / period;
    let step = ((phase * steps as f32).floor() as u32).min(steps - 1);
    let half = (steps - 1) as f32 / 2.0;
    let t = 1.0 - (step as f32 - half).abs() / half;
    PostBattle::GLOW_DIM_COLOR.lerp(PostBattle::GLOW_BRIGHT_COLOR, t)
}

/// Outsets `frame` by `columns::GLOW_MARGIN_DOTS`, rasterizes a 1-dot
/// chamfered ring there (colored `glow_color(elapsed)`), and composites the
/// caller's already-built `frame_buf` ON TOP of the ring (via
/// `composite_dots`, which copies only `Dot::Lit` source dots) so the ring's
/// outer perimeter and the frame's own border coexist in any braille cell
/// they share. Returns `(ring_rect, combined)` — the caller blits this ONE
/// buffer, once, instead of two separate full-cell-replacing blits.
///
/// Degenerate (`ring.w <= 0 || ring.h <= 0`) returns `(frame, frame_buf)`
/// unchanged, no clone.
pub(super) fn frame_with_ring(
    frame: DotRect,
    frame_buf: DotBuffer,
    elapsed: Duration,
) -> (DotRect, DotBuffer) {
    let m = columns::GLOW_MARGIN_DOTS;
    let ring = frame.inset(-m, -m, -m, -m);
    if ring.w <= 0 || ring.h <= 0 {
        return (frame, frame_buf);
    }
    let ring_buf = engine_render::rounded_rect(
        ring.w as usize,
        ring.h as usize,
        columns::FRAME_THICKNESS,
        columns::FRAME_RADIUS + columns::GLOW_MARGIN_DOTS as usize,
        glow_color(elapsed),
        engine_render::dots::Dot::Transparent,
    );
    let combined = composite_dots(
        ring.w as usize,
        ring.h as usize,
        &[
            DotPlacement { dots: &ring_buf, dot_x: 0, dot_y: 0, depth: 0 },
            DotPlacement { dots: &frame_buf, dot_x: m, dot_y: m, depth: 1 },
        ],
    );
    (ring, combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `glow_color` is quantized: two elapsed values inside the same
    /// (`GLOW_PERIOD` / `GLOW_STEPS`) step decode to the identical color.
    #[test]
    fn glow_color_is_quantized_within_a_step() {
        assert_eq!(
            glow_color(Duration::ZERO),
            glow_color(Duration::from_millis(100)),
            "elapsed=0 and elapsed=100ms must fall in the same quantized step"
        );
    }

    /// `glow_color` pulses: a half-period apart lands in a different step
    /// (dim vs bright), so the colors must differ.
    #[test]
    fn glow_color_pulses_across_half_period() {
        assert_ne!(
            glow_color(Duration::ZERO),
            glow_color(PostBattle::GLOW_PERIOD / 2),
            "glow_color at elapsed=0 and elapsed=GLOW_PERIOD/2 must differ, proving the pulse"
        );
    }

    // NOTE (b4-t5 iter3): the two former tests here
    // (`glow_ring_present_on_selected_column_absent_on_others`,
    // `glow_ring_color_changes_across_elapsed`) exercised `render` in
    // isolation (a separate blit into an otherwise-empty buffer). Per this
    // iteration's research.md, `render` is deleted and replaced by
    // `frame_with_ring`, which composes the ring with the caller's frame
    // buffer — the isolated-`render` contract these tests asserted no longer
    // applies once composition is mandatory. The equivalent behavior (ring
    // present on column 0 / absent on 1..=3, pulsing color) is now covered
    // through the FULLY COMPOSED `Scene::render` in `post_battle::tests`:
    // `composed_render_shows_ring_and_intact_frame_on_selected_column`,
    // `composed_render_no_ring_on_unselected_columns`,
    // `composed_render_ring_color_pulses_across_elapsed` — the exact seam
    // where the ring/frame shared-cell collision actually manifests.
}
