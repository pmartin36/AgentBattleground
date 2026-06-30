//! IPC envelope, message types, and framing for the scene-switcher wire protocol.
//!
//! Wire format: 4-byte LE length prefix + UTF-8 JSON body.
//! All encode/decode paths live here; b4 server and b5 client use `write_frame`/`read_frame`.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

use crate::scene_id::SceneId;

/// Maximum JSON body length (bytes) the 4-byte length prefix may declare.
/// Frames claiming more than this are rejected as `BadFrame`.
pub const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

// ── Payload structs ────────────────────────────────────────────────────────────

/// One entry in the scene catalog sent in a `Hello` message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: SceneId,
    pub name: String,
}

/// Server → inspector on connect (and on reconnect). Lists every available
/// scene with its display name and identifies the currently-active scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub scenes: Vec<CatalogEntry>,
    pub active: SceneId,
}

/// Server → inspector (unsolicited) after any scene switch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneChanged {
    pub id: SceneId,
}

/// Inspector → server: request a scene switch.
/// `target` is a raw string so that an unknown scene name still produces a
/// valid `SwitchScene` value; the server resolves it via `SceneId::from_wire`
/// and replies `Error{UnknownScene}` when it is not in the catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwitchScene {
    pub target: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// Payload for wire `Error` messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub code: ErrorCode,
    pub message: String,
}

/// Discriminants used in `ErrorPayload.code` and mapped from `IpcError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    BadFrame,
    UnknownType,
    UnknownScene,
}

// ── Message (the M1 message set) ───────────────────────────────────────────────

/// All message bodies understood by this protocol version.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Hello(Hello),
    SceneChanged(SceneChanged),
    SwitchScene(SwitchScene),
    /// Acknowledgement of a `SwitchScene`; no payload.
    Ack,
    Error(ErrorPayload),
}

impl Message {
    /// The wire `"type"` discriminant string for this variant.
    pub fn type_str(&self) -> &'static str {
        match self {
            Message::Hello(_) => "Hello",
            Message::SceneChanged(_) => "SceneChanged",
            Message::SwitchScene(_) => "SwitchScene",
            Message::Ack => "Ack",
            Message::Error(_) => "Error",
        }
    }
}

// ── Private wire bridge ────────────────────────────────────────────────────────

/// Private serde bridge: keeps the type/payload split explicit and supports
/// the UnknownType path without brittle flatten+adjacent-tag derive.
#[derive(Serialize, Deserialize)]
struct Wire {
    #[serde(rename = "type")]
    typ: String,
    seq: u64,
    reply_to: Option<u64>,
    #[serde(default)]
    payload: serde_json::Value,
}

// ── Envelope ───────────────────────────────────────────────────────────────────

/// Wire envelope: caller controls `seq` and `reply_to`; framing never rewrites them.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub seq: u64,
    pub reply_to: Option<u64>,
    pub body: Message,
}

impl Envelope {
    /// Construct an envelope. Callers set `seq`/`reply_to`; encode never modifies them.
    pub fn new(seq: u64, reply_to: Option<u64>, body: Message) -> Self {
        Envelope { seq, reply_to, body }
    }

    /// Serialize to a complete frame: 4-byte LE length prefix + JSON body.
    /// Returns `Err(IpcError::BadFrame)` if the serialized body exceeds `MAX_FRAME_LEN`.
    pub fn encode_frame(&self) -> Result<Vec<u8>, IpcError> {
        let payload = match &self.body {
            Message::Hello(h) => {
                serde_json::to_value(h).map_err(|e| IpcError::BadFrame(e.to_string()))?
            }
            Message::SceneChanged(sc) => {
                serde_json::to_value(sc).map_err(|e| IpcError::BadFrame(e.to_string()))?
            }
            Message::SwitchScene(ss) => {
                serde_json::to_value(ss).map_err(|e| IpcError::BadFrame(e.to_string()))?
            }
            Message::Ack => serde_json::Value::Null,
            Message::Error(ep) => {
                serde_json::to_value(ep).map_err(|e| IpcError::BadFrame(e.to_string()))?
            }
        };

        let wire = Wire {
            typ: self.body.type_str().to_string(),
            seq: self.seq,
            reply_to: self.reply_to,
            payload,
        };

        let body_bytes =
            serde_json::to_vec(&wire).map_err(|e| IpcError::BadFrame(e.to_string()))?;

        if body_bytes.len() > MAX_FRAME_LEN {
            return Err(IpcError::BadFrame(format!(
                "serialized frame {} bytes exceeds MAX_FRAME_LEN {}",
                body_bytes.len(),
                MAX_FRAME_LEN
            )));
        }

        let len_prefix = (body_bytes.len() as u32).to_le_bytes();
        let mut frame = Vec::with_capacity(4 + body_bytes.len());
        frame.extend_from_slice(&len_prefix);
        frame.extend_from_slice(&body_bytes);
        Ok(frame)
    }

    /// Decode one complete frame (length prefix + body) from a byte slice.
    pub fn decode_frame(bytes: &[u8]) -> Result<Envelope, IpcError> {
        if bytes.len() < 4 {
            return Err(IpcError::BadFrame(
                "frame too short: missing length prefix".to_string(),
            ));
        }

        let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;

        if len > MAX_FRAME_LEN {
            return Err(IpcError::BadFrame(format!(
                "length prefix {} exceeds MAX_FRAME_LEN {}",
                len, MAX_FRAME_LEN
            )));
        }

        if bytes.len() < 4 + len {
            return Err(IpcError::BadFrame(format!(
                "frame truncated: expected {} body bytes, got {}",
                len,
                bytes.len().saturating_sub(4)
            )));
        }

        decode_body(&bytes[4..4 + len])
    }
}

// ── Stream framing helpers ─────────────────────────────────────────────────────

/// Read exactly one framed message from `r`.
/// Reads the 4-byte LE length prefix, validates it against `MAX_FRAME_LEN`,
/// reads that many body bytes, then delegates to the body-decode path.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Envelope, IpcError> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;

    let len = u32::from_le_bytes(len_buf) as usize;

    if len > MAX_FRAME_LEN {
        return Err(IpcError::BadFrame(format!(
            "length prefix {} exceeds MAX_FRAME_LEN {}",
            len, MAX_FRAME_LEN
        )));
    }

    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            IpcError::BadFrame(format!("frame body truncated: {}", e))
        } else {
            IpcError::Io(e)
        }
    })?;

    decode_body(&body)
}

/// Serialize `env` via `encode_frame` and write all bytes to `w`.
pub fn write_frame<W: Write>(w: &mut W, env: &Envelope) -> Result<(), IpcError> {
    let frame = env.encode_frame()?;
    w.write_all(&frame)?;
    Ok(())
}

// ── Private body decoder ───────────────────────────────────────────────────────

/// Deserialize a payload from a `serde_json::Value`.
///
/// `serde_json::from_value` cannot satisfy deserializers that borrow `&'de str`
/// from input (e.g. `SceneId` uses `<&str as Deserialize>`). Going through
/// JSON bytes (`to_vec` → `from_slice`) keeps the normal string-borrow path
/// alive and avoids a separate `Owned`/`Borrowed` split in `SceneId`.
fn from_payload<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T, IpcError> {
    let bytes = serde_json::to_vec(&v).map_err(|e| IpcError::BadFrame(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| IpcError::BadFrame(e.to_string()))
}

/// Parse the JSON body (no length prefix) into an `Envelope`.
/// Used by both `decode_frame` and `read_frame`.
fn decode_body(body: &[u8]) -> Result<Envelope, IpcError> {
    let wire: Wire =
        serde_json::from_slice(body).map_err(|e| IpcError::BadFrame(e.to_string()))?;

    let message = match wire.typ.as_str() {
        "Hello" => {
            let h: Hello = from_payload(wire.payload)?;
            Message::Hello(h)
        }
        "SceneChanged" => {
            let sc: SceneChanged = from_payload(wire.payload)?;
            Message::SceneChanged(sc)
        }
        "SwitchScene" => {
            let ss: SwitchScene = from_payload(wire.payload)?;
            Message::SwitchScene(ss)
        }
        "Ack" => Message::Ack,
        "Error" => {
            let ep: ErrorPayload = from_payload(wire.payload)?;
            Message::Error(ep)
        }
        unknown => return Err(IpcError::UnknownType(unknown.to_string())),
    };

    Ok(Envelope {
        seq: wire.seq,
        reply_to: wire.reply_to,
        body: message,
    })
}

// ── Errors ─────────────────────────────────────────────────────────────────────

/// Errors produced by the framing / decoding layer.
#[derive(Debug)]
pub enum IpcError {
    /// Oversize length prefix, short/partial body, structurally-invalid JSON,
    /// or a well-formed JSON object whose `seq` field is missing.
    BadFrame(String),
    /// Well-framed JSON whose `"type"` is not a known M1 message.
    UnknownType(String),
    /// Underlying I/O failure (e.g. connection dropped mid-read).
    Io(std::io::Error),
}

impl IpcError {
    /// Map a decode failure to the `ErrorCode` the server should reply with.
    /// `BadFrame` => `Some(BadFrame)`, `UnknownType` => `Some(UnknownType)`,
    /// `Io` => `None` (transport failure, not a protocol error).
    pub fn error_code(&self) -> Option<ErrorCode> {
        match self {
            IpcError::BadFrame(_) => Some(ErrorCode::BadFrame),
            IpcError::UnknownType(_) => Some(ErrorCode::UnknownType),
            IpcError::Io(_) => None,
        }
    }
}

impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        IpcError::Io(e)
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcError::BadFrame(msg) => write!(f, "bad frame: {}", msg),
            IpcError::UnknownType(typ) => write!(f, "unknown message type: {:?}", typ),
            IpcError::Io(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl std::error::Error for IpcError {}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ── helpers ────────────────────────────────────────────────────────────────

    fn hello_msg() -> Message {
        Message::Hello(Hello {
            scenes: vec![
                CatalogEntry {
                    id: SceneId::MainHub,
                    name: "Main Hub".to_string(),
                },
                CatalogEntry {
                    id: SceneId::BattleViewer,
                    name: "Battle Viewer".to_string(),
                },
            ],
            active: SceneId::MainHub,
        })
    }

    fn switch_scene_msg() -> Message {
        Message::SwitchScene(SwitchScene {
            target: "BattleViewer".to_string(),
            params: None,
        })
    }

    fn error_msg() -> Message {
        Message::Error(ErrorPayload {
            code: ErrorCode::UnknownScene,
            message: "scene not found".to_string(),
        })
    }

    fn scene_changed_msg() -> Message {
        Message::SceneChanged(SceneChanged {
            id: SceneId::BattleViewer,
        })
    }

    // ── encode_frame / decode_frame round-trips (one per message variant) ──────

    #[test]
    fn round_trip_hello_encode_decode() {
        let env = Envelope::new(1, None, hello_msg());
        let bytes = env.encode_frame().expect("encode Hello");
        let decoded = Envelope::decode_frame(&bytes).expect("decode Hello");
        assert_eq!(env, decoded);
    }

    #[test]
    fn round_trip_scene_changed_encode_decode() {
        let env = Envelope::new(2, Some(1), scene_changed_msg());
        let bytes = env.encode_frame().expect("encode SceneChanged");
        let decoded = Envelope::decode_frame(&bytes).expect("decode SceneChanged");
        assert_eq!(env, decoded);
    }

    #[test]
    fn round_trip_switch_scene_encode_decode() {
        let env = Envelope::new(3, None, switch_scene_msg());
        let bytes = env.encode_frame().expect("encode SwitchScene");
        let decoded = Envelope::decode_frame(&bytes).expect("decode SwitchScene");
        assert_eq!(env, decoded);
    }

    #[test]
    fn round_trip_ack_encode_decode() {
        let env = Envelope::new(4, Some(3), Message::Ack);
        let bytes = env.encode_frame().expect("encode Ack");
        let decoded = Envelope::decode_frame(&bytes).expect("decode Ack");
        assert_eq!(env, decoded);
    }

    #[test]
    fn round_trip_error_encode_decode() {
        let env = Envelope::new(5, Some(4), error_msg());
        let bytes = env.encode_frame().expect("encode Error");
        let decoded = Envelope::decode_frame(&bytes).expect("decode Error");
        assert_eq!(env, decoded);
    }

    // ── write_frame / read_frame round-trips (same 5 variants, stream path) ────

    #[test]
    fn round_trip_hello_write_read_frame() {
        let env = Envelope::new(10, None, hello_msg());
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).expect("write_frame Hello");
        let decoded = read_frame(&mut Cursor::new(buf)).expect("read_frame Hello");
        assert_eq!(env, decoded);
    }

    #[test]
    fn round_trip_scene_changed_write_read_frame() {
        let env = Envelope::new(11, Some(10), scene_changed_msg());
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).expect("write_frame SceneChanged");
        let decoded = read_frame(&mut Cursor::new(buf)).expect("read_frame SceneChanged");
        assert_eq!(env, decoded);
    }

    #[test]
    fn round_trip_switch_scene_write_read_frame() {
        let env = Envelope::new(12, None, switch_scene_msg());
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).expect("write_frame SwitchScene");
        let decoded = read_frame(&mut Cursor::new(buf)).expect("read_frame SwitchScene");
        assert_eq!(env, decoded);
    }

    #[test]
    fn round_trip_ack_write_read_frame() {
        let env = Envelope::new(13, Some(12), Message::Ack);
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).expect("write_frame Ack");
        let decoded = read_frame(&mut Cursor::new(buf)).expect("read_frame Ack");
        assert_eq!(env, decoded);
    }

    #[test]
    fn round_trip_error_write_read_frame() {
        let env = Envelope::new(14, Some(13), error_msg());
        let mut buf = Vec::new();
        write_frame(&mut buf, &env).expect("write_frame Error");
        let decoded = read_frame(&mut Cursor::new(buf)).expect("read_frame Error");
        assert_eq!(env, decoded);
    }

    // ── reply_to threading ─────────────────────────────────────────────────────

    #[test]
    fn reply_to_some_survives_round_trip() {
        let env = Envelope::new(99, Some(42), Message::Ack);
        let bytes = env.encode_frame().expect("encode");
        let decoded = Envelope::decode_frame(&bytes).expect("decode");
        assert_eq!(decoded.seq, 99);
        assert_eq!(decoded.reply_to, Some(42));
    }

    #[test]
    fn reply_to_none_survives_round_trip() {
        let env = Envelope::new(7, None, Message::Ack);
        let bytes = env.encode_frame().expect("encode");
        let decoded = Envelope::decode_frame(&bytes).expect("decode");
        assert_eq!(decoded.reply_to, None);
    }

    // ── SwitchScene with unknown target decodes OK ──────────────────────────────

    #[test]
    fn switch_scene_unknown_target_decodes_as_string_not_error() {
        let env = Envelope::new(20, None, Message::SwitchScene(SwitchScene {
            target: "NotARealScene".to_string(),
            params: None,
        }));
        let bytes = env.encode_frame().expect("encode");
        let decoded = Envelope::decode_frame(&bytes).expect("decode must succeed — target is raw string");
        match decoded.body {
            Message::SwitchScene(ss) => assert_eq!(ss.target, "NotARealScene"),
            other => panic!("expected SwitchScene, got {:?}", other),
        }
    }

    // ── SceneId fields round-trip as wire-name strings ──────────────────────────

    #[test]
    fn scene_id_fields_serialize_as_wire_name_strings() {
        // Hello.active and SceneChanged.id must survive as "BattleViewer", not a numeric id.
        let env = Envelope::new(30, None, Message::SceneChanged(SceneChanged {
            id: SceneId::BattleViewer,
        }));
        let bytes = env.encode_frame().expect("encode");
        // The raw JSON somewhere inside bytes must contain the wire name.
        let json_body = &bytes[4..]; // skip 4-byte prefix
        let text = std::str::from_utf8(json_body).expect("valid UTF-8");
        assert!(text.contains("BattleViewer"), "wire JSON must embed wire name string: {}", text);
    }

    // ── Exact wire shape ───────────────────────────────────────────────────────

    #[test]
    fn switch_scene_wire_shape_matches_spec() {
        // Spec 14 example:
        // {"type":"SwitchScene","seq":12,"reply_to":null,"payload":{"target":"BattleViewer","params":null}}
        let env = Envelope::new(
            12,
            None,
            Message::SwitchScene(SwitchScene {
                target: "BattleViewer".to_string(),
                params: None,
            }),
        );
        let bytes = env.encode_frame().expect("encode");
        let json_body = std::str::from_utf8(&bytes[4..]).expect("valid UTF-8");
        let v: serde_json::Value = serde_json::from_str(json_body).expect("valid JSON");
        assert_eq!(v["type"], "SwitchScene");
        assert_eq!(v["seq"], 12u64);
        assert!(v["reply_to"].is_null(), "reply_to must be JSON null");
        assert_eq!(v["payload"]["target"], "BattleViewer");
    }

    #[test]
    fn ack_wire_shape_has_type_and_seq() {
        let env = Envelope::new(5, Some(3), Message::Ack);
        let bytes = env.encode_frame().expect("encode");
        let json_body = std::str::from_utf8(&bytes[4..]).expect("valid UTF-8");
        let v: serde_json::Value = serde_json::from_str(json_body).expect("valid JSON");
        assert_eq!(v["type"], "Ack");
        assert_eq!(v["seq"], 5u64);
        assert_eq!(v["reply_to"], 3u64);
    }

    // ── Framing error paths ────────────────────────────────────────────────────

    #[test]
    fn oversize_length_prefix_yields_bad_frame() {
        // Write a 4-byte prefix claiming MAX_FRAME_LEN + 1 bytes; do NOT write a body.
        let too_big = (MAX_FRAME_LEN as u32) + 1;
        let header = too_big.to_le_bytes();
        let result = read_frame(&mut Cursor::new(header.as_slice()));
        assert!(
            matches!(result, Err(IpcError::BadFrame(_))),
            "expected BadFrame for oversize prefix, got {:?}", result
        );
    }

    #[test]
    fn truncated_body_yields_bad_frame() {
        // Prefix says 50 bytes; actual body is only 10.
        let claimed_len: u32 = 50;
        let mut buf = claimed_len.to_le_bytes().to_vec();
        buf.extend_from_slice(b"short_bod_"); // 10 bytes
        let result = read_frame(&mut Cursor::new(buf));
        assert!(
            matches!(result, Err(IpcError::BadFrame(_)) | Err(IpcError::Io(_))),
            "expected BadFrame or Io for truncated body, got {:?}", result
        );
    }

    #[test]
    fn missing_length_prefix_yields_bad_frame_or_io() {
        // Empty buffer — not even 4 bytes for the length prefix.
        let result = read_frame(&mut Cursor::new(&[] as &[u8]));
        assert!(
            matches!(result, Err(IpcError::BadFrame(_)) | Err(IpcError::Io(_))),
            "expected BadFrame or Io for empty input, got {:?}", result
        );
    }

    #[test]
    fn malformed_json_body_yields_bad_frame() {
        let body = b"not valid json!!!";
        let mut buf = (body.len() as u32).to_le_bytes().to_vec();
        buf.extend_from_slice(body);
        let result = read_frame(&mut Cursor::new(buf));
        assert!(
            matches!(result, Err(IpcError::BadFrame(_))),
            "expected BadFrame for malformed JSON, got {:?}", result
        );
    }

    // ── Unknown type yields UnknownType + correct error_code ──────────────────

    #[test]
    fn unknown_type_field_yields_unknown_type_error() {
        let json = r#"{"type":"Frobnicate","seq":1,"reply_to":null,"payload":null}"#;
        let body = json.as_bytes();
        let mut buf = (body.len() as u32).to_le_bytes().to_vec();
        buf.extend_from_slice(body);
        let result = read_frame(&mut Cursor::new(buf));
        assert!(
            matches!(result, Err(IpcError::UnknownType(_))),
            "expected UnknownType for unknown type field, got {:?}", result
        );
    }

    #[test]
    fn unknown_type_error_code_maps_to_unknown_type() {
        let json = r#"{"type":"Frobnicate","seq":1,"reply_to":null,"payload":null}"#;
        let body = json.as_bytes();
        let mut buf = (body.len() as u32).to_le_bytes().to_vec();
        buf.extend_from_slice(body);
        let err = read_frame(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(
            err.error_code(),
            Some(ErrorCode::UnknownType),
            "UnknownType IpcError must map to ErrorCode::UnknownType"
        );
    }

    #[test]
    fn bad_frame_error_code_maps_to_bad_frame() {
        // Malformed JSON -> BadFrame -> error_code() == Some(BadFrame)
        let body = b"{{broken";
        let mut buf = (body.len() as u32).to_le_bytes().to_vec();
        buf.extend_from_slice(body);
        let err = read_frame(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(
            err.error_code(),
            Some(ErrorCode::BadFrame),
            "BadFrame IpcError must map to ErrorCode::BadFrame"
        );
    }

    #[test]
    fn io_error_code_maps_to_none() {
        let ipc_err = IpcError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "dropped",
        ));
        assert_eq!(
            ipc_err.error_code(),
            None,
            "Io IpcError must map to None"
        );
    }
}
