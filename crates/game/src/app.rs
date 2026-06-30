use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{inspect, ipc_server, manager, manager::SceneManager, scene::Scene};

/// RAII guard: restores the terminal on drop (covers both normal exit and panic).
pub(crate) struct TerminalGuard;

impl TerminalGuard {
    pub(crate) fn new() -> io::Result<Self> {
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

/// Single entrypoint for the game engine.
///
/// Sets up the terminal, wires IPC/inspect, then runs the 30 fps loop until
/// the user quits. `initial` is the boot scene — it may be any `Box<dyn Scene>`,
/// including off-catalog example scenes.
pub fn run(initial: Box<dyn Scene>) -> io::Result<()> {
    // Inspect setup BEFORE alt-screen so the socket-path println is visible.
    let ipc = if inspect::flag_present(std::env::args()) {
        inspect::start(inspect::INSPECT_SUPPORTED)?
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

    let mut mgr = SceneManager::with_scene(initial);

    let frame_budget = Duration::from_nanos(1_000_000_000 / 30);

    loop {
        let frame_start = Instant::now();

        // 1. Drain debug command channel (debug always overrides gameplay).
        while let Ok(cmd) = cmd_rx.try_recv() {
            mgr.apply_command(cmd, &events);
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

        // 4. Apply any pending scene transition; notify inspector of the switch.
        mgr.process_pending_notify(&events);

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
