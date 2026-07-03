use std::path::{Path, PathBuf};

use crate::registry;
use crate::scene_id::SceneId;

// ── Public surface ─────────────────────────────────────────────────────────────

/// Which boot path the CLI resolved to, before scene-id/params validation.
#[derive(Debug, PartialEq, Eq)]
pub enum BootSource {
    /// Neither `--scene` nor `--state` was given.
    Default,
    /// `--scene <name>` was given, carrying the raw (unvalidated) wire name.
    Scene(String),
    /// `--state <path>` was given, carrying the raw path.
    State(PathBuf),
}

/// Errors surfaced while resolving the boot scene/params from argv or a
/// `--state` file. Variants beyond `MutuallyExclusive` are populated by later
/// tasks in this bucket (b3-t2 state-file parsing, b3-t3 scene validation).
#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    MutuallyExclusive,
    /// The `--state` file could not be read.
    StateFileUnreadable(PathBuf),
    /// The `--state` file's contents are not valid JSON.
    StateFileMalformed(PathBuf),
    /// The `--state` file's JSON is missing a required `"scene"` string field.
    StateFileMissingScene(PathBuf),
    /// A `--scene`/state-file scene name that is not a known SceneId wire string.
    UnknownScene(String),
    /// A known SceneId that is not constructible in M1 (would panic in construct()).
    NotImplemented(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::MutuallyExclusive => {
                write!(f, "--scene and --state are mutually exclusive")
            }
            CliError::StateFileUnreadable(p) => {
                write!(f, "could not read state file: {}", p.display())
            }
            CliError::StateFileMalformed(p) => {
                write!(f, "state file is not valid JSON: {}", p.display())
            }
            CliError::StateFileMissingScene(p) => {
                write!(
                    f,
                    "state file missing required \"scene\" field: {}",
                    p.display()
                )
            }
            CliError::UnknownScene(name) => write!(f, "unknown scene: {name}"),
            CliError::NotImplemented(name) => write!(f, "scene not implemented: {name}"),
        }
    }
}

impl std::error::Error for CliError {}

/// Parse a `--state` file's JSON envelope: `{ "scene": <string>, "params": <any> }`.
/// `params` is optional — both an absent key and an explicit `null` yield `None`;
/// any other JSON value (including `{}`, `[]`, `0`, `false`) is passed through
/// opaquely as `Some(that value)`.
// Wired into `resolve_boot`'s `BootSource::State` arm (b3-t3, same module).
fn parse_state_file(path: &Path) -> Result<(String, Option<serde_json::Value>), CliError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|_| CliError::StateFileUnreadable(path.to_path_buf()))?;

    let value: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|_| CliError::StateFileMalformed(path.to_path_buf()))?;

    let scene = value
        .get("scene")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CliError::StateFileMissingScene(path.to_path_buf()))?;

    let params = match value.get("params") {
        None | Some(serde_json::Value::Null) => None,
        Some(other) => Some(other.clone()),
    };

    Ok((scene, params))
}

/// Scan argv-shaped `args` for `--scene <name>` / `--state <path>`. Unrelated
/// tokens (e.g. `--inspect`) are ignored/passed through. Both flags present
/// (either order) is an error; neither present yields `Default`.
pub fn parse_flags<I: IntoIterator<Item = String>>(args: I) -> Result<BootSource, CliError> {
    let mut scene: Option<String> = None;
    let mut state: Option<PathBuf> = None;

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--scene" => scene = Some(iter.next().unwrap_or_default()),
            "--state" => state = Some(PathBuf::from(iter.next().unwrap_or_default())),
            _ => {}
        }
    }

    match (scene, state) {
        (Some(_), Some(_)) => Err(CliError::MutuallyExclusive),
        (Some(name), None) => Ok(BootSource::Scene(name)),
        (None, Some(path)) => Ok(BootSource::State(path)),
        (None, None) => Ok(BootSource::Default),
    }
}

/// Validate a wire-format scene name into a constructible `SceneId`. The
/// single place a name becomes a validated id — shared by both the
/// `--scene` and `--state` arms of `resolve_boot` (point 6: no second
/// validation path).
fn validate_scene(name: &str) -> Result<SceneId, CliError> {
    let id = SceneId::from_wire(name).ok_or_else(|| CliError::UnknownScene(name.to_string()))?;
    if registry::is_implemented(id) {
        Ok(id)
    } else {
        Err(CliError::NotImplemented(name.to_string()))
    }
}

/// Resolve the full boot decision from argv: combines `parse_flags` and
/// `parse_state_file`, then validates any named scene against
/// `SceneId::from_wire` + `registry::is_implemented` (b3-t3).
pub fn resolve_boot<I: IntoIterator<Item = String>>(
    args: I,
) -> Result<(SceneId, Option<serde_json::Value>), CliError> {
    match parse_flags(args)? {
        BootSource::Default => Ok((SceneId::MainHub, None)),
        BootSource::Scene(name) => Ok((validate_scene(&name)?, None)),
        BootSource::State(path) => {
            let (scene, params) = parse_state_file(&path)?;
            Ok((validate_scene(&scene)?, params))
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_flags_no_flags_yields_default() {
        let result = parse_flags(args(&[]));
        assert_eq!(result, Ok(BootSource::Default));
    }

    #[test]
    fn parse_flags_scene_flag_yields_scene_variant() {
        let result = parse_flags(args(&["--scene", "BattleViewer"]));
        assert_eq!(result, Ok(BootSource::Scene("BattleViewer".to_string())));
    }

    #[test]
    fn parse_flags_state_flag_yields_state_variant() {
        let result = parse_flags(args(&["--state", "some/file.json"]));
        assert_eq!(
            result,
            Ok(BootSource::State(PathBuf::from("some/file.json")))
        );
    }

    #[test]
    fn parse_flags_both_flags_scene_then_state_is_mutually_exclusive() {
        let result = parse_flags(args(&["--scene", "X", "--state", "Y"]));
        assert!(matches!(result, Err(CliError::MutuallyExclusive)));
    }

    #[test]
    fn parse_flags_both_flags_state_then_scene_is_mutually_exclusive() {
        let result = parse_flags(args(&["--state", "Y", "--scene", "X"]));
        assert!(matches!(result, Err(CliError::MutuallyExclusive)));
    }

    #[test]
    fn parse_flags_ignores_unrelated_flag_alongside_scene() {
        let result = parse_flags(args(&["--inspect", "--scene", "BattleViewer"]));
        assert_eq!(result, Ok(BootSource::Scene("BattleViewer".to_string())));
    }

    #[test]
    fn parse_flags_ignores_unrelated_flag_after_scene() {
        let result = parse_flags(args(&["--scene", "BattleViewer", "--inspect"]));
        assert_eq!(result, Ok(BootSource::Scene("BattleViewer".to_string())));
    }

    // ── parse_state_file (b3-t2) ────────────────────────────────────────────

    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Write `contents` to a unique temp file and return its path. Concurrent
    /// tests don't collide (pid + monotonic counter in the filename), per the
    /// no-`tempfile`-crate pattern in `crates/inspector/src/client.rs`.
    fn write_temp_json(tag: &str, contents: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "game-cli-test-{}-{}-{}.json",
            std::process::id(),
            tag,
            n
        ));
        std::fs::write(&path, contents).expect("write temp state file");
        path
    }

    #[test]
    fn parse_state_file_with_params_object_returns_exact_params_value() {
        let path = write_temp_json(
            "params-object",
            r#"{"scene": "BattleViewer", "params": {"battle_id": 42}}"#,
        );
        let result = parse_state_file(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            result,
            Ok((
                "BattleViewer".to_string(),
                Some(serde_json::json!({"battle_id": 42}))
            ))
        );
    }

    #[test]
    fn parse_state_file_with_params_absent_returns_none() {
        let path = write_temp_json("params-absent", r#"{"scene": "BattleViewer"}"#);
        let result = parse_state_file(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(result, Ok(("BattleViewer".to_string(), None)));
    }

    #[test]
    fn parse_state_file_with_params_null_returns_none() {
        let path = write_temp_json(
            "params-null",
            r#"{"scene": "BattleViewer", "params": null}"#,
        );
        let result = parse_state_file(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(result, Ok(("BattleViewer".to_string(), None)));
    }

    #[test]
    fn parse_state_file_missing_scene_field_errors() {
        let path = write_temp_json("missing-scene", r#"{"params": {"x": 1}}"#);
        let result = parse_state_file(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(result, Err(CliError::StateFileMissingScene(path)));
    }

    #[test]
    fn parse_state_file_nonexistent_path_errors() {
        let path = std::env::temp_dir().join(format!(
            "game-cli-test-{}-does-not-exist-{}.json",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let result = parse_state_file(&path);
        assert_eq!(result, Err(CliError::StateFileUnreadable(path)));
    }

    #[test]
    fn parse_state_file_malformed_json_errors() {
        let path = write_temp_json("malformed", "{not json");
        let result = parse_state_file(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(result, Err(CliError::StateFileMalformed(path)));
    }

    // ── resolve_boot (b3-t3) ─────────────────────────────────────────────────

    #[test]
    fn resolve_boot_no_flags_yields_main_hub_and_no_params() {
        let result = resolve_boot(args(&[]));
        assert_eq!(result, Ok((SceneId::MainHub, None)));
    }

    #[test]
    fn resolve_boot_scene_flag_known_implemented_name_resolves_with_no_params() {
        let result = resolve_boot(args(&["--scene", "BattleViewer"]));
        assert_eq!(result, Ok((SceneId::BattleViewer, None)));
    }

    #[test]
    fn resolve_boot_scene_flag_roster_manager_resolves_with_no_params() {
        let result = resolve_boot(args(&["--scene", "RosterManager"]));
        assert_eq!(result, Ok((SceneId::RosterManager, None)));
    }

    #[test]
    fn resolve_boot_scene_flag_unknown_name_errors() {
        let result = resolve_boot(args(&["--scene", "Nope"]));
        assert_eq!(result, Err(CliError::UnknownScene("Nope".to_string())));
    }

    #[test]
    fn resolve_boot_state_file_unknown_scene_name_errors() {
        let path = write_temp_json("resolve-unknown-scene", r#"{"scene": "Nope"}"#);
        let result = resolve_boot(args(&["--state", path.to_str().unwrap()]));
        let _ = std::fs::remove_file(&path);
        assert_eq!(result, Err(CliError::UnknownScene("Nope".to_string())));
    }

    /// A syntactically-valid-but-unimplemented scene name must be rejected as
    /// `NotImplemented`, and must NEVER reach `registry::construct`'s panic —
    /// asserted directly via `catch_unwind` around the `resolve_boot` call.
    #[test]
    fn resolve_boot_scene_flag_unimplemented_scene_errors_without_panicking() {
        use std::panic::{self, AssertUnwindSafe};

        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            resolve_boot(args(&["--scene", "Settings"]))
        }));
        panic::set_hook(prev_hook);

        assert!(
            outcome.is_ok(),
            "resolve_boot must not panic for a valid-but-unimplemented scene"
        );
        assert_eq!(
            outcome.unwrap(),
            Err(CliError::NotImplemented("Settings".to_string()))
        );
    }

    /// Same guarantee via the `--state` file path (point 6: single validation path).
    #[test]
    fn resolve_boot_state_file_unimplemented_scene_errors_without_panicking() {
        use std::panic::{self, AssertUnwindSafe};

        let path = write_temp_json("resolve-not-implemented", r#"{"scene": "Settings"}"#);

        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            resolve_boot(args(&["--state", path.to_str().unwrap()]))
        }));
        panic::set_hook(prev_hook);

        let _ = std::fs::remove_file(&path);
        assert!(
            outcome.is_ok(),
            "resolve_boot must not panic for a valid-but-unimplemented scene via --state"
        );
        assert_eq!(
            outcome.unwrap(),
            Err(CliError::NotImplemented("Settings".to_string()))
        );
    }

    /// Mandatory point-7 proof: a concrete `params` value written to a
    /// `--state` file travels intact through `resolve_boot` and then through
    /// `SceneManager::with_scene_and_params` into the scene's `enter()`.
    #[test]
    fn resolve_boot_end_to_end_state_file_params_reach_scene_enter() {
        use std::sync::{Arc, Mutex};

        use ratatui::layout::Rect;
        use serde_json::{json, Value as JsonValue};

        use scene_core::scene::manager::SceneManager;
        use scene_core::scene::{EngineCtx, InputEvent, Scene, Transition};

        struct ParamsCapturingScene {
            params: Arc<Mutex<Option<JsonValue>>>,
            no_inspect: scene_core::scene::NoInspect,
        }

        impl Scene for ParamsCapturingScene {
            fn id(&self) -> scene_core::SceneKey {
                SceneId::BattleViewer.into()
            }
            fn enter(&mut self, _ctx: &mut EngineCtx, params: Option<JsonValue>) {
                *self.params.lock().unwrap() = params;
            }
            fn update(
                &mut self,
                _ctx: &mut EngineCtx,
                _dt: std::time::Duration,
            ) -> Option<Transition> {
                None
            }
            fn render(&self, _frame: &mut ratatui::Frame, _area: Rect) {}
            fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
                None
            }
            fn exit(&mut self, _ctx: &mut EngineCtx) {}
            fn inspect(&mut self) -> &mut dyn scene_core::Inspectable {
                &mut self.no_inspect
            }
        }

        let path = write_temp_json(
            "resolve-end-to-end",
            r#"{"scene": "BattleViewer", "params": {"battle_id": 42}}"#,
        );

        let (id, params) = resolve_boot(args(&["--state", path.to_str().unwrap()]))
            .expect("resolve_boot must succeed for a valid implemented scene + params");
        let _ = std::fs::remove_file(&path);

        assert_eq!(id, SceneId::BattleViewer);

        let captured = Arc::new(Mutex::new(None));
        let scene = ParamsCapturingScene {
            params: Arc::clone(&captured),
            no_inspect: scene_core::scene::NoInspect,
        };
        let _mgr = SceneManager::with_scene_and_params(
            Box::new(scene),
            params,
            Box::new(crate::registry::GameCatalog),
        );

        assert_eq!(
            *captured.lock().unwrap(),
            Some(json!({"battle_id": 42})),
            "params from the --state file must reach the scene's enter() intact"
        );
    }
}
