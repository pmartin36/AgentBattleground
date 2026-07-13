//! Soft-wrap layout model: wrapping logical lines to a display width and
//! mapping between logical caret position and display `(row, col)`.

use super::{TextEditor, WrapRow};

impl TextEditor {
    /// Wrap every logical line to `width` display columns, top-to-bottom.
    /// Greedy word-wrap; an over-long token (no space in the row window)
    /// splits into width-sized char chunks. Offsets partition each logical
    /// line; every line yields at least one row.
    pub(super) fn wrap_rows(&self, width: usize) -> Vec<WrapRow> {
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
    pub(super) fn cursor_display_pos(&self, width: usize) -> (usize, usize) {
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

    /// Reverse-map a display `(row_idx, desired_col)` back to a logical
    /// `(line, col)` caret position, consistent with `cursor_display_pos`'s
    /// boundary rule: on a non-last wrapped row the caret's max column is the
    /// last VISIBLE char, never the wrap-boundary column (which belongs to
    /// the continuation row).
    pub(super) fn set_from_display(&mut self, rows: &[WrapRow], row_idx: usize, desired_col: usize) {
        let row = rows[row_idx];
        let row_width = row.end - row.start;
        let is_last_row = row.end == self.lines[row.line].chars().count();
        let max_col = if is_last_row { row_width } else { row_width.saturating_sub(1) };
        self.cursor_line = row.line;
        self.cursor_col = row.start + desired_col.min(max_col);
    }
}
