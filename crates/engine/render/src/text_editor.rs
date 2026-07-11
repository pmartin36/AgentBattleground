//! Multi-line text editor widget (spec 50) — config/event types and the
//! text round-trip primitive. Editing, movement, scrolling, and rendering
//! are added by later tasks in this same module.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

/// Display rows scrolled per mouse wheel notch (one `ScrollUp`/`ScrollDown`
/// event = one notch, crossterm's model).
const WHEEL_SCROLL_ROWS: usize = 3;

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
    /// Wrap every logical line to `width` display columns, top-to-bottom.
    /// Greedy word-wrap; an over-long token (no space in the row window)
    /// splits into width-sized char chunks. Offsets partition each logical
    /// line; every line yields at least one row.
    fn wrap_rows(&self, width: usize) -> Vec<WrapRow> {
        let w = width.max(1);
        let mut rows = Vec::new();
        for (line_idx, line) in self.lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let n = chars.len();
            if n == 0 {
                rows.push(WrapRow { line: line_idx, start: 0, end: 0 });
                continue;
            }
            let mut start = 0;
            while start < n {
                let max_end = (start + w).min(n);
                if max_end == n {
                    rows.push(WrapRow { line: line_idx, start, end: n });
                    break;
                }
                let space = chars[start..max_end].iter().rposition(|&c| c == ' ');
                match space {
                    Some(offset) => {
                        let end = start + offset + 1;
                        rows.push(WrapRow { line: line_idx, start, end });
                        start = end;
                    }
                    None => {
                        rows.push(WrapRow { line: line_idx, start, end: max_end });
                        start = max_end;
                    }
                }
            }
        }
        rows
    }

    /// Display `(row_index_into_wrap_rows, col_within_row)` of the logical
    /// cursor under `width` wrapping.
    fn cursor_display_pos(&self, width: usize) -> (usize, usize) {
        let rows = self.wrap_rows(width);
        let mut last_matching = 0;
        for (idx, row) in rows.iter().enumerate() {
            if row.line != self.cursor_line {
                continue;
            }
            last_matching = idx;
            if self.cursor_col < row.end {
                return (idx, self.cursor_col - row.start);
            }
        }
        let row = &rows[last_matching];
        (last_matching, self.cursor_col - row.start)
    }

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
        }
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

    /// Reverse-map a display `(row_idx, desired_col)` back to a logical
    /// `(line, col)` caret position, consistent with `cursor_display_pos`'s
    /// boundary rule: on a non-last wrapped row the caret's max column is the
    /// last VISIBLE char, never the wrap-boundary column (which belongs to
    /// the continuation row).
    fn set_from_display(&mut self, rows: &[WrapRow], row_idx: usize, desired_col: usize) {
        let row = rows[row_idx];
        let row_width = row.end - row.start;
        let is_last_row = row.end == self.lines[row.line].chars().count();
        let max_col = if is_last_row { row_width } else { row_width.saturating_sub(1) };
        self.cursor_line = row.line;
        self.cursor_col = row.start + desired_col.min(max_col);
    }

    /// Adjust `scroll_offset` so the caret's current display row stays
    /// within `[scroll_offset, scroll_offset + viewport_height)`.
    fn scroll_to_cursor(&mut self) {
        let row = self.cursor_display_pos(self.viewport_width.max(1)).0;
        let h = self.viewport_height.max(1);
        if row < self.scroll_offset {
            self.scroll_offset = row;
        } else if row >= self.scroll_offset + h {
            self.scroll_offset = row + 1 - h;
        }
    }

    /// Insert `c` at the logical caret, advancing the caret one char.
    fn insert_char(&mut self, c: char) {
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
    /// viewport (see `scroll_by`). Returns `true` iff it actually moved
    /// `scroll_offset`; `false` for a clamped no-op or any non-wheel event.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> bool {
        match ev.kind {
            MouseEventKind::ScrollDown => self.scroll_by(WHEEL_SCROLL_ROWS as isize),
            MouseEventKind::ScrollUp => self.scroll_by(-(WHEEL_SCROLL_ROWS as isize)),
            _ => false,
        }
    }

    /// Total wrapped display rows at the current cached viewport width.
    fn total_display_rows(&self) -> usize {
        self.wrap_rows(self.viewport_width.max(1)).len()
    }

    /// Largest valid `scroll_offset`: the last display row can sit at the
    /// bottom of the viewport.
    fn max_scroll_offset(&self) -> usize {
        self.total_display_rows()
            .saturating_sub(self.viewport_height.max(1))
    }

    /// Move `scroll_offset` by `delta` display rows, clamped to
    /// `[0, max_scroll_offset()]`. Returns `true` iff the offset actually
    /// changed. Touches only `scroll_offset` — never the caret or buffer.
    fn scroll_by(&mut self, delta: isize) -> bool {
        let max = self.max_scroll_offset() as isize;
        let new = (self.scroll_offset as isize + delta).clamp(0, max) as usize;
        if new != self.scroll_offset {
            self.scroll_offset = new;
            true
        } else {
            false
        }
    }

    /// Remove the char at the caret, or join the next line up at
    /// end-of-line. Returns `true` if it mutated the buffer, `false` at the
    /// buffer end.
    fn delete(&mut self) -> bool {
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
#[path = "text_editor_tests.rs"]
mod text_editor_tests;
