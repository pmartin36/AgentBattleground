//! Read-only detail rendering for a selected, defined (`Incubating`/`Ready`)
//! egg: paints its completed `mad_lib` sentence as plain wrapped prose in
//! the panel body region, with no editable underline or caret. The
//! remaining-time readout lives in the panel's STATUS row, so the prose
//! fills the full body with no countdown inset.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::mad_lib_paragraph::{self, ParaRun};

impl super::Hatchery {
    /// Paints `egg`'s completed `mad_lib` sentence, if any, as read-only
    /// prose into `body`. No-op if the egg has no `mad_lib`.
    pub(crate) fn draw_defined_detail(&self, buf: &mut Buffer, body: Rect, egg: usize) {
        let Some(sentence) = self.eggs[egg].mad_lib.as_deref() else { return };

        mad_lib_paragraph::render(buf, body, &[ParaRun::Literal(sentence)], None);
    }
}
