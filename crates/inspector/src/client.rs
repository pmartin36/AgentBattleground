//! IPC client for the inspector — connects to the game's inspect Unix socket,
//! receives decoded `Message` bodies on a channel, and sends `SwitchScene` commands.

// Public API items (`InspectorClient`, `connect`, `reader_thread`, `send_switch`)
// are consumed by b5-t2 (`main.rs`). Suppress dead_code until that wiring lands.
#![allow(dead_code)]

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};

use scene_core::ipc::{read_frame, write_frame, Envelope, IpcError, Message, SwitchScene};

/// Live connection to the game's inspect socket. Owns the write half and the
/// receiver of decoded inbound messages. Dropping it closes the write half;
/// the reader thread ends on the next failed read.
pub struct InspectorClient {
    pub write: UnixStream,
    pub seq: u64,
    pub incoming: Receiver<Message>,
}

/// Connect to the game's per-pid Unix socket and start the reader thread.
pub fn connect(path: &Path) -> std::io::Result<InspectorClient> {
    let stream = UnixStream::connect(path)?;
    let mut read_half = stream.try_clone()?;
    let write_half = stream;

    let (tx, rx) = mpsc::channel::<Message>();

    std::thread::spawn(move || {
        reader_thread(&mut read_half, tx);
    });

    Ok(InspectorClient {
        write: write_half,
        seq: 0,
        incoming: rx,
    })
}

/// Background reader thread: loops `read_frame`, forwards each decoded
/// `Message` body to the UI via the mpsc sender. Exits on any read error or
/// EOF (which drops the Sender, closing the channel).
fn reader_thread(stream: &mut UnixStream, tx: mpsc::Sender<Message>) {
    loop {
        match read_frame(stream) {
            Ok(env) => {
                if tx.send(env.body).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

impl InspectorClient {
    /// Frame + write a `SwitchScene{target, params}`, stamping the next seq.
    pub fn send_switch(
        &mut self,
        target: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), IpcError> {
        let body = Message::SwitchScene(SwitchScene {
            target: target.to_string(),
            params,
        });
        let env = Envelope::new(self.seq, None, body);
        write_frame(&mut self.write, &env)?;
        self.seq += 1;
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use scene_core::ipc::{
        write_frame, read_frame, CatalogEntry, Envelope, Hello, Message,
    };
    use scene_core::scene_id::SceneId;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::time::Duration;

    // ── helpers ────────────────────────────────────────────────────────────────

    /// Bind a temporary Unix socket and return the listener + path.
    /// `tag` is a short unique label (e.g. the test name suffix) so concurrent
    /// tests don't collide on the same socket path.
    fn tmp_listener(tag: &str) -> (UnixListener, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "inspector-client-test-{}-{}.sock",
            std::process::id(),
            tag,
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind tmp socket");
        (listener, path)
    }

    fn two_scene_hello() -> Message {
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

    // ── connect_then_receives_hello_catalog ────────────────────────────────────

    /// The client connects to a stub server that writes a framed Hello; the
    /// decoded message must appear on `incoming` with the correct catalog.
    #[test]
    fn connect_then_receives_hello_catalog() {
        let (listener, path) = tmp_listener("hello");

        // Stub server: accept one connection, write a Hello, then idle.
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let env = Envelope::new(0, None, two_scene_hello());
            write_frame(&mut stream, &env).expect("write_frame Hello");
            // Keep the connection open so the reader thread doesn't see EOF yet.
            std::thread::sleep(Duration::from_secs(5));
        });

        let client = connect(&path).expect("connect must succeed");

        let msg = client
            .incoming
            .recv_timeout(Duration::from_secs(2))
            .expect("Hello must arrive on incoming");

        match msg {
            Message::Hello(h) => {
                assert_eq!(h.scenes.len(), 2, "catalog must have 2 scenes");
                assert_eq!(h.scenes[0].id, SceneId::MainHub);
                assert_eq!(h.scenes[1].id, SceneId::BattleViewer);
                assert_eq!(h.active, SceneId::MainHub);
            }
            other => panic!("expected Hello, got {:?}", other),
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── send_switch_arrives_framed_at_server ───────────────────────────────────

    /// `send_switch("BattleViewer", None)` must land at the stub server as a
    /// framed SwitchScene with the correct target and no params.
    #[test]
    fn send_switch_arrives_framed_at_server() {
        let (listener, path) = tmp_listener("switch");

        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        // Stub server: accept, read one frame, forward decoded envelope.
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let env = read_frame(&mut stream).expect("read_frame SwitchScene");
            tx.send(env).ok();
        });

        let mut client = connect(&path).expect("connect must succeed");
        client
            .send_switch("BattleViewer", None)
            .expect("send_switch must succeed");

        let env = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("SwitchScene frame must arrive at stub");

        match env.body {
            Message::SwitchScene(ss) => {
                assert_eq!(ss.target, "BattleViewer");
                assert!(ss.params.is_none(), "params must be None");
            }
            other => panic!("expected SwitchScene, got {:?}", other),
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── send_switch_seq_is_monotonic ───────────────────────────────────────────

    /// Two successive `send_switch` calls must stamp seq 0 then seq 1.
    #[test]
    fn send_switch_seq_is_monotonic() {
        let (listener, path) = tmp_listener("seq");

        let (tx, rx) = std::sync::mpsc::sync_channel(2);

        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            for _ in 0..2 {
                if let Ok(env) = read_frame(&mut stream) {
                    tx.send(env.seq).ok();
                }
            }
        });

        let mut client = connect(&path).expect("connect");
        client.send_switch("MainHub", None).expect("send 0");
        client.send_switch("BattleViewer", None).expect("send 1");

        let seq0 = rx.recv_timeout(Duration::from_secs(2)).expect("seq 0");
        let seq1 = rx.recv_timeout(Duration::from_secs(2)).expect("seq 1");

        assert_eq!(seq0, 0, "first send_switch must stamp seq=0");
        assert_eq!(seq1, 1, "second send_switch must stamp seq=1");

        let _ = std::fs::remove_file(&path);
    }

    // ── socket_close_disconnects_incoming ──────────────────────────────────────

    /// When the stub server closes the connection, the reader thread must end
    /// and `incoming.recv()` must return `Err` (disconnected).
    #[test]
    fn socket_close_disconnects_incoming() {
        let (listener, path) = tmp_listener("eof");

        // Stub server: accept then immediately drop the connection.
        std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("accept");
            // _stream drops here, closing the socket.
        });

        let client = connect(&path).expect("connect");

        // The reader thread should detect EOF and close the sender side.
        // Give it up to 2 s to notice.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match client.incoming.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break, // expected
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "incoming channel must be disconnected within 2 s of socket close"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(msg) => {
                    panic!("unexpected message on closed connection: {:?}", msg)
                }
            }
        }

        let _ = std::fs::remove_file(&path);
    }
}
