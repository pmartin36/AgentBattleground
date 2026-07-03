//! Unix-socket IPC transport (spec 14 / spec 31 Phase B bucket b4).
//!
//! Relocated here from `game::ipc_server` (b4-t1). The `SceneId::from_wire`
//! validation gate is replaced by `catalog.is_available` so this crate has no
//! dependency on game-only scene identifiers.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::ipc::{read_frame, write_frame, Envelope, ErrorCode, ErrorPayload, IpcError, Message};
use crate::scene::manager::Command;
use crate::scene_key::SceneKey;
use crate::SceneCatalog;

/// Outbound item pushed by the main loop to the IPC writer thread. Canonical
/// definition lives in `crate::ipc::Event`; re-exported here so every caller
/// (game's `pub use` shim included) keeps resolving `ipc_server::Event`.
pub use crate::ipc::Event;

/// Handle returned by `spawn`. Owns the socket-path record and the outbound
/// event sender. Removes the socket file on `Drop`.
pub struct IpcHandle {
    pub socket_path: PathBuf,
    pub events: Sender<Event>,
}

impl Drop for IpcHandle {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Bind the per-pid Unix socket, start IPC threads, and return:
/// - `IpcHandle` — socket path + outbound event sender
/// - `Receiver<Command>` — inbound commands for the main loop to drain
///
/// `catalog` replaces the old `SceneId::from_wire` gate: a `SwitchScene`
/// wire target is now validated via `catalog.is_available(&SceneKey::new(target))`.
pub fn spawn(catalog: Arc<dyn SceneCatalog>) -> std::io::Result<(IpcHandle, Receiver<Command>)> {
    let path = socket_path();

    // Ensure parent directory exists; remove any stale socket at this path.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&path);

    // Bind the Unix domain socket.
    let listener = UnixListener::bind(&path)?;

    // Restrict socket to owner-only access (0600).
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(&path, perms)?;

    // Command channel: READER threads → main loop.
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Command>();

    // Event channel: main loop (and READER error replies) → WRITER thread.
    let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();

    // Shared write slot: None when no client is connected; Some(stream) otherwise.
    let slot: Arc<Mutex<Option<UnixStream>>> = Arc::new(Mutex::new(None));

    // Single-client guard: true while a client connection is active.
    let connected = Arc::new(AtomicBool::new(false));

    // ── WRITER thread ─────────────────────────────────────────────────────────
    // Receives Event values, stamps monotonic seq, writes framed Envelope to
    // the currently-connected client.  Events arrive while no client is
    // connected are dropped silently (spec 14:131).
    {
        let slot_writer = Arc::clone(&slot);
        let connected_writer = Arc::clone(&connected);
        std::thread::spawn(move || {
            for (next_seq, event) in (0_u64..).zip(event_rx) {
                let env = Envelope::new(next_seq, event.reply_to, event.body);
                // Deliver to the connected client.  If the slot is not yet
                // populated (possible during connection-setup races), retry
                // briefly so events sent immediately after connect() are not
                // silently dropped.  The 50 ms cap still honours spec 14:131
                // ("stateless, no buffering") for the steady-state absent-
                // client case.
                let deadline = std::time::Instant::now()
                    + std::time::Duration::from_millis(50);
                loop {
                    {
                        let mut guard = slot_writer.lock().unwrap();
                        if let Some(ref mut writer) = *guard {
                            if write_frame(writer, &env).is_err() {
                                *guard = None;
                                // Mirror the reader loop's IO-failure handling
                                // (below): clearing only the slot leaves
                                // `connected` stuck true forever if the read
                                // half doesn't independently error, permanently
                                // refusing new clients in the accept-supervisor.
                                connected_writer.store(false, Ordering::SeqCst);
                            }
                            break;
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        break; // truly no client; drop the event
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
        });
    }

    // ── ACCEPT-SUPERVISOR thread ──────────────────────────────────────────────
    // Loops listener.accept().  Enforces single-client via AtomicBool; drops
    // a second concurrent connection immediately (EOF to that client).
    {
        let slot_supervisor = Arc::clone(&slot);
        let connected_supervisor = Arc::clone(&connected);
        let event_tx_supervisor = event_tx.clone();
        let catalog_supervisor = Arc::clone(&catalog);

        std::thread::spawn(move || {
            for accept_result in listener.incoming() {
                let stream = match accept_result {
                    Ok(s) => s,
                    Err(_) => break,
                };

                // Attempt to claim the single-client slot.
                if connected_supervisor
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    // Already have a client — refuse by dropping (EOF to caller).
                    drop(stream);
                    continue;
                }

                // Clone the stream so we keep a write half in the shared slot
                // and give the read half to the READER thread.
                let write_half = match stream.try_clone() {
                    Ok(s) => s,
                    Err(_) => {
                        connected_supervisor.store(false, Ordering::SeqCst);
                        continue;
                    }
                };

                {
                    let mut guard = slot_supervisor.lock().unwrap();
                    *guard = Some(write_half);
                }

                // Notify the main loop that a client has connected (b4-t2).
                let _ = cmd_tx.send(Command::ClientConnected);

                // Spawn a READER for this connection.
                let cmd_tx_r = cmd_tx.clone();
                let event_tx_r = event_tx_supervisor.clone();
                let slot_r = Arc::clone(&slot_supervisor);
                let connected_r = Arc::clone(&connected_supervisor);
                let catalog_r = Arc::clone(&catalog_supervisor);

                std::thread::spawn(move || {
                    reader_loop(stream, cmd_tx_r, event_tx_r, slot_r, connected_r, catalog_r);
                });
            }
        });
    }

    let handle = IpcHandle {
        socket_path: path,
        events: event_tx,
    };

    Ok((handle, cmd_rx))
}

/// Per-pid socket path:
///   `$XDG_RUNTIME_DIR/agent-battleground/inspect-<pid>.sock`
/// Fallback: `/tmp/agent-battleground/inspect-<pid>.sock`
///
/// An internal atomic counter makes successive calls within the same process
/// return distinct paths — necessary for test isolation when `spawn()` is
/// called multiple times in parallel tests.  In production (one `spawn()` call
/// per process), the counter is always 0 and the path matches the public spec.
pub fn socket_path() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let filename = if n == 0 {
        format!("inspect-{}.sock", pid)
    } else {
        format!("inspect-{}-{}.sock", pid, n)
    };
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let mut path = PathBuf::from(dir);
        path.push("agent-battleground");
        path.push(filename);
        path
    } else {
        let mut path = PathBuf::from("/tmp/agent-battleground");
        path.push(filename);
        path
    }
}

// ── READER thread body ────────────────────────────────────────────────────────

/// Forward `cmd` to the main loop, then reply `Ack{reply_to: seq}` — the
/// "forward a command, then acknowledge" shape shared by `SwitchScene`
/// (known target), `ApplyState`, and `Subscribe`, previously duplicated
/// verbatim at each call site.
fn forward_and_ack(
    cmd_tx: &std::sync::mpsc::Sender<Command>,
    event_tx: &Sender<Event>,
    cmd: Command,
    seq: u64,
) {
    let _ = cmd_tx.send(cmd);
    let _ = event_tx.send(Event {
        body: Message::Ack,
        reply_to: Some(seq),
    });
}

/// Reads framed messages from `stream` (the read half of the accepted connection).
///
/// - `Message::SwitchScene` with an available target (`catalog.is_available`)
///   → forwards `Command::SwitchScene`.
/// - `Message::SwitchScene` with an unavailable target → pushes `Event{Error{UnknownScene}}`.
/// - `Message::ApplyState` → forwards `Command::ApplyState` + replies `Ack{reply_to: seq}`.
/// - `Message::Subscribe` → forwards `Command::Subscribe` + replies `Ack{reply_to: seq}`.
/// - Other body types → ignored.
/// - `IpcError::Io` → connection closed: clear slot, release `connected`, return.
/// - `IpcError::BadFrame` / `IpcError::UnknownType` → push `Event{Error{code}}`, continue.
fn reader_loop(
    mut stream: UnixStream,
    cmd_tx: std::sync::mpsc::Sender<Command>,
    event_tx: Sender<Event>,
    slot: Arc<Mutex<Option<UnixStream>>>,
    connected: Arc<AtomicBool>,
    catalog: Arc<dyn SceneCatalog>,
) {
    loop {
        match read_frame(&mut stream) {
            Ok(env) => {
                match env.body {
                    Message::SwitchScene(ss) => {
                        let target = SceneKey::new(ss.target.clone());
                        if catalog.is_available(&target) {
                            forward_and_ack(
                                &cmd_tx,
                                &event_tx,
                                Command::SwitchScene {
                                    target,
                                    params: ss.params,
                                },
                                env.seq,
                            );
                        } else {
                            let _ = event_tx.send(Event {
                                body: Message::Error(ErrorPayload {
                                    code: ErrorCode::UnknownScene,
                                    message: format!("unknown scene: {}", ss.target),
                                }),
                                reply_to: Some(env.seq),
                            });
                        }
                    }
                    Message::ApplyState(a) => {
                        forward_and_ack(
                            &cmd_tx,
                            &event_tx,
                            Command::ApplyState {
                                id: a.id,
                                patch: a.patch,
                            },
                            env.seq,
                        );
                    }
                    Message::Subscribe(s) => {
                        forward_and_ack(
                            &cmd_tx,
                            &event_tx,
                            Command::Subscribe { live: s.live },
                            env.seq,
                        );
                    }
                    _ => {
                        // All other message types are ignored in M1.
                    }
                }
            }
            Err(IpcError::Io(_)) => {
                // Transport-level failure: connection closed or reset.
                {
                    let mut guard = slot.lock().unwrap();
                    *guard = None;
                }
                connected.store(false, Ordering::SeqCst);
                return;
            }
            Err(e) => {
                // Protocol error (BadFrame / UnknownType): send error back and keep looping.
                if let Some(code) = e.error_code() {
                    let _ = event_tx.send(Event {
                        body: Message::Error(ErrorPayload {
                            code,
                            message: e.to_string(),
                        }),
                        reply_to: None,
                    });
                }
                // IpcError::Io cannot reach here (handled by the arm above).
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────
//
// Relocated verbatim (in meaning) from `game/src/ipc_server.rs`'s test module.
// Wire spellings that referenced game-only `SceneId`/`scene_for_digit` are
// re-targeted at the shared `test_support::MockCatalog` fixture ("A"/"B"/"C"
// available, everything else unavailable) per the b4-t1 blueprint — the
// assertion *meaning* ("known target forwards+Acks; unknown target is
// rejected with reply_to=seq, no Command forwarded") is unchanged.

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    use crate::inspect::{FieldSchema, FieldTag};
    use crate::ipc::{
        read_frame, write_frame, CatalogEntry, Envelope, ErrorCode, Hello, Message, SceneChanged,
        SwitchScene,
    };
    use crate::test_support::MockCatalog;
    use crate::SceneKey;

    use super::*;

    /// Retry-connect until the server's accept loop is listening (up to 2 s).
    fn connect_retry(path: &std::path::Path) -> UnixStream {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match UnixStream::connect(path) {
                Ok(s) => return s,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("timed out connecting to {:?}: {}", path, e),
            }
        }
    }

    // ── switch_scene_frame_forwards_command ───────────────────────────────────

    /// A framed `SwitchScene{target:"A"}` from a raw client causes
    /// `Command::SwitchScene{target: SceneKey::new("A"), ..}` on cmd_rx.
    #[test]
    fn switch_scene_frame_forwards_command() {
        let (handle, cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");
        let mut client = connect_retry(&handle.socket_path);

        let first = cmd_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ClientConnected must arrive before SwitchScene");
        assert!(
            matches!(first, Command::ClientConnected),
            "first command after connect must be ClientConnected"
        );

        let req = Envelope::new(
            1,
            None,
            Message::SwitchScene(SwitchScene {
                target: "A".to_string(),
                params: None,
            }),
        );
        write_frame(&mut client, &req).expect("write_frame");

        let cmd = cmd_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("SwitchScene Command must be forwarded within 2 s");
        match cmd {
            Command::ClientConnected => panic!("expected SwitchScene, got ClientConnected"),
            Command::SwitchScene { target, .. } => {
                assert_eq!(
                    target,
                    SceneKey::new("A"),
                    "forwarded Command target must be A"
                );
            }
            Command::ApplyState { .. } | Command::Subscribe { .. } => {
                panic!("expected SwitchScene, got ApplyState/Subscribe")
            }
        }
    }

    // ── unknown_target_yields_error_no_command ────────────────────────────────

    /// A `SwitchScene` with an unavailable wire target ("Nope") causes the
    /// server to send back `Error{UnknownScene, reply_to: <request seq>}` and
    /// NOT forward a `SwitchScene` Command.
    #[test]
    fn unknown_target_yields_error_no_command() {
        let (handle, cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");
        let mut client = connect_retry(&handle.socket_path);
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let first = cmd_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ClientConnected must arrive before SwitchScene");
        assert!(
            matches!(first, Command::ClientConnected),
            "first command after connect must be ClientConnected"
        );

        let req_seq = 42u64;
        let req = Envelope::new(
            req_seq,
            None,
            Message::SwitchScene(SwitchScene {
                target: "Nope".to_string(),
                params: None,
            }),
        );
        write_frame(&mut client, &req).expect("write_frame");

        let resp = read_frame(&mut client).expect("error response must arrive");
        assert_eq!(
            resp.reply_to,
            Some(req_seq),
            "Error reply_to must equal the request seq"
        );
        match resp.body {
            Message::Error(ep) => {
                assert_eq!(
                    ep.code,
                    ErrorCode::UnknownScene,
                    "error code must be UnknownScene"
                );
            }
            other => panic!("expected Error message, got {:?}", other),
        }

        assert!(
            cmd_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "unknown target must not produce a SwitchScene Command on the channel"
        );
    }

    // ── outbound_event_is_framed_to_client ────────────────────────────────────

    /// Events pushed on `handle.events` are framed and delivered to the
    /// connected client in order; seq is monotonically increasing across events.
    #[test]
    fn outbound_event_is_framed_to_client() {
        let (handle, _cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");
        let mut client = connect_retry(&handle.socket_path);
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        handle
            .events
            .send(Event {
                body: Message::Ack,
                reply_to: Some(10),
            })
            .expect("send event 1");
        handle
            .events
            .send(Event {
                body: Message::Ack,
                reply_to: Some(11),
            })
            .expect("send event 2");

        let ev1 = read_frame(&mut client).expect("first event must arrive");
        let ev2 = read_frame(&mut client).expect("second event must arrive");

        assert_eq!(ev1.body, Message::Ack, "ev1 body must be Ack");
        assert_eq!(ev1.reply_to, Some(10), "ev1 reply_to must be 10");
        assert_eq!(ev2.body, Message::Ack, "ev2 body must be Ack");
        assert!(
            ev2.seq > ev1.seq,
            "seq must be monotonically increasing: ev1.seq={} ev2.seq={}",
            ev1.seq,
            ev2.seq
        );
    }

    // ── second_connection_refused ─────────────────────────────────────────────

    /// A second concurrent connection is refused (immediate EOF); the first
    /// client continues to forward Commands normally.
    #[test]
    fn second_connection_refused() {
        let (handle, cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");

        let mut client_a = connect_retry(&handle.socket_path);

        let first = cmd_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ClientConnected must arrive after client A connects");
        assert!(
            matches!(first, Command::ClientConnected),
            "first command after connect must be ClientConnected"
        );

        let mut client_b =
            UnixStream::connect(&handle.socket_path).expect("connect B must not error");
        client_b
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let b_result = read_frame(&mut client_b);
        assert!(
            b_result.is_err(),
            "second client read must return Err (EOF or IO error indicating refusal)"
        );

        let req = Envelope::new(
            1,
            None,
            Message::SwitchScene(SwitchScene {
                target: "A".to_string(),
                params: None,
            }),
        );
        write_frame(&mut client_a, &req).expect("A write_frame must succeed");
        let cmd = cmd_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("A's Command must still arrive after B was refused");
        match cmd {
            Command::ClientConnected => panic!("expected SwitchScene, got ClientConnected"),
            Command::SwitchScene { target, .. } => {
                assert_eq!(target, SceneKey::new("A"), "A's Command target must be A");
            }
            Command::ApplyState { .. } | Command::Subscribe { .. } => {
                panic!("expected SwitchScene, got ApplyState/Subscribe")
            }
        }
    }

    // ── writer_failure_resets_connected_for_reconnect ─────────────────────────

    /// A write failure to a dead client (its socket closed without the read
    /// half independently erroring first) must reset `connected` so a new
    /// client can connect afterward — not permanently refuse everyone.
    #[test]
    fn writer_failure_resets_connected_for_reconnect() {
        let (handle, cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");

        let client_a = connect_retry(&handle.socket_path);
        let _ = cmd_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ClientConnected must arrive after client A connects");
        drop(client_a);

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            handle
                .events
                .send(Event {
                    body: Message::Ack,
                    reply_to: None,
                })
                .expect("event channel must accept the send");
            std::thread::sleep(Duration::from_millis(30));

            if let Ok(_client_b) = UnixStream::connect(&handle.socket_path) {
                if let Ok(Command::ClientConnected) =
                    cmd_rx.recv_timeout(Duration::from_millis(150))
                {
                    return; // reconnect succeeded — test passes
                }
            }

            if std::time::Instant::now() >= deadline {
                panic!(
                    "client B was never able to reconnect within the deadline — \
                     `connected` was likely never reset after the writer failure"
                );
            }
        }
    }

    // ── drop_handle_removes_socket_file ───────────────────────────────────────

    /// The socket file exists after `spawn` and is absent after dropping the handle.
    #[test]
    fn drop_handle_removes_socket_file() {
        let (handle, _cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");
        let path = handle.socket_path.clone();
        assert!(
            path.exists(),
            "socket file must exist immediately after spawn"
        );

        drop(handle);
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !path.exists(),
            "socket file must be removed after IpcHandle is dropped"
        );
    }

    // ═══════════════════════════════════════ b4t2: protocol integration ════════
    //
    // Scene is not Send, so we cannot move a real SceneManager across threads
    // here. This lightweight protocol-stub harness mirrors what
    // apply_command + process_pending_notify do, using inline synthetic
    // `CatalogEntry` values (no reference to any real game scene — b4-t1
    // strips the old harness's `crate::scenes::scene_for_digit` dependency,
    // which cannot exist in scene-core).

    fn stub_schema(name: &str) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            label: None,
            tag: FieldTag::Struct,
            readonly: false,
            hidden: false,
            range: None,
            children: vec![],
            variants: vec![],
        }
    }

    fn b4t2_harness(
        cmd_rx: std::sync::mpsc::Receiver<Command>,
        event_tx: std::sync::mpsc::Sender<Event>,
    ) {
        let scenes: Vec<CatalogEntry> = ["Scene1", "Scene2", "Scene3", "Scene4"]
            .iter()
            .map(|&n| CatalogEntry {
                id: SceneKey::new(n),
                name: n.to_string(),
                schema: stub_schema(n),
            })
            .collect();
        let active = SceneKey::new("MainHub");

        loop {
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    Command::ClientConnected => {
                        let _ = event_tx.send(Event {
                            body: Message::Hello(Hello {
                                scenes: scenes.clone(),
                                active: active.clone(),
                            }),
                            reply_to: None,
                        });
                    }
                    Command::SwitchScene { target, .. } => {
                        let _ = event_tx.send(Event {
                            body: Message::SceneChanged(SceneChanged {
                                id: target,
                                snapshot: serde_json::Value::Null,
                            }),
                            reply_to: None,
                        });
                    }
                    Command::ApplyState { .. } | Command::Subscribe { .. } => {
                        // Not exercised by this harness.
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// On connect the server must push a framed Hello with four synthetic
    /// scenes and `active: MainHub`; `reply_to` must be `None` (unsolicited).
    #[test]
    fn b4t2_connect_receives_hello_with_four_scenes_active_main_hub() {
        let (handle, cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");
        let event_tx = handle.events.clone();
        std::thread::spawn(move || b4t2_harness(cmd_rx, event_tx));

        let mut client = connect_retry(&handle.socket_path);
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let env = read_frame(&mut client).expect("Hello must arrive within 2 s of connect");
        assert!(
            env.reply_to.is_none(),
            "Hello must be unsolicited (reply_to: None)"
        );
        match env.body {
            Message::Hello(h) => {
                assert_eq!(
                    h.active,
                    SceneKey::new("MainHub"),
                    "Hello.active must be MainHub"
                );
                assert_eq!(h.scenes.len(), 4, "Hello.scenes must list four scenes");
            }
            other => panic!("expected Hello, got {:?}", other),
        }
    }

    /// After an available-target `SwitchScene` the client must receive `Ack`
    /// (reply_to = request seq) then `SceneChanged` (reply_to: None).
    #[test]
    fn b4t2_switch_scene_yields_ack_then_scene_changed() {
        let (handle, cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");
        let event_tx = handle.events.clone();
        std::thread::spawn(move || b4t2_harness(cmd_rx, event_tx));

        let mut client = connect_retry(&handle.socket_path);
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let first = read_frame(&mut client).expect("Hello must arrive");
        assert!(
            matches!(first.body, Message::Hello(_)),
            "first message must be Hello, got {:?}",
            first.body
        );

        let req_seq = 42u64;
        let req = Envelope::new(
            req_seq,
            None,
            Message::SwitchScene(SwitchScene {
                target: "A".to_string(),
                params: None,
            }),
        );
        write_frame(&mut client, &req).expect("write SwitchScene");

        let ack_env = read_frame(&mut client).expect("Ack must arrive after SwitchScene");
        assert_eq!(
            ack_env.reply_to,
            Some(req_seq),
            "Ack reply_to must equal the request seq"
        );
        assert_eq!(ack_env.body, Message::Ack, "Ack body must be Ack");

        let sc_env = read_frame(&mut client).expect("SceneChanged must arrive after switch");
        assert!(
            sc_env.reply_to.is_none(),
            "SceneChanged must be unsolicited (reply_to: None)"
        );
        match sc_env.body {
            Message::SceneChanged(sc) => {
                assert_eq!(
                    sc.id,
                    SceneKey::new("A"),
                    "SceneChanged.id must be A"
                );
            }
            other => panic!("expected SceneChanged, got {:?}", other),
        }
    }

    /// An unavailable `SwitchScene` target must produce `Error{UnknownScene}`
    /// (reply_to = request seq) and NO subsequent `SceneChanged`.
    #[test]
    fn b4t2_unknown_target_yields_error_no_scene_changed() {
        let (handle, cmd_rx) = spawn(Arc::new(MockCatalog)).expect("spawn must succeed");
        let event_tx = handle.events.clone();
        std::thread::spawn(move || b4t2_harness(cmd_rx, event_tx));

        let mut client = connect_retry(&handle.socket_path);
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let _ = read_frame(&mut client).expect("Hello");

        let req_seq = 77u64;
        let req = Envelope::new(
            req_seq,
            None,
            Message::SwitchScene(SwitchScene {
                target: "Nope".to_string(),
                params: None,
            }),
        );
        write_frame(&mut client, &req).expect("write SwitchScene");

        let err_env = read_frame(&mut client).expect("Error must arrive");
        assert_eq!(
            err_env.reply_to,
            Some(req_seq),
            "Error reply_to must equal the request seq"
        );
        match err_env.body {
            Message::Error(ep) => {
                assert_eq!(ep.code, ErrorCode::UnknownScene, "code must be UnknownScene");
            }
            other => panic!("expected Error, got {:?}", other),
        }

        client
            .set_read_timeout(Some(Duration::from_millis(400)))
            .unwrap();
        let maybe = read_frame(&mut client);
        assert!(
            maybe.is_err(),
            "unknown target must not produce a SceneChanged; got: {:?}",
            maybe
        );
    }
}
