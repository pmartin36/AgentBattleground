//! The mad-lib definition modal: renders a `MadLibTemplate`'s sentence with
//! its blanks as inline editable slots, routes input to the focused blank,
//! and yields a completed sentence when Done is pressed with every blank
//! filled.
//!
//! This module owns the modal's state container, its pure layout (a greedy
//! word-wrap over a mixed stream of literal words and fixed-width blank
//! slots), its standalone render, and its input routing (`handle_input`
//! reports what the hatchery scene should do: stay, close, or submit the
//! completed sentence). The hatchery scene opens a `DefineModal` for an
//! `Undefined` egg and drives it via that report.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};

use ratatui::buffer::Buffer;
// Key/mouse event vocabulary `handle_input` routes on.
use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::Frame;

use engine_core::color::Rgba;
use engine_core::scene::InputEvent;
use engine_render::dots::{Dot, DotBuffer};
use engine_render::{
    draw_dots, label, ui_primitives, Button, ButtonCore, DotRect, Sizing, TextAlign, TextEditor,
    TextEditorConfig,
};

use super::mad_lib::{completed_sentence, MadLibTemplate, Segment};

/// What the modal asks its caller to do after one input event. `None` means
/// the event was fully consumed and the modal stays open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModalAction {
    None,
    Close,
    Submit(String),
}

/// Minimum width, in cells, of a blank's inline slot — keeps a short label
/// (e.g. "size") legible.
const MIN_BLANK_W: i32 = 8;
/// Maximum width, in cells, of a blank's inline slot — long labels are
/// truncated rather than dominating a wrapped line.
const MAX_BLANK_W: i32 = 20;

/// Fraction of the screen width/height the modal frame occupies.
const FRAME_W_FRAC: f32 = 0.8;
const FRAME_H_FRAC: f32 = 0.7;
/// Minimum frame height, in cells, so a short template still gets a
/// comfortable box.
const FRAME_MIN_H_CELLS: i32 = 10;

/// Modal frame border color (amber), matching the shared modal convention
/// (`battle_menu.rs`, `prompt_editor.rs`).
const BORDER_COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00);
const BORDER_THICKNESS: usize = 1;
const CORNER_RADIUS: usize = 2;

/// Literal word text color (light gray).
const TEXT_COLOR: Rgba = Rgba::rgb(0xe0, 0xe0, 0xe0);
/// Blank slot underline color (gray) — the rule drawn beneath each blank's
/// editor row so an empty slot still reads as fillable.
const SLOT_UNDERLINE_COLOR: Rgba = Rgba::rgb(0x88, 0x88, 0x88);

/// Close (X) hit-area, in cells (matches `battle_menu.rs`/`prompt_editor.rs`).
const CLOSE_W_CELLS: i32 = 3;
const CLOSE_H_CELLS: i32 = 3;
const CLOSE_TOP_INSET_CELLS: i32 = 0;
const CLOSE_RIGHT_INSET_CELLS: i32 = 2;
/// Rows reserved at the frame top for the close button, so the flowed body
/// never collides with the corner X.
const TOP_BAND_CELLS: i32 = CLOSE_TOP_INSET_CELLS + CLOSE_H_CELLS;

/// "Done" button, bottom-centered in the frame.
const DONE_W_CELLS: i32 = 12;
const DONE_H_CELLS: i32 = 3;
const DONE_BOTTOM_INSET_CELLS: i32 = 1;
/// Rows reserved at the frame bottom for the Done button.
const BOTTOM_BAND_CELLS: i32 = DONE_BOTTOM_INSET_CELLS + DONE_H_CELLS;

/// Interior padding, each side, between the frame border and the flowed
/// sentence body.
const PAD_X_CELLS: i32 = 1;

/// Single space between adjacent flowed tokens on the same row, in cells.
const SPACE_W_CELLS: i32 = 1;
/// Row height per flowed line: the text row itself plus one row for the
/// blank underline / line gap.
const LINE_H_CELLS: i32 = 2;

/// One blank-filling `TextEditor` per template blank, a shared close (X), a
/// Done button, and the cached last-rendered area used for hit-testing.
pub(crate) struct DefineModal {
    template: &'static MadLibTemplate,
    blanks: Vec<RefCell<TextEditor>>,
    /// Index into `blanks` currently receiving keyboard input; defaults to
    /// the first blank.
    focus: usize,
    close_button: RefCell<ButtonCore>,
    done_button: RefCell<Button>,
    /// The screen `area` last passed to `render`, cached so click-to-focus
    /// hit-testing can recompute this modal's layout without threading
    /// `area` through separately.
    last_area: Cell<Rect>,
}

/// One literal word placed at a specific on-screen row during flow-wrap.
pub(crate) struct LiteralPlacement {
    pub rect: Rect,
    pub text: String,
}

/// Pure, total layout output of [`DefineModal::compute_layout`]: the modal
/// frame, its X/Done controls, and the flowed placement of every template
/// token (literal words + blank slots), index-aligned with the template's
/// blanks.
pub(crate) struct ModalLayout {
    pub frame: DotRect,
    pub close: DotRect,
    pub done: DotRect,
    pub literals: Vec<LiteralPlacement>,
    pub slots: Vec<Rect>,
}

/// One item in the flowed token stream: a literal word, or a blank
/// (carrying its index into the template's blanks).
enum FlowItem {
    Word(String),
    Blank(usize),
}

impl DefineModal {
    /// One single-line `TextEditor` per blank, all initially empty with no
    /// placeholder (an empty blank shows only its underline; the surrounding
    /// sentence is the hint); focus defaults to the first blank.
    pub(crate) fn new(template: &'static MadLibTemplate) -> Self {
        let blanks = template
            .blank_labels()
            .map(|_label| {
                RefCell::new(TextEditor::new(TextEditorConfig {
                    sizing: Sizing::Fixed,
                    submit_on_enter: false,
                    placeholder: String::new(),
                }))
            })
            .collect();

        Self {
            template,
            blanks,
            focus: 0,
            close_button: RefCell::new(ButtonCore::new(Rect::default())),
            done_button: RefCell::new(
                Button::new(Rect::default(), crate::assets::FRAME_PANEL).label("Done"),
            ),
            last_area: Cell::new(Rect::default()),
        }
    }

    /// Pure, total layout: screen `area` + `template` -> the modal frame,
    /// X/Done control rects, and the flowed token placements. No `self`
    /// access, no mutation. Never panics on a zero-area `area`.
    pub(crate) fn compute_layout(area: Rect, template: &MadLibTemplate) -> ModalLayout {
        let frame = Self::frame_rect(area);
        let close = Self::close_rect(frame);
        let done = Self::done_rect(frame);
        let body = Self::body_rect(frame);
        let (literals, slots) = Self::flow_wrap(body, template);
        ModalLayout { frame, close, done, literals, slots }
    }

    /// Method wrapper around [`Self::compute_layout`] using this modal's own
    /// template — the single source of truth `render` and hit-testing share.
    pub(crate) fn layout(&self, area: Rect) -> ModalLayout {
        Self::compute_layout(area, self.template)
    }

    /// Draws the modal: an occluding braille frame, every literal word, each
    /// blank's slot (underline rule + its editor), the X, and the Done
    /// button.
    pub(crate) fn render(&self, frame: &mut Frame, area: Rect) {
        self.last_area.set(area);
        let layout = self.layout(area);
        if layout.frame.w <= 0 || layout.frame.h <= 0 {
            return;
        }
        let buf = frame.buffer_mut();

        let ring = ui_primitives::rounded_rect(
            layout.frame.w as usize,
            layout.frame.h as usize,
            BORDER_THICKNESS,
            CORNER_RADIUS,
            BORDER_COLOR,
            Dot::Occlude,
        );
        draw_dots(buf, layout.frame.to_cell_rect(), &ring);

        for placement in &layout.literals {
            label(
                buf,
                placement.rect,
                &placement.text,
                TextAlign::Left,
                Style::default().fg(ratatui::style::Color::Rgb(
                    TEXT_COLOR.r,
                    TEXT_COLOR.g,
                    TEXT_COLOR.b,
                )),
            );
        }

        for (i, slot) in layout.slots.iter().enumerate() {
            if slot.width == 0 || slot.height == 0 {
                continue;
            }
            Self::draw_slot_underline(buf, *slot);
            let mut editor = self.blanks[i].borrow_mut();
            editor.set_focused(i == self.focus);
            editor.render(buf, *slot);
        }

        self.close_button.borrow_mut().set_rect(layout.close.to_cell_rect());
        let close_state = self.close_button.borrow().state();
        crate::scenes::close_button::draw_close_button(buf, layout.close, close_state);

        self.done_button.borrow_mut().set_rect(layout.done.to_cell_rect());
        self.done_button.borrow_mut().render(buf);
    }

    /// Advances every blank editor's cursor-blink accumulator, so the focused
    /// blank blinks like the roster's text field. Called from the scene's
    /// `update` each frame.
    pub(crate) fn tick(&self, dt: std::time::Duration) {
        for blank in &self.blanks {
            blank.borrow_mut().tick(dt);
        }
    }

    /// Draws a thin fill-in underline rule along `slot`'s bottom dot-row, via
    /// the dot
    /// pipeline (CLAUDE.md rule 4), so an empty blank still reads as a
    /// fillable slot before its editor draws over part of it.
    fn draw_slot_underline(buf: &mut Buffer, slot: Rect) {
        let w_dots = slot.width as usize * 2;
        // A thin rule along the cell's bottom dot-row — a fill-in underline,
        // not a solid bar filling the whole slot.
        let mut dots = DotBuffer::new(w_dots, 4);
        for x in 0..w_dots {
            dots.set(x, 3, Dot::Lit(SLOT_UNDERLINE_COLOR));
        }
        draw_dots(buf, slot, &dots);
    }

    /// The whole-cell-rooted modal frame: `FRAME_W_FRAC × FRAME_H_FRAC` of
    /// `area` (height floored no lower than `FRAME_MIN_H_CELLS`), centered,
    /// clamped to the screen (mirrors `battle_menu::get_dot_rect`).
    fn frame_rect(area: Rect) -> DotRect {
        let screen = DotRect {
            x: area.x as i32 * 2,
            y: area.y as i32 * 4,
            w: area.width as i32 * 2,
            h: area.height as i32 * 4,
        };

        let w = ((screen.w as f32 * FRAME_W_FRAC) as i32).min(screen.w);
        let h = ((screen.h as f32 * FRAME_H_FRAC) as i32)
            .max(FRAME_MIN_H_CELLS * 4)
            .min(screen.h);

        // Round down to whole cells (2 dots wide, 4 tall) so the frame maps
        // 1:1 onto braille cells.
        let w = w - w.rem_euclid(2);
        let h = h - h.rem_euclid(4);

        let x = screen.x + (screen.w - w) / 2;
        let y = screen.y + (screen.h - h) / 2;

        DotRect { x, y, w, h }
    }

    /// Close (X) hit-rect: a fixed cell box in the frame's top-right corner,
    /// inset from the top/right borders (mirrors `battle_menu`/`prompt_editor`).
    fn close_rect(frame: DotRect) -> DotRect {
        let w = CLOSE_W_CELLS * 2;
        let h = CLOSE_H_CELLS * 4;
        DotRect {
            x: frame.x + frame.w - CLOSE_RIGHT_INSET_CELLS * 2 - w,
            y: frame.y + CLOSE_TOP_INSET_CELLS * 4,
            w,
            h,
        }
    }

    /// "Done" button rect: a fixed cell box centered horizontally in the
    /// frame's bottom band.
    fn done_rect(frame: DotRect) -> DotRect {
        let w = DONE_W_CELLS * 2;
        let h = DONE_H_CELLS * 4;
        DotRect {
            x: frame.x + (frame.w - w) / 2,
            y: frame.y + frame.h - DONE_BOTTOM_INSET_CELLS * 4 - h,
            w,
            h,
        }
    }

    /// The flowable interior: the frame's padding on each side, minus the
    /// top band (clears the close button) and the bottom band (clears the
    /// Done button).
    fn body_rect(frame: DotRect) -> Rect {
        frame
            .inset(
                PAD_X_CELLS * 2,
                PAD_X_CELLS * 2,
                TOP_BAND_CELLS * 4,
                BOTTOM_BAND_CELLS * 4,
            )
            .to_cell_rect()
    }

    /// Builds the flowed item stream from `template`'s segments: each
    /// literal segment splits on ASCII whitespace into word items; each
    /// blank segment becomes one blank item carrying its blank index (in
    /// segment order, matching `template.blank_labels()`).
    fn flow_items(template: &MadLibTemplate) -> Vec<FlowItem> {
        let mut items = Vec::new();
        let mut blank_idx = 0;
        for segment in template.segments() {
            match segment {
                Segment::Literal(text) => {
                    items.extend(text.split_whitespace().map(|w| FlowItem::Word(w.to_string())));
                }
                Segment::Blank { .. } => {
                    items.push(FlowItem::Blank(blank_idx));
                    blank_idx += 1;
                }
            }
        }
        items
    }

    /// Greedy word-wrap over `template`'s flowed items within `body`: places
    /// literal words and fixed-width blank slots left-to-right, wrapping to
    /// the next line when the next item would overflow `body`'s width.
    /// Returns one `LiteralPlacement` per word and a `slots` vector sized to
    /// `template.blank_count()`, index-aligned with the template's blanks.
    /// A zero-area `body` yields empty/unfilled output rather than panicking.
    fn flow_wrap(body: Rect, template: &MadLibTemplate) -> (Vec<LiteralPlacement>, Vec<Rect>) {
        let mut slots = vec![Rect::default(); template.blank_count()];
        let mut literals = Vec::new();
        if body.width == 0 || body.height == 0 {
            return (literals, slots);
        }

        let labels: Vec<&str> = template.blank_labels().collect();
        let body_w = body.width as i32;
        let body_h = body.height as i32;

        let mut cursor_x = 0i32;
        let mut row = 0i32;

        for item in Self::flow_items(template) {
            let width = match &item {
                FlowItem::Word(w) => w.chars().count() as i32,
                FlowItem::Blank(i) => {
                    (labels[*i].chars().count() as i32).clamp(MIN_BLANK_W, MAX_BLANK_W)
                }
            }
            .min(body_w);

            if cursor_x > 0 {
                if cursor_x + SPACE_W_CELLS + width > body_w {
                    cursor_x = 0;
                    row += 1;
                } else {
                    cursor_x += SPACE_W_CELLS;
                }
            }

            let y = row * LINE_H_CELLS;
            if y >= body_h {
                break;
            }

            let rect = Rect::new(body.x + cursor_x as u16, body.y + y as u16, width as u16, 1);
            match item {
                FlowItem::Word(text) => literals.push(LiteralPlacement { rect, text }),
                FlowItem::Blank(i) => slots[i] = rect,
            }
            cursor_x += width;
        }

        (literals, slots)
    }

    #[cfg(test)]
    pub(crate) fn blank_count(&self) -> usize {
        self.blanks.len()
    }

    /// Sole input entry point while the modal is open: fully consumes every
    /// event and reports what the caller should do (stay, close, or submit
    /// the completed sentence).
    pub(crate) fn handle_input(&mut self, ev: &InputEvent) -> ModalAction {
        match ev {
            InputEvent::Key(key) => match key.code {
                KeyCode::Esc => ModalAction::Close,
                KeyCode::Tab => {
                    if !self.blanks.is_empty() {
                        self.focus = (self.focus + 1) % self.blanks.len();
                    }
                    ModalAction::None
                }
                KeyCode::BackTab => {
                    if !self.blanks.is_empty() {
                        self.focus = (self.focus + self.blanks.len() - 1) % self.blanks.len();
                    }
                    ModalAction::None
                }
                // Swallowed: a blank is single-line and typing must never
                // trigger anything (spec 67 §"Mad-Lib Definition Flow" item 5).
                KeyCode::Enter => ModalAction::None,
                _ => {
                    self.blanks[self.focus].get_mut().handle_key(*key);
                    ModalAction::None
                }
            },
            InputEvent::Mouse(me) => {
                let layout = self.layout(self.last_area.get());
                self.close_button.get_mut().set_rect(layout.close.to_cell_rect());
                self.done_button.get_mut().set_rect(layout.done.to_cell_rect());

                if self.close_button.get_mut().handle_mouse(me) {
                    return ModalAction::Close;
                }
                if self.done_button.get_mut().handle_mouse(me) {
                    return self.try_submit().map_or(ModalAction::None, ModalAction::Submit);
                }

                if me.kind == MouseEventKind::Down(MouseButton::Left) {
                    let pos = Position { x: me.column, y: me.row };
                    for (i, slot) in layout.slots.iter().enumerate() {
                        if slot.contains(pos) {
                            self.focus = i;
                            self.blanks[i].get_mut().handle_mouse(me);
                            break;
                        }
                    }
                }
                ModalAction::None
            }
        }
    }

    /// `Some(completed sentence)` iff every blank holds non-empty (post-trim)
    /// text, composed through `mad_lib::completed_sentence` so every caller
    /// agrees byte-for-byte; else `None` (Done is inert).
    fn try_submit(&self) -> Option<String> {
        let texts: Vec<String> = self.blanks.iter().map(|b| b.borrow().text()).collect();
        if texts.iter().any(|t| t.trim().is_empty()) {
            return None;
        }
        Some(completed_sentence(self.template, &texts))
    }

    #[cfg(test)]
    pub(crate) fn blank_text(&self, i: usize) -> String {
        self.blanks[i].borrow().text()
    }

    #[cfg(test)]
    pub(crate) fn focus(&self) -> usize {
        self.focus
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;

    use crate::scenes::hatchery::mad_lib;
    use crate::scenes::test_util::rect_text;

    /// A dot-space rect representing the full screen, for margin/bounds
    /// checks against `compute_layout`'s output (mirrors
    /// `prompt_editor.rs::screen_dots`).
    fn screen_dots(area: Rect) -> DotRect {
        DotRect {
            x: area.x as i32 * 2,
            y: area.y as i32 * 4,
            w: area.width as i32 * 2,
            h: area.height as i32 * 4,
        }
    }

    /// Render `modal` into a fresh `w`x`h` `TestBackend` and return the
    /// resulting buffer (mirrors `prompt_editor.rs::render_popup`).
    fn render_modal(modal: &DefineModal, w: u16, h: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                modal.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// Count blank-occluder cells (braille U+2800) and lit-braille cells
    /// (U+2801..=U+28FF) across the whole buffer (mirrors
    /// `prompt_editor.rs::tally`).
    fn tally(buf: &Buffer, w: u16, h: u16) -> (usize, usize) {
        let (mut blanks, mut lit) = (0, 0);
        for y in 0..h {
            for x in 0..w {
                let s = buf.cell((x, y)).unwrap().symbol().to_string();
                if s == "\u{2800}" {
                    blanks += 1;
                } else if s.chars().next().is_some_and(|c| ('\u{2801}'..='\u{28FF}').contains(&c)) {
                    lit += 1;
                }
            }
        }
        (blanks, lit)
    }

    /// `DefineModal::new` must build exactly one blank editor per template
    /// blank.
    #[test]
    fn new_builds_one_editor_per_blank() {
        let template = &mad_lib::pool()[0];
        let modal = DefineModal::new(template);
        assert_eq!(modal.blank_count(), template.blank_count());
    }

    /// The layout frame must be whole-cell rooted (CLAUDE.md rule 5) and
    /// fully contained within the screen it was laid out against.
    #[test]
    fn layout_frame_is_cell_rooted_and_within_area() {
        let area = Rect::new(0, 0, 80, 24);
        let template = &mad_lib::pool()[0];
        let layout = DefineModal::compute_layout(area, template);
        let frame = layout.frame;
        let screen = screen_dots(area);

        assert_eq!(frame.x % 2, 0, "frame.x not cell-aligned: {frame:?}");
        assert_eq!(frame.y % 4, 0, "frame.y not cell-aligned: {frame:?}");
        assert_eq!(frame.w % 2, 0, "frame.w not cell-aligned: {frame:?}");
        assert_eq!(frame.h % 4, 0, "frame.h not cell-aligned: {frame:?}");

        assert!(frame.x >= screen.x && frame.y >= screen.y);
        assert!(frame.x + frame.w <= screen.x + screen.w);
        assert!(frame.y + frame.h <= screen.y + screen.h);
    }

    /// `compute_layout` must emit exactly one slot rect per template blank.
    #[test]
    fn layout_has_one_slot_per_blank() {
        let area = Rect::new(0, 0, 80, 24);
        let template = &mad_lib::pool()[0];
        let layout = DefineModal::compute_layout(area, template);
        assert_eq!(layout.slots.len(), template.blank_count());
    }

    /// The close (X) control sits inside the frame, biased to its top-right
    /// corner (mirrors `prompt_editor.rs::close_button_sits_top_right_of_popup`).
    #[test]
    fn close_sits_top_right() {
        let area = Rect::new(0, 0, 80, 24);
        let template = &mad_lib::pool()[0];
        let layout = DefineModal::compute_layout(area, template);
        let frame = layout.frame;
        let close = layout.close;

        assert!(close.x >= frame.x && close.x + close.w <= frame.x + frame.w);
        assert!(close.y >= frame.y);

        let left_gap = close.x - frame.x;
        let right_gap = (frame.x + frame.w) - (close.x + close.w);
        assert!(
            right_gap <= left_gap,
            "close not right-aligned: left_gap={left_gap} right_gap={right_gap}"
        );
        assert!(
            close.y - frame.y <= frame.h / 4,
            "close not in the top band: close.y={} frame.y={} frame.h={}",
            close.y,
            frame.y,
            frame.h
        );
    }

    /// The Done control sits inside the frame's bottom band.
    #[test]
    fn done_sits_bottom() {
        let area = Rect::new(0, 0, 80, 24);
        let template = &mad_lib::pool()[0];
        let layout = DefineModal::compute_layout(area, template);
        let frame = layout.frame;
        let done = layout.done;

        assert!(done.x >= frame.x && done.x + done.w <= frame.x + frame.w);
        assert!(done.y + done.h <= frame.y + frame.h);
        assert!(
            done.y >= frame.y + frame.h / 2,
            "done not in the bottom band: done.y={} frame.y={} frame.h={}",
            done.y,
            frame.y,
            frame.h
        );
    }

    /// An open modal must occlude its interior (braille-blank cells appear)
    /// AND draw a lit border (lit-braille cells appear) — same contract as
    /// `prompt_editor.rs::popup_occludes_and_draws_border`.
    #[test]
    fn render_occludes_and_draws_border() {
        let template = &mad_lib::pool()[0];
        let modal = DefineModal::new(template);
        let (w, h) = (80u16, 30u16);
        let buf = render_modal(&modal, w, h);
        let (blanks, lit) = tally(&buf, w, h);
        assert!(blanks > 0, "open modal must cover its interior with braille-blank occluders");
        assert!(lit > 0, "open modal must draw a lit border");
    }

    /// Every literal word of the template must appear as plain text
    /// somewhere in the rendered frame.
    #[test]
    fn render_shows_template_literal_words() {
        let template = &mad_lib::pool()[0];
        let modal = DefineModal::new(template);
        let (w, h) = (80u16, 30u16);
        let buf = render_modal(&modal, w, h);

        let layout = DefineModal::compute_layout(Rect::new(0, 0, w, h), template);
        let frame_text = rect_text(&buf, layout.frame.to_cell_rect());

        for segment in template.segments() {
            if let mad_lib::Segment::Literal(text) = segment {
                for word in text.split_whitespace() {
                    assert!(
                        frame_text.contains(word),
                        "literal word {word:?} missing from rendered frame: {frame_text:?}"
                    );
                }
            }
        }
    }

    /// An empty blank shows only its underline rule — no placeholder label
    /// text (clean underlines, not `size`/`temperament`).
    #[test]
    fn render_empty_blank_shows_underline_not_label() {
        let template = &mad_lib::pool()[0];
        let modal = DefineModal::new(template);
        let (w, h) = (80u16, 30u16);
        let buf = render_modal(&modal, w, h);

        let layout = DefineModal::compute_layout(Rect::new(0, 0, w, h), template);
        for (label, slot) in template.blank_labels().zip(layout.slots.iter()) {
            let text = rect_text(&buf, *slot);
            assert!(
                !text.contains(label),
                "empty blank must not show its label {label:?}, got {text:?}"
            );
            assert!(
                crate::scenes::test_util::has_non_space(&buf, *slot),
                "empty blank must still show its underline rule"
            );
        }
    }

    /// The X and Done controls must draw their glyph/label at their laid-out
    /// rects.
    #[test]
    fn render_draws_x_and_done() {
        let template = &mad_lib::pool()[0];
        let modal = DefineModal::new(template);
        let (w, h) = (80u16, 30u16);
        let buf = render_modal(&modal, w, h);

        let layout = DefineModal::compute_layout(Rect::new(0, 0, w, h), template);
        let close_text = rect_text(&buf, layout.close.to_cell_rect());
        assert!(close_text.contains('X'), "close control must draw an X, got {close_text:?}");

        let done_text = rect_text(&buf, layout.done.to_cell_rect());
        assert!(done_text.contains("Done"), "done control must draw its label, got {done_text:?}");
    }

    /// A zero-area screen must not panic, in either layout or render.
    #[test]
    fn zero_area_does_not_panic() {
        let template = &mad_lib::pool()[0];
        let layout = DefineModal::compute_layout(Rect::new(0, 0, 0, 0), template);
        assert!(layout.slots.is_empty() || !layout.slots.is_empty()); // no panic is the assertion

        let modal = DefineModal::new(template);
        let mut terminal = Terminal::new(TestBackend::new(0, 0)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                modal.render(f, area);
            })
            .unwrap();
    }

    // ── input routing, Done-gating, completed-sentence signal ─────────────

    use crate::scenes::test_util::{key_event, mouse_event};

    /// The center cell of `rect`, for aiming a mouse event at a hit-rect.
    fn rect_center(rect: Rect) -> (u16, u16) {
        (rect.x + rect.width / 2, rect.y + rect.height / 2)
    }

    /// A completed Left click (Down then Up) at the center of `rect`. Returns
    /// the action reported by the Up event — the one a button completes on.
    fn click(modal: &mut DefineModal, rect: Rect) -> ModalAction {
        let (x, y) = rect_center(rect);
        let _ = modal.handle_input(&mouse_event(MouseEventKind::Down(MouseButton::Left), x, y));
        modal.handle_input(&mouse_event(MouseEventKind::Up(MouseButton::Left), x, y))
    }

    /// Types every char of `s` into the currently focused blank, asserting
    /// each keystroke stays fully inert (never submits/closes).
    fn type_into_focused(modal: &mut DefineModal, s: &str) {
        for c in s.chars() {
            let action = modal.handle_input(&key_event(KeyCode::Char(c)));
            assert_eq!(action, ModalAction::None, "typing must never submit or close");
        }
    }

    /// A char key routed to the focused blank must land in that blank's text
    /// and never submit or close the modal.
    #[test]
    fn typing_fills_focused_blank_and_does_not_submit_or_close() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        type_into_focused(&mut modal, "big");
        assert_eq!(modal.blank_text(0), "big");
    }

    /// Tab advances focus to the next blank, wrapping back to the first
    /// after the last.
    #[test]
    fn tab_advances_focus_wrapping() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        let n = modal.blank_count();
        assert_eq!(modal.focus(), 0);
        for i in 1..=n {
            let action = modal.handle_input(&key_event(KeyCode::Tab));
            assert_eq!(action, ModalAction::None);
            assert_eq!(modal.focus(), i % n, "focus after {i} Tab(s)");
        }
    }

    /// BackTab (Shift+Tab) retreats focus, wrapping from the first blank to
    /// the last.
    #[test]
    fn backtab_retreats_focus_wrapping() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        let n = modal.blank_count();
        let action = modal.handle_input(&key_event(KeyCode::BackTab));
        assert_eq!(action, ModalAction::None);
        assert_eq!(modal.focus(), n - 1, "BackTab from focus 0 must wrap to the last blank");
    }

    /// Enter is swallowed: it does not insert a newline into the focused
    /// blank and does not submit or close the modal.
    #[test]
    fn enter_is_swallowed_no_newline_no_submit() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        type_into_focused(&mut modal, "big");
        let action = modal.handle_input(&key_event(KeyCode::Enter));
        assert_eq!(action, ModalAction::None, "Enter must not submit or close");
        assert_eq!(modal.blank_text(0), "big", "Enter must not insert a newline into the blank");
    }

    /// Esc always requests close.
    #[test]
    fn esc_returns_close() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        assert_eq!(modal.handle_input(&key_event(KeyCode::Esc)), ModalAction::Close);
    }

    /// A completed click on Done with one blank left empty is inert — no
    /// `Submit`.
    #[test]
    fn done_with_empty_blank_is_inert() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        let _ = render_modal(&modal, 80, 30); // seeds last_area for hit-testing

        // Fill every blank except the first, which stays empty.
        for _ in 1..modal.blank_count() {
            modal.handle_input(&key_event(KeyCode::Tab));
            type_into_focused(&mut modal, "x");
        }

        let layout = DefineModal::compute_layout(Rect::new(0, 0, 80, 30), template);
        let action = click(&mut modal, layout.done.to_cell_rect());
        assert_eq!(action, ModalAction::None, "Done with an empty blank must be inert");
    }

    /// A blank holding only whitespace does not arm Done — a lone space is
    /// not a fill.
    #[test]
    fn done_with_whitespace_only_blank_is_inert() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        let _ = render_modal(&modal, 80, 30);

        for i in 0..modal.blank_count() {
            type_into_focused(&mut modal, " ");
            if i + 1 < modal.blank_count() {
                modal.handle_input(&key_event(KeyCode::Tab));
            }
        }

        let layout = DefineModal::compute_layout(Rect::new(0, 0, 80, 30), template);
        let action = click(&mut modal, layout.done.to_cell_rect());
        assert_eq!(action, ModalAction::None, "a whitespace-only blank must not arm Done");
    }

    /// With every blank filled, a completed click on Done yields the exact
    /// sentence `mad_lib::completed_sentence` composes from the same raw
    /// (untrimmed) blank texts.
    #[test]
    fn done_all_filled_yields_exact_completed_sentence() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        let _ = render_modal(&modal, 80, 30);

        let texts = ["gigantic", "fiery", "a flurry of claws"];
        assert_eq!(texts.len(), modal.blank_count(), "fixture must supply one text per blank");
        for (i, text) in texts.iter().enumerate() {
            type_into_focused(&mut modal, text);
            if i + 1 < texts.len() {
                modal.handle_input(&key_event(KeyCode::Tab));
            }
        }

        let layout = DefineModal::compute_layout(Rect::new(0, 0, 80, 30), template);
        let action = click(&mut modal, layout.done.to_cell_rect());
        let expected = mad_lib::completed_sentence(template, &texts);
        assert_eq!(action, ModalAction::Submit(expected));
    }

    /// A completed click on the X returns `Close`.
    #[test]
    fn x_click_returns_close() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        let _ = render_modal(&modal, 80, 30);
        let layout = DefineModal::compute_layout(Rect::new(0, 0, 80, 30), template);
        let action = click(&mut modal, layout.close.to_cell_rect());
        assert_eq!(action, ModalAction::Close);
    }

    /// Clicking inside a blank's slot moves keyboard focus to that blank.
    #[test]
    fn click_in_slot_sets_focus() {
        let template = &mad_lib::pool()[0];
        let mut modal = DefineModal::new(template);
        let _ = render_modal(&modal, 80, 30);
        let layout = DefineModal::compute_layout(Rect::new(0, 0, 80, 30), template);
        let (x, y) = rect_center(layout.slots[1]);
        let action = modal.handle_input(&mouse_event(MouseEventKind::Down(MouseButton::Left), x, y));
        assert_eq!(action, ModalAction::None);
        assert_eq!(modal.focus(), 1);
    }

}
