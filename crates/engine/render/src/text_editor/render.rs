//! Rendering: visible wrapped text, caret, and scrollbar; plus the scroll
//! helpers that keep `scroll_offset` consistent with the wrapped layout.

use engine_core::color::Rgba;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::dots::{Dot, DotBuffer};

use super::{TextEditor, WrapRow};

/// Dim ink color for the scrollbar thumb (see `draw_scrollbar`).
const SCROLLBAR_COLOR: Rgba = Rgba::rgb(0x5a, 0x5a, 0x5a);

/// Background tint for the selection highlight (b1-t5). A muted blue tint,
/// not `Modifier::REVERSED` — REVERSED is reserved for the caret so the two
/// stay visually distinguishable where they overlap.
pub(super) const SELECTION_BG: Color = Color::Rgb(0x2f, 0x4f, 0x6f);

impl TextEditor {
    /// Total wrapped display rows at the current cached viewport width.
    pub(super) fn total_display_rows(&self) -> usize {
        self.wrap_rows(self.viewport_width.max(1)).len()
    }

    /// Largest valid `scroll_offset`: the last display row can sit at the
    /// bottom of the viewport.
    pub(super) fn max_scroll_offset(&self) -> usize {
        self.total_display_rows()
            .saturating_sub(self.viewport_height.max(1))
    }

    /// Move `scroll_offset` by `delta` display rows, clamped to
    /// `[0, max_scroll_offset()]`. Returns `true` iff the offset actually
    /// changed. Touches only `scroll_offset` — never the caret or buffer.
    pub(super) fn scroll_by(&mut self, delta: isize) -> bool {
        let max = self.max_scroll_offset() as isize;
        let new = (self.scroll_offset as isize + delta).clamp(0, max) as usize;
        if new != self.scroll_offset {
            self.scroll_offset = new;
            true
        } else {
            false
        }
    }

    /// Adjust `scroll_offset` so the caret's current display row stays
    /// within `[scroll_offset, scroll_offset + viewport_height)`.
    pub(super) fn scroll_to_cursor(&mut self) {
        let row = self.cursor_display_pos(self.viewport_width.max(1)).0;
        let h = self.viewport_height.max(1);
        if row < self.scroll_offset {
            self.scroll_offset = row;
        } else if row >= self.scroll_offset + h {
            self.scroll_offset = row + 1 - h;
        }
    }

    /// Render the visible wrapped rows as plain text, a reverse-video block
    /// cursor, and (when the buffer is empty) a dimmed placeholder. Caches
    /// `viewport_width`/`viewport_height` from `rect` (re-clamping
    /// `scroll_offset` in case the rect shrank since the last input event).
    /// Text is drawn as plain terminal chars (the rule-4 exception); as the
    /// last step, draws a dot-pipeline vertical scrollbar in the right-most
    /// cell column when content overflows the viewport (see
    /// [`Self::draw_scrollbar`]).
    pub fn render(&mut self, buf: &mut Buffer, rect: Rect) {
        self.viewport_width = rect.width as usize;
        self.viewport_height = rect.height as usize;
        self.viewport_x = rect.x;
        self.viewport_y = rect.y;
        self.scroll_offset = self.scroll_offset.min(self.max_scroll_offset());

        if self.text().is_empty() {
            let style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);
            crate::wrapped_text(buf, rect, &self.config.placeholder, crate::TextAlign::Left, style, false);
            // Caret at position 0 for an empty buffer — otherwise clicking into
            // an empty box would leave the caret invisible.
            if self.caret_visible() {
                if let Some(cell) = buf.cell_mut((rect.x, rect.y)) {
                    cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
                }
            }
            return;
        }

        let width = (rect.width as usize).max(1);
        let height = rect.height as usize;
        let rows = self.wrap_rows(width);
        let text_style = Style::default();

        let end = (self.scroll_offset + height).min(rows.len());
        for (i, row) in rows[self.scroll_offset..end].iter().enumerate() {
            let y = rect.y + i as u16;
            if y >= buf.area.bottom() {
                continue;
            }
            let line = &self.lines[row.line];
            let s: String = line.chars().skip(row.start).take(row.end - row.start).collect();
            buf.set_stringn(rect.x, y, &s, width, text_style);
        }

        self.draw_selection(buf, rect, &rows);

        if self.caret_visible() {
            let (crow, ccol) = self.cursor_display_pos(width);
            if crow >= self.scroll_offset && crow < self.scroll_offset + height {
                let cy = rect.y + (crow - self.scroll_offset) as u16;
                let cx = rect.x + ccol as u16;
                if cx < rect.right() {
                    if let Some(cell) = buf.cell_mut((cx, cy)) {
                        cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
                    }
                }
            }
        }

        self.draw_scrollbar(buf, rect);
    }

    /// Paint the selection highlight (a background tint, not `REVERSED`) on
    /// every visible display cell whose logical column falls within the
    /// normalized `selection_span()`. `rows` is the caller's already-wrapped
    /// row vector (no re-wrap); iterates the same
    /// `scroll_offset..scroll_offset+height` window as the text loop so
    /// highlighted cells line up with the drawn glyphs. A no-op when there is
    /// no active selection.
    fn draw_selection(&self, buf: &mut Buffer, rect: Rect, rows: &[WrapRow]) {
        let Some(((sl, sc), (el, ec))) = self.selection_span() else {
            return;
        };
        let height = rect.height as usize;
        let end = (self.scroll_offset + height).min(rows.len());
        for (i, row) in rows[self.scroll_offset..end].iter().enumerate() {
            if row.line < sl || row.line > el {
                continue;
            }
            let y = rect.y + i as u16;
            if y >= buf.area.bottom() {
                continue;
            }
            let sel_start = if row.line == sl { sc } else { 0 };
            let sel_end = if row.line == el { ec } else { self.lines[row.line].chars().count() };
            let hl_start = sel_start.max(row.start);
            let hl_end = sel_end.min(row.end);
            if hl_start >= hl_end {
                continue;
            }
            for col in hl_start..hl_end {
                let x = rect.x + (col - row.start) as u16;
                if x >= rect.right() {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_bg(SELECTION_BG);
                }
            }
        }
    }

    /// Draw a vertical scrollbar THUMB in the right-most terminal cell column
    /// of `rect` through the dot pipeline, when `total_display_rows()` exceeds
    /// the viewport height. A no-op when content fits.
    ///
    /// Thumb only — no full-height track: a short 2-dot-wide dim segment sized
    /// and positioned to the visible fraction, so it reads as a scrollbar
    /// indicator rather than a solid bar down the field.
    fn draw_scrollbar(&self, buf: &mut Buffer, rect: Rect) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let Some((thumb_top, thumb_dots, track_dots)) = self.scrollbar_thumb(rect.height as usize)
        else {
            return;
        };

        let mut dotbuf = DotBuffer::new(2, track_dots);
        for row in thumb_top..(thumb_top + thumb_dots) {
            dotbuf.set(0, row, Dot::Lit(SCROLLBAR_COLOR));
            dotbuf.set(1, row, Dot::Lit(SCROLLBAR_COLOR));
        }

        let area = Rect::new(rect.right().saturating_sub(1), rect.y, 1, rect.height);
        crate::draw_dots(buf, area, &dotbuf);
    }

    /// Scrollbar thumb geometry at viewport height `view`, in dot units:
    /// `(thumb_top_dots, thumb_dots, track_dots)`. `None` when content fits
    /// (`total_display_rows() <= view`) — the single source of truth shared
    /// by [`Self::draw_scrollbar`] and the mouse hit-test/drag math (b3-t1),
    /// so the interactive thumb always agrees with what's rendered.
    pub(super) fn scrollbar_thumb(&self, view: usize) -> Option<(usize, usize, usize)> {
        let total = self.total_display_rows();
        if view == 0 || total <= view {
            return None;
        }
        let track_dots = view * 4;
        let max_scroll = self.max_scroll_offset();
        let thumb_dots = ((view * track_dots) / total).clamp(4, track_dots);
        let max_top = track_dots - thumb_dots;
        // `max_scroll` is `total - view`, which is > 0 here: the `total <=
        // view` early-return above already ruled out the fits-viewport case.
        let thumb_top = self.scroll_offset * max_top / max_scroll;
        Some((thumb_top, thumb_dots, track_dots))
    }

    /// The terminal cell column the scrollbar thumb/track occupies: the
    /// right-most column of the cached content viewport. b3-t1.
    pub(super) fn scrollbar_col(&self) -> u16 {
        self.viewport_x + self.viewport_width.saturating_sub(1) as u16
    }

    /// `true` iff `(column, row)` is inside the scrollbar's cell column AND
    /// content currently overflows (`max_scroll_offset() > 0`) — mirrors
    /// `draw_scrollbar`'s early-return so a click in the last column is only
    /// ever routed to the scrollbar when a thumb is actually drawn there.
    /// b3-t1.
    pub(super) fn scrollbar_hit(&self, column: u16, row: u16) -> bool {
        self.max_scroll_offset() > 0
            && column == self.scrollbar_col()
            && row >= self.viewport_y
            && row < self.viewport_y + self.viewport_height as u16
    }

    /// Begin a scrollbar drag at the `Down(Left)` cell `row`: sets
    /// `scrollbar_drag = true`, then pages the viewport if the press landed
    /// above/below the thumb (track paging), or does nothing if it landed on
    /// the thumb itself. Never touches the caret. b3-t1.
    pub(super) fn begin_scrollbar_drag(&mut self, row: u16) {
        self.scrollbar_drag = true;
        let Some((thumb_top_dots, thumb_dots, _track_dots)) =
            self.scrollbar_thumb(self.viewport_height)
        else {
            return;
        };
        let top_cell = thumb_top_dots / 4;
        let bot_cell = (thumb_top_dots + thumb_dots - 1) / 4;
        let local_y = row.saturating_sub(self.viewport_y) as usize;
        if local_y < top_cell {
            self.scroll_by(-(self.viewport_height as isize));
        } else if local_y > bot_cell {
            self.scroll_by(self.viewport_height as isize);
        }
    }

    /// Map an in-progress scrollbar drag's cell `row` to `scroll_offset`,
    /// proportionally over the track: top row -> 0, bottom row ->
    /// `max_scroll_offset()`, interior row -> floor(local_y *
    /// max_scroll_offset() / (view-1)). Clamps `row` into the track first.
    /// No caret/buffer mutation. b3-t1.
    pub(super) fn scrollbar_drag_to(&mut self, row: u16) {
        let view = self.viewport_height.max(1);
        let local_y = (row.max(self.viewport_y) - self.viewport_y) as usize;
        let local_y = local_y.min(view - 1);
        let denom = view.saturating_sub(1).max(1);
        let max = self.max_scroll_offset();
        self.scroll_offset = ((local_y * max) / denom).min(max);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Sizing, TextEditorConfig};
    use super::*;

    fn config() -> TextEditorConfig {
        TextEditorConfig {
            sizing: Sizing::Fixed,
            submit_on_enter: false,
            placeholder: String::new(),
        }
    }

    fn make_buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    /// Assert `bg == SELECTION_BG` for every x in `highlighted`, and
    /// `bg == Color::Reset` for every other x in `0..width`, on row `y`.
    fn assert_row_highlight(buf: &Buffer, y: u16, width: u16, highlighted: &[u16]) {
        for x in 0..width {
            let bg = buf.cell((x, y)).unwrap().bg;
            if highlighted.contains(&x) {
                assert_eq!(bg, SELECTION_BG, "expected highlight at ({x},{y})");
            } else {
                assert_eq!(bg, Color::Reset, "expected no highlight at ({x},{y})");
            }
        }
    }

    #[test]
    fn selection_highlights_exact_cells_on_one_row() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcdef");
        ed.set_selection((0, 1), (0, 4));
        let rect = Rect::new(0, 0, 10, 3);
        let mut buf = make_buf(10, 3);

        ed.render(&mut buf, rect);

        assert_row_highlight(&buf, 0, 10, &[1, 2, 3]);
    }

    #[test]
    fn selection_across_wrap_boundary_highlights_both_rows() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcdefgh"); // no spaces: wraps to "abcd" / "efgh" at width 4
        ed.set_selection((0, 2), (0, 6));
        let rect = Rect::new(0, 0, 4, 2);
        let mut buf = make_buf(4, 2);

        ed.render(&mut buf, rect);

        assert_row_highlight(&buf, 0, 4, &[2, 3]);
        assert_row_highlight(&buf, 1, 4, &[0, 1]);
    }

    #[test]
    fn multiline_selection_highlights_tail_middle_head() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abc\ndef\nghi");
        ed.set_selection((0, 1), (2, 2));
        let rect = Rect::new(0, 0, 5, 3);
        let mut buf = make_buf(5, 3);

        ed.render(&mut buf, rect);

        assert_row_highlight(&buf, 0, 5, &[1, 2]); // tail of "abc"
        assert_row_highlight(&buf, 1, 5, &[0, 1, 2]); // full middle line "def"
        assert_row_highlight(&buf, 2, 5, &[0, 1]); // head of "ghi"
    }

    #[test]
    fn no_selection_renders_no_highlight() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcdef");
        let rect = Rect::new(0, 0, 10, 3);
        let mut buf = make_buf(10, 3);

        ed.render(&mut buf, rect);

        for y in 0..3 {
            assert_row_highlight(&buf, y, 10, &[]);
        }
    }

    #[test]
    fn active_end_caret_keeps_reversed_over_highlight() {
        let mut ed = TextEditor::new(config());
        ed.set_text("abcdef");
        // Reverse selection so the caret (active end) sits at the span
        // start, inside the highlighted range.
        ed.set_selection((0, 4), (0, 1));
        ed.cursor_line = 0;
        ed.cursor_col = 1;
        let rect = Rect::new(0, 0, 10, 3);
        let mut buf = make_buf(10, 3);

        ed.render(&mut buf, rect);

        let caret = buf.cell((1, 0)).unwrap();
        assert!(caret.modifier.contains(Modifier::REVERSED), "caret must stay REVERSED over the highlight");
        assert_eq!(caret.bg, SELECTION_BG, "caret cell must still carry the highlight bg underneath");
    }

    // --- b3-t1: draggable scrollbar thumb + track paging ---

    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent { kind, column, row, modifiers: KeyModifiers::empty() }
    }

    /// 6 single-char lines rendered at width 6, height 2 -> 6 display rows,
    /// `max_scroll_offset() == 4`. No interior track cell (view-1 == 1); only
    /// exercises the endpoints. Established via `render()` so viewport dims /
    /// scrollbar geometry match exactly what `render()` itself computes.
    fn overflowing_editor_and_rect() -> (TextEditor, Rect) {
        let mut editor = TextEditor::new(config());
        editor.set_text("a\nb\nc\nd\ne\nf");
        let rect = Rect::new(0, 0, 6, 2);
        let mut buf = make_buf(rect.width, rect.height);
        editor.render(&mut buf, rect);
        (editor, rect)
    }

    /// 8 single-char lines @ height 4 -> 8 display rows, `max_scroll_offset()
    /// == 4`, view-1 == 3: has an interior track cell for the mid-track case.
    fn overflowing_editor_and_rect_tall() -> (TextEditor, Rect) {
        let mut editor = TextEditor::new(config());
        editor.set_text("a\nb\nc\nd\ne\nf\ng\nh");
        let rect = Rect::new(0, 0, 6, 4);
        let mut buf = make_buf(rect.width, rect.height);
        editor.render(&mut buf, rect);
        (editor, rect)
    }

    #[test]
    fn scrollbar_drag_to_bottom_and_back_to_top_hits_the_endpoints() {
        let (mut editor, rect) = overflowing_editor_and_rect();
        let max = editor.max_scroll_offset();
        let col = rect.right() - 1;

        editor.handle_mouse(&mouse_at(MouseEventKind::Down(MouseButton::Left), col, rect.y));
        editor.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            col,
            rect.y + rect.height - 1,
        ));
        assert_eq!(editor.scroll_offset, max, "drag to track bottom must hit max_scroll_offset()");

        editor.handle_mouse(&mouse_at(MouseEventKind::Drag(MouseButton::Left), col, rect.y));
        assert_eq!(editor.scroll_offset, 0, "drag back to track top must hit 0");
    }

    #[test]
    fn scrollbar_drag_interior_cell_maps_proportionally() {
        let (mut editor, rect) = overflowing_editor_and_rect_tall();
        let col = rect.right() - 1;

        editor.handle_mouse(&mouse_at(MouseEventKind::Down(MouseButton::Left), col, rect.y));
        editor.handle_mouse(&mouse_at(
            MouseEventKind::Drag(MouseButton::Left),
            col,
            rect.y + 2, // local_y == 2, view - 1 == 3
        ));

        assert_eq!(editor.scroll_offset, 2, "floor(2 * max_scroll_offset(4) / (view-1)(3)) == 2");
    }

    #[test]
    fn scrollbar_column_down_does_not_move_the_caret() {
        let (mut editor, rect) = overflowing_editor_and_rect();
        let col = rect.right() - 1;
        let caret_before = (editor.cursor_line, editor.cursor_col);

        editor.handle_mouse(&mouse_at(MouseEventKind::Down(MouseButton::Left), col, rect.y));

        assert_eq!(
            (editor.cursor_line, editor.cursor_col),
            caret_before,
            "a scrollbar-column press must never place the text caret"
        );
    }
}
