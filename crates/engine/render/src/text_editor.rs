//! Multi-line text editor widget (spec 50) — config/event types and the
//! text round-trip primitive. Editing, movement, scrolling, and rendering
//! are added by later tasks in this same module.

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
    // Read by later tasks (rendering, submit-on-enter handling); not yet
    // consumed by this task's text round-trip surface.
    #[allow(dead_code)]
    config: TextEditorConfig,
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
}

#[cfg(test)]
#[path = "text_editor_tests.rs"]
mod text_editor_tests;
