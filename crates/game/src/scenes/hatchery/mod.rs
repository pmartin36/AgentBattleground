//! Hatchery scene shell: reachable from the roster, with a back button that
//! returns to it. Egg tray rendering, lifecycle and focus interaction land
//! in later scenes of this module.

use std::cell::RefCell;
use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::Frame;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use engine_core::Inspectable;
use engine_core::SceneKey;

#[derive(Inspectable)]
pub struct Hatchery {
    /// Loaded from the player-data store; read by the egg tray render.
    #[allow(dead_code)]
    #[inspect(hidden)]
    eggs: Vec<crate::player_data::Egg>,
    /// Kept for the promotion/persist path; not written to yet.
    #[allow(dead_code)]
    #[inspect(hidden)]
    store: Option<crate::player_data::PlayerStore>,
    #[inspect(hidden)]
    back_button: RefCell<engine_render::ButtonCore>,
}

impl Hatchery {
    /// Store-less constructor for hermetic tests: no eggs, no persistence.
    pub fn new() -> Self {
        Self {
            eggs: Vec::new(),
            store: None,
            back_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
        }
    }

    /// Store-backed construction used by the production scene factory:
    /// loads `eggs` from the persisted `PlayerData` and keeps `store`. Reads
    /// only — never writes (roster seeding/persistence is `RosterManager`'s
    /// responsibility, and calling `store.save` here would clobber it if
    /// Hatchery is ever constructed first).
    pub fn from_store(store: crate::player_data::PlayerStore) -> Self {
        let data = store
            .load(|| crate::player_data::PlayerData {
                roster: Vec::new(),
                eggs: Vec::new(),
            })
            .into_data();
        Self {
            eggs: data.eggs,
            store: Some(store),
            back_button: RefCell::new(engine_render::ButtonCore::new(Rect::default())),
        }
    }

    /// Background fill color — teal, distinct from every other scene's fill.
    const COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0x1a, 0x66, 0x66);

    /// Dot-space placement of the back button within `area`; reuses the
    /// roster's shared home-button geometry (same top-right slot, since the
    /// back button plays the same navigational role here).
    fn back_dot_rect(area: Rect) -> engine_render::DotRect {
        crate::scenes::home_button::home_dot_rect(area)
    }
}

impl Default for Hatchery {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for Hatchery {
    fn id(&self) -> SceneKey {
        crate::scene_id::SceneId::Hatchery.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        engine_render::fill(frame.buffer_mut(), area, Self::COLOR);

        let dr = Self::back_dot_rect(area);
        let mut b = self.back_button.borrow_mut();
        b.set_rect(dr.to_cell_rect());
        crate::scenes::home_button::draw_home_button(frame.buffer_mut(), dr, b.state());
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        if let InputEvent::Mouse(me) = ev {
            if self.back_button.get_mut().handle_mouse(&me) {
                return Some(Transition {
                    target: crate::scene_id::SceneId::RosterManager.into(),
                    params: None,
                });
            }
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn Inspectable {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_id::SceneId;
    use crate::scenes::test_util::{mouse_event, render_to_buffer};
    use engine_core::scene::Scene;
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};

    #[test]
    fn hatchery_id_is_hatchery() {
        let scene = Hatchery::new();
        assert_eq!(scene.id(), SceneKey::from(SceneId::Hatchery));
    }

    /// A completed click (Moved+Down+Up) on the back button returns a
    /// `Transition` to `RosterManager` with no params.
    #[test]
    fn back_button_click_transitions_to_roster_manager() {
        let mut scene = Hatchery::new();
        let (w, h) = (40u16, 20u16);
        // Render once so the button's rect is set for this frame (handle_input
        // hit-tests against the previous frame's render, mirroring the
        // roster's home-button pattern).
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = crate::scenes::home_button::home_dot_rect(area).to_cell_rect();
        let (cx, cy) = (rect.x, rect.y);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        let t = t.expect("a completed click on the back button must return a Transition");
        assert_eq!(
            t.target,
            SceneKey::from(SceneId::RosterManager),
            "back button must transition to RosterManager"
        );
        assert!(t.params.is_none(), "back button transition must carry no params");
    }
}
