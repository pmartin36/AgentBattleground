//! Spec 51's `PromptEditor` popup state container (b1-t1): two `TextEditor`s
//! (agent input + instructions), the close (X) `ButtonCore`, focus tracking,
//! and the write-debounce/dirty flag. Layout/render land in b2, input
//! routing + write-through land in b3.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::Frame;

use engine_core::color::Rgba;
use engine_core::scene::InputEvent;
use engine_render::dots::{Dot, DotBuffer};
use engine_render::{
    draw_dots, flex, label, ui_primitives, Align, Basis, ButtonCore, ButtonState, Direction,
    DotRect, EditorEvent, FlexChild, FlexStyle, Justify, Sizing, TextAlign, TextEditor,
    TextEditorConfig,
};

use super::RosterManager;

/// Fraction of the screen width/height the popup occupies (b2-t1).
const POPUP_W_FRAC: f32 = 0.8;
const POPUP_H_FRAC: f32 = 0.8;
/// Minimum popup height, in cells (b2-t1).
const POPUP_MIN_H: u16 = 12;
/// Idle time after the last instructions-editor edit before the pending
/// write is flushed to disk (b3-t2).
const WRITE_DEBOUNCE: Duration = Duration::from_millis(300);
/// Cap on the agent input's grow-with-content row count (b3-t3).
const AGENT_INPUT_MAX_ROWS: u16 = 6;

/// Interior horizontal padding, each side (b2-t1 layout).
const POPUP_PAD_X_CELLS: u16 = 1;
/// Interior bottom padding, below the file-path row (b2-t1 layout).
const POPUP_PAD_BOTTOM_CELLS: u16 = 1;
/// Close (X) hit-target size, in cells (b2-t1 layout). 2 cells tall so the
/// braille circle around the "X" is round, not squashed.
const CLOSE_W_CELLS: u16 = 3;
const CLOSE_H_CELLS: u16 = 2;
/// Reserves the X row (+ a 1-cell clearance) so the body clears it (b2-t1
/// layout).
const POPUP_TOP_BAND_CELLS: u16 = CLOSE_H_CELLS + 1;
/// Spec's explicit 1-cell margin between the agent input and the
/// instructions editor (b2-t1 layout).
const AGENT_MARGIN_CELLS: u16 = 1;
/// File-path row height, in cells (b2-t1 layout).
const FILE_PATH_H_CELLS: u16 = 1;

/// Popup frame border color (amber), matching the `BattleMenu`/tooltip modal
/// convention (b2-t2 render).
const POPUP_BORDER_COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00);
/// Popup frame border thickness, in dots (b2-t2 render).
const POPUP_BORDER_THICKNESS: usize = 1;
/// Popup frame chamfered-corner radius, in dots (house-default chamfer 1).
const POPUP_CORNER_RADIUS: usize = 1;
/// File-path row text color (dim gray) (b2-t2 render).
const POPUP_PATH_COLOR: Rgba = Rgba::rgb(0x88, 0x88, 0x88);
/// Per-field braille box border color (gray).
const FIELD_BORDER_COLOR: Rgba = Rgba::rgb(0x88, 0x88, 0x88);
/// Per-field braille box border thickness, in cells (1 cell each side is
/// reserved so the border never overlaps the editor's first/last text row/col).
const FIELD_BORDER_CELLS: i32 = 1;
/// Close (X) glyph colors by `ButtonCore` state — red, brightening on hover.
const CLOSE_IDLE_COLOR: Rgba = Rgba::rgb(0xc0, 0x30, 0x30);
const CLOSE_HOVER_COLOR: Rgba = Rgba::rgb(0xff, 0x55, 0x55);
const CLOSE_PRESSED_COLOR: Rgba = Rgba::rgb(0x80, 0x20, 0x20);

/// Which of the two editors currently receives keyboard input (b3-t1's
/// Tab-cycle). Defaults to `Instructions` on open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopupFocus {
    AgentInput,
    Instructions,
}

pub(super) struct PromptEditor {
    #[allow(dead_code)] // read by b3-t1's write-through / mod.rs call-site plumbing
    creature_index: usize,
    creature_name: String,
    instructions_base: Option<PathBuf>,
    agent_input: RefCell<TextEditor>,
    instructions: RefCell<TextEditor>,
    close_button: RefCell<ButtonCore>,
    focus: PopupFocus,
    dirty: bool,
    /// Time remaining until the pending edit flushes to disk — meaningful
    /// only while `dirty` (b3-t2).
    debounce: Duration,
    /// The screen `area` last passed to `render` (b2-t1), cached so
    /// `handle_input`'s mouse branch can recompute `layout` for click-to-
    /// focus hit-testing without `Scene::handle_input`'s fixed signature
    /// threading `area` through. Defaults to `Rect::default()` before the
    /// first render, which is always ahead of the first possible click in
    /// practice (input is only routed here while the popup, itself just
    /// rendered, is open).
    last_area: Cell<Rect>,
}

impl PromptEditor {
    pub(super) fn new(creature_index: usize, name: &str, base: Option<&Path>) -> Self {
        let seed = crate::instructions::read_instructions_maybe(base, name).unwrap_or_default();

        let mut instructions = TextEditor::new(TextEditorConfig {
            sizing: Sizing::Fixed,
            submit_on_enter: false,
            placeholder: String::new(),
        });
        instructions.set_text(&seed);

        let agent_input = TextEditor::new(TextEditorConfig {
            sizing: Sizing::Grow {
                max_rows: AGENT_INPUT_MAX_ROWS,
            },
            submit_on_enter: true,
            placeholder: "Prompt agent to update".to_string(),
        });

        Self {
            creature_index,
            creature_name: name.to_string(),
            instructions_base: base.map(Path::to_path_buf),
            agent_input: RefCell::new(agent_input),
            instructions: RefCell::new(instructions),
            close_button: RefCell::new(ButtonCore::new(Rect::default())),
            focus: PopupFocus::Instructions,
            dirty: false,
            debounce: Duration::ZERO,
            last_area: Cell::new(Rect::default()),
        }
    }

    /// Modal input router (b3-t1): the popup's sole entry point for both key
    /// and mouse events while it is open. Returns `true` iff the popup
    /// requests close (`Esc`, or a completed click on the close (X)) — the
    /// scene's `handle_input` closes on `true` and refreshes its cached
    /// instructions from disk. All other input is fully consumed here and
    /// never falls through to the scene's normal roster bindings.
    pub(super) fn handle_input(&mut self, ev: &InputEvent) -> bool {
        match ev {
            InputEvent::Key(key) => match key.code {
                KeyCode::Esc => return true,
                KeyCode::Tab => self.toggle_focus(),
                _ => match self.focus {
                    PopupFocus::AgentInput => {
                        if self.agent_input.get_mut().handle_key(*key) == EditorEvent::Submit {
                            self.submit_agent_input();
                        }
                    }
                    PopupFocus::Instructions => {
                        if self.instructions.get_mut().handle_key(*key) == EditorEvent::Changed {
                            self.mark_dirty();
                        }
                    }
                },
            },
            InputEvent::Mouse(me) => {
                if self.close_button.get_mut().handle_mouse(me) {
                    return true;
                }
                if me.kind == MouseEventKind::Down(MouseButton::Left) {
                    // Click-to-focus (b2-t1): route the click to whichever
                    // field's box it lands in and make that field the
                    // keyboard focus, forwarding the click ONLY to that
                    // editor so the other editor's caret does not move. A
                    // click inside the popup but outside both field boxes
                    // leaves focus unchanged and forwards nothing.
                    let layout = self.layout(self.last_area.get());
                    let pos = Position { x: me.column, y: me.row };
                    if layout.agent_input.to_cell_rect().contains(pos) {
                        self.focus = PopupFocus::AgentInput;
                        self.agent_input.get_mut().handle_mouse(me);
                    } else if layout.instructions.to_cell_rect().contains(pos) {
                        self.focus = PopupFocus::Instructions;
                        self.instructions.get_mut().handle_mouse(me);
                    }
                } else {
                    self.agent_input.get_mut().handle_mouse(me);
                    self.instructions.get_mut().handle_mouse(me);
                }
            }
        }
        false
    }

    /// Cycles keyboard focus between the two editors (b3-t1's `Tab`
    /// handling).
    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            PopupFocus::AgentInput => PopupFocus::Instructions,
            PopupFocus::Instructions => PopupFocus::AgentInput,
        };
    }

    /// Handles `EditorEvent::Submit` from the agent input (Enter, non-Shift).
    /// v1 stub: no model call, no I/O, no close — clears the agent input so
    /// the Submit path has a real, observable effect instead of being
    /// silently swallowed. Future agent flow (`needs-research/ai-prompt-rewrite-agent`)
    /// will capture the prompt text here before clearing.
    fn submit_agent_input(&mut self) {
        self.agent_input.get_mut().set_text("");
    }

    /// Marks the popup dirty and (re)starts the `WRITE_DEBOUNCE` countdown
    /// (b3-t2). Called on the instructions editor's `EditorEvent::Changed`
    /// only — every keystroke restarts the timer so a burst of edits
    /// coalesces into a single trailing-idle write.
    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.debounce = WRITE_DEBOUNCE;
    }

    /// Scene tick (b3-t2, b2-t2): advances both editors' blink accumulators
    /// every frame, then — while a pending edit exists — advances the
    /// debounce, flushing once it elapses. The debounce countdown stays
    /// gated on `dirty`; only the blink tick runs unconditionally.
    pub(super) fn update(&mut self, dt: Duration) {
        self.agent_input.get_mut().tick(dt);
        self.instructions.get_mut().tick(dt);

        if !self.dirty {
            return;
        }
        self.debounce = self.debounce.saturating_sub(dt);
        if self.debounce.is_zero() {
            self.flush_pending();
        }
    }

    /// Writes the instructions editor's current text to disk and clears the
    /// pending-edit state (b3-t2). Called both on debounce elapse and, by
    /// the scene's close path, before `reload_instructions` so the cached
    /// preview never refreshes from stale disk. No-op while `dirty` is
    /// false.
    pub(super) fn flush_pending(&mut self) {
        if !self.dirty {
            return;
        }
        let text = self.instructions.borrow().text();
        let _ = crate::instructions::write_instructions_maybe(
            self.instructions_base.as_deref(),
            &self.creature_name,
            &text,
        );
        self.dirty = false;
        self.debounce = Duration::ZERO;
    }

    /// Test-only accessor for the seeded instructions-editor text
    /// (private-field tests live in this module; kept for symmetry with
    /// `instructions_text` mentioned in research.md, in case a later task's
    /// tests need to live outside this module).
    #[cfg(test)]
    fn instructions_text(&self) -> String {
        self.instructions.borrow().text()
    }

    /// b2-t1: pure, total layout — screen `area` + the agent input's
    /// `desired_rows` in, five whole-cell-rooted `DotRect`s out. No self
    /// access, no mutation; single source of truth for both `render` (b2-t2)
    /// and hit-testing (b3-t1).
    pub(super) fn compute_layout(area: Rect, agent_rows: u16) -> PopupLayout {
        let popup = Self::popup_box(area, agent_rows);

        // Interior body: clears the top band (X row + clearance) and the
        // bottom pad below the file-path row.
        let body = popup.inset(
            POPUP_PAD_X_CELLS as i32 * 2,
            POPUP_PAD_X_CELLS as i32 * 2,
            POPUP_TOP_BAND_CELLS as i32 * 4,
            POPUP_PAD_BOTTOM_CELLS as i32 * 4,
        );

        let body_children = [
            // Agent input BOX: content rows + a 1-cell border top and bottom.
            FlexChild {
                basis: Basis::Fixed((agent_rows as i32 + 2 * FIELD_BORDER_CELLS) * 4),
                grow: 0.0,
                shrink: 0.0,
            },
            FlexChild {
                basis: Basis::Fixed(AGENT_MARGIN_CELLS as i32 * 4),
                grow: 0.0,
                shrink: 0.0,
            },
            FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            FlexChild {
                basis: Basis::Fixed(FILE_PATH_H_CELLS as i32 * 4),
                grow: 0.0,
                shrink: 0.0,
            },
        ];
        let body_style = FlexStyle {
            direction: Direction::Column,
            justify_content: Justify::Start,
            align_items: Align::Center,
            gap: 0,
        };
        let body_out = flex(body, body_style, &body_children);
        let agent_input = body_out[0];
        let instructions = body_out[2];
        let file_path = body_out[3];

        // Top band: the X row, inset from the popup's top-left border by the
        // same 1-cell pad as the body's sides.
        let close_band = DotRect {
            x: popup.x,
            y: popup.y,
            w: popup.w,
            h: POPUP_TOP_BAND_CELLS as i32 * 4,
        }
        .inset(
            POPUP_PAD_X_CELLS as i32 * 2,
            POPUP_PAD_X_CELLS as i32 * 2,
            POPUP_PAD_X_CELLS as i32 * 4,
            0,
        );
        let close_children = [FlexChild {
            basis: Basis::Fixed(CLOSE_W_CELLS as i32 * 2),
            grow: 0.0,
            shrink: 0.0,
        }];
        let close_style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::End,
            align_items: Align::Start,
            gap: 0,
        };
        let close = flex(close_band, close_style, &close_children)[0];

        PopupLayout { popup, close, agent_input, instructions, file_path }
    }

    /// Method wrapper around [`Self::compute_layout`]: resolves the agent
    /// input's current `desired_rows` (width in cells) so `render` (b2-t2)
    /// and input hit-testing (b3-t1) never hand-compute it themselves.
    pub(super) fn layout(&self, area: Rect) -> PopupLayout {
        // Content width is the interior minus the field box's 1-cell border
        // each side, so the agent input wraps to its true drawable width.
        let content_w = Self::interior_w_cells(area).saturating_sub(2 * FIELD_BORDER_CELLS as u16);
        let rows = self.agent_input.borrow().desired_rows(content_w);
        Self::compute_layout(area, rows)
    }

    /// Draws the popup: the chamfered Occlude frame, the close (X), both
    /// editors, and the file-path row, all positioned from `self.layout(area)`
    /// (b2-t2). Order = z-order, topmost last, mirroring `BattleMenu::render`.
    pub(super) fn render(&self, frame: &mut Frame, area: Rect) {
        self.last_area.set(area);
        let layout = self.layout(area);
        let buf = frame.buffer_mut();

        // Chamfered Occlude frame over the whole popup box — covers the
        // roster beneath.
        let ring = ui_primitives::rounded_rect(
            layout.popup.w as usize,
            layout.popup.h as usize,
            POPUP_BORDER_THICKNESS,
            POPUP_CORNER_RADIUS,
            POPUP_BORDER_COLOR,
            Dot::Occlude,
        );
        draw_dots(buf, layout.popup.to_cell_rect(), &ring);

        // Close (X): refresh the hit-rect from layout, then draw the glyph in
        // a red keyed to hover/press state.
        self.close_button.borrow_mut().set_rect(layout.close.to_cell_rect());
        let close_color = match self.close_button.borrow().state() {
            ButtonState::Idle => CLOSE_IDLE_COLOR,
            ButtonState::Hover => CLOSE_HOVER_COLOR,
            ButtonState::Pressed => CLOSE_PRESSED_COLOR,
        };
        Self::draw_close_glyph(buf, layout.close, close_color);

        // Agent input: braille box + placeholder/text inset inside the border.
        Self::draw_field_box(buf, layout.agent_input);
        self.agent_input.borrow_mut().set_focused(self.focus == PopupFocus::AgentInput);
        self.agent_input.borrow_mut().render(buf, Self::field_content(layout.agent_input));

        // Instructions editor: braille box + seeded/edited text (+ its own
        // scrollbar thumb) inset inside the border.
        Self::draw_field_box(buf, layout.instructions);
        self.instructions.borrow_mut().set_focused(self.focus == PopupFocus::Instructions);
        self.instructions.borrow_mut().render(buf, Self::field_content(layout.instructions));

        // File-path row: plain text, beneath the instructions editor. Long
        // paths (e.g. a deep base dir) are front-truncated, not tail-
        // truncated like `label`'s own clipping, so the file name (and its
        // `.md` extension) always stays visible rather than the dir prefix.
        let path_row = layout.file_path.to_cell_rect();
        let path_text = Self::fit_path_text(&self.display_path().to_string_lossy(), path_row.width);
        label(
            buf,
            path_row,
            &path_text,
            TextAlign::Left,
            Style::default().fg(Color::Rgb(
                POPUP_PATH_COLOR.r,
                POPUP_PATH_COLOR.g,
                POPUP_PATH_COLOR.b,
            )),
        );
    }

    /// Front-truncates `text` (keeping its tail, prefixed with `…`) so it
    /// fits within `max_width` cells — the inverse of `label`'s own tail
    /// truncation, so a long path's file name (and extension) stays visible
    /// instead of being clipped off the end.
    fn fit_path_text(text: &str, max_width: u16) -> String {
        let max_width = max_width as usize;
        if max_width == 0 || text.chars().count() <= max_width {
            return text.to_string();
        }
        let keep = max_width.saturating_sub(1);
        let tail: String = text.chars().rev().take(keep).collect::<Vec<_>>().into_iter().rev().collect();
        format!("…{tail}")
    }

    /// The editor content rect inside a field's braille box — the box rect
    /// inset by `FIELD_BORDER_CELLS` on every side so text never overlaps the
    /// border. `rect` is whole-cell, so `to_cell_rect` is lossless.
    fn field_content(rect: DotRect) -> Rect {
        rect.inset(
            FIELD_BORDER_CELLS * 2,
            FIELD_BORDER_CELLS * 2,
            FIELD_BORDER_CELLS * 4,
            FIELD_BORDER_CELLS * 4,
        )
        .to_cell_rect()
    }

    /// Draws a chamfer-1 hollow braille box around a field rect via the dot
    /// pipeline (CLAUDE.md rule 4). `rect` is whole-cell, so `to_cell_rect` is
    /// lossless.
    fn draw_field_box(buf: &mut Buffer, rect: DotRect) {
        let (w, h) = (rect.w as usize, rect.h as usize);
        if w == 0 || h == 0 {
            return;
        }
        let ring = ui_primitives::rounded_rect(w, h, 1, 1, FIELD_BORDER_COLOR, Dot::Transparent);
        draw_dots(buf, rect.to_cell_rect(), &ring);
    }

    /// Draws the close control — a braille ring with an "X" through it — into
    /// `close`'s dot-precise rect via the dot pipeline (CLAUDE.md rule 4 —
    /// non-text UI chrome). `close` is whole-cell, so `to_cell_rect` is
    /// lossless.
    fn draw_close_glyph(buf: &mut Buffer, close: DotRect, color: Rgba) {
        let (w, h) = (close.w as usize, close.h as usize);
        if w == 0 || h == 0 {
            return;
        }
        let mut dotbuf = DotBuffer::new(w, h);
        let cx = (w as f32 - 1.0) / 2.0;
        let cy = (h as f32 - 1.0) / 2.0;
        let radius = cx.min(cy).max(1.0);

        // Ring: dots within ~half a dot of the circle radius.
        for row in 0..h {
            for col in 0..w {
                let (dx, dy) = (col as f32 - cx, row as f32 - cy);
                if ((dx * dx + dy * dy).sqrt() - radius).abs() <= 0.6 {
                    dotbuf.set(col, row, Dot::Lit(color));
                }
            }
        }

        // X through the center: two short diagonals inside the ring.
        let arm = (radius * 0.5).round().max(1.0) as i32;
        for d in -arm..=arm {
            for (px, py) in [(cx + d as f32, cy + d as f32), (cx + d as f32, cy - d as f32)] {
                let (c, r) = (px.round() as i32, py.round() as i32);
                if c >= 0 && (c as usize) < w && r >= 0 && (r as usize) < h {
                    dotbuf.set(c as usize, r as usize, Dot::Lit(color));
                }
            }
        }

        draw_dots(buf, close.to_cell_rect(), &dotbuf);
    }

    /// The instructions path actually in use for this popup — the temp base
    /// dir in tests, the runtime resolver in prod — honest with where writes
    /// go (b2-t2).
    fn display_path(&self) -> PathBuf {
        crate::instructions::instructions_path_maybe(
            self.instructions_base.as_deref(),
            &self.creature_name,
        )
    }

    /// Popup width, in cells, for a given screen `area` (`POPUP_W_FRAC` of
    /// the screen, clamped to the screen).
    #[allow(dead_code)] // consumed by b2-t1's compute_layout + layout
    fn popup_w_cells(area: Rect) -> u16 {
        ((area.width as f32 * POPUP_W_FRAC) as u16).min(area.width)
    }

    /// Interior editor width, in cells — the popup width minus the
    /// left/right pad — fed into `TextEditor::desired_rows`.
    #[allow(dead_code)] // consumed by b2-t1's layout
    fn interior_w_cells(area: Rect) -> u16 {
        Self::popup_w_cells(area).saturating_sub(POPUP_PAD_X_CELLS * 2)
    }

    /// The whole-cell centered popup frame: `POPUP_W_FRAC × POPUP_H_FRAC` of
    /// `area`, height clamped up to `POPUP_MIN_H` then grown by 1 cell per
    /// agent-input row beyond the first, centered via integer cell-space
    /// math (mirrors `battle_menu::get_dot_rect`).
    #[allow(dead_code)] // consumed by b2-t1's compute_layout
    fn popup_box(area: Rect, agent_rows: u16) -> DotRect {
        let popup_w_cells = Self::popup_w_cells(area);
        let base_h_cells = ((area.height as f32 * POPUP_H_FRAC) as u16).max(POPUP_MIN_H);
        let popup_h_cells =
            base_h_cells.saturating_add(agent_rows.saturating_sub(1)).min(area.height);

        let px = area.x + (area.width.saturating_sub(popup_w_cells)) / 2;
        let py = area.y + (area.height.saturating_sub(popup_h_cells)) / 2;

        RosterManager::cell_rect_to_dots(Rect::new(px, py, popup_w_cells, popup_h_cells))
    }
}

/// The five laid-out sub-rects of an open prompt-editor popup (b2-t1),
/// resolved fresh from `(area, agent_rows)` on every call — no stored
/// offsets, so a taller agent input re-centers for free.
#[allow(dead_code)] // fields consumed by b2-t2 (render) and b3-t1 (hit-test)
pub(super) struct PopupLayout {
    /// Whole-cell centered frame (x%2==0, y%4==0, w%2==0, h%4==0) — the
    /// `rounded_rect`/Occlude draw target.
    pub(super) popup: DotRect,
    /// Close (X) hit-rect, top-right of `popup`.
    pub(super) close: DotRect,
    /// Agent-input editor body; `h` tracks `agent_rows` cells.
    pub(super) agent_input: DotRect,
    /// Instructions editor body; fills the remaining interior height.
    pub(super) instructions: DotRect,
    /// File-path text row, directly beneath `instructions`.
    pub(super) file_path: DotRect,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::Terminal;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::scenes::test_util::rect_text;

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp base dir (pid + monotonic counter), mirroring
    /// `crate::instructions`'s own test helper (instructions.rs:105).
    fn temp_base_dir(tag: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "game-prompt-editor-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ))
    }

    #[test]
    fn seed_matches_file() {
        let base = temp_base_dir("seed-matches");
        let name = "Ember Wolf";
        crate::instructions::write_instructions_in(&base, name, "# known md")
            .expect("seed write should succeed");

        let popup = PromptEditor::new(0, name, Some(&base));

        assert_eq!(popup.instructions_text(), "# known md");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_file_seeds_empty() {
        let base = temp_base_dir("seed-missing");
        let name = "Nonexistent Creature";

        let popup = PromptEditor::new(0, name, Some(&base));

        assert_eq!(popup.instructions_text(), "");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// A dot-space rect representing the full screen, for margin/bounds
    /// checks against `compute_layout`'s output.
    fn screen_dots(area: Rect) -> DotRect {
        DotRect {
            x: area.x as i32 * 2,
            y: area.y as i32 * 4,
            w: area.width as i32 * 2,
            h: area.height as i32 * 4,
        }
    }

    #[test]
    fn layout_is_centered_for_given_screen() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = PromptEditor::compute_layout(area, 1);
        let popup = layout.popup;
        let screen = screen_dots(area);

        // Whole-cell rooted: the frame maps 1:1 onto braille cells (rule 4/5).
        assert_eq!(popup.x % 2, 0, "popup.x not cell-aligned: {popup:?}");
        assert_eq!(popup.y % 4, 0, "popup.y not cell-aligned: {popup:?}");
        assert_eq!(popup.w % 2, 0, "popup.w not cell-aligned: {popup:?}");
        assert_eq!(popup.h % 4, 0, "popup.h not cell-aligned: {popup:?}");

        // Fully inside the screen.
        assert!(popup.x >= screen.x && popup.y >= screen.y);
        assert!(popup.x + popup.w <= screen.x + screen.w);
        assert!(popup.y + popup.h <= screen.y + screen.h);

        // Centered: left/right (resp. top/bottom) margins differ by at most
        // one cell — whole-cell rounding can only round each side to the
        // nearest cell, never further.
        let left = popup.x - screen.x;
        let right = (screen.x + screen.w) - (popup.x + popup.w);
        assert!((left - right).abs() <= 2, "left={left} right={right}");

        let top = popup.y - screen.y;
        let bottom = (screen.y + screen.h) - (popup.y + popup.h);
        assert!((top - bottom).abs() <= 4, "top={top} bottom={bottom}");
    }

    #[test]
    fn taller_agent_input_grows_popup_and_keeps_center() {
        let area = Rect::new(0, 0, 80, 24);
        let l1 = PromptEditor::compute_layout(area, 1);
        let l3 = PromptEditor::compute_layout(area, 3);

        // Two extra agent rows -> popup + agent-input grow by exactly 2
        // cells (8 dots).
        assert_eq!(l3.popup.h, l1.popup.h + 2 * 4);
        assert_eq!(l3.agent_input.h, l1.agent_input.h + 2 * 4);
        // Agent box height = content rows + a 1-cell border top and bottom.
        assert_eq!(l1.agent_input.h, (1 + 2 * FIELD_BORDER_CELLS) * 4);
        assert_eq!(l3.agent_input.h, (3 + 2 * FIELD_BORDER_CELLS) * 4);

        // Width/horizontal placement never changes with agent_rows.
        assert_eq!(l3.popup.x, l1.popup.x);
        assert_eq!(l3.popup.w, l1.popup.w);

        // Even growth delta re-centers exactly (no stored offsets).
        let center1 = l1.popup.y + l1.popup.h / 2;
        let center3 = l3.popup.y + l3.popup.h / 2;
        assert_eq!(center1, center3, "vertical center drifted on regrow");
    }

    #[test]
    fn close_button_sits_top_right_of_popup() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = PromptEditor::compute_layout(area, 1);
        let popup = layout.popup;
        let close = layout.close;

        assert!(close.x >= popup.x && close.x + close.w <= popup.x + popup.w);
        assert!(close.y >= popup.y);

        // Right-aligned: closer to the popup's right edge than its left.
        let left_gap = close.x - popup.x;
        let right_gap = (popup.x + popup.w) - (close.x + close.w);
        assert!(
            right_gap <= left_gap,
            "close not right-aligned: left_gap={left_gap} right_gap={right_gap}"
        );

        // Top band: within the popup's top quarter.
        assert!(
            close.y - popup.y <= popup.h / 4,
            "close not in the top band: close.y={} popup.y={} popup.h={}",
            close.y,
            popup.y,
            popup.h
        );
    }

    #[test]
    fn file_path_row_sits_directly_beneath_instructions_editor() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = PromptEditor::compute_layout(area, 1);

        assert_eq!(layout.file_path.y, layout.instructions.y + layout.instructions.h);
        assert_eq!(layout.file_path.x, layout.instructions.x);
    }

    #[test]
    fn instructions_body_fills_space_below_agent_input_margin() {
        let area = Rect::new(0, 0, 80, 24);
        let layout = PromptEditor::compute_layout(area, 1);

        assert!(
            layout.instructions.y >= layout.agent_input.y + layout.agent_input.h,
            "instructions overlaps agent_input: {:?} vs {:?}",
            layout.instructions,
            layout.agent_input
        );
        assert!(layout.instructions.h > 0);
    }

    // ── b2-t2: render ───────────────────────────────────────────────────

    /// Render `popup` into a fresh `w`×`h` `TestBackend` and return the
    /// resulting buffer (mirrors `battle_menu.rs`'s standalone-render test
    /// helper; no full `RosterManager` scene needed).
    fn render_popup(popup: &PromptEditor, w: u16, h: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                popup.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// Count blank-occluder cells (braille U+2800) and lit-braille cells
    /// (U+2801..=U+28FF) across the whole buffer (mirrors `battle_menu.rs`'s
    /// `tally`).
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

    /// An open popup must occlude its interior (braille-blank cells appear)
    /// AND draw a lit border ring (lit-braille cells appear) — spec 51
    /// Testing Guidance line 66/67.
    #[test]
    fn popup_occludes_and_draws_border() {
        let popup = PromptEditor::new(0, "Test Creature", None);
        let (w, h) = (80u16, 30u16);
        let buf = render_popup(&popup, w, h);
        let (blanks, lit) = tally(&buf, w, h);
        assert!(blanks > 0, "open popup must cover its interior with braille-blank occluders");
        assert!(lit > 0, "open popup must draw a lit border");
    }

    /// The file-path row beneath the instructions editor must show the
    /// creature's instructions path as plain text.
    #[test]
    fn file_path_visible() {
        let base = temp_base_dir("file-path-visible");
        let name = "Path Creature";
        let popup = PromptEditor::new(0, name, Some(&base));
        let (w, h) = (80u16, 30u16);
        let buf = render_popup(&popup, w, h);

        let layout = PromptEditor::compute_layout(Rect::new(0, 0, w, h), 1);
        let text = rect_text(&buf, layout.file_path.to_cell_rect());
        assert!(text.contains(".md"), "file-path row must show the instructions path, got {text:?}");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// An empty agent input must show its placeholder in its laid-out rect.
    #[test]
    fn agent_placeholder_renders() {
        let popup = PromptEditor::new(0, "Test Creature", None);
        let (w, h) = (80u16, 30u16);
        let buf = render_popup(&popup, w, h);

        let layout = PromptEditor::compute_layout(Rect::new(0, 0, w, h), 1);
        let text = rect_text(&buf, layout.agent_input.to_cell_rect());
        assert!(
            text.contains("Prompt agent to update"),
            "empty agent input must show its placeholder, got {text:?}"
        );
    }

    /// Seeded instructions text must render into the instructions editor's
    /// laid-out rect.
    #[test]
    fn instructions_text_renders() {
        let base = temp_base_dir("instructions-text-renders");
        let name = "Seeded Creature";
        crate::instructions::write_instructions_in(&base, name, "distinctseedword")
            .expect("seed write should succeed");
        let popup = PromptEditor::new(0, name, Some(&base));
        let (w, h) = (80u16, 30u16);
        let buf = render_popup(&popup, w, h);

        let layout = PromptEditor::compute_layout(Rect::new(0, 0, w, h), 1);
        let text = rect_text(&buf, layout.instructions.to_cell_rect());
        assert!(
            text.contains("distinctseedword"),
            "seeded instructions text must render, got {text:?}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// After render, the close button's hit-rect must match the layout's
    /// `close` rect — b3-t1's hit-test reuses this rect, not a recompute.
    #[test]
    fn close_rect_matches_layout() {
        let popup = PromptEditor::new(0, "Test Creature", None);
        let (w, h) = (80u16, 30u16);
        let _ = render_popup(&popup, w, h);

        let expected = PromptEditor::compute_layout(Rect::new(0, 0, w, h), 1).close.to_cell_rect();
        assert_eq!(popup.close_button.borrow().rect(), expected);
    }

    // ── b3-t3: agent input submit stub + Shift+Enter grow ─────────────────

    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

    /// Focuses the agent input (default focus on open is Instructions).
    fn focus_agent_input(popup: &mut PromptEditor) {
        let tab = InputEvent::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(!popup.handle_input(&tab));
        assert_eq!(popup.focus, PopupFocus::AgentInput);
    }

    /// Enter on a focused agent input must be a no-op stub: it does not
    /// close the popup, does not mark the popup dirty (no instructions
    /// write is scheduled), and clears the agent input's text — spec 51
    /// Testing Guidance line 64.
    #[test]
    fn agent_submit_clears_input_and_stays_open() {
        let mut popup = PromptEditor::new(0, "Test Creature", None);
        focus_agent_input(&mut popup);

        for c in ['h', 'i'] {
            let key = InputEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            popup.handle_input(&key);
        }
        assert_eq!(popup.agent_input.borrow().text(), "hi");

        let enter = InputEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let closed = popup.handle_input(&enter);

        assert!(!closed, "Enter on the agent input must not close the popup");
        assert_eq!(
            popup.agent_input.borrow().text(),
            "",
            "submit stub must clear the agent input"
        );
        assert!(!popup.dirty, "agent submit must never mark the instructions dirty");
    }

    /// Shift+Enter on a focused agent input inserts a newline and grows the
    /// laid-out popup height by one cell (one more agent-input row) — spec
    /// 51 Testing Guidance line 64.
    #[test]
    fn agent_shift_enter_grows_desired_rows() {
        let mut popup = PromptEditor::new(0, "Test Creature", None);
        focus_agent_input(&mut popup);

        let area = Rect::new(0, 0, 80, 30);
        let before = popup.layout(area).agent_input.h;

        let shift_enter = InputEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        popup.handle_input(&shift_enter);

        let after = popup.layout(area).agent_input.h;
        assert_eq!(after, before + 4, "Shift+Enter must grow desired_rows by one cell");
    }

    /// The agent input's grow-with-content is capped at `AGENT_INPUT_MAX_ROWS`
    /// — repeated Shift+Enter must stop growing the popup once the cap is
    /// hit.
    #[test]
    fn agent_grow_caps_at_max_rows() {
        let mut popup = PromptEditor::new(0, "Test Creature", None);
        focus_agent_input(&mut popup);

        let area = Rect::new(0, 0, 80, 30);
        let shift_enter = InputEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        for _ in 0..8 {
            popup.handle_input(&shift_enter);
        }

        let rows = popup.agent_input.borrow().desired_rows(PromptEditor::interior_w_cells(area));
        assert_eq!(rows, AGENT_INPUT_MAX_ROWS, "agent input growth must clamp at the max-rows cap");
    }
}
