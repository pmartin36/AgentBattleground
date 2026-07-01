use std::sync::mpsc::Sender;
use std::time::Duration;

use ratatui::Frame;
use scene_core::inspect::{FieldSchema, FieldTag};
use scene_core::ipc::{CatalogEntry, Hello, Message, SceneChanged};
use scene_core::scene_id::SceneId;
use serde_json::Value as JsonValue;

/// Placeholder schema for a catalog entry until b5-t3 wires
/// `registry::schema_for(id)` in with the real per-scene schema.
fn stub_schema(id: SceneId) -> FieldSchema {
    FieldSchema {
        name: id.display_name().to_string(),
        label: None,
        tag: FieldTag::Struct,
        readonly: false,
        hidden: false,
        range: None,
        children: vec![],
        variants: vec![],
    }
}

use crate::ipc_server::Event;
use crate::registry;
use crate::scene::{EngineCtx, InputEvent, Scene, Transition};

/// Inbound command drained at top-of-frame.
/// `target` is already resolved to a `SceneId` — the wire-string
/// → SceneId resolution (and Error{UnknownScene}) happens at the IPC boundary in b4,
/// NOT in the manager.
pub enum Command {
    /// A new inspector client has connected; the loop must push a Hello.
    ClientConnected,
    SwitchScene {
        target: SceneId,
        params: Option<JsonValue>,
    },
}

pub struct SceneManager {
    active: Box<dyn Scene>,
    pending: Option<Transition>,
    /// True once a debug command has claimed `pending` this frame; gameplay
    /// calls to `set_gameplay_transition` are no-ops while this is set.
    pending_is_debug: bool,
    ctx: EngineCtx,
}

impl SceneManager {
    /// Construct `boot` via the registry and call `enter(None)`.
    pub fn new(boot: SceneId) -> Self {
        Self::with_scene_and_params(registry::construct(boot), None)
    }

    /// Boot an already-constructed scene (off-catalog or example-supplied)
    /// with `enter(&mut ctx, None)`. Delegates to `with_scene_and_params`.
    pub fn with_scene(boot: Box<dyn Scene>) -> Self {
        Self::with_scene_and_params(boot, None)
    }

    /// Boot `boot` and call `enter(&mut ctx, params)` once with the exact
    /// `params` given, setting it as the active scene.
    pub fn with_scene_and_params(
        mut boot: Box<dyn Scene>,
        params: Option<serde_json::Value>,
    ) -> Self {
        let mut ctx = EngineCtx;
        boot.enter(&mut ctx, params);
        SceneManager {
            active: boot,
            pending: None,
            pending_is_debug: false,
            ctx,
        }
    }

    /// Return the id of the currently active scene.
    pub fn active_id(&self) -> SceneId {
        self.active.id()
    }

    /// Debug command: set `pending`, overriding any prior transition (debug always wins).
    pub fn set_debug_transition(&mut self, t: Transition) {
        self.pending = Some(t);
        self.pending_is_debug = true;
    }

    /// Gameplay (input/update) path: set `pending` only if a debug command has not
    /// already claimed it this frame (`if !self.pending_is_debug`).
    pub fn set_gameplay_transition(&mut self, t: Transition) {
        if !self.pending_is_debug {
            self.pending = Some(t);
        }
    }

    /// Route an input event to the active scene.
    pub fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        self.active.handle_input(ev)
    }

    /// Advance the active scene by `dt`.
    pub fn update(&mut self, dt: Duration) -> Option<Transition> {
        self.active.update(&mut self.ctx, dt)
    }

    /// Apply `pending` if any: `exit` → `construct` → `enter(params)` → swap active.
    /// Clears `pending` and resets `pending_is_debug`.
    /// Returns the new active id if a switch occurred; `None` otherwise.
    /// (b4-t2 emits `SceneChanged` off this return value.)
    pub fn process_pending(&mut self) -> Option<SceneId> {
        let transition = self.pending.take()?;
        self.pending_is_debug = false;
        self.active.exit(&mut self.ctx);
        let mut new_scene = registry::construct(transition.target);
        new_scene.enter(&mut self.ctx, transition.params);
        let new_id = new_scene.id();
        self.active = new_scene;
        Some(new_id)
    }

    /// Draw the active scene over the full frame area.
    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        self.active.render(frame, area);
    }

    /// Returns the M1 Hello catalog (four implemented scenes in '1'–'4' order,
    /// names from `display_name()`, active = current scene id).
    /// b4-t2 implements this by iterating `scenes::scene_for_digit('1'..='4')`.
    pub fn hello(&self) -> Hello {
        let scenes = ['1', '2', '3', '4']
            .iter()
            .filter_map(|&c| crate::scenes::scene_for_digit(c))
            .map(|id| CatalogEntry {
                id,
                name: id.display_name().to_string(),
                schema: stub_schema(id),
            })
            .collect();
        Hello {
            scenes,
            active: self.active_id(),
        }
    }

    /// Handle one inbound IPC command from the bridge.
    ///
    /// - `ClientConnected` → push `Event { body: Message::Hello(self.hello()), reply_to: None }`.
    /// - `SwitchScene { target, params }` → `set_debug_transition`; no event pushed here
    ///   (SceneChanged is pushed by `process_pending_notify` after the swap).
    pub fn apply_command(&mut self, cmd: Command, events: &Sender<Event>) {
        match cmd {
            Command::ClientConnected => {
                let _ = events.send(Event {
                    body: Message::Hello(self.hello()),
                    reply_to: None,
                });
            }
            Command::SwitchScene { target, params } => {
                self.set_debug_transition(Transition { target, params });
            }
        }
    }

    /// Notifying wrapper around `process_pending`: if a transition is pending,
    /// applies it (via `process_pending`) and pushes
    /// `Event { body: Message::SceneChanged { id }, reply_to: None }` on `events`.
    /// Returns `Some(id)` when a switch occurred, `None` otherwise.
    pub fn process_pending_notify(&mut self, events: &Sender<Event>) -> Option<SceneId> {
        let id = self.process_pending()?;
        let _ = events.send(Event {
            body: Message::SceneChanged(SceneChanged {
                id,
                snapshot: JsonValue::Null,
            }),
            reply_to: None,
        });
        Some(id)
    }

    /// Route one key event. Returns `true` if the app should quit (`q` or Ctrl-C).
    /// Keys `1`–`4` set a gameplay transition to the corresponding scene.
    /// All other keys are forwarded to the active scene via `handle_input`.
    pub fn route_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Quit keys checked first; active scene is left unchanged.
        if key.code == KeyCode::Char('q') {
            return true;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }

        // Digit keys 1–4: global scene switch via the gameplay path.
        if let KeyCode::Char(c) = key.code {
            if let Some(target) = crate::scenes::scene_for_digit(c) {
                self.set_gameplay_transition(Transition { target, params: None });
                return false;
            }
        }

        // All other keys: forward to the active scene.
        if let Some(t) = self.handle_input(InputEvent::Key(key)) {
            self.set_gameplay_transition(t);
        }
        false
    }
}

#[cfg(test)]
#[allow(unused_mut)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;
    use scene_core::ipc::Message;
    use scene_core::scene_id::SceneId;

    use crate::ipc_server::Event;
    use crate::scenes::MainHub;

    fn key(c: char, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), mods)
    }

    // -------------------------------------------------------- with_scene (b3-t1)

    /// `SceneManager::with_scene` boots a non-registry (example-supplied) scene:
    ///   - sets `active_id()` to that scene's own id
    ///   - calls `enter()` exactly once before returning
    ///
    /// This is the seam that `game::run(Box<dyn Scene>)` and b3-t2's render
    /// example depend on.
    #[test]
    fn with_scene_boots_arbitrary_scene_and_calls_enter() {
        use std::sync::{Arc, Mutex};
        use ratatui::layout::Rect;
        use serde_json::Value as JsonValue;

        struct TestScene {
            entered: Arc<Mutex<bool>>,
        }

        impl Scene for TestScene {
            fn id(&self) -> SceneId {
                SceneId::Leaderboard
            }
            fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {
                *self.entered.lock().unwrap() = true;
            }
            fn update(
                &mut self,
                _ctx: &mut EngineCtx,
                _dt: std::time::Duration,
            ) -> Option<Transition> {
                None
            }
            fn render(&self, _frame: &mut ratatui::Frame, _area: Rect) {}
            fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
                None
            }
            fn exit(&mut self, _ctx: &mut EngineCtx) {}
        }

        let entered = Arc::new(Mutex::new(false));
        let scene = TestScene { entered: Arc::clone(&entered) };
        let mgr = SceneManager::with_scene(Box::new(scene));

        assert_eq!(
            mgr.active_id(),
            SceneId::Leaderboard,
            "with_scene must set active_id to the provided scene's id"
        );
        assert!(
            *entered.lock().unwrap(),
            "with_scene must call enter() on the scene before returning"
        );
    }

    // -------------------------------------------------- with_scene_and_params (b1-t1)

    /// `SceneManager::with_scene_and_params` delivers the exact `Some(json!(...))`
    /// value into the scene's `enter(_ctx, params)` — the primitive every other
    /// bucket in this feature threads params through.
    #[test]
    fn with_scene_and_params_delivers_exact_params_to_enter() {
        use std::sync::{Arc, Mutex};
        use ratatui::layout::Rect;
        use serde_json::{json, Value as JsonValue};

        struct ParamsCapturingScene {
            params: Arc<Mutex<Option<JsonValue>>>,
        }

        impl Scene for ParamsCapturingScene {
            fn id(&self) -> SceneId {
                SceneId::Leaderboard
            }
            fn enter(&mut self, _ctx: &mut EngineCtx, params: Option<JsonValue>) {
                *self.params.lock().unwrap() = params;
            }
            fn update(
                &mut self,
                _ctx: &mut EngineCtx,
                _dt: std::time::Duration,
            ) -> Option<Transition> {
                None
            }
            fn render(&self, _frame: &mut ratatui::Frame, _area: Rect) {}
            fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
                None
            }
            fn exit(&mut self, _ctx: &mut EngineCtx) {}
        }

        let captured = Arc::new(Mutex::new(None));
        let scene = ParamsCapturingScene { params: Arc::clone(&captured) };
        let expected = json!({"k": 1});
        let mgr = SceneManager::with_scene_and_params(Box::new(scene), Some(expected.clone()));

        assert_eq!(
            mgr.active_id(),
            SceneId::Leaderboard,
            "with_scene_and_params must set active_id to the provided scene's id"
        );
        assert_eq!(
            *captured.lock().unwrap(),
            Some(expected),
            "with_scene_and_params must deliver the exact params value into enter()"
        );
    }

    /// `with_scene_and_params(scene, None)` delivers `None` to `enter()` —
    /// pinning the delegation contract `with_scene`/`new` rely on.
    #[test]
    fn with_scene_and_params_none_delivers_none_to_enter() {
        use std::sync::{Arc, Mutex};
        use ratatui::layout::Rect;
        use serde_json::{json, Value as JsonValue};

        struct ParamsCapturingScene {
            params: Arc<Mutex<Option<JsonValue>>>,
        }

        impl Scene for ParamsCapturingScene {
            fn id(&self) -> SceneId {
                SceneId::Leaderboard
            }
            fn enter(&mut self, _ctx: &mut EngineCtx, params: Option<JsonValue>) {
                *self.params.lock().unwrap() = params;
            }
            fn update(
                &mut self,
                _ctx: &mut EngineCtx,
                _dt: std::time::Duration,
            ) -> Option<Transition> {
                None
            }
            fn render(&self, _frame: &mut ratatui::Frame, _area: Rect) {}
            fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
                None
            }
            fn exit(&mut self, _ctx: &mut EngineCtx) {}
        }

        let captured = Arc::new(Mutex::new(Some(json!({"stale": true}))));
        let scene = ParamsCapturingScene { params: Arc::clone(&captured) };
        let _mgr = SceneManager::with_scene_and_params(Box::new(scene), None);

        assert_eq!(
            *captured.lock().unwrap(),
            None,
            "with_scene_and_params(scene, None) must deliver None to enter()"
        );
    }

    // ------------------------------------------------------------------ boot

    /// `SceneManager::new(MainHub)` boots with `active_id() == MainHub` and
    /// renders cell (0,0) as the braille glyph ⣿ in MainHub's declared blue.
    /// This doubles as the BEHAVIORAL render evidence (TestBackend, no real TTY).
    #[test]
    fn boot_is_main_hub_and_renders_blue() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        assert_eq!(
            manager.active_id(),
            SceneId::MainHub,
            "boot scene must be MainHub"
        );

        let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
        terminal.draw(|f| manager.render(f)).unwrap();
        let buf = terminal.backend().buffer().clone();

        let cell = buf.cell((0, 0)).expect("cell (0,0) must exist in a 40×10 buffer");
        assert_eq!(
            cell.symbol(),
            "⣿",
            "boot render must fill cell (0,0) with braille glyph ⣿"
        );
        assert_eq!(
            cell.fg,
            Color::Rgb(MainHub::COLOR.r, MainHub::COLOR.g, MainHub::COLOR.b),
            "boot render fg must match MainHub::COLOR (blue 0x1e3ac8)"
        );
    }

    // -------------------------------------------------------- transition precedence

    /// Debug transition always overrides a gameplay transition set first in the same tick.
    #[test]
    fn debug_transition_overrides_gameplay_gameplay_first() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        manager.set_gameplay_transition(Transition {
            target: SceneId::BattleViewer,
            params: None,
        });
        manager.set_debug_transition(Transition {
            target: SceneId::ArmyEditor,
            params: None,
        });
        manager.process_pending();
        assert_eq!(
            manager.active_id(),
            SceneId::ArmyEditor,
            "debug must override gameplay when gameplay transition was set first"
        );
    }

    /// Debug transition always overrides a gameplay transition set second in the same tick.
    #[test]
    fn debug_transition_overrides_gameplay_debug_first() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        manager.set_debug_transition(Transition {
            target: SceneId::ArmyEditor,
            params: None,
        });
        manager.set_gameplay_transition(Transition {
            target: SceneId::BattleViewer,
            params: None,
        });
        manager.process_pending();
        assert_eq!(
            manager.active_id(),
            SceneId::ArmyEditor,
            "debug must override gameplay when debug transition was set first"
        );
    }

    // -------------------------------------------------------- transition swap

    /// A queued gameplay transition is applied by `process_pending`: `active_id`
    /// changes and the return value reports the new scene id.
    #[test]
    fn queued_gameplay_transition_swaps_active() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        manager.set_gameplay_transition(Transition {
            target: SceneId::Leaderboard,
            params: None,
        });
        let result = manager.process_pending();
        assert_eq!(
            result,
            Some(SceneId::Leaderboard),
            "process_pending must return Some(Leaderboard) after a gameplay transition"
        );
        assert_eq!(
            manager.active_id(),
            SceneId::Leaderboard,
            "active must be Leaderboard after the transition"
        );
    }

    /// `process_pending` with nothing queued returns `None` and leaves `active_id` unchanged.
    #[test]
    fn process_pending_noop_when_empty() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let result = manager.process_pending();
        assert_eq!(
            result,
            None,
            "process_pending with no pending transition must return None"
        );
        assert_eq!(
            manager.active_id(),
            SceneId::MainHub,
            "active must remain MainHub when nothing is pending"
        );
    }

    // -------------------------------------------------------- route_key: digit switch (DELIVERABLE)

    /// Pressing key '2' schedules a gameplay transition; after process_pending
    /// the active scene is BattleViewer. route_key must return false (not quit).
    #[test]
    fn route_key_digit_2_switches_to_battle_viewer() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let quit = manager.route_key(key('2', KeyModifiers::NONE));
        assert!(!quit, "route_key('2') must return false (not a quit key)");
        manager.process_pending();
        assert_eq!(
            manager.active_id(),
            SceneId::BattleViewer,
            "after key '2' + process_pending, active must be BattleViewer"
        );
    }

    // -------------------------------------------------------- route_key: quit keys

    /// Pressing 'q' returns true (quit) and leaves active unchanged.
    #[test]
    fn route_key_q_returns_quit_active_unchanged() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let quit = manager.route_key(key('q', KeyModifiers::NONE));
        assert!(quit, "route_key('q') must return true (quit signal)");
        assert_eq!(
            manager.active_id(),
            SceneId::MainHub,
            "active must remain MainHub after 'q'"
        );
    }

    /// Ctrl-C returns true (quit).
    #[test]
    fn route_key_ctrl_c_returns_quit() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let quit = manager.route_key(key('c', KeyModifiers::CONTROL));
        assert!(quit, "route_key(Ctrl-C) must return true (quit signal)");
    }

    // -------------------------------------------------------- route_key: global (not per-scene)

    /// Key '1' switches to MainHub even when the current scene is BattleViewer.
    /// Proves the binding is global, not delegated to the active scene.
    #[test]
    fn route_key_digit_1_is_global_from_battle_viewer() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        // Switch to BattleViewer first.
        manager.set_gameplay_transition(Transition {
            target: SceneId::BattleViewer,
            params: None,
        });
        manager.process_pending();
        assert_eq!(manager.active_id(), SceneId::BattleViewer);

        // Now press '1' — must switch back to MainHub from BattleViewer.
        let quit = manager.route_key(key('1', KeyModifiers::NONE));
        assert!(!quit, "route_key('1') must return false");
        manager.process_pending();
        assert_eq!(
            manager.active_id(),
            SceneId::MainHub,
            "key '1' from BattleViewer must switch to MainHub (global keybind)"
        );
    }

    // -------------------------------------------------------- route_key: debug overrides digit

    /// A debug transition wins over a same-tick digit key (gameplay path).
    #[test]
    fn route_key_debug_transition_overrides_digit_gameplay() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        // Debug claims pending first.
        manager.set_debug_transition(Transition {
            target: SceneId::ArmyEditor,
            params: None,
        });
        // Digit '2' tries the gameplay path; must be blocked by pending_is_debug.
        manager.route_key(key('2', KeyModifiers::NONE));
        manager.process_pending();
        assert_eq!(
            manager.active_id(),
            SceneId::ArmyEditor,
            "debug transition must override the digit gameplay transition"
        );
    }

    // ═══════════════════════════════════════ b4-t2: IPC protocol methods ═══════

    /// `hello()` returns exactly four M1 scenes in digit-key order
    /// (MainHub, BattleViewer, ArmyEditor, Leaderboard), each name matching
    /// `display_name()`, with `active` == current active id (MainHub at boot).
    #[test]
    fn hello_lists_four_scenes_active_main_hub() {
        let manager = SceneManager::new(SceneId::MainHub);
        let hello = manager.hello();
        assert_eq!(hello.scenes.len(), 4, "hello must list exactly four M1 scenes");
        assert_eq!(hello.active, SceneId::MainHub, "hello.active must be MainHub at boot");
        let ids: Vec<SceneId> = hello.scenes.iter().map(|e| e.id).collect();
        for expected in [
            SceneId::MainHub,
            SceneId::BattleViewer,
            SceneId::ArmyEditor,
            SceneId::Leaderboard,
        ] {
            assert!(
                ids.contains(&expected),
                "hello catalog must include {:?}, got {:?}",
                expected,
                ids
            );
        }
        for entry in &hello.scenes {
            assert_eq!(
                entry.name,
                entry.id.display_name(),
                "CatalogEntry.name must equal display_name() for {:?}",
                entry.id
            );
        }
    }

    /// `apply_command(ClientConnected, ..)` pushes exactly one event with
    /// `body: Message::Hello(…)` and `reply_to: None`.
    #[test]
    fn apply_command_client_connected_pushes_hello_event() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        manager.apply_command(Command::ClientConnected, &event_tx);
        let ev = event_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("apply_command(ClientConnected) must push exactly one event");
        assert!(
            ev.reply_to.is_none(),
            "Hello push must have reply_to: None (unsolicited)"
        );
        match ev.body {
            Message::Hello(h) => {
                assert_eq!(h.active, SceneId::MainHub, "Hello.active must be MainHub");
                assert_eq!(h.scenes.len(), 4, "Hello.scenes must list four M1 scenes");
            }
            other => panic!("expected Hello body, got {:?}", other),
        }
        assert!(
            event_rx.try_recv().is_err(),
            "ClientConnected must push exactly one event"
        );
    }

    /// `apply_command(SwitchScene{target,params}, ..)` queues a debug transition
    /// and pushes no immediate event (SceneChanged is deferred to process_pending_notify).
    #[test]
    fn apply_command_switchscene_queues_debug_transition_pushes_no_event() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        manager.apply_command(
            Command::SwitchScene { target: SceneId::BattleViewer, params: None },
            &event_tx,
        );
        assert!(
            event_rx.try_recv().is_err(),
            "SwitchScene command must not push any immediate event"
        );
        // Debug transition must be queued; process_pending (not notify) applies it.
        let result = manager.process_pending();
        assert_eq!(
            result,
            Some(SceneId::BattleViewer),
            "SwitchScene command must queue a debug transition resolved by process_pending"
        );
    }

    /// `process_pending_notify` after a queued transition returns `Some(id)` and
    /// pushes `Event { body: Message::SceneChanged { id }, reply_to: None }`.
    #[test]
    fn process_pending_notify_pushes_scene_changed_and_returns_id_on_switch() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        manager.set_debug_transition(Transition {
            target: SceneId::BattleViewer,
            params: None,
        });
        let result = manager.process_pending_notify(&event_tx);
        assert_eq!(
            result,
            Some(SceneId::BattleViewer),
            "process_pending_notify must return Some(BattleViewer) after a debug transition"
        );
        let ev = event_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("process_pending_notify must push a SceneChanged event after a switch");
        assert!(
            ev.reply_to.is_none(),
            "SceneChanged push must have reply_to: None (unsolicited)"
        );
        match ev.body {
            Message::SceneChanged(sc) => {
                assert_eq!(sc.id, SceneId::BattleViewer, "SceneChanged.id must be BattleViewer");
            }
            other => panic!("expected SceneChanged body, got {:?}", other),
        }
    }

    /// `process_pending_notify` with nothing pending returns `None` and pushes
    /// no event.
    #[test]
    fn process_pending_notify_returns_none_and_pushes_nothing_when_empty() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        let result = manager.process_pending_notify(&event_tx);
        assert_eq!(
            result,
            None,
            "process_pending_notify with nothing pending must return None"
        );
        assert!(
            event_rx.try_recv().is_err(),
            "process_pending_notify with nothing pending must push no event"
        );
    }

    /// `process_pending_notify` fires SceneChanged for a GAMEPLAY transition too,
    /// not only for debug ones — confirming the "any switch" contract.
    #[test]
    fn process_pending_notify_pushes_scene_changed_for_gameplay_transition() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        // Use the gameplay path (not debug).
        manager.set_gameplay_transition(Transition {
            target: SceneId::ArmyEditor,
            params: None,
        });
        let result = manager.process_pending_notify(&event_tx);
        assert_eq!(
            result,
            Some(SceneId::ArmyEditor),
            "process_pending_notify must return Some(ArmyEditor) for a gameplay transition"
        );
        let ev = event_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("process_pending_notify must push SceneChanged for a gameplay transition");
        assert!(ev.reply_to.is_none());
        match ev.body {
            Message::SceneChanged(sc) => {
                assert_eq!(sc.id, SceneId::ArmyEditor);
            }
            other => panic!("expected SceneChanged body, got {:?}", other),
        }
    }
}
