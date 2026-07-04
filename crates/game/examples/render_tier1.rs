//! render_tier1 — behavioral deliverable for b3-t2.
//!
//! Demonstrates the full engine render path (image → braille → terminal) using
//! a real fixture sprite embedded at compile time.
//!
//! Modes:
//!   - **Interactive** (TTY stdout, no `--once`): runs the live 30 fps game loop
//!     via `game::run`.
//!   - **Headless** (`--once` flag present OR stdout is not a TTY): renders one
//!     frame into a `TestBackend`, serializes the buffer as ANSI truecolor braille
//!     to stdout, and exits 0.  Used by the automated integration test.

use std::io::{self, IsTerminal};
use std::time::Duration;

use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Frame;
use ratatui::Terminal;
use engine_core::SceneKey;
use serde_json::Value as JsonValue;

use engine_core::scene::manager::SceneManager;
use engine_core::scene::{EngineCtx, InputEvent, Scene, Transition};
use game::scene_id::SceneId;

// ─── Tier1Scene ───────────────────────────────────────────────────────────────

/// Non-animated fixture sprite, routed through the shared decode+rasterize
/// cache (b6-t1) instead of holding a manually-decoded `DynamicImage` field.
const WIZARD_STILL: &[u8] = include_bytes!("assets/wizard_still.png");

struct Tier1Scene {
    no_inspect: engine_core::scene::NoInspect,
}

impl Tier1Scene {
    fn new() -> Self {
        Tier1Scene {
            no_inspect: engine_core::scene::NoInspect,
        }
    }
}

impl Scene for Tier1Scene {
    fn id(&self) -> SceneKey {
        SceneId::BattleViewer.into()
    }

    fn enter(&mut self, _ctx: &mut EngineCtx, _params: Option<JsonValue>) {}

    fn update(&mut self, _ctx: &mut EngineCtx, _dt: Duration) -> Option<Transition> {
        None
    }

    fn handle_input(&mut self, _ev: InputEvent) -> Option<Transition> {
        None
    }

    fn exit(&mut self, _ctx: &mut EngineCtx) {}

    /// Real engine render path: convert image → Grid → draw into frame buffer.
    fn render(&self, frame: &mut Frame, area: Rect) {
        let grid = engine_render::asset_cache::convert(WIZARD_STILL, area);
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
        // Fixed frame size for determinism; the 32×32 fixture renders in detail.
        const W: u16 = 40;
        const H: u16 = 20;

        let backend = TestBackend::new(W, H);
        let mut terminal = Terminal::new(backend)?;
        let mgr = SceneManager::with_scene(
            Box::new(Tier1Scene::new()),
            Box::new(game::registry::GameCatalog),
        );
        terminal.draw(|f| mgr.render(f))?;
        let buf = terminal.backend().buffer().clone();
        let output = dump_buffer_ansi(&buf);
        print!("{}", output);
        Ok(())
    } else {
        game::run(Box::new(Tier1Scene::new()))
    }
}
