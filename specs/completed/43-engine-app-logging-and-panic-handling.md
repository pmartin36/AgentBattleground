> # ✅ DONE! — Completed 2026-07-09
> Status: implemented via the `tdd-pipeline`, all tasks GREEN, full workspace gate clean. Manually verified beyond the automated gate: read the dedicated panic-hook integration test (isolated in its own test binary since `panic::set_hook` is global process state) and confirmed it genuinely triggers a panic via `catch_unwind` and asserts the log file captures the message at ERROR while the terminal actually leaves raw mode — real behavioral proof, not a compile check.

# Engine App Logging & Panic Handling

## Purpose
The game has zero logging infrastructure today — no `log`/`tracing` crate, no log files, nothing. When it hangs or panics during a play session, there is nothing to inspect afterward: no trail of what the app was doing, no captured panic message (and even if the default panic hook fired, its message is invisible — it prints while the terminal is still in raw/alternate-screen mode, so it's swallowed the instant `TerminalGuard::drop` restores the screen). This spec adds a durable, extensible logging facility at the engine layer, plus a panic hook that actually surfaces its message to the user and to the log.

This is squarely a cross-cutting mechanism per CLAUDE.md's engine/game boundary rule: any future game built on this engine will want the same thing, and every caller (present and future) should inherit it automatically rather than opting in.

## Scope
- **Engine** (`crates/engine/core/src/logging.rs`, new): `tracing`-based structured logging — one log file per process run, in an OS-standard data directory, with startup retention pruning and a level filter overridable via `RUST_LOG`.
- **Engine** (`crates/engine/core/src/terminal.rs`, new): a single `restore_terminal()` function, extracted so both the panic hook and the game crate's existing `TerminalGuard` call the same idempotent restore logic instead of duplicating it.
- **Engine**: a panic hook, installed by `logging::init`, that restores the terminal *before* the panic message prints (so it's actually visible), logs the panic (message + location + backtrace) at ERROR, then re-invokes the previously-installed hook so behavior otherwise matches stock Rust.
- **Engine** (`crates/engine/core/src/scene/manager.rs`): `SceneManager` logs an INFO line on every scene transition (`from -> to`) — the single existing chokepoint, so this is "free" instrumentation, not per-scene opt-in.
- **Engine** (`crates/engine/core/src/net/ipc_server.rs`, `inspect.rs`): log IPC connect/disconnect and each received command at DEBUG — the other plausible source of a silent hang (blocking on a socket).
- **Game** (`crates/game/src/main.rs`): calls `engine_core::logging::init("agent-battleground")` as the very first thing, before anything else runs; holds the returned `LoggingHandle` for the entire process lifetime (its `WorkerGuard` must not drop early — see Decision 1/2); prints the resulting log file path to stdout (before the alternate screen is entered, same existing convention as the IPC socket-path printout).
- **Game** (`crates/game/src/app.rs`): `TerminalGuard::drop` calls the new shared `engine_core::terminal::restore_terminal()` instead of its own inlined crossterm calls (removes duplication, doesn't change behavior).

Out of scope (confirmed with the project owner):
- Any active hang/stall watchdog (a thread that flags "the frame loop hasn't ticked in N seconds"). V1 is a general logging trail you can read *after* the fact — not an active detector. A future spec can add one if the trail alone proves insufficient.
- Wiring the `inspector` binary (`crates/engine/inspector`) up to this facility. The facility itself lives in engine-core with zero game-crate coupling, so inspector can adopt it later by calling the same `logging::init` — no rework needed then, just not done now.
- Per-frame/per-input trace logging at 30fps volumes — v1's baseline instrumentation is discrete events (transitions, IPC, panics, startup/shutdown), not a firehose. Any future code is free to add `tracing::debug!`/`trace!` calls anywhere; the facility supports it, this spec just doesn't add that volume itself.
- Log shipping, remote aggregation, structured JSON output — this is a local single-player terminal app; plain text a human tails locally is sufficient.

## Decisions (v1)

### 1. `tracing`, with `tracing-appender::non_blocking` — the render thread never blocks on log I/O
`tracing` is the standard modern choice over plain `log`+`env_logger` when structured fields and level filtering matter, and it's what the ecosystem (`tracing-subscriber`, `tracing-appender`) is built around.

The writer is **async** (`tracing_appender::non_blocking`): the calling thread formats the event and pushes it onto a channel; a dedicated background thread drains the channel and does the actual file write. Reasoning, not analogy: the game/render thread should never be at the mercy of disk latency — a slow/sleeping disk, a network-mounted home directory, or ordinary OS scheduling noise can turn a "cheap" blocking write into a multi-millisecond stall, and at a 33ms/frame budget that's a directly visible hitch. Unreal Engine backs this shape directly and is a verified precedent, not an assumed one: `UE_LOG` output is buffered by default, and Unreal ships a `-ForceLogFlush` command-line flag specifically to force immediate per-line disk writes when crash-debugging — documented by Epic as an opt-in, perf-costly mode for exactly that scenario, not the default. (Unity's `Debug.Log`, by contrast, is synchronous on the calling/main thread with no built-in async offload — Unity's own guidance is to avoid calling it in hot paths and gate it manually; it does not support "async is what every engine does," and isn't cited as precedent here — the reasoning above stands on its own regardless.)

This does **not** weaken the panic/crash case: `non_blocking` returns a `WorkerGuard` whose `Drop` blocks until its channel is fully drained. `logging::init` returns this guard; `main()` holds it for the entire process lifetime, so when a panic unwinds up through `main()`, the guard's drop runs during that unwind and guarantees the panic's own log line (and anything queued before it) is flushed before the process actually exits — no data lost for the scenario this spec exists to fix. The one honest gap: checking the log file *while the process is still hung and hasn't exited* may miss the last few milliseconds of activity, since the background thread hasn't drained yet at the exact instant you look. In practice this lag is small (the worker thread drains continuously, not in large batches) and is an accepted, standard trade — not something worth adding a forced-flush mode for in v1; call sites stay low-frequency (Scope) specifically so this gap stays negligible.

### 2. One log file per process run, in the OS-standard data directory, with retention
```rust
// crates/engine/core/src/logging.rs
pub struct LoggingHandle {
    pub log_path: PathBuf,
    _guard: tracing_appender::non_blocking::WorkerGuard, // held for process lifetime; drop() flushes fully
}
pub fn init(app_name: &str) -> io::Result<LoggingHandle> // real entry point, resolves the OS dir via `directories`
fn init_at(dir: &Path) -> io::Result<LoggingHandle>      // testable primitive: caller supplies the base dir
fn prune_old_logs(dir: &Path, keep: usize)               // deletes all but the `keep` most-recently-named run logs
```
- Directory: `directories::ProjectDirs::from("", "", app_name)`'s data dir + `/logs/` (e.g. `~/.local/share/agent-battleground/logs/` on Linux). New workspace dependencies: `directories`, `tracing-appender`.
- Filename: `game-<unix-epoch-seconds>.log` — one per run, lexicographically sortable (so pruning-by-name and pruning-by-recency agree), no new date-formatting dependency needed.
- Retention: `init` prunes down to the most recent **20** run-logs on every startup (`prune_old_logs(dir, 20)`) — deterministic, no unbounded growth across many play sessions, no manual cleanup ever required.
- `init` returns a `LoggingHandle` — `main()` binds it to a variable held for the whole process lifetime (`let _log = engine_core::logging::init(..)?;`), prints `log_path` to stdout immediately, before `TerminalGuard` enters the alternate screen (same visibility convention this codebase already uses for the IPC socket path), and never drops it early — dropping early would tear down the background writer thread while the app is still running.

### 3. Level filter via `RUST_LOG`, default `info`
`EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))` — respects `RUST_LOG` when set (so turning up verbosity for a specific investigation needs no code change), defaults to `info` otherwise. Text format (not JSON): timestamp, level, target (module path — distinguishes an `engine_core::` line from a `game::` line at a glance), message, fields. ANSI color codes disabled (`with_ansi(false)`) since the destination is a plain file.

### 4. Panic hook: restore terminal first, then log, then delegate
```rust
// installed inside logging::init, unconditionally — every caller inherits it, no opt-in
let previous = std::panic::take_hook();
std::panic::set_hook(Box::new(move |info| {
    engine_core::terminal::restore_terminal(); // idempotent; safe even pre-raw-mode or if TerminalGuard restores again later
    tracing::error!(
        panic = %info,
        backtrace = %std::backtrace::Backtrace::force_capture(),
        "panicked"
    );
    previous(info); // stock "thread 'main' panicked at ..." message — now actually visible, terminal already restored
}));
```
Today, a panic's default message prints while the terminal is still in raw/alternate-screen mode — it's swallowed the instant `TerminalGuard::drop` runs during unwinding, so the user never sees it. Restoring the terminal *inside the hook itself*, before delegating to the previous hook, fixes this directly rather than working around it. `TerminalGuard::drop`'s later restore (Decision 6) is then a harmless no-op repeat.

### 5. `restore_terminal()` extracted to engine-core, shared
```rust
// crates/engine/core/src/terminal.rs
pub fn restore_terminal() {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show,
    );
}
```
Exactly the four calls `TerminalGuard::drop` (`crates/game/src/app.rs`) already makes, moved so the panic hook (engine-core) and `TerminalGuard` (game crate) call the same function instead of the game crate's copy being the only copy. `TerminalGuard::drop` is updated to call it; no behavior change for the existing non-panic exit path.

### 6. `SceneManager` logs every transition
One `tracing::info!(from = ?prev_id, to = ?new_id, "scene transition")` where `SceneManager` actually applies a pending transition (the existing chokepoint every scene switch already passes through, per `14-scene-architecture`). This alone answers "what scene was active right before it froze" without any per-scene code.

### 7. IPC/inspector connection and command logging
In `engine_core::net::ipc_server` / `inspect`: `tracing::debug!` on socket accept/connect, on each `Command` received off the wire, and on disconnect. Covers the other realistic hang source (blocked on a socket read/write) with the same "last thing logged" forensic value.

### 8. Startup and unhandled-error logging
`main.rs` logs an INFO line at startup (log file path, plus whatever version/build info is cheaply available) right after `init()`. The existing `eprintln!("{e}")` path for a `resolve_boot` CLI error additionally logs the same message at ERROR, so a CLI-arg mistake shows up in the log trail too, not just stderr.

## Where the code lives
| Decision | Crate | Files |
|---|---|---|
| 1–3. `logging::init`, async writer + guard, filter, format | **Engine** | `crates/engine/core/src/logging.rs` (new) |
| 2. `directories`, `tracing-appender` dependencies | **Engine** | `crates/engine/core/Cargo.toml`, workspace `Cargo.toml` |
| 4. Panic hook | **Engine** | `crates/engine/core/src/logging.rs` |
| 5. `restore_terminal()` | **Engine** | `crates/engine/core/src/terminal.rs` (new) |
| 5. `TerminalGuard::drop` calls the shared fn | **Game** | `crates/game/src/app.rs` |
| 6. Scene-transition logging | **Engine** | `crates/engine/core/src/scene/manager.rs` |
| 7. IPC/inspect logging | **Engine** | `crates/engine/core/src/net/ipc_server.rs`, `crates/engine/core/src/net/inspect.rs` |
| 8. `main.rs` wiring + startup/error logging | **Game** | `crates/game/src/main.rs` |

No inspector-binary wiring in v1 (Purpose/Scope) — the facility is fully usable by it later with zero rework, since it carries no game-crate coupling.

## Open Questions / TBDs
None outstanding — log location (OS-standard data dir via `directories`), hang-detection scope (general logging only, no watchdog), and inspector scope (game binary only for v1) were confirmed with the project owner before writing this spec. Retention count (20 files) and filter default (`info`) are pinned above as reasonable defaults, not TBDs — trivially adjustable constants if they prove wrong in practice.

## Dependencies
- `31-engine-game-crate-split` ✅ — this spec's engine/game placement follows that split directly: the facility and panic hook are pure engine-core (zero game-crate coupling), `main.rs`/`app.rs` wiring is the only game-crate change.
- `14-scene-architecture` ✅ — `SceneManager`'s existing transition-application path is the chokepoint Decision 6 instruments; no change to its transition logic itself.
