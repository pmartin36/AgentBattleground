//! Integration test for the process-wide panic hook installed by
//! `logging::init_at` (spec 43, Decision 4; task b3-t1).
//!
//! Deliberately its own test binary (not the lib's `#[cfg(test)]` module):
//! both `std::panic::set_hook` and `tracing::subscriber::set_global_default`
//! are process-global, one-shot state, so this file must be the sole caller
//! of `init_at` in its process for the assertions below to be deterministic.
//! See `.agent_handoffs/engine-app-logging-and-panic-handling/b3-t1/research.md`.
//!
//! Kept to a single `#[test]` fn for the same reason.

use std::io::Read;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

/// No-`tempfile`-crate tempdir helper, matching the pattern already used in
/// `crates/engine/core/src/logging.rs`'s unit tests and `crates/game/src/cli.rs`.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "engine-core-panic-hook-test-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `init_at` (via the panic hook it installs) must: restore the terminal out
/// of raw mode, and log an ERROR line containing the panic's message to the
/// returned handle's log file — even when the panic is caught by
/// `catch_unwind` (spec's motivating bug: today the terminal stays raw and
/// the message is invisible/lost).
#[test]
fn panic_hook_restores_terminal_and_logs_error() {
    let raw_ok = crossterm::terminal::enable_raw_mode().is_ok();

    let tmp = TempDir::new("panic-hook");
    let handle = engine_core::logging::init_at(tmp.path())
        .expect("init_at should succeed against a fresh tempdir");

    let distinctive_message = "distinctive-panic-b3t1-b7e2b9b4";
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        panic!("{distinctive_message}");
    }));

    let log_path = handle.log_path.clone();
    drop(handle); // flush the WorkerGuard before reading

    let mut contents = String::new();
    std::fs::File::open(&log_path)
        .unwrap_or_else(|e| panic!("failed to open {log_path:?}: {e}"))
        .read_to_string(&mut contents)
        .unwrap();

    assert!(
        contents.contains(distinctive_message),
        "expected the panic message in {log_path:?}, got: {contents:?}"
    );
    assert!(
        contents.contains("ERROR"),
        "expected an ERROR-level line in {log_path:?}, got: {contents:?}"
    );

    if raw_ok {
        if let Ok(still_raw) = crossterm::terminal::is_raw_mode_enabled() {
            assert!(
                !still_raw,
                "panic hook must leave the terminal out of raw mode"
            );
        }
    }
}
