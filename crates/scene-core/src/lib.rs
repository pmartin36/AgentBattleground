pub mod catalog;
pub mod color;
pub mod inspect;
pub mod scene_key;
pub mod ipc;

pub use catalog::SceneCatalog;
pub use inspect::{
    parse_path_segment, FieldSchema, FieldTag, Inspectable, PatchError, Range, Segment,
};
pub use scene_core_derive::Inspectable;
pub use scene_key::SceneKey;

/// Re-exports used only by `#[derive(Inspectable)]`-generated code so that
/// generated bodies never emit a bare `serde_json::...` path (callers may not
/// depend on `serde_json` directly, e.g. `render`).
#[doc(hidden)]
pub mod __private {
    pub use serde_json;
}
