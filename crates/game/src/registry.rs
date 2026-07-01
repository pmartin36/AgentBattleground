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
}
