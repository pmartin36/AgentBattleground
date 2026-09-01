//! Shared contract types for the text-generation API: the request shape a
//! caller submits (`TextRequest`), the closed set of providers a
//! `ResolvedModelConfig` can select (`Provider`), the resolved routing +
//! auth config every backend is constructed from (`ResolvedModelConfig`),
//! and the structured error both backends map their failures into
//! (`TextError`).

use std::path::{Path, PathBuf};

/// A caller-assembled generation request. The API does not wrap or inject
/// content into `user`; the caller has already assembled it.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRequest {
    pub system: String,
    pub user: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub stop: Vec<String>,
    pub seed: Option<u64>,
    /// Optional GBNF grammar text constraining decoding on the local
    /// backend. `None` means unconstrained. Online backends ignore it.
    pub grammar: Option<String>,
}

/// The closed set of providers a `TextBackend` can be constructed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    Local,
    Claude,
    OpenAi,
    OpenAiCompatible,
}

impl Provider {
    /// Every variant, hand-maintained. The conformance-enforcement test
    /// iterates this to prove every provider constructs a conforming
    /// backend.
    pub const ALL: [Provider; 4] = [
        Provider::Local,
        Provider::Claude,
        Provider::OpenAi,
        Provider::OpenAiCompatible,
    ];
}

/// Exhaustiveness guard: adding a `Provider` variant fails to compile here
/// until `ALL` above is also updated by a human.
#[cfg(test)]
#[allow(dead_code)]
fn _provider_exhaustive(p: Provider) {
    match p {
        Provider::Local | Provider::Claude | Provider::OpenAi | Provider::OpenAiCompatible => {}
    }
}

/// The provider selection, auth/routing config a `TextBackend` is
/// constructed from. Authoritative over routing only; `model_identity` is
/// the stable short id the opt-in cache keys on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedModelConfig {
    provider: Provider,
    model_identity: String,
    api_key: Option<String>,
    local_command: Option<String>,
    base_url: Option<String>,
    model_id: Option<String>,
    runtime_path: Option<PathBuf>,
    weights_path: Option<PathBuf>,
}

impl ResolvedModelConfig {
    pub fn new(
        provider: Provider,
        model_identity: impl Into<String>,
        api_key: Option<String>,
        local_command: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        ResolvedModelConfig {
            provider,
            model_identity: model_identity.into(),
            api_key,
            local_command,
            base_url,
            model_id: None,
            runtime_path: None,
            weights_path: None,
        }
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    pub fn model_identity(&self) -> &str {
        &self.model_identity
    }

    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    pub fn local_command(&self) -> Option<&str> {
        self.local_command.as_deref()
    }

    /// A registry-resolved `Provider::Local` config: `model_identity` and
    /// `model_id()` are the same stable registry id, carrying the resolved
    /// `llm-cli` runtime path and installed weights path.
    pub fn local_registry(
        model_id: impl Into<String>,
        runtime_path: PathBuf,
        weights_path: PathBuf,
    ) -> Self {
        let model_id = model_id.into();
        ResolvedModelConfig {
            provider: Provider::Local,
            model_identity: model_id.clone(),
            api_key: None,
            local_command: None,
            base_url: None,
            model_id: Some(model_id),
            runtime_path: Some(runtime_path),
            weights_path: Some(weights_path),
        }
    }

    /// The registry `model_id` for a registry-resolved Local config; `None`
    /// for online configs and for the `local_command` escape hatch.
    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    /// The resolved `llm-cli` sibling runtime path for a registry-resolved
    /// Local config; `None` otherwise.
    pub fn runtime_path(&self) -> Option<&Path> {
        self.runtime_path.as_deref()
    }

    /// The resolved installed GGUF weights path for a registry-resolved
    /// Local config; `None` otherwise.
    pub fn weights_path(&self) -> Option<&Path> {
        self.weights_path.as_deref()
    }
}

/// A structured failure from either backend. No GPU variants (that is an
/// asset_gen-only concept).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextError {
    /// Local: could not launch the sibling binary.
    Spawn(String),
    /// Local: the process exited non-zero.
    Process { code: Option<i32>, stderr: String },
    /// Local: a pipe/wait failure talking to the child process.
    Io(String),
    /// Online: a non-success HTTP status.
    Http { status: u16, body: String },
    /// Online: a network/connect failure.
    Transport(String),
    /// Either: the response could not be parsed.
    Parse(String),
    /// Either: e.g. `OpenAiCompatible` selected with no base URL.
    Config(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `local_registry` builds a `Provider::Local` config whose
    /// `model_identity` and `model_id()` are the same registry id, and whose
    /// runtime/weights accessors surface the paths it was given; the
    /// `local_command` escape hatch stays unset.
    #[test]
    fn local_registry_constructor_populates_paths() {
        let cfg = ResolvedModelConfig::local_registry(
            "qwen3-4b-instruct",
            PathBuf::from("/fake/bin/llm-cli"),
            PathBuf::from("/fake/models/qwen3-4b-instruct/w.gguf"),
        );

        assert_eq!(cfg.provider(), Provider::Local);
        assert_eq!(cfg.model_identity(), "qwen3-4b-instruct");
        assert_eq!(cfg.model_id(), Some("qwen3-4b-instruct"));
        assert_eq!(cfg.runtime_path(), Some(Path::new("/fake/bin/llm-cli")));
        assert_eq!(
            cfg.weights_path(),
            Some(Path::new("/fake/models/qwen3-4b-instruct/w.gguf"))
        );
        assert_eq!(cfg.local_command(), None);
    }
}
