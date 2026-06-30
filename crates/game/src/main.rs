use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use scene_core::scene_id::SceneId;

use game::manager::{self, SceneManager};
use game::scene::Transition;

/// RAII guard: restores the terminal on drop (covers both normal exit and panic).
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}

fn main() -> io::Result<()> {
    let _guard = TerminalGuard::new()?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // M1 stub: command channel is unused; b4-t1 replaces _cmd_tx with the IPC sender.
    let (_cmd_tx, cmd_rx) = std::sync::mpsc::channel::<manager::Command>();
    let mut mgr = SceneManager::new(SceneId::MainHub);

    let frame_budget = Duration::from_nanos(1_000_000_000 / 30);

    loop {
        let frame_start = Instant::now();

        // 1. Drain debug command channel (debug always overrides gameplay).
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                manager::Command::SwitchScene { target, params } => {
                    mgr.set_debug_transition(Transition { target, params });
                }
            }
        }

        // 2. Poll crossterm input (non-blocking). All key routing (1–4 / q / Ctrl-C)
        //    is handled by route_key.
        if event::poll(Duration::ZERO)? {
            if let Event::Key(key) = event::read()? {
                if mgr.route_key(key) {
                    break;
                }
            }
        }

        // 3. Update active scene with wall-clock dt.
        let dt = frame_start.elapsed();
        if let Some(t) = mgr.update(dt) {
            mgr.set_gameplay_transition(t);
        }

        // 4. Apply any pending scene transition.
        mgr.process_pending();

        // 5. Render.
        terminal.draw(|f| mgr.render(f))?;

        // 6. Sleep the remainder of the 33.33 ms frame budget.
        let elapsed = frame_start.elapsed();
        if elapsed < frame_budget {
            std::thread::sleep(frame_budget - elapsed);
        }
    }

    Ok(())
}
