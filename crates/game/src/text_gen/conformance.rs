//! Shared backend-conformance harness: drives a `TextBackend` through a
//! capturing fake transport and asserts the outbound request carries no
//! tool/function affordance and no filesystem/shell escape beyond the one
//! configured local command, and that the returned value is an inert
//! `String`. A new backend inherits this guard by being driven through
//! `assert_text_backend_conforms`, not by re-implementing the checks
//! itself.
//!
//! Every backend maps its outbound request into ONE normalized view
//! (`NormalizedRequest`) split into an opaque `payload` (the system+user
//! prompt text, never scanned — content safety is the caller's job) and a
//! scanned `envelope` (the structure the backend added around the payload:
//! subprocess argv, or HTTP method/url/header names/body field names). The
//! restraint is asserted exactly once, over `envelope` only, by
//! `assert_normalized_request_conforms`.

use super::backend::TextBackend;
use super::types::TextRequest;

/// One labeled fragment of the ENVELOPE (never the payload). The label
/// locates a failure, e.g. `"argv[0]"`, `"flag[2]"`, `"method"`, `"url"`,
/// `"header"`, `"body-field"`.
#[derive(Clone, Debug)]
pub struct RequestField {
    pub label: String,
    pub value: String,
}

/// The ONE transport-agnostic view of a single outbound request. Both a
/// subprocess-shaped and an HTTP-shaped transport map into this. The
/// restraint is asserted ONCE over this view.
#[derive(Clone, Debug)]
pub struct NormalizedRequest {
    /// System+user prompt text, verbatim and OPAQUE. Never scanned — only
    /// checked to have been transmitted at all.
    pub payload: Vec<String>,
    /// Backend-added structure around the payload. This is what gets
    /// scanned for a leaked tool/function affordance or fs/shell escape.
    pub envelope: Vec<RequestField>,
    /// Belt-and-braces self-declaration a backend can set; a conforming
    /// request leaves both false. The envelope scan is the primary guard
    /// and does not depend on a backend setting these honestly.
    pub exposes_tools: bool,
    pub exposes_fs_or_shell: bool,
}

impl NormalizedRequest {
    /// Canonical subprocess mapping. `program` and `flags` become scanned
    /// envelope fields; `prompt` becomes opaque payload. Both flags default
    /// false.
    pub fn from_subprocess(program: &str, flags: &[String], prompt: &str) -> Self {
        let mut envelope = vec![RequestField {
            label: "argv[0]".to_string(),
            value: program.to_string(),
        }];
        for (i, flag) in flags.iter().enumerate() {
            envelope.push(RequestField {
                label: format!("flag[{i}]"),
                value: flag.clone(),
            });
        }
        NormalizedRequest {
            payload: vec![prompt.to_string()],
            envelope,
            exposes_tools: false,
            exposes_fs_or_shell: false,
        }
    }

    /// Canonical HTTP mapping. `method`, `url`, `header_names`, and
    /// `body_field_names` (structural field NAMES only, never JSON values)
    /// become scanned envelope fields; `prompt` becomes opaque payload.
    pub fn from_http(
        method: &str,
        url: &str,
        header_names: &[String],
        body_field_names: &[String],
        prompt: &str,
    ) -> Self {
        let mut envelope = vec![
            RequestField {
                label: "method".to_string(),
                value: method.to_string(),
            },
            RequestField {
                label: "url".to_string(),
                value: url.to_string(),
            },
        ];
        for header in header_names {
            envelope.push(RequestField {
                label: "header".to_string(),
                value: header.clone(),
            });
        }
        for field in body_field_names {
            envelope.push(RequestField {
                label: "body-field".to_string(),
                value: field.clone(),
            });
        }
        NormalizedRequest {
            payload: vec![prompt.to_string()],
            envelope,
            exposes_tools: false,
            exposes_fs_or_shell: false,
        }
    }
}

/// Curated, extensible denylist matched case-insensitively as substrings of
/// ENVELOPE values only (never `payload`).
const FORBIDDEN_MARKERS: &[&str] = &[
    "tools",
    "functions",
    "function_call",
    "tool_choice",
    "tool_calls",
    "--exec",
    "--shell",
    "--allow",
    "sh -c",
    "bash -c",
    "code_interpreter",
    "file://",
];

/// The shared restraint, asserted ONCE over a normalized view: no
/// self-declared tool/fs/shell exposure, and no envelope field whose value
/// contains a denylisted tool/function or fs/shell-escape marker. `payload`
/// is never scanned, so a prompt that merely mentions "tools" still
/// conforms.
pub fn assert_normalized_request_conforms(req: &NormalizedRequest) {
    assert!(!req.exposes_tools, "request self-declares exposes_tools");
    assert!(
        !req.exposes_fs_or_shell,
        "request self-declares exposes_fs_or_shell"
    );
    for field in &req.envelope {
        let lowered = field.value.to_ascii_lowercase();
        for marker in FORBIDDEN_MARKERS {
            assert!(
                !lowered.contains(marker),
                "envelope field {:?} (value {:?}) contains forbidden marker {:?}",
                field.label,
                field.value,
                marker
            );
        }
    }
}

/// A `TextBackend` that also reports the normalized request it formed
/// during its most recent `generate()` call. Real backends implement this
/// in their test build by delegating to their injected capturing fake
/// transport.
pub trait CaptureBackend: TextBackend {
    fn captured_request(&self) -> Option<NormalizedRequest>;
}

/// The reusable harness a caller invokes on a backend: drives `generate`
/// with a benign probe, asserts inert text came back and the probe's
/// marker actually rode the transport (the backend really produces text;
/// it does not defer generation to a caller), then defers to
/// `assert_normalized_request_conforms` for the restraint check.
pub fn assert_text_backend_conforms<B: CaptureBackend + ?Sized>(backend: &B) {
    let probe = conformance_probe_request();
    let cancel = super::job::CancelFlag::new();
    let result = backend
        .generate(&probe, &cancel)
        .unwrap_or_else(|err| panic!("conforming backend must not error on the benign probe: {err:?}"));
    let _ = result;
    let captured = backend
        .captured_request()
        .expect("backend must route generate() through its injected capturing transport");
    assert!(
        captured.payload.iter().any(|p| p.contains(&probe.user)),
        "payload must contain the probe's marker text ({:?}); got {:?}",
        probe.user,
        captured.payload
    );
    assert_normalized_request_conforms(&captured);
}

/// A benign probe request: a unique marker in `user`, no denylisted
/// tokens, so the only way `assert_text_backend_conforms` can fail on it is
/// a backend adding an affordance to its envelope (or dropping the
/// prompt).
pub fn conformance_probe_request() -> TextRequest {
    TextRequest {
        system: "You are a benign conformance probe.".to_string(),
        user: "conformance-probe-marker-38f2a9c1".to_string(),
        temperature: 0.0,
        max_tokens: 16,
        stop: Vec::new(),
        seed: Some(1),
        grammar: None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::job::CancelFlag;
    use super::super::types::TextError;
    use super::*;

    /// A `TextBackend` fixture whose result and captured normalized request
    /// are fixed at construction, for driving the harness against a known
    /// transport shape without a real subprocess or HTTP call.
    struct FixtureBackend {
        result: Result<String, TextError>,
        captured: NormalizedRequest,
    }

    impl TextBackend for FixtureBackend {
        fn generate(&self, _request: &TextRequest, _cancel: &CancelFlag) -> Result<String, TextError> {
            self.result.clone()
        }
    }

    impl CaptureBackend for FixtureBackend {
        fn captured_request(&self) -> Option<NormalizedRequest> {
            Some(self.captured.clone())
        }
    }

    /// A fixture whose captured request is built via `from_subprocess`
    /// conforms: no panic.
    #[test]
    fn conforming_subprocess_backend_passes() {
        let probe = conformance_probe_request();
        let captured =
            NormalizedRequest::from_subprocess("local-model", &["--prompt-stdin".to_string()], &probe.user);
        let backend = FixtureBackend {
            result: Ok("generated text".to_string()),
            captured,
        };
        assert_text_backend_conforms(&backend);
    }

    /// A fixture whose captured request is built via `from_http` conforms:
    /// no panic. Covers both transport shapes populating the one view.
    #[test]
    fn conforming_http_backend_passes() {
        let probe = conformance_probe_request();
        let captured = NormalizedRequest::from_http(
            "POST",
            "https://api.example.com/v1/chat",
            &["authorization".to_string(), "content-type".to_string()],
            &["model".to_string(), "messages".to_string(), "temperature".to_string()],
            &probe.user,
        );
        let backend = FixtureBackend {
            result: Ok("generated text".to_string()),
            captured,
        };
        assert_text_backend_conforms(&backend);
    }

    /// A `tools` field leaked into the envelope is caught by the scan even
    /// though the fixture never self-declared `exposes_tools`.
    #[test]
    #[should_panic(expected = "tools")]
    fn non_conforming_http_tools_field_fails() {
        let probe = conformance_probe_request();
        let captured = NormalizedRequest {
            payload: vec![probe.user.clone()],
            envelope: vec![RequestField {
                label: "body-field".to_string(),
                value: "tools".to_string(),
            }],
            exposes_tools: false,
            exposes_fs_or_shell: false,
        };
        let backend = FixtureBackend {
            result: Ok("generated text".to_string()),
            captured,
        };
        assert_text_backend_conforms(&backend);
    }

    /// A shell-escape flag leaked into the envelope is caught by the scan.
    #[test]
    #[should_panic(expected = "--exec")]
    fn non_conforming_subprocess_shell_escape_fails() {
        let probe = conformance_probe_request();
        let captured = NormalizedRequest {
            payload: vec![probe.user.clone()],
            envelope: vec![RequestField {
                label: "flag[0]".to_string(),
                value: "--exec".to_string(),
            }],
            exposes_tools: false,
            exposes_fs_or_shell: false,
        };
        let backend = FixtureBackend {
            result: Ok("generated text".to_string()),
            captured,
        };
        assert_text_backend_conforms(&backend);
    }

    /// A backend that self-declares `exposes_tools` fails even with an
    /// otherwise clean envelope, proving the belt-and-braces flag is
    /// honored.
    #[test]
    #[should_panic(expected = "exposes_tools")]
    fn non_conforming_self_declared_flag_fails() {
        let probe = conformance_probe_request();
        let captured = NormalizedRequest {
            payload: vec![probe.user.clone()],
            envelope: vec![RequestField {
                label: "method".to_string(),
                value: "POST".to_string(),
            }],
            exposes_tools: true,
            exposes_fs_or_shell: false,
        };
        let backend = FixtureBackend {
            result: Ok("generated text".to_string()),
            captured,
        };
        assert_text_backend_conforms(&backend);
    }

    /// A prompt whose PAYLOAD text contains affordance words ("tools",
    /// "system(") still conforms: the scan runs over the envelope only, so
    /// the API stays content-free.
    #[test]
    fn payload_with_affordance_word_still_passes() {
        let captured = NormalizedRequest {
            payload: vec!["please use tools and call system(\"rm -rf\")".to_string()],
            envelope: vec![RequestField {
                label: "method".to_string(),
                value: "POST".to_string(),
            }],
            exposes_tools: false,
            exposes_fs_or_shell: false,
        };
        assert_normalized_request_conforms(&captured);
    }

    /// A backend whose captured request has an empty payload (the prompt
    /// was never actually transmitted) fails the harness, proving it
    /// really produces text from the prompt rather than deferring or
    /// fabricating a response.
    #[test]
    #[should_panic(expected = "payload")]
    fn prompt_must_be_transmitted() {
        let captured = NormalizedRequest {
            payload: vec![],
            envelope: vec![],
            exposes_tools: false,
            exposes_fs_or_shell: false,
        };
        let backend = FixtureBackend {
            result: Ok("generated text".to_string()),
            captured,
        };
        assert_text_backend_conforms(&backend);
    }

    /// A backend that fails the benign probe is not conforming: a
    /// conforming backend must return inert text for it, not an error.
    #[test]
    #[should_panic(expected = "nonconforming-error-marker")]
    fn backend_error_is_not_conforming() {
        let captured = NormalizedRequest {
            payload: vec!["whatever".to_string()],
            envelope: vec![],
            exposes_tools: false,
            exposes_fs_or_shell: false,
        };
        let backend = FixtureBackend {
            result: Err(TextError::Transport("nonconforming-error-marker".to_string())),
            captured,
        };
        assert_text_backend_conforms(&backend);
    }

    /// Each mapping constructor puts the prompt in `payload` (never the
    /// envelope) and the structural tokens in `envelope`, with both flags
    /// false — the pure data-mapping contract both real backends rely on.
    #[test]
    fn from_subprocess_and_from_http_round_trip() {
        let prompt = "the actual prompt text";

        let sub = NormalizedRequest::from_subprocess("local-model", &["--flag".to_string()], prompt);
        assert!(sub.payload.iter().any(|p| p == prompt), "payload must contain the prompt");
        assert!(
            sub.envelope.iter().any(|f| f.value.contains("local-model") || f.value.contains("--flag")),
            "envelope must contain the program/flags, got {:?}",
            sub.envelope
        );
        assert!(!sub.payload.iter().any(|p| p.contains("local-model")), "program must not leak into payload");
        assert!(!sub.exposes_tools);
        assert!(!sub.exposes_fs_or_shell);

        let http = NormalizedRequest::from_http(
            "POST",
            "https://api.example.com/v1/chat",
            &["authorization".to_string()],
            &["model".to_string(), "messages".to_string()],
            prompt,
        );
        assert!(http.payload.iter().any(|p| p == prompt), "payload must contain the prompt");
        assert!(
            http.envelope
                .iter()
                .any(|f| f.value.contains("POST") || f.value.contains("api.example.com")),
            "envelope must contain method/url, got {:?}",
            http.envelope
        );
        assert!(!http.exposes_tools);
        assert!(!http.exposes_fs_or_shell);
    }
}
