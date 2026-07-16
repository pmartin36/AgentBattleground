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
use engine_render::dots::Dot;
use engine_render::{
    draw_dots, flex, label, ui_primitives, Align, Basis, ButtonCore, Direction,
    DotRect, EditorEvent, FlexChild, FlexStyle, Justify, Sizing, TextAlign, TextEditor,
    TextEditorConfig,
};

use super::diagnostics_ui::DiagnosticsState;
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
/// Close (X) button hit-area, in cells. `draw_close_button` draws a round
/// 6×6-dot circle centered inside this 3×3 area with the "X" centered.
const CLOSE_W_CELLS: u16 = 3;
const CLOSE_H_CELLS: u16 = 3;
/// Inset of the close button from the popup's top and right. With the circle's
/// top half removed, the "X" (box cell 1) is the topmost visible dot, 4 dots
/// below the box top — a 0-cell top inset lands it 4 dots below the border,
/// even with the 2-cell (4-dot) right inset.
const CLOSE_TOP_INSET_CELLS: u16 = 0;
const CLOSE_RIGHT_INSET_CELLS: u16 = 2;
/// Reserves the close button's rows (inset + button) so the body clears them.
const POPUP_TOP_BAND_CELLS: u16 = CLOSE_TOP_INSET_CELLS + CLOSE_H_CELLS;
/// `[!!]` diagnostics badge inset from the popup's top-left, mirroring the
/// close (X) button on the opposite corner. Left inset matches the X's right
/// inset; the 1-cell top inset drops the 1-cell-tall text badge to align with
/// the X glyph (whose circle top-half is clipped, landing it ~1 cell down).
const BADGE_LEFT_INSET_CELLS: u16 = 2;
const BADGE_TOP_INSET_CELLS: u16 = 1;
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
    /// Buffer revision of `instructions` as of the last `poll_instructions`
    /// call (research.md b4-t2 iteration 2). Seeded from
    /// `TextEditor::revision()` AFTER `set_text` in `new()` so opening on an
    /// already-flagged file is not mistaken for an edit.
    last_revision: u64,
    /// The screen `area` last passed to `render` (b2-t1), cached so
    /// `handle_input`'s mouse branch can recompute `layout` for click-to-
    /// focus hit-testing without `Scene::handle_input`'s fixed signature
    /// threading `area` through. Defaults to `Rect::default()` before the
    /// first render, which is always ahead of the first possible click in
    /// practice (input is only routed here while the popup, itself just
    /// rendered, is open).
    last_area: Cell<Rect>,
    /// Lint cache + badge hit-rect + hover state for the instructions editor
    /// (research.md b4-t2 blueprint). `pub(super)` so the sibling
    /// `diagnostics_integration_tests` can observe it, mirroring the
    /// established `scene.prompt_editor` reach-in
    /// (`prompt_editor_modal_tests.rs:43`).
    pub(super) diagnostics: DiagnosticsState,
}

impl PromptEditor {
    /// `roster` (b3-t1): the scene's roster (e.g. `RosterManager::creatures`),
    /// used to build the instructions editor's `@`-mention vocabulary via
    /// `GameMentionProvider`. `roster.get(creature_index)` guards out-of-range
    /// indices so an empty/mismatched roster leaves mentions off rather than
    /// panicking.
    pub(super) fn new(
        creature_index: usize,
        name: &str,
        base: Option<&Path>,
        roster: &[crate::creatures::Creature],
    ) -> Self {
        let seed = crate::instructions::read_instructions_maybe(base, name).unwrap_or_default();

        let mut instructions = TextEditor::new(TextEditorConfig {
            sizing: Sizing::Fixed,
            submit_on_enter: false,
            placeholder: String::new(),
        });
        instructions.set_text(&seed);
        if let Some(creature) = roster.get(creature_index) {
            let provider = crate::mention::GameMentionProvider::new(creature, roster);
            instructions.set_mention_provider(Box::new(provider), '@');
        }
        // b4-t2: opening on an already-flagged file badges + tints
        // immediately, before the first keystroke.
        let vocab = roster.get(creature_index).map(|c| crate::mention::Vocabulary::new(c, roster));
        let mut diagnostics = DiagnosticsState::new(vocab);
        diagnostics.relint(&seed, &mut instructions);
        let last_revision = instructions.revision();

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
            last_revision,
            last_area: Cell::new(Rect::default()),
            diagnostics,
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
                KeyCode::Esc => {
                    // Spec 56: while the focused editor has an open
                    // @-mention popup, Esc dismisses the popup (leaving the
                    // literal @query typed) instead of closing the whole
                    // modal. Only close the modal when no popup captures it.
                    let editor = match self.focus {
                        PopupFocus::AgentInput => self.agent_input.get_mut(),
                        PopupFocus::Instructions => self.instructions.get_mut(),
                    };
                    if editor.mention_active() {
                        editor.handle_key(*key);
                    } else {
                        return true;
                    }
                }
                KeyCode::Tab => {
                    // Spec 56: while the focused editor has an open @-mention
                    // popup, Tab accepts the highlighted candidate (like
                    // Enter) instead of switching editor focus.
                    let editor = match self.focus {
                        PopupFocus::AgentInput => self.agent_input.get_mut(),
                        PopupFocus::Instructions => self.instructions.get_mut(),
                    };
                    if editor.mention_active() {
                        editor.handle_key(*key);
                    } else {
                        self.toggle_focus();
                    }
                }
                _ => match self.focus {
                    PopupFocus::AgentInput => {
                        if self.agent_input.get_mut().handle_key(*key) == EditorEvent::Submit {
                            self.submit_agent_input();
                        }
                    }
                    PopupFocus::Instructions => {
                        self.instructions.get_mut().handle_key(*key);
                    }
                },
            },
            InputEvent::Mouse(me) => {
                if me.kind == MouseEventKind::Moved {
                    self.diagnostics.set_hover(Position { x: me.column, y: me.row });
                }
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

    /// Whether one of this popup's text editors currently holds focus, and so
    /// owns the break key (Ctrl-C = copy). Exactly one of the two is focused
    /// while the popup is open — `render` mirrors `self.focus` onto them via
    /// `set_focused`, and both start focused before the first render.
    pub(super) fn consumes_break(&self) -> bool {
        self.agent_input.borrow().focused() || self.instructions.borrow().focused()
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

    /// Raise the pending-write/lint state when the instructions buffer has
    /// moved since the last poll, restarting the `WRITE_DEBOUNCE` countdown.
    /// The ONE place `dirty`/`debounce` are raised (research.md b4-t2
    /// iteration 2, replaces `mark_dirty`).
    ///
    /// Polls `TextEditor::revision` rather than capturing
    /// `EditorEvent::Changed` per call site: the mention popup's Tab-accept
    /// and click-accept both mutate the buffer with a `Changed` this popup
    /// cannot observe (`handle_mouse` returns `bool`), so per-call-site
    /// wiring silently misses them and leaves the decorations tinting stale
    /// columns. Polling the editor's own source of truth at one choke point
    /// makes every mutation path — including any added later — mark dirty
    /// by default.
    fn poll_instructions(&mut self) {
        let rev = self.instructions.borrow().revision();
        if rev != self.last_revision {
            self.last_revision = rev;
            self.dirty = true;
            self.debounce = WRITE_DEBOUNCE;
        }
    }

    /// Scene tick (b3-t2, b2-t2): advances both editors' blink accumulators
    /// every frame, then — while a pending edit exists — advances the
    /// debounce, flushing once it elapses. The debounce countdown stays
    /// gated on `dirty`; only the blink tick runs unconditionally.
    pub(super) fn update(&mut self, dt: Duration) {
        self.agent_input.get_mut().tick(dt);
        self.instructions.get_mut().tick(dt);
        self.poll_instructions();

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
        self.poll_instructions();
        if !self.dirty {
            return;
        }
        let text = self.instructions.borrow().text();
        let _ = crate::instructions::write_instructions_maybe(
            self.instructions_base.as_deref(),
            &self.creature_name,
            &text,
        );
        self.diagnostics.relint(&text, self.instructions.get_mut());
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

        // Close button: a fixed 4×2-cell box in the popup's top-right corner,
        // inset evenly (in dots) from the top and right edges.
        let close_w = CLOSE_W_CELLS as i32 * 2;
        let close_h = CLOSE_H_CELLS as i32 * 4;
        let close = DotRect {
            x: popup.x + popup.w - CLOSE_RIGHT_INSET_CELLS as i32 * 2 - close_w,
            y: popup.y + CLOSE_TOP_INSET_CELLS as i32 * 4,
            w: close_w,
            h: close_h,
        };

        // Diagnostics badge: top-left, mirroring the close (X) top-right.
        let badge = DotRect {
            x: popup.x + BADGE_LEFT_INSET_CELLS as i32 * 2,
            y: popup.y + BADGE_TOP_INSET_CELLS as i32 * 4,
            w: super::diagnostics_ui::BADGE_TEXT.chars().count() as i32 * 2,
            h: 4,
        };

        PopupLayout { popup, close, badge, agent_input, instructions, file_path }
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

        // Close (X): refresh the hit-rect from layout, then draw the shared
        // red rounded-rect close button keyed to hover/press state.
        self.close_button.borrow_mut().set_rect(layout.close.to_cell_rect());
        let close_state = self.close_button.borrow().state();
        crate::scenes::close_button::draw_close_button(buf, layout.close, close_state);

        // Agent input: braille box + placeholder/text inset inside the border.
        Self::draw_field_box(buf, layout.agent_input);
        self.agent_input.borrow_mut().set_focused(self.focus == PopupFocus::AgentInput);
        self.agent_input.borrow_mut().render(buf, Self::field_content(layout.agent_input));

        // Instructions editor: braille box + seeded/edited text (+ its own
        // scrollbar thumb) inset inside the border.
        Self::draw_field_box(buf, layout.instructions);
        self.instructions.borrow_mut().set_focused(self.focus == PopupFocus::Instructions);
        self.instructions.borrow_mut().render(buf, Self::field_content(layout.instructions));

        // Diagnostics badge: top-left of the popup, mirroring the close (X).
        self.diagnostics.draw_badge_in(buf, Some(layout.badge.to_cell_rect()));

        // File-path row: plain text, beneath the instructions editor, full
        // width. Long paths (e.g. a deep base dir) are front-truncated, not
        // tail-truncated like `label`'s own clipping, so the file name (and
        // its `.md` extension) always stays visible rather than the dir prefix.
        let path_rect = layout.file_path.to_cell_rect();
        let path_text = Self::fit_path_text(&self.display_path().to_string_lossy(), path_rect.width);
        label(
            buf,
            path_rect,
            &path_text,
            TextAlign::Left,
            Style::default().fg(Color::Rgb(
                POPUP_PATH_COLOR.r,
                POPUP_PATH_COLOR.g,
                POPUP_PATH_COLOR.b,
            )),
        );

        // Topmost overlay within the modal: the editor's warning card,
        // hover-only.
        self.diagnostics.render_card(buf);
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
    /// `[!!]` diagnostics badge slot, top-left of `popup` (mirrors `close`).
    pub(super) badge: DotRect,
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

        let popup = PromptEditor::new(0, name, Some(&base), &[]);

        assert_eq!(popup.instructions_text(), "# known md");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_file_seeds_empty() {
        let base = temp_base_dir("seed-missing");
        let name = "Nonexistent Creature";

        let popup = PromptEditor::new(0, name, Some(&base), &[]);

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
        let popup = hermetic_popup("Test Creature", &[]);
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
        let popup = PromptEditor::new(0, name, Some(&base), &[]);
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
        let popup = hermetic_popup("Test Creature", &[]);
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
        let popup = PromptEditor::new(0, name, Some(&base), &[]);
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
        let popup = hermetic_popup("Test Creature", &[]);
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
        let mut popup = hermetic_popup("Test Creature", &[]);
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
        let mut popup = hermetic_popup("Test Creature", &[]);
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
        let mut popup = hermetic_popup("Test Creature", &[]);
        focus_agent_input(&mut popup);

        let area = Rect::new(0, 0, 80, 30);
        let shift_enter = InputEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        for _ in 0..8 {
            popup.handle_input(&shift_enter);
        }

        let rows = popup.agent_input.borrow().desired_rows(PromptEditor::interior_w_cells(area));
        assert_eq!(rows, AGENT_INPUT_MAX_ROWS, "agent input growth must clamp at the max-rows cap");
    }

    // ── b3-t1: @-mention wiring on the instructions editor ────────────────

    use crate::ability::Ability;
    use crate::creatures::Creature;

    /// A 2-creature roster: index 0 (the edited creature) has a "Douse"
    /// ability; index 1 is a plain roster-mate for `@Frost` resolution.
    fn wired_roster() -> Vec<Creature> {
        vec![
            Creature::new("Ember Wolf").with_abilities(vec![Ability::new("Douse", vec![])]),
            Creature::new("Frost Lizard"),
        ]
    }

    /// Builds a `PromptEditor` against a fresh hermetic temp base dir (empty
    /// instructions), so no test reads or writes the real
    /// `creature_instructions/` (spec 47's mandate). `PromptEditor::new` copies
    /// the base into its own `PathBuf`, so the editor keeps working after the
    /// local `base` drops.
    fn hermetic_popup(name: &str, roster: &[Creature]) -> PromptEditor {
        let base = temp_base_dir(name);
        PromptEditor::new(0, name, Some(&base), roster)
    }

    /// Types each char of `s` into the popup's currently-focused editor.
    fn type_str(popup: &mut PromptEditor, s: &str) {
        for c in s.chars() {
            let key = InputEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
            popup.handle_input(&key);
        }
    }

    fn press_enter(popup: &mut PromptEditor) {
        let key = InputEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        popup.handle_input(&key);
    }

    /// The deliverable: `@enemy` (accept, continues) -> `least-hp` (accept,
    /// terminal) collapses to the single round-tripped token
    /// `@enemy:least-hp`, proving the game `GameMentionProvider` is attached
    /// to the instructions editor (default focus on open).
    #[test]
    fn instructions_at_mention_two_stage_yields_single_token() {
        let roster = wired_roster();
        let mut popup = hermetic_popup("Ember Wolf", &roster);

        type_str(&mut popup, "@enemy");
        press_enter(&mut popup);
        type_str(&mut popup, "least-hp");
        press_enter(&mut popup);

        assert_eq!(popup.instructions_text(), "@enemy:least-hp");
    }

    /// Spec 56 feedback: Tab with an open `@`-mention popup ACCEPTS the
    /// highlighted candidate (like Enter) and must NOT switch editor focus.
    #[test]
    fn tab_accepts_mention_when_popup_open_without_switching_focus() {
        let roster = wired_roster();
        let mut popup = hermetic_popup("Ember Wolf", &roster);

        // Instructions field focused by default; '@enemy' opens the popup
        // with the `continues` target 'enemy' highlighted.
        type_str(&mut popup, "@enemy");

        let tab = InputEvent::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let closed = popup.handle_input(&tab);

        assert!(!closed, "Tab must not close the modal");
        assert_eq!(
            popup.focus,
            PopupFocus::Instructions,
            "Tab with an open popup accepts the candidate — it must NOT switch focus"
        );
        assert_eq!(
            popup.instructions_text(),
            "@enemy:",
            "Tab must accept the highlighted mention like Enter (two-stage target)"
        );
    }

    /// With no popup open, Tab keeps its normal behavior: switch editor focus.
    #[test]
    fn tab_switches_focus_when_no_mention_popup() {
        let roster = wired_roster();
        let mut popup = hermetic_popup("Ember Wolf", &roster);

        let tab = InputEvent::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        popup.handle_input(&tab);

        assert_eq!(
            popup.focus,
            PopupFocus::AgentInput,
            "Tab toggles focus when no mention popup is open"
        );
    }

    /// The edited creature's own abilities and the roster's other creature
    /// names are offered and accept to their underscored `insert_text`.
    #[test]
    fn instructions_at_mention_offers_own_abilities_and_roster_names() {
        let roster = wired_roster();

        let mut ability_popup = hermetic_popup("Ember Wolf", &roster);
        type_str(&mut ability_popup, "@Douse");
        press_enter(&mut ability_popup);
        assert_eq!(ability_popup.instructions_text(), "@Douse");

        let mut roster_popup = hermetic_popup("Ember Wolf", &roster);
        type_str(&mut roster_popup, "@Frost");
        press_enter(&mut roster_popup);
        assert_eq!(roster_popup.instructions_text(), "@Frost_Lizard");
    }

    /// Spec 56:35 — Esc while the instructions editor has an open
    /// `@`-mention popup dismisses only the popup (leaving the literal
    /// `@query` typed), instead of closing the whole modal. A second Esc,
    /// with no popup open, closes the modal normally (unchanged behavior,
    /// mirrors `prompt_editor_modal_tests::esc_closes_popup`).
    #[test]
    fn esc_dismisses_mention_popup_not_modal() {
        let roster = wired_roster();
        let mut popup = hermetic_popup("Ember Wolf", &roster);

        type_str(&mut popup, "@enemy");

        let esc = InputEvent::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        let closed_first = popup.handle_input(&esc);

        assert!(!closed_first, "Esc with an open mention popup must not close the modal");
        assert_eq!(
            popup.instructions_text(),
            "@enemy",
            "Esc must leave the literal @query typed, not mutate it"
        );

        let closed_second = popup.handle_input(&esc);
        assert!(closed_second, "Esc with no popup open must close the modal (normal behavior)");
    }
}
