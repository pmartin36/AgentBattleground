//! Pure egg incubation timing: the 24h readiness predicate, batch
//! promotion of elapsed eggs, and the focus-view countdown accessor. No
//! scene state; every function takes an explicit `now` so timing is
//! deterministic under test.

use std::time::{Duration, SystemTime};

use crate::player_data::{Egg, EggState};

/// Time an `Incubating` egg must accumulate before it is ready.
pub(crate) const INCUBATION: Duration = Duration::from_secs(24 * 3600);

/// True iff `egg` is `Incubating` and has accumulated at least `INCUBATION`
/// as of `now` (boundary inclusive). `Undefined`/`Ready` are always false,
/// as is an `Incubating` egg whose `started_at` is in the future.
pub(crate) fn is_ready_at(egg: &Egg, now: SystemTime) -> bool {
    match egg.state {
        EggState::Incubating { started_at } => match now.duration_since(started_at) {
            Ok(elapsed) => elapsed >= INCUBATION,
            Err(_) => false,
        },
        EggState::Undefined | EggState::Ready => false,
    }
}

/// Time remaining until `egg` is ready, or `None` if `egg` is not
/// `Incubating`. Saturates at zero once elapsed time exceeds `INCUBATION`.
/// The countdown accessor backing the focused-egg readout.
pub(crate) fn remaining(egg: &Egg, now: SystemTime) -> Option<Duration> {
    match egg.state {
        EggState::Incubating { started_at } => {
            let elapsed = now.duration_since(started_at).unwrap_or(Duration::ZERO);
            Some(INCUBATION.saturating_sub(elapsed))
        }
        EggState::Undefined | EggState::Ready => None,
    }
}

/// Promotes every `Incubating` egg in `eggs` that `is_ready_at(now)` to
/// `Ready`, in place. Returns whether any egg changed.
pub(crate) fn promote_ready(eggs: &mut [Egg], now: SystemTime) -> bool {
    let mut changed = false;
    for egg in eggs.iter_mut() {
        if is_ready_at(egg, now) {
            egg.state = EggState::Ready;
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::Element;

    fn incubating_since(started_at: SystemTime) -> Egg {
        Egg {
            element: Element::Fire,
            state: EggState::Incubating { started_at },
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        }
    }

    fn undefined_egg() -> Egg {
        Egg {
            element: Element::Fire,
            state: EggState::Undefined,
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

    /// The incubation duration is pinned to exactly 24 hours.
    #[test]
    fn incubation_constant_is_exactly_24_hours() {
        assert_eq!(INCUBATION, Duration::from_secs(24 * 3600));
    }

    /// An `Incubating` egg past its 24h mark is ready.
    #[test]
    fn is_ready_at_true_once_24h_elapsed() {
        let now = SystemTime::now();
        let egg = incubating_since(now - Duration::from_secs(24 * 3600 + 1));
        assert!(is_ready_at(&egg, now));
    }

    /// An `Incubating` egg short of its 24h mark is not ready.
    #[test]
    fn is_ready_at_false_before_24h_elapsed() {
        let now = SystemTime::now();
        let egg = incubating_since(now - Duration::from_secs(23 * 3600));
        assert!(!is_ready_at(&egg, now));
    }

    /// The 24h boundary is inclusive: exactly 24h elapsed is ready.
    #[test]
    fn is_ready_at_true_at_exact_24h_boundary() {
        let now = SystemTime::now();
        let egg = incubating_since(now - Duration::from_secs(24 * 3600));
        assert!(is_ready_at(&egg, now));
    }

    /// `Undefined` and `Ready` eggs are never reported ready by this
    /// predicate, regardless of `now`.
    #[test]
    fn is_ready_at_false_for_undefined_and_ready_states() {
        let now = SystemTime::now();
        assert!(!is_ready_at(&undefined_egg(), now));
        assert!(!is_ready_at(&ready_egg(), now));
    }

    /// A `started_at` in the future (clock skew) never reads as ready.
    #[test]
    fn is_ready_at_false_when_started_at_is_in_the_future() {
        let now = SystemTime::now();
        let egg = incubating_since(now + Duration::from_secs(3600));
        assert!(!is_ready_at(&egg, now));
    }

    /// `remaining` is `None` for eggs that carry no countdown.
    #[test]
    fn remaining_is_none_for_undefined_and_ready() {
        let now = SystemTime::now();
        assert_eq!(remaining(&undefined_egg(), now), None);
        assert_eq!(remaining(&ready_egg(), now), None);
    }

    /// After 23h elapsed, roughly 1h remains.
    #[test]
    fn remaining_is_close_to_one_hour_after_23_hours_elapsed() {
        let now = SystemTime::now();
        let egg = incubating_since(now - Duration::from_secs(23 * 3600));
        let left = remaining(&egg, now).expect("an incubating egg has a remaining duration");
        let expected = Duration::from_secs(3600);
        assert!(left.abs_diff(expected) < Duration::from_secs(2), "expected ~1h remaining, got {left:?}");
    }

    /// Once elapsed time exceeds the incubation window, `remaining`
    /// saturates at zero rather than reporting a negative duration.
    #[test]
    fn remaining_saturates_at_zero_once_elapsed() {
        let now = SystemTime::now();
        let egg = incubating_since(now - Duration::from_secs(24 * 3600 + 10));
        assert_eq!(remaining(&egg, now), Some(Duration::ZERO));
    }

    /// A mixed batch: only the elapsed `Incubating` egg is promoted; the
    /// not-yet-elapsed, `Undefined`, and already-`Ready` eggs are untouched.
    #[test]
    fn promote_ready_flips_only_elapsed_incubating_eggs() {
        let now = SystemTime::now();
        let mut eggs = vec![
            incubating_since(now - Duration::from_secs(24 * 3600 + 1)),
            incubating_since(now - Duration::from_secs(23 * 3600)),
            undefined_egg(),
            ready_egg(),
        ];

        let changed = promote_ready(&mut eggs, now);

        assert!(changed);
        assert_eq!(eggs[0].state, EggState::Ready);
        assert!(matches!(eggs[1].state, EggState::Incubating { .. }));
        assert_eq!(eggs[2].state, EggState::Undefined);
        assert_eq!(eggs[3].state, EggState::Ready);
    }

    /// A second `promote_ready` call over an already-settled batch reports
    /// no change — there is nothing left to promote.
    #[test]
    fn promote_ready_is_idempotent_on_a_second_call() {
        let now = SystemTime::now();
        let mut eggs = vec![incubating_since(now - Duration::from_secs(24 * 3600 + 1))];

        assert!(promote_ready(&mut eggs, now));
        assert!(!promote_ready(&mut eggs, now), "a second call over a settled batch must report no change");
    }
}
