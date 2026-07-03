use scene_core::inspect::FieldSchema;
use scene_core::{Inspectable, SceneCatalog, SceneKey};

use crate::scene_id::SceneId;

use crate::scene::Scene;
use crate::scenes::{BattleViewer, Leaderboard, MainHub, RosterManager};

/// Build a fresh boxed instance of the scene for `id` (spec 14: fresh-construct
/// on switch, state resets). M1 implements four scenes; the other five catalog
/// variants are not yet built and panic via `unimplemented!`.
pub fn construct(id: SceneId) -> Box<dyn Scene> {
    match id {
        SceneId::MainHub => Box::new(MainHub::default()),
        SceneId::BattleViewer => Box::new(BattleViewer::default()),
        SceneId::RosterManager => Box::new(RosterManager::new()),
        SceneId::Leaderboard => Box::new(Leaderboard),
        other => unimplemented!("scene {:?} is not implemented in M1", other),
    }
}

/// Type-level schema for `id` — the source of every `CatalogEntry.schema`
/// (b5-t3). Dispatches on the concrete scene type WITHOUT constructing an
/// instance (`Inspectable::schema()` is `where Self: Sized`, so it is not
/// reachable through `dyn Scene`/`dyn Inspectable`). Mirrors `construct`'s
/// exact implemented-id set and panic contract.
pub fn schema_for(id: SceneId) -> FieldSchema {
    match id {
        SceneId::MainHub => <MainHub as Inspectable>::schema(),
        SceneId::BattleViewer => <BattleViewer as Inspectable>::schema(),
        SceneId::RosterManager => <RosterManager as Inspectable>::schema(),
        SceneId::Leaderboard => <Leaderboard as Inspectable>::schema(),
        other => unimplemented!("scene {:?} is not implemented in M1", other),
    }
}

/// Whether `construct(id)` will succeed (vs. panic) for `id`. Derived from
/// `scene_for_digit`, the single source of truth for constructible scenes.
pub fn is_implemented(id: SceneId) -> bool {
    ('1'..='9')
        .filter_map(crate::scenes::scene_for_digit)
        .any(|s| s == id)
}

/// `scene_core::SceneCatalog` adapter wrapping the free `construct`/`schema_for`/
/// `is_implemented` functions above (b1-t4). Stateless; converts the engine-opaque
/// `&SceneKey` boundary value to `SceneId` via `SceneId::from_key`, then delegates.
pub struct GameCatalog;

impl SceneCatalog for GameCatalog {
    type Scene = dyn Scene;

    fn construct(&self, key: &SceneKey) -> Box<Self::Scene> {
        let id = SceneId::from_key(key)
            .unwrap_or_else(|| panic!("unknown SceneKey: {:?}", key.as_str()));
        construct(id)
    }

    fn schema_for(&self, key: &SceneKey) -> FieldSchema {
        let id = SceneId::from_key(key)
            .unwrap_or_else(|| panic!("unknown SceneKey: {:?}", key.as_str()));
        schema_for(id)
    }

    fn display_name(&self, key: &SceneKey) -> &str {
        let id = SceneId::from_key(key)
            .unwrap_or_else(|| panic!("unknown SceneKey: {:?}", key.as_str()));
        id.display_name()
    }

    fn catalog_keys(&self) -> Vec<SceneKey> {
        ['1', '2', '3', '4']
            .iter()
            .filter_map(|&c| crate::scenes::scene_for_digit(c))
            .map(SceneKey::from)
            .collect()
    }

    fn is_available(&self, key: &SceneKey) -> bool {
        SceneId::from_key(key).is_some_and(is_implemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construct_main_hub_id_roundtrip() {
        let scene = construct(SceneId::MainHub);
        assert_eq!(scene.id(), SceneId::MainHub.into());
    }

    #[test]
    fn construct_battle_viewer_id_roundtrip() {
        let scene = construct(SceneId::BattleViewer);
        assert_eq!(scene.id(), SceneId::BattleViewer.into());
    }

    #[test]
    fn construct_roster_manager_id_roundtrip() {
        let scene = construct(SceneId::RosterManager);
        assert_eq!(scene.id(), SceneId::RosterManager.into());
    }

    #[test]
    fn construct_leaderboard_id_roundtrip() {
        let scene = construct(SceneId::Leaderboard);
        assert_eq!(scene.id(), SceneId::Leaderboard.into());
    }

    #[test]
    #[should_panic]
    fn construct_unimplemented_scene_panics() {
        // Five catalog ids are not implemented in M1; they must panic.
        let _ = construct(SceneId::Settings);
    }

    #[test]
    fn is_implemented_partitions_catalog_into_expected_sets() {
        use std::collections::HashSet;

        let expected_true: HashSet<SceneId> = [
            SceneId::MainHub,
            SceneId::BattleViewer,
            SceneId::RosterManager,
            SceneId::Leaderboard,
        ]
        .into_iter()
        .collect();
        let expected_false: HashSet<SceneId> = [
            SceneId::Onboarding,
            SceneId::Matchmaking,
            SceneId::PostBattle,
            SceneId::ReplayBrowser,
            SceneId::Settings,
        ]
        .into_iter()
        .collect();

        let mut actual_true: HashSet<SceneId> = HashSet::new();
        let mut actual_false: HashSet<SceneId> = HashSet::new();
        for &id in SceneId::all() {
            if is_implemented(id) {
                actual_true.insert(id);
            } else {
                actual_false.insert(id);
            }
        }

        assert_eq!(actual_true, expected_true, "true-set mismatch");
        assert_eq!(actual_false, expected_false, "false-set mismatch");
    }

    // ------------------------------------------------------------ schema_for (b5-t3)

    /// `schema_for(id)` must return exactly `<T as Inspectable>::schema()` for
    /// each of the 4 implemented scenes — not a placeholder — since it is the
    /// sole source of every `CatalogEntry.schema` (b5-t3).
    #[test]
    fn schema_for_returns_each_scene_type_schema() {
        use scene_core::Inspectable;

        assert_eq!(
            schema_for(SceneId::MainHub),
            <MainHub as Inspectable>::schema(),
            "schema_for(MainHub) must equal <MainHub as Inspectable>::schema()"
        );
        assert_eq!(
            schema_for(SceneId::BattleViewer),
            <BattleViewer as Inspectable>::schema(),
            "schema_for(BattleViewer) must equal <BattleViewer as Inspectable>::schema()"
        );
        assert_eq!(
            schema_for(SceneId::RosterManager),
            <RosterManager as Inspectable>::schema(),
            "schema_for(RosterManager) must equal <RosterManager as Inspectable>::schema()"
        );
        assert_eq!(
            schema_for(SceneId::Leaderboard),
            <Leaderboard as Inspectable>::schema(),
            "schema_for(Leaderboard) must equal <Leaderboard as Inspectable>::schema()"
        );
    }

    /// `schema_for` must panic for an unimplemented id, mirroring `construct`'s
    /// panic contract (same implemented/unimplemented partition as `is_implemented`).
    #[test]
    #[should_panic]
    fn schema_for_panics_for_unimplemented() {
        let _ = schema_for(SceneId::Settings);
    }

    #[test]
    fn is_implemented_matches_construct_panic_behavior() {
        use std::panic::{self, AssertUnwindSafe};

        // Suppress the unimplemented!() backtrace noise on stderr for this test.
        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));

        for &id in SceneId::all() {
            let result = panic::catch_unwind(AssertUnwindSafe(|| construct(id)));
            assert_eq!(
                result.is_ok(),
                is_implemented(id),
                "is_implemented({:?}) disagrees with construct's panic behavior",
                id
            );
        }

        panic::set_hook(prev_hook);
    }

    /// `schema_for`'s panic/success partition must agree with `is_implemented`
    /// for every catalog entry, not just the single id `schema_for_panics_for_unimplemented`
    /// pins — mirrors `is_implemented_matches_construct_panic_behavior` exactly so a
    /// scene added to `construct` but missed in `schema_for` (or vice versa) is caught.
    #[test]
    fn is_implemented_matches_schema_for_panic_behavior() {
        use std::panic::{self, AssertUnwindSafe};

        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));

        for &id in SceneId::all() {
            let result = panic::catch_unwind(AssertUnwindSafe(|| schema_for(id)));
            assert_eq!(
                result.is_ok(),
                is_implemented(id),
                "is_implemented({:?}) disagrees with schema_for's panic behavior",
                id
            );
        }

        panic::set_hook(prev_hook);
    }

    // ------------------------------------------------------------ GameCatalog (b1-t4)

    #[test]
    fn game_catalog_construct_roundtrips_id() {
        let catalog = GameCatalog;
        for &id in &[
            SceneId::MainHub,
            SceneId::BattleViewer,
            SceneId::RosterManager,
            SceneId::Leaderboard,
        ] {
            let key: SceneKey = id.into();
            let scene = catalog.construct(&key);
            assert_eq!(scene.id(), id.into(), "construct({:?}) returned wrong scene id", id);
        }
    }

    #[test]
    fn game_catalog_schema_for_matches_inspectable() {
        let catalog = GameCatalog;
        assert_eq!(
            catalog.schema_for(&SceneKey::from(SceneId::MainHub)),
            <MainHub as Inspectable>::schema()
        );
        assert_eq!(
            catalog.schema_for(&SceneKey::from(SceneId::BattleViewer)),
            <BattleViewer as Inspectable>::schema()
        );
        assert_eq!(
            catalog.schema_for(&SceneKey::from(SceneId::RosterManager)),
            <RosterManager as Inspectable>::schema()
        );
        assert_eq!(
            catalog.schema_for(&SceneKey::from(SceneId::Leaderboard)),
            <Leaderboard as Inspectable>::schema()
        );
    }

    #[test]
    #[should_panic]
    fn game_catalog_construct_panics_for_valid_unbuilt_key() {
        let catalog = GameCatalog;
        let _ = catalog.construct(&SceneKey::from(SceneId::Settings));
    }

    #[test]
    fn game_catalog_is_available_partitions_like_is_implemented() {
        let catalog = GameCatalog;
        for &id in SceneId::all() {
            let key: SceneKey = id.into();
            assert_eq!(
                catalog.is_available(&key),
                is_implemented(id),
                "is_available({:?}) disagrees with is_implemented",
                id
            );
        }
        assert!(
            !catalog.is_available(&SceneKey::new("Nope")),
            "an unknown wire key must never be available"
        );
    }

    #[test]
    fn game_catalog_catalog_keys_are_four_in_digit_order() {
        let catalog = GameCatalog;
        let expected: Vec<SceneKey> = vec![
            SceneId::MainHub.into(),
            SceneId::BattleViewer.into(),
            SceneId::RosterManager.into(),
            SceneId::Leaderboard.into(),
        ];
        assert_eq!(catalog.catalog_keys(), expected);
    }

    #[test]
    fn game_catalog_display_name_is_spaced_battle_viewer() {
        let catalog = GameCatalog;
        assert_eq!(
            catalog.display_name(&SceneKey::from(SceneId::BattleViewer)),
            "Battle Viewer"
        );
    }
}
