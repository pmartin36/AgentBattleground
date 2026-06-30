use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use scene_core::color::Rgba;
use scene_core::scene_id::SceneId;
use serde_json::Value as JsonValue;

use crate::scene::{EngineCtx, InputEvent, Scene, Transition};

#[derive(Default)]
pub struct MainHub;

impl MainHub {
    pub const COLOR: Rgba = Rgba::rgb(0x1e, 0x3a, 0xc8);
}

impl Scene for MainHub {
    fn id(&self) -> SceneId {
        SceneId::MainHub
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        super::fill_and_label(frame, area, Self::COLOR, self.id().display_name());
    }

    fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}
}
