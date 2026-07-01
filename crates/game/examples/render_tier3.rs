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
use scene_core::scene_id::SceneId;
use serde_json::Value as JsonValue;

use game::scene::{EngineCtx, InputEvent, Scene, Transition};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Uniform per-frame duration. Matches tier2; ensures GIF frame 0 and frame 1
/// are distinct at elapsed = 0 vs elapsed = FRAME_DUR.
const FRAME_DUR: Duration = Duration::from_millis(100);

/// Separator printed between the two headless frames. Must contain no braille
/// codepoint (U+2800..U+28FF) and no ESC byte so the integration test's
/// split + per-half braille assertions are unambiguous.
const MARKER: &str = "---FRAME-BREAK---";

/// Headless terminal dimensions (parity with tier1/tier2).
const W: u16 = 40;
const H: u16 = 20;

/// Per-sprite braille-cell dimensions used as the `convert` area.
/// For the ~square wizard.gif the natural rows ≈ SPRITE_W/2 = 6; SPRITE_H=8
/// gives a comfortable ceiling that the aspect-fit will not exceed.
const SPRITE_W: u16 = 12;
const SPRITE_H: u16 = 8;

// ─── Wizard ───────────────────────────────────────────────────────────────────

/// One animated wizard instance: an `AnimatedSprite` plus placement metadata.
struct Wizard {
    sprite: render::AnimatedSprite,
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
}

impl Tier3Scene {
    fn new() -> Self {
        let gif_bytes = include_bytes!("assets/wizard.gif");

        // Three wizards sharing the same GIF, staggered in position/speed/phase
        // so their bounding boxes overlap and the compositor's depth-sort is
        // exercised.  Positions (col, row) are chosen so all sprites fit in the
        // 40×20 headless screen.
        //
        // depth = row (side-view ordering): higher row = nearer = drawn on top.
        // The frontmost wizard (row=7, speed=2.0) provably advances a frame
        // between elapsed=0 and elapsed=FRAME_DUR (GIF frames 1→3), so the
        // composited output differs between the two headless captures.
        let make = |speed: f32, col: i32, row: i32, phase_ms: u64| Wizard {
            sprite: render::AnimatedSprite::from_gif(gif_bytes, FRAME_DUR)
                .expect("decode wizard.gif")
                .with_speed(speed),
            col,
            row,
            phase: Duration::from_millis(phase_ms),
        };

        Tier3Scene {
            wizards: vec![
                make(1.0, 2, 1, 0),   // far     — depth=1, drawn first
                make(1.5, 10, 4, 33), // middle  — depth=4
                make(2.0, 18, 7, 66), // nearest — depth=7, drawn last (on top)
            ],
            elapsed: Duration::ZERO,
        }
    }
}

impl Scene for Tier3Scene {
    fn id(&self) -> SceneId {
        SceneId::BattleViewer
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

    /// Multi-sprite render path:
    ///   1. Convert each wizard's current frame to an owned `Grid`.
    ///   2. Wrap each in a `Placement { depth = row }` (side-view ordering).
    ///   3. Composite all into a single full-area `Grid`.
    ///   4. Blit with `draw_grid`.
    fn render(&self, frame: &mut Frame, area: Rect) {
        let cols = area.width as usize;
        let rows = area.height as usize;

        // Per-sprite convert area — smaller than the full screen so sprites
        // can be independently positioned and staggered.
        let sprite_area = Rect::new(0, 0, SPRITE_W, SPRITE_H);

        // Build all per-sprite Grids into an owned Vec first; Placement borrows
        // &Grid, so the Grids must outlive the Placements.
        let grids: Vec<render::Grid> = self
            .wizards
            .iter()
            .map(|w| render::convert(w.sprite.frame_at(self.elapsed + w.phase), sprite_area))
            .collect();

        let placements: Vec<render::Placement> = self
            .wizards
            .iter()
            .zip(&grids)
            .map(|(w, g)| render::Placement {
                grid: g,
                col: w.col,
                row: w.row,
                depth: w.row, // depth = row → higher row = nearer = drawn on top
            })
            .collect();

        let composed = render::composite(cols, rows, &placements);
        render::draw_grid(frame.buffer_mut(), area, &composed);
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
