//! The 4-variant `StatKind` enum and the `Stats` struct it keys into.
//! `StatKind` is the single source of truth shared by named-field access
//! (`stats.strength`) and enum-keyed access (`stats.value(StatKind::Strength)`)
//! — and, downstream, by `StatRequirement` (ability gating). There must never
//! be a second, independently-hardcoded stat list.

use serde::{Deserialize, Serialize};

/// One of the 4 stats every `Stats` tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StatKind {
    Strength,
    Dexterity,
    Intelligence,
    Vitality,
}

impl StatKind {
    /// All variants, in a fixed order — for iterating without re-hardcoding
    /// the list a second time.
    pub const ALL: [StatKind; 4] = [
        StatKind::Strength,
        StatKind::Dexterity,
        StatKind::Intelligence,
        StatKind::Vitality,
    ];
}

/// A creature's 4 core stats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Stats {
    pub strength: u32,
    pub dexterity: u32,
    pub intelligence: u32,
    pub vitality: u32,
}

impl Stats {
    /// The value of `kind`, resolved through the single `kind -> field`
    /// mapping — the one crossing point that keeps enum-keyed access and
    /// named-field access from silently diverging.
    pub fn value(&self, kind: StatKind) -> u32 {
        match kind {
            StatKind::Strength => self.strength,
            StatKind::Dexterity => self.dexterity,
            StatKind::Intelligence => self.intelligence,
            StatKind::Vitality => self.vitality,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Stats::value(kind)` must equal the matching named field, for every
    /// variant — using 4 DISTINCT values so a wrong match arm (e.g. Vitality
    /// returning `dexterity`) can't silently pass on identical placeholder
    /// values. Iterates `StatKind::ALL` rather than listing variants by hand.
    #[test]
    fn value_matches_named_field_for_every_stat_kind() {
        let stats = Stats { strength: 11, dexterity: 22, intelligence: 33, vitality: 44 };
        let named = [
            (StatKind::Strength, stats.strength),
            (StatKind::Dexterity, stats.dexterity),
            (StatKind::Intelligence, stats.intelligence),
            (StatKind::Vitality, stats.vitality),
        ];
        for kind in StatKind::ALL {
            let expected = named.iter().find(|(k, _)| *k == kind).unwrap().1;
            assert_eq!(stats.value(kind), expected, "{kind:?} diverged from its named field");
        }
    }

    #[test]
    fn stats_json_round_trip_preserves_every_field() {
        let stats = Stats { strength: 11, dexterity: 22, intelligence: 33, vitality: 44 };
        let json = serde_json::to_string(&stats).expect("serialize");
        let decoded: Stats = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(stats, decoded);
    }
}
