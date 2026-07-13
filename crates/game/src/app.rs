use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    cursor::Hide,
    event::{self, EnableMouseCapture, Event, KeyEvent},
    execute,
    terminal::{enable_raw_mode, EnterAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::registry::GameCatalog;
use engine_core::net::{inspect, ipc_server};
use engine_core::scene::manager::{self, SceneManager};
use engine_core::scene::{InputEvent, Scene};

/// Routes a key event to the active scene. Returns `true` iff the app should quit.
fn handle_key(mgr: &mut SceneManager, key: KeyEvent) -> bool {
    mgr.route_key(key)
}

/// Render the active scene, then apply the global debug cell-boundary
/// gridline overlay (bucket b5) as a final post-composite pass when toggled
/// on via `SceneManager::debug_grid_enabled()` — scene-agnostic by
/// construction, no per-scene opt-in.
fn render_frame(mgr: &SceneManager, f: &mut ratatui::Frame) {
    mgr.render(f);
    if mgr.debug_grid_enabled() {
        engine_render::draw_debug_grid(f.buffer_mut());
    }
}

/// RAII guard: restores the terminal on drop (covers both normal exit and panic).
pub(crate) struct TerminalGuard;

impl TerminalGuard {
    pub(crate) fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide)?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        engine_core::terminal::restore_terminal();
    }
}

/// Tracks wall-clock time between frames. `tick()` returns the elapsed time
/// since the previous `tick()` — i.e. the full previous-frame interval
/// (render + sleep), which is the correct `dt` to feed scene updates. The bug
/// it replaces read intra-frame elapsed (~µs), so `elapsed` barely grew and
/// animation appeared frozen.
struct FrameClock {
    prev: Instant,
}

impl FrameClock {
    fn new() -> Self {
        FrameClock {
            prev: Instant::now(),
        }
    }

    fn tick(&mut self) -> Duration {
        let now = Instant::now();
        let dt = now.duration_since(self.prev);
        self.prev = now;
        dt
    }
}

/// Single entrypoint for the game engine.
///
/// Sets up the terminal, wires IPC/inspect, then runs the 30 fps loop until
/// the user quits. `initial` is the boot scene — it may be any `Box<dyn Scene>`,
/// including off-catalog example scenes. `params` is delivered to `initial`'s
/// `enter()` via `SceneManager::with_scene_and_params`.
pub fn run_with_params(
    initial: Box<dyn Scene>,
    params: Option<serde_json::Value>,
) -> io::Result<()> {
    // Inspect setup BEFORE alt-screen so the socket-path println is visible.
    let ipc = if inspect::flag_present(std::env::args()) {
        inspect::start(inspect::INSPECT_SUPPORTED, std::sync::Arc::new(GameCatalog))?
    } else {
        None
    };

    // Split into held handle (Drop unlinks socket), outbound event sender,
    // and inbound command receiver — works for both IPC-on and IPC-off cases.
    let (_ipc_handle, events, cmd_rx) = match ipc {
        Some((handle, rx)) => {
            let tx = handle.events.clone();
            (Some(handle), tx, rx)
        }
        None => {
            let (tx, _drop_rx) = std::sync::mpsc::channel::<ipc_server::Event>();
            let (_unused_tx, rx) = std::sync::mpsc::channel::<manager::Command>();
            (None, tx, rx)
        }
    };

    let _guard = TerminalGuard::new()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut mgr =
        SceneManager::with_scene_and_params(initial, params, Box::new(GameCatalog));

    let frame_budget = Duration::from_nanos(1_000_000_000 / 30);
    let mut clock = FrameClock::new();

    loop {
        let frame_start = Instant::now();

        // 1. Drain debug command channel (debug always overrides gameplay).
        while let Ok(cmd) = cmd_rx.try_recv() {
            mgr.apply_command(cmd, &events);
        }

        // 2. Poll crossterm input (non-blocking), draining ALL events queued
        //    this frame — not just one. A single `if` here let a burst of
        //    rapid real input (fast mouse movement, key mashing) back up
        //    across multiple frames, each event only advancing 33ms of
        //    apparent lag per frame; the debug command channel above already
        //    drains fully with `while let Ok(..) = try_recv()`, this matches
        //    that pattern. Every key (Ctrl-C / gameplay input, including
        //    digits) is handled by route_key.
        let mut should_quit = false;
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key(&mut mgr, key) {
                        should_quit = true;
                        break;
                    }
                }
                Event::Mouse(me) => {
                    if let Some(t) = mgr.handle_input(InputEvent::Mouse(me)) {
                        mgr.set_gameplay_transition(t);
                    }
                }
                _ => {}
            }
        }
        if should_quit {
            break;
        }

        // 2b. A scene may itself request the same exit Ctrl-C produces
        //     (b4-t1), reachable from inside `handle_input` on either the
        //     keyboard or mouse path above.
        if mgr.active_quit_requested() {
            break;
        }

        // 3. Update active scene with wall-clock dt = time since the previous
        //    frame (≈ frame_budget incl. sleep), NOT intra-frame elapsed.
        let dt = clock.tick();
        if let Some(t) = mgr.update(dt) {
            mgr.set_gameplay_transition(t);
        }

        // 4. Apply any pending scene transition; notify inspector of the switch.
        mgr.process_pending_notify(&events);

        // 4b. While Subscribe{live} is on, push a coalesced ~10Hz StateSnapshot.
        mgr.pump_live_snapshots(&events, Instant::now());

        // 5. Render.
        terminal.draw(|f| render_frame(&mgr, f))?;

        // 6. Sleep the remainder of the 33.33 ms frame budget.
        let elapsed = frame_start.elapsed();
        if elapsed < frame_budget {
            std::thread::sleep(frame_budget - elapsed);
        }
    }

    Ok(())
}

/// Boots `initial` with no params (`enter(&mut ctx, None)`). See `run_with_params`.
pub fn run(initial: Box<dyn Scene>) -> io::Result<()> {
    run_with_params(initial, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use crate::scenes::{Leaderboard, MainHub};

    /// Renders `mgr` through `render_frame` into a fresh `TestBackend` and
    /// returns the resulting buffer.
    fn render(mgr: &SceneManager) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();
        terminal.draw(|f| render_frame(mgr, f)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// The debug grid toggle (Ctrl-G, routed through `SceneManager::route_key`)
    /// must change `render_frame`'s output for ANY active scene, with no
    /// per-scene opt-in — the DELIVERABLE's before/after evidence across two
    /// different scenes.
    #[test]
    fn render_frame_applies_debug_grid_overlay_globally_across_scenes() {
        let scenes: Vec<Box<dyn Scene>> = vec![
            Box::new(MainHub::default()),
            Box::new(Leaderboard),
        ];

        for scene in scenes {
            let mut mgr = SceneManager::with_scene(scene, Box::new(GameCatalog));

            let buf_before = render(&mgr);

            let quit = mgr.route_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
            assert!(!quit, "Ctrl-G must not be treated as quit");
            assert!(
                mgr.debug_grid_enabled(),
                "Ctrl-G must toggle the debug grid on"
            );

            let buf_after = render(&mgr);

            assert_ne!(
                buf_before, buf_after,
                "toggling the overlay on must change the rendered buffer for scene {:?}, \
                 with no per-scene opt-in",
                mgr.active_id()
            );

            let (mask, _color) = engine_render::decode_braille_cell(&buf_after, 0, 0)
                .expect("top-left cell must be braille after the overlay pass");
            assert_ne!(
                mask, 0,
                "top-left cell-boundary dot must be lit once the overlay is applied for scene {:?}",
                mgr.active_id()
            );

            // Every plain-text cell (a scene label/menu glyph — anything
            // `decode_braille_cell` reports as non-braille and non-blank in
            // `buf_before`) must survive completely unchanged in `buf_after`.
            // The overlay must never convert real text into a braille glyph.
            let area = buf_before.area;
            let mut checked_a_text_cell = false;
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    let before_cell = buf_before.cell((x, y)).unwrap();
                    let is_text = engine_render::decode_braille_cell(&buf_before, x, y).is_none()
                        && !before_cell.symbol().trim().is_empty();
                    if !is_text {
                        continue;
                    }
                    checked_a_text_cell = true;
                    let after_cell = buf_after.cell((x, y)).unwrap();
                    assert_eq!(
                        before_cell.symbol(),
                        after_cell.symbol(),
                        "text cell ({x},{y}) must not be converted to braille by the overlay for scene {:?}",
                        mgr.active_id()
                    );
                    assert_eq!(
                        before_cell.fg,
                        after_cell.fg,
                        "text cell ({x},{y})'s color must be untouched by the overlay for scene {:?}",
                        mgr.active_id()
                    );
                }
            }
            assert!(
                checked_a_text_cell,
                "scene {:?} must render at least one plain-text cell for this guard to be meaningful",
                mgr.active_id()
            );
        }
    }

    /// Regression for the dt freeze bug: `tick()` must report real wall-clock
    /// time between calls (~ms), not intra-frame elapsed (~µs).
    #[test]
    fn frame_clock_tick_reports_real_elapsed() {
        let mut clock = FrameClock::new();
        std::thread::sleep(Duration::from_millis(20));
        let dt = clock.tick();
        assert!(
            dt >= Duration::from_millis(15),
            "tick() dt {dt:?} should reflect the ~20ms slept, not intra-frame µs"
        );
        std::thread::sleep(Duration::from_millis(10));
        let dt2 = clock.tick();
        assert!(
            dt2 >= Duration::from_millis(7),
            "second tick {dt2:?} should reflect ~10ms independently"
        );
    }

}
