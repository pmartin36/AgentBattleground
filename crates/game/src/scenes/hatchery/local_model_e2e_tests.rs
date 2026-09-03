//! End-to-end proof that the mad-lib Done pipeline reaches the resolved
//! local model through the production `TextGen`/`backend_kind`/`LocalBackend`
//! chain: a registry-resolved `Provider::Local` config drives `begin_definition`,
//! and the llm-cli argv the production backend builds is captured via an
//! injected transport rather than a real subprocess.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use crate::ability::Element;
use crate::model_config::ConfigError;
use crate::player_data::{Egg, EggState, PlayerData, PlayerStore};
use crate::text_gen::backend_local::{LocalBackend, LocalInvocation, LocalTransport};
use crate::text_gen::job::CancelFlag;
use crate::text_gen::model_registry::{self, DEFAULT_MODEL_ID};
use crate::text_gen::operation::{backend_kind, BackendKind, TextGen};
use crate::text_gen::{Provider, ResolvedModelConfig, TextBackend, TextError};

static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique per-test temp dir, mirroring the sibling `definition` tests'
/// hermetic no-`tempfile`-crate pattern.
fn temp_store_dir(tag: &str) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "game-hatchery-local-model-e2e-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ))
}

fn undefined_egg() -> Egg {
    Egg { element: Element::Fire, state: EggState::Undefined, mad_lib: None, egg_art: None, hatchling: None }
}

/// The shape `resolve_model_config` produces for the registry default: a
/// `Provider::Local` config carrying the resolved `llm-cli` runtime path and
/// installed weights path for `DEFAULT_MODEL_ID`.
fn local_registry_config() -> ResolvedModelConfig {
    ResolvedModelConfig::local_registry(
        DEFAULT_MODEL_ID,
        PathBuf::from("/fake/bin/llm-cli"),
        PathBuf::from(format!("/fake/models/{DEFAULT_MODEL_ID}/model.gguf")),
    )
}

/// A `LocalTransport` fixture that records the invocation it was run with
/// and reports a canned parts completion, with no real subprocess.
struct CapturingTransport {
    captured: Arc<Mutex<Option<LocalInvocation>>>,
}

impl LocalTransport for CapturingTransport {
    fn run(&self, invocation: &LocalInvocation, _cancel: &CancelFlag) -> Result<String, TextError> {
        *self.captured.lock().unwrap() = Some(invocation.clone());
        Ok("NAME: Ember\nDESCRIPTION: A tiny beast.\nARCHETYPE: Melee\n".to_string())
    }
}

/// A `TextGenFactory` that routes through the production `backend_kind`
/// decision, injecting a capturing transport on the `Local` arm and panicking
/// on the `Online` arm — the config under test must route Local.
fn capturing_factory(captured: Arc<Mutex<Option<LocalInvocation>>>) -> super::TextGenFactory {
    Box::new(move |cfg: &ResolvedModelConfig| {
        let captured = captured.clone();
        TextGen::with_backend_factory(
            cfg.clone(),
            Box::new(move |c: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                match backend_kind(c.provider()) {
                    BackendKind::Local => Box::new(LocalBackend::with_transport(
                        c.clone(),
                        Box::new(CapturingTransport { captured: captured.clone() }),
                    )),
                    BackendKind::Online => panic!("e2e config must route Local, got an online provider"),
                }
            }),
            Duration::from_secs(2),
        )
    })
}

/// Constructs a hermetic `Hatchery` with one `Undefined` egg, the
/// registry-resolved default Local config, and a capturing `TextGenFactory`,
/// then enters edit mode for egg 0.
fn scene_with_local_registry_config(tag: &str) -> (super::Hatchery, Arc<Mutex<Option<LocalInvocation>>>) {
    let dir = temp_store_dir(tag);
    let seed = PlayerData { roster: Vec::new(), eggs: vec![undefined_egg()] };
    PlayerStore::with_dir(&dir).save(&seed).expect("seed save should succeed");

    let captured: Arc<Mutex<Option<LocalInvocation>>> = Arc::new(Mutex::new(None));
    let mut scene = super::Hatchery::from_store_with_gen(
        PlayerStore::with_dir(&dir),
        SystemTime::now(),
        super::Hatchery::production_asset_gen(),
        Result::<ResolvedModelConfig, ConfigError>::Ok(local_registry_config()),
        capturing_factory(captured.clone()),
    );
    scene.enter_edit(0);
    (scene, captured)
}

/// The registry-resolved default config ties to the real registry: it names
/// the `Local` provider, its `model_id` is `DEFAULT_MODEL_ID`, and that id
/// resolves to a real registry entry.
#[test]
fn resolved_default_config_is_local_qwen() {
    let config = local_registry_config();
    assert_eq!(config.provider(), Provider::Local);
    assert_eq!(config.model_id(), Some(DEFAULT_MODEL_ID));
    assert_eq!(DEFAULT_MODEL_ID, "qwen3-4b-instruct");
    assert!(model_registry::lookup(DEFAULT_MODEL_ID).is_some());
}

/// Pressing Done against the registry-resolved default Local config drives
/// the real mad-lib pipeline (`begin_definition` -> `build_parts_prompt` ->
/// `generate_text`) through the production `TextGen`/`backend_kind`/
/// `LocalBackend` chain: the pipeline starts with no error, and the
/// captured llm-cli invocation carries the resolved runtime, the resolved
/// weights via `-m`, the chat-template/single-turn flags, and a `-f` prompt
/// file containing the parts-prompt text.
#[test]
fn done_reaches_resolved_local_model_argv() {
    let (mut scene, captured) = scene_with_local_registry_config("argv-proof");

    scene.begin_definition("A small brave creature.".to_string());

    assert!(
        scene.definition_error.is_none(),
        "Done against a resolved Local config must not take an error branch, got {:?}",
        scene.definition_error
    );
    assert!(scene.definition.is_some(), "Done must start the pipeline's AwaitingText slot");

    let deadline = Instant::now() + Duration::from_secs(2);
    let invocation = loop {
        if let Some(inv) = captured.lock().unwrap().clone() {
            break inv;
        }
        assert!(Instant::now() < deadline, "queue worker never invoked the transport within 2s");
        std::thread::sleep(Duration::from_millis(5));
    };

    assert_eq!(
        invocation.program, "/fake/bin/llm-cli",
        "program must be the resolved llm-cli runtime path"
    );
    assert!(
        invocation.flags.windows(2).any(|w| w[0] == "-m" && w[1].contains("model.gguf")),
        "argv must pass the resolved weights via -m, got {:?}",
        invocation.flags
    );
    for flag in ["--jinja", "-st", "--no-display-prompt", "--simple-io", "--log-disable"] {
        assert!(
            invocation.flags.iter().any(|f| f == flag),
            "argv must contain {flag}, got {:?}",
            invocation.flags
        );
    }

    let sys_file = invocation
        .flags
        .windows(2)
        .find(|w| w[0] == "-sysf")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| panic!("argv must pass the system via -sysf <file>, got {:?}", invocation.flags));
    let contents = std::fs::read_to_string(&sys_file).expect("system file must exist and be readable");
    assert!(
        contents.contains("You describe only the parts of a creature"),
        "sysf file must carry the parts-prompt system text, got {contents:?}"
    );
}
