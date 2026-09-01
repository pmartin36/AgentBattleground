//! Runtime model-config resolver: turns environment variables and/or a
//! local JSON config file into a `ResolvedModelConfig` (text_gen type), or
//! a clear absent signal when nothing usable is configured. Never panics.

use crate::text_gen::model_install::InstallError;
use crate::text_gen::model_registry::ModelEntry;
use crate::text_gen::{Provider, ResolvedModelConfig};
use std::path::{Path, PathBuf};

const ENV_PROVIDER: &str = "AGENTBATTLEGROUND_MODEL_PROVIDER";
const ENV_IDENTITY: &str = "AGENTBATTLEGROUND_MODEL_IDENTITY";
const ENV_API_KEY: &str = "AGENTBATTLEGROUND_MODEL_API_KEY";
const ENV_BASE_URL: &str = "AGENTBATTLEGROUND_MODEL_BASE_URL";
const ENV_LOCAL_COMMAND: &str = "AGENTBATTLEGROUND_MODEL_LOCAL_COMMAND";
const ENV_MODEL_ID: &str = "AGENTBATTLEGROUND_MODEL_ID";
const CONFIG_FILE_NAME: &str = "model_config.json";
/// The bundled llama.cpp runtime sibling name.
const RUNTIME_BIN: &str = "llm-cli";

/// Model-config resolution failure: either the Local registry path (a
/// selected `model_id`, not the raw `local_command` escape hatch) or the
/// general case of nothing usable being configured at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// No online provider is complete, no `local_command` is set, and the
    /// runtime sibling directory could not be located.
    NotConfigured,
    UnknownModel { model_id: String },
    NotDownloaded { model_id: String },
}

/// Pure, seam-injected Local-registry resolver: resolves `model_id` (or the
/// registry default when `None`) through the registry to a
/// `ResolvedModelConfig` carrying the `llm-cli` sibling path under
/// `runtime_dir` and the installed weights path `weights` reports. Never
/// touches the filesystem or environment directly.
fn resolve_local_registry(
    model_id: Option<&str>,
    runtime_dir: &Path,
    weights: &dyn Fn(&ModelEntry) -> Result<PathBuf, InstallError>,
) -> Result<ResolvedModelConfig, ConfigError> {
    let id = model_id.unwrap_or(crate::text_gen::model_registry::DEFAULT_MODEL_ID);
    let entry = crate::text_gen::model_registry::lookup(id)
        .ok_or_else(|| ConfigError::UnknownModel { model_id: id.to_string() })?;
    let weights_path =
        weights(entry).map_err(|_| ConfigError::NotDownloaded { model_id: id.to_string() })?;
    Ok(ResolvedModelConfig::local_registry(
        id,
        runtime_dir.join(RUNTIME_BIN),
        weights_path,
    ))
}

/// Production entry: real env, then the JSON file under the base data dir.
/// Online configs and the `local_command` escape hatch resolve via
/// `resolve_from_sources`; a `Local` selection with no `local_command` (the
/// shipped default, including when nothing is configured at all) falls
/// through to the registry, resolving `model_id` against the real `llm-cli`
/// sibling and the installed weights. `Err` carries the reason nothing
/// usable is configured (an online provider absent/incomplete/malformed, a
/// registry model that is unknown, or one that is not yet downloaded) —
/// never a silent absence.
pub fn resolve_model_config() -> Result<ResolvedModelConfig, ConfigError> {
    let base = crate::instructions::base_data_dir(None);
    let path = base.join(CONFIG_FILE_NAME);
    let file_json = read_config_file(&path);
    let env = |k: &str| std::env::var(k).ok();

    if let Some(cfg) = resolve_from_sources(env, file_json.as_deref()) {
        return Ok(cfg);
    }

    let raw: Option<RawConfig> =
        file_json.as_deref().and_then(|j| serde_json::from_str(j).ok());
    let provider = non_empty(env(ENV_PROVIDER))
        .or_else(|| raw.as_ref().and_then(|r| r.provider.clone()))
        .and_then(|p| parse_provider(&p));
    let has_local_command = non_empty(env(ENV_LOCAL_COMMAND))
        .or_else(|| raw.as_ref().and_then(|r| r.local_command.clone()))
        .is_some();
    if !matches!(provider, None | Some(Provider::Local)) || has_local_command {
        return Err(ConfigError::NotConfigured);
    }

    let model_id = non_empty(env(ENV_MODEL_ID))
        .or_else(|| raw.as_ref().and_then(|r| r.model_id.clone()));
    let runtime_dir = sibling_dir().map_err(|_| ConfigError::NotConfigured)?;
    let weights = |entry: &ModelEntry| crate::text_gen::model_install::require_present(&base, entry);
    resolve_local_registry(model_id.as_deref(), &runtime_dir, &weights)
}

/// The directory holding the `llm-cli` sibling runtime binary: the running
/// executable's own directory, mirroring `SdCliRunner::sibling`
/// (asset_gen/runner.rs).
fn sibling_dir() -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent().map(Path::to_path_buf).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "current_exe has no parent directory")
    })
}

/// Pure, hermetic core: env wins wholesale over the file.
fn resolve_from_sources(
    env: impl Fn(&str) -> Option<String>,
    file_json: Option<&str>,
) -> Option<ResolvedModelConfig> {
    if let Some(cfg) = from_env(&env) {
        return Some(cfg);
    }
    file_json.and_then(from_file_json)
}

/// Non-empty check: an env var or JSON field set to `""` is not a value.
fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.is_empty())
}

fn from_env(env: &impl Fn(&str) -> Option<String>) -> Option<ResolvedModelConfig> {
    let provider = parse_provider(&non_empty(env(ENV_PROVIDER))?)?;
    build(
        provider,
        non_empty(env(ENV_IDENTITY)),
        non_empty(env(ENV_API_KEY)),
        non_empty(env(ENV_BASE_URL)),
        non_empty(env(ENV_LOCAL_COMMAND)),
    )
}

fn from_file_json(json: &str) -> Option<ResolvedModelConfig> {
    let raw: RawConfig = serde_json::from_str(json).ok()?;
    let provider = parse_provider(&non_empty(raw.provider)?)?;
    build(
        provider,
        non_empty(raw.model_identity),
        non_empty(raw.api_key),
        non_empty(raw.base_url),
        non_empty(raw.local_command),
    )
}

fn parse_provider(s: &str) -> Option<Provider> {
    match s.to_lowercase().as_str() {
        "local" => Some(Provider::Local),
        "claude" => Some(Provider::Claude),
        "openai" => Some(Provider::OpenAi),
        "openai_compatible" | "openai-compatible" => Some(Provider::OpenAiCompatible),
        _ => None,
    }
}

fn read_config_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Per-provider required-field gate, mirroring what the backends themselves
/// demand: Claude/OpenAi need identity + api_key; OpenAiCompatible also
/// needs base_url; Local needs identity + local_command. Any missing
/// required field yields `None`.
fn build(
    provider: Provider,
    identity: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    local_command: Option<String>,
) -> Option<ResolvedModelConfig> {
    match provider {
        Provider::Claude | Provider::OpenAi => {
            let identity = identity?;
            api_key.as_ref()?;
            Some(ResolvedModelConfig::new(
                provider,
                identity,
                api_key,
                local_command,
                base_url,
            ))
        }
        Provider::OpenAiCompatible => {
            let identity = identity?;
            api_key.as_ref()?;
            base_url.as_ref()?;
            Some(ResolvedModelConfig::new(
                provider,
                identity,
                api_key,
                local_command,
                base_url,
            ))
        }
        Provider::Local => {
            let identity = identity?;
            local_command.as_ref()?;
            Some(ResolvedModelConfig::new(
                provider,
                identity,
                api_key,
                local_command,
                base_url,
            ))
        }
    }
}

#[derive(serde::Deserialize)]
struct RawConfig {
    provider: Option<String>,
    model_identity: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    local_command: Option<String>,
    model_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_map(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| {
            owned
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn claude_env_config_complete_resolves() {
        let env = env_map(&[
            (ENV_PROVIDER, "claude"),
            (ENV_IDENTITY, "claude-3-5-sonnet"),
            (ENV_API_KEY, "sk-test"),
        ]);
        let cfg = resolve_from_sources(env, None).expect("should resolve");
        assert_eq!(cfg.provider(), Provider::Claude);
        assert_eq!(cfg.model_identity(), "claude-3-5-sonnet");
        assert_eq!(cfg.api_key(), Some("sk-test"));
    }

    #[test]
    fn openai_env_config_complete_resolves() {
        let env = env_map(&[
            (ENV_PROVIDER, "openai"),
            (ENV_IDENTITY, "gpt-4o"),
            (ENV_API_KEY, "sk-test"),
        ]);
        let cfg = resolve_from_sources(env, None).expect("should resolve");
        assert_eq!(cfg.provider(), Provider::OpenAi);
    }

    #[test]
    fn openai_compatible_with_base_url_resolves() {
        let env = env_map(&[
            (ENV_PROVIDER, "openai_compatible"),
            (ENV_IDENTITY, "local-llm"),
            (ENV_API_KEY, "sk-test"),
            (ENV_BASE_URL, "http://localhost:8080"),
        ]);
        let cfg = resolve_from_sources(env, None).expect("should resolve");
        assert_eq!(cfg.provider(), Provider::OpenAiCompatible);
        assert_eq!(cfg.base_url(), Some("http://localhost:8080"));
    }

    #[test]
    fn openai_compatible_missing_base_url_is_absent() {
        let env = env_map(&[
            (ENV_PROVIDER, "openai_compatible"),
            (ENV_IDENTITY, "local-llm"),
            (ENV_API_KEY, "sk-test"),
        ]);
        assert_eq!(resolve_from_sources(env, None), None);
    }

    #[test]
    fn local_env_config_with_command_resolves() {
        let env = env_map(&[
            (ENV_PROVIDER, "local"),
            (ENV_IDENTITY, "local-model"),
            (ENV_LOCAL_COMMAND, "/usr/bin/flux4"),
        ]);
        let cfg = resolve_from_sources(env, None).expect("should resolve");
        assert_eq!(cfg.provider(), Provider::Local);
        assert_eq!(cfg.local_command(), Some("/usr/bin/flux4"));
    }

    /// A `weights` seam reporting the model as present, keyed by whichever
    /// entry's `model_id` the resolver looks up.
    fn present_seam(entry: &ModelEntry) -> Result<PathBuf, InstallError> {
        Ok(PathBuf::from("/fake/models").join(entry.model_id).join("w.gguf"))
    }

    /// A `weights` seam reporting the model as never downloaded.
    fn absent_seam(entry: &ModelEntry) -> Result<PathBuf, InstallError> {
        Err(InstallError::NotDownloaded { model_id: entry.model_id.to_string() })
    }

    /// With no explicit `model_id`, the registry resolver picks the
    /// registry's default model.
    #[test]
    fn default_model_id_resolves_when_unset() {
        let rt_dir = Path::new("/fake/bin");
        let cfg = resolve_local_registry(None, rt_dir, &present_seam)
            .expect("default model_id should resolve");
        assert_eq!(cfg.model_id(), Some(crate::text_gen::model_registry::DEFAULT_MODEL_ID));
    }

    /// An explicit `model_id` overrides the registry default.
    #[test]
    fn explicit_model_id_overrides_default() {
        let rt_dir = Path::new("/fake/bin");
        let cfg = resolve_local_registry(Some("phi-4-mini-instruct"), rt_dir, &present_seam)
            .expect("explicit model_id should resolve");
        assert_eq!(cfg.model_id(), Some("phi-4-mini-instruct"));
    }

    /// A `model_id` absent from the registry is a config error, not a panic
    /// or a silent fallback to the default.
    #[test]
    fn unknown_model_id_is_config_error() {
        let rt_dir = Path::new("/fake/bin");
        let result = resolve_local_registry(Some("does-not-exist"), rt_dir, &present_seam);
        assert_eq!(
            result,
            Err(ConfigError::UnknownModel { model_id: "does-not-exist".to_string() })
        );
    }

    /// A known `model_id` whose weights are not yet installed surfaces a
    /// distinct "not downloaded" error naming the model id, never a silent
    /// `None`/absent result.
    #[test]
    fn absent_weights_is_model_not_downloaded() {
        let rt_dir = Path::new("/fake/bin");
        let result = resolve_local_registry(Some("qwen3-4b-instruct"), rt_dir, &absent_seam);
        assert_eq!(
            result,
            Err(ConfigError::NotDownloaded { model_id: "qwen3-4b-instruct".to_string() })
        );
    }

    /// The resolved runtime path is the `llm-cli` sibling under the given
    /// runtime dir.
    #[test]
    fn runtime_path_is_llm_cli_sibling() {
        let rt_dir = Path::new("/fake/bin");
        let cfg = resolve_local_registry(None, rt_dir, &present_seam).expect("should resolve");
        assert_eq!(cfg.runtime_path(), Some(Path::new("/fake/bin/llm-cli")));
    }

    #[test]
    fn claude_provider_set_but_api_key_absent_is_absent() {
        let env = env_map(&[(ENV_PROVIDER, "claude"), (ENV_IDENTITY, "claude-3-5-sonnet")]);
        assert_eq!(resolve_from_sources(env, None), None);
    }

    #[test]
    fn unknown_provider_string_is_absent() {
        let env = env_map(&[(ENV_PROVIDER, "not-a-real-provider"), (ENV_IDENTITY, "x")]);
        assert_eq!(resolve_from_sources(env, None), None);
    }

    #[test]
    fn no_env_no_file_is_absent() {
        assert_eq!(resolve_from_sources(no_env, None), None);
    }

    #[test]
    fn valid_file_json_resolves() {
        let json = r#"{"provider":"claude","model_identity":"claude-3-5-sonnet","api_key":"sk-test"}"#;
        let cfg = resolve_from_sources(no_env, Some(json)).expect("should resolve");
        assert_eq!(cfg.provider(), Provider::Claude);
        assert_eq!(cfg.model_identity(), "claude-3-5-sonnet");
    }

    #[test]
    fn malformed_file_json_is_absent_not_panic() {
        assert_eq!(resolve_from_sources(no_env, Some("{ not json")), None);
    }

    #[test]
    fn env_provider_wins_over_conflicting_file() {
        let env = env_map(&[
            (ENV_PROVIDER, "claude"),
            (ENV_IDENTITY, "env-identity"),
            (ENV_API_KEY, "sk-env"),
        ]);
        let json = r#"{"provider":"openai","model_identity":"file-identity","api_key":"sk-file"}"#;
        let cfg = resolve_from_sources(env, Some(json)).expect("should resolve");
        assert_eq!(cfg.provider(), Provider::Claude);
        assert_eq!(cfg.model_identity(), "env-identity");
    }

    /// Hermetic real-file check for `read_config_file`'s fs wiring: unique
    /// temp path per test run, no reliance on ambient state.
    /// no-`tempfile`-crate pattern.
    fn hermetic_temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "game-model-config-test-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn read_config_file_reads_real_file() {
        let path = hermetic_temp_path("read");
        let json = r#"{"provider":"claude","model_identity":"claude-3-5-sonnet","api_key":"sk-test"}"#;
        std::fs::write(&path, json).unwrap();

        let read = read_config_file(&path).expect("file should be read");
        assert_eq!(read, json);

        let cfg = from_file_json(&read).expect("should resolve from read contents");
        assert_eq!(cfg.provider(), Provider::Claude);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn read_config_file_missing_file_is_none() {
        let path = hermetic_temp_path("missing");
        assert_eq!(read_config_file(&path), None);
    }
}
