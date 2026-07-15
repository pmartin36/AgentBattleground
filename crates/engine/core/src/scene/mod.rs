use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use crate::Inspectable;
use crate::SceneKey;
use serde_json::Value as JsonValue;

pub mod manager;

/// A full-screen game mode. Exactly one is active at a time (spec 14).
pub trait Scene {
    fn id(&self) -> SceneKey;
    fn enter(&mut self, ctx: &mut EngineCtx, params: Option<JsonValue>);
    fn update(&mut self, ctx: &mut EngineCtx, dt: Duration) -> Option<Transition>;
    fn render(&self, frame: &mut Frame, area: Rect);
    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition>;
    fn exit(&mut self, ctx: &mut EngineCtx);
    /// M2 hook (spec 14 line 85): a live view into this scene's own
    /// `Inspectable` state. Mutations through the returned reference must
    /// persist on the real scene (b5-t2) — every implementor below is a
    /// RED stub (`unimplemented!()`) pending the code-writer's real bodies.
    fn inspect(&mut self) -> &mut dyn Inspectable;
    /// Engine polls this once per frame after input dispatch (b4-t1); return
    /// `true` to request the same app exit `route_key`'s Ctrl-C/Ctrl-Q
    /// produce. Defaults to `false` so every existing `Scene` impl compiles
    /// unchanged.
    fn quit_requested(&self) -> bool {
        false
    }

    /// Whether this scene currently owns the break key (Ctrl-C) — i.e. a
    /// text field has focus and binds it to something else (copy). While
    /// `true`, `route_key` forwards Ctrl-C to this scene instead of quitting;
    /// Ctrl-Q remains an unconditional quit either way.
    ///
    /// Defaults to `false`: a scene with no text field lets Ctrl-C quit.
    fn consumes_break(&self) -> bool {
        false
    }
}

/// A request to switch the active scene (spec 14 line 87).
pub struct Transition {
    pub target: SceneKey,
    pub params: Option<JsonValue>,
}

/// Per-frame engine services handed to a scene. Unit for M1 (reserved to grow:
/// renderer/clock/input/rng per spec 14 line 33). Scenes do NOT get the IPC handle.
pub struct EngineCtx;

/// A single input event delivered to the active scene.
pub enum InputEvent {
    Key(crossterm::event::KeyEvent),
    Mouse(crossterm::event::MouseEvent),
}

/// Shared placeholder `Inspectable` for scenes with no editable state (test
/// doubles, examples). One DRY empty-`Struct` impl reused everywhere a scene
/// has no real inspectable fields (b5-t2).
#[derive(Default, Inspectable)]
pub struct NoInspect;
