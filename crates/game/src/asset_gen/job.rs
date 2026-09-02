//! The async job lifecycle: `JobHandle` (poll / blocking wait), `JobStatus`
//! (its terminal outcomes), and `JobQueue`, the single-worker serial
//! scheduler that enforces the one-job-at-a-time guarantee and the
//! wall-clock timeout. `JobQueue` is the sole scheduler and `JobRunner` (see
//! `runner.rs`) the sole execution seam, so parallelism is structurally
//! impossible: there is exactly one worker.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::recipe::SdCliInvocation;
use super::runner::{CancelFlag, JobError, JobRunner, RunOutput};

/// The terminal (or pending) outcome of a submitted job. A job resolves to
/// exactly one of `Success`, `Failed`, or `TimedOut`; it never stalls
/// silently in `Pending` forever.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobStatus<T> {
    Pending,
    Success(T),
    Failed(JobError),
    TimedOut,
}

/// Shared resolution state behind a `JobHandle`: the current status plus a
/// condvar so `wait` can block without polling.
struct JobSlot<T> {
    state: Mutex<JobStatus<T>>,
    cv: Condvar,
}

impl<T> JobSlot<T> {
    fn new() -> Self {
        JobSlot {
            state: Mutex::new(JobStatus::Pending),
            cv: Condvar::new(),
        }
    }

    fn resolve(&self, status: JobStatus<T>) {
        let mut guard = self.state.lock().unwrap();
        *guard = status;
        self.cv.notify_all();
    }
}

/// A handle to one submitted job's eventual result.
pub struct JobHandle<T> {
    slot: Arc<JobSlot<T>>,
}

impl<T> JobHandle<T> {
    /// Builds a handle that is already resolved to `status`, for a caller
    /// whose result is known without going through the queue (a cache hit,
    /// an import, or a gating failure like no GPU available).
    pub fn resolved(status: JobStatus<T>) -> Self {
        let slot = JobSlot::new();
        slot.resolve(status);
        JobHandle { slot: Arc::new(slot) }
    }
}

impl<T: Clone> JobHandle<T> {
    /// Non-blocking snapshot of the job's current status.
    pub fn poll(&self) -> JobStatus<T> {
        self.slot.state.lock().unwrap().clone()
    }

    /// Blocks until the job resolves. Never returns `Pending`.
    pub fn wait(&self) -> JobStatus<T> {
        let guard = self.slot.state.lock().unwrap();
        let guard = self
            .slot
            .cv
            .wait_while(guard, |status| matches!(status, JobStatus::Pending))
            .unwrap();
        guard.clone()
    }
}

/// A terminal outcome the worker hands back to a job's `complete` callback,
/// distinguishing a runner-reported error from a scheduler-enforced timeout.
enum JobFailure {
    Error(JobError),
    TimedOut,
}

/// One enqueued job: the invocation to run, its wall-clock bound, and the
/// type-erased callback that resolves the submitter's `JobHandle`.
struct QueuedJob {
    invocation: SdCliInvocation,
    timeout: Duration,
    complete: Box<dyn FnOnce(Result<RunOutput, JobFailure>) + Send>,
}

/// The single serial scheduler: one worker thread drains one queue, fully
/// resolving each job (including its wall-clock timeout) before starting
/// the next. `submit` requires `timeout` so no caller can silently create
/// an unbounded job.
pub struct JobQueue {
    tx: Sender<QueuedJob>,
    _worker: JoinHandle<()>,
}

impl JobQueue {
    pub fn new(runner: Arc<dyn JobRunner>) -> Self {
        let (tx, rx) = mpsc::channel::<QueuedJob>();
        let worker = thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                let QueuedJob {
                    invocation,
                    timeout,
                    complete,
                } = job;

                let cancel = CancelFlag::new();
                let run_cancel = cancel.clone();
                let run_runner = Arc::clone(&runner);
                let (result_tx, result_rx) = mpsc::channel();
                let child = thread::spawn(move || {
                    let result = run_runner.run(&invocation, &run_cancel);
                    let _ = result_tx.send(result);
                });

                match result_rx.recv_timeout(timeout) {
                    Ok(result) => {
                        let _ = child.join();
                        complete(result.map_err(JobFailure::Error));
                    }
                    Err(_) => {
                        cancel.cancel();
                        let _ = child.join();
                        complete(Err(JobFailure::TimedOut));
                    }
                }
            }
        });

        JobQueue { tx, _worker: worker }
    }

    /// Submits one job. `materialize` converts the runner's raw output into
    /// the operation's asset handle.
    pub fn submit<T, F>(&self, invocation: SdCliInvocation, timeout: Duration, materialize: F) -> JobHandle<T>
    where
        T: Send + 'static,
        F: FnOnce(RunOutput) -> T + Send + 'static,
    {
        let slot = Arc::new(JobSlot::new());
        let resolve_slot = Arc::clone(&slot);
        let complete: Box<dyn FnOnce(Result<RunOutput, JobFailure>) + Send> = Box::new(move |result| {
            let status = match result {
                Ok(out) => JobStatus::Success(materialize(out)),
                Err(JobFailure::Error(e)) => JobStatus::Failed(e),
                Err(JobFailure::TimedOut) => JobStatus::TimedOut,
            };
            resolve_slot.resolve(status);
        });

        // The worker thread only exits if the queue itself has been dropped,
        // in which case there is no handle left to observe a send failure.
        let _ = self.tx.send(QueuedJob {
            invocation,
            timeout,
            complete,
        });

        JobHandle { slot }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::super::runner::CancelFlag;
    use super::*;

    /// A `JobRunner` whose behavior is supplied by a closure, so each test
    /// can drive a distinct success/error/timeout/serial scenario without a
    /// real subprocess.
    struct FnRunner<F>(F)
    where
        F: Fn(&SdCliInvocation, &CancelFlag) -> Result<RunOutput, JobError> + Send + Sync + 'static;

    impl<F> JobRunner for FnRunner<F>
    where
        F: Fn(&SdCliInvocation, &CancelFlag) -> Result<RunOutput, JobError> + Send + Sync + 'static,
    {
        fn run(&self, invocation: &SdCliInvocation, cancel: &CancelFlag) -> Result<RunOutput, JobError> {
            (self.0)(invocation, cancel)
        }
    }

    fn fn_runner<F>(f: F) -> Arc<dyn JobRunner>
    where
        F: Fn(&SdCliInvocation, &CancelFlag) -> Result<RunOutput, JobError> + Send + Sync + 'static,
    {
        Arc::new(FnRunner(f))
    }

    /// Builds a minimal invocation whose one arg (`label`) doubles as the
    /// job label these tests read back from `inv.args[0]`.
    fn invocation(label: &str) -> SdCliInvocation {
        SdCliInvocation {
            args: vec![label.to_string()],
        }
    }

    /// A runner that returns success resolves the handle to `Success` with
    /// the materialized output.
    #[test]
    fn submit_success_resolves() {
        let queue = JobQueue::new(fn_runner(|_inv, _cancel| {
            Ok(RunOutput { stdout: "ok".into() })
        }));
        let handle: JobHandle<String> =
            queue.submit(invocation("job"), Duration::from_secs(2), |out| out.stdout);
        assert_eq!(handle.wait(), JobStatus::Success("ok".to_string()));
    }

    /// A runner that returns a process error (e.g. out-of-VRAM) resolves the
    /// handle to `Failed` with that error, not a hang.
    #[test]
    fn submit_error_resolves() {
        let queue = JobQueue::new(fn_runner(|_inv, _cancel| {
            Err(JobError::Process {
                code: Some(1),
                stderr: "out of vram".into(),
            })
        }));
        let handle: JobHandle<String> =
            queue.submit(invocation("job"), Duration::from_secs(2), |out| out.stdout);
        assert_eq!(
            handle.wait(),
            JobStatus::Failed(JobError::Process {
                code: Some(1),
                stderr: "out of vram".into(),
            })
        );
    }

    /// A runner that blocks past the supplied timeout (honoring the cancel
    /// flag) resolves to `TimedOut` promptly, never an indefinite wait.
    #[test]
    fn submit_timeout_fires() {
        let queue = JobQueue::new(fn_runner(|_inv, cancel| {
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(JobError::Cancelled)
        }));
        let start = Instant::now();
        let handle: JobHandle<String> =
            queue.submit(invocation("job"), Duration::from_millis(50), |out| out.stdout);
        let status = handle.wait();
        assert_eq!(status, JobStatus::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout must resolve promptly, took {:?}",
            start.elapsed()
        );
    }

    /// The runner observes the cancel flag on the timeout path rather than
    /// being abandoned, so a real subprocess would be killed, not orphaned.
    #[test]
    fn cancel_flag_observed_on_timeout() {
        let observed = Arc::new(AtomicBool::new(false));
        let observed_write = observed.clone();
        let queue = JobQueue::new(fn_runner(move |_inv, cancel| {
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            observed_write.store(true, Ordering::SeqCst);
            Err(JobError::Cancelled)
        }));
        let handle: JobHandle<String> =
            queue.submit(invocation("job"), Duration::from_millis(50), |out| out.stdout);
        assert_eq!(handle.wait(), JobStatus::TimedOut);
        assert!(
            observed.load(Ordering::SeqCst),
            "runner must observe the cancel flag rather than being abandoned"
        );
    }

    /// Two jobs submitted back-to-back through one queue run serially: the
    /// second does not start until the first has fully resolved.
    #[test]
    fn jobs_run_serially() {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log_write = log.clone();
        let queue = JobQueue::new(fn_runner(move |inv, _cancel| {
            log_write.lock().unwrap().push(format!("start:{}", inv.args[0]));
            std::thread::sleep(Duration::from_millis(30));
            log_write.lock().unwrap().push(format!("end:{}", inv.args[0]));
            Ok(RunOutput {
                stdout: inv.args[0].clone(),
            })
        }));

        let h1: JobHandle<String> =
            queue.submit(invocation("job1"), Duration::from_secs(2), |out| out.stdout);
        let h2: JobHandle<String> =
            queue.submit(invocation("job2"), Duration::from_secs(2), |out| out.stdout);

        assert_eq!(h1.wait(), JobStatus::Success("job1".to_string()));
        assert_eq!(h2.wait(), JobStatus::Success("job2".to_string()));

        let recorded = log.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec!["start:job1", "end:job1", "start:job2", "end:job2"],
            "job2 must not start until job1 has fully resolved, got {recorded:?}"
        );
    }

    /// `poll` reports `Pending` while the runner is still working, then the
    /// terminal status once it completes, feeding per-sub-job progress.
    #[test]
    fn poll_reports_pending_then_resolved() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let gate_wait = gate.clone();
        let queue = JobQueue::new(fn_runner(move |_inv, _cancel| {
            let (lock, cv) = &*gate_wait;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = cv.wait(released).unwrap();
            }
            Ok(RunOutput { stdout: "done".into() })
        }));

        let handle: JobHandle<String> =
            queue.submit(invocation("gated"), Duration::from_secs(2), |out| out.stdout);

        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(handle.poll(), JobStatus::Pending);

        {
            let (lock, cv) = &*gate;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }

        assert_eq!(handle.wait(), JobStatus::Success("done".to_string()));
    }
}
