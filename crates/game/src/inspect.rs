use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcCommand};
use std::sync::mpsc::Receiver;

use crate::ipc_server::{self, IpcHandle};
use crate::manager::Command;

// ── Public surface ─────────────────────────────────────────────────────────────

/// True on debug builds; false on release. Used as the default gate in `start`.
pub const INSPECT_SUPPORTED: bool = cfg!(debug_assertions);

/// Returns `true` iff `--inspect` appears in `args`.
pub fn flag_present<I: IntoIterator<Item = String>>(args: I) -> bool {
    args.into_iter().any(|a| a == "--inspect")
}

/// Path to the sibling `inspector` binary: `current_exe().parent()/"inspector"`.
pub fn inspector_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "current_exe has no parent directory",
        )
    })?;
    Ok(dir.join("inspector"))
}

/// Spawn the inspector binary with `socket_path` as its sole argument.
/// Returns `Err` on failure; callers warn-and-continue, never propagate.
pub fn spawn_inspector(bin: &Path, socket_path: &Path) -> io::Result<Child> {
    ProcCommand::new(bin).arg(socket_path).spawn()
}

/// Conditionally start the IPC server and inspector.
///
/// - `supported == false` → eprintln warning, return `Ok(None)` (no socket bound).
/// - `supported == true`  → bind socket via `ipc_server::spawn()`, print the path,
///   attempt to spawn the inspector (warn-and-continue on `Err`), return
///   `Ok(Some((handle, cmd_rx)))`.
pub fn start(supported: bool) -> io::Result<Option<(IpcHandle, Receiver<Command>)>> {
    if !supported {
        eprintln!("[inspect] --inspect is not supported in release builds; ignoring");
        return Ok(None);
    }

    let (handle, cmd_rx) = ipc_server::spawn()?;
    println!("[inspect] socket: {}", handle.socket_path.display());

    // Attempt to spawn the sibling inspector binary; swallow failures so the
    // game keeps running headless (spec 14:198).
    match inspector_path() {
        Ok(bin) => {
            if let Err(e) = spawn_inspector(&bin, &handle.socket_path) {
                eprintln!("[inspect] failed to spawn inspector: {}", e);
            }
        }
        Err(e) => eprintln!("[inspect] failed to find inspector path: {}", e),
    }

    Ok(Some((handle, cmd_rx)))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── flag_present_detects_and_rejects ──────────────────────────────────────

    /// `flag_present` returns true only when `--inspect` appears in args.
    #[test]
    fn flag_present_detects_and_rejects() {
        assert!(
            flag_present(vec!["--inspect".to_string()].into_iter()),
            "must detect --inspect when present"
        );
        assert!(
            !flag_present(vec!["other-flag".to_string()].into_iter()),
            "must reject unrelated flags"
        );
        assert!(
            !flag_present(std::iter::empty::<String>()),
            "must return false for empty args"
        );
    }

    // ── start_release_build_is_noop ───────────────────────────────────────────

    /// `start(false)` simulates a release build: must return `Ok(None)` and must
    /// not bind any socket.
    #[test]
    fn start_release_build_is_noop() {
        let result = start(false);
        assert!(result.is_ok(), "start(false) must not return Err");
        assert!(
            result.unwrap().is_none(),
            "start(false) must return Ok(None) — no socket bound on release builds"
        );
    }

    // ── start_debug_binds_socket_and_survives_missing_inspector ──────────────

    /// `start(true)` must return `Ok(Some)` with a live socket even when the
    /// sibling `inspector` binary is absent (cargo-test exe has no sibling).
    /// Dropping the handle must unlink the socket file.
    #[test]
    fn start_debug_binds_socket_and_survives_missing_inspector() {
        let result = start(true);
        assert!(result.is_ok(), "start(true) must not return Err");
        let inner = result.unwrap();
        assert!(
            inner.is_some(),
            "start(true) must return Ok(Some(..)) even when inspector binary is absent"
        );
        let (handle, _rx) = inner.unwrap();
        let path = handle.socket_path.clone();
        assert!(
            path.exists(),
            "socket file must exist immediately after start(true)"
        );
        drop(handle);
        // Allow the Drop impl a brief moment to unlink.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            !path.exists(),
            "socket file must be removed after IpcHandle is dropped"
        );
    }

    // ── spawn_inspector_missing_binary_errors ─────────────────────────────────

    /// Calling `spawn_inspector` with a nonexistent binary path must return `Err`
    /// without panicking — this is the failure mode `start` must tolerate.
    #[test]
    fn spawn_inspector_missing_binary_errors() {
        let result = spawn_inspector(
            Path::new("/nonexistent/inspector-xyz-b4t3"),
            Path::new("/tmp/fake.sock"),
        );
        assert!(
            result.is_err(),
            "spawn_inspector with a nonexistent binary must return Err, not Ok"
        );
    }
}
