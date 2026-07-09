//! Integration test for `net::inspect::start`'s DEBUG logging (spec 43,
//! Decision 7; task b5-t1).
//!
//! Deliberately its own test binary (not the lib's `#[cfg(test)]` module):
//! `tracing::subscriber::set_global_default` is one-shot per process, and
//! several already-shipped tests in `net::inspect`'s own `#[cfg(test)]`
//! module call `start(..)` with no subscriber installed at all — sharing a
//! process with one of those races `tracing`'s process-global per-callsite
//! interest cache (whichever call hits a callsite first can permanently
//! disable it for the rest of the process; see
//! `.agent_handoffs/engine-app-logging-and-panic-handling/b5-t1/validator.md`).
//! Isolating into its own process removes the race entirely.
//!
//! Kept to a single `#[test]` fn for the same one-shot-global-state reason as
//! `tests/panic_hook.rs`.

use std::io::Write;
use std::sync::{Arc, Mutex};

use engine_core::net::inspect;
use engine_core::scene::Scene;
use engine_core::{FieldSchema, SceneCatalog, SceneKey};

/// `start` never calls any `SceneCatalog` method itself — it only threads
/// `catalog` through to `ipc_server::spawn`, which stores it for use by a
/// client connection this test never makes. Every method is unreachable.
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
    fn is_available(&self, _key: &SceneKey) -> bool {
        unimplemented!("not exercised by this test")
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

/// `start(true, ..)` must DEBUG-log the bound socket and the inspector spawn
/// attempt. Under `cargo test` the sibling `inspector` binary is absent, so
/// the spawn attempt fails — asserting on the "spawn failed" line proves the
/// failure arm logs, not just the success arm.
#[test]
fn start_debug_logs_bound_socket_and_inspector_spawn_attempt() {
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

    let result = inspect::start(true, Arc::new(LocalCatalog));
    let (handle, _rx) = result
        .expect("start(true) must not return Err")
        .expect("start(true) must return Ok(Some(..)) even when inspector binary is absent");
    drop(handle);

    let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(
        output.contains("DEBUG"),
        "expected DEBUG-level events, got: {output:?}"
    );
    assert!(
        output.contains("ipc server bound"),
        "expected an 'ipc server bound' line, got: {output:?}"
    );
    assert!(
        output.contains("inspector spawn failed"),
        "expected an 'inspector spawn failed' line (no sibling binary under cargo test), got: {output:?}"
    );
}
