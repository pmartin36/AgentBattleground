use std::collections::BTreeMap;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use ratatui::Frame;
use scene_core::ipc::{
    CatalogEntry, ErrorCode, ErrorPayload, Hello, Message, SceneChanged, StateSnapshot,
};
use scene_core::scene_id::SceneId;
use serde_json::Value as JsonValue;

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
    /// Apply a batch of field patches to scene `id` (must be the active scene).
    ApplyState {
        id: SceneId,
        patch: BTreeMap<String, JsonValue>,
    },
    /// Toggle "apply on change" live mode (b5-t4: decode + route only; the
    /// 10Hz-coalesced live-push behaviour is b5-t5).
    Subscribe { live: bool },
}

/// Minimum wall-clock gap between automatic live-mode `StateSnapshot` pushes
/// (spec 14 line 231 / spec 15: ~10 Hz coalescing while `Subscribe{live}` is on).
const LIVE_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(100);

pub struct SceneManager {
    active: Box<dyn Scene>,
    pending: Option<Transition>,
    /// True once a debug command has claimed `pending` this frame; gameplay
    /// calls to `set_gameplay_transition` are no-ops while this is set.
    pending_is_debug: bool,
    ctx: EngineCtx,
    /// Set by `Command::Subscribe { live }`; gates `pump_live_snapshots` and
    /// suppresses `ApplyState`'s immediate reply `StateSnapshot` (b5-t5).
    live_subscribed: bool,
    /// Wall-clock time of the last automatic live-mode `StateSnapshot` push;
    /// `None` means "not yet pushed since subscribing" (next pump is due).
    last_live_push: Option<Instant>,
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
            live_subscribed: false,
            last_live_push: None,
        }
    }

    /// Return the id of the currently active scene.
    pub fn active_id(&self) -> SceneId {
        self.active.id()
    }

    /// Return the active scene's `Inspectable` hook (spec 14 line 85's M2
    /// hook, b5-t2). Mutations through the returned reference persist on the
    /// real active scene.
    pub fn active_inspect(&mut self) -> &mut dyn scene_core::Inspectable {
        self.active.inspect()
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
                schema: registry::schema_for(id),
            })
            .collect();
        Hello {
            scenes,
            active: self.active_id(),
        }
    }

    /// Handle one inbound IPC command from the bridge.
    ///
    /// - `ClientConnected` → push `Event { body: Message::Hello(self.hello()), reply_to: None }`,
    ///   then an unprompted `Event { body: Message::StateSnapshot { id: active_id(),
    ///   snapshot: active.inspect().snapshot() }, reply_to: None }` so a freshly
    ///   connected inspector's panel has real values immediately (b5-t3).
    /// - `SwitchScene { target, params }` → `set_debug_transition`; no event pushed here
    ///   (SceneChanged is pushed by `process_pending_notify` after the swap).
    pub fn apply_command(&mut self, cmd: Command, events: &Sender<Event>) {
        match cmd {
            Command::ClientConnected => {
                let _ = events.send(Event {
                    body: Message::Hello(self.hello()),
                    reply_to: None,
                });
                self.push_state_snapshot(events);
            }
            Command::SwitchScene { target, params } => {
                self.set_debug_transition(Transition { target, params });
            }
            Command::ApplyState { id, patch } => {
                if id != self.active_id() {
                    let _ = events.send(Event {
                        body: Message::Error(ErrorPayload {
                            code: ErrorCode::NotActive,
                            message: format!("scene {id:?} is not active"),
                        }),
                        reply_to: None,
                    });
                    return;
                }
                // SOLE OWNER of the batch-apply loop (spec point 7): body is
                // here, not delegated to a shared helper.
                let mut rejected: Vec<String> = Vec::new();
                let mut applied_any = false;
                for (path, value) in patch {
                    match self.active_inspect().apply_patch(&path, value) {
                        Ok(()) => applied_any = true,
                        Err(e) => rejected.push(format!("{path}: {e}")),
                    }
                }
                if !rejected.is_empty() {
                    let _ = events.send(Event {
                        body: Message::Error(ErrorPayload {
                            code: ErrorCode::BadField,
                            message: rejected.join(", "),
                        }),
                        reply_to: None,
                    });
                }
                // In live mode the immediate reply is suppressed: the change
                // is folded into the next pump_live_snapshots coalesced push.
                if applied_any && !self.live_subscribed {
                    self.push_state_snapshot(events);
                }
            }
            Command::Subscribe { live } => {
                self.live_subscribed = live;
                if live {
                    // Reset the clock so the next pump fires immediately.
                    self.last_live_push = None;
                }
            }
        }
    }

    /// Shared `Event { StateSnapshot { id, snapshot }, reply_to: None }` push
    /// for the active scene — reused by `ClientConnected`, `ApplyState`'s
    /// (non-live) immediate reply, and `pump_live_snapshots`.
    fn push_state_snapshot(&mut self, events: &Sender<Event>) {
        let id = self.active_id();
        let snapshot = self.active_inspect().snapshot();
        let _ = events.send(Event {
            body: Message::StateSnapshot(StateSnapshot { id, snapshot }),
            reply_to: None,
        });
    }

    /// Push at most one automatic `StateSnapshot` per `LIVE_SNAPSHOT_INTERVAL`
    /// (~10 Hz) while `Subscribe{live:true}` is active. No-op when not
    /// subscribed. The first pump after subscribing always pushes.
    pub fn pump_live_snapshots(&mut self, events: &Sender<Event>, now: Instant) {
        if !self.live_subscribed {
            return;
        }
        let due = match self.last_live_push {
            None => true,
            Some(prev) => now.duration_since(prev) >= LIVE_SNAPSHOT_INTERVAL,
        };
        if !due {
            return;
        }
        self.push_state_snapshot(events);
        self.last_live_push = Some(now);
    }

    /// Notifying wrapper around `process_pending`: if a transition is pending,
    /// applies it (via `process_pending`) and pushes
    /// `Event { body: Message::SceneChanged { id }, reply_to: None }` on `events`.
    /// Returns `Some(id)` when a switch occurred, `None` otherwise.
    pub fn process_pending_notify(&mut self, events: &Sender<Event>) -> Option<SceneId> {
        let id = self.process_pending()?;
        let snapshot = self.active.inspect().snapshot();
        let _ = events.send(Event {
            body: Message::SceneChanged(SceneChanged { id, snapshot }),
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
            no_inspect: crate::scene::NoInspect,
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
            fn inspect(&mut self) -> &mut dyn scene_core::Inspectable {
                &mut self.no_inspect
            }
        }

        let entered = Arc::new(Mutex::new(false));
        let scene = TestScene {
            entered: Arc::clone(&entered),
            no_inspect: crate::scene::NoInspect,
        };
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
            no_inspect: crate::scene::NoInspect,
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
            fn inspect(&mut self) -> &mut dyn scene_core::Inspectable {
                &mut self.no_inspect
            }
        }

        let captured = Arc::new(Mutex::new(None));
        let scene = ParamsCapturingScene {
            params: Arc::clone(&captured),
            no_inspect: crate::scene::NoInspect,
        };
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
            no_inspect: crate::scene::NoInspect,
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
            fn inspect(&mut self) -> &mut dyn scene_core::Inspectable {
                &mut self.no_inspect
            }
        }

        let captured = Arc::new(Mutex::new(Some(json!({"stale": true}))));
        let scene = ParamsCapturingScene {
            params: Arc::clone(&captured),
            no_inspect: crate::scene::NoInspect,
        };
        let _mgr = SceneManager::with_scene_and_params(Box::new(scene), None);

        assert_eq!(
            *captured.lock().unwrap(),
            None,
            "with_scene_and_params(scene, None) must deliver None to enter()"
        );
    }

    // ------------------------------------------------------ handle_input (b1-t1)

    /// `SceneManager::handle_input(InputEvent::Mouse(me))` forwards the exact
    /// `MouseEvent` payload to the active scene's `handle_input` — pinning the
    /// generic-forwarding contract for the new `Mouse` variant.
    #[test]
    fn handle_input_forwards_mouse_event_to_active_scene() {
        use std::sync::{Arc, Mutex};
        use ratatui::layout::Rect;
        use serde_json::Value as JsonValue;
        use crossterm::event::{MouseEvent, MouseEventKind};

        struct MouseCapturingScene {
            captured: Arc<Mutex<Option<MouseEvent>>>,
            no_inspect: crate::scene::NoInspect,
        }

        impl Scene for MouseCapturingScene {
            fn id(&self) -> SceneId {
                SceneId::Leaderboard
            }
            fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}
            fn update(
                &mut self,
                _ctx: &mut EngineCtx,
                _dt: std::time::Duration,
            ) -> Option<Transition> {
                None
            }
            fn render(&self, _frame: &mut ratatui::Frame, _area: Rect) {}
            fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
                if let InputEvent::Mouse(me) = ev {
                    *self.captured.lock().unwrap() = Some(me);
                }
                None
            }
            fn exit(&mut self, _ctx: &mut EngineCtx) {}
            fn inspect(&mut self) -> &mut dyn scene_core::Inspectable {
                &mut self.no_inspect
            }
        }

        let captured = Arc::new(Mutex::new(None));
        let scene = MouseCapturingScene {
            captured: Arc::clone(&captured),
            no_inspect: crate::scene::NoInspect,
        };
        let mut mgr = SceneManager::with_scene(Box::new(scene));

        let me = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 7,
            row: 3,
            modifiers: KeyModifiers::empty(),
        };
        mgr.handle_input(InputEvent::Mouse(me));

        assert_eq!(
            *captured.lock().unwrap(),
            Some(me),
            "handle_input must forward the exact MouseEvent to the active scene"
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
            target: SceneId::RosterManager,
            params: None,
        });
        manager.process_pending();
        assert_eq!(
            manager.active_id(),
            SceneId::RosterManager,
            "debug must override gameplay when gameplay transition was set first"
        );
    }

    /// Debug transition always overrides a gameplay transition set second in the same tick.
    #[test]
    fn debug_transition_overrides_gameplay_debug_first() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        manager.set_debug_transition(Transition {
            target: SceneId::RosterManager,
            params: None,
        });
        manager.set_gameplay_transition(Transition {
            target: SceneId::BattleViewer,
            params: None,
        });
        manager.process_pending();
        assert_eq!(
            manager.active_id(),
            SceneId::RosterManager,
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
            target: SceneId::RosterManager,
            params: None,
        });
        // Digit '2' tries the gameplay path; must be blocked by pending_is_debug.
        manager.route_key(key('2', KeyModifiers::NONE));
        manager.process_pending();
        assert_eq!(
            manager.active_id(),
            SceneId::RosterManager,
            "debug transition must override the digit gameplay transition"
        );
    }

    // ═══════════════════════════════════════ b4-t2: IPC protocol methods ═══════

    /// `hello()` returns exactly four M1 scenes in digit-key order
    /// (MainHub, BattleViewer, RosterManager, Leaderboard), each name matching
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
            SceneId::RosterManager,
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

    /// `apply_command(ClientConnected, ..)` pushes exactly TWO events, in
    /// order: `Hello` then `StateSnapshot` for the active scene (b5-t3,
    /// task brief point 13 / round-2 MEDIUM-2 fix) — REPLACES the
    /// pre-b5-t3 "exactly one event" contract.
    #[test]
    fn client_connected_pushes_hello_then_statesnapshot_in_order() {
        use scene_core::ipc::StateSnapshot;
        use serde_json::json;

        let mut manager = SceneManager::new(SceneId::MainHub);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        manager.apply_command(Command::ClientConnected, &event_tx);

        let first = event_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("apply_command(ClientConnected) must push a Hello event first");
        assert!(
            first.reply_to.is_none(),
            "Hello push must have reply_to: None (unsolicited)"
        );
        match first.body {
            Message::Hello(h) => {
                assert_eq!(h.active, SceneId::MainHub, "Hello.active must be MainHub");
                assert_eq!(h.scenes.len(), 4, "Hello.scenes must list four M1 scenes");
            }
            other => panic!("expected Hello body first, got {:?}", other),
        }

        let second = event_rx.recv_timeout(Duration::from_millis(200)).expect(
            "apply_command(ClientConnected) must push a second, unprompted \
             StateSnapshot event for the active scene",
        );
        assert!(
            second.reply_to.is_none(),
            "StateSnapshot push must have reply_to: None (unsolicited)"
        );
        match second.body {
            Message::StateSnapshot(StateSnapshot { id, snapshot }) => {
                assert_eq!(id, SceneId::MainHub, "StateSnapshot.id must be the active scene");
                assert_eq!(
                    snapshot,
                    json!({}),
                    "StateSnapshot.snapshot must be the active scene's real \
                     inspect().snapshot() (MainHub has no fields, so {{}})"
                );
            }
            other => panic!("expected StateSnapshot body second, got {:?}", other),
        }

        assert!(
            event_rx.try_recv().is_err(),
            "ClientConnected must push exactly two events (Hello, StateSnapshot)"
        );
    }

    /// Every `CatalogEntry.schema` in `hello()` must equal
    /// `registry::schema_for(entry.id)` — the real per-type schema, not a
    /// stub (b5-t3, round-2 HIGH-2 fix).
    #[test]
    fn hello_entries_carry_real_schema_matching_schema_for() {
        let manager = SceneManager::new(SceneId::MainHub);
        let hello = manager.hello();
        assert!(!hello.scenes.is_empty(), "hello() must list scenes to check");
        for entry in &hello.scenes {
            assert_eq!(
                entry.schema,
                crate::registry::schema_for(entry.id),
                "CatalogEntry.schema for {:?} must equal registry::schema_for(id)",
                entry.id
            );
        }
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

    /// `process_pending_notify` after a switch must push `SceneChanged.snapshot`
    /// equal to the NEW active scene's real `inspect().snapshot()` — not the
    /// pre-b5-t3 `JsonValue::Null` placeholder (task brief point 13).
    #[test]
    fn process_pending_notify_scene_changed_snapshot_matches_new_scene() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        manager.set_debug_transition(Transition {
            target: SceneId::BattleViewer,
            params: None,
        });
        manager.process_pending_notify(&event_tx);
        let ev = event_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("process_pending_notify must push a SceneChanged event after a switch");
        match ev.body {
            Message::SceneChanged(sc) => {
                assert_eq!(sc.id, SceneId::BattleViewer);
                assert_ne!(
                    sc.snapshot,
                    JsonValue::Null,
                    "SceneChanged.snapshot must carry the new scene's real \
                     values, not the Null placeholder"
                );
                let expected = manager.active_inspect().snapshot();
                assert_eq!(
                    sc.snapshot, expected,
                    "SceneChanged.snapshot must equal the new active scene's \
                     inspect().snapshot()"
                );
            }
            other => panic!("expected SceneChanged body, got {:?}", other),
        }
    }

    /// `process_pending_notify` fires SceneChanged for a GAMEPLAY transition too,
    /// not only for debug ones — confirming the "any switch" contract.
    #[test]
    fn process_pending_notify_pushes_scene_changed_for_gameplay_transition() {
        let mut manager = SceneManager::new(SceneId::MainHub);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        // Use the gameplay path (not debug).
        manager.set_gameplay_transition(Transition {
            target: SceneId::RosterManager,
            params: None,
        });
        let result = manager.process_pending_notify(&event_tx);
        assert_eq!(
            result,
            Some(SceneId::RosterManager),
            "process_pending_notify must return Some(RosterManager) for a gameplay transition"
        );
        let ev = event_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("process_pending_notify must push SceneChanged for a gameplay transition");
        assert!(ev.reply_to.is_none());
        match ev.body {
            Message::SceneChanged(sc) => {
                assert_eq!(sc.id, SceneId::RosterManager);
            }
            other => panic!("expected SceneChanged body, got {:?}", other),
        }
    }

    // ------------------------------------------------------------- inspect (b5-t2)

    /// `SceneManager::active_inspect()` (backed by `Scene::inspect()`) must return
    /// a LIVE hook into the active scene, not a fresh/throwaway value: a mutation
    /// applied through one call must be visible via the very next call.
    #[test]
    fn inspect_hooks_battle_viewer_live() {
        use serde_json::json;

        let mut mgr = SceneManager::new(SceneId::BattleViewer);
        mgr.active_inspect()
            .apply_patch("elapsed", json!(9.0))
            .expect("elapsed is a plain, editable f32 field (b5-t1)");

        assert_eq!(
            mgr.active_inspect().snapshot()["elapsed"],
            json!(9.0),
            "inspect() must return a live hook into the real active scene — the \
             patched value must be visible on the next inspect() call"
        );
    }

    /// `MainHub` is a zero-field scene; its `inspect()` hook must still be
    /// genuinely wired (real derive + `{ self }`), not a stub that merely
    /// compiles — the exact "silently unreal" risk this task guards against.
    #[test]
    fn trivial_scene_inspect_snapshot_is_empty_object() {
        use serde_json::json;

        let mut hub = MainHub;
        assert_eq!(
            hub.inspect().snapshot(),
            json!({}),
            "MainHub::inspect() must expose a real (derived) Inspectable \
             snapshot, not an unimplemented stub"
        );
    }

    // ═══════════════════ b5-t5: live-mode 10Hz-coalesced StateSnapshot ═════════

    /// While `Subscribe{live:true}` is active, `pump_live_snapshots` pushes at
    /// most one `StateSnapshot` per ~100ms window: first call after subscribe
    /// pushes immediately; a second call <100ms later is a no-op; a third call
    /// >=100ms after the FIRST push pushes again.
    #[test]
    fn pump_live_snapshots_gated_to_one_per_100ms_window() {
        use std::time::Instant;

        let mut mgr = SceneManager::new(SceneId::BattleViewer);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        mgr.apply_command(Command::Subscribe { live: true }, &event_tx);

        let t0 = Instant::now();
        mgr.pump_live_snapshots(&event_tx, t0);
        let first = event_rx.try_recv().expect(
            "first pump_live_snapshots call after Subscribe{live:true} must push a StateSnapshot",
        );
        match first.body {
            Message::StateSnapshot(StateSnapshot { id, .. }) => {
                assert_eq!(id, SceneId::BattleViewer, "pumped StateSnapshot.id must be active scene");
            }
            other => panic!("expected StateSnapshot, got {:?}", other),
        }

        mgr.pump_live_snapshots(&event_tx, t0 + Duration::from_millis(50));
        assert!(
            event_rx.try_recv().is_err(),
            "a pump call 50ms after the first push (within the 100ms window) must not push again"
        );

        mgr.pump_live_snapshots(&event_tx, t0 + Duration::from_millis(100));
        assert!(
            event_rx.try_recv().is_ok(),
            "a pump call >=100ms after the first push must push a second StateSnapshot"
        );
    }

    /// `pump_live_snapshots` is a no-op when not subscribed, and becomes a
    /// no-op again after `Subscribe{live:false}` turns live mode off.
    #[test]
    fn pump_live_snapshots_noop_when_not_subscribed() {
        use std::time::Instant;

        let mut mgr = SceneManager::new(SceneId::BattleViewer);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();

        mgr.pump_live_snapshots(&event_tx, Instant::now());
        assert!(
            event_rx.try_recv().is_err(),
            "pump_live_snapshots must push nothing before any Subscribe{{live:true}}"
        );

        mgr.apply_command(Command::Subscribe { live: true }, &event_tx);
        let t0 = Instant::now();
        mgr.pump_live_snapshots(&event_tx, t0);
        event_rx.try_recv().expect("expected the initial push while subscribed");

        mgr.apply_command(Command::Subscribe { live: false }, &event_tx);
        mgr.pump_live_snapshots(&event_tx, t0 + Duration::from_millis(500));
        assert!(
            event_rx.try_recv().is_err(),
            "pump_live_snapshots must push nothing after Subscribe{{live:false}}"
        );
    }

    /// Regression guard for b5-t4's contract + the new live gate: a valid
    /// `ApplyState` still pushes its immediate reply `StateSnapshot` when NOT
    /// subscribed, but that immediate push is suppressed while
    /// `Subscribe{live:true}` is active (coalesced into the pump instead).
    #[test]
    fn apply_state_immediate_snapshot_suppressed_only_in_live_mode() {
        use serde_json::json;

        // Not subscribed: immediate StateSnapshot reply is retained.
        let mut mgr = SceneManager::new(SceneId::BattleViewer);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        let mut patch = BTreeMap::new();
        patch.insert("elapsed".to_string(), json!(2.0));
        mgr.apply_command(
            Command::ApplyState { id: mgr.active_id(), patch },
            &event_tx,
        );
        assert!(
            matches!(
                event_rx.recv_timeout(Duration::from_millis(200)).map(|e| e.body),
                Ok(Message::StateSnapshot(_))
            ),
            "non-live ApplyState must still push an immediate StateSnapshot (b5-t4 contract)"
        );

        // Subscribed live: immediate StateSnapshot reply must be suppressed.
        let mut mgr = SceneManager::new(SceneId::BattleViewer);
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        mgr.apply_command(Command::Subscribe { live: true }, &event_tx);
        let mut patch = BTreeMap::new();
        patch.insert("elapsed".to_string(), json!(2.0));
        mgr.apply_command(
            Command::ApplyState { id: mgr.active_id(), patch },
            &event_tx,
        );
        assert!(
            event_rx.try_recv().is_err(),
            "live-mode ApplyState must NOT push an immediate StateSnapshot — it is \
             coalesced into the next pump_live_snapshots push instead"
        );
    }
}
