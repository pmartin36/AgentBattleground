//! Shared detail-panel body rendering: the stamina row and the abilities
//! section, consumed by both the roster detail screen and the hatchery
//! settled-placement stats dock. Each caller supplies its own
//! `Stamina`/`Ability` data and geometry rather than a concrete creature
//! type, so both callers render through this one component.

use crate::ability::Ability;
use crate::stamina::Stamina;
use engine_render::DotRect;
use ratatui::buffer::Buffer;

/// Dot-rows tall for a header underline.
pub(crate) const HEADER_UNDERLINE_THICKNESS_DOTS: i32 = 2;
/// Extra dot-width the underline runs past the header text's right edge.
pub(crate) const HEADER_UNDERLINE_PAD_DOTS: i32 = 2;
/// Underline color — white, matching the header/label text color.
pub(crate) const HEADER_UNDERLINE_COLOR: engine_core::color::Rgba =
    engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);

/// Stamina status text colour — white, matching `HEADER_UNDERLINE_COLOR`.
pub(crate) const STAMINA_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);
/// Divisor for converting an `injured_until` remaining `Duration` into
/// whole days-remaining. Not `stamina::RECOVERY_DURATION`, which is a
/// recovery span, not a per-day unit.
pub(crate) const SECS_PER_DAY: u64 = 24 * 60 * 60;

/// Ability list text colour — white, matching `STAMINA_COLOR`.
pub(crate) const ABILITY_COLOR: engine_core::color::Rgba = engine_core::color::Rgba::rgb(0xff, 0xff, 0xff);

/// Dot gap between the stamina label slot and its bar slot.
pub(crate) const STAMINA_LABEL_BAR_GAP_DOTS: i32 = 4;
/// 1-cell (2-dot) left/right margin the stamina row keeps off the panel
/// interior edges.
pub(crate) const STAMINA_EDGE_MARGIN_DOTS: i32 = 2;

/// 1-cell blank top margin above the stamina row, measured from the panel
/// interior's top edge.
pub(crate) const PANEL_TOP_MARGIN_CELLS: i32 = 1;
/// Stamina band height — 2 cells: the "Stamina" label + bar on the top
/// cell-row, and the braille header underline on the row beneath (like the
/// Abilities header band). `render_stamina_row` constrains the label+bar to
/// the top 4-dot row so `bars::draw_bar` still gets an exact-height target.
pub(crate) const STAMINA_ROW_H_CELLS: i32 = 2;
/// Gap between the stamina row and the abilities header band.
pub(crate) const STAMINA_ABILITIES_GAP_CELLS: i32 = 2;
/// Header band height — row 0 is the header text, row 1 is the blank row
/// reserved for the braille underline.
pub(crate) const HEADER_BAND_H_CELLS: i32 = 2;
/// Height of the 2x2 ability grid band (two 1-cell rows, no inter-row gap).
pub(crate) const ABILITY_GRID_H_CELLS: i32 = 2;
/// Pinned inter-column gap between the ability grid's two columns.
pub(crate) const ABILITY_GRID_COL_GAP_CELLS: i32 = 1;
/// Inter-row gap within an ability grid column (none — the two rows sit
/// flush).
pub(crate) const ABILITY_GRID_ROW_GAP_CELLS: i32 = 0;
/// Gap between the ability grid band and the caller's `bottom` slot.
pub(crate) const GRID_BOTTOM_GAP_CELLS: i32 = 2;

/// Named interior regions of the shared detail-panel body, all in DOT space
/// (unfloored `DotRect` — CLAUDE.md rule 5; every dot-drawn consumer floors
/// at its own draw site, never here). Reading order for `ability_cells`:
/// `[0]`=top-left `[1]`=top-right `[2]`=bottom-left `[3]`=bottom-right.
/// `bottom` is the single grow region below the ability grid + its gap; the
/// caller subdivides it into its own content (roster: instructions header
/// band + edit button + preview; the hatchery dock: Keep/Discard controls).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DetailPanelRegions {
    /// 1-cell-tall row for the stamina label + bar.
    pub stamina: DotRect,
    /// 1-cell text rect; a blank cell row is reserved directly beneath for
    /// the braille underline.
    pub abilities_header: DotRect,
    pub ability_cells: [DotRect; 4],
    /// Grow remainder below the ability grid and its gap.
    pub bottom: DotRect,
}

/// Carves `border`'s interior (inset 1 cell) into the shared body's regions,
/// composed entirely from `engine_render::flex` over `DotRect` — no
/// hand-computed cell offsets.
pub(crate) fn interior_regions(border: DotRect) -> DetailPanelRegions {
    let interior = border.inset(2, 2, 4, 4);

    let column = engine_render::flex(
        interior,
        engine_render::FlexStyle {
            direction: engine_render::Direction::Column,
            justify_content: engine_render::Justify::Start,
            align_items: engine_render::Align::Stretch,
            gap: 0,
        },
        &[
            // top margin
            engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(PANEL_TOP_MARGIN_CELLS * 4),
                grow: 0.0,
                shrink: 0.0,
            },
            // stamina row
            engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(STAMINA_ROW_H_CELLS * 4),
                grow: 0.0,
                shrink: 0.0,
            },
            // gap
            engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(STAMINA_ABILITIES_GAP_CELLS * 4),
                grow: 0.0,
                shrink: 0.0,
            },
            // abilities header band (text row + reserved underline row)
            engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(HEADER_BAND_H_CELLS * 4),
                grow: 0.0,
                shrink: 0.0,
            },
            // ability grid band
            engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(ABILITY_GRID_H_CELLS * 4),
                grow: 0.0,
                shrink: 0.0,
            },
            // gap
            engine_render::FlexChild {
                basis: engine_render::Basis::Fixed(GRID_BOTTOM_GAP_CELLS * 4),
                grow: 0.0,
                shrink: 0.0,
            },
            // bottom — sole grow child, absorbs remaining space; the caller
            // subdivides it into its own content.
            engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
        ],
    );
    let [_top_margin, stamina, _gap1, abilities_header_band, grid_band, _gap2, bottom] = column[..] else {
        unreachable!("flex() with 7 children returns exactly 7 rects")
    };

    // `.min(4)` (not a bare `4`) so a collapsed band (insufficient vertical
    // room — e.g. a short terminal) yields a genuinely zero-height header
    // instead of one whose height claims more than the band actually has,
    // which would make `draw_header_underline` (anchored at
    // `header.y + header.h`) draw past the band's real bottom edge.
    let abilities_header = DotRect { h: abilities_header_band.h.min(4), ..abilities_header_band };

    // Ability grid: 2 columns x 2 rows, reading order [TL, TR, BL, BR].
    // Names are centered within each column (no left indent).
    let grid_cols = engine_render::flex(
        grid_band,
        engine_render::FlexStyle {
            direction: engine_render::Direction::Row,
            justify_content: engine_render::Justify::Start,
            align_items: engine_render::Align::Stretch,
            gap: ABILITY_GRID_COL_GAP_CELLS * 2,
        },
        &[
            engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
        ],
    );
    let [left_col, right_col] = grid_cols[..] else {
        unreachable!("flex() with 2 children returns exactly 2 rects")
    };

    let grid_rows_style = engine_render::FlexStyle {
        direction: engine_render::Direction::Column,
        justify_content: engine_render::Justify::Start,
        align_items: engine_render::Align::Stretch,
        gap: ABILITY_GRID_ROW_GAP_CELLS * 4,
    };
    let row_children = [
        engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
        engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
    ];
    let left_rows = engine_render::flex(left_col, grid_rows_style, &row_children);
    let right_rows = engine_render::flex(right_col, grid_rows_style, &row_children);
    let [tl, bl] = left_rows[..] else { unreachable!("flex() with 2 children returns exactly 2 rects") };
    let [tr, br] = right_rows[..] else { unreachable!("flex() with 2 children returns exactly 2 rects") };
    let ability_cells = [tl, tr, bl, br];

    DetailPanelRegions { stamina, abilities_header, ability_cells, bottom }
}

/// The stamina status line for `e`: `"Exhausted: {days} days remain"` when
/// injured (days derived from `injured_until()`), else `"Stamina:
/// {percent}%"`. Single source of both format strings.
pub(crate) fn stamina_text(e: &Stamina) -> String {
    match e.injured_until() {
        Some(remaining) => {
            let days = remaining.as_secs().div_ceil(SECS_PER_DAY);
            format!("Exhausted: {days} days remain")
        }
        None => format!("Stamina: {}%", e.percent()),
    }
}

/// Draws a horizontal lit-dot underline `HEADER_UNDERLINE_THICKNESS_DOTS`
/// dot-rows tall, in the cell-row directly beneath `header`, spanning
/// `text`'s rendered width (`text.chars().count() * 2` dots, mirroring
/// `engine_render::label`'s width measure) plus `HEADER_UNDERLINE_PAD_DOTS`,
/// clamped to `header.w`. Anchored to `header.to_cell_rect()` — the same
/// cell-floored position `label` draws the header text at (CLAUDE.md rule
/// 5) — never a raw sub-cell `header` field. Empty `text` or a zero clamped
/// width paints nothing. Bespoke dot-pipeline chrome (spec line 36): routed
/// through the shared `crate::scenes::post_battle::columns::blit_dots`
/// sub-cell placer, not a hand-rolled `dots_to_grid`/`draw_grid` call.
pub(crate) fn draw_header_underline(buf: &mut Buffer, header: DotRect, text: &str) {
    if text.is_empty() {
        return;
    }
    // A collapsed header (no vertical room — e.g. a short terminal; the
    // interior-region `.min(4)` clamp yields a genuine `h == 0` in this
    // case) means the header text itself already painted nothing
    // (`engine_render::label` no-ops on a zero-height cell rect); the
    // underline must match by not drawing either. Without this guard,
    // `target.y = header.y + header.h` collapses to `header.y` itself —
    // whatever row immediately follows the (nonexistent) header band, which
    // can be the details panel's own bottom border row — and the underline
    // paints directly into it.
    if header.h <= 0 {
        return;
    }
    let text_w = text.chars().count() as i32 * 2;
    let w = (text_w + HEADER_UNDERLINE_PAD_DOTS).min(header.w).max(0);
    if w == 0 {
        return;
    }

    // Anchor the underline to the SAME cell grid `label` drew the header
    // text at (`header.to_cell_rect()`), NOT the sub-cell `header.y`: the
    // header text is cell-quantized terminal text, and the panel interior
    // carries a structural ~2-dot y offset, so `header.y + header.h` lands
    // ~2 dots low — in the BOTTOM rows of the cell below the text, leaving
    // a visible gap. Flooring to the text's own cell places the underline
    // in the TOP `HEADER_UNDERLINE_THICKNESS_DOTS` rows of the cell row
    // directly beneath the text, hugging it. (For a cell-aligned header
    // this is identical to the old computation.)
    let header_cell = header.to_cell_rect();
    let target = DotRect {
        x: header_cell.x as i32 * 2,
        y: header_cell.bottom() as i32 * 4,
        w,
        h: HEADER_UNDERLINE_THICKNESS_DOTS,
    };
    let mut local = engine_render::dots::DotBuffer::new(target.w as usize, target.h as usize);
    for row in 0..target.h as usize {
        for col in 0..target.w as usize {
            local.set(col, row, engine_render::dots::Dot::Lit(HEADER_UNDERLINE_COLOR));
        }
    }
    crate::scenes::post_battle::columns::blit_dots(buf, target, &local);
}

/// Draws a left-aligned white section header label at `header`'s cell rect,
/// plus the braille underline beneath it. Shared by every section header in
/// the panel so they stay pixel-identical.
pub(crate) fn draw_section_header(buf: &mut Buffer, header: DotRect, text: &str) {
    engine_render::label(
        buf,
        header.to_cell_rect(),
        text,
        engine_render::TextAlign::Left,
        ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
            HEADER_UNDERLINE_COLOR.r,
            HEADER_UNDERLINE_COLOR.g,
            HEADER_UNDERLINE_COLOR.b,
        )),
    );
    draw_header_underline(buf, header, text);
}

/// Paints, into `region`, a left `"Stamina"` label plus the stamina bar for
/// `stamina`. Layout on one line: `[1-cell margin]` + label + bar-area
/// (grows to fill the rest) `[1-cell margin]`. The bar TRACK length scales
/// with `stamina.max()` — `clamp(max / STAMINA_MAX_CAP, 0.25, 1.0)` of the
/// bar area, so a full-cap creature spans the whole line and a low-max one
/// is shorter but never below 25%. The FILL inside the track is
/// `percent/100` (= `current/max`) and can be anywhere from 0 to full (the
/// 25% floor is on the track bounds, not the fill). Injured shows
/// `stamina_text` in place of the label.
pub(crate) fn render_stamina_row(buf: &mut Buffer, region: DotRect, stamina: &Stamina) {
    let percent = stamina.percent();
    let max = stamina.max();
    let label = if stamina.is_injured() { stamina_text(stamina) } else { "Stamina".to_string() };
    let fraction = percent as f32 / 100.0;
    let fill = crate::scenes::post_battle::columns::stamina_fill_color(percent);

    // The stamina band is 2 cells tall: label + bar on the top cell-row,
    // and a braille header underline on the row beneath the "Stamina" label
    // (matching the Abilities/Instructions headers). Constrain the label+bar
    // to the top 4-dot row; the underline draws into the row below.
    let top_row = DotRect { h: 4, ..region };
    // 1-cell L/R margin, then [label | bar-area]; the bar area grows to
    // absorb all remaining width of the line.
    // No LEFT inset — "Stamina" sits flush with the Abilities/Instructions
    // headers at the interior's left edge; keep the RIGHT inset so the bar
    // keeps its margin off the panel frame.
    let inner = top_row.inset(0, STAMINA_EDGE_MARGIN_DOTS, 0, 0);
    let label_w = ((label.chars().count() as i32) * 2).min(inner.w.max(0));

    let style = engine_render::FlexStyle {
        direction: engine_render::Direction::Row,
        justify_content: engine_render::Justify::Start,
        align_items: engine_render::Align::Stretch,
        gap: STAMINA_LABEL_BAR_GAP_DOTS,
    };
    let children = [
        engine_render::FlexChild { basis: engine_render::Basis::Fixed(label_w), grow: 0.0, shrink: 0.0 },
        engine_render::FlexChild { basis: engine_render::Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
    ];
    let parts = engine_render::flex(inner, style, &children);
    if parts.len() < 2 {
        return;
    }

    engine_render::label(
        buf,
        parts[0].to_cell_rect(),
        &label,
        engine_render::TextAlign::Left,
        ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
            STAMINA_COLOR.r,
            STAMINA_COLOR.g,
            STAMINA_COLOR.b,
        )),
    );

    // Header-style braille underline beneath the "Stamina" label, in the
    // second cell-row of the band.
    draw_header_underline(buf, parts[0], &label);

    // Track scales with max stamina, left-aligned in the bar area (all bars
    // start at the same x). Floor to the cell grid so the bar shares the
    // label's plane (the panel interior carries a sub-cell y offset).
    let bar_cell = parts[1].to_cell_rect();
    let bar_x = bar_cell.x as i32 * 2;
    let bar_y = bar_cell.y as i32 * 4;
    let bar_h = bar_cell.height as i32 * 4;
    let bar_full_w = bar_cell.width as i32 * 2;
    let track_frac = (max as f32 / crate::stamina::STAMINA_MAX_CAP as f32).clamp(0.25, 1.0);
    let track_w = ((bar_full_w as f32) * track_frac).round() as i32;
    let track_rect = DotRect { x: bar_x, y: bar_y, w: track_w, h: bar_h };
    crate::scenes::bars::draw_bar(buf, track_rect, fraction, fill);
}

/// Renders the "Abilities" section header (via `draw_section_header`) then
/// `abilities` into `cells` in reading order `[TL, TR, BL, BR]` — each
/// ability's `description()` left-aligned and terminal-underlined in
/// `ABILITY_COLOR`. A cell with no corresponding ability (fewer than
/// `MAX_ABILITIES`) paints nothing (blank, no placeholder/border).
pub(crate) fn render_abilities(buf: &mut Buffer, header: DotRect, cells: [DotRect; 4], abilities: &[Ability]) {
    draw_section_header(buf, header, "Abilities");

    for (i, cell) in cells.iter().enumerate() {
        let Some(ability) = abilities.get(i) else {
            continue;
        };
        engine_render::label(
            buf,
            cell.to_cell_rect(),
            ability.description(),
            engine_render::TextAlign::Center,
            ratatui::style::Style::default()
                .fg(ratatui::style::Color::Rgb(ABILITY_COLOR.r, ABILITY_COLOR.g, ABILITY_COLOR.b))
                .add_modifier(ratatui::style::Modifier::UNDERLINED),
        );
    }
}
