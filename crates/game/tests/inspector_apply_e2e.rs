/// End-to-end headless integration test for `ApplyState` (spec 15 point 5).
///
/// Exercises the full wire path — connect → Hello → initial StateSnapshot →
/// `ApplyState` → `Ack` + `StateSnapshot`/`Error` — against a REAL `SceneManager`
/// booted into a REAL `BattleViewer`, over a REAL Unix socket. No direct
/// Rust-level field mutation: every assertion about scene state goes through
/// the wire (`StateSnapshot`) or through `render()`.
///
/// Threading shape mirrors `switch_e2e.rs`:
///   MAIN TEST THREAD — owns SceneManager (Scene is !Send), runs the driver loop.
///   CLIENT THREAD    — owns UnixStream, runs the assertion sequence.
use std::collections::BTreeMap;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_core::net::ipc_server;
use engine_core::scene::manager::SceneManager;
use game::registry::GameCatalog;
use game::scene_id::SceneId;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use engine_core::ipc::{ApplyState, Envelope, Message, StateSnapshot, read_frame, write_frame};
use engine_core::SceneKey;
use serde_json::{json, Value as JsonValue};

/// Retry-connect to the Unix socket until the accept thread is ready (up to 2 s).
fn connect_retry(path: &std::path::Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match UnixStream::connect(path) {
            Ok(s) => return s,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("timed out connecting to {:?}: {}", path, e),
        }
    }
}

/// Boots a real `SceneManager` into `BattleViewer`, spawns the real IPC
/// server, connects a client, consumes the connect-time `Hello` + initial
/// `StateSnapshot`, then runs `client_fn(client)` on a client thread while
/// draining `cmd_rx` on the main thread. Returns the manager after the
/// client signals done, so callers can assert on post-loop state (e.g. via
/// `render()`).
fn run_e2e(client_fn: impl FnOnce(&mut UnixStream) + Send + 'static) -> SceneManager {
    let (handle, cmd_rx) =
        ipc_server::spawn(Arc::new(GameCatalog)).expect("ipc_server::spawn must succeed");
    let events = handle.events.clone();
    let mut mgr = SceneManager::new(SceneKey::from(SceneId::BattleViewer), Box::new(GameCatalog));

    let done = Arc::new(AtomicBool::new(false));
    let done_client = Arc::clone(&done);
    let socket_path = handle.socket_path.clone();

    let client_thread = std::thread::spawn(move || {
        let mut client = connect_retry(&socket_path);
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set_read_timeout");

        let hello_env = read_frame(&mut client).expect("Hello must arrive after connect");
        assert!(
            matches!(hello_env.body, Message::Hello(_)),
            "expected Hello as first message, got {:?}",
            hello_env.body
        );
        let snap_env =
            read_frame(&mut client).expect("initial StateSnapshot must arrive after Hello");
        match snap_env.body {
            Message::StateSnapshot(ref payload) => {
                assert_eq!(
                    payload.id,
                    SceneKey::from(SceneId::BattleViewer),
                    "initial StateSnapshot.id must be the active scene (BattleViewer)"
                );
            }
            ref other => panic!("expected initial StateSnapshot, got {:?}", other),
        }

        client_fn(&mut client);
        done_client.store(true, Ordering::Release);
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if done.load(Ordering::Acquire) {
            break;
        }
        if Instant::now() >= deadline {
            panic!("e2e driver loop timed out after 5 s — client did not signal done");
        }
        while let Ok(cmd) = cmd_rx.try_recv() {
            mgr.apply_command(cmd, &events);
        }
        mgr.process_pending_notify(&events);
        std::thread::sleep(Duration::from_millis(2));
    }

    client_thread.join().expect("client thread panicked");
    mgr
}

/// Sends `ApplyState{id, patch}` with `seq`, then reads exactly two reply
/// frames and sorts them into `(Ack, other)` — order-tolerant, mirroring
/// `switch_e2e.rs`'s handling of `Ack`+`SceneChanged`.
fn send_apply_state_and_split_replies(
    client: &mut UnixStream,
    seq: u64,
    id: SceneKey,
    patch: BTreeMap<String, JsonValue>,
) -> Message {
    let req = Envelope::new(seq, None, Message::ApplyState(ApplyState { id, patch }));
    write_frame(client, &req).expect("write ApplyState");

    let frame_a = read_frame(client).expect("first ApplyState reply must arrive");
    let frame_b = read_frame(client).expect("second ApplyState reply must arrive");

    let mut ack_seen = false;
    let mut other: Option<Message> = None;
    for frame in [frame_a, frame_b] {
        match frame.body {
            Message::Ack => {
                assert_eq!(
                    frame.reply_to,
                    Some(seq),
                    "Ack reply_to must equal the request seq ({seq})"
                );
                ack_seen = true;
            }
            body => other = Some(body),
        }
    }
    assert!(ack_seen, "Ack must be received after ApplyState");
    other.expect("a StateSnapshot or Error must be received after ApplyState")
}

/// (1) `ApplyState{"elapsed": 5.0}` changes `BattleViewer.elapsed` (visible in
/// the StateSnapshot reply) and the next `render()` reflects it (differs from
/// an elapsed=0.0 baseline — wizard.gif has 15 frames @ 100ms, so 5.0 s lands
/// on a different frame than 0.0 s).
#[test]
fn apply_state_elapsed_changes_state_and_next_render_reflects_it() {
    let mgr = run_e2e(|client| {
        let mut patch = BTreeMap::new();
        patch.insert("elapsed".to_string(), json!(5.0));
        let reply = send_apply_state_and_split_replies(client, 1, SceneKey::from(SceneId::BattleViewer), patch);
        match reply {
            Message::StateSnapshot(StateSnapshot { id, snapshot }) => {
                assert_eq!(id, SceneKey::from(SceneId::BattleViewer));
                assert_eq!(
                    snapshot["elapsed"],
                    json!(5.0),
                    "StateSnapshot must reflect the applied elapsed value"
                );
            }
            other => panic!("expected StateSnapshot after a valid ApplyState, got {other:?}"),
        }
    });

    let mut after_terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    after_terminal.draw(|f| mgr.render(f)).unwrap();
    let after = after_terminal.backend().buffer().clone();

    let baseline_mgr = SceneManager::new(SceneKey::from(SceneId::BattleViewer), Box::new(GameCatalog));
    let mut baseline_terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    baseline_terminal.draw(|f| baseline_mgr.render(f)).unwrap();
    let baseline = baseline_terminal.backend().buffer().clone();

    assert_ne!(
        after, baseline,
        "render() after ApplyState{{elapsed:5.0}} must differ from the elapsed=0.0 baseline"
    );
}

/// (2) `ApplyState{"pieces[0].color":..., "pieces[0].transform.translate.x":...}`
/// visibly recolors/moves piece 0 per the StateSnapshot reply.
#[test]
fn apply_state_nested_piece_color_and_transform_paths_apply() {
    let mut mgr = run_e2e(|client| {
        let mut patch = BTreeMap::new();
        patch.insert("pieces[0].color".to_string(), json!("#ff0000ff"));
        patch.insert("pieces[0].transform.translate.x".to_string(), json!(3.0));
        let reply = send_apply_state_and_split_replies(client, 1, SceneKey::from(SceneId::BattleViewer), patch);
        match reply {
            Message::StateSnapshot(StateSnapshot { id, snapshot }) => {
                assert_eq!(id, SceneKey::from(SceneId::BattleViewer));
                assert_eq!(
                    snapshot["pieces"][0]["color"],
                    json!("#ff0000ff"),
                    "pieces[0].color must reflect the applied hex value"
                );
                assert_eq!(
                    snapshot["pieces"][0]["transform"]["translate"]["x"],
                    json!(3.0),
                    "pieces[0].transform.translate.x must reflect the applied value"
                );
            }
            other => panic!("expected StateSnapshot after a valid ApplyState, got {other:?}"),
        }
    });

    assert_eq!(
        mgr.active_inspect().snapshot()["pieces"][0]["color"],
        json!("#ff0000ff"),
        "the live SceneManager's piece 0 color must match the applied patch \
         after the wire round-trip"
    );
}

/// (3) `ApplyState{"pieces[0].team":"B"}` changes only `team` — `color` and
/// `transform` are byte-identical to the pre-apply snapshot (team change does
/// not re-derive stored fields).
#[test]
fn apply_state_team_only_patch_isolates_color_and_transform() {
    let baseline_snapshot = BattleViewerSnapshot::baseline();

    let mgr = run_e2e(move |client| {
        let mut patch = BTreeMap::new();
        patch.insert("pieces[0].team".to_string(), json!("B"));
        let reply = send_apply_state_and_split_replies(client, 1, SceneKey::from(SceneId::BattleViewer), patch);
        match reply {
            Message::StateSnapshot(StateSnapshot { id, snapshot }) => {
                assert_eq!(id, SceneKey::from(SceneId::BattleViewer));
                assert_eq!(
                    snapshot["pieces"][0]["team"],
                    json!("B"),
                    "pieces[0].team must change to B"
                );
                assert_eq!(
                    snapshot["pieces"][0]["color"], baseline_snapshot.piece0_color,
                    "pieces[0].color must be byte-identical to the pre-apply snapshot"
                );
                assert_eq!(
                    snapshot["pieces"][0]["transform"], baseline_snapshot.piece0_transform,
                    "pieces[0].transform must be byte-identical to the pre-apply snapshot"
                );
            }
            other => panic!("expected StateSnapshot after a valid ApplyState, got {other:?}"),
        }
    });
    let _ = mgr;
}

/// Baseline `pieces[0]` color/transform snapshot from a freshly-booted
/// `BattleViewer`, captured before any wire mutation.
struct BattleViewerSnapshot {
    piece0_color: JsonValue,
    piece0_transform: JsonValue,
}

impl BattleViewerSnapshot {
    fn baseline() -> Self {
        let mut mgr = SceneManager::new(SceneKey::from(SceneId::BattleViewer), Box::new(GameCatalog));
        let snap = mgr.active_inspect().snapshot();
        Self {
            piece0_color: snap["pieces"][0]["color"].clone(),
            piece0_transform: snap["pieces"][0]["transform"].clone(),
        }
    }
}

/// (4) `ApplyState{"pieces[0].col":99, "elapsed":1.0}` — the readonly `col`
/// path is rejected with `Error{BadField}` naming that path; `pieces[0].col`
/// stays unchanged while the valid sibling `elapsed` in the SAME patch still
/// applies (spec 14:230 partial-apply policy) and a `StateSnapshot`
/// reflecting the applied subset is still sent.
#[test]
fn apply_state_rejects_readonly_field_but_applies_valid_sibling_in_same_patch() {
    run_e2e(|client| {
        let baseline_col = BattleViewerSnapshot::baseline_col0();

        let mut patch = BTreeMap::new();
        patch.insert("pieces[0].col".to_string(), json!(99));
        patch.insert("elapsed".to_string(), json!(1.0));

        let req = Envelope::new(
            1,
            None,
            Message::ApplyState(ApplyState {
                id: SceneKey::from(SceneId::BattleViewer),
                patch,
            }),
        );
        write_frame(client, &req).expect("write ApplyState");

        // Rejected+accepted-sibling case: THREE replies (Ack, Error, StateSnapshot),
        // order-tolerant per research.md's DATA_FLOW.
        let mut ack_seen = false;
        let mut error_msg: Option<String> = None;
        let mut snapshot: Option<JsonValue> = None;
        for _ in 0..3 {
            let frame = read_frame(client).expect("a reply to the mixed ApplyState must arrive");
            match frame.body {
                Message::Ack => ack_seen = true,
                Message::Error(payload) => {
                    assert_eq!(
                        payload.code,
                        engine_core::ipc::ErrorCode::BadField,
                        "readonly rejection must use ErrorCode::BadField"
                    );
                    error_msg = Some(payload.message);
                }
                Message::StateSnapshot(StateSnapshot { id, snapshot: s }) => {
                    assert_eq!(id, SceneKey::from(SceneId::BattleViewer));
                    snapshot = Some(s);
                }
                other => panic!("unexpected message after mixed ApplyState: {other:?}"),
            }
        }

        assert!(ack_seen, "Ack must be received after ApplyState");
        let error_msg = error_msg.expect("Error{BadField} must be received for the readonly path");
        assert!(
            error_msg.contains("pieces[0].col"),
            "Error message must name the rejected path pieces[0].col; got: {error_msg}"
        );
        let snapshot = snapshot.expect(
            "StateSnapshot must still be received — the valid sibling `elapsed` must apply",
        );
        assert_eq!(
            snapshot["elapsed"],
            json!(1.0),
            "the valid sibling field elapsed must apply even though pieces[0].col was rejected"
        );
        assert_eq!(
            snapshot["pieces"][0]["col"], baseline_col,
            "pieces[0].col must be unchanged by the rejected readonly patch"
        );
    });
}

impl BattleViewerSnapshot {
    fn baseline_col0() -> JsonValue {
        let mut mgr = SceneManager::new(SceneKey::from(SceneId::BattleViewer), Box::new(GameCatalog));
        mgr.active_inspect().snapshot()["pieces"][0]["col"].clone()
    }
}
