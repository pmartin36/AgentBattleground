//! Integration test for `net::ipc_server`'s IPC lifecycle DEBUG logging
//! (spec 43, Decision 7; task b5-t1).
//!
//! Deliberately its own test binary (not the lib's `#[cfg(test)]` module):
//! `reader_loop`'s DEBUG events fire on `ipc_server::spawn`'s spawned
//! accept/reader threads, which only inherit a **global** default dispatcher
//! (`tracing::subscriber::set_global_default`) — not a thread-local
//! `with_default` one. `set_global_default` is one-shot per process, and
//! several already-shipped tests in `net::ipc_server`'s own `#[cfg(test)]`
//! module exercise the identical call sites via `spawn()` with no subscriber
//! installed at all; `tracing`'s per-callsite interest cache is
//! process-global, so if those unguarded calls and a guarded assertion share
//! one process, whichever hits a callsite first can permanently disable it
//! for the rest of that process (this is exactly what made the previous
//! `with_default`-based version of this test flaky — see
//! `.agent_handoffs/engine-app-logging-and-panic-handling/b5-t1/validator.md`).
//! Isolating into its own process and installing the global default before
//! `spawn()` is ever called removes the race: this process's first-ever hit
//! on each callsite is guaranteed to see this subscriber.
//!
//! Kept to a single `#[test]` fn for the same one-shot-global-state reason as
//! `tests/panic_hook.rs`.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use engine_core::ipc::{write_frame, Envelope, Message, SwitchScene};
use engine_core::net::ipc_server;
use engine_core::scene::manager::Command;
use engine_core::scene::Scene;
use engine_core::{FieldSchema, SceneCatalog, SceneKey};

/// Only `is_available("A")` is exercised by `reader_loop`'s `SwitchScene`
/// handling on this test's path; the other `SceneCatalog` methods are never
/// called along it.
struct LocalCatalog;

impl SceneCatalog for LocalCatalog {
    fn construct(&self, _key: &SceneKey) -> Box<dyn Scene> {
        unimplemented!("not exercised by this test")
    }
    fn schema_for(&self, _key: &SceneKey) -> FieldSchema {
        unimplemented!("not exercised by this test")
    }
    fn display_name(&self, _key: &SceneKey) -> &str {
        unimplemented!("not exercised by this test")
    }
    fn catalog_keys(&self) -> Vec<SceneKey> {
        unimplemented!("not exercised by this test")
    }
    fn is_available(&self, key: &SceneKey) -> bool {
        key.as_str() == "A"
    }
}

#[derive(Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Retry-connect until the server's accept loop is listening (up to 2 s) —
/// mirrors `net::ipc_server`'s own private test helper of the same shape.
fn connect_retry(path: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match UnixStream::connect(path) {
            Ok(s) => return s,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("timed out connecting to {:?}: {}", path, e),
        }
    }
}

/// Connecting a client, sending one `SwitchScene` frame, then disconnecting
/// must produce, at DEBUG, a connect line, a command-received line, and a
/// disconnect line (spec 43 Decision 7).
#[test]
fn ipc_lifecycle_logs_connect_command_disconnect_at_debug() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let make_writer = {
        let buf = buf.clone();
        move || SharedBuf(buf.clone())
    };
    let subscriber = tracing_subscriber::fmt()
        .with_writer(make_writer)
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("set_global_default must succeed exactly once in this process");

    let (handle, cmd_rx) =
        ipc_server::spawn(Arc::new(LocalCatalog)).expect("spawn must succeed");
    let mut client = connect_retry(&handle.socket_path);

    let first = cmd_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("ClientConnected must arrive before SwitchScene");
    assert!(
        matches!(first, Command::ClientConnected),
        "first command after connect must be ClientConnected"
    );

    write_frame(
        &mut client,
        &Envelope::new(
            1,
            None,
            Message::SwitchScene(SwitchScene {
                target: "A".to_string(),
                params: None,
            }),
        ),
    )
    .expect("write_frame");

    let cmd = cmd_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("SwitchScene Command must be forwarded");
    assert!(
        matches!(cmd, Command::SwitchScene { .. }),
        "expected a SwitchScene Command to be forwarded"
    );

    drop(client); // EOF -> triggers the disconnect log on the reader thread.

    // The disconnect log fires asynchronously on the reader thread after
    // EOF is observed; poll instead of asserting instantly.
    let deadline = Instant::now() + Duration::from_secs(2);
    let output = loop {
        let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        if output.contains("ipc client disconnected") || Instant::now() >= deadline {
            break output;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    assert!(
        output.contains("DEBUG"),
        "expected DEBUG-level events, got: {output:?}"
    );
    assert!(
        output.contains("ipc client connected"),
        "expected a connect line, got: {output:?}"
    );
    assert!(
        output.contains("ipc command received"),
        "expected a command-received line, got: {output:?}"
    );
    assert!(
        output.contains("ipc client disconnected"),
        "expected a disconnect line, got: {output:?}"
    );

    drop(handle);
}
