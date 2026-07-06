//! Squad-role slots and the pure positional index -> role lookup.
//! `squad_role` is the SINGLE index -> role mapping — roster position, not a
//! stored role field, is the source of truth (no independent field to keep
//! in sync).

/// Roster positions `0..ACTIVE_SLOTS` are the active squad.
pub const ACTIVE_SLOTS: usize = 3;
/// The next `BENCH_SLOTS` positions are the bench.
pub const BENCH_SLOTS: usize = 1;
/// The final `RESERVE_SLOTS` positions are the reserve.
pub const RESERVE_SLOTS: usize = 2;
/// Total roster size, derived — never re-hardcode `6` at a call site.
pub const ROSTER_SIZE: usize = ACTIVE_SLOTS + BENCH_SLOTS + RESERVE_SLOTS;

/// A roster entry's squad role, derived purely from its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SquadRole {
    Active,
    Bench,
    Reserve,
}

/// The squad role of the roster entry at `index`, purely from its position.
pub fn squad_role(index: usize) -> SquadRole {
    if index < ACTIVE_SLOTS {
        SquadRole::Active
    } else if index < ACTIVE_SLOTS + BENCH_SLOTS {
        SquadRole::Bench
    } else {
        SquadRole::Reserve
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every index in `0..ROSTER_SIZE` maps to the role its position implies,
    /// computed FROM the constants (never hardcoded 0/1/2/3/4/5 literals) so
    /// the test survives a constant change.
    #[test]
    fn squad_role_matches_position_across_full_roster() {
        for index in 0..ROSTER_SIZE {
            let expected = if index < ACTIVE_SLOTS {
                SquadRole::Active
            } else if index < ACTIVE_SLOTS + BENCH_SLOTS {
                SquadRole::Bench
            } else {
                SquadRole::Reserve
            };
            assert_eq!(
                squad_role(index),
                expected,
                "index {index} did not map to the expected role"
            );
        }
    }

    /// The three slot counts sum to the documented total roster size (spec:
    /// "6 pieces per player").
    #[test]
    fn slot_counts_sum_to_roster_size() {
        assert_eq!(ACTIVE_SLOTS + BENCH_SLOTS + RESERVE_SLOTS, ROSTER_SIZE);
        assert_eq!(ROSTER_SIZE, 6);
    }

    /// Exact boundary indices at each slot-edge, to catch off-by-one errors.
    #[test]
    fn boundary_indices_land_on_correct_side() {
        assert_eq!(squad_role(ACTIVE_SLOTS - 1), SquadRole::Active);
        assert_eq!(squad_role(ACTIVE_SLOTS), SquadRole::Bench);
        assert_eq!(squad_role(ACTIVE_SLOTS + BENCH_SLOTS), SquadRole::Reserve);
    }
}
