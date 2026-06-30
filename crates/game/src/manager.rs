use std::time::Duration;

use ratatui::Frame;
use scene_core::scene_id::SceneId;
use serde_json::Value as JsonValue;

use crate::registry;
use crate::scene::{EngineCtx, InputEvent, Scene, Transition};

/// Inbound command drained at top-of-frame. M1 stub: only a debug scene switch.
/// b4 (ipc_server) owns the real mpsc channel + the outbound `Event` type and may
/// extend this enum. `target` is already resolved to a `SceneId` — the wire-string
/// → SceneId resolution (and Error{UnknownScene}) happens at the IPC boundary in b4,
/// NOT in the manager.
pub enum Command {
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
        let mut ctx = EngineCtx;
        let mut active = registry::construct(boot);
        active.enter(&mut ctx, None);
        SceneManager {
            active,
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
    use scene_core::scene_id::SceneId;

    use crate::scenes::MainHub;

    fn key(c: char, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), mods)
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
}
