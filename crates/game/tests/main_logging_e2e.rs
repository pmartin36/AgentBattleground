//! End-to-end coverage for spec 43 / task b6-t2: `game/main.rs` must init
//! logging (with the panic hook installed transitively) before anything
//! else runs, print the resolved log path to stdout up front, and log CLI
//! boot errors at ERROR before exiting non-zero.
//!
//! Runs the real compiled `game` binary as a subprocess with an invalid
//! `--scene` flag so no alternate-screen/terminal setup is reached — the
//! process should print the log path, log the CLI error, and exit(1).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique per-invocation temp dir (pid + monotonic counter), matching the
/// no-`tempfile`-crate pattern in `crates/game/src/cli.rs`'s tests.
fn temp_data_home(tag: &str) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "game-main-logging-e2e-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ));
    std::fs::create_dir_all(&path).expect("create temp XDG_DATA_HOME dir");
    path
}

#[test]
fn unknown_scene_boot_prints_log_path_logs_error_and_exits_1() {
    let data_home = temp_data_home("bogus-scene");

    let output = Command::new(env!("CARGO_BIN_EXE_game"))
        .arg("--scene")
        .arg("BogusName")
        .env("XDG_DATA_HOME", &data_home)
        .output()
        .expect("spawn game binary");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit code 1, got {:?}; stdout={stdout}",
        output.status.code()
    );

    let log_line = stdout
        .lines()
        .find(|line| line.starts_with("[log] "))
        .unwrap_or_else(|| panic!("no '[log] ' line in stdout; stdout was:\n{stdout}"));
    let log_path = log_line.trim_start_matches("[log] ").trim();
    assert!(
        log_path.ends_with(".log"),
        "printed log path doesn't look like a log file: {log_path}"
    );

    let log_contents = std::fs::read_to_string(log_path)
        .unwrap_or_else(|e| panic!("reading printed log path {log_path}: {e}"));
    assert!(
        log_contents.contains("ERROR"),
        "log file missing ERROR line; contents:\n{log_contents}"
    );
    assert!(
        log_contents.contains("unknown scene"),
        "log file missing 'unknown scene' text; contents:\n{log_contents}"
    );

    let _ = std::fs::remove_dir_all(&data_home);
}
