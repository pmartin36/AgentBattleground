/// End-to-end headless integration test for the scene-switcher M1 protocol.
///
/// Exercises the full round-trip: connect → Hello → SwitchScene → Ack + SceneChanged.
/// Uses a REAL SceneManager (not the b4t2_harness stub) over a REAL Unix socket.
///
/// Threading shape (Scene is !Send → SceneManager must stay on main test thread):
///   MAIN TEST THREAD — owns SceneManager, runs the headless driver loop.
///   CLIENT THREAD    — owns UnixStream, runs the assertion sequence.
///   Arc<AtomicBool> `done` signals when the client is finished.
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use scene_core::net::ipc_server;
use scene_core::scene::manager::SceneManager;
use game::registry::GameCatalog;
use game::scene_id::SceneId;
use scene_core::ipc::{Envelope, Message, SwitchScene, read_frame, write_frame};
use scene_core::SceneKey;

/// Retry-connect to the Unix socket until the accept thread is ready (up to 2 s).
/// Mirrors the pattern used in ipc_server.rs tests.
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

/// Spec-14 M1 acceptance test — steps 1–3 + automated step 5.
///
/// Connect → assert Hello{four scenes, active:MainHub} →
/// send SwitchScene{BattleViewer} → assert Ack{reply_to:1} + SceneChanged{BattleViewer}.
#[test]
fn e2e_connect_then_switch_battle_viewer() {
    // ── Spawn the IPC server ──────────────────────────────────────────────────
    let (handle, cmd_rx) =
        ipc_server::spawn(Arc::new(GameCatalog)).expect("ipc_server::spawn must succeed");

    // Clone the event sender so the driver loop can push SceneChanged.
    let events = handle.events.clone();

    // ── Boot the SceneManager (stays on this thread; Scene is !Send) ──────────
    let mut mgr = SceneManager::new(SceneKey::from(SceneId::MainHub), Box::new(GameCatalog));

    // ── Shared done flag ──────────────────────────────────────────────────────
    let done = Arc::new(AtomicBool::new(false));
    let done_client = Arc::clone(&done);

    // ── Socket path captured before spawning the client thread ───────────────
    let socket_path = handle.socket_path.clone();

    // ── CLIENT THREAD ─────────────────────────────────────────────────────────
    let client_thread = std::thread::spawn(move || {
        let mut client = connect_retry(&socket_path);
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set_read_timeout");

        // Step 1–2: read Hello (unsolicited, reply_to: None).
        let hello_env = read_frame(&mut client).expect("Hello must arrive within 2 s of connect");
        assert!(
            hello_env.reply_to.is_none(),
            "Hello must be unsolicited (reply_to: None), got: {:?}",
            hello_env.reply_to
        );
        let hello = match hello_env.body {
            Message::Hello(h) => h,
            other => panic!("expected Hello as first message, got {:?}", other),
        };

        // Step 2: verify Hello contents.
        assert_eq!(
            hello.active,
            SceneKey::from(SceneId::MainHub),
            "Hello.active must be MainHub at boot"
        );
        assert_eq!(
            hello.scenes.len(),
            4,
            "Hello.scenes must list exactly four M1 scenes, got {}",
            hello.scenes.len()
        );
        let expected_ids = [
            SceneKey::from(SceneId::MainHub),
            SceneKey::from(SceneId::BattleViewer),
            SceneKey::from(SceneId::RosterManager),
            SceneKey::from(SceneId::Leaderboard),
        ];
        let got_ids: Vec<SceneKey> = hello.scenes.iter().map(|e| e.id.clone()).collect();
        for expected in expected_ids {
            assert!(
                got_ids.contains(&expected),
                "Hello catalog must include {:?}; got {:?}",
                expected,
                got_ids
            );
        }
        for entry in &hello.scenes {
            let id = SceneId::from_key(&entry.id).expect("catalog key must map to a SceneId");
            assert_eq!(
                entry.name,
                id.display_name(),
                "CatalogEntry.name must equal display_name() for {:?}",
                entry.id
            );
        }

        // ClientConnected also pushes an unprompted StateSnapshot for the
        // active scene immediately after Hello (b5-t3) — consume it before
        // driving the SwitchScene exchange.
        let snapshot_env =
            read_frame(&mut client).expect("StateSnapshot must arrive after Hello");
        assert!(
            snapshot_env.reply_to.is_none(),
            "initial StateSnapshot must be unsolicited (reply_to: None), got: {:?}",
            snapshot_env.reply_to
        );
        match snapshot_env.body {
            Message::StateSnapshot(ref payload) => {
                assert_eq!(
                    payload.id,
                    SceneKey::from(SceneId::MainHub),
                    "initial StateSnapshot.id must be the active scene (MainHub)"
                );
            }
            ref other => panic!("expected StateSnapshot after Hello, got {:?}", other),
        }

        // Step 3: send SwitchScene{BattleViewer} with seq=1.
        let req_seq: u64 = 1;
        let req = Envelope::new(
            req_seq,
            None,
            Message::SwitchScene(SwitchScene {
                target: "BattleViewer".to_string(),
                params: None,
            }),
        );
        write_frame(&mut client, &req).expect("write SwitchScene");

        // Step 4–5: read Ack and SceneChanged (order-tolerant).
        let frame_a = read_frame(&mut client).expect("first response must arrive");
        let frame_b = read_frame(&mut client).expect("second response must arrive");

        let mut ack_env: Option<Envelope> = None;
        let mut sc_env: Option<Envelope> = None;
        for frame in [frame_a, frame_b] {
            match &frame.body {
                Message::Ack => ack_env = Some(frame),
                Message::SceneChanged(_) => sc_env = Some(frame),
                other => panic!("unexpected message after SwitchScene: {:?}", other),
            }
        }

        let ack = ack_env.expect("Ack must be received after SwitchScene");
        assert_eq!(
            ack.reply_to,
            Some(req_seq),
            "Ack reply_to must equal the request seq ({})",
            req_seq
        );

        let sc = sc_env.expect("SceneChanged must be received after SwitchScene");
        assert!(
            sc.reply_to.is_none(),
            "SceneChanged must be unsolicited (reply_to: None), got: {:?}",
            sc.reply_to
        );
        match sc.body {
            Message::SceneChanged(ref payload) => {
                assert_eq!(
                    payload.id,
                    SceneKey::from(SceneId::BattleViewer),
                    "SceneChanged.id must be BattleViewer"
                );
            }
            ref other => panic!("expected SceneChanged body, got {:?}", other),
        }

        // Signal the driver loop to stop.
        done_client.store(true, Ordering::Release);
    });

    // ── DRIVER LOOP (main test thread) ────────────────────────────────────────
    // Mirrors main.rs steps 1 & 4; terminal/render/input stripped.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if done.load(Ordering::Acquire) {
            break;
        }
        if Instant::now() >= deadline {
            panic!("e2e driver loop timed out after 5 s — client did not signal done");
        }
        // Drain all pending inbound commands.
        while let Ok(cmd) = cmd_rx.try_recv() {
            mgr.apply_command(cmd, &events);
        }
        // Apply any queued transition and emit SceneChanged if one fired.
        mgr.process_pending_notify(&events);

        std::thread::sleep(Duration::from_millis(2));
    }

    // Propagate any assertion panic from the client thread.
    client_thread.join().expect("client thread panicked");

    // Verify the manager itself switched to BattleViewer.
    assert_eq!(
        mgr.active_id(),
        SceneKey::from(SceneId::BattleViewer),
        "SceneManager.active_id must be BattleViewer after the switch"
    );
}
