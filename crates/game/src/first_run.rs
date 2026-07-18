//! First-run persistence flag (spec 61, task b4-t2): a small marker file
//! under the resolved game data dir, reusing
//! `instructions::base_data_dir`. Neutral naming/API so a later onboarding
//! flow can share this same mechanism rather than reinventing it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Filename (under the resolved base data dir) of the first-run marker.
const FIRST_RUN_FLAG_FILE: &str = "first_run_complete";

// ── explicit-base forms (test surface, race-free) ──────────────────────────

/// `base/first_run_complete`. Pure path construction, no IO.
pub fn first_run_flag_path_in(base: &Path) -> PathBuf {
    base.join(FIRST_RUN_FLAG_FILE)
}

/// True iff the first-run flag file is absent under `base`.
pub fn is_first_run_in(base: &Path) -> bool {
    !first_run_flag_path_in(base).exists()
}

/// Creates the first-run flag file under `base` (autocreating the parent
/// dir), so a subsequent `is_first_run_in(base)` returns `false`. Idempotent.
pub fn mark_first_run_done_in(base: &Path) -> io::Result<()> {
    let path = first_run_flag_path_in(base);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, "")
}

// ── runtime forms (resolver-driven; honor AGENTBATTLEGROUND_DATA_DIR) ──────

/// `is_first_run_in(&base_data_dir(None))`.
pub fn is_first_run() -> bool {
    is_first_run_in(&crate::instructions::base_data_dir(None))
}

/// `mark_first_run_done_in(&base_data_dir(None))`.
pub fn mark_first_run_done() -> io::Result<()> {
    mark_first_run_done_in(&crate::instructions::base_data_dir(None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp base dir (pid + monotonic counter), mirroring
    /// `instructions.rs`'s `temp_base_dir` pattern (no `tempfile` crate,
    /// race-free under parallel test execution).
    fn temp_base_dir(tag: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "game-first-run-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ))
    }

    #[test]
    fn flag_path_ends_with_first_run_complete() {
        let base = temp_base_dir("flag-path");
        let path = first_run_flag_path_in(&base);
        assert!(
            path.ends_with("first_run_complete"),
            "expected path to end with first_run_complete, got {:?}",
            path
        );
    }

    #[test]
    fn is_first_run_true_on_empty_dir() {
        let base = temp_base_dir("empty-dir");
        assert!(
            is_first_run_in(&base),
            "a base dir with no flag file must report first-run == true"
        );
    }

    #[test]
    fn mark_first_run_done_flips_is_first_run_to_false() {
        let base = temp_base_dir("mark-done");

        mark_first_run_done_in(&base).expect("mark_first_run_done_in should succeed");
        assert!(
            !is_first_run_in(&base),
            "after mark_first_run_done_in, is_first_run_in must be false"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn mark_first_run_done_is_idempotent() {
        let base = temp_base_dir("idempotent");

        mark_first_run_done_in(&base).expect("first mark should succeed");
        mark_first_run_done_in(&base).expect("second mark should still succeed");
        assert!(
            !is_first_run_in(&base),
            "after a second mark_first_run_done_in call, is_first_run_in must still be false"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn gitignore_contains_first_run_complete() {
        let gitignore_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitignore");
        let contents = std::fs::read_to_string(&gitignore_path)
            .unwrap_or_else(|e| panic!("failed to read {:?}: {}", gitignore_path, e));
        assert!(
            contents.lines().any(|l| l.trim() == "first_run_complete"),
            ".gitignore should contain a `first_run_complete` line, got:\n{}",
            contents
        );
    }
}
