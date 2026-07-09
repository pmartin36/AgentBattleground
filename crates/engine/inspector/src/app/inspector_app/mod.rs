use std::collections::BTreeMap;

use super::state::SwitcherState;
use crate::client::InspectorClient;

mod render;

/// The eframe app: owns state + the live client.
pub struct InspectorApp {
    state: SwitcherState,
    client: InspectorClient,
    /// "Apply on change" toggle (b4-t9); on by default (b2-t1).
    live_apply: bool,
}


impl InspectorApp {
    pub fn new(client: InspectorClient) -> Self {
        InspectorApp {
            state: SwitcherState::new(),
            client,
            live_apply: true,
        }
    }

    /// Drain all pending inbound messages into state.
    fn pump(&mut self) {
        loop {
            match self.client.incoming.try_recv() {
                Ok(msg) => self.state.apply(&msg),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.state.set_disconnected();
                    break;
                }
            }
        }
    }

    /// Send `ApplyState{active, dirty_patch()}` if the dirty buffer is
    /// non-empty, and mark the submit in-flight (b4-t9).
    pub fn submit(&mut self) {
        let patch = self.state.dirty_patch();
        if patch.is_empty() {
            return;
        }
        if let Some(id) = self.state.active.clone() {
            if let Err(e) = self.client.send_apply_state(id, patch) {
                eprintln!("inspector: send_apply_state failed: {e}");
            }
            self.state.begin_submit();
        }
    }

    /// Discard buffered edits; no socket I/O (b4-t9).
    pub fn revert(&mut self) {
        self.state.revert();
    }

    /// Toggle "apply on change"; sends `Subscribe{live}` only on a real
    /// transition (b4-t9).
    pub fn set_live_apply(&mut self, on: bool) {
        if on != self.live_apply {
            if let Err(e) = self.client.send_subscribe(on) {
                eprintln!("inspector: send_subscribe failed: {e}");
            }
            self.live_apply = on;
        }
    }

    /// Send the initial `Subscribe` reflecting `live_apply`'s default,
    /// exactly once at startup (b2-t1). Callers must invoke this exactly
    /// once, right after construction — it does not run automatically.
    pub fn start(&mut self) {
        if let Err(e) = self.client.send_subscribe(self.live_apply) {
            eprintln!("inspector: send_subscribe failed: {e}");
        }
    }

    /// Drain this frame's live-edit signal; while `live_apply` is on, send
    /// one `ApplyState` per edit (b4-t9).
    pub fn flush_edits(&mut self) {
        let edits = self.state.take_frame_edits();
        if !self.live_apply {
            return;
        }
        if let Some(id) = self.state.active.clone() {
            for (path, value) in edits {
                let mut patch = BTreeMap::new();
                patch.insert(path, value);
                if let Err(e) = self.client.send_apply_state(id.clone(), patch) {
                    eprintln!("inspector: send_apply_state failed: {e}");
                }
            }
        }
    }

}

/// Submit / Revert button `Response`s from the action row, surfaced so tests
/// can inspect `Response::enabled()` (b2-t2). Fields are read only by tests.
#[allow(dead_code)]
struct ActionRow {
    submit: egui::Response,
    revert: egui::Response,
}


impl eframe::App for InspectorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();

        if self.state.should_exit() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let _ = self.render(ui);
        self.flush_edits();

        ui.ctx().request_repaint();
    }
}


#[cfg(test)]
/// Test harness: accepts one connection, forwards every frame the client
/// sends to the returned receiver, and hands back a cloned write half so
/// the test can push server -> client replies (Ack/StateSnapshot)
/// directly, without a second background thread racing the assertions.
fn stub_app_harness(
    tag: &str,
) -> (
    InspectorApp,
    std::sync::mpsc::Receiver<engine_core::ipc::Envelope>,
    std::os::unix::net::UnixStream,
    std::path::PathBuf,
) {
    use crate::client::connect;
    use engine_core::ipc::read_frame;
    use std::os::unix::net::UnixListener;

    let path = std::env::temp_dir().join(format!(
        "inspector-app-test-b4t9-{}-{}.sock",
        std::process::id(),
        tag,
    ));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("bind tmp socket");

    let (frame_tx, frame_rx) = std::sync::mpsc::channel();
    let (stream_tx, stream_rx) = std::sync::mpsc::sync_channel(1);

    std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let write_half = stream.try_clone().expect("clone stream");
        stream_tx.send(write_half).ok();
        let mut read_half = stream;
        while let Ok(env) = read_frame(&mut read_half) {
            if frame_tx.send(env).is_err() {
                break;
            }
        }
    });

    let client = connect(&path).expect("connect must succeed");
    let app = InspectorApp::new(client);
    let server_write = stream_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("server must accept and hand back a write half");

    (app, frame_rx, server_write, path)
}

// ── b1-t1: full-width stacked layout (replaces `egui::Panel::right`) ────


#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_fixtures::*;
    use engine_core::ipc::{Message, StateSnapshot};
    use engine_core::SceneKey;

    /// When the stub server closes the socket, pump() must propagate the EOF
    /// through the reader-thread channel to state.should_exit() == true.
    /// RED: set_disconnected() does not yet set should_exit, so the loop times out.
    #[test]
    fn pump_sets_should_exit_on_eof() {
        use crate::client::connect;
        use engine_core::ipc::{write_frame, Envelope};
        use std::os::unix::net::UnixListener;
        use std::time::Duration;

        let path = std::env::temp_dir().join(format!(
            "inspector-app-test-eof-{}.sock",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind tmp socket");
        let path2 = path.clone();

        // Stub server: write a Hello, then drop the stream (EOF).
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let env = Envelope::new(0, None, four_scene_hello());
            let _ = write_frame(&mut stream, &env);
            // stream drops here → EOF on the client side
        });

        let client = connect(&path2).expect("connect must succeed");
        let mut app = InspectorApp::new(client);

        // Loop pump() until should_exit flips or 2 s timeout.
        let mut exited = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            app.pump();
            if app.state.should_exit() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = std::fs::remove_file(&path2);

        assert!(
            exited,
            "app.state.should_exit() must become true within 2 s of socket close"
        );
    }
    // ── b4-t9: Submit / Revert / Apply-on-change wired to the wire protocol ──

    /// Calls `app.pump()` in a loop until `pred` is true or `timeout` elapses.
    /// Returns whether `pred` became true.
    fn pump_until(
        app: &mut InspectorApp,
        mut pred: impl FnMut(&InspectorApp) -> bool,
        timeout: std::time::Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            app.pump();
            if pred(app) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Submitting a dirty field sends exactly one `ApplyState` whose `patch`
    /// is exactly the dirty overlay and whose `id` is the active scene, and
    /// marks the submit in-flight.
    #[test]
    fn submit_sends_apply_state_with_only_dirty_patch_and_marks_awaiting() {
        use engine_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("submit");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        assert!(
            pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2)),
            "app must observe Hello before submit"
        );

        app.state.mark_dirty("f0", serde_json::json!(true));
        app.submit();

        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ApplyState must arrive at the stub after submit");
        match env.body {
            Message::ApplyState(a) => {
                assert_eq!(
                    a.id,
                    SceneKey::new("MainHub"),
                    "submit's ApplyState must target the active scene"
                );
                let mut expected = BTreeMap::new();
                expected.insert("f0".to_string(), serde_json::json!(true));
                assert_eq!(
                    a.patch, expected,
                    "submit's ApplyState.patch must contain exactly the dirty overlay"
                );
            }
            other => panic!("expected ApplyState, got {:?}", other),
        }
        assert!(
            app.state.awaiting_submit,
            "submit must mark the submit in-flight (awaiting_submit)"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Submitting with an empty dirty buffer sends nothing over the socket.
    #[test]
    fn submit_with_empty_dirty_sends_nothing() {
        use engine_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("submit-empty");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2));

        app.submit();

        match frame_rx.recv_timeout(Duration::from_millis(200)) {
            Err(_) => {} // expected: nothing arrived
            Ok(env) => panic!(
                "submit with an empty dirty buffer must send nothing, got {:?}",
                env.body
            ),
        }

        let _ = std::fs::remove_file(&path);
    }

    /// After a Submit, feeding an `Ack` then a `StateSnapshot` for the active
    /// scene clears the dirty buffer and refreshes the displayed value from
    /// the new snapshot.
    #[test]
    fn ack_then_state_snapshot_after_submit_clears_dirty_and_refreshes() {
        use engine_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("submit-reply");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2));

        app.state.mark_dirty("f0", serde_json::json!(true));
        app.submit();
        let _ = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ApplyState must arrive after submit");

        write_frame(&mut server_write, &Envelope::new(1, None, Message::Ack)).expect("write Ack");
        write_frame(
            &mut server_write,
            &Envelope::new(
                2,
                None,
                Message::StateSnapshot(StateSnapshot {
                    id: SceneKey::new("MainHub"),
                    snapshot: serde_json::json!({"f0": true}),
                }),
            ),
        )
        .expect("write StateSnapshot");

        let cleared = pump_until(
            &mut app,
            |a| a.state.dirty_patch().is_empty(),
            Duration::from_secs(2),
        );
        assert!(
            cleared,
            "the dirty buffer must clear once Ack + StateSnapshot arrive after submit"
        );
        assert_eq!(
            app.state.display_value("f0"),
            Some(&serde_json::json!(true)),
            "display_value must reflect the refreshed StateSnapshot once the dirty overlay clears"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Revert with no prior submit performs no socket I/O and clears the
    /// dirty buffer.
    #[test]
    fn revert_sends_nothing_and_clears_dirty() {
        use engine_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("revert");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2));

        app.state.mark_dirty("f0", serde_json::json!(true));
        app.revert();

        assert!(
            app.state.dirty_patch().is_empty(),
            "revert must clear the dirty buffer"
        );

        match frame_rx.recv_timeout(Duration::from_millis(200)) {
            Err(_) => {} // expected: nothing arrived
            Ok(env) => panic!("revert must perform no socket I/O, got {:?}", env.body),
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── b2-t2: gate Revert on dirty_empty (mirror Submit's existing gate) ──

    /// The Revert button must render disabled while the dirty buffer is
    /// empty and enabled once a field has been marked dirty — mirroring
    /// Submit's existing `dirty_empty` gate exactly (round-2 issue 6).
    #[test]
    fn revert_button_disabled_when_dirty_empty_enabled_when_dirty() {
        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("revert-gate");
        app.state.apply(&four_scene_hello());

        let ctx = egui::Context::default();

        let mut captured: Option<ActionRow> = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1000.0),
                )),
                time: Some(0.0),
                ..Default::default()
            },
            |ui| {
                captured = Some(app.render_action_row(ui));
            },
        );
        let row =
            captured.expect("render_action_row must be invoked by run_ui's closure");
        assert!(
            !row.revert.enabled(),
            "Revert must render disabled when the dirty buffer is empty"
        );

        app.state.mark_dirty("f0", serde_json::json!(true));

        let mut captured: Option<ActionRow> = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1000.0),
                )),
                time: Some(1.0),
                ..Default::default()
            },
            |ui| {
                captured = Some(app.render_action_row(ui));
            },
        );
        let row =
            captured.expect("render_action_row must be invoked by run_ui's closure");
        assert!(
            row.revert.enabled(),
            "Revert must render enabled once a field is marked dirty"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Enabling "apply on change" sends exactly one `Subscribe{live:true}` on
    /// the transition (a repeat call does not resend), and a subsequent edit
    /// sends its own `ApplyState` without waiting for Submit.
    #[test]
    fn set_live_apply_sends_subscribe_once_then_edit_sends_own_apply_state() {
        use engine_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("live-apply");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2));

        // live_apply now defaults to true (b2-t1); toggle it off first so the
        // subsequent ON transition below is a real transition to observe.
        app.set_live_apply(false);
        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Subscribe must arrive on the OFF transition from the true default");
        match env.body {
            Message::Subscribe(s) => assert!(!s.live, "Subscribe.live must be false"),
            other => panic!("expected Subscribe, got {:?}", other),
        }

        app.set_live_apply(true);
        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Subscribe must arrive on the ON transition");
        match env.body {
            Message::Subscribe(s) => assert!(s.live, "Subscribe.live must be true"),
            other => panic!("expected Subscribe, got {:?}", other),
        }

        // Re-enabling an already-on toggle must not resend (the "once" contract).
        app.set_live_apply(true);
        match frame_rx.recv_timeout(Duration::from_millis(200)) {
            Err(_) => {} // expected: nothing arrived
            Ok(env) => panic!(
                "re-enabling an already-on live_apply must not resend Subscribe, got {:?}",
                env.body
            ),
        }

        app.state.mark_dirty("f0", serde_json::json!(true));
        app.flush_edits();

        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("a live edit must send its own ApplyState");
        match env.body {
            Message::ApplyState(a) => {
                assert_eq!(a.id, SceneKey::new("MainHub"));
                let mut expected = BTreeMap::new();
                expected.insert("f0".to_string(), serde_json::json!(true));
                assert_eq!(
                    a.patch, expected,
                    "the live ApplyState must carry exactly the edited field"
                );
            }
            other => panic!("expected ApplyState, got {:?}", other),
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── b2-t1: auto-apply default + startup Subscribe ──────────────────────

    /// `InspectorApp::new` must default `live_apply` to `true` — the
    /// "Apply on change" checkbox starts checked (spec: auto-apply on by
    /// default).
    #[test]
    fn new_defaults_live_apply_to_true() {
        let (app, _frame_rx, _server_write, path) = stub_app_harness("default-live-apply");

        assert!(
            app.live_apply,
            "InspectorApp::new must default live_apply to true"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Calling `start()` once at startup must result in exactly one framed
    /// `Subscribe{live: true}` arriving at the connected server — proving the
    /// wire message is actually sent, not just that the field defaults to
    /// true.
    #[test]
    fn start_sends_subscribe_live_true_at_startup() {
        use std::time::Duration;

        let (mut app, frame_rx, _server_write, path) = stub_app_harness("start-subscribe");

        app.start();

        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Subscribe must arrive at the server after start()");
        match env.body {
            Message::Subscribe(s) => assert!(
                s.live,
                "start() must send Subscribe{{live:true}} reflecting the true default"
            ),
            other => panic!("expected Subscribe, got {:?}", other),
        }

        let _ = std::fs::remove_file(&path);
    }
}
