//! `backend_for`, the single `TextBackend` construction path keyed on
//! `Provider`, its conformance-enforcement test, and the public
//! `generate_text` entry point.

use std::sync::Arc;
use std::time::Duration;

use super::backend::TextBackend;
use super::backend_local::LocalBackend;
use super::backend_online::OnlineBackend;
use super::cache::TextCache;
use super::job::{JobHandle, JobQueue, JobStatus};
use super::types::{Provider, ResolvedModelConfig, TextRequest};

/// Text generation is model-latency-bound; a fake backend in tests resolves
/// in milliseconds, well under this bound.
pub const DEFAULT_TEXT_TIMEOUT: Duration = Duration::from_secs(120);

/// Whether a `generate_text` call engages the opt-in cache. Off is the
/// default: a battle turn is unique and non-deterministic, so caching would
/// be wrong. Cached is opt-in for deterministic seed-pinned reuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CachePolicy {
    Off,
    Cached,
}

/// Which backend implements a provider. A closed set so a new backend kind
/// forces the enforcement test's transport-injection match to grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Local,
    Online,
}

/// The ONE mapping `Provider` -> backend kind, wildcard-free. `backend_for`
/// and the conformance-enforcement test both dispatch on this, so they
/// cannot diverge; a new `Provider` variant fails to compile here until
/// handled.
pub fn backend_kind(provider: Provider) -> BackendKind {
    match provider {
        Provider::Local => BackendKind::Local,
        Provider::Claude | Provider::OpenAi | Provider::OpenAiCompatible => BackendKind::Online,
    }
}

/// THE single production construction path. Nothing else constructs a
/// `TextBackend` in production. Keyed on the closed `Provider` enum via
/// `backend_kind`.
pub fn backend_for(config: ResolvedModelConfig) -> Box<dyn TextBackend> {
    match backend_kind(config.provider()) {
        BackendKind::Local => Box::new(LocalBackend::new(config)),
        BackendKind::Online => Box::new(OnlineBackend::new(config)),
    }
}

/// Factory the API calls at submit time to obtain the routed backend.
/// Production = `backend_for`; tests inject a call-counting/capturing fake.
type BackendFactory = Box<dyn Fn(&ResolvedModelConfig) -> Box<dyn TextBackend> + Send + Sync>;

/// The text-generation API. Constructed with an injected
/// `ResolvedModelConfig` (never reads a settings file); owns the serial job
/// queue, the opt-in cache, and the timeout. `generate_text` is the single
/// public entry point.
pub struct TextGen {
    config: ResolvedModelConfig,
    make_backend: BackendFactory,
    queue: JobQueue,
    cache: Arc<TextCache>,
    timeout: Duration,
}

impl TextGen {
    /// Production: routes via `backend_for` on the injected config.
    pub fn new(config: ResolvedModelConfig) -> Self {
        Self::with_backend_factory(
            config,
            Box::new(|c: &ResolvedModelConfig| backend_for(c.clone())),
            DEFAULT_TEXT_TIMEOUT,
        )
    }

    /// Test/advanced: inject the backend factory (a counting fake) and
    /// timeout.
    pub fn with_backend_factory(config: ResolvedModelConfig, make_backend: BackendFactory, timeout: Duration) -> Self {
        TextGen {
            config,
            make_backend,
            queue: JobQueue::new(),
            cache: Arc::new(TextCache::new()),
            timeout,
        }
    }

    /// Submit a request. Off (default): the backend runs every time, cache
    /// untouched. Cached: a hit returns a resolved handle without routing
    /// or running the backend; a miss submits, and only a successful result
    /// is stored.
    pub fn generate_text(&self, request: TextRequest, cache: CachePolicy) -> JobHandle<String> {
        if let CachePolicy::Cached = cache {
            if let Some(text) = self.cache.get(&self.config, &request) {
                return JobHandle::resolved(JobStatus::Success(text));
            }
        }

        let backend = (self.make_backend)(&self.config);
        let timeout = self.timeout;

        match cache {
            CachePolicy::Off => self.queue.submit(timeout, move |cancel| backend.generate(&request, cancel)),
            CachePolicy::Cached => {
                let cache = Arc::clone(&self.cache);
                let config = self.config.clone();
                self.queue
                    .submit(timeout, move |cancel| cache.get_or_bake(&config, &request, || backend.generate(&request, cancel)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::super::backend_local::{LocalBackend, LocalInvocation, LocalTransport};
    use super::super::backend_online::{HttpRequest, HttpResponse, HttpTransport, OnlineBackend};
    use super::super::conformance::{assert_text_backend_conforms, CaptureBackend};
    use super::super::job::{CancelFlag, JobStatus};
    use super::super::types::TextError;
    use super::*;

    fn config_for(provider: Provider) -> ResolvedModelConfig {
        match provider {
            Provider::Local => {
                ResolvedModelConfig::new(Provider::Local, "local-model", None, Some("local-model-bin".to_string()), None)
            }
            Provider::Claude => ResolvedModelConfig::new(Provider::Claude, "claude-3", Some("key".to_string()), None, None),
            Provider::OpenAi => ResolvedModelConfig::new(Provider::OpenAi, "gpt-4", Some("key".to_string()), None, None),
            Provider::OpenAiCompatible => ResolvedModelConfig::new(
                Provider::OpenAiCompatible,
                "compat-model",
                Some("key".to_string()),
                None,
                Some("https://example.com/v1".to_string()),
            ),
        }
    }

    fn sample_request() -> TextRequest {
        TextRequest {
            system: "you are a battle narrator".to_string(),
            user: "describe the opening move".to_string(),
            temperature: 0.7,
            max_tokens: 128,
            stop: Vec::new(),
            seed: None,
        }
    }

    fn seed_pinned_request() -> TextRequest {
        TextRequest {
            seed: Some(42),
            ..sample_request()
        }
    }

    /// A `TextBackend` fixture that always returns a fixed string.
    struct FixedBackend(String);

    impl TextBackend for FixedBackend {
        fn generate(&self, _request: &TextRequest, _cancel: &CancelFlag) -> Result<String, TextError> {
            Ok(self.0.clone())
        }
    }

    /// A `TextBackend` fixture that records every request it is called
    /// with and returns a fixed string.
    struct RecordingBackend {
        calls: Arc<Mutex<Vec<TextRequest>>>,
        text: String,
    }

    impl TextBackend for RecordingBackend {
        fn generate(&self, request: &TextRequest, _cancel: &CancelFlag) -> Result<String, TextError> {
            self.calls.lock().unwrap().push(request.clone());
            Ok(self.text.clone())
        }
    }

    /// A `TextBackend` fixture that counts calls and returns a shared,
    /// mutable canned result (so a test can flip success/failure between
    /// calls).
    struct CountingBackend {
        calls: Arc<AtomicUsize>,
        result: Arc<Mutex<Result<String, TextError>>>,
    }

    impl TextBackend for CountingBackend {
        fn generate(&self, _request: &TextRequest, _cancel: &CancelFlag) -> Result<String, TextError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.lock().unwrap().clone()
        }
    }

    /// A `LocalTransport` fixture that records the invocation it was run
    /// with and reports success, for driving the conformance harness
    /// without a real subprocess.
    struct CapturingLocalTransport {
        captured: Mutex<Option<LocalInvocation>>,
    }

    impl CapturingLocalTransport {
        fn new() -> Self {
            CapturingLocalTransport { captured: Mutex::new(None) }
        }
    }

    impl LocalTransport for CapturingLocalTransport {
        fn run(&self, invocation: &LocalInvocation, _cancel: &CancelFlag) -> Result<String, TextError> {
            *self.captured.lock().unwrap() = Some(invocation.clone());
            Ok("ok".to_string())
        }

        fn captured(&self) -> Option<LocalInvocation> {
            self.captured.lock().unwrap().clone()
        }
    }

    /// An `HttpTransport` fixture that records the request it was sent and
    /// reports a 200 response body shaped to satisfy Claude AND OpenAI
    /// (-compatible) parsing, for driving the conformance harness against
    /// every online provider without a real network call.
    struct CapturingHttpTransport {
        captured: Mutex<Option<HttpRequest>>,
    }

    impl CapturingHttpTransport {
        fn new() -> Self {
            CapturingHttpTransport { captured: Mutex::new(None) }
        }
    }

    impl HttpTransport for CapturingHttpTransport {
        fn send(&self, request: &HttpRequest, _cancel: &CancelFlag) -> Result<HttpResponse, TextError> {
            *self.captured.lock().unwrap() = Some(request.clone());
            Ok(HttpResponse {
                status: 200,
                body: r#"{"content":[{"text":"ok"}],"choices":[{"message":{"content":"ok"}}]}"#.to_string(),
            })
        }

        fn captured(&self) -> Option<HttpRequest> {
            self.captured.lock().unwrap().clone()
        }
    }

    /// `backend_kind` maps `Local` to the local backend and every online
    /// provider (`Claude`/`OpenAi`/`OpenAiCompatible`) to the online
    /// backend.
    #[test]
    fn backend_kind_routes_each_provider() {
        assert_eq!(backend_kind(Provider::Local), BackendKind::Local);
        assert_eq!(backend_kind(Provider::Claude), BackendKind::Online);
        assert_eq!(backend_kind(Provider::OpenAi), BackendKind::Online);
        assert_eq!(backend_kind(Provider::OpenAiCompatible), BackendKind::Online);
    }

    /// `backend_for` constructs a backend for every `Provider` variant
    /// without panicking.
    #[test]
    fn backend_for_constructs_every_variant() {
        for provider in Provider::ALL {
            let config = config_for(provider);
            let _backend: Box<dyn TextBackend> = backend_for(config);
        }
    }

    /// The structural enforcement: for every `Provider` variant, a backend
    /// built through the same routing decision (`backend_kind`) over a
    /// capturing transport passes the shared conformance harness. A variant
    /// routed to a non-conforming or absent backend fails this test.
    #[test]
    fn every_provider_backend_conforms() {
        for provider in Provider::ALL {
            let config = config_for(provider);
            let backend: Box<dyn CaptureBackend> = match backend_kind(provider) {
                BackendKind::Local => {
                    Box::new(LocalBackend::with_transport(config, Box::new(CapturingLocalTransport::new())))
                }
                BackendKind::Online => {
                    Box::new(OnlineBackend::with_transport(config, Box::new(CapturingHttpTransport::new())))
                }
            };
            assert_text_backend_conforms(&*backend);
        }
    }

    /// `generate_text` routes the request through the injected backend
    /// factory and resolves the handle to that backend's text.
    #[test]
    fn generate_text_routes_to_backend_and_returns_text() {
        let calls: Arc<Mutex<Vec<TextRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let calls_factory = calls.clone();
        let text_gen = TextGen::with_backend_factory(
            config_for(Provider::Local),
            Box::new(move |_cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                Box::new(RecordingBackend {
                    calls: calls_factory.clone(),
                    text: "generated".to_string(),
                })
            }),
            Duration::from_secs(2),
        );

        let request = sample_request();
        let handle = text_gen.generate_text(request.clone(), CachePolicy::Off);
        assert_eq!(handle.wait(), JobStatus::Success("generated".to_string()));
        assert_eq!(calls.lock().unwrap().as_slice(), &[request]);
    }

    /// A config selecting the local provider reaches a local-kind backend;
    /// a config selecting an online provider reaches an online-kind
    /// backend, verified via a factory that dispatches on `backend_kind`
    /// and a counting fake per kind.
    #[test]
    fn generate_text_local_vs_online_reaches_corresponding_backend() {
        fn make_factory(local_calls: Arc<AtomicUsize>, online_calls: Arc<AtomicUsize>) -> BackendFactory {
            Box::new(move |cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                match backend_kind(cfg.provider()) {
                    BackendKind::Local => {
                        local_calls.fetch_add(1, Ordering::SeqCst);
                        Box::new(FixedBackend("local-response".to_string()))
                    }
                    BackendKind::Online => {
                        online_calls.fetch_add(1, Ordering::SeqCst);
                        Box::new(FixedBackend("online-response".to_string()))
                    }
                }
            })
        }

        let local_calls = Arc::new(AtomicUsize::new(0));
        let online_calls = Arc::new(AtomicUsize::new(0));

        let local_gen = TextGen::with_backend_factory(
            config_for(Provider::Local),
            make_factory(local_calls.clone(), online_calls.clone()),
            Duration::from_secs(2),
        );
        let local_result = local_gen.generate_text(sample_request(), CachePolicy::Off).wait();
        assert_eq!(local_result, JobStatus::Success("local-response".to_string()));
        assert_eq!(local_calls.load(Ordering::SeqCst), 1);
        assert_eq!(online_calls.load(Ordering::SeqCst), 0);

        let online_gen = TextGen::with_backend_factory(
            config_for(Provider::Claude),
            make_factory(local_calls.clone(), online_calls.clone()),
            Duration::from_secs(2),
        );
        let online_result = online_gen.generate_text(sample_request(), CachePolicy::Off).wait();
        assert_eq!(online_result, JobStatus::Success("online-response".to_string()));
        assert_eq!(online_calls.load(Ordering::SeqCst), 1);
    }

    /// `CachePolicy::Off` calls the backend on every `generate_text` call,
    /// even for identical requests.
    #[test]
    fn cache_off_calls_backend_every_time() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_factory = calls.clone();
        let text_gen = TextGen::with_backend_factory(
            config_for(Provider::Local),
            Box::new(move |_cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                Box::new(CountingBackend {
                    calls: calls_factory.clone(),
                    result: Arc::new(Mutex::new(Ok("text".to_string()))),
                })
            }),
            Duration::from_secs(2),
        );

        let request = sample_request();
        text_gen.generate_text(request.clone(), CachePolicy::Off).wait();
        text_gen.generate_text(request, CachePolicy::Off).wait();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// `CachePolicy::Cached`: a first seed-pinned request calls the backend
    /// and stores the result; an identical repeat returns the cached
    /// string without a second backend call.
    #[test]
    fn cache_hit_skips_backend_and_queue() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_factory = calls.clone();
        let text_gen = TextGen::with_backend_factory(
            config_for(Provider::Local),
            Box::new(move |_cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                Box::new(CountingBackend {
                    calls: calls_factory.clone(),
                    result: Arc::new(Mutex::new(Ok("cached-text".to_string()))),
                })
            }),
            Duration::from_secs(2),
        );

        let request = seed_pinned_request();
        let first = text_gen.generate_text(request.clone(), CachePolicy::Cached).wait();
        assert_eq!(first, JobStatus::Success("cached-text".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let second = text_gen.generate_text(request, CachePolicy::Cached).wait();
        assert_eq!(second, JobStatus::Success("cached-text".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "cache hit must not re-invoke the backend");
    }

    /// `CachePolicy::Cached`: a request differing only in seed misses the
    /// cache and re-invokes the backend.
    #[test]
    fn cache_miss_on_differing_key() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_factory = calls.clone();
        let text_gen = TextGen::with_backend_factory(
            config_for(Provider::Local),
            Box::new(move |_cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                Box::new(CountingBackend {
                    calls: calls_factory.clone(),
                    result: Arc::new(Mutex::new(Ok("text".to_string()))),
                })
            }),
            Duration::from_secs(2),
        );

        let a = TextRequest { seed: Some(1), ..sample_request() };
        let b = TextRequest { seed: Some(2), ..sample_request() };

        text_gen.generate_text(a, CachePolicy::Cached).wait();
        text_gen.generate_text(b, CachePolicy::Cached).wait();
        assert_eq!(calls.load(Ordering::SeqCst), 2, "differing seed must miss the cache and re-invoke");
    }

    /// `CachePolicy::Cached`: a failed generation is never cached, so an
    /// identical retry re-invokes the backend.
    #[test]
    fn cache_failure_not_stored() {
        let calls = Arc::new(AtomicUsize::new(0));
        let result = Arc::new(Mutex::new(Err(TextError::Transport("boom".to_string()))));
        let calls_factory = calls.clone();
        let result_factory = result.clone();
        let text_gen = TextGen::with_backend_factory(
            config_for(Provider::Local),
            Box::new(move |_cfg: &ResolvedModelConfig| -> Box<dyn TextBackend> {
                Box::new(CountingBackend {
                    calls: calls_factory.clone(),
                    result: result_factory.clone(),
                })
            }),
            Duration::from_secs(2),
        );

        let request = seed_pinned_request();
        let first = text_gen.generate_text(request.clone(), CachePolicy::Cached).wait();
        assert_eq!(first, JobStatus::Failed(TextError::Transport("boom".to_string())));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        *result.lock().unwrap() = Ok("recovered".to_string());
        let second = text_gen.generate_text(request, CachePolicy::Cached).wait();
        assert_eq!(second, JobStatus::Success("recovered".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 2, "a failed generation must not be cached");
    }
}
