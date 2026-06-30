use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use scene_core::scene_id::SceneId;
use serde_json::Value as JsonValue;

/// A full-screen game mode. Exactly one is active at a time (spec 14).
pub trait Scene {
    fn id(&self) -> SceneId;
    fn enter(&mut self, ctx: &mut EngineCtx, params: Option<JsonValue>);
    fn update(&mut self, ctx: &mut EngineCtx, dt: Duration) -> Option<Transition>;
    fn render(&self, frame: &mut Frame, area: Rect);
    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition>;
    fn exit(&mut self, ctx: &mut EngineCtx);
}

/// A request to switch the active scene (spec 14 line 87).
pub struct Transition {
    pub target: SceneId,
    pub params: Option<JsonValue>,
}

/// Per-frame engine services handed to a scene. Unit for M1 (reserved to grow:
/// renderer/clock/input/rng per spec 14 line 33). Scenes do NOT get the IPC handle.
pub struct EngineCtx;

/// A single input event delivered to the active scene.
pub enum InputEvent {
    Key(crossterm::event::KeyEvent),
}
