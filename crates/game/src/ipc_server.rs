//! Shim: the Unix-socket IPC transport moved to `scene_core::net::ipc_server`
//! (b4-t1). Re-exported here so every existing `ipc_server::` import path in
//! `game` keeps resolving.
pub use scene_core::net::ipc_server::*;
