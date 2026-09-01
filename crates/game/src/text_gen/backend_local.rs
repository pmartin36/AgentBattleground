//! The local subprocess `TextBackend`: drives the configured local model
//! out-of-process via the sibling-binary pattern, fully on the player's
//! machine. The framed prompt (system+user) is written to a scratch file
//! and passed via `-f`, never piped to stdin or scanned into argv; the
//! program and sampling flags form the scanned argv envelope, so caller
//! content can never leak an affordance into argv.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::backend::TextBackend;
use super::conformance::{CaptureBackend, NormalizedRequest};
use super::job::CancelFlag;
use super::types::{ResolvedModelConfig, TextError, TextRequest};

/// Fixed poll interval the sibling-binary transport uses while waiting on
/// the child process and checking for cancellation.
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A fully-formed local-model subprocess invocation. `program` and `flags`
/// are the scanned argv envelope; `prompt` (system+user, also written to the
/// `-f` scratch file named in `flags`) is the opaque payload the
/// conformance harness checks was transmitted. `temp_files` lists every
/// scratch file `flags` references, for the production transport to clean
/// up after the child exits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalInvocation {
    pub program: String,
    pub flags: Vec<String>,
    pub prompt: String,
    pub temp_files: Vec<std::path::PathBuf>,
}

/// Process-static counter giving each scratch file a unique name within
/// this process, alongside the pid.
static SCRATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique scratch file path under `abg_text` in the system temp
/// directory, with the given extension. Mirrors asset_gen's
/// `temp_dir().join("abg_...")` naming.
fn scratch_file(extension: &str) -> Result<std::path::PathBuf, TextError> {
    let dir = std::env::temp_dir().join("abg_text");
    std::fs::create_dir_all(&dir).map_err(|e| TextError::Io(e.to_string()))?;
    let pid = std::process::id();
    let n = SCRATCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    Ok(dir.join(format!("{pid}-{n}.{extension}")))
}

/// Execution seam: runs one invocation to completion, observing `cancel`.
/// Injectable so tests and the conformance harness drive the backend
/// without a real subprocess.
pub trait LocalTransport: Send + Sync {
    fn run(&self, invocation: &LocalInvocation, cancel: &CancelFlag) -> Result<String, TextError>;

    /// The invocation captured on the most recent `run`, for the
    /// conformance harness. Production transports need not capture.
    fn captured(&self) -> Option<LocalInvocation> {
        None
    }
}

/// One-shot sibling-binary transport: spawns the configured local model,
/// pipes the prompt to stdin, and returns stdout.
pub struct SiblingBinaryTransport;

impl LocalTransport for SiblingBinaryTransport {
    fn run(&self, invocation: &LocalInvocation, cancel: &CancelFlag) -> Result<String, TextError> {
        let mut child = Command::new(&invocation.program)
            .args(&invocation.flags)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TextError::Spawn(e.to_string()))?;

        let result = loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                break Err(TextError::Io("cancelled".to_string()));
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout = String::new();
                    if let Some(mut out) = child.stdout.take() {
                        let _ = out.read_to_string(&mut stdout);
                    }
                    if status.success() {
                        break Ok(stdout);
                    }
                    let mut stderr = String::new();
                    if let Some(mut err) = child.stderr.take() {
                        let _ = err.read_to_string(&mut stderr);
                    }
                    break Err(TextError::Process {
                        code: status.code(),
                        stderr,
                    });
                }
                Ok(None) => std::thread::sleep(POLL_INTERVAL),
                Err(e) => break Err(TextError::Io(e.to_string())),
            }
        };

        for f in &invocation.temp_files {
            let _ = std::fs::remove_file(f);
        }
        result
    }
}

/// The local-model `TextBackend`: builds an argv envelope plus a stdin
/// payload from a `TextRequest` and hands it to an injected `LocalTransport`.
pub struct LocalBackend {
    config: ResolvedModelConfig,
    transport: Box<dyn LocalTransport>,
}

impl LocalBackend {
    /// Production constructor: drives the configured local model via a real
    /// sibling-binary subprocess.
    pub fn new(config: ResolvedModelConfig) -> Self {
        LocalBackend {
            config,
            transport: Box::new(SiblingBinaryTransport),
        }
    }

    /// Test/harness constructor: drives an injected transport instead of a
    /// real subprocess.
    pub fn with_transport(config: ResolvedModelConfig, transport: Box<dyn LocalTransport>) -> Self {
        LocalBackend { config, transport }
    }

    fn build_invocation(&self, request: &TextRequest) -> Result<LocalInvocation, TextError> {
        let program = match (self.config.runtime_path(), self.config.local_command()) {
            (Some(runtime), _) => runtime.to_string_lossy().into_owned(),
            (None, Some(cmd)) => cmd.to_string(),
            (None, None) => return Err(TextError::Config("no local runtime configured".to_string())),
        };

        let mut flags = Vec::new();
        let mut temp_files = Vec::new();

        if let Some(weights) = self.config.weights_path() {
            flags.push("-m".to_string());
            flags.push(weights.to_string_lossy().into_owned());
        }

        let prompt = if request.system.is_empty() {
            request.user.clone()
        } else {
            format!("{}\n\n{}", request.system, request.user)
        };
        let prompt_file = scratch_file("txt")?;
        std::fs::write(&prompt_file, &prompt).map_err(|e| TextError::Io(e.to_string()))?;
        flags.push("-f".to_string());
        flags.push(prompt_file.to_string_lossy().into_owned());
        temp_files.push(prompt_file);

        flags.extend(["--jinja", "-no-cnv", "-st", "--no-display-prompt"].map(String::from));
        flags.push("--temp".to_string());
        flags.push(request.temperature.to_string());
        flags.push("-n".to_string());
        flags.push(request.max_tokens.to_string());
        if let Some(seed) = request.seed {
            flags.push("-s".to_string());
            flags.push(seed.to_string());
        }
        // request.stop is intentionally dropped on the local llm-cli path.

        if let Some(grammar) = &request.grammar {
            let grammar_file = scratch_file("gbnf")?;
            std::fs::write(&grammar_file, grammar).map_err(|e| TextError::Io(e.to_string()))?;
            flags.push("--grammar-file".to_string());
            flags.push(grammar_file.to_string_lossy().into_owned());
            temp_files.push(grammar_file);
        }

        Ok(LocalInvocation {
            program,
            flags,
            prompt,
            temp_files,
        })
    }
}

impl TextBackend for LocalBackend {
    fn generate(&self, request: &TextRequest, cancel: &CancelFlag) -> Result<String, TextError> {
        let invocation = self.build_invocation(request)?;
        self.transport.run(&invocation, cancel)
    }
}

impl CaptureBackend for LocalBackend {
    fn captured_request(&self) -> Option<NormalizedRequest> {
        self.transport
            .captured()
            .map(|inv| NormalizedRequest::from_subprocess(&inv.program, &inv.flags, &inv.prompt))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use super::super::conformance::assert_text_backend_conforms;
    use super::super::job::{JobQueue, JobStatus};
    use super::super::types::Provider;
    use super::*;

    fn local_config() -> ResolvedModelConfig {
        ResolvedModelConfig::new(
            Provider::Local,
            "local-test-model",
            None,
            Some("local-model-bin".to_string()),
            None,
        )
    }

    fn no_command_config() -> ResolvedModelConfig {
        ResolvedModelConfig::new(Provider::Local, "local-test-model", None, None, None)
    }

    fn registry_config() -> ResolvedModelConfig {
        ResolvedModelConfig::local_registry(
            "qwen3-4b-instruct",
            PathBuf::from("/fake/bin/llm-cli"),
            PathBuf::from("/fake/models/qwen3-4b-instruct/model.gguf"),
        )
    }

    fn sample_request() -> TextRequest {
        TextRequest {
            system: "you are a battle narrator".into(),
            user: "describe the opening move".into(),
            temperature: 0.7,
            max_tokens: 128,
            stop: Vec::new(),
            seed: None,
            grammar: None,
        }
    }

    /// A `LocalTransport` fixture that records the invocation it ran and
    /// returns a fixed result, with no real subprocess.
    struct CannedTransport {
        result: Result<String, TextError>,
        recorded: Mutex<Option<LocalInvocation>>,
    }

    impl CannedTransport {
        fn ok(text: &str) -> Self {
            CannedTransport {
                result: Ok(text.to_string()),
                recorded: Mutex::new(None),
            }
        }

        fn err(error: TextError) -> Self {
            CannedTransport {
                result: Err(error),
                recorded: Mutex::new(None),
            }
        }
    }

    impl LocalTransport for CannedTransport {
        fn run(&self, invocation: &LocalInvocation, _cancel: &CancelFlag) -> Result<String, TextError> {
            *self.recorded.lock().unwrap() = Some(invocation.clone());
            self.result.clone()
        }

        fn captured(&self) -> Option<LocalInvocation> {
            self.recorded.lock().unwrap().clone()
        }
    }

    /// A `LocalTransport` fixture that flips a flag if `run` is ever called,
    /// so a test can assert the transport was never invoked.
    struct NeverCalledTransport(Arc<AtomicBool>);

    impl LocalTransport for NeverCalledTransport {
        fn run(&self, _invocation: &LocalInvocation, _cancel: &CancelFlag) -> Result<String, TextError> {
            self.0.store(true, Ordering::SeqCst);
            Ok("should not be reached".to_string())
        }
    }

    /// A `LocalTransport` fixture that blocks until `cancel` fires, marking
    /// an observed flag before returning, standing in for a real subprocess
    /// being torn down rather than orphaned.
    struct BlockingTransport(Arc<AtomicBool>);

    impl LocalTransport for BlockingTransport {
        fn run(&self, _invocation: &LocalInvocation, cancel: &CancelFlag) -> Result<String, TextError> {
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            self.0.store(true, Ordering::SeqCst);
            Err(TextError::Io("cancelled".to_string()))
        }
    }

    /// A backend built over a capturing canned transport passes the shared
    /// conformance harness.
    #[test]
    fn local_backend_conforms() {
        let backend = LocalBackend::with_transport(local_config(), Box::new(CannedTransport::ok("text")));
        assert_text_backend_conforms(&backend);
    }

    /// `generate` returns the transport's completion text, and the captured
    /// invocation carries the configured program plus the llm-cli sampling
    /// flags (`--temp`/`-n`), never the old `--temperature`/`--max-tokens`.
    #[test]
    fn returns_completion_text() {
        let backend =
            LocalBackend::with_transport(local_config(), Box::new(CannedTransport::ok("the completion")));
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert_eq!(result, Ok("the completion".to_string()));

        let captured = backend
            .captured_request()
            .expect("backend must map the transport's captured invocation");
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();
        assert!(
            envelope.iter().any(|v| v == "local-model-bin"),
            "envelope must contain the configured program, got {envelope:?}"
        );
        assert!(
            envelope.iter().any(|v| v == "--temp"),
            "envelope must contain --temp, got {envelope:?}"
        );
        assert!(
            envelope.iter().any(|v| v == "-n"),
            "envelope must contain -n, got {envelope:?}"
        );
        assert!(
            !envelope.iter().any(|v| v.contains("--temperature") || v.contains("--max-tokens")),
            "envelope must not contain the superseded --temperature/--max-tokens flags, got {envelope:?}"
        );
    }

    /// The framed prompt (system+user) is written to a scratch file passed
    /// via `-f`, not piped to stdin; the raw text never leaks into the
    /// scanned argv envelope.
    #[test]
    fn prompt_written_to_file_not_stdin() {
        let backend = LocalBackend::with_transport(local_config(), Box::new(CannedTransport::ok("text")));
        let cancel = CancelFlag::new();
        let request = TextRequest {
            system: "you are terse".into(),
            user: "describe the opening move".into(),
            temperature: 0.1,
            max_tokens: 16,
            stop: Vec::new(),
            seed: None,
            grammar: None,
        };

        backend.generate(&request, &cancel).expect("canned transport always succeeds");

        let captured = backend.captured_request().expect("backend must capture the invocation");
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();

        let prompt_file = envelope
            .windows(2)
            .find(|w| w[0] == "-f")
            .map(|w| w[1].clone())
            .unwrap_or_else(|| panic!("envelope must pass the prompt via -f <file>, got {envelope:?}"));
        let contents = std::fs::read_to_string(&prompt_file).expect("prompt file must exist and be readable");
        assert!(
            contents.contains(&request.system) && contents.contains(&request.user),
            "prompt file must contain both system and user text, got {contents:?}"
        );
        assert!(
            !envelope.iter().any(|v| v.contains("you are terse")),
            "system/user content must not leak into the envelope, got {envelope:?}"
        );
    }

    /// A registry-resolved config (`ResolvedModelConfig::local_registry`)
    /// builds an llm-cli argv: weights via `-m`, chat-template + single-turn
    /// flags, and sampling via `--temp`/`-n`/`-s`.
    #[test]
    fn emits_llm_cli_argv_for_registry_config() {
        let backend = LocalBackend::with_transport(registry_config(), Box::new(CannedTransport::ok("text")));
        let cancel = CancelFlag::new();
        let request = TextRequest { seed: Some(7), ..sample_request() };

        backend
            .generate(&request, &cancel)
            .expect("a registry-resolved config must produce a runnable llm-cli invocation");

        let captured = backend.captured_request().expect("backend must capture the invocation");
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();

        assert!(
            envelope.windows(2).any(|w| w[0] == "-m" && w[1].contains("model.gguf")),
            "envelope must pass the resolved weights via -m, got {envelope:?}"
        );
        for flag in ["--jinja", "-no-cnv", "-st", "--no-display-prompt"] {
            assert!(envelope.iter().any(|v| v == flag), "envelope must contain {flag}, got {envelope:?}");
        }
        assert!(
            envelope.windows(2).any(|w| w[0] == "--temp" && w[1] == request.temperature.to_string()),
            "envelope must pass temperature via --temp, got {envelope:?}"
        );
        assert!(
            envelope.windows(2).any(|w| w[0] == "-n" && w[1] == request.max_tokens.to_string()),
            "envelope must pass max_tokens via -n, got {envelope:?}"
        );
        assert!(
            envelope.windows(2).any(|w| w[0] == "-s" && w[1] == "7"),
            "envelope must pass the seed via -s, got {envelope:?}"
        );

        let prompt_file = envelope
            .windows(2)
            .find(|w| w[0] == "-f")
            .map(|w| w[1].clone())
            .unwrap_or_else(|| panic!("envelope must pass the prompt via -f <file>, got {envelope:?}"));
        let contents = std::fs::read_to_string(&prompt_file).expect("prompt file must exist and be readable");
        assert!(
            contents.contains(&request.system) && contents.contains(&request.user),
            "prompt file must contain both system and user text, got {contents:?}"
        );
    }

    /// A request carrying GBNF grammar text maps to `--grammar-file <path>`
    /// (never the inline `--grammar` form), whose file contents equal the
    /// grammar text.
    #[test]
    fn grammar_maps_to_grammar_file_when_set() {
        let backend = LocalBackend::with_transport(local_config(), Box::new(CannedTransport::ok("text")));
        let cancel = CancelFlag::new();
        let grammar_text = "root ::= \"yes\" | \"no\"".to_string();
        let request = TextRequest { grammar: Some(grammar_text.clone()), ..sample_request() };

        backend.generate(&request, &cancel).expect("canned transport always succeeds");

        let captured = backend.captured_request().expect("backend must capture the invocation");
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();

        assert!(
            !envelope.iter().any(|v| v == "--grammar"),
            "grammar must use the file form, not the inline --grammar flag, got {envelope:?}"
        );
        let grammar_file = envelope
            .windows(2)
            .find(|w| w[0] == "--grammar-file")
            .map(|w| w[1].clone())
            .unwrap_or_else(|| panic!("envelope must pass the grammar via --grammar-file <file>, got {envelope:?}"));
        let contents = std::fs::read_to_string(&grammar_file).expect("grammar file must exist and be readable");
        assert_eq!(contents, grammar_text, "grammar file contents must equal the request's grammar text");
    }

    /// A request with no grammar never emits `--grammar-file` or the inline
    /// `--grammar` flag.
    #[test]
    fn grammar_omitted_when_unset() {
        let backend = LocalBackend::with_transport(local_config(), Box::new(CannedTransport::ok("text")));
        let cancel = CancelFlag::new();

        backend.generate(&sample_request(), &cancel).expect("canned transport always succeeds");

        let captured = backend.captured_request().expect("backend must capture the invocation");
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();
        assert!(
            !envelope.iter().any(|v| v == "--grammar-file" || v == "--grammar"),
            "no grammar flag must appear when the request carries none, got {envelope:?}"
        );
    }

    /// `TextRequest.stop` is dropped on the local llm-cli path: no `--stop`
    /// flag and no stop text reaches the argv.
    #[test]
    fn stop_sequences_dropped() {
        let backend = LocalBackend::with_transport(local_config(), Box::new(CannedTransport::ok("text")));
        let cancel = CancelFlag::new();
        let request = TextRequest { stop: vec!["HALT_MARKER".to_string()], ..sample_request() };

        backend.generate(&request, &cancel).expect("canned transport always succeeds");

        let captured = backend.captured_request().expect("backend must capture the invocation");
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();
        assert!(
            !envelope.iter().any(|v| v == "--stop" || v == "HALT_MARKER"),
            "stop sequences must be dropped on the local path, got {envelope:?}"
        );
    }

    /// A process failure the transport returns surfaces from `generate`
    /// unchanged, as a structured error rather than a hang.
    #[test]
    fn process_failure_maps_to_error() {
        let error = TextError::Process {
            code: Some(1),
            stderr: "boom".to_string(),
        };
        let backend = LocalBackend::with_transport(local_config(), Box::new(CannedTransport::err(error.clone())));
        let cancel = CancelFlag::new();

        assert_eq!(backend.generate(&sample_request(), &cancel), Err(error));
    }

    /// A spawn failure the transport returns surfaces from `generate`
    /// unchanged.
    #[test]
    fn spawn_failure_maps_to_error() {
        let error = TextError::Spawn("no such binary".to_string());
        let backend = LocalBackend::with_transport(local_config(), Box::new(CannedTransport::err(error.clone())));
        let cancel = CancelFlag::new();

        assert_eq!(backend.generate(&sample_request(), &cancel), Err(error));
    }

    /// A config with no local command fails with a structured `Config`
    /// error, and the transport is never invoked.
    #[test]
    fn missing_local_command_is_config_error() {
        let called = Arc::new(AtomicBool::new(false));
        let backend =
            LocalBackend::with_transport(no_command_config(), Box::new(NeverCalledTransport(called.clone())));
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert!(matches!(result, Err(TextError::Config(_))), "expected Config error, got {result:?}");
        assert!(!called.load(Ordering::SeqCst), "transport must not run when local_command is None");
    }

    /// Driven through the job queue with a short timeout, the transport
    /// observes the cancel flag rather than being abandoned, and the handle
    /// resolves `TimedOut` promptly.
    #[test]
    fn cancel_observed_no_orphan() {
        let observed = Arc::new(AtomicBool::new(false));
        let backend = LocalBackend::with_transport(local_config(), Box::new(BlockingTransport(observed.clone())));
        let queue = JobQueue::new();
        let request = sample_request();

        let start = Instant::now();
        let handle = queue.submit(Duration::from_millis(50), move |cancel| backend.generate(&request, cancel));
        let status = handle.wait();

        assert_eq!(status, JobStatus::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout must resolve promptly, took {:?}",
            start.elapsed()
        );
        assert!(
            observed.load(Ordering::SeqCst),
            "transport must observe the cancel flag rather than being abandoned"
        );
    }
}
