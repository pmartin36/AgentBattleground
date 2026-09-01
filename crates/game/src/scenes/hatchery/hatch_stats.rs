//! Post-hatch stats panel: a grey rounded-border chrome (dot pipeline) plus
//! the revealed creature's level and STR/DEX/INT/VIT values (plain text),
//! drawn in the right-hand gutter beside the focused reveal from the
//! name-reveal phase onward through idle and attack. Pure geometry + a
//! render fn; no state of its own.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{ui_primitives, DotRect, TextAlign};

use crate::player_data::PersistedCreature;
use crate::scenes::post_battle::columns::blit_dots;
use crate::scenes::roster_manager::RosterManager;
use crate::stats::StatKind;

/// Gap, in dots, between the focus rect's right edge and the panel's left
/// edge — keeps the panel visibly separate from the reveal, never touching.
const PANEL_GAP_DOTS: i32 = 4;
/// Margin, in dots, between the panel's right edge and `area`'s right edge.
const PANEL_RIGHT_MARGIN_DOTS: i32 = 2;
/// Interior padding, in dots, between the panel's border and its text rows.
const PANEL_INSET_DOTS: i32 = 4;
/// Panel border color — the same grey as the roster details panel's own
/// chrome (`BORDER_COLOR`).
const BORDER_COLOR: Rgba = Rgba::rgb(0x88, 0x88, 0x88);

/// Right-gutter panel rect in DOT space (unfloored — the draw site floors),
/// disjoint from `focus_dr`. Exposed so tests can locate the exact region
/// to decode/scan.
pub(super) fn stats_panel_rect(area: Rect, focus_dr: DotRect) -> DotRect {
    let area_right_dots = (area.x as i32 + area.width as i32) * 2;
    let x = focus_dr.x + focus_dr.w + PANEL_GAP_DOTS;
    let w = (area_right_dots - PANEL_RIGHT_MARGIN_DOTS - x).max(0);
    DotRect { x, y: focus_dr.y, w, h: focus_dr.h }
}

/// Draws the grey rounded border chrome plus the creature's stat values.
/// A no-op when the computed panel is too small to fit chrome + text —
/// never panics.
pub(super) fn draw_stats_panel(buf: &mut Buffer, area: Rect, focus_dr: DotRect, creature: &PersistedCreature) {
    let panel = stats_panel_rect(area, focus_dr);
    let panel_cells = panel.to_cell_rect();
    let interior = panel.inset(PANEL_INSET_DOTS, PANEL_INSET_DOTS, PANEL_INSET_DOTS, PANEL_INSET_DOTS).to_cell_rect();
    // One row for "Lv {level}" plus one row per stat.
    let rows_needed = 1 + StatKind::ALL.len() as u16;
    if panel_cells.width == 0 || panel_cells.height == 0 || interior.width == 0 || interior.height < rows_needed {
        return;
    }

    let dots = ui_primitives::rounded_rect(panel.w.max(0) as usize, panel.h.max(0) as usize, 1, 1, BORDER_COLOR, Dot::Transparent);
    blit_dots(buf, panel, &dots);

    let text_style = Style::default().fg(Color::Rgb(0xff, 0xff, 0xff));
    let mut row = Rect { x: interior.x, y: interior.y, width: interior.width, height: 1 };
    engine_render::label(buf, row, &format!("Lv {}", creature.level), TextAlign::Left, text_style);
    for kind in StatKind::ALL {
        row.y += 1;
        let text = format!("{} {}", RosterManager::stat_label(kind), creature.stats.value(kind));
        engine_render::label(buf, row, &text, TextAlign::Left, text_style);
    }
}
