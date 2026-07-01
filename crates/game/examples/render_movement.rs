//! render_movement — wandering wizards (dot-compositor-camera demo).
//!
//! Three wizards drift along deterministic Lissajous paths in world x/y through
//! the full dot-level pipeline: world position → SideView camera → dot offset →
//! DotBuffer compositor → braille → terminal. Because `depth = screen-y`, a
//! wizard moving down in y passes IN FRONT OF one higher up, so occlusion
//! updates live — and the motion is sub-cell (dot-granularity), not cell-snapping.
//!
//! Modes:
//!   - **Interactive** (TTY, no `--once`): the live 30 fps loop via `game::run`.
//!   - **Headless** (`--once` or non-TTY): renders the scene at two close times
//!     (both inside the first GIF frame window, so the animation frame is FROZEN
//!     and the only change is sub-cell movement), separated by `---FRAME-BREAK---`,
//!     then exits 0.

use std::f32::consts::TAU;
use std::io::{self, IsTerminal};
use std::time::Duration;

use image::GenericImageView;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Frame;
use ratatui::Terminal;
use scene_core::scene_id::SceneId;
use serde_json::Value as JsonValue;

use game::scene::{EngineCtx, InputEvent, Scene, Transition};
use render::camera::{Camera, SideView, WorldPos};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Uniform per-frame GIF duration. Defines the frame window; both headless
/// samples fall inside [0, FRAME_DUR) so every wizard stays on GIF frame 0.
const FRAME_DUR: Duration = Duration::from_millis(100);

/// Separator between the two headless frames. No braille codepoint / no ESC byte.
const MARKER: &str = "---FRAME-BREAK---";

/// Headless terminal dimensions (parity with the tier examples).
const W: u16 = 50;
const H: u16 = 30;

/// Camera scale: dots per world unit.
const SCALE_DOTS: f32 = 2.0;

/// Wizard height in dots — small enough that three fit and overlap.
const WIZARD_DOT_ROWS: u32 = 40;

/// Per-wizard animation speed multipliers (also the wizard count). All ≤ 1.8 so
/// that within the headless window [0, SAMPLE_B] every wizard stays on GIF
/// frame 0 (SAMPLE_B × 1.8 / FRAME_DUR = 0.9 < 1) — the two headless captures
/// therefore differ by MOVEMENT only, not animation.
const SPEEDS: [f32; 3] = [0.8, 1.3, 1.8];

/// Spatial phase per wizard — spread so they start in distinct spots and cross
/// as the Lissajous curves self-intersect over time.
const PHASES: [f32; 3] = [0.0, TAU / 4.0, TAU / 2.0];

// Deterministic Lissajous wander (world units and radians/second).
const CX: f32 = 22.0;
const CY: f32 = 22.0;
const AX: f32 = 14.0;
const AY: f32 = 8.0;
const WX: f32 = 0.7;
const WY: f32 = 1.1;

const SAMPLE_A: Duration = Duration::ZERO;
/// 50 ms: same GIF frame as A for all speeds ≤ 1.8; the wizards drift a sub-cell.
const SAMPLE_B: Duration = Duration::from_millis(50);

/// World position of a wizard with the given spatial phase at time `t` (seconds):
/// a deterministic ellipse (Lissajous — differing x/y frequencies).
fn wizard_world_pos(spatial_phase: f32, t: f32) -> WorldPos {
    WorldPos::new(
        CX + AX * (WX * t + spatial_phase).cos(),
        CY + AY * (WY * t + spatial_phase).sin(),
    )
}

// ─── Scene ────────────────────────────────────────────────────────────────────

struct Wizard {
    sprite: render::AnimatedSprite,
    spatial_phase: f32,
}

struct RenderMovementScene {
    wizards: Vec<Wizard>,
    camera: SideView,
    elapsed: Duration,
}

impl RenderMovementScene {
    fn new() -> Self {
        let gif_bytes = include_bytes!("assets/wizard.gif");
        let wizards = SPEEDS
            .iter()
            .zip(PHASES.iter())
            .map(|(&speed, &spatial_phase)| Wizard {
                sprite: render::AnimatedSprite::from_gif(gif_bytes, FRAME_DUR)
                    .expect("decode wizard.gif")
                    .with_speed(speed),
                spatial_phase,
            })
            .collect();
        RenderMovementScene {
            wizards,
            camera: SideView::new(SCALE_DOTS),
            elapsed: Duration::ZERO,
        }
    }
}

impl Scene for RenderMovementScene {
    fn id(&self) -> SceneId {
        SceneId::BattleViewer
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt;
        None
    }

    fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
        // Quit handled globally by the engine (manager.rs route_key).
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    /// Dot-level multi-sprite render:
    ///   1. Rasterize every wizard's current frame to a DotBuffer.
    ///   2. Project each world position → dot offset + depth via the camera.
    ///   3. `composite_dots` at dot granularity, sorted by depth (screen-y).
    ///   4. `dots_to_grid` → braille → blit.
    fn render(&self, frame: &mut Frame, area: Rect) {
        let cols = area.width as usize;
        let rows = area.height as usize;
        let t = self.elapsed.as_secs_f32();

        // Rasterize first — the DotPlacements below borrow these, so the buffers
        // must outlive the composite call.
        let dotbufs: Vec<render::dots::DotBuffer> = self
            .wizards
            .iter()
            .map(|w| {
                let img = w.sprite.frame_at(self.elapsed);
                let (img_w, img_h) = img.dimensions();
                let aspect = img_w as f32 / img_h as f32;
                let dot_rows = WIZARD_DOT_ROWS;
                let dot_cols = (dot_rows as f32 * aspect).round() as u32;
                render::dots::sprite_to_dots(img, dot_cols, dot_rows)
            })
            .collect();

        let placements: Vec<render::composite::DotPlacement> = self
            .wizards
            .iter()
            .zip(&dotbufs)
            .map(|(w, dots)| {
                let pos = wizard_world_pos(w.spatial_phase, t);
                let (dot_x, dot_y) = self.camera.project(pos);
                render::composite::DotPlacement {
                    dots,
                    dot_x,
                    dot_y,
                    depth: self.camera.depth_key(pos),
                }
            })
            .collect();

        let composed = render::composite::composite_dots(cols * 2, rows * 4, &placements);
        let grid = render::dots::dots_to_grid(&composed);
        render::draw_grid(frame.buffer_mut(), area, &grid);
    }
}

// ─── Headless buffer serializer ───────────────────────────────────────────────

/// Serialize a ratatui `Buffer` to ANSI truecolor braille on stdout: braille
/// cells (U+2800..=U+28FF) with an `Rgb` fg become `\x1b[38;2;r;g;bm<glyph>`;
/// other cells emit their symbol; each row ends with `\x1b[0m\n`.
fn dump_buffer_ansi(buf: &ratatui::buffer::Buffer) -> String {
    let area = buf.area;
    let mut out = String::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell((x, y)) {
                let sym = cell.symbol();
                let is_braille = sym.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c));
                if is_braille {
                    if let Color::Rgb(r, g, b) = cell.fg {
                        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m{sym}"));
                    } else {
                        out.push_str(sym);
                    }
                } else {
                    out.push_str(sym);
                }
            }
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    let once = std::env::args().any(|a| a == "--once");
    let headless = once || !io::stdout().is_terminal();

    if headless {
        let backend = TestBackend::new(W, H);
        let mut terminal = Terminal::new(backend)?;
        let mut scene = RenderMovementScene::new();

        // Frame A: elapsed = SAMPLE_A.
        scene.elapsed = SAMPLE_A;
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_a = terminal.backend().buffer().clone();
        print!("{}", dump_buffer_ansi(&buf_a));

        println!("{}", MARKER);

        // Frame B: elapsed = SAMPLE_B (same GIF frame; wizards drifted a sub-cell).
        scene.elapsed = SAMPLE_B;
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_b = terminal.backend().buffer().clone();
        print!("{}", dump_buffer_ansi(&buf_b));

        Ok(())
    } else {
        game::run(Box::new(RenderMovementScene::new()))
    }
}
