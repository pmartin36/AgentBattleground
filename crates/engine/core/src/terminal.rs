//! Shared terminal-restore primitive (spec 43, Decision 5).
//!
//! Extracted from `crates/game/src/app.rs`'s `TerminalGuard::drop` so the
//! panic hook (bucket b3) and the game's normal-exit path (b6-t1) both call
//! the identical, idempotent restore sequence.

/// Idempotent terminal restore: leaves raw mode, disables mouse capture,
/// leaves the alternate screen, shows the cursor. Every call ignores errors
/// so it is safe to invoke even when the terminal was never put into raw
/// mode / alt-screen, and safe to call more than once in a row.
pub fn restore_terminal() {
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show,
    );
}

#[cfg(test)]
mod tests {
    use super::restore_terminal;
    use crossterm::terminal::{enable_raw_mode, is_raw_mode_enabled};

    /// After `restore_terminal()`, the terminal must not be in raw mode.
    /// Degrades gracefully in a headless/no-TTY CI sandbox where
    /// `enable_raw_mode` itself can error (crossterm operates on
    /// `/dev/tty` on Linux, not stdout).
    #[test]
    fn restore_terminal_leaves_raw_mode() {
        if enable_raw_mode().is_ok() {
            restore_terminal();
            assert!(!is_raw_mode_enabled().unwrap());
        }
    }

    /// Calling it twice in a row must not panic, and raw mode must still be
    /// left disabled — this needs no TTY, so it runs unconditionally.
    #[test]
    fn restore_terminal_is_idempotent() {
        restore_terminal();
        restore_terminal();
        if is_raw_mode_enabled().is_ok() {
            assert!(!is_raw_mode_enabled().unwrap());
        }
    }
}
