//! The text-local async job lifecycle: `JobHandle` (poll / blocking wait),
//! `JobStatus` (its terminal outcomes), `CancelFlag` (cooperative
//! cancellation a `TextBackend::generate` call observes), and `JobQueue`,
//! the single-worker serial scheduler that enforces the one-job-at-a-time
//! guarantee and the wall-clock timeout. This mirrors `asset_gen::job`'s
//! shape but owns its own types end to end: the work unit is
//! `FnOnce(&CancelFlag) -> Result<String, TextError>` rather than an
//! `SdCliInvocation`, so no asset_gen type is reused.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::types::TextError;

/// A cooperative cancellation signal a `TextBackend::generate` call polls so
/// a timed-out job tears down its work instead of running forever.
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

/// The terminal (or pending) outcome of a submitted job. A job resolves to
/// exactly one of `Success`, `Failed`, or `TimedOut`; it never stalls
/// silently in `Pending` forever.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobStatus<T> {
    Pending,
    Success(T),
    Failed(TextError),
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
    /// whose result is known without going through the queue (a cache hit).
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

/// A terminal outcome the worker hands back to the submitter's `complete`
/// callback, distinguishing the work unit's own error from a
/// scheduler-enforced timeout.
enum JobFailure {
    Error(TextError),
    TimedOut,
}

/// A submitted work unit, already closed over its backend and request.
type Work = Box<dyn FnOnce(&CancelFlag) -> Result<String, TextError> + Send>;

/// The type-erased callback that resolves a submitter's `JobHandle`.
type Complete = Box<dyn FnOnce(Result<String, JobFailure>) + Send>;

/// One enqueued job: the work unit, its wall-clock bound, and the callback
/// that resolves the submitter's `JobHandle`.
struct QueuedJob {
    work: Work,
    timeout: Duration,
    complete: Complete,
}

/// The single serial scheduler: one worker thread drains one queue, fully
/// resolving each job (including cancelling and joining a timed-out work
/// unit) before starting the next.
pub struct JobQueue {
    tx: Sender<QueuedJob>,
    _worker: JoinHandle<()>,
}

impl JobQueue {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<QueuedJob>();
        let worker = thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                let QueuedJob {
                    work,
                    timeout,
                    complete,
                } = job;

                let cancel = CancelFlag::new();
                let run_cancel = cancel.clone();
                let (result_tx, result_rx) = mpsc::channel();
                let child = thread::spawn(move || {
                    let result = work(&run_cancel);
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

    /// Submits one job. `work` is handed the queue's `CancelFlag` so it can
    /// observe cooperative cancellation; on timeout the queue cancels and
    /// joins `work` before starting the next submission.
    pub fn submit<F>(&self, timeout: Duration, work: F) -> JobHandle<String>
    where
        F: FnOnce(&CancelFlag) -> Result<String, TextError> + Send + 'static,
    {
        let slot = Arc::new(JobSlot::new());
        let resolve_slot = Arc::clone(&slot);
        let complete: Complete = Box::new(move |result| {
            let status = match result {
                Ok(text) => JobStatus::Success(text),
                Err(JobFailure::Error(e)) => JobStatus::Failed(e),
                Err(JobFailure::TimedOut) => JobStatus::TimedOut,
            };
            resolve_slot.resolve(status);
        });

        // The worker thread only exits if the queue itself has been dropped,
        // in which case there is no handle left to observe a send failure.
        let _ = self.tx.send(QueuedJob {
            work: Box::new(work),
            timeout,
            complete,
        });

        JobHandle { slot }
    }
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::super::backend::TextBackend;
    use super::super::types::TextRequest;
    use super::*;

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

    /// A `TextBackend` whose `generate` behavior is supplied by a closure,
    /// so each test drives a distinct success/error/timeout scenario
    /// without a real subprocess or HTTP call.
    struct FnBackend<F>(F)
    where
        F: Fn(&TextRequest, &CancelFlag) -> Result<String, TextError> + Send + Sync + 'static;

    impl<F> TextBackend for FnBackend<F>
    where
        F: Fn(&TextRequest, &CancelFlag) -> Result<String, TextError> + Send + Sync + 'static,
    {
        fn generate(&self, request: &TextRequest, cancel: &CancelFlag) -> Result<String, TextError> {
            (self.0)(request, cancel)
        }
    }

    /// A backend that returns success resolves the handle to `Success` with
    /// its text.
    #[test]
    fn submit_success_resolves() {
        let backend = FnBackend(|_req, _cancel| Ok("ok".to_string()));
        let queue = JobQueue::new();
        let request = sample_request();
        let handle: JobHandle<String> =
            queue.submit(Duration::from_secs(2), move |cancel| backend.generate(&request, cancel));
        assert_eq!(handle.wait(), JobStatus::Success("ok".to_string()));
    }

    /// A backend that returns a structured error resolves the handle to
    /// `Failed` with that error, not a hang.
    #[test]
    fn submit_error_resolves() {
        let backend = FnBackend(|_req, _cancel| {
            Err(TextError::Process {
                code: Some(1),
                stderr: "model crashed".into(),
            })
        });
        let queue = JobQueue::new();
        let request = sample_request();
        let handle: JobHandle<String> =
            queue.submit(Duration::from_secs(2), move |cancel| backend.generate(&request, cancel));
        assert_eq!(
            handle.wait(),
            JobStatus::Failed(TextError::Process {
                code: Some(1),
                stderr: "model crashed".into(),
            })
        );
    }

    /// A backend that blocks past the supplied timeout (honoring the cancel
    /// flag) resolves to `TimedOut` promptly, never an indefinite wait.
    #[test]
    fn submit_timeout_fires() {
        let backend = FnBackend(|_req, cancel| {
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(TextError::Config("cancelled".into()))
        });
        let queue = JobQueue::new();
        let request = sample_request();
        let start = Instant::now();
        let handle: JobHandle<String> =
            queue.submit(Duration::from_millis(50), move |cancel| backend.generate(&request, cancel));
        let status = handle.wait();
        assert_eq!(status, JobStatus::TimedOut);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout must resolve promptly, took {:?}",
            start.elapsed()
        );
    }

    /// The backend observes the cancel flag on the timeout path rather than
    /// being abandoned, so a real subprocess/HTTP call would be torn down,
    /// not orphaned.
    #[test]
    fn cancel_flag_observed_on_timeout() {
        let observed = Arc::new(AtomicBool::new(false));
        let observed_write = observed.clone();
        let backend = FnBackend(move |_req, cancel| {
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            observed_write.store(true, Ordering::SeqCst);
            Err(TextError::Config("cancelled".into()))
        });
        let queue = JobQueue::new();
        let request = sample_request();
        let handle: JobHandle<String> =
            queue.submit(Duration::from_millis(50), move |cancel| backend.generate(&request, cancel));
        assert_eq!(handle.wait(), JobStatus::TimedOut);
        assert!(
            observed.load(Ordering::SeqCst),
            "backend must observe the cancel flag rather than being abandoned"
        );
    }

    /// A timed-out job's work unit is cancelled and joined before the next
    /// submission starts, so two model processes never run at once.
    #[test]
    fn timed_out_job_is_joined_before_next_job_starts() {
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let queue = JobQueue::new();

        let log1 = log.clone();
        let handle1: JobHandle<String> = queue.submit(Duration::from_millis(50), move |cancel| {
            log1.lock().unwrap().push("job1:start".to_string());
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            log1.lock().unwrap().push("job1:cancelled".to_string());
            Err(TextError::Config("cancelled".into()))
        });

        let log2 = log.clone();
        let handle2: JobHandle<String> = queue.submit(Duration::from_secs(2), move |_cancel| {
            log2.lock().unwrap().push("job2:start".to_string());
            Ok("job2 done".to_string())
        });

        assert_eq!(handle1.wait(), JobStatus::TimedOut);
        assert_eq!(handle2.wait(), JobStatus::Success("job2 done".to_string()));

        let recorded = log.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec!["job1:start", "job1:cancelled", "job2:start"],
            "job2 must not start until job1's work unit observed cancellation and was joined, got {recorded:?}"
        );
    }

    /// `poll` reports `Pending` while the backend is still working, then the
    /// terminal status once it completes.
    #[test]
    fn poll_reports_pending_then_resolved() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let gate_wait = gate.clone();
        let backend = FnBackend(move |_req, _cancel| {
            let (lock, cv) = &*gate_wait;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = cv.wait(released).unwrap();
            }
            Ok("done".to_string())
        });
        let queue = JobQueue::new();
        let request = sample_request();
        let handle: JobHandle<String> =
            queue.submit(Duration::from_secs(2), move |cancel| backend.generate(&request, cancel));

        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(handle.poll(), JobStatus::Pending);

        {
            let (lock, cv) = &*gate;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }

        assert_eq!(handle.wait(), JobStatus::Success("done".to_string()));
    }

    /// `JobHandle::resolved` reports its status without going through a
    /// queue at all, for a cache-hit caller.
    #[test]
    fn resolved_reports_status_without_a_queue() {
        let success: JobHandle<String> = JobHandle::resolved(JobStatus::Success("cached".to_string()));
        assert_eq!(success.wait(), JobStatus::Success("cached".to_string()));

        let timed_out: JobHandle<String> = JobHandle::resolved(JobStatus::TimedOut);
        assert_eq!(timed_out.wait(), JobStatus::TimedOut);
    }
}
