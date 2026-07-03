//! render_tier2 — behavioral deliverable for b3-t1.
//!
//! Demonstrates the full animated-sprite render path (GIF → AnimatedSprite →
//! braille → terminal) using the committed GIF fixture embedded at compile time.
//!
//! Modes:
//!   - **Interactive** (TTY stdout, no `--once`): runs the live 30 fps game loop
//!     via `game::run`.
//!   - **Headless** (`--once` flag present OR stdout is not a TTY): renders the
//!     scene at `elapsed = 0` (frame A), prints `---FRAME-BREAK---`, then renders
//!     the scene at `elapsed = FRAME_DUR` (frame B), and exits 0. Used by the
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

/// Uniform per-frame duration. Any value > 0 ensures frame 0 and frame 1 of
/// the GIF are distinct at elapsed = 0 vs elapsed = FRAME_DUR.
const FRAME_DUR: Duration = Duration::from_millis(100);

/// Separator printed between the two headless frames. Must contain no braille
/// codepoint (U+2800..U+28FF) and no ESC byte so the integration test's
/// split + per-half braille assertions are unambiguous.
const MARKER: &str = "---FRAME-BREAK---";

// ─── Tier2Scene ───────────────────────────────────────────────────────────────

struct Tier2Scene {
    sprite: engine_render::AnimatedSprite,
    elapsed: Duration,
    no_inspect: engine_core::scene::NoInspect,
}

impl Tier2Scene {
    fn new() -> Self {
        Tier2Scene {
            sprite: engine_render::AnimatedSprite::from_gif(
                include_bytes!("assets/wizard.gif"),
                FRAME_DUR,
            )
            .expect("decode wizard.gif"),
            elapsed: Duration::ZERO,
            no_inspect: engine_core::scene::NoInspect,
        }
    }
}

impl Scene for Tier2Scene {
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

    /// Real engine render path: select animated frame → convert to Grid → draw.
    fn render(&self, frame: &mut Frame, area: Rect) {
        let grid = engine_render::convert(self.sprite.frame_at(self.elapsed), area);
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
/// Copied verbatim from render_tier1 (examples are standalone binaries).
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
        // Fixed frame size for determinism; matches tier-1 dimensions.
        const W: u16 = 40;
        const H: u16 = 20;

        let backend = TestBackend::new(W, H);
        let mut terminal = Terminal::new(backend)?;

        let mut scene = Tier2Scene::new();

        // Frame A: elapsed = 0 (GIF frame 0).
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_a = terminal.backend().buffer().clone();
        let output_a = dump_buffer_ansi(&buf_a);
        print!("{}", output_a);

        // Separator line.
        println!("{}", MARKER);

        // Frame B: elapsed = FRAME_DUR (GIF frame 1).
        scene.elapsed = FRAME_DUR;
        terminal.draw(|f| scene.render(f, f.area()))?;
        let buf_b = terminal.backend().buffer().clone();
        let output_b = dump_buffer_ansi(&buf_b);
        print!("{}", output_b);

        Ok(())
    } else {
        game::run(Box::new(Tier2Scene::new()))
    }
}
