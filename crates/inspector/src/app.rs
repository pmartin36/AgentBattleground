//! Inspector UI state and egui app (b5-t2).
//!
//! `SwitcherState` is the pure, egui-free controller layer. All UI-relevant
//! state lives here and is updated through `apply(&Message)`. The egui draw
//! layer (`InspectorApp`) is built on top of this struct; tests only target
//! `SwitcherState`.

use scene_core::ipc::{CatalogEntry, Message};
use scene_core::scene_id::SceneId;
use crate::client::InspectorClient;

/// Pure UI state — the testable controller layer (no egui, no client).
pub struct SwitcherState {
    pub catalog: Vec<CatalogEntry>, // from Hello; empty until connected
    pub active: Option<SceneId>,    // game's current scene
    pub selected: Option<SceneId>,  // dropdown selection
    pub connected: bool,            // true once Hello seen; false on disconnect
    pub should_exit: bool,          // true once set_disconnected() is called
}

impl SwitcherState {
    pub fn new() -> Self {
        SwitcherState {
            catalog: Vec::new(),
            active: None,
            selected: None,
            connected: false,
            should_exit: false,
        }
    }

    /// Reducer: updates state for one inbound message.
    /// Hello  -> catalog + active + selected + connected=true
    /// SceneChanged -> active=Some(id), selected=Some(id)
    /// Ack / Error / SwitchScene -> no-op (M1)
    pub fn apply(&mut self, msg: &Message) {
        match msg {
            Message::Hello(h) => {
                self.catalog = h.scenes.clone();
                self.active = Some(h.active);
                self.selected = Some(h.active);
                self.connected = true;
            }
            Message::SceneChanged(sc) => {
                self.active = Some(sc.id);
                self.selected = Some(sc.id);
            }
            Message::Ack | Message::Error(_) | Message::SwitchScene(_) => {
                // no-op in M1
            }
        }
    }

    /// Drop connected flag (catalog may remain for greyed display).
    /// disconnect => exit
    pub fn set_disconnected(&mut self) {
        self.connected = false;
        self.should_exit = true;
    }

    /// Returns true when the app should exit (game connection was lost).
    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    /// Returns the catalog display name for the current selection, or "-".
    pub fn selected_name(&self) -> &str {
        if let Some(id) = self.selected {
            for entry in &self.catalog {
                if entry.id == id {
                    return &entry.name;
                }
            }
        }
        "-"
    }
}

/// The eframe app: owns state + the live client.
pub struct InspectorApp {
    state: SwitcherState,
    client: InspectorClient,
}

impl InspectorApp {
    pub fn new(client: InspectorClient) -> Self {
        InspectorApp {
            state: SwitcherState::new(),
            client,
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
}

impl eframe::App for InspectorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();

        if self.state.should_exit() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Snapshot copy/owned values before splitting the borrow for the ComboBox.
        let connected = self.state.connected;
        let catalog_empty = self.state.catalog.is_empty();
        let selected_display = self.state.selected_name().to_string();

        ui.horizontal(|ui| {
            // Status dot
            if connected {
                ui.colored_label(egui::Color32::GREEN, "live");
            } else {
                ui.colored_label(egui::Color32::GRAY, "disconnected");
            }

            // Dropdown — borrow disjoint fields of state inside a scoped block so the
            // mutable borrow of `selected` ends before the Go button reads it.
            {
                let cat = &self.state.catalog;
                let sel = &mut self.state.selected;

                ui.add_enabled_ui(connected && !catalog_empty, |ui| {
                    egui::ComboBox::from_label("Scene")
                        .selected_text(selected_display.as_str())
                        .show_ui(ui, |ui| {
                            for e in cat {
                                ui.selectable_value(sel, Some(e.id), &e.name);
                            }
                        });
                });
            } // cat and sel borrows end here

            // Go button
            let go_enabled = self.state.selected.is_some() && connected;
            if ui
                .add_enabled(go_enabled, egui::Button::new("Go"))
                .clicked()
            {
                if let Some(s) = self.state.selected {
                    let _ = self.client.send_switch(s.wire_name(), None);
                }
            }
        });

        ui.ctx().request_repaint();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use scene_core::ipc::{CatalogEntry, Hello, Message, SceneChanged};
    use scene_core::scene_id::SceneId;

    fn four_scene_hello() -> Message {
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
                CatalogEntry {
                    id: SceneId::ArmyEditor,
                    name: "Army Editor".to_string(),
                },
                CatalogEntry {
                    id: SceneId::Leaderboard,
                    name: "Leaderboard".to_string(),
                },
            ],
            active: SceneId::MainHub,
        })
    }

    /// New state: empty catalog, no active/selected, not connected.
    #[test]
    fn new_state_is_empty_and_disconnected() {
        let s = SwitcherState::new();
        assert!(s.catalog.is_empty(), "catalog must start empty");
        assert_eq!(s.active, None, "active must start None");
        assert_eq!(s.selected, None, "selected must start None");
        assert!(!s.connected, "connected must start false");
    }

    /// Hello sets catalog, marks active, pre-selects it, marks connected.
    #[test]
    fn hello_populates_catalog_and_selects_active() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        assert_eq!(s.catalog.len(), 4, "catalog must have 4 scenes after Hello");
        assert_eq!(s.active, Some(SceneId::MainHub), "active must be MainHub");
        assert_eq!(s.selected, Some(SceneId::MainHub), "selected must be pre-set to active");
        assert!(s.connected, "connected must be true after Hello");
    }

    /// SceneChanged updates both active and selected (covers unsolicited switches).
    #[test]
    fn scene_changed_updates_selection() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneId::BattleViewer,
        }));
        assert_eq!(s.active, Some(SceneId::BattleViewer), "active must track SceneChanged");
        assert_eq!(s.selected, Some(SceneId::BattleViewer), "selected must mirror active on SceneChanged");
    }

    /// selected_name returns the catalog entry's name for the selected scene.
    #[test]
    fn selected_name_reflects_catalog() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        // Hello pre-selects MainHub; catalog entry name is "Main Hub".
        assert_eq!(s.selected_name(), "Main Hub", "selected_name must return catalog name");
    }

    /// set_disconnected flips connected to false; catalog may remain.
    #[test]
    fn set_disconnected_clears_connected() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        assert!(s.connected, "must be connected after Hello");
        s.set_disconnected();
        assert!(!s.connected, "must be disconnected after set_disconnected");
    }

    // ── b5-t3: connection lifecycle ───────────────────────────────────────────

    /// should_exit is false on a fresh SwitcherState.
    #[test]
    fn should_exit_false_on_new() {
        let s = SwitcherState::new();
        assert!(!s.should_exit(), "should_exit must be false on new state");
    }

    /// should_exit stays false after a Hello (not a disconnect event).
    #[test]
    fn should_exit_false_after_hello() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        assert!(!s.should_exit(), "should_exit must remain false after Hello");
    }

    /// set_disconnected must flip should_exit to true.
    /// RED: set_disconnected() does not yet set should_exit.
    #[test]
    fn set_disconnected_sets_should_exit() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.set_disconnected();
        assert!(
            s.should_exit(),
            "set_disconnected must set should_exit to true"
        );
    }

    /// When the stub server closes the socket, pump() must propagate the EOF
    /// through the reader-thread channel to state.should_exit() == true.
    /// RED: set_disconnected() does not yet set should_exit, so the loop times out.
    #[test]
    fn pump_sets_should_exit_on_eof() {
        use crate::client::connect;
        use scene_core::ipc::{write_frame, Envelope};
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
}
