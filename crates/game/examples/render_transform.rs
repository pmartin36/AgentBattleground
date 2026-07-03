//! render_transform — spin + pulse + drift (sprite-transform demo, b3-t1).
//!
//! A single frozen wizard sprite (`assets/wizard_still.png`) whose `Transform`
//! animates purely from `elapsed`: `rotation` sweeps a continuous full turn,
//! `scale` pulses via `engine_render::tween::{lerp, ease_in_out}`, and `translate`
//! gently drifts along a Lissajous path. Because the source frame is frozen,
//! the only motion across frames is the `Transform` — exercising `rasterize`
//! (b1-t2), `place` (b1-t3), and `tween` (b2-t1) end-to-end through the real
//! dot pipeline.
//!
//! Modes:
//!   - **Interactive** (TTY, no `--once`): the live 30 fps loop via `game::run`.
//!   - **Headless** (`--once` or non-TTY): renders the scene at two distinct
//!     elapsed times, separated by `---FRAME-BREAK---`, then exits 0.

use std::f32::consts::TAU;
use std::io::{self, IsTerminal};
use std::time::Duration;

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Frame;
use ratatui::Terminal;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use game::scene_id::SceneId;
use engine_render::camera::{SideView, WorldPos};
use engine_render::transform::{Transform, Vec2};
use engine_render::tween;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Separator between the two headless frames. No braille codepoint / no ESC byte.
const MARKER: &str = "---FRAME-BREAK---";

/// Headless terminal dimensions (parity with the other examples).
const W: u16 = 50;
const H: u16 = 30;

/// Camera scale: dots per world unit.
const SCALE_DOTS: f32 = 2.0;

/// Wizard height in dots (base, before scale pulse).
const WIZARD_DOT_ROWS: u32 = 40;

/// Full-turn rotation period (seconds).
const ROT_PERIOD: f32 = 3.0;

/// Scale-pulse period (seconds).
const PULSE_PERIOD: f32 = 1.5;
const SCALE_MIN: f32 = 0.7;
const SCALE_MAX: f32 = 1.2;

/// Drift center on-screen (world units) and Lissajous parameters.
const CX: f32 = 25.0;
const CY: f32 = 30.0;
const DRIFT_AX: f32 = 3.0;
const DRIFT_AY: f32 = 2.0;
const WX: f32 = 0.7;
const WY: f32 = 1.1;

const SAMPLE_A: Duration = Duration::ZERO;
const SAMPLE_B: Duration = Duration::from_millis(600);

/// Derive the current `Transform` from elapsed time (seconds): continuous
/// rotation, sine-driven eased scale pulse, and a gentle Lissajous drift.
fn transform_at(elapsed: Duration) -> Transform {
    let t = elapsed.as_secs_f32();

    let rotation = (t / ROT_PERIOD) * 360.0;

    let pulse_t = 0.5 * (1.0 + (TAU * t / PULSE_PERIOD).sin());
    let mag = tween::lerp(SCALE_MIN, SCALE_MAX, tween::ease_in_out(pulse_t));
    let scale = Vec2::splat(mag);

    let translate = WorldPos::new(
        CX + DRIFT_AX * (WX * t).cos(),
        CY + DRIFT_AY * (WY * t).sin(),
    );

    Transform { translate, rotation, scale }
}

// ─── Scene ────────────────────────────────────────────────────────────────────

struct RenderTransformScene {
    img: image::DynamicImage,
    camera: SideView,
    elapsed: Duration,
    no_inspect: engine_core::scene::NoInspect,
}

impl RenderTransformScene {
    fn new() -> Self {
        RenderTransformScene {
            img: image::load_from_memory(include_bytes!("assets/wizard_still.png"))
                .expect("decode wizard_still.png"),
            camera: SideView::new(SCALE_DOTS),
            elapsed: Duration::ZERO,
            no_inspect: engine_core::scene::NoInspect,
        }
    }
}

impl Scene for RenderTransformScene {
    fn id(&self) -> SceneKey {
        SceneId::BattleViewer.into()
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

    /// Single-sprite transform pipeline:
    ///   1. Derive the current `Transform` from `elapsed`.
    ///   2. `rasterize` the frozen source frame with that transform.
    ///   3. `place` it (pivot-centered) via the `SideView` camera.
    ///   4. `composite_dots` + `dots_to_grid` → braille → blit.
    fn render(&self, frame: &mut Frame, area: Rect) {
        let cols = area.width as usize;
        let rows = area.height as usize;

        let transform = transform_at(self.elapsed);

        // Rasterize before place — the DotPlacement borrows this buffer, so it
        // must outlive the composite call.
        let dots = engine_render::transform::rasterize(&self.img, &transform, WIZARD_DOT_ROWS);
        let placement = engine_render::transform::place(&dots, transform.translate, &self.camera);

        let composed = engine_render::composite::composite_dots(cols * 2, rows * 4, &[placement]);
        let grid = engine_render::dots::dots_to_grid(&composed);
        engine_render::draw_grid(frame.buffer_mut(), area, &grid);
    }

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
        &mut self.no_inspect
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
        let mut scene = RenderTransformScene::new();

        // Frame A: elapsed = SAMPLE_A.
        scene.elapsed = SAMPLE_A;
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_a = terminal.backend().buffer().clone();
        print!("{}", dump_buffer_ansi(&buf_a));

        println!("{}", MARKER);

        // Frame B: elapsed = SAMPLE_B (transform advanced: rotation/scale/drift differ).
        scene.elapsed = SAMPLE_B;
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_b = terminal.backend().buffer().clone();
        print!("{}", dump_buffer_ansi(&buf_b));

        Ok(())
    } else {
        game::run(Box::new(RenderTransformScene::new()))
    }
}
