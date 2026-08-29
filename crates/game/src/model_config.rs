//! Runtime model-config resolver: turns environment variables and/or a
//! local JSON config file into a `ResolvedModelConfig` (text_gen type), or
//! a clear absent signal when nothing usable is configured. Never panics.

use crate::text_gen::{Provider, ResolvedModelConfig};
use std::path::Path;

const ENV_PROVIDER: &str = "AGENTBATTLEGROUND_MODEL_PROVIDER";
const ENV_IDENTITY: &str = "AGENTBATTLEGROUND_MODEL_IDENTITY";
const ENV_API_KEY: &str = "AGENTBATTLEGROUND_MODEL_API_KEY";
const ENV_BASE_URL: &str = "AGENTBATTLEGROUND_MODEL_BASE_URL";
const ENV_LOCAL_COMMAND: &str = "AGENTBATTLEGROUND_MODEL_LOCAL_COMMAND";
const CONFIG_FILE_NAME: &str = "model_config.json";

/// Production entry: real env, then the JSON file under the base data dir.
/// `None` = no usable model configured (absent OR incomplete/malformed).
pub fn resolve_model_config() -> Option<ResolvedModelConfig> {
    let path = crate::instructions::base_data_dir(None).join(CONFIG_FILE_NAME);
    let file_json = read_config_file(&path);
    resolve_from_sources(|k| std::env::var(k).ok(), file_json.as_deref())
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

    #[test]
    fn local_env_config_missing_command_is_absent() {
        let env = env_map(&[(ENV_PROVIDER, "local"), (ENV_IDENTITY, "local-model")]);
        assert_eq!(resolve_from_sources(env, None), None);
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
