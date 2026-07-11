//! Cached creature instructions (b1-t4): `current_instructions` is loaded
//! once at construction and reloaded only when a navigation slide SETTLES
//! (never per frame/render). See research.md b1-t4.

use super::*;
use engine_core::scene::EngineCtx;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique per-test temp base dir (pid + monotonic counter), mirroring
/// `crate::instructions`'s own test helper (instructions.rs:105) — no env
/// var, no `tempfile` crate, race-free under parallel test execution.
fn temp_base_dir(tag: &str) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "game-roster-instructions-cache-test-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ))
}

/// The first two `demo_roster()` entries, by name (see
/// `slide_transition_tests.rs`'s `right_nav_slide_animates_and_settles`).
const CREATURE_0: &str = "Ember Wolf";
const CREATURE_1: &str = "Frost Lizard";

#[test]
fn new_loads_current_creature_file() {
    let base = temp_base_dir("new-loads");
    crate::instructions::write_instructions_in(&base, CREATURE_0, "ember wolf notes")
        .expect("seed write should succeed");

    let scene = RosterManager::new_with_instructions_base(base.clone());

    assert_eq!(
        scene.current_instructions, "ember wolf notes",
        "current_instructions must be loaded from the current creature's file at construction"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn settle_reloads_incoming_creature() {
    let base = temp_base_dir("settle-reloads");
    crate::instructions::write_instructions_in(&base, CREATURE_0, "ember wolf notes")
        .expect("seed write should succeed");
    crate::instructions::write_instructions_in(&base, CREATURE_1, "frost lizard notes")
        .expect("seed write should succeed");

    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    let mut ctx = EngineCtx;

    scene.navigate(Direction::Right);
    assert_eq!(scene.current_index, 1, "navigate must update current_index immediately");
    assert_eq!(
        scene.current_instructions, "ember wolf notes",
        "cache must still hold the OUTGOING creature's text mid-slide, before settle"
    );

    // Drive past SLIDE_DUR so the slide settles.
    scene.update(&mut ctx, RosterManager::SLIDE_DUR + Duration::from_millis(1));

    assert_eq!(
        scene.current_instructions, "frost lizard notes",
        "cache must reflect the INCOMING creature's file contents once the slide settles"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn not_reread_per_frame() {
    let base = temp_base_dir("not-reread");
    crate::instructions::write_instructions_in(&base, CREATURE_0, "original text")
        .expect("seed write should succeed");

    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    let mut ctx = EngineCtx;

    assert_eq!(scene.current_instructions, "original text");

    // Mutate the file on disk with NO intervening slide/settle.
    crate::instructions::write_instructions_in(&base, CREATURE_0, "mutated text")
        .expect("mutating write should succeed");

    // A frame update with no active slide must not re-read the file.
    scene.update(&mut ctx, Duration::from_millis(16));
    assert_eq!(
        scene.current_instructions, "original text",
        "current_instructions must stay stale across ordinary frame updates (no settle occurred)"
    );

    let _ = std::fs::remove_dir_all(&base);
}
