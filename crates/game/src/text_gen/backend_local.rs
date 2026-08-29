//! The local subprocess `TextBackend`: drives the configured local model
//! out-of-process via the sibling-binary pattern, fully on the player's
//! machine. The prompt (system+user) is piped to the child's stdin as the
//! opaque payload; the program and sampling flags form the scanned argv
//! envelope, so caller content can never leak an affordance into argv.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::backend::TextBackend;
use super::conformance::{CaptureBackend, NormalizedRequest};
use super::job::CancelFlag;
use super::types::{ResolvedModelConfig, TextError, TextRequest};

/// Fixed poll interval the sibling-binary transport uses while waiting on
/// the child process and checking for cancellation.
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A fully-formed local-model subprocess invocation. `program` and `flags`
/// are the scanned argv envelope; `prompt` (system+user, piped to stdin) is
/// the opaque payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalInvocation {
    pub program: String,
    pub flags: Vec<String>,
    pub prompt: String,
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
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TextError::Spawn(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(invocation.prompt.as_bytes());
        }

        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(TextError::Io("cancelled".to_string()));
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout = String::new();
                    if let Some(mut out) = child.stdout.take() {
                        let _ = out.read_to_string(&mut stdout);
                    }
                    if status.success() {
                        return Ok(stdout);
                    }
                    let mut stderr = String::new();
                    if let Some(mut err) = child.stderr.take() {
                        let _ = err.read_to_string(&mut stderr);
                    }
                    return Err(TextError::Process {
                        code: status.code(),
                        stderr,
                    });
                }
                Ok(None) => std::thread::sleep(POLL_INTERVAL),
                Err(e) => return Err(TextError::Io(e.to_string())),
            }
        }
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
        let program = self
            .config
            .local_command()
            .ok_or_else(|| TextError::Config("no local command configured".to_string()))?
            .to_string();

        let mut flags = vec![
            "--temperature".to_string(),
            request.temperature.to_string(),
            "--max-tokens".to_string(),
            request.max_tokens.to_string(),
        ];
        if let Some(seed) = request.seed {
            flags.push("--seed".to_string());
            flags.push(seed.to_string());
        }
        for stop in &request.stop {
            flags.push("--stop".to_string());
            flags.push(stop.clone());
        }

        let prompt = if request.system.is_empty() {
            request.user.clone()
        } else {
            format!("{}\n\n{}", request.system, request.user)
        };

        Ok(LocalInvocation { program, flags, prompt })
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

    fn sample_request() -> TextRequest {
        TextRequest {
            system: "you are a battle narrator".into(),
            user: "describe the opening move".into(),
            temperature: 0.7,
            max_tokens: 128,
            stop: Vec::new(),
            seed: None,
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
    /// invocation carries the configured program plus the sampling flags in
    /// its envelope.
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
            envelope.iter().any(|v| v.contains("--temperature")),
            "envelope must contain --temperature, got {envelope:?}"
        );
        assert!(
            envelope.iter().any(|v| v.contains("--max-tokens")),
            "envelope must contain --max-tokens, got {envelope:?}"
        );
    }

    /// The stdin payload carries both the system and user text; neither
    /// leaks into the scanned argv envelope.
    #[test]
    fn prompt_frames_system_and_user() {
        let backend = LocalBackend::with_transport(local_config(), Box::new(CannedTransport::ok("text")));
        let cancel = CancelFlag::new();
        let request = TextRequest {
            system: "you are terse".into(),
            user: "describe the opening move".into(),
            temperature: 0.1,
            max_tokens: 16,
            stop: Vec::new(),
            seed: None,
        };

        backend.generate(&request, &cancel).expect("canned transport always succeeds");

        let captured = backend.captured_request().expect("backend must capture the invocation");
        assert!(
            captured
                .payload
                .iter()
                .any(|p| p.contains("you are terse") && p.contains("describe the opening move")),
            "payload must contain both system and user text, got {:?}",
            captured.payload
        );
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();
        assert!(
            !envelope.iter().any(|v| v.contains("you are terse")),
            "system/user content must not leak into the envelope, got {envelope:?}"
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
