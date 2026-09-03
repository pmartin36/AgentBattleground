//! Pure hatch-sequence timeline: phase selection, crack burst/hold cadence,
//! and phase-1 wiggle motion. No rendering, no scene state, no gif decoding —
//! every query is a pure function of elapsed time so the whole timeline is
//! unit-testable without stepping a scene.
//!
//! The scene wires this module's `HatchSequence` into `update`/`render`;
//! nothing outside this file's tests constructs it yet, so the module allows
//! dead code until that wiring lands.
#![allow(dead_code)]

use std::time::Duration;

/// Escalating, non-interruptible progression a hatch sequence walks through.
/// Phase selection is monotonic in `elapsed` by construction: it is a walk
/// over cumulative duration thresholds, so ordinal never decreases as time
/// advances and there is no input path back into an earlier phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HatchPhase {
    Wiggle,
    Crack,
    Break,
    RevealFlash,
    RevealColor,
    Beat,
    Slide,
    Done,
}

/// Enum declaration order, used only by tests to assert monotonicity.
#[cfg(test)]
const PHASE_ORDER: [HatchPhase; 8] = [
    HatchPhase::Wiggle,
    HatchPhase::Crack,
    HatchPhase::Break,
    HatchPhase::RevealFlash,
    HatchPhase::RevealColor,
    HatchPhase::Beat,
    HatchPhase::Slide,
    HatchPhase::Done,
];

/// Number of stutter-step burst+hold cycles the crack phase is split into.
const CRACK_BURSTS: u64 = 4;
/// Duration within a cycle where the crack frame index advances quickly.
const CRACK_BURST_MS: u64 = 120;
/// Duration within a cycle where the crack frame index holds still.
const CRACK_HOLD_MS: u64 = 180;

/// Per-phase nominal durations, in the same order as [`HatchPhase`]'s
/// non-terminal variants. `phase_at` and `phase_progress` both walk this
/// as cumulative thresholds, so it is the single source of the timeline.
const PHASE_DURATIONS_MS: [(HatchPhase, u64); 7] = [
    (HatchPhase::Wiggle, 500),
    (HatchPhase::Crack, CRACK_BURSTS * (CRACK_BURST_MS + CRACK_HOLD_MS)),
    (HatchPhase::Break, 400),
    (HatchPhase::RevealFlash, 300),
    (HatchPhase::RevealColor, 500),
    (HatchPhase::Beat, 900),
    (HatchPhase::Slide, SLIDE_MS),
];

/// Approximate total nominal duration of the non-terminal timeline, used
/// only to size the sampling window in tests.
#[cfg(test)]
const APPROX_TOTAL_MS: u64 = 5_000;

/// Duration of the pre-reveal hatch-out transition (egg to screen-center,
/// panel off the right edge) — the same duration as the reveal's own
/// `Slide` phase, so the two movements read at one consistent pace.
pub(super) const SLIDE_MS: u64 = 500;
pub(super) const SLIDE_DURATION: Duration = Duration::from_millis(SLIDE_MS);

/// Cumulative duration of every phase strictly before `Crack` (just
/// `Wiggle`, in the phase list above) — the offset the Crack phase's
/// instance-method cadence must subtract from the whole-sequence `elapsed`
/// so its burst/hold blocks align with the phase boundary rather than the
/// sequence's start.
const CRACK_PHASE_START_MS: u64 = PHASE_DURATIONS_MS[0].1;

/// Phase active at `elapsed`, walking cumulative per-phase duration
/// thresholds (each phase's duration is a tunable named constant). Clamps
/// to `Done` once the full timeline has elapsed.
pub(crate) fn phase_at(elapsed: Duration) -> HatchPhase {
    let ms = elapsed.as_millis() as u64;
    let mut cumulative = 0u64;
    for (phase, dur) in PHASE_DURATIONS_MS {
        cumulative += dur;
        if ms < cumulative {
            return phase;
        }
    }
    HatchPhase::Done
}

/// Crack-phase gif frame index at `elapsed`, under the game-driven
/// burst/hold stutter cadence (not the gif's own frame delays): advance a
/// burst of frames quickly for `CRACK_BURST_MS`, hold on a frame for
/// `CRACK_HOLD_MS`, repeat for `CRACK_BURSTS` cycles. `frame_count <= 1`
/// returns 0.
pub(crate) fn crack_frame_index_at(elapsed: Duration, frame_count: usize) -> usize {
    if frame_count <= 1 {
        return 0;
    }
    let frame_count = frame_count as u64;
    let cycle_ms = CRACK_BURST_MS + CRACK_HOLD_MS;
    let ms = elapsed.as_millis() as u64;
    let cycle = (ms / cycle_ms).min(CRACK_BURSTS - 1);
    let within_cycle = ms - cycle * cycle_ms;

    // Split the gif's frames into CRACK_BURSTS contiguous blocks; this
    // cycle's block is [block_start, block_end].
    let block_size = frame_count.div_ceil(CRACK_BURSTS);
    let block_start = (cycle * block_size).min(frame_count - 1);
    let block_end = ((cycle + 1) * block_size - 1).min(frame_count - 1);

    if within_cycle < CRACK_BURST_MS {
        // Burst: advance through the block's frames over CRACK_BURST_MS.
        let block_len = block_end - block_start;
        if block_len == 0 {
            block_start as usize
        } else {
            let frac = within_cycle as f64 / CRACK_BURST_MS as f64;
            (block_start + (frac * block_len as f64).floor() as u64).min(block_end) as usize
        }
    } else {
        // Hold: frozen at the block's last frame.
        block_end as usize
    }
}

/// Vertical dot offset of the aggressive phase-1 wiggle at `elapsed`, a
/// sinusoid mirroring `super::tray::wiggle_offset_y` in shape but with a
/// larger peak amplitude (spec: stronger than the tray's ready-state bob).
const HATCH_WIGGLE_PERIOD: Duration = Duration::from_millis(300);
/// Peak vertical wiggle amplitude, in dots — exceeds `tray::WIGGLE_AMP_DOTS`.
const HATCH_WIGGLE_AMP_DOTS: i32 = 5;

pub(crate) fn wiggle_offset_y_at(elapsed: Duration) -> i32 {
    let phase = elapsed.as_secs_f64() / HATCH_WIGGLE_PERIOD.as_secs_f64() * std::f64::consts::TAU;
    (HATCH_WIGGLE_AMP_DOTS as f64 * phase.sin()).round() as i32
}

/// Time-driven hatch sequence: accumulates `elapsed` and derives phase,
/// crack frame, wiggle offset, and progress from it. Owned and ticked by the
/// scene; produces no rendering or scene mutation itself.
pub(crate) struct HatchSequence {
    elapsed: Duration,
}

impl HatchSequence {
    pub(crate) fn new() -> Self {
        Self { elapsed: Duration::ZERO }
    }

    pub(crate) fn advance(&mut self, dt: Duration) {
        self.elapsed += dt;
    }

    pub(crate) fn phase(&self) -> HatchPhase {
        phase_at(self.elapsed)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.phase() != HatchPhase::Done
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.phase() == HatchPhase::Done
    }

    /// Fraction (0.0..=1.0) through the current phase; 1.0 once `Done`.
    pub(crate) fn phase_progress(&self) -> f32 {
        let ms = self.elapsed.as_millis() as u64;
        let mut cumulative = 0u64;
        for (_, dur) in PHASE_DURATIONS_MS {
            let start = cumulative;
            cumulative += dur;
            if ms < cumulative {
                return ((ms - start) as f32 / dur as f32).clamp(0.0, 1.0);
            }
        }
        1.0
    }

    pub(crate) fn crack_frame(&self, frame_count: usize) -> usize {
        let local = self.elapsed.saturating_sub(Duration::from_millis(CRACK_PHASE_START_MS));
        crack_frame_index_at(local, frame_count)
    }

    pub(crate) fn wiggle_offset_y(&self) -> i32 {
        wiggle_offset_y_at(self.elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase_ordinal(p: HatchPhase) -> usize {
        PHASE_ORDER.iter().position(|x| *x == p).unwrap()
    }

    /// Stepping `t` across the full timeline (plus slack past the end), the
    /// phase ordinal never decreases, and every phase in the enum's declared
    /// order is reached at least once.
    #[test]
    fn phase_at_is_monotonic_and_covers_every_phase() {
        let mut seen = [false; 8];
        let mut last_ordinal = 0;
        let mut t = 0u64;
        while t <= APPROX_TOTAL_MS {
            let p = phase_at(Duration::from_millis(t));
            let ord = phase_ordinal(p);
            assert!(
                ord >= last_ordinal,
                "phase ordinal decreased at t={t}ms: {ord} < {last_ordinal}"
            );
            last_ordinal = ord;
            seen[ord] = true;
            t += 5;
        }
        assert!(seen.iter().all(|s| *s), "not every phase was reached: {seen:?}");
    }

    /// `Beat` is not reached until the last `t` where `phase_at(t) ==
    /// RevealColor` has fully elapsed (name-reveal gated on color-lerp
    /// completion).
    #[test]
    fn name_not_reached_until_reveal_color_completes() {
        let mut latest_reveal_color = None;
        let mut earliest_beat = None;
        let mut t = 0u64;
        while t <= APPROX_TOTAL_MS {
            match phase_at(Duration::from_millis(t)) {
                HatchPhase::RevealColor => latest_reveal_color = Some(t),
                HatchPhase::Beat if earliest_beat.is_none() => earliest_beat = Some(t),
                _ => {}
            }
            t += 1;
        }
        let latest_reveal_color = latest_reveal_color.expect("RevealColor phase never reached");
        let earliest_beat = earliest_beat.expect("Beat phase never reached");
        assert!(
            earliest_beat >= latest_reveal_color,
            "Beat reached at t={earliest_beat} before RevealColor finished at t={latest_reveal_color}"
        );
    }

    /// Beat precedes Slide, and both precede Done.
    #[test]
    fn beat_precedes_slide_precedes_done() {
        let mut earliest_beat = None;
        let mut earliest_slide = None;
        let mut earliest_done = None;
        let mut t = 0u64;
        while t <= APPROX_TOTAL_MS {
            match phase_at(Duration::from_millis(t)) {
                HatchPhase::Beat if earliest_beat.is_none() => earliest_beat = Some(t),
                HatchPhase::Slide if earliest_slide.is_none() => earliest_slide = Some(t),
                HatchPhase::Done if earliest_done.is_none() => earliest_done = Some(t),
                _ => {}
            }
            t += 1;
        }
        let beat = earliest_beat.expect("Beat phase never reached");
        let slide = earliest_slide.expect("Slide phase never reached");
        let done = earliest_done.expect("Done phase never reached");
        assert!(beat < slide, "Beat at {beat} did not precede Slide at {slide}");
        assert!(slide < done, "Slide at {slide} did not precede Done at {done}");
    }

    /// `is_active()` is true for the whole non-terminal timeline and false
    /// once the sequence has reached `Done`.
    #[test]
    fn is_active_true_until_done_then_false() {
        let mut seq = HatchSequence::new();
        assert!(seq.is_active(), "sequence must start active");
        assert!(!seq.is_complete());

        seq.advance(Duration::from_millis(APPROX_TOTAL_MS + 500));
        assert!(!seq.is_active(), "sequence must be inactive once Done");
        assert!(seq.is_complete());
    }

    /// Two `t`s inside the same crack hold window yield an equal frame
    /// index; a `t` in the following burst's play window yields a strictly
    /// larger index than the preceding hold's index — bursts then holds,
    /// not a uniform advance.
    #[test]
    fn crack_index_flat_across_hold_then_jumps_on_burst() {
        let frame_count = 6;
        let cycle_ms = CRACK_BURST_MS + CRACK_HOLD_MS;

        // Two samples late within the first cycle's hold window.
        let hold_a = crack_frame_index_at(Duration::from_millis(CRACK_BURST_MS + 5), frame_count);
        let hold_b = crack_frame_index_at(Duration::from_millis(CRACK_BURST_MS + CRACK_HOLD_MS - 5), frame_count);
        assert_eq!(hold_a, hold_b, "frame index must be flat across a hold window");

        // A sample early in the SECOND cycle's burst window should have
        // advanced past the first hold's frozen index.
        let next_burst = crack_frame_index_at(Duration::from_millis(cycle_ms + 5), frame_count);
        assert!(
            next_burst > hold_a,
            "frame index did not advance across a burst boundary: {next_burst} <= {hold_a}"
        );
    }

    /// `frame_count == 0` and `== 1` return 0 without panicking or
    /// overflowing.
    #[test]
    fn crack_index_frame_count_guard() {
        assert_eq!(crack_frame_index_at(Duration::from_millis(0), 0), 0);
        assert_eq!(crack_frame_index_at(Duration::from_millis(500), 0), 0);
        assert_eq!(crack_frame_index_at(Duration::from_millis(0), 1), 0);
        assert_eq!(crack_frame_index_at(Duration::from_millis(500), 1), 0);
    }

    /// The Crack phase does not begin at `elapsed == 0` (Wiggle precedes
    /// it), so the instance method's burst/hold cadence must be measured
    /// from the start of the Crack phase, not from the whole sequence's
    /// elapsed time — otherwise the burst/hold blocks land misaligned with
    /// the phase boundary.
    #[test]
    fn crack_frame_is_measured_from_crack_phase_start_not_sequence_elapsed() {
        let mut seq = HatchSequence::new();
        let wiggle_ms = PHASE_DURATIONS_MS[0].1;
        seq.advance(Duration::from_millis(wiggle_ms));
        assert_eq!(seq.phase(), HatchPhase::Crack, "fixture must land exactly at the start of Crack");

        let frame_count = 6;
        let at_phase_start = seq.crack_frame(frame_count);
        let expected = crack_frame_index_at(Duration::ZERO, frame_count);
        assert_eq!(
            at_phase_start, expected,
            "crack_frame at the exact start of the Crack phase must match crack_frame_index_at(0, ..), \
             not the whole-sequence elapsed time"
        );
    }

    /// The hatch wiggle's peak amplitude strictly exceeds the tray's
    /// ready-state bob amplitude over the same sampled period (spec:
    /// "aggressive wiggle STRONGER than the tray's ready-state wiggle").
    #[test]
    fn hatch_wiggle_stronger_than_tray_wiggle() {
        let mut hatch_peak = 0i32;
        let mut tray_peak = 0i32;
        let mut ms = 0u64;
        while ms <= 2_000 {
            let t = Duration::from_millis(ms);
            hatch_peak = hatch_peak.max(wiggle_offset_y_at(t).abs());
            tray_peak = tray_peak.max(super::super::tray::wiggle_offset_y(t).abs());
            ms += 5;
        }
        assert!(
            hatch_peak > tray_peak,
            "hatch wiggle peak {hatch_peak} did not exceed tray wiggle peak {tray_peak}"
        );
    }
}
