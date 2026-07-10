use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use engine_render::camera::{AnyCamera, Camera, OrthographicCamera, PerspectiveCamera, WorldPos};
use engine_render::composite::{composite_scene, SpriteContent, SpriteDraw};
use engine_render::dots::{dots_to_grid, Dot, DotBuffer};
use engine_render::transform::{Transform, Vec2, VerticalAnchor};
use engine_render::tween::Tween;
use engine_render::{draw_grid, rasterize_shape, AnimatedSprite, ShapeKind};
use engine_core::color::Rgba;
use engine_core::Inspectable;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use crate::creatures::{AnimationKind, Creature};
use crate::scene_id::SceneId;


/// Single source of truth for the board's column count. Every downstream
/// consumer must reference this constant, never a bare literal `8`.
pub const BOARD_COLS: u16 = 7;
/// Single source of truth for the board's row count.
pub const BOARD_ROWS: u16 = 7;

/// World-x the Sideline camera anchors on — the board's horizontal center
/// column, derived from `BOARD_COLS` (never a bare literal `3.5`).
const BOARD_CENTER_COL: f32 = BOARD_COLS as f32 / 2.0;

/// Hard ceiling on any per-piece sprite/shadow dot dimension derived from
/// `Camera::local_dots_per_world_unit` (`shadow.rs`'s `shadow_buffers`,
/// `sizing.rs`'s `sprite_base_dot_rows_width_fill`). That rate is floored
/// against divide-by-zero at the projection layer (`NEAR_EPS`) but not
/// capped against blowing up — a free-roam camera passing very close to a
/// piece drives `forward_distance` down to that floor, spiking the rate
/// into the thousands, which turns a single frame's buffer allocation +
/// rasterization/resize into a multi-second stall (shipped as a real hang:
/// the game freezes whenever the camera passes near the board's midpoint,
/// where the demo pieces stand). A shadow/sprite this size would already be
/// many times the visible board's own width, so clamping loses nothing a
/// player could ever actually see. Odd, so `| 1` (oddness-forcing) never
/// pushes a capped value past the ceiling.
pub(crate) const MAX_SPRITE_DOT_DIMENSION: u32 = 401;

#[derive(Inspectable)]
pub struct BattleViewer {
    elapsed: f32,
    /// The 8 bundled creatures (`crate::creatures::all()`), index-matched to
    /// `Piece.index` (b5-t1). Sourced by `piece_sprite` in `render()`'s draw
    /// loop instead of a single shared sprite.
    #[inspect(hidden)]
    creatures: Vec<Creature>,
    /// Owned piece state, seeded once from `pieces()` at construction.
    /// `render()` reads each piece's own `transform`/`color` fields directly
    /// — mutating an entry here changes what the next `render()` draws.
    pub pieces: Vec<Piece>,
    /// Playback event sequence (b1-t2's `Event`/`EventKind`). Hidden from the
    /// inspector: playback-internal runtime state, not part of the
    /// `Piece`-level editable surface. Seeded from `demo_events()` (b3-t1);
    /// driven each frame by `update()`/`drive_events()`.
    #[inspect(hidden)]
    pub events: Vec<Event>,
    /// Transient, scene-internal bookkeeping documented on `Event` above (NOT
    /// part of the authored `Event` data): the piece's starting
    /// `Transform.translate`/`Transform.scale` `(x, y)` captured the frame an
    /// event's window opens, keyed by `piece_index`. Populated/consumed by
    /// `update()`'s per-event driving loop (b2-t2/b2-t3) and empty otherwise.
    #[inspect(hidden)]
    pub event_from_values: std::collections::HashMap<usize, (f32, f32)>,
    /// Indices (into `events`) of events whose window has fully elapsed and
    /// already been finalized (exact landing assigned, `event_from_values`
    /// entry cleared). Distinct from `event_from_values`'s presence/absence
    /// because that alone can't tell "never started" apart from "already
    /// finalized" once its entry is removed — this set is the single source
    /// of truth `update()`'s driving loop checks to guarantee an event is
    /// only ever finalized once, so it never re-fights a later external edit
    /// (e.g. an inspector edit) to the same piece's `transform`.
    #[inspect(hidden)]
    settled_events: std::collections::HashSet<usize>,
    /// Active camera mode (b5-t1). Not part of the inspectable surface —
    /// `BattleCamera` does not implement `Inspectable` — and never persists
    /// across a scene re-entry; `enter()` resets it to
    /// `Self::default_camera_mode()` every time.
    #[inspect(hidden)]
    camera_mode: BattleCamera,
    /// Camera-dependent rendering tuning constants (b2-t1). Not yet consumed
    /// by any renderer — wired in by b3-t1/b4-t2/b6-t1/b7-t1.
    pub tuning: BattleViewerTuning,
    /// Toggled by `t`/`T` in `handle_input` (case-insensitive, like the
    /// free-roam movement keys). Controls whether `render()` draws just the
    /// one-line "press t to toggle controls" hint (top-right corner) or
    /// also the expanded key-scheme reference beneath it. Debug/dev HUD
    /// state, not part of the inspectable surface.
    #[inspect(hidden)]
    show_controls_help: bool,
}

impl Default for BattleViewer {
    fn default() -> Self {
        Self {
            elapsed: 0.0,
            creatures: crate::creatures::all(),
            pieces: pieces(),
            // Hand-authored demo sequence (b3-t1); driven each frame by
            // `update()`/`drive_events()`.
            events: demo_events(),
            event_from_values: std::collections::HashMap::new(),
            settled_events: std::collections::HashSet::new(),
            camera_mode: Self::default_camera_mode(),
            tuning: BattleViewerTuning::default(),
            show_controls_help: false,
        }
    }
}

impl BattleViewer {
    /// Single source of truth for the starting camera, shared by `Default`
    /// and `Scene::enter()` so the two can never drift apart (b5-t1).
    fn default_camera_mode() -> BattleCamera {
        BattleCamera::sideline_preset()
    }

    /// Single source of truth for `index -> idle AnimatedSprite` (b5-t1).
    /// Used by `render()`'s per-piece draw loop instead of a single shared
    /// sprite.
    fn piece_sprite(&self, index: usize) -> Option<&AnimatedSprite> {
        self.creatures.get(index)?.animation(AnimationKind::Idle)
    }

    /// Shared drawable-piece iterator: alive pieces that have a sprite,
    /// paired with that sprite. `shadow_buffers` and `build_draws` both fold
    /// over this SAME iterator so their per-piece indices can never drift.
    fn drawable_pieces(&self) -> impl Iterator<Item = (&Piece, &AnimatedSprite)> {
        self.pieces
            .iter()
            .filter(|p| p.alive)
            .filter_map(|p| Some((p, self.piece_sprite(p.index)?)))
    }

    /// Discrete-nudge free-roam movement keys (spec 42 Decision 5, revised).
    /// One effect per keypress, no continuous/velocity state.
    ///
    /// Move/height letter keys are matched case-insensitively (`'w'` and
    /// `'W'` both apply): `crossterm` reports the literal character a
    /// keypress produces, which is lowercase for a normal press (no shift)
    /// and uppercase with shift/caps-lock — the original all-uppercase
    /// scheme meant a normal keypress matched NOTHING here (shipped bug: "I
    /// press the keys but nothing happens"), and matching lowercase only
    /// would reintroduce the same class of bug for shift/caps-lock users.
    /// Yaw/pitch are on the arrow keys, not letters at all: lowercase `'q'`
    /// specifically is intercepted upstream by `SceneManager::route_key` as
    /// the global quit signal before it ever reaches a scene, so no key
    /// scheme reachable by normal typing could ever put a real action on
    /// `q` — confirmed with the project owner that `q`/`Q` must never be
    /// bound to anything here. Arrow keys have no case and no such
    /// conflict.
    fn nudge_camera(cam: &mut PerspectiveCamera, code: crossterm::event::KeyCode) {
        use crossterm::event::KeyCode;
        match code {
            KeyCode::Char('w') | KeyCode::Char('W') => cam.nudge(0.5, 0.0, 0.0, 0.0, 0.0),
            KeyCode::Char('s') | KeyCode::Char('S') => cam.nudge(-0.5, 0.0, 0.0, 0.0, 0.0),
            KeyCode::Char('d') | KeyCode::Char('D') => cam.nudge(0.0, 0.5, 0.0, 0.0, 0.0),
            KeyCode::Char('a') | KeyCode::Char('A') => cam.nudge(0.0, -0.5, 0.0, 0.0, 0.0),
            KeyCode::Right => cam.nudge(0.0, 0.0, 5.0, 0.0, 0.0),
            KeyCode::Left => cam.nudge(0.0, 0.0, -5.0, 0.0, 0.0),
            KeyCode::Up => cam.nudge(0.0, 0.0, 0.0, 5.0, 0.0),
            KeyCode::Down => cam.nudge(0.0, 0.0, 0.0, -5.0, 0.0),
            KeyCode::Char('x') | KeyCode::Char('X') => cam.nudge(0.0, 0.0, 0.0, 0.0, 0.5),
            KeyCode::Char('z') | KeyCode::Char('Z') => cam.nudge(0.0, 0.0, 0.0, 0.0, -0.5),
            KeyCode::Char(']') => cam.scale_dots *= 1.1,
            KeyCode::Char('[') => cam.scale_dots *= 0.9,
            _ => {}
        }
    }

    /// Plain-terminal-text color for the controls-help hint/panel (CLAUDE.md:
    /// text always stays plain characters, never the dot pipeline).
    const HELP_TEXT_COLOR: Rgba = Rgba::rgb(0xff, 0xff, 0xff);

    /// Draws the always-visible one-line "press t to toggle controls" hint
    /// in `area`'s top-right corner, plus — only while
    /// `show_controls_help` is toggled on — the expanded key-scheme
    /// reference beneath it: camera-mode keys always, and (only while
    /// currently in Free-roam) the movement scheme too. Plain `Buffer`
    /// text, not the braille/dot pipeline (CLAUDE.md: text is the one
    /// exception to "everything renders through the dot pipeline") — drawn
    /// last in `render()` so it sits on top of the board, and free to
    /// overwrite whatever board content is beneath it (no background box,
    /// no dot-pipeline border — not worth the complexity for a debug hint).
    fn draw_controls_help(&self, buf: &mut Buffer, area: Rect) {
        let mut lines: Vec<&str> = vec!["press t to toggle controls"];
        if self.show_controls_help {
            lines.push("1 Sideline  2 Over-the-shoulder  3 Top-down  4 Free-roam");
            if matches!(self.camera_mode.fit, FitMode::Manual) {
                lines.push("move wasd  height z/x  look arrow keys  zoom [ ]");
            }
        }
        for (i, line) in lines.iter().enumerate() {
            let y = area.top() + i as u16;
            if y >= area.bottom() {
                break;
            }
            let w = (line.chars().count() as u16).min(area.width);
            let x = area.right().saturating_sub(w);
            engine_render::label(buf, Rect::new(x, y, w, 1), line, Self::HELP_TEXT_COLOR);
        }
    }
}

impl Scene for BattleViewer {
    fn id(&self) -> SceneKey {
        SceneId::BattleViewer.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {
        self.camera_mode = Self::default_camera_mode();
    }

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt.as_secs_f32();
        self.drive_events();
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let geom = board_geometry(area, self.camera_mode, self.tuning);
        draw_board_lines(frame.buffer_mut(), &geom);

        let elapsed = Duration::from_secs_f32(self.elapsed);
        let shadow_bufs = self.shadow_buffers(&geom);
        let draws = self.build_draws(&geom, &shadow_bufs, elapsed);

        let w = (geom.board_rect.width * 2) as usize;
        let h = (geom.board_rect.height * 4) as usize;
        let cam = geom.framed_camera();
        let grid = composite_scene(w, h, &cam, &draws);
        draw_grid(frame.buffer_mut(), geom.board_rect, &grid);

        self.draw_controls_help(frame.buffer_mut(), area);
    }

    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition> {
        use crossterm::event::KeyCode;
        if let InputEvent::Key(key) = ev {
            // Direct selection: each digit always picks the same view,
            // never a next/prev cycle (spec 37). 1=Sideline 2=OverShoulder 3=TopDown.
            match key.code {
                KeyCode::Char('1') => self.camera_mode = BattleCamera::sideline_preset(),
                KeyCode::Char('2') => self.camera_mode = BattleCamera::over_shoulder_preset(),
                KeyCode::Char('3') => self.camera_mode = BattleCamera::top_down_preset(),
                // Free-roam entry (spec 42 Decision 5): re-pressing '4' while
                // already in FreeRoam (`fit: FitMode::Manual`) must NOT
                // clobber a flown-to position/orientation back to the
                // starting preset.
                KeyCode::Char('4') if !matches!(self.camera_mode.fit, FitMode::Manual) => {
                    self.camera_mode = BattleCamera::free_roam_preset();
                }
                // Case-insensitive toggle for the controls-help HUD hint
                // (never `q`/`Q` — that's the global quit key, intercepted
                // upstream of this scene entirely).
                KeyCode::Char('t') | KeyCode::Char('T') => {
                    self.show_controls_help = !self.show_controls_help;
                }
                code => {
                    if matches!(self.camera_mode.fit, FitMode::Manual) {
                        if let AnyCamera::Perspective(p) = &mut self.camera_mode.camera {
                            Self::nudge_camera(p, code);
                        }
                    }
                }
            }
        }
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    fn inspect(&mut self) -> &mut dyn Inspectable {
        self
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: board_geometry (b1-t1)
// ─────────────────────────────────────────────────────────────────────────────


mod piece;
mod camera;
mod playback;
mod geometry;
mod grid;
mod sizing;
mod shadow;

pub(crate) use piece::*;
pub(crate) use camera::*;
pub(crate) use playback::*;
pub(crate) use geometry::*;
pub(crate) use grid::*;

#[cfg(test)]
mod scene_integration_tests;

#[cfg(test)]
mod creature_sourcing_tests {
    use super::*;

    /// DELIVERABLE (primary): `BattleViewer::default().creatures` is exactly
    /// `crate::creatures::all()`, in order — proves each `Piece.index` maps
    /// 1:1 to a distinct bundled creature, not one shared sprite.
    #[test]
    fn default_creatures_match_creatures_all_in_order() {
        let scene = BattleViewer::default();
        let expected: Vec<String> = crate::creatures::all()
            .iter()
            .map(|c| c.name().to_string())
            .collect();
        let actual: Vec<String> = scene.creatures.iter().map(|c| c.name().to_string()).collect();

        assert_eq!(
            actual, expected,
            "BattleViewer::default().creatures must equal crate::creatures::all(), in order"
        );
    }

    /// DELIVERABLE: `piece_sprite(p.index)` resolves to `Some` (the piece's
    /// own idle animation) for every piece the scene starts with — the
    /// per-piece draw loop must have a distinct sprite source for each of
    /// the 8 seeded pieces, not a shared fallback.
    #[test]
    fn piece_sprite_is_some_for_every_seeded_piece_index() {
        let scene = BattleViewer::default();

        for p in &scene.pieces {
            assert!(
                scene.piece_sprite(p.index).is_some(),
                "piece_sprite({}) must be Some for every seeded piece index",
                p.index
            );
        }
    }

    /// DELIVERABLE (render-seam distinctness): two different piece indices
    /// resolve to two different creatures' idle sprites (by name lookup
    /// through `creatures::all()`), not the same shared sprite instance.
    #[test]
    fn piece_sprite_differs_by_index_across_distinct_creatures() {
        let scene = BattleViewer::default();

        let all = crate::creatures::all();
        assert_ne!(
            all[0].name(),
            all[1].name(),
            "sanity: creatures::all()[0] and [1] must be distinct creatures"
        );

        let sprite0 = scene
            .piece_sprite(0)
            .expect("piece_sprite(0) must be Some");
        let sprite1 = scene
            .piece_sprite(1)
            .expect("piece_sprite(1) must be Some");

        assert_ne!(
            sprite0.frame_count(),
            0,
            "sanity: index-0 sprite must have at least one frame"
        );
        assert_ne!(
            sprite1.frame_count(),
            0,
            "sanity: index-1 sprite must have at least one frame"
        );

        // The two pieces' underlying creatures must be genuinely distinct
        // (per `scene.creatures[0].name() != scene.creatures[1].name()`),
        // proving `piece_sprite` sources per-index art rather than one
        // shared sprite repeated across every piece.
        assert_ne!(
            scene.creatures[0].name(),
            scene.creatures[1].name(),
            "piece_sprite(0) and piece_sprite(1) must source from distinct creatures"
        );
    }
}

#[cfg(test)]
mod controls_help_tests {
    use super::*;
    use crate::scenes::test_util::{key_event, rect_text, render_to_buffer};
    use crossterm::event::KeyCode;
    use engine_render::diff_dots;
    use ratatui::buffer::Buffer;

    const HINT: &str = "press t to toggle controls";

    /// Every cell of `buf`, concatenated row-major with no separators —
    /// enough to assert a single-line label's text appears somewhere,
    /// regardless of exactly which row/column it landed on.
    fn full_text(buf: &Buffer) -> String {
        rect_text(buf, buf.area)
    }

    /// Default state: the one-line hint is always visible; the expanded
    /// panel is not (nothing toggled it on yet).
    #[test]
    fn hint_visible_by_default_expanded_panel_is_not() {
        let scene = BattleViewer::default();
        assert!(!scene.show_controls_help);

        let buf = render_to_buffer(&scene, 80, 40);
        let text = full_text(&buf);
        assert!(text.contains(HINT), "expected hint {HINT:?} in default render, got {text:?}");
        assert!(
            !text.contains("Sideline"),
            "expanded panel (camera-mode key reference) must not appear before toggling"
        );
    }

    /// Pressing lowercase 't' toggles `show_controls_help` on, and the
    /// rendered output then includes the expanded panel's always-present
    /// camera-mode key reference alongside the hint.
    #[test]
    fn t_toggles_expanded_panel_showing_camera_modes() {
        let mut scene = BattleViewer::default();

        scene.handle_input(key_event(KeyCode::Char('t')));
        assert!(scene.show_controls_help);

        let buf = render_to_buffer(&scene, 80, 40);
        let text = full_text(&buf);
        assert!(text.contains(HINT));
        assert!(text.contains("Sideline"));
        assert!(text.contains("Over-the-shoulder"));
        assert!(text.contains("Top-down"));
        assert!(text.contains("Free-roam"));
    }

    /// Uppercase 'T' also toggles, matching the case-insensitivity of the
    /// free-roam movement keys.
    #[test]
    fn uppercase_t_also_toggles() {
        let mut scene = BattleViewer::default();
        scene.handle_input(key_event(KeyCode::Char('T')));
        assert!(scene.show_controls_help, "'T' (uppercase) must toggle the help panel too");
    }

    /// Toggling 't' twice returns to the untoggled state: the field is back
    /// to `false`, and the rendered output (both the plain-text cells and
    /// the board's own braille dots) is identical to a render that was
    /// never toggled at all.
    #[test]
    fn toggling_twice_returns_to_untoggled_state() {
        let scene = BattleViewer::default();
        let before = render_to_buffer(&scene, 80, 40);

        let mut toggled_twice = BattleViewer::default();
        toggled_twice.handle_input(key_event(KeyCode::Char('t')));
        toggled_twice.handle_input(key_event(KeyCode::Char('t')));
        assert!(!toggled_twice.show_controls_help);

        let after = render_to_buffer(&toggled_twice, 80, 40);
        assert!(
            diff_dots(&before, &after).is_match(),
            "board dots must be identical after toggling 't' twice"
        );
        assert_eq!(
            full_text(&before),
            full_text(&after),
            "help text must be identical after toggling 't' twice"
        );
    }

    /// The movement-scheme line only appears while BOTH the panel is
    /// expanded AND the camera is currently in Free-roam — never under a
    /// fixed preset, even with the panel expanded.
    #[test]
    fn movement_scheme_only_shown_in_free_roam() {
        let mut free_roam_scene = BattleViewer {
            camera_mode: BattleCamera::free_roam_preset(),
            ..Default::default()
        };
        free_roam_scene.handle_input(key_event(KeyCode::Char('t')));
        let free_roam_text = full_text(&render_to_buffer(&free_roam_scene, 80, 40));
        assert!(
            free_roam_text.contains("wasd"),
            "expected the movement scheme in the expanded panel while in Free-roam"
        );

        let mut sideline_scene = BattleViewer {
            camera_mode: BattleCamera::sideline_preset(),
            ..Default::default()
        };
        sideline_scene.handle_input(key_event(KeyCode::Char('t')));
        let sideline_text = full_text(&render_to_buffer(&sideline_scene, 80, 40));
        assert!(
            !sideline_text.contains("wasd"),
            "movement scheme must not appear under a fixed (non-Free-roam) preset"
        );
    }
}
