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

mod bars;
mod columns;
mod glow;
mod spoils;

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
    /// Portrait frame border color (b4-t1) — muted slate, distinct from
    /// `TITLE_COLOR`/`DEFEAT_COLOR`. TBD placeholder.
    pub const FRAME_COLOR: Rgba = Rgba::rgb(0x80, 0x80, 0x90);
    pub const XP_ANIM_DUR: Duration = Duration::from_millis(1200);
    /// XP bar fill color (b4-t3) — blue, distinct from the amber title and
    /// the green/yellow/red stamina bands (b4-t4). TBD placeholder.
    pub const XP_BAR_COLOR: Rgba = Rgba::rgb(0x40, 0xa0, 0xff);
    /// Row label text color (b4-t3's "LVL"/"XP" and b4-t4's "STA"). TBD
    /// placeholder light grey.
    pub const LABEL_COLOR: Rgba = Rgba::rgb(0xd0, 0xd0, 0xd0);
    /// Stamina bar fill color for `percent > 50` (b4-t4). TBD placeholder
    /// green, distinct from every other scene color.
    pub const STAMINA_HIGH_COLOR: Rgba = Rgba::rgb(0x40, 0xc0, 0x60);
    /// Stamina bar fill color for `15..=50` (b4-t4). TBD placeholder yellow.
    pub const STAMINA_MID_COLOR: Rgba = Rgba::rgb(0xe0, 0xc0, 0x40);
    /// Stamina bar fill color for `< 15` (b4-t4). TBD placeholder red.
    pub const STAMINA_LOW_COLOR: Rgba = Rgba::rgb(0xe0, 0x50, 0x40);
    /// Fraction of the screen height reserved for the spoils band (b4-t6).
    pub const SPOILS_BAND_FRAC: f32 = 0.25;
    /// Pulse period of the selection glow ring (b4-t5).
    pub const GLOW_PERIOD: Duration = Duration::from_millis(1000);
    /// Number of discrete color steps the glow pulse quantizes into (b4-t5).
    pub const GLOW_STEPS: u32 = 5;
    /// Selection glow ring's dim endpoint color (b4-t5) — `TITLE_COLOR` toward
    /// the background. TBD placeholder.
    pub const GLOW_DIM_COLOR: Rgba = Rgba::rgb(0x4a, 0x38, 0x00);
    /// Selection glow ring's bright endpoint color (b4-t5) — `TITLE_COLOR`
    /// toward bright. TBD placeholder.
    pub const GLOW_BRIGHT_COLOR: Rgba = Rgba::rgb(0xff, 0xe6, 0x80);

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

    /// Single-source top-to-bottom split of the full render `area`: the
    /// title band (cell rect, `TITLE_BAND_H` cells), the creature-column band
    /// (dot-precise, the gap between title and spoils), and the spoils band
    /// (dot-precise, the bottom `SPOILS_BAND_FRAC` of the screen). b4-t6's
    /// spoils render must consume the same `spoils` rect this returns, never
    /// recompute it.
    fn bands(&self, area: Rect) -> (Rect, engine_render::DotRect, engine_render::DotRect) {
        let title = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: Self::TITLE_BAND_H.min(area.height),
        };

        let area_dots = engine_render::DotRect {
            x: area.x as i32 * 2,
            y: area.y as i32 * 4,
            w: area.width as i32 * 2,
            h: area.height as i32 * 4,
        };
        let title_h_dots = title.height as i32 * 4;
        let spoils_h_dots = (area_dots.h as f32 * Self::SPOILS_BAND_FRAC).round() as i32;
        let spoils = engine_render::DotRect {
            x: area_dots.x,
            y: area_dots.y + area_dots.h - spoils_h_dots,
            w: area_dots.w,
            h: spoils_h_dots,
        };
        let creatures = engine_render::DotRect {
            x: area_dots.x,
            y: area_dots.y + title_h_dots,
            w: area_dots.w,
            h: (spoils.y - (area_dots.y + title_h_dots)).max(0),
        };

        (title, creatures, spoils)
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
        let (title, creature_area, spoils) = self.bands(area);
        let (text, color) = match self.outcome {
            Outcome::Victory => ("VICTORY", Self::TITLE_COLOR),
            Outcome::Defeat => ("DEFEAT", Self::DEFEAT_COLOR),
        };
        crate::braille_name::draw_name(frame.buffer_mut(), title, text, color);

        let dr = self.home_dot_rect(area);
        let mut b = self.home_button.borrow_mut();
        b.set_rect(dr.to_cell_rect());
        b.set_dot_offset_down(dr.cell_remainder().1);
        b.render(frame.buffer_mut());

        columns::render(self, frame.buffer_mut(), creature_area);
        spoils::render(self, frame.buffer_mut(), spoils);
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
    use crate::scenes::test_util::{blit_footprint_cell_rect, key_event, mouse_event, render_to_buffer};
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

    /// (b4-t5 iter3) The frame and ring occupy DIFFERENT dot positions but
    /// routinely share the same 2x4 braille CELL at their boundary. A cell
    /// that is a genuine dot-merge decodes each pure color only in cells that
    /// are NOT shared; shared cells blend. So the correct composed-render
    /// check is an EXISTENCE scan, never a single fixed shared-cell probe:
    /// column 0's frame perimeter must still show at least one cell decoding
    /// to `FRAME_COLOR` (the frame survives, isn't fully recolored) AND
    /// column 0's ring band (the 1-dot margin strictly outside `frame`) must
    /// show at least one cell decoding to `glow_color(scene.elapsed)` (the
    /// ring is present). Both must hold in the SAME render.
    #[test]
    fn composed_render_shows_ring_and_intact_frame_on_selected_column() {
        let scene = PostBattle::new();
        let (w, h) = (80u16, 40u16);
        let buf = render_to_buffer(&scene, w, h);
        let area = Rect::new(0, 0, w, h);

        let (_, creature_area, _) = scene.bands(area);
        let layouts = columns::column_layouts(creature_area, columns::N_COLUMNS);
        let frame = layouts[0].frame;

        assert!(
            frame_perimeter_has_frame_color(&buf, frame),
            "column 0's frame perimeter must still show a FRAME_COLOR cell; \
             the ring must not fully recolor the frame border"
        );
        assert!(
            ring_band_has_glow_color(&buf, frame, glow::glow_color(scene.elapsed)),
            "column 0's ring band must show a glow_color cell; the ring must be present"
        );
    }

    /// Columns 1..=3 (not selected) show no glow ring in their ring band, and
    /// their frame border still shows `FRAME_COLOR` (no ring to contend
    /// with).
    #[test]
    fn composed_render_no_ring_on_unselected_columns() {
        let scene = PostBattle::new();
        let (w, h) = (80u16, 40u16);
        let buf = render_to_buffer(&scene, w, h);
        let area = Rect::new(0, 0, w, h);

        let (_, creature_area, _) = scene.bands(area);
        let layouts = columns::column_layouts(creature_area, columns::N_COLUMNS);

        for (i, l) in layouts.iter().enumerate().skip(1) {
            let frame = l.frame;
            assert!(
                !ring_band_has_glow_color(&buf, frame, glow::glow_color(scene.elapsed)),
                "column {i} (not selected) must show no glow_color cell in its ring band"
            );
            assert!(
                frame_perimeter_has_frame_color(&buf, frame),
                "column {i} frame perimeter must still show a FRAME_COLOR cell"
            );
        }
    }

    /// Two composed renders a half-period apart (`elapsed = 0` and
    /// `GLOW_PERIOD / 2`, advanced via `Scene::update`) show column 0's ring
    /// band lit at each elapsed's own `glow_color`, and the two colors
    /// differ — proving the pulse survives full composition.
    #[test]
    fn composed_render_ring_color_pulses_across_elapsed() {
        let (w, h) = (80u16, 40u16);
        let area = Rect::new(0, 0, w, h);

        let scene0 = PostBattle::new();
        let buf0 = render_to_buffer(&scene0, w, h);
        let (_, creature_area, _) = scene0.bands(area);
        let frame = columns::column_layouts(creature_area, columns::N_COLUMNS)[0].frame;

        let color0 = glow::glow_color(Duration::ZERO);
        assert!(
            ring_band_has_glow_color(&buf0, frame, color0),
            "column 0's ring band must show glow_color(0) at elapsed=0"
        );

        let mut scene1 = PostBattle::new();
        let mut ctx = EngineCtx;
        scene1.update(&mut ctx, PostBattle::GLOW_PERIOD / 2);
        let buf1 = render_to_buffer(&scene1, w, h);

        let color1 = glow::glow_color(PostBattle::GLOW_PERIOD / 2);
        assert!(
            ring_band_has_glow_color(&buf1, frame, color1),
            "column 0's ring band must show glow_color(GLOW_PERIOD/2) after advancing elapsed"
        );

        assert_ne!(color0, color1, "ring color must differ a half-period apart, proving the pulse");
    }

    /// True if any cell within `frame`'s actual blit footprint (see
    /// `blit_footprint_cell_rect` — NOT `to_cell_rect()`, which floors and can
    /// under-report the trailing edge by one cell) decodes to
    /// `PostBattle::FRAME_COLOR` — the frame border survives, isn't fully
    /// recolored by an overlapping ring.
    fn frame_perimeter_has_frame_color(buf: &ratatui::buffer::Buffer, frame: engine_render::DotRect) -> bool {
        let r = blit_footprint_cell_rect(frame);
        (r.top()..r.bottom())
            .flat_map(|y| (r.left()..r.right()).map(move |x| (x, y)))
            .any(|(x, y)| {
                engine_render::decode_braille_cell(buf, x, y).map(|(_, c)| c)
                    == Some(PostBattle::FRAME_COLOR)
            })
    }

    /// True if any cell strictly OUTSIDE `frame`'s blit footprint but within
    /// its 1-dot-outset ring's own blit footprint decodes to `expected` — the
    /// ring is present. Restricted to cells outside `frame` so interior
    /// sprite/bar pixels can't be mistaken for the ring. Uses
    /// `blit_footprint_cell_rect` (not `to_cell_rect()`) for both rects so the
    /// scan reaches the true trailing-edge cells `blit_dots` actually paints.
    fn ring_band_has_glow_color(
        buf: &ratatui::buffer::Buffer,
        frame: engine_render::DotRect,
        expected: Rgba,
    ) -> bool {
        let m = columns::GLOW_MARGIN_DOTS;
        let ring = frame.inset(-m, -m, -m, -m);
        let ring_r = blit_footprint_cell_rect(ring);
        let frame_r = blit_footprint_cell_rect(frame);
        (ring_r.top()..ring_r.bottom())
            .flat_map(|y| (ring_r.left()..ring_r.right()).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                !(x >= frame_r.left() && x < frame_r.right() && y >= frame_r.top() && y < frame_r.bottom())
            })
            .any(|(x, y)| engine_render::decode_braille_cell(buf, x, y).map(|(_, c)| c) == Some(expected))
    }
}
