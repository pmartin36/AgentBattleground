//! Text-generation API: turns a `TextRequest` into inert text via a
//! `Provider`-selected `TextBackend`, running the backend through this
//! module's own text-local async job lifecycle with cooperative
//! cancellation. Structured like `asset_gen` but with its own job lifecycle
//! and types — see `job.rs`'s doc comment for why they are not shared.

pub mod backend;
pub mod backend_local;
pub mod backend_online;
pub mod cache;
pub mod conformance;
pub mod job;
pub mod model_install;
pub mod model_registry;
pub mod operation;
pub mod types;

pub use backend::TextBackend;
pub use job::{CancelFlag, JobHandle, JobQueue, JobStatus};
pub use types::{Provider, ResolvedModelConfig, TextError, TextRequest};

#[cfg(test)]
mod boundary {
    // Paths are relative to this file's directory (`text_gen/`).
    const SOURCES: &[(&str, &str)] = &[
        ("mod.rs", include_str!("mod.rs")),
        ("types.rs", include_str!("types.rs")),
        ("backend.rs", include_str!("backend.rs")),
        ("job.rs", include_str!("job.rs")),
        ("model_registry.rs", include_str!("model_registry.rs")),
        ("model_install.rs", include_str!("model_install.rs")),
        ("conformance.rs", include_str!("conformance.rs")),
        ("backend_local.rs", include_str!("backend_local.rs")),
        ("backend_online.rs", include_str!("backend_online.rs")),
        ("operation.rs", include_str!("operation.rs")),
        ("cache.rs", include_str!("cache.rs")),
    ];

    /// `text_gen` owns its entire job lifecycle and types; no file in this
    /// module imports from `asset_gen` (doc comments may still name it as
    /// the pattern this module's shape mirrors). Built via concatenation so
    /// this check does not flag its own source as a false positive.
    #[test]
    fn text_gen_never_imports_asset_gen() {
        let needle = format!("{}::{}", "crate", "asset_gen");
        for (name, src) in SOURCES {
            assert!(!src.contains(&needle), "{name} must not import from asset_gen");
        }
    }
}
