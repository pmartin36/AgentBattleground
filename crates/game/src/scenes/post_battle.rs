use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use engine_core::color::Rgba;
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::scene_id::SceneId;

#[derive(Default, Inspectable)]
pub struct PostBattle;

impl PostBattle {
    pub const COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00);
}

impl Scene for PostBattle {
    fn id(&self) -> SceneKey {
        SceneId::PostBattle.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        super::fill_and_label(frame, area, Self::COLOR, SceneId::PostBattle.display_name());
    }

    fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
        self
    }
}
