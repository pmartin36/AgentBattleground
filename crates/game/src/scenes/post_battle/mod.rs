use std::cell::RefCell;
use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use engine_core::color::Rgba;
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::scene_id::SceneId;

/// Battle result — drives the title band's text + color (b3-t1). Only
/// `Victory` is currently reachable (hardcoded in `seed()`); `Defeat` is
/// exercised directly in tests by mutating `outcome`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Outcome {
    #[default]
    Victory,
    Defeat,
}

/// A single post-battle reward item (b4-t6 renders these; this task only
/// seeds them).
pub struct Spoil {
    pub icon: &'static [u8],
    pub description: String,
}

#[derive(Inspectable)]
pub struct PostBattle {
    #[inspect(hidden)]
    outcome: Outcome,
    #[inspect(hidden)]
    creatures: Vec<crate::creatures::Creature>,
    #[inspect(hidden)]
    xp_gained: [u32; 4],
    #[inspect(hidden)]
    elapsed: Duration,
    // Not yet read (b4-t5 selection glow ring is the first consumer).
    #[allow(dead_code)]
    #[inspect(hidden)]
    selected_index: usize,
    #[inspect(hidden)]
    spoils: Vec<Spoil>,
    /// Top-right button that transitions back to `MainHub`. `RefCell`
    /// because `render(&self, ..)` must mutate its rect/state from an
    /// immutable receiver (mirrors `RosterManager::home_button`).
    #[inspect(hidden)]
    home_button: RefCell<engine_render::Button>,
}

impl PostBattle {
    pub const TITLE_COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00);
    pub const DEFEAT_COLOR: Rgba = Rgba::rgb(0xff, 0x30, 0x30);
    pub const XP_ANIM_DUR: Duration = Duration::from_millis(1200);
    /// Fraction of the screen height reserved for the spoils band (b4-t6).
    pub const SPOILS_BAND_FRAC: f32 = 0.25;
    /// Pulse period of the selection glow ring (b4-t5).
    pub const GLOW_PERIOD: Duration = Duration::from_millis(1000);
    /// Number of discrete color steps the glow pulse quantizes into (b4-t5).
    pub const GLOW_STEPS: u32 = 5;

    const HOME_W: u16 = 6;
    const HOME_H: u16 = 3;
    const EDGE_MARGIN: u16 = 1;
    /// Height (cells) of the title band: `braille_name::GLYPH_H` (8 dots = 2
    /// cells) + 1 cell margin above and below.
    const TITLE_BAND_H: u16 = 4;

    /// Single source of the scene's placeholder data, fed to both `new()`
    /// and `enter()` so they never drift apart. `xp_gained` is
    /// contract-locked by `creatures::demo_roster_seeded_xp_supports_two_rollovers`
    /// (creatures.rs) — do not change these deltas without re-checking that
    /// test.
    fn seed() -> (Vec<crate::creatures::Creature>, [u32; 4], Vec<Spoil>) {
        let creatures = crate::creatures::demo_roster().into_iter().take(4).collect();
        let xp_gained = [50, 30, 40, 25];
        let spoils = vec![
            Spoil {
                icon: crate::assets::ICON_SPOIL_CANDY,
                description: "Spoil 1".to_string(),
            },
            Spoil {
                icon: crate::assets::ICON_SPOIL_CANDY,
                description: "Spoil 2".to_string(),
            },
        ];
        (creatures, xp_gained, spoils)
    }

    pub fn new() -> Self {
        let (creatures, xp_gained, spoils) = Self::seed();
        Self {
            outcome: Outcome::Victory,
            creatures,
            xp_gained,
            elapsed: Duration::ZERO,
            selected_index: 0,
            spoils,
            home_button: RefCell::new(engine_render::Button::new(
                Rect::default(),
                crate::assets::BUTTON_PANEL,
                crate::assets::ICON_HOME,
            )),
        }
    }

    /// Dot-space top-right geometry for the home button, mirroring
    /// `RosterManager::home_dot_rect` (chrome.rs) — inset from the right/top
    /// edges of `area` by `EDGE_MARGIN` cells, sized `HOME_W`x`HOME_H` cells.
    fn home_dot_rect(&self, area: Rect) -> engine_render::DotRect {
        let a = engine_render::DotRect {
            x: area.x as i32 * 2,
            y: area.y as i32 * 4,
            w: area.width as i32 * 2,
            h: area.height as i32 * 4,
        };
        let inner = a.inset(0, Self::EDGE_MARGIN as i32 * 2, Self::EDGE_MARGIN as i32 * 4, 0);
        let w = Self::HOME_W as i32 * 2;
        let h = Self::HOME_H as i32 * 4;
        engine_render::DotRect { x: inner.x + inner.w - w, y: inner.y, w, h }
    }

    /// Cell-space view of `home_dot_rect`, for tests only (mirrors
    /// `RosterManager::home_rect`).
    #[cfg(test)]
    fn home_rect(&self, area: Rect) -> Rect {
        self.home_dot_rect(area).to_cell_rect()
    }
}

impl Default for PostBattle {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene for PostBattle {
    fn id(&self) -> SceneKey {
        SceneId::PostBattle.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {
        let (creatures, xp_gained, spoils) = Self::seed();
        self.creatures = creatures;
        self.xp_gained = xp_gained;
        self.spoils = spoils;
        self.elapsed = Duration::ZERO;
    }

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt;
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let band = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: Self::TITLE_BAND_H.min(area.height),
        };
        let (text, color) = match self.outcome {
            Outcome::Victory => ("VICTORY", Self::TITLE_COLOR),
            Outcome::Defeat => ("DEFEAT", Self::DEFEAT_COLOR),
        };
        crate::braille_name::draw_name(frame.buffer_mut(), band, text, color);

        let dr = self.home_dot_rect(area);
        let mut b = self.home_button.borrow_mut();
        b.set_rect(dr.to_cell_rect());
        b.set_dot_offset_down(dr.cell_remainder().1);
        b.render(frame.buffer_mut());
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use ratatui::crossterm::event::KeyCode;

        match ev {
            InputEvent::Key(key) if key.code == KeyCode::Esc => Some(Transition {
                target: SceneId::MainHub.into(),
                params: None,
            }),
            InputEvent::Mouse(me) => {
                if self.home_button.get_mut().handle_mouse(&me) {
                    Some(Transition {
                        target: SceneId::MainHub.into(),
                        params: None,
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::test_util::{key_event, mouse_event, render_to_buffer};
    use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};

    fn title_band(area: Rect) -> Rect {
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: PostBattle::TITLE_BAND_H.min(area.height),
        }
    }

    /// First lit braille cell found in `band` (row-major), decoded via the
    /// engine's own `decode_braille_cell` — the CLAUDE.md-mandated way to
    /// check rendered color, never a raw `Cell::fg` comparison.
    fn first_lit_color(buf: &ratatui::buffer::Buffer, band: Rect) -> Option<Rgba> {
        (band.top()..band.bottom())
            .flat_map(|y| (band.left()..band.right()).map(move |x| (x, y)))
            .find_map(|(x, y)| engine_render::decode_braille_cell(buf, x, y).map(|(_, color)| color))
    }

    fn any_lit_cell(buf: &ratatui::buffer::Buffer, band: Rect) -> bool {
        (band.top()..band.bottom())
            .flat_map(|y| (band.left()..band.right()).map(move |x| (x, y)))
            .any(|(x, y)| engine_render::decode_braille_cell(buf, x, y).is_some())
    }

    /// A default (Victory) scene paints lit braille cells in the title band,
    /// colored `TITLE_COLOR`.
    #[test]
    fn victory_title_paints_amber_lit_cells() {
        let scene = PostBattle::new();
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);
        let area = Rect::new(0, 0, w, h);
        let band = title_band(area);

        assert!(
            any_lit_cell(&buf, band),
            "victory title band must paint at least one lit braille cell"
        );
        let color = first_lit_color(&buf, band)
            .expect("title band must have a lit cell to sample color from");
        assert_eq!(color, PostBattle::TITLE_COLOR, "victory title text must be colored TITLE_COLOR");
    }

    /// Flipping `outcome` to `Defeat` changes both the rendered text (proven
    /// via a differing lit-dot pattern from the Victory render) and the
    /// color to `DEFEAT_COLOR`.
    #[test]
    fn defeat_outcome_switches_title_text_and_color() {
        let mut scene = PostBattle::new();
        scene.outcome = Outcome::Defeat;
        let (w, h) = (40u16, 20u16);
        let buf = render_to_buffer(&scene, w, h);
        let area = Rect::new(0, 0, w, h);
        let band = title_band(area);

        assert!(
            any_lit_cell(&buf, band),
            "defeat title band must paint at least one lit braille cell"
        );
        let color = first_lit_color(&buf, band)
            .expect("title band must have a lit cell to sample color from");
        assert_eq!(color, PostBattle::DEFEAT_COLOR, "defeat title text must be colored DEFEAT_COLOR");

        let victory_scene = PostBattle::new();
        let victory_buf = render_to_buffer(&victory_scene, w, h);
        let victory_pattern: Vec<Option<u8>> = (band.top()..band.bottom())
            .flat_map(|y| (band.left()..band.right()).map(move |x| (x, y)))
            .map(|(x, y)| engine_render::decode_braille_cell(&victory_buf, x, y).map(|(mask, _)| mask))
            .collect();
        let defeat_pattern: Vec<Option<u8>> = (band.top()..band.bottom())
            .flat_map(|y| (band.left()..band.right()).map(move |x| (x, y)))
            .map(|(x, y)| engine_render::decode_braille_cell(&buf, x, y).map(|(mask, _)| mask))
            .collect();
        assert_ne!(
            victory_pattern, defeat_pattern,
            "DEFEAT must paint a different lit-dot pattern than VICTORY, proving the text branch (not just color)"
        );
    }

    /// A completed click (Down then Up, both inside the home button's rect,
    /// after a prior render sets that rect) returns a `Transition` to
    /// `MainHub` with no params.
    #[test]
    fn home_button_click_transitions_to_main_hub() {
        let mut scene = PostBattle::new();
        let (w, h) = (40u16, 20u16);
        let _ = render_to_buffer(&scene, w, h);

        let area = Rect::new(0, 0, w, h);
        let rect = scene.home_rect(area);
        let (cx, cy) = (rect.x, rect.y);

        scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
        let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

        let t = t.expect("a completed click on the home button must return a Transition");
        assert_eq!(
            t.target,
            SceneKey::from(SceneId::MainHub),
            "home button must transition to MainHub"
        );
        assert!(t.params.is_none(), "home button transition must carry no params");
    }

    /// `Esc` returns a `Transition` to `MainHub`; an unrelated key returns
    /// `None`.
    #[test]
    fn esc_key_transitions_to_main_hub() {
        let mut scene = PostBattle::new();

        let t = scene
            .handle_input(key_event(KeyCode::Esc))
            .expect("Esc must return a Transition");
        assert_eq!(
            t.target,
            SceneKey::from(SceneId::MainHub),
            "Esc must transition to MainHub"
        );

        let none = scene.handle_input(key_event(KeyCode::Char('x')));
        assert!(none.is_none(), "an unrelated key must not return a Transition");
    }

    /// `enter` reseeds four creatures, the pinned `xp_gained` deltas, two
    /// spoils, and resets `elapsed` to zero even after `update` has advanced
    /// it.
    #[test]
    fn enter_resets_elapsed_and_seeds_four_creatures() {
        let mut scene = PostBattle::new();
        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(500));
        assert!(scene.elapsed > Duration::ZERO);

        scene.enter(&mut ctx, None);

        assert_eq!(scene.elapsed, Duration::ZERO, "enter must reset elapsed to zero");
        assert_eq!(scene.creatures.len(), 4, "enter must seed exactly 4 creatures");
        assert_eq!(
            scene.xp_gained,
            [50, 30, 40, 25],
            "enter must seed the pinned xp_gained deltas"
        );
        assert_eq!(scene.spoils.len(), 2, "enter must seed exactly 2 spoils");
    }
}
