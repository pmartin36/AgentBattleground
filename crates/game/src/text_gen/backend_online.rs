//! The online HTTP `TextBackend`: adapts Claude, OpenAI, and
//! OpenAI-API-compatible providers behind the one `TextBackend` trait over
//! HTTPS using the player's own API key. Each provider category diverges
//! only in how the outbound request is shaped and how the completion is
//! parsed out of the response; both paths are behind one injectable
//! `HttpTransport` so no real network call happens under test.

use serde_json::{Map, Value};

use super::backend::TextBackend;
use super::conformance::{CaptureBackend, NormalizedRequest};
use super::job::CancelFlag;
use super::types::{Provider, ResolvedModelConfig, TextError, TextRequest};

/// A fully-formed outbound HTTP request. `method`/`url`/header NAMES/body
/// field NAMES are the scanned envelope; `prompt` (system+user) is the
/// opaque payload; `body` is the serialized JSON actually sent. Header
/// values (e.g. the auth key) are carried for sending but never mapped
/// into the scanned envelope — only header names are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub body_field_names: Vec<String>,
    pub prompt: String,
}

/// A completed HTTP response (any status). Transport errors are a
/// `TextError`, never folded into this type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Execution seam: sends one request, returns the response (2xx or not).
/// Injectable so tests and the conformance harness never hit the network.
pub trait HttpTransport: Send + Sync {
    fn send(&self, request: &HttpRequest, cancel: &CancelFlag) -> Result<HttpResponse, TextError>;

    /// The request captured on the most recent `send`, for the conformance
    /// harness. Production transports need not capture.
    fn captured(&self) -> Option<HttpRequest> {
        None
    }
}

/// Production transport: exactly one network request, no retry (the
/// billing contract). Not exercised under the gate.
pub struct UreqTransport;

impl HttpTransport for UreqTransport {
    fn send(&self, request: &HttpRequest, _cancel: &CancelFlag) -> Result<HttpResponse, TextError> {
        let mut builder = ureq::post(&request.url).config().http_status_as_error(false).build();
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let mut response = builder
            .send(request.body.clone())
            .map_err(|e| TextError::Transport(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| TextError::Transport(e.to_string()))?;
        Ok(HttpResponse { status, body })
    }
}

/// The online-provider `TextBackend`: shapes a `TextRequest` into a
/// provider-specific `HttpRequest`, hands it to an injected `HttpTransport`,
/// and parses the completion out of the response.
pub struct OnlineBackend {
    config: ResolvedModelConfig,
    transport: Box<dyn HttpTransport>,
}

impl OnlineBackend {
    /// Production constructor: sends over a real `UreqTransport`.
    pub fn new(config: ResolvedModelConfig) -> Self {
        OnlineBackend { config, transport: Box::new(UreqTransport) }
    }

    /// Test/harness constructor: drives an injected transport instead of a
    /// real network call.
    pub fn with_transport(config: ResolvedModelConfig, transport: Box<dyn HttpTransport>) -> Self {
        OnlineBackend { config, transport }
    }

    fn build_request(&self, request: &TextRequest) -> Result<HttpRequest, TextError> {
        let prompt = if request.system.is_empty() {
            request.user.clone()
        } else {
            format!("{}\n\n{}", request.system, request.user)
        };

        match self.config.provider() {
            Provider::Local => Err(TextError::Config(
                "online backend does not handle the Local provider".to_string(),
            )),
            Provider::Claude => {
                let api_key = self
                    .config
                    .api_key()
                    .ok_or_else(|| TextError::Config("api key required".to_string()))?;

                let mut body = Map::new();
                body.insert("model".to_string(), Value::String(self.config.model_identity().to_string()));
                body.insert("max_tokens".to_string(), Value::from(request.max_tokens));
                body.insert("temperature".to_string(), Value::from(request.temperature));
                if !request.system.is_empty() {
                    body.insert("system".to_string(), Value::String(request.system.clone()));
                }
                body.insert(
                    "messages".to_string(),
                    Value::Array(vec![Value::Object(Map::from_iter([
                        ("role".to_string(), Value::String("user".to_string())),
                        ("content".to_string(), Value::String(request.user.clone())),
                    ]))]),
                );
                if !request.stop.is_empty() {
                    body.insert(
                        "stop_sequences".to_string(),
                        Value::Array(request.stop.iter().cloned().map(Value::String).collect()),
                    );
                }

                let body_field_names: Vec<String> = body.keys().cloned().collect();
                let serialized = serde_json::to_string(&Value::Object(body))
                    .map_err(|e| TextError::Parse(e.to_string()))?;

                Ok(HttpRequest {
                    method: "POST".to_string(),
                    url: "https://api.anthropic.com/v1/messages".to_string(),
                    headers: vec![
                        ("x-api-key".to_string(), api_key.to_string()),
                        ("anthropic-version".to_string(), "2023-06-01".to_string()),
                        ("content-type".to_string(), "application/json".to_string()),
                    ],
                    body: serialized,
                    body_field_names,
                    prompt,
                })
            }
            Provider::OpenAi => {
                let api_key = self
                    .config
                    .api_key()
                    .ok_or_else(|| TextError::Config("api key required".to_string()))?;
                self.build_openai_shaped_request(
                    request,
                    "https://api.openai.com/v1/chat/completions".to_string(),
                    api_key,
                    prompt,
                )
            }
            Provider::OpenAiCompatible => {
                let api_key = self
                    .config
                    .api_key()
                    .ok_or_else(|| TextError::Config("api key required".to_string()))?;
                let base = self
                    .config
                    .base_url()
                    .ok_or_else(|| TextError::Config("OpenAiCompatible requires a base_url".to_string()))?;
                let url = format!("{}/chat/completions", base.trim_end_matches('/'));
                self.build_openai_shaped_request(request, url, api_key, prompt)
            }
        }
    }

    /// The OpenAI-shaped request body (also used by `OpenAiCompatible`,
    /// which diverges only in `url`): Bearer auth, `messages` array of
    /// optional system + user turns, `choices`-shaped response.
    fn build_openai_shaped_request(
        &self,
        request: &TextRequest,
        url: String,
        api_key: &str,
        prompt: String,
    ) -> Result<HttpRequest, TextError> {
        let mut messages = Vec::new();
        if !request.system.is_empty() {
            messages.push(Value::Object(Map::from_iter([
                ("role".to_string(), Value::String("system".to_string())),
                ("content".to_string(), Value::String(request.system.clone())),
            ])));
        }
        messages.push(Value::Object(Map::from_iter([
            ("role".to_string(), Value::String("user".to_string())),
            ("content".to_string(), Value::String(request.user.clone())),
        ])));

        let mut body = Map::new();
        body.insert("model".to_string(), Value::String(self.config.model_identity().to_string()));
        body.insert("temperature".to_string(), Value::from(request.temperature));
        body.insert("max_tokens".to_string(), Value::from(request.max_tokens));
        body.insert("messages".to_string(), Value::Array(messages));
        if !request.stop.is_empty() {
            body.insert(
                "stop".to_string(),
                Value::Array(request.stop.iter().cloned().map(Value::String).collect()),
            );
        }
        if let Some(seed) = request.seed {
            body.insert("seed".to_string(), Value::from(seed));
        }

        let body_field_names: Vec<String> = body.keys().cloned().collect();
        let serialized =
            serde_json::to_string(&Value::Object(body)).map_err(|e| TextError::Parse(e.to_string()))?;

        Ok(HttpRequest {
            method: "POST".to_string(),
            url,
            headers: vec![
                ("authorization".to_string(), format!("Bearer {api_key}")),
                ("content-type".to_string(), "application/json".to_string()),
            ],
            body: serialized,
            body_field_names,
            prompt,
        })
    }

    fn parse_response(&self, resp: &HttpResponse) -> Result<String, TextError> {
        if !(200..300).contains(&resp.status) {
            return Err(TextError::Http { status: resp.status, body: resp.body.clone() });
        }

        let json: Value = serde_json::from_str(&resp.body).map_err(|e| TextError::Parse(e.to_string()))?;

        match self.config.provider() {
            Provider::Claude => json
                .get("content")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("text"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| TextError::Parse("missing content[0].text".to_string())),
            Provider::OpenAi | Provider::OpenAiCompatible => json
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| TextError::Parse("missing choices[0].message.content".to_string())),
            Provider::Local => Err(TextError::Config(
                "online backend does not handle the Local provider".to_string(),
            )),
        }
    }
}

impl TextBackend for OnlineBackend {
    fn generate(&self, request: &TextRequest, cancel: &CancelFlag) -> Result<String, TextError> {
        let req = self.build_request(request)?;
        let resp = self.transport.send(&req, cancel)?;
        self.parse_response(&resp)
    }
}

impl CaptureBackend for OnlineBackend {
    fn captured_request(&self) -> Option<NormalizedRequest> {
        self.transport.captured().map(|r| {
            let header_names: Vec<String> = r.headers.iter().map(|(name, _)| name.clone()).collect();
            NormalizedRequest::from_http(&r.method, &r.url, &header_names, &r.body_field_names, &r.prompt)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::super::conformance::assert_text_backend_conforms;
    use super::super::types::Provider;
    use super::*;

    fn claude_config() -> ResolvedModelConfig {
        ResolvedModelConfig::new(Provider::Claude, "claude-test-model", Some("sk-ant-test".to_string()), None, None)
    }

    fn openai_config() -> ResolvedModelConfig {
        ResolvedModelConfig::new(Provider::OpenAi, "openai-test-model", Some("sk-oa-test".to_string()), None, None)
    }

    fn openai_compatible_config(base_url: Option<&str>) -> ResolvedModelConfig {
        ResolvedModelConfig::new(
            Provider::OpenAiCompatible,
            "compat-test-model",
            Some("sk-compat-test".to_string()),
            None,
            base_url.map(|s| s.to_string()),
        )
    }

    fn no_key_config(provider: Provider) -> ResolvedModelConfig {
        ResolvedModelConfig::new(provider, "no-key-model", None, None, None)
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

    fn claude_success_body() -> String {
        r#"{"content":[{"type":"text","text":"the completion"}]}"#.to_string()
    }

    fn openai_success_body() -> String {
        r#"{"choices":[{"message":{"content":"the completion"}}]}"#.to_string()
    }

    /// An `HttpTransport` fixture that records the request it sent and
    /// returns a fixed result. `calls` is shared via `Arc` so a test can
    /// observe the count after the fixture has been boxed into a backend.
    struct CapturingTransport {
        result: Result<HttpResponse, TextError>,
        recorded: Mutex<Option<HttpRequest>>,
        calls: Arc<AtomicUsize>,
    }

    impl CapturingTransport {
        fn ok(status: u16, body: &str) -> Self {
            CapturingTransport {
                result: Ok(HttpResponse { status, body: body.to_string() }),
                recorded: Mutex::new(None),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn err(error: TextError) -> Self {
            CapturingTransport {
                result: Err(error),
                recorded: Mutex::new(None),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn call_count_handle(&self) -> Arc<AtomicUsize> {
            self.calls.clone()
        }
    }

    impl HttpTransport for CapturingTransport {
        fn send(&self, request: &HttpRequest, _cancel: &CancelFlag) -> Result<HttpResponse, TextError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.recorded.lock().unwrap() = Some(request.clone());
            self.result.clone()
        }

        fn captured(&self) -> Option<HttpRequest> {
            self.recorded.lock().unwrap().clone()
        }
    }

    /// An `HttpTransport` fixture that flips a flag if `send` is ever
    /// called, so a test can assert the transport was never invoked.
    struct NeverCalledTransport(Arc<AtomicBool>);

    impl HttpTransport for NeverCalledTransport {
        fn send(&self, _request: &HttpRequest, _cancel: &CancelFlag) -> Result<HttpResponse, TextError> {
            self.0.store(true, Ordering::SeqCst);
            Ok(HttpResponse { status: 200, body: "should not be reached".to_string() })
        }
    }

    /// A backend built over a capturing transport passes the shared
    /// conformance harness, for each online provider category.
    #[test]
    fn online_backend_conforms_claude() {
        let backend =
            OnlineBackend::with_transport(claude_config(), Box::new(CapturingTransport::ok(200, &claude_success_body())));
        assert_text_backend_conforms(&backend);
    }

    #[test]
    fn online_backend_conforms_openai() {
        let backend =
            OnlineBackend::with_transport(openai_config(), Box::new(CapturingTransport::ok(200, &openai_success_body())));
        assert_text_backend_conforms(&backend);
    }

    #[test]
    fn online_backend_conforms_openai_compatible() {
        let backend = OnlineBackend::with_transport(
            openai_compatible_config(Some("https://my-proxy.example/v1")),
            Box::new(CapturingTransport::ok(200, &openai_success_body())),
        );
        assert_text_backend_conforms(&backend);
    }

    /// A Claude config forms an Anthropic messages request (x-api-key auth,
    /// `content` response shape) and returns the parsed completion.
    #[test]
    fn claude_request_shape() {
        let backend =
            OnlineBackend::with_transport(claude_config(), Box::new(CapturingTransport::ok(200, &claude_success_body())));
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert_eq!(result, Ok("the completion".to_string()));

        let captured = backend
            .captured_request()
            .expect("backend must map the transport's captured request");
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();
        assert!(envelope.iter().any(|v| v == "https://api.anthropic.com/v1/messages"), "got {envelope:?}");
        assert!(envelope.iter().any(|v| v == "x-api-key"), "got {envelope:?}");
        assert!(envelope.iter().any(|v| v == "model"), "got {envelope:?}");
        assert!(envelope.iter().any(|v| v == "max_tokens"), "got {envelope:?}");
        assert!(envelope.iter().any(|v| v == "messages"), "got {envelope:?}");
        assert!(!envelope.iter().any(|v| v.to_ascii_lowercase().contains("tools")), "got {envelope:?}");
        assert!(!envelope.iter().any(|v| v.to_ascii_lowercase().contains("functions")), "got {envelope:?}");
    }

    /// An OpenAI config forms a chat/completions request (Bearer auth,
    /// `choices` response shape) and returns the parsed completion.
    #[test]
    fn openai_request_shape() {
        let backend =
            OnlineBackend::with_transport(openai_config(), Box::new(CapturingTransport::ok(200, &openai_success_body())));
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert_eq!(result, Ok("the completion".to_string()));

        let captured = backend
            .captured_request()
            .expect("backend must map the transport's captured request");
        let envelope: Vec<String> = captured.envelope.iter().map(|f| f.value.clone()).collect();
        assert!(envelope.iter().any(|v| v == "https://api.openai.com/v1/chat/completions"), "got {envelope:?}");
        assert!(envelope.iter().any(|v| v == "authorization"), "got {envelope:?}");
        assert!(envelope.iter().any(|v| v == "model"), "got {envelope:?}");
        assert!(envelope.iter().any(|v| v == "messages"), "got {envelope:?}");
        assert!(!envelope.iter().any(|v| v.to_ascii_lowercase().contains("tools")), "got {envelope:?}");
        assert!(!envelope.iter().any(|v| v.to_ascii_lowercase().contains("functions")), "got {envelope:?}");
    }

    /// An OpenAI-API-compatible config sends an OpenAI-shaped body to the
    /// config's caller-supplied base URL, not to `api.openai.com` — the
    /// two categories are distinct, not one subsumed by the other.
    #[test]
    fn openai_compatible_is_distinct() {
        let backend = OnlineBackend::with_transport(
            openai_compatible_config(Some("https://my-proxy.example/v1")),
            Box::new(CapturingTransport::ok(200, &openai_success_body())),
        );
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert_eq!(result, Ok("the completion".to_string()));

        let captured = backend
            .captured_request()
            .expect("backend must map the transport's captured request");
        let urls: Vec<&String> = captured.envelope.iter().filter(|f| f.label == "url").map(|f| &f.value).collect();
        assert!(urls.iter().any(|u| u.contains("my-proxy.example")), "got {urls:?}");
        assert!(!urls.iter().any(|u| u.contains("api.openai.com")), "got {urls:?}");
    }

    /// A non-2xx response maps to a structured `Http` error, and the
    /// transport is called exactly once — no silently multiplied billed
    /// call on failure.
    #[test]
    fn http_error_maps_to_error_no_retry() {
        let transport = CapturingTransport::ok(500, "server error");
        let calls = transport.call_count_handle();
        let backend = OnlineBackend::with_transport(claude_config(), Box::new(transport));
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert!(
            matches!(result, Err(TextError::Http { status: 500, .. })),
            "expected Http{{status:500}}, got {result:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "transport must be called exactly once");
    }

    /// A network/connect failure the transport returns propagates
    /// unchanged as a structured error, never a hang.
    #[test]
    fn transport_failure_maps_to_error() {
        let backend = OnlineBackend::with_transport(
            claude_config(),
            Box::new(CapturingTransport::err(TextError::Transport("connreset".to_string()))),
        );
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert_eq!(result, Err(TextError::Transport("connreset".to_string())));
    }

    /// An unparseable 2xx body maps to a structured `Parse` error.
    #[test]
    fn unparseable_body_is_parse_error() {
        let backend = OnlineBackend::with_transport(claude_config(), Box::new(CapturingTransport::ok(200, "not json")));
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert!(matches!(result, Err(TextError::Parse(_))), "expected Parse error, got {result:?}");
    }

    /// An `OpenAiCompatible` config with no `base_url` fails with a
    /// structured `Config` error, and the transport is never invoked.
    #[test]
    fn missing_base_url_is_config_error() {
        let called = Arc::new(AtomicBool::new(false));
        let backend = OnlineBackend::with_transport(
            openai_compatible_config(None),
            Box::new(NeverCalledTransport(called.clone())),
        );
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert!(matches!(result, Err(TextError::Config(_))), "expected Config error, got {result:?}");
        assert!(!called.load(Ordering::SeqCst), "transport must not run when base_url is None");
    }

    /// A Claude config with no API key fails with a structured `Config`
    /// error, and the transport is never invoked.
    #[test]
    fn missing_api_key_is_config_error_claude() {
        let called = Arc::new(AtomicBool::new(false));
        let backend = OnlineBackend::with_transport(
            no_key_config(Provider::Claude),
            Box::new(NeverCalledTransport(called.clone())),
        );
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert!(matches!(result, Err(TextError::Config(_))), "expected Config error, got {result:?}");
        assert!(!called.load(Ordering::SeqCst), "transport must not run when api_key is None");
    }

    /// An OpenAI config with no API key fails with a structured `Config`
    /// error, and the transport is never invoked.
    #[test]
    fn missing_api_key_is_config_error_openai() {
        let called = Arc::new(AtomicBool::new(false));
        let backend = OnlineBackend::with_transport(
            no_key_config(Provider::OpenAi),
            Box::new(NeverCalledTransport(called.clone())),
        );
        let cancel = CancelFlag::new();

        let result = backend.generate(&sample_request(), &cancel);
        assert!(matches!(result, Err(TextError::Config(_))), "expected Config error, got {result:?}");
        assert!(!called.load(Ordering::SeqCst), "transport must not run when api_key is None");
    }

    /// The stdin-equivalent payload carries both the system and user text;
    /// neither leaks into the scanned envelope (method/url/header/body-field
    /// names).
    #[test]
    fn payload_and_field_names_are_structural() {
        let backend =
            OnlineBackend::with_transport(claude_config(), Box::new(CapturingTransport::ok(200, &claude_success_body())));
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

        let captured = backend.captured_request().expect("backend must capture the request");
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
        assert!(envelope.iter().any(|v| v == "model"), "envelope must carry structural body field names, got {envelope:?}");
    }
}
