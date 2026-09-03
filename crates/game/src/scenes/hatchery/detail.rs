//! Read-only detail rendering for a selected, defined (`Incubating`/`Ready`)
//! egg: paints its completed `mad_lib` sentence as plain wrapped prose in
//! the detail body region, with no editable underline or caret. For an
//! `Incubating` egg the countdown occupies the body's top row (the same
//! floored cell-row `focus::draw_countdown` paints), so the prose is inset
//! one row below it; a `Ready` egg (no countdown) uses the full body.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use engine_render::DotRect;

use crate::player_data::EggState;

use super::focus;
use super::mad_lib_paragraph::{self, ParaRun};

impl super::Hatchery {
    /// Paints `egg`'s completed `mad_lib` sentence, if any, as read-only
    /// prose into `body`. No-op if the egg has no `mad_lib`.
    pub(crate) fn draw_defined_detail(&self, buf: &mut Buffer, egg_dr: DotRect, body: Rect, egg: usize) {
        let Some(sentence) = self.eggs[egg].mad_lib.as_deref() else { return };

        let prose = if matches!(self.eggs[egg].state, EggState::Incubating { .. }) {
            let y = body.y.max(focus::countdown_row(egg_dr, buf) + 1);
            Rect { x: body.x, y, width: body.width, height: body.height.saturating_sub(y - body.y) }
        } else {
            body
        };

        mad_lib_paragraph::render(buf, prose, &[ParaRun::Literal(sentence)], None);
    }
}
