//! `SceneManager` — the engine's per-frame scene lifecycle driver (spec 14).
//!
//! Relocated here from `game::manager` (b3-t1). Production bodies below are
//! RED stubs (`unimplemented!()`) pending the code-writer's real move — see
//! `.agent_handoffs/engine-game-crate-split-phase-b/b3-t1/research.md` for
//! the exact body each stub must receive (moved verbatim from the old
//! `game/src/manager.rs`, minus the stripped digit-hotkey branch in
//! `route_key`).

use std::collections::BTreeMap;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use ratatui::Frame;
use serde_json::Value as JsonValue;

use crate::ipc::{
    CatalogEntry, ErrorCode, ErrorPayload, Event, Hello, Message, SceneChanged, StateSnapshot,
};
use crate::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::{Inspectable, SceneCatalog, SceneKey};

/// Concrete, nameable box type for the catalog `SceneManager` stores — the
/// spec's literal, non-generic shape (b3-t1 collapses the Phase-A interim
/// associated-type form `Box<dyn SceneCatalog<Scene = dyn Scene>>`).
pub type SceneCatalogBox = Box<dyn SceneCatalog>;

/// Minimum wall-clock gap between automatic live-mode `StateSnapshot` pushes
/// (spec 14 line 231 / spec 15: ~10 Hz coalescing while `Subscribe{live}` is on).
const LIVE_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(100);

/// Inbound command drained at top-of-frame.
/// `target` is already resolved to a `SceneKey` — the wire-string
/// → SceneKey resolution (and Error{UnknownScene}) happens at the IPC
/// boundary (b4), NOT in the manager.
pub enum Command {
    /// A new inspector client has connected; the loop must push a Hello.
    ClientConnected,
    SwitchScene {
        target: SceneKey,
        params: Option<JsonValue>,
    },
    /// Apply a batch of field patches to scene `id` (must be the active scene).
    ApplyState {
        id: SceneKey,
        patch: BTreeMap<String, JsonValue>,
    },
    /// Toggle "apply on change" live mode.
    Subscribe { live: bool },
}

pub struct SceneManager {
    active: Box<dyn Scene>,
    pending: Option<Transition>,
    /// True once a debug command has claimed `pending` this frame; gameplay
    /// calls to `set_gameplay_transition` are no-ops while this is set.
    pending_is_debug: bool,
    ctx: EngineCtx,
    /// Set by `Command::Subscribe { live }`; gates `pump_live_snapshots` and
    /// suppresses `ApplyState`'s immediate reply `StateSnapshot`.
    live_subscribed: bool,
    /// Wall-clock time of the last automatic live-mode `StateSnapshot` push;
    /// `None` means "not yet pushed since subscribing" (next pump is due).
    last_live_push: Option<Instant>,
    /// Game-supplied scene registry.
    catalog: SceneCatalogBox,
}

impl SceneManager {
    /// Construct `boot` via `catalog` and call `enter(None)`.
    pub fn new(boot: SceneKey, catalog: SceneCatalogBox) -> Self {
        let scene = catalog.construct(&boot);
        Self::with_scene_and_params(scene, None, catalog)
    }

    /// Boot an already-constructed scene (off-catalog or example-supplied)
    /// with `enter(&mut ctx, None)`. Delegates to `with_scene_and_params`.
    pub fn with_scene(boot: Box<dyn Scene>, catalog: SceneCatalogBox) -> Self {
        Self::with_scene_and_params(boot, None, catalog)
    }

    /// Boot `boot` and call `enter(&mut ctx, params)` once with the exact
    /// `params` given, setting it as the active scene.
    pub fn with_scene_and_params(
        mut boot: Box<dyn Scene>,
        params: Option<JsonValue>,
        catalog: SceneCatalogBox,
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
            catalog,
        }
    }

    /// Return the id of the currently active scene.
    pub fn active_id(&self) -> SceneKey {
        self.active.id()
    }

    /// Return the active scene's `Inspectable` hook.
    pub fn active_inspect(&mut self) -> &mut dyn Inspectable {
        self.active.inspect()
    }

    /// Returns whether the active scene has requested application exit.
    pub fn active_quit_requested(&self) -> bool {
        self.active.quit_requested()
    }

    /// Debug command: set `pending`, overriding any prior transition (debug always wins).
    pub fn set_debug_transition(&mut self, t: Transition) {
        self.pending = Some(t);
        self.pending_is_debug = true;
    }

    /// Gameplay (input/update) path: set `pending` only if a debug command has not
    /// already claimed it this frame.
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
    pub fn process_pending(&mut self) -> Option<SceneKey> {
        let transition = self.pending.take()?;
        self.pending_is_debug = false;
        self.active.exit(&mut self.ctx);
        let mut new_scene = self.catalog.construct(&transition.target);
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

    /// Returns the Hello catalog (names from `display_name()`, active = current scene id).
    pub fn hello(&self) -> Hello {
        let scenes = self
            .catalog
            .catalog_keys()
            .into_iter()
            .map(|key| {
                let name = self.catalog.display_name(&key).to_string();
                let schema = self.catalog.schema_for(&key);
                CatalogEntry { id: key, name, schema }
            })
            .collect();
        Hello {
            scenes,
            active: self.active_id(),
        }
    }

    /// Handle one inbound IPC command from the bridge.
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
                if !self.catalog.is_available(&target) {
                    let _ = events.send(Event {
                        body: Message::Error(ErrorPayload {
                            code: ErrorCode::UnknownScene,
                            message: format!("scene {target:?} is not implemented"),
                        }),
                        reply_to: None,
                    });
                    return;
                }
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

    /// Push at most one automatic `StateSnapshot` per ~100ms while
    /// `Subscribe{live:true}` is active. No-op when not subscribed. The
    /// first pump after subscribing always pushes.
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
    /// applies it and pushes `Event { body: Message::SceneChanged { id }, .. }`.
    /// Returns `Some(id)` when a switch occurred, `None` otherwise.
    pub fn process_pending_notify(&mut self, events: &Sender<Event>) -> Option<SceneKey> {
        let id = self.process_pending()?;
        let snapshot = self.active.inspect().snapshot();
        let _ = events.send(Event {
            body: Message::SceneChanged(SceneChanged { id: id.clone(), snapshot }),
            reply_to: None,
        });
        Some(id)
    }

    /// Route one key event. Returns `true` if the app should quit (`q` or
    /// Ctrl-C). Digit-key ('1'-'9') scene switching is REMOVED here (b3-t1)
    /// — it moves to `game::app`'s own dispatch table (b3-t2). All other
    /// keys forward to the active scene via `handle_input`.
    pub fn route_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Quit keys checked first; active scene is left unchanged.
        if key.code == KeyCode::Char('q') {
            return true;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }

        // All other keys: forward to the active scene.
        if let Some(t) = self.handle_input(InputEvent::Key(key)) {
            self.set_gameplay_transition(t);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
    use std::sync::mpsc::channel;

    use crate::ipc::{ErrorCode, ErrorPayload, Message, StateSnapshot};
    use crate::test_support::MockCatalog;

    fn key(c: char, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), mods)
    }

    // ---------------------------------------------------------- boot / enter

    #[test]
    fn with_scene_and_params_delivers_params_and_sets_active_id() {
        let scene = crate::test_support::MockScene::new(SceneKey::new("A"));
        let rec = scene.recorder();
        let params = serde_json::json!({"k": 1});
        let mgr = SceneManager::with_scene_and_params(
            Box::new(scene),
            Some(params.clone()),
            Box::new(MockCatalog),
        );
        assert_eq!(mgr.active_id(), SceneKey::new("A"));
        assert_eq!(rec.lock().unwrap().entered_params, vec![Some(params)]);
    }

    #[test]
    fn with_scene_and_params_none_delivers_none_to_enter() {
        let scene = crate::test_support::MockScene::new(SceneKey::new("A"));
        let rec = scene.recorder();
        let _mgr =
            SceneManager::with_scene_and_params(Box::new(scene), None, Box::new(MockCatalog));
        assert_eq!(rec.lock().unwrap().entered_params, vec![None]);
    }

    #[test]
    fn with_scene_sets_active_id_and_calls_enter() {
        let scene = crate::test_support::MockScene::new(SceneKey::new("B"));
        let rec = scene.recorder();
        let mgr = SceneManager::with_scene(Box::new(scene), Box::new(MockCatalog));
        assert_eq!(mgr.active_id(), SceneKey::new("B"));
        assert_eq!(rec.lock().unwrap().entered_params, vec![None]);
    }

    // ------------------------------------------------------------ handle_input

    #[test]
    fn handle_input_forwards_key_event_to_active_scene() {
        let scene = crate::test_support::MockScene::new(SceneKey::new("A"));
        let rec = scene.recorder();
        let mut mgr = SceneManager::with_scene(Box::new(scene), Box::new(MockCatalog));
        let ke = key('x', KeyModifiers::NONE);
        mgr.handle_input(InputEvent::Key(ke));
        assert_eq!(rec.lock().unwrap().last_key, Some(ke));
    }

    #[test]
    fn handle_input_forwards_mouse_event_to_active_scene() {
        let scene = crate::test_support::MockScene::new(SceneKey::new("A"));
        let rec = scene.recorder();
        let mut mgr = SceneManager::with_scene(Box::new(scene), Box::new(MockCatalog));
        let me = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 3,
            row: 4,
            modifiers: KeyModifiers::empty(),
        };
        mgr.handle_input(InputEvent::Mouse(me));
        assert_eq!(rec.lock().unwrap().last_mouse, Some(me));
    }

    // ------------------------------------------------------- transition precedence

    #[test]
    fn debug_transition_overrides_gameplay_gameplay_first() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        mgr.set_gameplay_transition(Transition {
            target: SceneKey::new("B"),
            params: None,
        });
        mgr.set_debug_transition(Transition {
            target: SceneKey::new("C"),
            params: None,
        });
        mgr.process_pending();
        assert_eq!(mgr.active_id(), SceneKey::new("C"));
    }

    #[test]
    fn debug_transition_overrides_gameplay_debug_first() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        mgr.set_debug_transition(Transition {
            target: SceneKey::new("C"),
            params: None,
        });
        mgr.set_gameplay_transition(Transition {
            target: SceneKey::new("B"),
            params: None,
        });
        mgr.process_pending();
        assert_eq!(mgr.active_id(), SceneKey::new("C"));
    }

    // --------------------------------------------------------------- process_pending

    #[test]
    fn process_pending_swaps_active_and_returns_new_id() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        mgr.set_gameplay_transition(Transition {
            target: SceneKey::new("B"),
            params: None,
        });
        let result = mgr.process_pending();
        assert_eq!(result, Some(SceneKey::new("B")));
        assert_eq!(mgr.active_id(), SceneKey::new("B"));
    }

    #[test]
    fn process_pending_noop_when_empty() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let result = mgr.process_pending();
        assert_eq!(result, None);
        assert_eq!(mgr.active_id(), SceneKey::new("A"));
    }

    // --------------------------------------------------------------------------- hello

    #[test]
    fn hello_lists_catalog_keys_with_names_schemas_and_active() {
        let mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let hello = mgr.hello();
        let catalog = MockCatalog;
        let expected_keys = catalog.catalog_keys();
        let got_keys: Vec<SceneKey> = hello.scenes.iter().map(|e| e.id.clone()).collect();
        assert_eq!(got_keys, expected_keys);
        for entry in &hello.scenes {
            assert_eq!(entry.name, catalog.display_name(&entry.id));
            assert_eq!(entry.schema, catalog.schema_for(&entry.id));
        }
        assert_eq!(hello.active, SceneKey::new("A"));
    }

    // ------------------------------------------------------------------ apply_command

    #[test]
    fn apply_command_client_connected_pushes_hello_then_state_snapshot() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        mgr.apply_command(Command::ClientConnected, &tx);

        let first = rx
            .recv_timeout(Duration::from_millis(200))
            .expect("Hello event expected");
        assert!(first.reply_to.is_none());
        assert!(matches!(first.body, Message::Hello(_)));

        let second = rx
            .recv_timeout(Duration::from_millis(200))
            .expect("StateSnapshot event expected");
        assert!(second.reply_to.is_none());
        assert!(matches!(second.body, Message::StateSnapshot(_)));

        assert!(
            rx.try_recv().is_err(),
            "ClientConnected must push exactly two events"
        );
    }

    #[test]
    fn apply_command_switchscene_available_queues_transition_no_immediate_event() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        mgr.apply_command(
            Command::SwitchScene {
                target: SceneKey::new("B"),
                params: None,
            },
            &tx,
        );
        assert!(
            rx.try_recv().is_err(),
            "SwitchScene must not push any immediate event"
        );
        let result = mgr.process_pending();
        assert_eq!(result, Some(SceneKey::new("B")));
    }

    #[test]
    fn apply_command_switchscene_unavailable_pushes_unknown_scene_error() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        mgr.apply_command(
            Command::SwitchScene {
                target: SceneKey::new("Nope"),
                params: None,
            },
            &tx,
        );
        let ev = rx
            .try_recv()
            .expect("an Error event must be pushed for an unavailable target");
        assert!(ev.reply_to.is_none());
        match ev.body {
            Message::Error(ErrorPayload { code, .. }) => assert_eq!(code, ErrorCode::UnknownScene),
            other => panic!("expected Error body, got {:?}", other),
        }
        assert_eq!(
            mgr.process_pending(),
            None,
            "an unavailable target must not queue a transition"
        );
    }

    #[test]
    fn apply_command_applystate_valid_field_applies_and_pushes_snapshot_when_not_live() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        let mut patch = BTreeMap::new();
        patch.insert("value".to_string(), serde_json::json!(3.5));
        mgr.apply_command(
            Command::ApplyState {
                id: mgr.active_id(),
                patch,
            },
            &tx,
        );
        let ev = rx
            .recv_timeout(Duration::from_millis(200))
            .expect("a StateSnapshot must be pushed for a valid, non-live ApplyState");
        match ev.body {
            Message::StateSnapshot(StateSnapshot { snapshot, .. }) => {
                assert_eq!(snapshot["value"], serde_json::json!(3.5));
            }
            other => panic!("expected StateSnapshot body, got {:?}", other),
        }
    }

    #[test]
    fn apply_command_applystate_bad_field_pushes_bad_field_error() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        let mut patch = BTreeMap::new();
        patch.insert("nonexistent".to_string(), serde_json::json!(1));
        mgr.apply_command(
            Command::ApplyState {
                id: mgr.active_id(),
                patch,
            },
            &tx,
        );
        let ev = rx
            .try_recv()
            .expect("an Error event must be pushed for an unknown field");
        match ev.body {
            Message::Error(ErrorPayload { code, .. }) => assert_eq!(code, ErrorCode::BadField),
            other => panic!("expected Error body, got {:?}", other),
        }
    }

    #[test]
    fn apply_command_applystate_wrong_id_pushes_not_active_error() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        mgr.apply_command(
            Command::ApplyState {
                id: SceneKey::new("B"),
                patch: BTreeMap::new(),
            },
            &tx,
        );
        let ev = rx
            .try_recv()
            .expect("an Error event must be pushed for a non-active id");
        match ev.body {
            Message::Error(ErrorPayload { code, .. }) => assert_eq!(code, ErrorCode::NotActive),
            other => panic!("expected Error body, got {:?}", other),
        }
    }

    // ---------------------------------------------------------- process_pending_notify

    #[test]
    fn process_pending_notify_pushes_scene_changed_and_returns_id_on_switch() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        mgr.set_debug_transition(Transition {
            target: SceneKey::new("B"),
            params: None,
        });
        let result = mgr.process_pending_notify(&tx);
        assert_eq!(result, Some(SceneKey::new("B")));
        let ev = rx
            .recv_timeout(Duration::from_millis(200))
            .expect("process_pending_notify must push a SceneChanged event after a switch");
        assert!(ev.reply_to.is_none());
        match ev.body {
            Message::SceneChanged(sc) => assert_eq!(sc.id, SceneKey::new("B")),
            other => panic!("expected SceneChanged body, got {:?}", other),
        }
    }

    #[test]
    fn process_pending_notify_returns_none_and_pushes_nothing_when_empty() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        assert_eq!(mgr.process_pending_notify(&tx), None);
        assert!(rx.try_recv().is_err());
    }

    // -------------------------------------------------------- pump_live_snapshots

    #[test]
    fn pump_live_snapshots_gated_to_one_per_100ms_window() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();
        mgr.apply_command(Command::Subscribe { live: true }, &tx);

        let t0 = Instant::now();
        mgr.pump_live_snapshots(&tx, t0);
        rx.try_recv()
            .expect("first pump after Subscribe{live:true} must push");

        mgr.pump_live_snapshots(&tx, t0 + Duration::from_millis(50));
        assert!(
            rx.try_recv().is_err(),
            "a pump within the 100ms window must not push again"
        );

        mgr.pump_live_snapshots(&tx, t0 + Duration::from_millis(100));
        assert!(
            rx.try_recv().is_ok(),
            "a pump >=100ms after the last push must push again"
        );
    }

    #[test]
    fn pump_live_snapshots_noop_when_not_subscribed() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let (tx, rx) = channel::<Event>();

        mgr.pump_live_snapshots(&tx, Instant::now());
        assert!(
            rx.try_recv().is_err(),
            "pump_live_snapshots must push nothing before any Subscribe{{live:true}}"
        );

        mgr.apply_command(Command::Subscribe { live: true }, &tx);
        let t0 = Instant::now();
        mgr.pump_live_snapshots(&tx, t0);
        rx.try_recv().expect("expected the initial push while subscribed");

        mgr.apply_command(Command::Subscribe { live: false }, &tx);
        mgr.pump_live_snapshots(&tx, t0 + Duration::from_millis(500));
        assert!(
            rx.try_recv().is_err(),
            "pump_live_snapshots must push nothing after Subscribe{{live:false}}"
        );
    }

    // ------------------------------------------------------------------------ route_key

    #[test]
    fn route_key_q_returns_quit_active_unchanged() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let quit = mgr.route_key(key('q', KeyModifiers::NONE));
        assert!(quit, "route_key('q') must return true (quit signal)");
        assert_eq!(mgr.active_id(), SceneKey::new("A"));
    }

    #[test]
    fn route_key_ctrl_c_returns_quit() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let quit = mgr.route_key(key('c', KeyModifiers::CONTROL));
        assert!(quit, "route_key(Ctrl-C) must return true (quit signal)");
    }

    #[test]
    fn route_key_forwards_non_quit_key_to_active_scene() {
        let scene = crate::test_support::MockScene::new(SceneKey::new("A"));
        let rec = scene.recorder();
        let mut mgr = SceneManager::with_scene(Box::new(scene), Box::new(MockCatalog));
        let ke = key('x', KeyModifiers::NONE);
        let quit = mgr.route_key(ke);
        assert!(!quit, "a non-quit key must return false");
        assert_eq!(rec.lock().unwrap().last_key, Some(ke));
    }

    /// Pins the digit-hotkey removal: a digit key press must forward like
    /// any other non-quit key and must NOT queue a scene transition itself
    /// (that dispatch moves to `game::app`, b3-t2).
    #[test]
    fn route_key_digit_key_no_longer_sets_a_pending_transition() {
        let mut mgr = SceneManager::new(SceneKey::new("A"), Box::new(MockCatalog));
        let quit = mgr.route_key(key('2', KeyModifiers::NONE));
        assert!(!quit);
        assert_eq!(
            mgr.process_pending(),
            None,
            "digit-key scene switching moved to game::app (b3-t2); \
             route_key must not queue a transition"
        );
    }
}
