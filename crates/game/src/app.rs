use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    inspect, ipc_server, manager,
    manager::SceneManager,
    registry::GameCatalog,
    scene::{InputEvent, Scene, Transition},
};

/// Digit ('1'-'9') hotkeys: global scene switch via the gameplay path. Moved
/// here from `SceneManager::route_key` in b3-t1 (scene-core cannot depend on
/// `game::scenes`). Falls through to `mgr.route_key(key)` for everything else
/// (quit keys, gameplay-forwarded input). Returns `true` iff the app should quit.
fn handle_key(mgr: &mut SceneManager, key: KeyEvent) -> bool {
    if let KeyCode::Char(c) = key.code {
        if let Some(id) = crate::scenes::scene_for_digit(c) {
            mgr.set_gameplay_transition(Transition {
                target: id.into(),
                params: None,
            });
            return false;
        }
    }
    mgr.route_key(key)
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
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen, Show);
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
        //    that pattern. Digit hotkeys (1–4) are intercepted by `handle_key`;
        //    everything else (q / Ctrl-C / gameplay input) is handled by
        //    route_key.
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

        // 2b. A scene may itself request the same exit `q`/Ctrl-C produce
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
        terminal.draw(|f| mgr.render(f))?;

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

    // ---- digit-hotkey dispatch (reinstated from manager.rs, moved in b3-t2) ----

    use crate::scene_id::SceneId;
    use crossterm::event::KeyModifiers;
    use scene_core::SceneKey;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn mgr_at(boot: SceneId) -> SceneManager {
        SceneManager::new(SceneKey::from(boot), Box::new(GameCatalog))
    }

    #[test]
    fn route_key_digit_2_switches_to_battle_viewer() {
        let mut mgr = mgr_at(SceneId::MainHub);
        let quit = handle_key(&mut mgr, key('2'));
        assert!(!quit, "digit key must never request quit");
        mgr.process_pending();
        assert_eq!(mgr.active_id(), SceneKey::from(SceneId::BattleViewer));
    }

    #[test]
    fn route_key_digit_1_is_global_from_battle_viewer() {
        let mut mgr = mgr_at(SceneId::MainHub);
        mgr.set_gameplay_transition(Transition {
            target: SceneKey::from(SceneId::BattleViewer),
            params: None,
        });
        mgr.process_pending();
        assert_eq!(mgr.active_id(), SceneKey::from(SceneId::BattleViewer));

        let quit = handle_key(&mut mgr, key('1'));
        assert!(!quit);
        mgr.process_pending();
        assert_eq!(
            mgr.active_id(),
            SceneKey::from(SceneId::MainHub),
            "digit '1' must switch scenes globally, not just from MainHub"
        );
    }

    #[test]
    fn route_key_debug_transition_overrides_digit_gameplay() {
        let mut mgr = mgr_at(SceneId::MainHub);
        mgr.set_debug_transition(Transition {
            target: SceneKey::from(SceneId::RosterManager),
            params: None,
        });

        let quit = handle_key(&mut mgr, key('2'));
        assert!(!quit);
        mgr.process_pending();
        assert_eq!(
            mgr.active_id(),
            SceneKey::from(SceneId::RosterManager),
            "a same-tick debug transition must win over a digit-triggered gameplay transition"
        );
    }
}
