//! render_movement — behavioral deliverable for dot-compositor-camera/b4-t1.
//!
//! Demonstrates sub-cell (dot-granularity) movement through the full
//! dot-level pipeline: world position → SideView camera → dot offset →
//! DotBuffer compositor → braille grid → terminal.
//!
//! Modes:
//!   - **Interactive** (TTY stdout, no `--once`): runs the live 30 fps game loop
//!     via `game::run`.
//!   - **Headless** (`--once` flag present OR stdout is not a TTY): renders the
//!     scene at `elapsed = SAMPLE_A` (frame A), prints `---FRAME-BREAK---`,
//!     then renders at `elapsed = SAMPLE_B` (frame B, 1-dot sub-cell shift,
//!     same GIF animation frame), and exits 0.

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

/// Uniform per-frame duration. Defines the GIF frame window; both headless
/// samples must fall inside the first frame window [0, FRAME_DUR) so the GIF
/// animation frame stays at 0.
const FRAME_DUR: Duration = Duration::from_millis(100);

/// Separator printed between the two headless frames. Must contain no braille
/// codepoint (U+2800..U+28FF) and no ESC byte so the integration test's
/// split + per-half braille assertions are unambiguous.
const MARKER: &str = "---FRAME-BREAK---";

/// Headless terminal dimensions (parity with tier1/tier2/tier3).
const W: u16 = 50;
const H: u16 = 30;

/// Camera scale: dots per world unit. 2 dots/unit → 1 world unit = 1 cell wide.
const SCALE_DOTS: f32 = 2.0;

/// Initial wizard world X position.
/// dot_x = round(5.0 × 2.0) = 10 (even → left edge of cell, cell-aligned).
const X0: f32 = 5.0;

/// Wizard world Y position (fixed; only X moves).
/// dot_y = round(10.0 × 2.0) = 20.
const Y0: f32 = 10.0;

/// Wizard movement speed in world units/second.
const SPEED: f32 = 5.0;

/// Wizard height in dots. aspect ≈ 64/144 ≈ 0.444;
/// w_dot_cols = round(80 × 0.444) = round(35.56) = 36.
const WIZARD_DOT_ROWS: u32 = 80;

/// Headless sample A: elapsed = 0 → x = 5.0 → dot_x = 10, GIF frame index = 0.
const SAMPLE_A: Duration = Duration::ZERO;

/// Headless sample B: elapsed = 90 ms → x = 5.0 + 5.0×0.09 = 5.45 →
/// dot_x = round(10.9) = 11 (odd → straddles cell boundary).
/// GIF frame index = floor(90 / 100) = 0 — same frame as A (proof is sound).
const SAMPLE_B: Duration = Duration::from_millis(90);

// ─── Scene ────────────────────────────────────────────────────────────────────

struct RenderMovementScene {
    wizard: render::AnimatedSprite,
    camera: SideView,
    elapsed: Duration,
}

impl RenderMovementScene {
    fn new() -> Self {
        let gif_bytes = include_bytes!("assets/wizard.gif");
        RenderMovementScene {
            wizard: render::AnimatedSprite::from_gif(gif_bytes, FRAME_DUR)
                .expect("decode wizard.gif"),
            camera: SideView::new(SCALE_DOTS),
            elapsed: Duration::ZERO,
        }
    }

    /// Current world position: linear horizontal motion over time.
    fn world_pos(&self) -> WorldPos {
        WorldPos::new(X0 + SPEED * self.elapsed.as_secs_f32(), Y0)
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
        // Quit is handled globally by the engine (manager.rs route_key).
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    /// Dot-level render pipeline:
    ///   1. Get current GIF animation frame.
    ///   2. Compute aspect-preserving dot dims from the frame's pixel dimensions.
    ///   3. Rasterize frame to DotBuffer via `sprite_to_dots`.
    ///   4. Project world position to dot offset via `SideView`.
    ///   5. Place in `DotPlacement`; composite into screen-sized DotBuffer.
    ///   6. Convert DotBuffer to braille Grid via `dots_to_grid`.
    ///   7. Blit with `draw_grid`.
    fn render(&self, frame: &mut Frame, area: Rect) {
        let cols = area.width as usize;
        let rows = area.height as usize;

        // 1. Current animation frame.
        let img = self.wizard.frame_at(self.elapsed);

        // 2. Aspect-preserving dot dimensions (inline; no shared helper exists).
        let (img_w, img_h) = img.dimensions();
        let aspect = img_w as f32 / img_h as f32;
        let w_dot_rows = WIZARD_DOT_ROWS;
        let w_dot_cols = (w_dot_rows as f32 * aspect).round() as u32;

        // 3. Rasterize to dot-level buffer.
        let dots = render::dots::sprite_to_dots(img, w_dot_cols, w_dot_rows);

        // 4. Project world position → screen dot coordinate + depth sort key.
        let pos = self.world_pos();
        let (dx, dy) = self.camera.project(pos);
        let depth = self.camera.depth_key(pos);

        // 5. Compositor placement.
        let place = render::composite::DotPlacement {
            dots: &dots,
            dot_x: dx,
            dot_y: dy,
            depth,
        };

        // 6. Composite into a screen-sized dot buffer (cols×2 wide, rows×4 tall).
        let composed = render::composite::composite_dots(cols * 2, rows * 4, &[place]);

        // 7. Convert dot buffer → braille cell grid.
        let grid = render::dots::dots_to_grid(&composed);

        // 8. Blit braille grid to the ratatui buffer.
        render::draw_grid(frame.buffer_mut(), area, &grid);
    }
}

// ─── Headless buffer serializer ───────────────────────────────────────────────

/// Serialize a ratatui `Buffer` to ANSI truecolor braille on stdout.
///
/// For each row:
///   - Cells containing a braille glyph (U+2800..=U+28FF) with an `Rgb` fg
///     are emitted as `\x1b[38;2;r;g;bm<glyph>`.
///   - All other cells (transparent / background) are emitted as their symbol
///     (typically a space).
///   - Row ends with `\x1b[0m\n` (reset + newline).
///
/// Copied verbatim from render_tier3 (examples are standalone binaries;
/// tier3 itself copied it from tier2 — this is the established pattern).
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
                        // Unexpected fg type: emit symbol without color.
                        out.push_str(sym);
                    }
                } else {
                    // Background / transparent cell: emit symbol (usually " ").
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

        // Frame A: elapsed = SAMPLE_A (dot_x = 10, GIF frame 0).
        scene.elapsed = SAMPLE_A;
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_a = terminal.backend().buffer().clone();
        let output_a = dump_buffer_ansi(&buf_a);
        print!("{}", output_a);

        // Separator line.
        println!("{}", MARKER);

        // Frame B: elapsed = SAMPLE_B (dot_x = 11, GIF frame 0 — same frame, 1-dot shift).
        scene.elapsed = SAMPLE_B;
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_b = terminal.backend().buffer().clone();
        let output_b = dump_buffer_ansi(&buf_b);
        print!("{}", output_b);

        Ok(())
    } else {
        game::run(Box::new(RenderMovementScene::new()))
    }
}
