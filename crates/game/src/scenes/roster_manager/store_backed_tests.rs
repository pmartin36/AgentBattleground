//! Store-backed construction: `from_store_in` loads `PlayerData`
//! from an isolated `PlayerStore`, seeds+writes on first run, hydrates the
//! persisted roster into runtime `Creature`s (re-attaching bundled sprites
//! by name so the scene renders after a restart), and routes the swap
//! mutation through a save so it survives a reload.

use super::*;
use crate::creatures::AnimationKind;
use crate::player_data::{Egg, EggState, PlayerData, PlayerStore};
use crate::scenes::test_util::key_event;
use crossterm::event::KeyCode;
use std::sync::atomic::{AtomicU32, Ordering};

static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique per-test temp store dir (pid + monotonic counter), mirroring
/// `player_data::store`'s own hermetic-dir test pattern.
fn temp_store_dir(tag: &str) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "game-roster-store-backed-test-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ))
}

fn panics_if_seed_used() -> PlayerData {
    panic!("seed must not be invoked once a save exists on disk")
}

/// On an empty dir, `from_store_in` writes the first-run seed (derived from
/// `demo_roster()`) to `store.main_path()` and exposes all 6 seed creatures
/// with matching names/levels/stats, each carrying a rehydrated Idle sprite
/// (the bundled GIF re-attached by name) rather than a blank creature.
#[test]
fn from_store_in_seeds_writes_and_hydrates_demo_roster() {
    let dir = temp_store_dir("seed");
    let store = PlayerStore::with_dir(&dir);
    let demo = crate::creatures::demo_roster();

    let scene = RosterManager::from_store_in(store, None);

    assert!(
        PlayerStore::with_dir(&dir).main_path().exists(),
        "first-run construction must write the seed save file"
    );
    assert_eq!(scene.creatures.len(), demo.len());
    for (i, expected) in demo.iter().enumerate() {
        assert_eq!(scene.creatures[i].name(), expected.name(), "creature {i} name must match demo_roster()");
        assert_eq!(scene.creatures[i].level(), expected.level(), "creature {i} level must match demo_roster()");
        assert_eq!(scene.creatures[i].stats(), expected.stats(), "creature {i} stats must match demo_roster()");
    }
    assert!(
        scene.creatures[0].animation(AnimationKind::Idle).is_some(),
        "the seeded roster's bundled sprite must be re-attached, not left blank"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A select-and-swap (Space, Right, Space) on a store-backed scene persists
/// the new order: reloading `PlayerData` straight from the store afterward
/// shows the swapped names, not the original demo order. A swap that never
/// called through to a save would leave the reload showing the old order.
#[test]
fn swap_persists_new_order_across_reload() {
    let dir = temp_store_dir("swap");
    let mut scene = RosterManager::from_store_in(PlayerStore::with_dir(&dir), None);
    let name_at_0_before = scene.creatures[0].name().to_string();
    let name_at_1_before = scene.creatures[1].name().to_string();

    scene.handle_input(key_event(KeyCode::Char(' '))); // select current (0)
    scene.handle_input(key_event(KeyCode::Right)); // navigate 0 -> 1
    scene.handle_input(key_event(KeyCode::Char(' '))); // select again -> swap + persist

    let reloaded = PlayerStore::with_dir(&dir).load(panics_if_seed_used);
    let roster = &reloaded.data().roster;
    assert_eq!(
        roster[0].name, name_at_1_before,
        "the creature originally at index 1 must be persisted at index 0 after the swap"
    );
    assert_eq!(
        roster[1].name, name_at_0_before,
        "the creature originally at index 0 must be persisted at index 1 after the swap"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A second `from_store_in` against the SAME dir (the `Loaded::Main` path,
/// i.e. after a restart) still exposes a rehydrated Idle sprite — hydration
/// is not a first-run-only special case.
#[test]
fn reload_rehydrates_sprites_after_restart() {
    let dir = temp_store_dir("reload-sprites");
    let _first = RosterManager::from_store_in(PlayerStore::with_dir(&dir), None);

    let second = RosterManager::from_store_in(PlayerStore::with_dir(&dir), None);

    assert!(
        second.creatures[0].animation(AnimationKind::Idle).is_some(),
        "reloading an existing save must still rehydrate bundled sprites, not render blank"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// An egg present in the store before construction survives a roster-save
/// mutation (the swap): the roster save must re-write `roster + eggs`
/// together, never dropping the egg list.
#[test]
fn eggs_survive_roster_save() {
    let dir = temp_store_dir("eggs");
    let pre_store = PlayerStore::with_dir(&dir);
    pre_store
        .save(&PlayerData {
            roster: crate::creatures::demo_roster()
                .iter()
                .map(crate::player_data::creature_to_persisted)
                .collect(),
            eggs: vec![Egg {
                element: crate::ability::Element::Fire,
                state: EggState::Ready,
                mad_lib: Some("a smoldering ???".to_string()),
                egg_art: None,
                hatchling: None,
            }],
        })
        .expect("pre-seed save should succeed");

    let mut scene = RosterManager::from_store_in(PlayerStore::with_dir(&dir), None);
    scene.handle_input(key_event(KeyCode::Char(' ')));
    scene.handle_input(key_event(KeyCode::Right));
    scene.handle_input(key_event(KeyCode::Char(' '))); // swap -> persist

    let reloaded = PlayerStore::with_dir(&dir).load(panics_if_seed_used);
    assert_eq!(
        reloaded.data().eggs.len(),
        1,
        "the pre-existing egg must survive a roster save triggered by the swap"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

