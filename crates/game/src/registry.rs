use scene_core::scene_id::SceneId;

use crate::scene::Scene;
use crate::scenes::{ArmyEditor, BattleViewer, Leaderboard, MainHub};

/// Build a fresh boxed instance of the scene for `id` (spec 14: fresh-construct
/// on switch, state resets). M1 implements four scenes; the other five catalog
/// variants are not yet built and panic via `unimplemented!`.
pub fn construct(id: SceneId) -> Box<dyn Scene> {
    match id {
        SceneId::MainHub => Box::new(MainHub),
        SceneId::BattleViewer => Box::new(BattleViewer::default()),
        SceneId::ArmyEditor => Box::new(ArmyEditor),
        SceneId::Leaderboard => Box::new(Leaderboard),
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

#[cfg(test)]
mod tests {
    use super::*;
    use scene_core::scene_id::SceneId;

    #[test]
    fn construct_main_hub_id_roundtrip() {
        let scene = construct(SceneId::MainHub);
        assert_eq!(scene.id(), SceneId::MainHub);
    }

    #[test]
    fn construct_battle_viewer_id_roundtrip() {
        let scene = construct(SceneId::BattleViewer);
        assert_eq!(scene.id(), SceneId::BattleViewer);
    }

    #[test]
    fn construct_army_editor_id_roundtrip() {
        let scene = construct(SceneId::ArmyEditor);
        assert_eq!(scene.id(), SceneId::ArmyEditor);
    }

    #[test]
    fn construct_leaderboard_id_roundtrip() {
        let scene = construct(SceneId::Leaderboard);
        assert_eq!(scene.id(), SceneId::Leaderboard);
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
            SceneId::ArmyEditor,
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
}
