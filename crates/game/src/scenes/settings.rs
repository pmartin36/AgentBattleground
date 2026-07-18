use std::cell::RefCell;
use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::Frame;

use engine_core::color::Rgba;
use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use crate::scene_id::SceneId;

#[derive(Inspectable)]
pub struct Settings {
    #[inspect(hidden)]
    home_button: RefCell<engine_render::ButtonCore>,
}

impl Settings {
    pub const COLOR: Rgba = Rgba::rgb(0x5a, 0x63, 0x8c);
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            home_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
        }
    }
}

impl Scene for Settings {
    fn id(&self) -> SceneKey {
        SceneId::Settings.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        super::fill_and_label(frame, area, Self::COLOR, SceneId::Settings.display_name());

        let home_dr = crate::scenes::home_button::home_dot_rect(area);
        let mut home = self.home_button.borrow_mut();
        home.set_rect(home_dr.to_cell_rect());
        crate::scenes::home_button::draw_home_button(frame.buffer_mut(), home_dr, home.state());
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        if let InputEvent::Mouse(me) = ev {
            if self.home_button.get_mut().handle_mouse(&me) {
                return Some(Transition {
                    target: SceneId::MainHub.into(),
                    params: None,
                });
            }
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::test_util::{has_non_space, mouse_event, render_to_buffer};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    /// `render()` paints the shared home button within its rect (from
    /// `home_button::home_dot_rect`), on top of the COLOR fill.
    #[test]
    fn render_paints_home_button() {
        let scene = Settings::default();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = crate::scenes::home_button::home_dot_rect(area).to_cell_rect();

        assert!(
            has_non_space(&buf, rect),
            "home button must paint at least one non-space cell within its rect"
        );
    }

    /// A completed click (Moved+Down+Up, all inside the home button's rect)
    /// returns a `Transition` to `MainHub` with no params.
    #[test]
    fn home_click_transitions_to_main_hub() {
        let mut scene = Settings::default();
        let (w, h) = (40u16, 20u16);
        // Render once so the button's rect is set to this frame's `area`
        // (handle_input hit-tests against the PREVIOUS frame's render).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = crate::scenes::home_button::home_dot_rect(area).to_cell_rect();
        let (cx, cy) = (rect.x, rect.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        let t = t.expect("a completed click on the home button must return a Transition");
        assert_eq!(t.target, SceneKey::from(SceneId::MainHub), "home button must transition to MainHub");
        assert!(t.params.is_none(), "home button transition must carry no params");
    }
}
