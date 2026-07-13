//! Multi-line text editor widget (spec 50) — config/event types, the text
//! round-trip primitive, editing, movement, scrolling, and rendering.

use std::time::Duration;

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

mod render;
mod wrap;

/// Display rows scrolled per mouse wheel notch (one `ScrollUp`/`ScrollDown`
/// event = one notch, crossterm's model).
const WHEEL_SCROLL_ROWS: usize = 3;

/// Blink half-cycle: caret is visible for one `BLINK_PERIOD`, hidden for the
/// next (full cycle = `2 * BLINK_PERIOD`). b1-t1.
const BLINK_PERIOD: Duration = Duration::from_millis(600);

/// How the editor's row count behaves: fixed at the caller's rect height,
/// or grows with content up to `max_rows`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sizing {
    Fixed,
    Grow { max_rows: u16 },
}

/// Static configuration for a [`TextEditor`].
#[derive(Debug, Clone)]
pub struct TextEditorConfig {
    pub sizing: Sizing,
    pub submit_on_enter: bool,
    pub placeholder: String,
}

/// Result of feeding an input event into the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorEvent {
    None,
    Changed,
    Submit,
}

/// Multi-line text editor state: logical line buffer + cursor position.
pub struct TextEditor {
    config: TextEditorConfig,
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
    /// Topmost visible display-row index into `wrap_rows`. Driven by
    /// movement (this task) and explicit scroll (b2-t3); read by render (b3).
    scroll_offset: usize,
    /// Cached render width in cells; 0 until `render()` (b3) sets it. Tests
    /// seed this directly to exercise wrap-aware movement.
    viewport_width: usize,
    /// Cached render height in display rows; 0 until `render()` (b3) sets
    /// it. Tests seed this directly to exercise viewport-follows-cursor.
    viewport_height: usize,
    /// Cached render-rect origin x; 0 until `render()` sets it. Tests seed
    /// this directly to exercise click-to-place (b1-t2).
    viewport_x: u16,
    /// Cached render-rect origin y; 0 until `render()` sets it. Tests seed
    /// this directly to exercise click-to-place (b1-t2).
    viewport_y: u16,
    /// Blink phase accumulator, `0..(2*BLINK_PERIOD)`. Advanced by `tick`;
    /// reset to `Duration::ZERO` on every keyboard mutation/movement.
    blink_accum: Duration,
    /// Whether this editor currently has input focus. An unfocused editor
    /// never draws a caret regardless of blink phase.
    focused: bool,
}

/// One display row produced by wrapping a logical line: `[start, end)` are
/// char offsets into `lines[line]`. Rows partition the logical line —
/// contiguous, gap-free, every char covered (`start == end` for an empty
/// logical line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WrapRow {
    line: usize,
    start: usize,
    end: usize,
}

impl TextEditor {
    /// New editor with an empty single-line buffer.
    pub fn new(config: TextEditorConfig) -> Self {
        Self {
            config,
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
            scroll_offset: 0,
            viewport_width: 0,
            viewport_height: 0,
            viewport_x: 0,
            viewport_y: 0,
            blink_accum: Duration::ZERO,
            focused: true,
        }
    }

    /// Advance the blink accumulator by `dt`, wrapping at the full cycle
    /// (`2 * BLINK_PERIOD`).
    pub fn tick(&mut self, dt: Duration) {
        let full = (BLINK_PERIOD * 2).as_nanos();
        let acc = (self.blink_accum + dt).as_nanos() % full;
        self.blink_accum = Duration::from_nanos(acc as u64);
    }

    /// Set whether this editor currently has input focus; an unfocused
    /// editor never draws a caret regardless of blink phase.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// `true` iff the caret should be drawn this frame: focused AND the
    /// blink accumulator is in the visible half of its cycle. Test hook per
    /// spec line 57.
    pub fn caret_visible(&self) -> bool {
        self.focused && self.blink_accum < BLINK_PERIOD
    }

    /// Reset the blink accumulator to the visible phase. Called from every
    /// keyboard mutation/movement primitive so the caret never blinks off
    /// mid-interaction.
    fn reset_blink(&mut self) {
        self.blink_accum = Duration::ZERO;
    }

    /// Replace the buffer contents. `text` is split on `'\n'` into logical
    /// lines (never `str::lines()`, which would drop a trailing newline).
    /// Cursor is clamped to the end of the new text.
    pub fn set_text(&mut self, text: &str) {
        self.lines = text.split('\n').map(String::from).collect();
        self.cursor_line = self.lines.len() - 1;
        self.cursor_col = self.lines[self.cursor_line].chars().count();
    }

    /// Join the logical lines back into a single string with `'\n'`.
    /// Exact inverse of [`Self::set_text`].
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Feed a key event into the editor: printable chars insert, Backspace/
    /// Delete remove, Enter inserts a newline (or submits, per
    /// `config.submit_on_enter`). Left/Right/Up/Down/Home/End reposition the
    /// caret (see `move_cursor`) and scroll the viewport to keep it visible.
    /// PageUp/PageDown scroll the viewport by one page without moving the
    /// caret or mutating the buffer.
    pub fn handle_key(&mut self, key: KeyEvent) -> EditorEvent {
        match key.code {
            KeyCode::Char(c) => {
                self.insert_char(c);
                EditorEvent::Changed
            }
            KeyCode::Enter => {
                if self.config.submit_on_enter && !key.modifiers.contains(KeyModifiers::SHIFT) {
                    EditorEvent::Submit
                } else {
                    self.insert_newline();
                    EditorEvent::Changed
                }
            }
            KeyCode::Backspace => {
                if self.backspace() {
                    EditorEvent::Changed
                } else {
                    EditorEvent::None
                }
            }
            KeyCode::Delete => {
                if self.delete() {
                    EditorEvent::Changed
                } else {
                    EditorEvent::None
                }
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End => {
                self.move_cursor(key.code);
                EditorEvent::None
            }
            KeyCode::PageUp => {
                self.scroll_by(-(self.viewport_height.max(1) as isize));
                EditorEvent::None
            }
            KeyCode::PageDown => {
                self.scroll_by(self.viewport_height.max(1) as isize);
                EditorEvent::None
            }
            _ => EditorEvent::None,
        }
    }

    /// Reposition the logical caret per `code` (Left/Right/Up/Down/Home/End)
    /// over the wrapped display model, then scroll the viewport to keep it
    /// visible. No buffer mutation.
    fn move_cursor(&mut self, code: KeyCode) {
        self.reset_blink();
        match code {
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End => {
                let w = self.viewport_width.max(1);
                let rows = self.wrap_rows(w);
                let (cur_row, cur_col) = self.cursor_display_pos(w);
                let (target_row, desired_col) = match code {
                    KeyCode::Up => (cur_row.saturating_sub(1), cur_col),
                    KeyCode::Down => ((cur_row + 1).min(rows.len() - 1), cur_col),
                    KeyCode::Home => (cur_row, 0),
                    KeyCode::End => (cur_row, usize::MAX),
                    _ => unreachable!(),
                };
                self.set_from_display(&rows, target_row, desired_col);
            }
            _ => {}
        }
        self.scroll_to_cursor();
    }

    /// Move the caret one char left, purely logically (width-independent).
    /// At column 0, wraps to the end of the previous logical line.
    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.lines[self.cursor_line].chars().count();
        }
    }

    /// Move the caret one char right, purely logically (width-independent).
    /// At line end, wraps to column 0 of the next logical line.
    fn move_right(&mut self) {
        let len = self.lines[self.cursor_line].chars().count();
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
    }

    /// Insert `c` at the logical caret, advancing the caret one char.
    fn insert_char(&mut self, c: char) {
        self.reset_blink();
        let line = &mut self.lines[self.cursor_line];
        let byte = line
            .char_indices()
            .nth(self.cursor_col)
            .map(|(b, _)| b)
            .unwrap_or(line.len());
        line.insert(byte, c);
        self.cursor_col += 1;
    }

    /// Split the current line at the caret into two logical lines, moving
    /// the caret to the start of the new (second) line.
    fn insert_newline(&mut self) {
        self.reset_blink();
        let line = &mut self.lines[self.cursor_line];
        let byte = line
            .char_indices()
            .nth(self.cursor_col)
            .map(|(b, _)| b)
            .unwrap_or(line.len());
        let tail = line.split_off(byte);
        self.lines.insert(self.cursor_line + 1, tail);
        self.cursor_line += 1;
        self.cursor_col = 0;
    }

    /// Remove the char before the caret, or join with the previous line at
    /// column 0. Returns `true` if it mutated the buffer, `false` at the
    /// buffer start (line 0, col 0).
    fn backspace(&mut self) -> bool {
        self.reset_blink();
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_line];
            let byte = line
                .char_indices()
                .nth(self.cursor_col - 1)
                .map(|(b, _)| b)
                .unwrap();
            line.remove(byte);
            self.cursor_col -= 1;
            true
        } else if self.cursor_line > 0 {
            let cur = self.lines.remove(self.cursor_line);
            self.cursor_line -= 1;
            self.cursor_col = self.lines[self.cursor_line].chars().count();
            self.lines[self.cursor_line].push_str(&cur);
            true
        } else {
            false
        }
    }

    /// Feed a mouse event into the editor: wheel up/down scrolls the
    /// viewport (see `scroll_by`); a left click inside the cached content
    /// rect places the caret at the clicked cell (via `wrap_rows` +
    /// `set_from_display`) and resets the blink to visible. Returns `true`
    /// iff the event was handled (scroll actually moved `scroll_offset`, or
    /// an in-rect click placed the caret); `false` for a clamped scroll
    /// no-op, an out-of-rect click, or any other event.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> bool {
        match ev.kind {
            MouseEventKind::ScrollDown => self.scroll_by(WHEEL_SCROLL_ROWS as isize),
            MouseEventKind::ScrollUp => self.scroll_by(-(WHEEL_SCROLL_ROWS as isize)),
            MouseEventKind::Down(MouseButton::Left) => {
                // Out-of-content-rect guard (also catches above/left via u16
                // underflow avoidance).
                if ev.column < self.viewport_x || ev.row < self.viewport_y {
                    return false;
                }
                let local_col = (ev.column - self.viewport_x) as usize;
                let local_row = (ev.row - self.viewport_y) as usize;
                if local_col >= self.viewport_width || local_row >= self.viewport_height {
                    return false;
                }
                let width = self.viewport_width.max(1);
                let rows = self.wrap_rows(width);
                let display_row = (self.scroll_offset + local_row).min(rows.len() - 1);
                self.set_from_display(&rows, display_row, local_col);
                self.reset_blink();
                true
            }
            _ => false,
        }
    }

    /// Rows the caller should allot at `width_cells`. Grow: wrapped
    /// display-row count clamped to `[1, max_rows]` (at max_rows the widget
    /// scrolls instead of growing). Fixed: natural wrapped row count,
    /// floored at 1. Side-effect-free: does not read or write viewport
    /// state.
    pub fn desired_rows(&self, width_cells: u16) -> u16 {
        let rows = self.wrap_rows(width_cells.max(1) as usize).len();
        let capped = match self.config.sizing {
            Sizing::Grow { max_rows } => rows.min(max_rows as usize),
            Sizing::Fixed => rows,
        };
        capped.max(1).min(u16::MAX as usize) as u16
    }

    /// Remove the char at the caret, or join the next line up at
    /// end-of-line. Returns `true` if it mutated the buffer, `false` at the
    /// buffer end.
    fn delete(&mut self) -> bool {
        self.reset_blink();
        let len = self.lines[self.cursor_line].chars().count();
        if self.cursor_col < len {
            let line = &mut self.lines[self.cursor_line];
            let byte = line
                .char_indices()
                .nth(self.cursor_col)
                .map(|(b, _)| b)
                .unwrap();
            line.remove(byte);
            true
        } else if self.cursor_line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_line + 1);
            self.lines[self.cursor_line].push_str(&next);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
#[path = "../text_editor_tests.rs"]
mod text_editor_tests;
