//! Inline edit-mode input routing: typed keys land in the active blank,
//! Tab/Shift-Tab cycle which blank is active without ever switching the
//! selected egg, Esc returns to browsing (both handled alongside browse
//! input in `mod.rs::handle_input`), and the gated submit composes the
//! blanks into the completed sentence and drives the Done pipeline only
//! once every blank holds non-empty text.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::mad_lib::{self, Segment};
use super::mad_lib_paragraph::{self, Caret, ParaRun};
use super::selection::HatcheryMode;

/// Width/height, in cells, of the panel's action button (Submit while
/// editing, Hatch otherwise), anchored at the reserved button slot's
/// bottom-right corner.
const ACTION_W_CELLS: u16 = 12;
const ACTION_H_CELLS: u16 = 3;

impl super::Hatchery {
    /// Paints the editing egg's mad-lib inline into `body`: `egg`'s
    /// template literals verbatim, each blank's live text from
    /// `blank_editors`, and a blinking `Caret` on the active blank.
    pub(crate) fn draw_editing_paragraph(&self, buf: &mut Buffer, body: Rect, egg: usize) {
        let template = mad_lib::select_template(egg);
        let texts: Vec<String> = self.blank_editors.iter().map(|e| e.borrow().text()).collect();
        let mut blank_idx = 0;
        let runs: Vec<ParaRun> = template
            .segments()
            .iter()
            .map(|seg| match seg {
                Segment::Literal(text) => ParaRun::Literal(text),
                Segment::Blank { .. } => {
                    let run = ParaRun::Blank(texts[blank_idx].as_str());
                    blank_idx += 1;
                    run
                }
            })
            .collect();

        let caret = match self.mode {
            HatcheryMode::Editing { active_blank } => self.blank_editors.get(active_blank).map(|editor| {
                let editor = editor.borrow();
                Caret {
                    blank_ordinal: active_blank,
                    cursor: editor.text().chars().count(),
                    visible: editor.caret_visible(),
                }
            }),
            HatcheryMode::Browsing { .. } => None,
        };

        mad_lib_paragraph::render(buf, body, &runs, caret);
    }

    /// The panel action button's fixed-size rect, anchored to `slot`'s
    /// bottom-right corner and clamped to its bounds. Shared by the Submit
    /// button (editing) and the Hatch button (Incubating/Ready) — see
    /// `browse_panel::draw_panel_action`.
    pub(crate) fn action_button_rect(slot: Rect) -> Rect {
        let w = ACTION_W_CELLS.min(slot.width);
        let h = ACTION_H_CELLS.min(slot.height);
        Rect {
            x: slot.x + slot.width.saturating_sub(w),
            y: slot.y + slot.height.saturating_sub(h),
            width: w,
            height: h,
        }
    }

    /// The submit gate: true iff an egg is under edit and every blank holds
    /// non-empty (post-trim) text. The single rule shared by `try_submit_edit`
    /// and the Submit button's grey/gold styling.
    pub(crate) fn edit_ready_to_submit(&self) -> bool {
        self.editing_egg().is_some()
            && !self.blank_editors.iter().any(|e| e.borrow().text().trim().is_empty())
    }

    /// Composes the edited egg's blanks into its template's completed
    /// sentence and submits it through `begin_definition`, but only once
    /// every blank holds non-empty (post-trim) text. Returns whether it
    /// actually submitted; a no-op returning `false` while browsing (no
    /// editing egg) or with any blank still empty or whitespace-only.
    pub(crate) fn try_submit_edit(&mut self) -> bool {
        if !self.edit_ready_to_submit() {
            return false;
        }
        let egg = self.editing_egg().expect("edit_ready_to_submit implies editing_egg().is_some()");
        let texts: Vec<String> = self.blank_editors.iter().map(|e| e.borrow().text()).collect();
        let sentence = mad_lib::completed_sentence(mad_lib::select_template(egg), &texts);
        self.begin_definition(sentence);
        true
    }
}
