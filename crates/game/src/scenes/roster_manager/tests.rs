use super::*;

#[test]
fn new_seeds_full_roster_at_index_zero_with_zero_elapsed() {
    let rm = RosterManager::new();
    assert_eq!(
        rm.creatures.len(),
        6,
        "RosterManager::new() must seed all 6 creatures from crate::creatures::demo_roster()"
    );
    assert_eq!(rm.creatures[0].name(), "Ember Wolf");
    assert_eq!(rm.current_index, 0);
    assert_eq!(rm.elapsed, Duration::ZERO);
}

/// `RosterManager::new()` must source its roster from
/// `crate::creatures::demo_roster()`, not `crate::creatures::all()` — the
/// per-creature RPG fields (stats/level/abilities/stamina) must match
/// `demo_roster()` element-for-element, not `Creature::new`'s defaults
/// (level 1, `Stats::default()`, empty abilities).
#[test]
fn new_sources_rpg_fields_from_demo_roster() {
    let rm = RosterManager::new();
    let demo = crate::creatures::demo_roster();

    for i in [0usize, 2usize] {
        assert_eq!(
            rm.creatures[i].level(),
            demo[i].level(),
            "creature {i} level must match demo_roster()"
        );
        assert_eq!(
            rm.creatures[i].stats(),
            demo[i].stats(),
            "creature {i} stats must match demo_roster()"
        );
        assert_eq!(
            rm.creatures[i].abilities(),
            demo[i].abilities(),
            "creature {i} abilities must match demo_roster()"
        );
        assert_eq!(
            rm.creatures[i].stamina(),
            demo[i].stamina(),
            "creature {i} stamina must match demo_roster()"
        );
    }

    // Guard against a missed swap: `all()`'s defaults would leave Ember
    // Wolf at level 1, which demo_roster() overrides to level 5. This is
    // the assertion that actually fails if `new()` still calls `all()`.
    assert_ne!(
        rm.creatures[0].level(),
        1,
        "RosterManager::new() must use demo_roster(), not all() (which defaults to level 1)"
    );
}

#[test]
fn schema_exposes_only_current_index() {
    let names: Vec<String> = <RosterManager as Inspectable>::schema()
        .children
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(
        names,
        vec!["current_index".to_string()],
        "creatures/elapsed must be #[inspect(hidden)]; only current_index is editable"
    );
}
