//! Right-panel body layout and per-state rendering: the panel border, the
//! STATUS zone, and the branchable body (editable mad-lib while Undefined,
//! read-only prose while Incubating/Ready).
#![allow(dead_code)]

use std::time::SystemTime;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use engine_core::color::Rgba;
use engine_render::{ActiveStyle, ButtonState, DotRect, TextAlign};

use crate::player_data::{Egg, EggState};

use super::mad_lib_paragraph::TEXT_COLOR;

/// The panel's border color — the same grey as the roster details panel's
/// own chrome.
const BORDER_COLOR: Rgba = Rgba::rgb(0x88, 0x88, 0x88);

/// Per-render override for the panel action button while disabled (a blank
/// still empty while editing, or a selected Incubating egg): greyscale
/// border + label, the same mechanism `main_hub`'s active-tab highlight
/// uses, reusing the panel's own border grey.
const DISABLED_STYLE: ActiveStyle = ActiveStyle { border: BORDER_COLOR, label: BORDER_COLOR };

/// Verbatim tooltip shown while hovering a selected Incubating egg's
/// disabled Hatch button.
const HATCH_DISABLED_TIP: &str = "Hatching is available once incubation is complete.";

/// Width, in cells, of the disabled-Hatch tooltip card — wide enough to fit
/// `HATCH_DISABLED_TIP` on a single line, so the card renders as one row.
const TIP_WIDTH_CELLS: u16 = 58;

/// The single per-state action the right-panel button represents for the
/// selected egg. Read by both `draw_browse_panel`'s render and `mod.rs`'s
/// mouse routing, so the button's look, its click outcome, and its
/// hover-tooltip all derive from one decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelAction {
    /// Editing an `Undefined` egg; `enabled` mirrors `edit_ready_to_submit`.
    Submit { enabled: bool },
    /// A selected `Incubating` egg: inert, tooltip on hover.
    HatchDisabled,
    /// A selected `Ready` egg: active, click records a hatch request.
    HatchReady,
}

/// Height, in cells, reserved for the action-button slot at the bottom of
/// the panel's inner content.
const BUTTON_H_CELLS: u16 = 3;

/// Height, in cells, reserved for the STATUS line — two rows so the longest
/// readout (e.g. "Incubating — 23h 59m remaining") still fits at the
/// panel's narrower widths via `wrapped_text`, without colliding with body.
const STATUS_H_CELLS: u16 = 2;

/// The panel's inner content split into a wrap-capable STATUS zone, the
/// DESCRIPTION body, and the reserved action-button slot — each rect inset
/// at least one cell from `panel`'s border ring.
pub(crate) struct PanelRegions {
    pub status: Rect,
    pub body: Rect,
    pub button: Rect,
}

/// Carves `panel`'s inner content (strictly inside the border ring) into the
/// STATUS zone (top), the DESCRIPTION body (middle), and the reserved
/// action-button slot (bottom). Saturating: a degenerate `panel` yields
/// non-negative zero-or-more-size rects, never a panic.
pub(crate) fn panel_regions(panel: DotRect) -> PanelRegions {
    // Inset by one full cell (2 dots wide, 4 dots tall) on every edge so the
    // inner content never overlaps the border ring drawn around `panel`.
    let inner = panel.inset(2, 2, 4, 4).to_cell_rect();

    let status_h = STATUS_H_CELLS.min(inner.height);
    let button_h = BUTTON_H_CELLS.min(inner.height.saturating_sub(status_h));
    let body_h = inner.height.saturating_sub(status_h).saturating_sub(button_h);

    let status = Rect { x: inner.x, y: inner.y, width: inner.width, height: status_h };
    let body = Rect { x: inner.x, y: inner.y + status_h, width: inner.width, height: body_h };
    let button = Rect { x: inner.x, y: inner.y + status_h + body_h, width: inner.width, height: button_h };

    PanelRegions { status, body, button }
}

/// The panel STATUS text for `egg` at `now`: "Awaiting Description"
/// (Undefined), "Incubating — {H}h {M}m remaining" (Incubating, whole
/// hours+minutes, seconds omitted), or "Ready to Hatch" (Ready).
pub(crate) fn status_text(egg: &Egg, now: SystemTime) -> String {
    match egg.state {
        EggState::Undefined => "Awaiting Description".to_string(),
        EggState::Ready => "Ready to Hatch".to_string(),
        EggState::Incubating { .. } => {
            let remaining = super::lifecycle::remaining(egg, now).unwrap_or_default();
            let secs = remaining.as_secs();
            let hours = secs / 3600;
            let minutes = (secs % 3600) / 60;
            format!("Incubating \u{2014} {hours}h {minutes}m remaining")
        }
    }
}

impl super::Hatchery {
    /// The one panel render site: grey border via `draw_dot_border`, the
    /// wrapped STATUS text, the editable mad-lib (Undefined/editing) or
    /// read-only prose (Incubating/Ready) in the reserved body region, and
    /// the per-state action button in the reserved button slot.
    pub(crate) fn draw_browse_panel(&self, buf: &mut Buffer, panel: DotRect, egg: usize) {
        engine_render::draw_dot_border(buf, panel, 1, 1, BORDER_COLOR);
        let regions = panel_regions(panel);

        let text = status_text(&self.eggs[egg], SystemTime::now());
        engine_render::wrapped_text(
            buf,
            regions.status,
            &text,
            TextAlign::Left,
            Style::default().fg(Color::Rgb(TEXT_COLOR.r, TEXT_COLOR.g, TEXT_COLOR.b)),
            false,
        );

        if let Some(editing) = self.editing_egg() {
            self.draw_editing_paragraph(buf, regions.body, editing);
        } else if self.eggs[egg].mad_lib.is_some() {
            self.draw_defined_detail(buf, regions.body, egg);
        }

        if let Some(action) = self.panel_action(egg) {
            self.draw_panel_action(buf, regions.button, action);
        }
    }

    /// The one action-decision site for egg `egg`'s panel button: `Submit`
    /// while editing an `Undefined` egg (gold once `edit_ready_to_submit`),
    /// `HatchDisabled`/`HatchReady` for a selected non-editing
    /// Incubating/Ready egg, or `None` for a non-editing `Undefined` egg
    /// (no button). Both `draw_panel_action` and `mod.rs`'s mouse routing
    /// must call this rather than re-deriving state from `eggs[egg].state`.
    pub(crate) fn panel_action(&self, egg: usize) -> Option<PanelAction> {
        if self.editing_egg().is_some() {
            return Some(PanelAction::Submit { enabled: self.edit_ready_to_submit() });
        }
        match self.eggs.get(egg)?.state {
            EggState::Undefined => None,
            EggState::Incubating { .. } => Some(PanelAction::HatchDisabled),
            EggState::Ready => Some(PanelAction::HatchReady),
        }
    }

    /// Draws `action`'s button into the reserved `slot`, positioned via
    /// `action_button_rect`, styled grey (disabled) or gold (active) via
    /// `set_active_style`, and — for a hovered `HatchDisabled` button — the
    /// disabled-hatch tooltip.
    pub(crate) fn draw_panel_action(&self, buf: &mut Buffer, slot: Rect, action: PanelAction) {
        let rect = Self::action_button_rect(slot);
        let is_submit = matches!(action, PanelAction::Submit { .. });
        let mut button = if is_submit { self.submit_button.borrow_mut() } else { self.hatch_button.borrow_mut() };
        button.set_rect(rect);
        let active_style = match action {
            PanelAction::Submit { enabled: true } | PanelAction::HatchReady => None,
            PanelAction::Submit { enabled: false } | PanelAction::HatchDisabled => Some(DISABLED_STYLE),
        };
        button.set_active_style(active_style);
        button.render(buf);

        if matches!(action, PanelAction::HatchDisabled) && button.state() == ButtonState::Hover {
            let anchor = DotRect {
                x: rect.x as i32 * 2,
                y: rect.y as i32 * 4,
                w: rect.width as i32 * 2,
                h: rect.height as i32 * 4,
            };
            crate::scenes::tooltip::render_text(buf, anchor, HATCH_DISABLED_TIP, TIP_WIDTH_CELLS);
        }
    }
}
