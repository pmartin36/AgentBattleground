//! render_tier3 — behavioral deliverable for renderer-tier3/b3-t1.
//!
//! Demonstrates depth-sorted multi-sprite compositing (three wizards sharing
//! the same GIF fixture, staggered in position/speed/phase) using the full
//! animated render path: GIF → AnimatedSprite → braille → composite → terminal.
//!
//! Modes:
//!   - **Interactive** (TTY stdout, no `--once`): runs the live 30 fps game loop
//!     via `game::run`.
//!   - **Headless** (`--once` flag present OR stdout is not a TTY): renders the
//!     composited scene at `elapsed = 0` (frame A), prints `---FRAME-BREAK---`,
//!     then renders at `elapsed = FRAME_DUR` (frame B), and exits 0. Used by the
//!     automated integration test.

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

// ─── Constants ────────────────────────────────────────────────────────────────

/// Uniform per-frame duration. Matches tier2; ensures GIF frame 0 and frame 1
/// are distinct at elapsed = 0 vs elapsed = FRAME_DUR.
const FRAME_DUR: Duration = Duration::from_millis(100);

/// Separator printed between the two headless frames. Must contain no braille
/// codepoint (U+2800..U+28FF) and no ESC byte so the integration test's
/// split + per-half braille assertions are unambiguous.
const MARKER: &str = "---FRAME-BREAK---";

/// Headless terminal dimensions (parity with tier1/tier2).
const W: u16 = 50;
const H: u16 = 30;

/// Per-sprite braille-cell dimensions used as the `convert` area. Large enough
/// that the tall wizard renders sizeable, so heavy positional overlap between
/// sprites produces visible body-on-body occlusion (not just adjacent boxes).
const SPRITE_W: u16 = 22;
const SPRITE_H: u16 = 22;

// ─── Wizard ───────────────────────────────────────────────────────────────────

/// One animated wizard instance: an `AnimatedSprite` plus placement metadata.
struct Wizard {
    sprite: engine_render::AnimatedSprite,
    /// Column (braille cells, 0-based) of the sprite's top-left corner.
    col: i32,
    /// Row (braille cells, 0-based) of the sprite's top-left corner.
    row: i32,
    /// Phase offset added to `elapsed` before frame selection.
    phase: Duration,
}

// ─── Tier3Scene ───────────────────────────────────────────────────────────────

struct Tier3Scene {
    wizards: Vec<Wizard>,
    elapsed: Duration,
    no_inspect: engine_core::scene::NoInspect,
}

impl Tier3Scene {
    fn new() -> Self {
        let gif_bytes = include_bytes!("assets/wizard.gif");

        // Three wizards sharing the same GIF, placed with heavy overlap (small
        // per-sprite offset) so their opaque BODIES — not just bounding boxes —
        // overlap, and the depth-sort visibly occludes: the nearer wizard clips
        // the one behind it. Positions fit the 50×30 headless screen.
        //
        // NOTE: `depth = row` here is ONLY this demo's stand-in side-view camera.
        // It is NOT the design. In the real game the CAMERA computes each
        // sprite's depth from its position via `depth_key(position)` (row for a
        // side view, row+col for isometric, …) — see specs/13-rendering.md
        // §"Depth & Draw Order". The compositor just sorts by whatever depth
        // scalar it's handed; it never assumes row.
        //
        // The frontmost wizard (speed=2.0) provably advances a frame between
        // elapsed=0 and elapsed=FRAME_DUR, so the composited output differs
        // between the two headless captures.
        let make = |speed: f32, col: i32, row: i32, phase_ms: u64| Wizard {
            sprite: engine_render::AnimatedSprite::from_gif(gif_bytes, FRAME_DUR)
                .expect("decode wizard.gif")
                .with_speed(speed),
            col,
            row,
            phase: Duration::from_millis(phase_ms),
        };

        Tier3Scene {
            wizards: vec![
                make(1.0, 4, 1, 0),   // back   — depth=row=1, drawn first (farthest)
                make(1.5, 9, 4, 33),  // middle — depth=row=4
                make(2.0, 14, 7, 66), // front  — depth=row=7, drawn last (on top)
            ],
            elapsed: Duration::ZERO,
            no_inspect: engine_core::scene::NoInspect,
        }
    }
}

impl Scene for Tier3Scene {
    fn id(&self) -> SceneKey {
        SceneId::BattleViewer.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, dt: Duration) -> Option<Transition> {
        self.elapsed += dt;
        None
    }

    fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    /// Multi-sprite render path (dot-level compositor — spec 16):
    ///   1. Rasterize each wizard's current frame to an owned `DotBuffer`.
    ///   2. Wrap each in a `DotPlacement { depth = row }` (side-view ordering),
    ///      converting each wizard's cell-granularity `col`/`row` into dot
    ///      units (1 cell = 2 dots wide × 4 dots tall).
    ///   3. Composite all into a single full-area `DotBuffer`.
    ///   4. Convert to a `Grid` and blit with `draw_grid`.
    fn render(&self, frame: &mut Frame, area: Rect) {
        let dot_cols = area.width as usize * 2;
        let dot_rows = area.height as usize * 4;

        // Per-sprite rasterize area — smaller than the full screen so sprites
        // can be independently positioned and staggered.
        let sprite_dot_cols = SPRITE_W as u32 * 2;
        let sprite_dot_rows = SPRITE_H as u32 * 4;

        // Build all per-sprite DotBuffers into an owned Vec first;
        // DotPlacement borrows &DotBuffer, so the buffers must outlive the
        // placements.
        let dotbufs: Vec<engine_render::dots::DotBuffer> = self
            .wizards
            .iter()
            .map(|w| {
                // Routed through the sprite's own cached accessor (b6-t1)
                // rather than a direct `sprite_to_dots` call, so the 3
                // wizards sharing one GIF pointer share one rasterization.
                w.sprite
                    .dots_at(self.elapsed + w.phase, sprite_dot_cols, sprite_dot_rows)
            })
            .collect();

        let placements: Vec<engine_render::composite::DotPlacement> = self
            .wizards
            .iter()
            .zip(&dotbufs)
            .map(|(w, dots)| engine_render::composite::DotPlacement {
                dots,
                dot_x: w.col * 2,
                dot_y: w.row * 4,
                depth: w.row, // depth = row → higher row = nearer = drawn on top
            })
            .collect();

        let composed = engine_render::composite::composite_dots(dot_cols, dot_rows, &placements);
        let grid = engine_render::dots::dots_to_grid(&composed);
        engine_render::draw_grid(frame.buffer_mut(), area, &grid);
    }

    fn inspect(&mut self) -> &mut dyn engine_core::Inspectable {
        &mut self.no_inspect
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
/// Mirrors the emission shape in `experiments/ascii_test/src/downrez.rs`.
/// Copied verbatim from render_tier2 (examples are standalone binaries).
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

        let mut scene = Tier3Scene::new();

        // Frame A: elapsed = 0 (initial composite).
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_a = terminal.backend().buffer().clone();
        let output_a = dump_buffer_ansi(&buf_a);
        print!("{}", output_a);

        // Separator line.
        println!("{}", MARKER);

        // Frame B: elapsed = FRAME_DUR (composited animation has advanced).
        scene.elapsed = FRAME_DUR;
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_b = terminal.backend().buffer().clone();
        let output_b = dump_buffer_ansi(&buf_b);
        print!("{}", output_b);

        Ok(())
    } else {
        game::run(Box::new(Tier3Scene::new()))
    }
}
