//! The ONE lint-core test fixture, shared by `mention`'s
//! `resolve_mention_tests` and `diagnostics`'s own test module. Consolidates
//! two byte-identical copies (scope/bucket-b1.md FINDING 3) so a future
//! reshape of the roster can't silently desync one copy from the other.
//! Mirrors `scenes/test_util.rs`'s pattern: declared once at the common
//! parent (`lib.rs`) with `#![cfg(test)]`.
#![cfg(test)]

use crate::ability::Ability;
use crate::creatures::Creature;
use crate::mention::Vocabulary;
use crate::squad_role::{squad_role, SquadRole};

/// A synthetic `ROSTER_SIZE`-long roster, built so every DELIVERABLE case is
/// reachable without depending on `demo_roster()`'s contents:
/// - index 0 "Ember Wolf" (Active) — same name/abilities as the edited
///   creature.
/// - index 1 "Frost Lizard" (Active) — owns "Frost Bite", NOT an ability of
///   the edited creature.
/// - index 2 "Stone Golem" (Active, filler).
/// - index 3 "Storm Hawk" (Bench).
/// - index 4 "Verdant Treant" (Reserve).
/// - index 5 "Shadow Cat" (Reserve, filler).
///
/// "Volt Scorpion" is deliberately NOT included: bundled elsewhere but off
/// this roster.
pub(crate) fn roster() -> Vec<Creature> {
    vec![
        Creature::new("Ember Wolf")
            .with_abilities(vec![Ability::new("Ember Fang", vec![]), Ability::new("Howl", vec![])]),
        Creature::new("Frost Lizard").with_abilities(vec![Ability::new("Frost Bite", vec![])]),
        Creature::new("Stone Golem"),
        Creature::new("Storm Hawk"),
        Creature::new("Verdant Treant"),
        Creature::new("Shadow Cat"),
    ]
}

/// The creature being authored: same name/abilities as roster index 0, but
/// a SEPARATE value (no `Clone` on `Creature`).
pub(crate) fn edited_creature() -> Creature {
    Creature::new("Ember Wolf")
        .with_abilities(vec![Ability::new("Ember Fang", vec![]), Ability::new("Howl", vec![])])
}

pub(crate) fn vocab() -> Vocabulary {
    Vocabulary::new(&edited_creature(), &roster())
}

/// First roster index whose `squad_role` is `role` — never a literal index.
pub(crate) fn first_index_with_role(role: SquadRole) -> usize {
    (0..roster().len())
        .find(|&i| squad_role(i) == role)
        .unwrap_or_else(|| panic!("no roster index has role {:?} in this fixture", role))
}
