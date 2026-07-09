use std::collections::BTreeMap;

use super::fields::{scroll_field_panel, section_frame};
use super::state::SwitcherState;
use crate::client::InspectorClient;

/// Deliberate vertical gap (px) between the scene-selector row and the
/// field-editor section in `InspectorApp::render_body` (b1-t2). Must exceed
/// egui's default `item_spacing.y` (~3.0) so the sections read as visually
/// distinct, not jammed together.
const SECTION_GAP: f32 = 12.0;
const ACTION_ROW_PADDING: f32 = 6.0;

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

    /// Render the scene-selector row and the field-editor section, stacked
    /// and each stretched to full width, directly into `ui` (no
    /// `egui::Panel`). Returns `(selector_response, field_editor_response)`
    /// so callers (and tests) can inspect the sections' rects.
    fn render_body(&mut self, ui: &mut egui::Ui) -> (egui::Response, egui::Response) {
        // Snapshot copy/owned values before splitting the borrow for the ComboBox.
        let connected = self.state.connected;
        let catalog_empty = self.state.catalog.is_empty();
        let selected_display = self.state.selected_name().to_string();

        let selector = section_frame(ui.style())
            .show(ui, |ui| {
                let w = ui.available_width();
                ui.set_min_width(w);

                ui.strong("Scene");
                ui.horizontal(|ui| {
                    // Status dot
                    if connected {
                        ui.colored_label(egui::Color32::GREEN, "live");
                    } else {
                        ui.colored_label(egui::Color32::GRAY, "disconnected");
                    }

                    // Dropdown — borrow disjoint fields of state inside a scoped block so
                    // the mutable borrow of `selected` ends before the Go button reads it.
                    {
                        let cat = &self.state.catalog;
                        let sel = &mut self.state.selected;

                        ui.add_enabled_ui(connected && !catalog_empty, |ui| {
                            egui::ComboBox::from_label("Scene")
                                .selected_text(selected_display.as_str())
                                .show_ui(ui, |ui| {
                                    for e in cat {
                                        ui.selectable_value(sel, Some(e.id.clone()), &e.name);
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
                        if let Some(s) = &self.state.selected {
                            if let Err(e) = self.client.send_switch(s.as_str(), None) {
                                eprintln!("inspector: send_switch failed: {e}");
                            }
                        }
                    }
                });
            })
            .response;

        ui.add_space(SECTION_GAP);

        let editor = section_frame(ui.style())
            .show(ui, |ui| {
                let w = ui.available_width();
                ui.set_min_width(w);

                ui.strong("Fields");
                scroll_field_panel(ui, &mut self.state);
            })
            .response;

        (selector, editor)
    }

    /// Submit / Revert / Apply-on-change row. Returns the Submit and Revert
    /// button responses so tests can assert their enabled state. Both buttons
    /// share one `dirty_empty` gate.
    fn render_action_row(&mut self, ui: &mut egui::Ui) -> ActionRow {
        ui.add_space(ACTION_ROW_PADDING);
        let row = ui
            .horizontal(|ui| {
                let dirty_empty = self.state.dirty_patch().is_empty();

                let submit = ui.add_enabled(!dirty_empty, egui::Button::new("Submit"));
                if submit.clicked() {
                    self.submit();
                }

                let revert = ui.add_enabled(!dirty_empty, egui::Button::new("Revert"));
                if revert.clicked() {
                    self.revert();
                }

                let mut live_apply = self.live_apply;
                if ui.checkbox(&mut live_apply, "Apply on change").changed() {
                    self.set_live_apply(live_apply);
                }

                ActionRow { submit, revert }
            })
            .inner;
        ui.add_space(ACTION_ROW_PADDING);
        row
    }

    /// Production entry point: docks the action row and the rest of the
    /// body together so the action row stays reachable regardless of
    /// field-list content height (b1-t1). The action row is pinned to a
    /// bottom panel (shown first, per egui's panel-ordering rule) and the
    /// scene-selector + field-editor body occupies the remaining central
    /// panel, which naturally caps the field list's scroll area to the
    /// remaining middle space.
    fn render(&mut self, ui: &mut egui::Ui) -> ActionRow {
        let action = egui::Panel::bottom("inspector_action_row")
            .show(ui, |ui| self.render_action_row(ui))
            .inner;
        egui::CentralPanel::default().show(ui, |ui| {
            let _ = self.render_body(ui);
        });
        action
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
mod tests {
    use super::*;
    use crate::app::fields::FieldResponses;
    use crate::app::test_fixtures::*;
    use engine_core::inspect::FieldTag;
    use engine_core::ipc::{Message, StateSnapshot};
    use engine_core::SceneKey;
    use std::rc::Rc;

    /// b1-t3: flattens every `Shape::Rect` (recursing into `Shape::Vec`) that
    /// has a visible stroke (`stroke.width > 0.0`) into its `(rect, stroke)`
    /// pair. Used to find the frame chrome painted by `egui::Frame::show`.
    fn collect_stroked_rects(output: &egui::FullOutput) -> Vec<(egui::Rect, egui::Stroke)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Stroke)>) {
            match shape {
                egui::Shape::Rect(r) if r.stroke.width > 0.0 => {
                    out.push((r.rect, r.stroke));
                }
                egui::Shape::Vec(nested) => {
                    for s in nested {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// b1-t3: finds the stroked rect (from `collect_stroked_rects`) whose
    /// bounds nearly coincide with `target` (an `egui::Response::rect`),
    /// within a small float-precision epsilon.
    fn find_matching_stroke(
        stroked: &[(egui::Rect, egui::Stroke)],
        target: egui::Rect,
    ) -> Option<egui::Stroke> {
        const EPS: f32 = 1.0;
        stroked
            .iter()
            .find(|(rect, _)| {
                (rect.min - target.min).length() < EPS && (rect.max - target.max).length() < EPS
            })
            .map(|(_, stroke)| *stroke)
    }
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

    /// `render_body` must lay out the scene-selector row and the
    /// field-editor section stacked (selector above editor), each stretched
    /// to (most of) the viewport width — not a narrow docked side-panel
    /// strip. This is the exact user-reported bug: `egui::Panel::right`
    /// leaves the selector row confined to a narrow top-left strip.
    #[test]
    fn body_sections_are_full_width_and_stacked() {
        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("layout");
        app.state.apply(&four_scene_hello());

        let ctx = egui::Context::default();
        let viewport_w = 1000.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(viewport_w, 1000.0),
            )),
            time: Some(0.0),
            ..Default::default()
        };

        let mut captured: Option<(egui::Response, egui::Response)> = None;
        let _ = ctx.run_ui(input, |ui| {
            captured = Some(app.render_body(ui));
        });
        let (selector, editor) =
            captured.expect("render_body must be invoked by run_ui's closure");

        let min_w = 0.9 * viewport_w;
        assert!(
            selector.rect.width() >= min_w,
            "selector row must span (most of) the full viewport width, got {} of {}",
            selector.rect.width(),
            viewport_w
        );
        assert!(
            editor.rect.width() >= min_w,
            "field-editor section must span (most of) the full viewport width, got {} of {}",
            editor.rect.width(),
            viewport_w
        );
        assert!(
            editor.rect.min.y >= selector.rect.max.y,
            "field-editor section must be stacked below the selector row, not beside it \
             (selector bottom {}, editor top {})",
            selector.rect.max.y,
            editor.rect.min.y
        );

        let _ = std::fs::remove_file(&path);
    }

    // ── b1-t2: deliberate vertical gap between the two sections ─────────────

    /// The vertical gap between the selector row's bottom and the
    /// field-editor section's top must be bounded below by `SECTION_GAP`, a
    /// real, deliberate value — not egui's ~3px default `item_spacing.y` and
    /// not a token 0.
    #[test]
    fn body_sections_have_deliberate_vertical_gap() {
        const {
            assert!(
                SECTION_GAP >= 8.0,
                "SECTION_GAP must be a deliberate value clearly above egui's \
                 default item_spacing.y (~3.0)"
            );
        }

        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("gap");
        app.state.apply(&four_scene_hello());

        let ctx = egui::Context::default();
        let viewport_w = 1000.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(viewport_w, 1000.0),
            )),
            time: Some(0.0),
            ..Default::default()
        };

        let mut captured: Option<(egui::Response, egui::Response)> = None;
        let _ = ctx.run_ui(input, |ui| {
            captured = Some(app.render_body(ui));
        });
        let (selector, editor) =
            captured.expect("render_body must be invoked by run_ui's closure");

        let gap = editor.rect.min.y - selector.rect.max.y;
        assert!(
            gap >= SECTION_GAP,
            "vertical gap between selector and field-editor sections must be \
             >= SECTION_GAP ({SECTION_GAP}), got {gap}"
        );

        let _ = std::fs::remove_file(&path);
    }

    // ── b1-t3: consistent framing between the two sections ──────────────────

    /// Both top-level sections must be enclosed by a visibly stroked frame,
    /// and those two frames must be identical (same stroke width and color)
    /// — a sameness contract, never a specific pixel value (spec explicitly
    /// defers exact styling to implementer judgment).
    #[test]
    fn both_sections_share_identical_frame_styling() {
        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("frame-style");
        app.state.apply(&four_scene_hello());

        let ctx = egui::Context::default();
        let viewport_w = 1000.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(viewport_w, 1000.0),
            )),
            time: Some(0.0),
            ..Default::default()
        };

        let mut captured: Option<(egui::Response, egui::Response)> = None;
        let output = ctx.run_ui(input, |ui| {
            captured = Some(app.render_body(ui));
        });
        let (selector, editor) =
            captured.expect("render_body must be invoked by run_ui's closure");

        let stroked = collect_stroked_rects(&output);

        let selector_stroke = find_matching_stroke(&stroked, selector.rect).expect(
            "the scene-selector section must be enclosed by a visibly stroked frame",
        );
        let editor_stroke = find_matching_stroke(&stroked, editor.rect).expect(
            "the field-editor section must be enclosed by a visibly stroked frame",
        );

        assert!(
            selector_stroke.width > 0.0 && editor_stroke.width > 0.0,
            "both sections' frames must have a non-zero stroke width; got selector={:?}, editor={:?}",
            selector_stroke,
            editor_stroke
        );
        assert_eq!(
            selector_stroke.width, editor_stroke.width,
            "both sections must share the same frame stroke width"
        );
        assert_eq!(
            selector_stroke.color, editor_stroke.color,
            "both sections must share the same frame stroke color"
        );

        let _ = std::fs::remove_file(&path);
    }

    // ── b1-t4: scroll container around the field-editor content ────────────

    /// A schema at least as large as `BattleViewer`'s (a 12-element list of
    /// multi-field structs), rendered into a height-constrained viewport,
    /// must produce a laid-out content height that exceeds the visible
    /// scroll viewport — proving `scroll_field_panel` puts a real
    /// `egui::ScrollArea` in the render path (a plain `Ui` would just clip
    /// silently, with `content_size ≈ inner_rect`).
    #[test]
    fn field_panel_scrolls_when_content_exceeds_viewport() {
        let element = struct_schema(
            "piece",
            vec![
                leaf("a", FieldTag::Int),
                leaf("b", FieldTag::Float),
                leaf("c", FieldTag::Bool),
            ],
        );
        let mut state = SwitcherState::new();
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![list_schema("pieces", element)])));

        let pieces: Vec<serde_json::Value> = (0..12)
            .map(|_| serde_json::json!({"a": 0, "b": 0.0, "c": false}))
            .collect();
        state.panel_snapshot = serde_json::json!({ "pieces": pieces });

        let ctx = egui::Context::default();
        let viewport = egui::vec2(400.0, 150.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, viewport)),
            time: Some(0.0),
            ..Default::default()
        };

        let mut captured: Option<egui::scroll_area::ScrollAreaOutput<FieldResponses>> = None;
        let _ = ctx.run_ui(input, |ui| {
            captured = Some(scroll_field_panel(ui, &mut state));
        });
        let out = captured.expect("scroll_field_panel must be invoked by run_ui's closure");

        assert!(
            out.content_size.y > viewport.y,
            "a 12-element list's full content height ({}) must exceed the \
             constrained viewport height ({})",
            out.content_size.y,
            viewport.y
        );
        assert!(
            out.content_size.y > out.inner_rect.height(),
            "content must overflow the clipped visible viewport, proving a real \
             egui::ScrollArea is in the render path (got content_size.y={}, \
             inner_rect.height()={})",
            out.content_size.y,
            out.inner_rect.height()
        );
        assert!(
            !out.inner.is_empty(),
            "scroll_field_panel must still return the wrapped panel's field Responses"
        );
    }

    // ── b1-t1: action row stays reachable when the field list overflows ────

    /// The Submit/Revert action row must be reachable (its rendered
    /// `Response` rects must lie entirely within the visible screen rect)
    /// through the real production wiring (`app.render`, not `render_body`
    /// alone), even when a 12-element list-of-structs field panel (at least
    /// as large as `BattleViewer`'s) overflows a deliberately small
    /// viewport. Regression guard for the unbounded-`ScrollArea` bug that
    /// could push the action row below the visible window.
    #[test]
    fn action_row_stays_within_viewport_when_field_list_overflows() {
        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("action-row-pin");
        app.state.apply(&four_scene_hello());

        let element = struct_schema(
            "piece",
            vec![
                leaf("a", FieldTag::Int),
                leaf("b", FieldTag::Float),
                leaf("c", FieldTag::Bool),
            ],
        );
        app.state.panel_schema = Some(Rc::new(struct_schema("Root", vec![list_schema("pieces", element)])));
        let pieces: Vec<serde_json::Value> = (0..12)
            .map(|_| serde_json::json!({"a": 0, "b": 0.0, "c": false}))
            .collect();
        app.state.panel_snapshot = serde_json::json!({ "pieces": pieces });

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let input = egui::RawInput {
            screen_rect: Some(screen_rect),
            time: Some(0.0),
            ..Default::default()
        };

        // Bottom-panel height negotiation needs one settle frame (egui
        // remembers last frame's measured content height when placing this
        // frame's panel) before the allocated rect reflects the padding
        // added by ACTION_ROW_PADDING — mirrors the settle-frame pattern
        // used elsewhere in this file for popups.
        let mut captured: Option<ActionRow> = None;
        for t in [0.0, 1.0] {
            let mut input = input.clone();
            input.time = Some(t);
            let _ = ctx.run_ui(input, |ui| {
                captured = Some(app.render(ui));
            });
        }
        let row = captured.expect("app.render must be invoked by run_ui's closure");

        assert!(
            row.submit.rect.min.y >= screen_rect.min.y && row.submit.rect.max.y <= screen_rect.max.y,
            "Submit button rect ({:?}) must lie entirely within the screen rect ({:?}) \
             even when the field list overflows",
            row.submit.rect,
            screen_rect
        );
        assert!(
            row.revert.rect.min.y >= screen_rect.min.y && row.revert.rect.max.y <= screen_rect.max.y,
            "Revert button rect ({:?}) must lie entirely within the screen rect ({:?}) \
             even when the field list overflows",
            row.revert.rect,
            screen_rect
        );

        let _ = std::fs::remove_file(&path);
    }

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
