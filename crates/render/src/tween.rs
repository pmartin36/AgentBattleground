//! Interpolation utilities (filled by b2-t1).
//!
//! Pure `f32`/`Duration` math: linear interpolation, easing curves, and a
//! single-channel eased `Tween` used to animate T/R/S components over time.

use std::time::Duration;

/// Linear interpolation. `t` is NOT clamped here (callers pass normalized t;
/// values outside `[0,1]` extrapolate).
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Identity easing on normalized `t`.
pub fn linear(t: f32) -> f32 {
    t
}

/// Smoothstep `3t^2 - 2t^3`. Clamps `t` to `[0,1]` first so `f(0)=0`,
/// `f(1)=1`, and it is monotonic non-decreasing on `[0,1]`.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A single-channel eased interpolation over a fixed duration.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub dur: Duration,
}

impl Tween {
    pub fn new(from: f32, to: f32, dur: Duration) -> Self {
        Tween { from, to, dur }
    }

    /// Value at `elapsed`. Clamps `t` to `[0,1]` (holds endpoints before `0`
    /// / after `dur`) and applies `ease_in_out`. Zero `dur` returns `to`.
    pub fn at(&self, elapsed: Duration) -> f32 {
        if self.dur.is_zero() {
            return self.to;
        }
        let t = (elapsed.as_secs_f32() / self.dur.as_secs_f32()).clamp(0.0, 1.0);
        lerp(self.from, self.to, ease_in_out(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-6;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() <= EPS
    }

    // -------------------------------------------------------------- lerp

    #[test]
    fn lerp_hits_endpoints_and_midpoint() {
        let a = 2.0;
        let b = 10.0;
        assert!(approx(lerp(a, b, 0.0), a), "t=0 must return a");
        assert!(approx(lerp(a, b, 1.0), b), "t=1 must return b");
        assert!(
            approx(lerp(a, b, 0.5), (a + b) / 2.0),
            "t=0.5 must return the midpoint"
        );
    }

    // ------------------------------------------------------------- linear

    #[test]
    fn linear_is_identity() {
        for t in [-0.5_f32, 0.0, 0.25, 0.5, 1.0, 1.5] {
            assert!(approx(linear(t), t), "linear({t}) must equal {t}");
        }
    }

    // ------------------------------------------------------- ease_in_out

    #[test]
    fn ease_in_out_endpoints_and_monotonic() {
        assert!(approx(ease_in_out(0.0), 0.0), "f(0) must be 0");
        assert!(approx(ease_in_out(1.0), 1.0), "f(1) must be 1");

        // Monotonic non-decreasing across 11 samples of [0,1].
        let mut prev = ease_in_out(0.0);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let cur = ease_in_out(t);
            assert!(
                cur + EPS >= prev,
                "ease_in_out must be non-decreasing: f({}) = {} < prev {}",
                t,
                cur,
                prev
            );
            prev = cur;
        }
    }

    #[test]
    fn ease_in_out_clamps_out_of_range_input() {
        assert!(
            approx(ease_in_out(-0.5), 0.0),
            "input below 0 must clamp to f(0)=0"
        );
        assert!(
            approx(ease_in_out(1.5), 1.0),
            "input above 1 must clamp to f(1)=1"
        );
    }

    // --------------------------------------------------------------- Tween

    #[test]
    fn tween_clamps_and_holds_endpoints() {
        let tw = Tween::new(0.0, 100.0, Duration::from_secs(2));

        assert!(
            approx(tw.at(Duration::from_secs(0)), 0.0),
            "at(0) must equal from"
        );
        assert!(
            approx(tw.at(Duration::from_secs(2)), 100.0),
            "at(dur) must equal to"
        );
        assert!(
            approx(tw.at(Duration::from_secs(10)), 100.0),
            "past dur must hold at to"
        );
    }

    #[test]
    fn tween_eases_in_between() {
        let tw = Tween::new(0.0, 100.0, Duration::from_secs(2));
        let half = tw.at(Duration::from_secs(1));
        let expected = lerp(0.0, 100.0, ease_in_out(0.5));
        assert!(
            approx(half, expected),
            "midpoint must equal the eased interpolation: got {half}, expected {expected}"
        );
    }

    #[test]
    fn tween_zero_dur_returns_to() {
        let tw = Tween::new(5.0, 42.0, Duration::from_secs(0));
        assert!(
            approx(tw.at(Duration::from_secs(0)), 42.0),
            "zero-duration tween must return `to` immediately (no NaN)"
        );
        assert!(
            approx(tw.at(Duration::from_secs(3)), 42.0),
            "zero-duration tween must return `to` regardless of elapsed"
        );
    }
}
