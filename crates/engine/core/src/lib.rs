extern crate self as engine_core;

pub mod catalog;
pub mod color;
pub mod inspect;
pub mod net;
pub mod scene;
pub mod scene_key;
pub mod ipc;

/// Shared mock fixtures for this crate's own test suites (`catalog.rs`,
/// `scene::manager`) — private (test-only) since no other crate needs it.
#[cfg(test)]
mod test_support;

pub use catalog::SceneCatalog;
pub use inspect::{
    parse_path_segment, FieldSchema, FieldTag, Inspectable, PatchError, Range, Segment,
};
pub use engine_derive::Inspectable;
pub use scene_key::SceneKey;

/// Re-exports used only by `#[derive(Inspectable)]`-generated code so that
/// generated bodies never emit a bare `serde_json::...` path (callers may not
/// depend on `serde_json` directly, e.g. `render`).
#[doc(hidden)]
pub mod __private {
    pub use serde_json;
}
