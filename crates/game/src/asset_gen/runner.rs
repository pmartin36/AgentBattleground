//! The job-runner execution seam: given a fully-formed `SdCliInvocation`
//! (model choice and prompt/args already decided by a `RecipeBackend`), a
//! `JobRunner` drives execution to completion, observing a `CancelFlag` so a
//! timed-out or aborted job tears down instead of hanging. `SdCliRunner` is
//! the production implementation that drives the sibling `sd-cli`
//! subprocess; `JobQueue` (see `job.rs`) is the sole caller.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::recipe::SdCliInvocation;

/// Fixed poll interval `SdCliRunner` uses while waiting on the child
/// process and checking for cancellation.
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Minimal success payload from a completed run. Produced file paths are
/// known to the caller from the output path it placed in the invocation
/// args, so this stays deliberately thin.
#[derive(Clone, Debug)]
pub struct RunOutput {
    pub stdout: String,
}

/// A terminal run failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobError {
    /// Could not find or launch the sd-cli sibling binary.
    Spawn(String),
    /// The process exited non-zero (includes out-of-VRAM/OOM failures).
    Process { code: Option<i32>, stderr: String },
    /// A wait/read failure talking to the child process.
    Io(String),
    /// Aborted via `CancelFlag`; the scheduler surfaces this as `TimedOut`.
    Cancelled,
}

/// A cooperative cancellation signal a `JobRunner` polls so a timed-out or
/// aborted job tears down its work instead of running forever.
#[derive(Clone)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        CancelFlag(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Default for CancelFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// Executes one `SdCliInvocation` to completion, observing `cancel` so it
/// can tear down promptly instead of hanging. Implementations own no
/// model/prompt logic; that is already decided in the invocation.
pub trait JobRunner: Send + Sync + 'static {
    fn run(&self, invocation: &SdCliInvocation, cancel: &CancelFlag) -> Result<RunOutput, JobError>;
}

/// Drives the sibling `sd-cli` subprocess. Not exercised by this crate's own
/// tests (no real subprocess in the gate); `JobQueue` tests inject a fake
/// `JobRunner` instead.
pub struct SdCliRunner {
    bin: PathBuf,
}

impl SdCliRunner {
    /// Locates `sd-cli` next to the current executable, mirroring the
    /// engine's sibling-binary lookup for a different binary name.
    pub fn sibling() -> std::io::Result<Self> {
        let exe = std::env::current_exe()?;
        let dir = exe.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "current_exe has no parent directory")
        })?;
        Ok(SdCliRunner { bin: dir.join("sd-cli") })
    }

    pub fn with_bin(bin: PathBuf) -> Self {
        SdCliRunner { bin }
    }
}

impl JobRunner for SdCliRunner {
    fn run(&self, invocation: &SdCliInvocation, cancel: &CancelFlag) -> Result<RunOutput, JobError> {
        let mut child = Command::new(&self.bin)
            .arg(&invocation.model)
            .args(&invocation.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| JobError::Spawn(e.to_string()))?;

        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(JobError::Cancelled);
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout = String::new();
                    if let Some(mut out) = child.stdout.take() {
                        let _ = out.read_to_string(&mut stdout);
                    }
                    if status.success() {
                        return Ok(RunOutput { stdout });
                    }
                    let mut stderr = String::new();
                    if let Some(mut err) = child.stderr.take() {
                        let _ = err.read_to_string(&mut stderr);
                    }
                    return Err(JobError::Process {
                        code: status.code(),
                        stderr,
                    });
                }
                Ok(None) => std::thread::sleep(POLL_INTERVAL),
                Err(e) => return Err(JobError::Io(e.to_string())),
            }
        }
    }
}
