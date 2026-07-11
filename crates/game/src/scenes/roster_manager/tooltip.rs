//! Ability hover tooltip (spec 49). This module lands incrementally:
//! b1-t1 = scaffold + tunable constants + pill color palette only.
//! No rendering/geometry lives here yet (b2).

// Constants and palette fns below are consumed by b2 (geometry/pill
// rendering) and b3 (wiring) — not yet called from any production path.
// Suppress dead_code until that wiring lands.
#![allow(dead_code)]

use crate::ability::{Ability, AbilityType, DamageClass, Element, StatusEffect};
use engine_core::color::Rgba;
use engine_render::dots::Dot;
use engine_render::{
    draw_dots, flex, label, ui_primitives, wrapped_text, Align, Basis, Direction, DotRect,
    FlexChild, FlexStyle, Justify, TextAlign,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

/// Tooltip card width (spec:26). Placeholder — tunable. Wide enough that the
/// full three-pill row (ability_type/element/class, each padded
/// `PILL_H_PAD_CELLS` cells a side) fits without the pill-row `flex` shrink
/// kicking in — shrinking a pill can floor its cell width down to exactly
/// its text width, leaving no un-overwritten margin for the tinted capsule
/// to show through.
pub(super) const TOOLTIP_WIDTH_CELLS: u16 = 36;
/// Interior vertical (top/bottom) `.inset` padding applied to the card's
/// content area (spec:26).
pub(super) const INTERIOR_PADDING_CELLS: u16 = 1;
/// Interior horizontal (left/right) padding — 2 cells so there's a blank
/// margin cell between the card frame and every content row (1 cell clears the
/// frame, the second is the visible margin).
pub(super) const INTERIOR_PADDING_H_CELLS: u16 = 2;
/// Corner radius (dots) for the pill capsule ends — 2 for visibly rounded
/// caps that close onto the over/underline (the fixed chamfer now lights the
/// diagonal connector dot, so radius 2 reads clean). Explicit deviation from
/// the chamfer-1 house default, per the pill design.
pub(super) const PILL_CORNER_RADIUS_DOTS: usize = 2;
/// Pill height — 3 cells: an overline row, the text row, and an underline
/// row. Braille border dots can't share a cell with the terminal-text label,
/// so the dot lines above/below the text live in the adjacent cells.
pub(super) const PILL_HEIGHT_CELLS: u16 = 3;
/// Gap between adjacent pills in the pill row.
pub(super) const INTER_PILL_GAP_CELLS: u16 = 1;
/// Pill capsule border ring thickness (dots).
pub(super) const PILL_BORDER_THICKNESS_DOTS: usize = 1;
/// Pill label text color — a legible fg over the tinted capsule (never
/// `Style::default()`'s unset `Color::Reset`; see `lib.rs::label`'s caveat).
pub(super) const PILL_TEXT_COLOR: Rgba = Rgba::rgb(0xff, 0xff, 0xff);

/// Maps an [`Element`] to its starter pill color. Exhaustive match — a
/// future variant fails to compile rather than silently falling through.
pub(super) fn element_color(element: Element) -> Rgba {
    match element {
        Element::Fire => Rgba::rgb(0xff, 0x8c, 0x00),
        Element::Water => Rgba::rgb(0x1e, 0x90, 0xff),
        Element::Earth => Rgba::rgb(0x2e, 0x8b, 0x57),
        Element::Lightning => Rgba::rgb(0xff, 0xd7, 0x00),
        Element::Normal => Rgba::rgb(0x9e, 0x9e, 0x9e),
    }
}

/// Maps an [`AbilityType`] to its starter pill color. Exhaustive match.
pub(super) fn ability_type_color(ability_type: AbilityType) -> Rgba {
    match ability_type {
        AbilityType::Attack => Rgba::rgb(0xd0, 0x30, 0x30),
        AbilityType::Buff => Rgba::rgb(0x3c, 0xb3, 0x71),
        AbilityType::Debuff => Rgba::rgb(0x8a, 0x2b, 0xe2),
    }
}

/// Maps a [`DamageClass`] to its starter pill color. Exhaustive match.
pub(super) fn class_color(class: DamageClass) -> Rgba {
    match class {
        DamageClass::Physical => Rgba::rgb(0xd2, 0xb4, 0x8c),
        DamageClass::Magic => Rgba::rgb(0x94, 0x00, 0xd3),
    }
}

/// A rounded capsule ("pill") drawn as a tinted OUTLINE hugging its centered
/// `text`: a 6-dot-tall ring centered in the 3-cell `rect` so its top edge is
/// the OVERLINE (bottom dot-row of the cell above the text) and its bottom edge
/// the UNDERLINE (top dot-row of the cell below), with radius-2 ends rounding
/// onto those rows. The interior is `Transparent` so the label stays legible.
/// `rect` is dot-precise; the label is placed at the floored cell rect (plain
/// terminal text is cell-quantized). A zero-area `rect` draws nothing and does
/// not panic.
pub(super) fn pill(buf: &mut Buffer, rect: DotRect, text: &str, color: Rgba) {
    let cr = rect.to_cell_rect();
    if cr.width == 0 || cr.height == 0 {
        return;
    }

    let w_dots = cr.width as i32 * 2;
    let box_h: i32 = 6;
    let dots = ui_primitives::rounded_rect(
        w_dots as usize,
        box_h as usize,
        PILL_BORDER_THICKNESS_DOTS,
        PILL_CORNER_RADIUS_DOTS,
        color,
        Dot::Transparent,
    );
    crate::scenes::post_battle::columns::blit_dots(
        buf,
        DotRect { x: cr.x as i32 * 2, y: cr.y as i32 * 4 + 3, w: w_dots, h: box_h },
        &dots,
    );

    label(
        buf,
        cr,
        text,
        TextAlign::Center,
        Style::default().fg(Color::Rgb(
            PILL_TEXT_COLOR.r,
            PILL_TEXT_COLOR.g,
            PILL_TEXT_COLOR.b,
        )),
    );
}

/// Max flavor-text lines the card reserves (spec:33 clips flavor to 2 rows).
pub(super) const FLAVOR_MAX_LINES: u16 = 2;
/// Spacer height (cells) inserted only before a present Flavor row, and only
/// when at least one row precedes it.
pub(super) const PRE_FLAVOR_GAP_CELLS: u16 = 1;
/// Whole-cell gap the card is anchored off the hovered ability cell's
/// top-left corner (card's bottom-right sits `ANCHOR_GAP_CELLS` cells
/// above-left, no clamp). 0 = the card sits right against the hover point.
pub(super) const ANCHOR_GAP_CELLS: u16 = 0;

/// Card frame border color — amber, matches the `BattleMenu` overlay.
pub(super) const CARD_BORDER_COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00);
/// Card frame border ring thickness (dots).
pub(super) const CARD_BORDER_THICKNESS_DOTS: usize = 1;
/// Card frame corner radius (dots) — chamfer 1, the house default (see the
/// chamfer-1 style memory).
pub(super) const CARD_CORNER_RADIUS_DOTS: usize = 1;
/// Card body text color — legible fg over the `Occlude`-filled interior
/// (matches `details_panel`'s white). Used for field *values*.
pub(super) const CARD_TEXT_COLOR: Rgba = Rgba::rgb(0xff, 0xff, 0xff);
/// Field *label* color (`Cost:`/`Damage:`/`Range:`/`Status:`) — a muted steel
/// blue, stylistically distinct from the white values beside them.
pub(super) const FIELD_LABEL_COLOR: Rgba = Rgba::rgb(0x8f, 0xa8, 0xc8);
/// Flavor-text color — gray, de-emphasized under the field rows.
pub(super) const FLAVOR_TEXT_COLOR: Rgba = Rgba::rgb(0x9e, 0x9e, 0x9e);
/// Blank spacer (cells) inserted before the Pills row when a row precedes it,
/// separating the pills from the Cost line above.
pub(super) const PRE_PILLS_GAP_CELLS: u16 = 1;
/// Horizontal padding (cells) each side of a pill label, inside the pill.
/// 2 cells (not 1) so at least one dot column of the tinted border/fill
/// clears `PILL_CORNER_RADIUS_DOTS`'s chamfer and survives outside the
/// centered text glyphs (the leftmost/rightmost pill cell is fully
/// chamfered on every row when the radius is 3 dots — see `pill_width_dots`).
pub(super) const PILL_H_PAD_CELLS: u16 = 2;
/// Minimum pill width (cells) so a shrunk pill never fully chamfers away.
pub(super) const MIN_PILL_WIDTH_CELLS: u16 = 4;

/// Which content rows a tooltip card may show, in fixed top-to-bottom order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TooltipRow {
    Cost,
    Pills,
    DamageRange,
    Status,
    Flavor,
}

/// The resolved geometry of one tooltip card: the outer frame rect, and the
/// present rows' rects in top-to-bottom order (absolute dot coords).
pub(super) struct TooltipLayout {
    pub card: DotRect,
    pub rows: Vec<(TooltipRow, DotRect)>,
}

/// The present-row set for `ability`, top-to-bottom, per the omit-if-absent
/// rules (spec:26-33): Cost iff `cost.is_some()`; Pills iff any of
/// ability_type/element/class is `Some`; DamageRange iff damage or range is
/// `Some`; Status iff `status_effects` is non-empty; Flavor iff
/// `flavor.is_some()`. An all-absent ability returns an empty vec.
pub(super) fn present_rows(ability: &Ability) -> Vec<TooltipRow> {
    let mut rows = Vec::with_capacity(5);
    if ability.cost().is_some() {
        rows.push(TooltipRow::Cost);
    }
    if ability.ability_type().is_some() || ability.element().is_some() || ability.class().is_some()
    {
        rows.push(TooltipRow::Pills);
    }
    if ability.damage().is_some() || ability.range().is_some() {
        rows.push(TooltipRow::DamageRange);
    }
    if !ability.status_effects().is_empty() {
        rows.push(TooltipRow::Status);
    }
    if ability.flavor().is_some() {
        rows.push(TooltipRow::Flavor);
    }
    rows
}

/// Computes the tooltip card's outer `DotRect` (fixed width, content-driven
/// height) and each present row's rect, laid out via `engine_render::flex`
/// (Column) over a 1-cell-inset interior, anchored so the card's
/// bottom-right corner sits `ANCHOR_GAP_CELLS` cells above-left of
/// `hovered_cell`'s top-left corner — no clamp. A single `Fixed` spacer
/// child of `PRE_FLAVOR_GAP_CELLS*4` dots is inserted before Flavor when
/// Flavor is present and at least one row precedes it; the spacer never
/// appears in the returned `rows`.
/// Fixed content height (cells) for one present row. Exhaustive match — a
/// future `TooltipRow` variant fails to compile rather than defaulting to a
/// wrong height.
fn row_height_cells(row: TooltipRow, ability: &Ability) -> u16 {
    match row {
        TooltipRow::Cost => 1,
        TooltipRow::Pills => PILL_HEIGHT_CELLS,
        TooltipRow::DamageRange => 1,
        TooltipRow::Status => 1 + (ability.status_effects().len() as u16).div_ceil(2),
        TooltipRow::Flavor => FLAVOR_MAX_LINES,
    }
}

pub(super) fn layout_tooltip(ability: &Ability, hovered_cell: DotRect) -> TooltipLayout {
    let rows = present_rows(ability);
    // Flavor is always the last present row (fixed order); the gap only
    // applies when at least one row precedes it.
    let pre_flavor_gap = rows.len() > 1 && matches!(rows.last(), Some(TooltipRow::Flavor));
    // A blank row before Pills when a row (Cost) precedes it, separating the
    // pills from the Cost line above.
    let pre_pills_gap = rows
        .iter()
        .position(|&r| r == TooltipRow::Pills)
        .is_some_and(|i| i > 0);

    let content_dots: i32 = rows.iter().map(|&r| row_height_cells(r, ability) as i32 * 4).sum::<i32>()
        + if pre_flavor_gap { PRE_FLAVOR_GAP_CELLS as i32 * 4 } else { 0 }
        + if pre_pills_gap { PRE_PILLS_GAP_CELLS as i32 * 4 } else { 0 };

    let card_w = TOOLTIP_WIDTH_CELLS as i32 * 2;
    let card_h = content_dots + 2 * INTERIOR_PADDING_CELLS as i32 * 4;
    let dx = ANCHOR_GAP_CELLS as i32 * 2;
    let dy = ANCHOR_GAP_CELLS as i32 * 4;

    let card = DotRect {
        x: hovered_cell.x - dx - card_w,
        y: hovered_cell.y - dy - card_h,
        w: card_w,
        h: card_h,
    };

    let interior = card.inset(
        INTERIOR_PADDING_H_CELLS as i32 * 2,
        INTERIOR_PADDING_H_CELLS as i32 * 2,
        INTERIOR_PADDING_CELLS as i32 * 4,
        INTERIOR_PADDING_CELLS as i32 * 4,
    );

    // A parallel `Option<TooltipRow>` marker per flex child — `None` for the
    // pre-flavor spacer — so the flex result can be zipped back and the
    // spacer dropped without ever appearing in the returned `rows`.
    let mut markers: Vec<Option<TooltipRow>> = Vec::with_capacity(rows.len() + 1);
    let mut children: Vec<FlexChild> = Vec::with_capacity(rows.len() + 1);
    for &row in &rows {
        if pre_pills_gap && row == TooltipRow::Pills {
            children.push(FlexChild {
                basis: Basis::Fixed(PRE_PILLS_GAP_CELLS as i32 * 4),
                grow: 0.0,
                shrink: 0.0,
            });
            markers.push(None);
        }
        if pre_flavor_gap && row == TooltipRow::Flavor {
            children.push(FlexChild {
                basis: Basis::Fixed(PRE_FLAVOR_GAP_CELLS as i32 * 4),
                grow: 0.0,
                shrink: 0.0,
            });
            markers.push(None);
        }
        children.push(FlexChild {
            basis: Basis::Fixed(row_height_cells(row, ability) as i32 * 4),
            grow: 0.0,
            shrink: 0.0,
        });
        markers.push(Some(row));
    }

    let style = FlexStyle {
        direction: Direction::Column,
        justify_content: Justify::Start,
        align_items: Align::Start,
        gap: 0,
    };
    let rects = flex(interior, style, &children);

    let rows_out = markers
        .into_iter()
        .zip(rects)
        .filter_map(|(marker, rect)| marker.map(|row| (row, rect)))
        .collect();

    TooltipLayout { card, rows: rows_out }
}

/// Draws the whole tooltip overlay — chamfered `Occlude` frame plus every
/// present row's content — into `buf`, anchored per [`layout_tooltip`] off
/// `hovered_cell`. A zero-area card (all-absent ability) draws nothing and
/// does not panic.
pub(super) fn render_tooltip(buf: &mut Buffer, ability: &Ability, hovered_cell: DotRect) {
    let layout = layout_tooltip(ability, hovered_cell);
    let card_cell_rect = layout.card.to_cell_rect();
    if card_cell_rect.width == 0 || card_cell_rect.height == 0 {
        return;
    }

    let frame = ui_primitives::rounded_rect(
        layout.card.w as usize,
        layout.card.h as usize,
        CARD_BORDER_THICKNESS_DOTS,
        CARD_CORNER_RADIUS_DOTS,
        CARD_BORDER_COLOR,
        Dot::Occlude,
    );
    draw_dots(buf, card_cell_rect, &frame);

    for &(row, rect) in &layout.rows {
        match row {
            TooltipRow::Cost => {
                if let Some(cost) = ability.cost() {
                    fill_cost(buf, rect, cost);
                }
            }
            TooltipRow::Pills => fill_pills(buf, rect, ability),
            TooltipRow::DamageRange => fill_damage_range(buf, rect, ability),
            TooltipRow::Status => fill_status(buf, rect, ability.status_effects()),
            TooltipRow::Flavor => {
                if let Some(flavor) = ability.flavor() {
                    fill_flavor(buf, rect, flavor);
                }
            }
        }
    }
}

/// Centered `"Cost: {n} stamina"` — label muted, value white.
fn fill_cost(buf: &mut Buffer, rect: DotRect, cost: u8) {
    draw_labeled_value(
        buf,
        rect.to_cell_rect(),
        "Cost:",
        &format!("{cost} stamina"),
        TextAlign::Center,
    );
}

/// A centered Row of value-colored pill capsules — one per present field, in
/// (ability_type, element, class) order.
fn fill_pills(buf: &mut Buffer, rect: DotRect, ability: &Ability) {
    let mut pills: Vec<(&'static str, Rgba)> = Vec::with_capacity(3);
    if let Some(ability_type) = ability.ability_type() {
        pills.push((ability_type.label(), ability_type_color(ability_type)));
    }
    if let Some(element) = ability.element() {
        pills.push((element.label(), element_color(element)));
    }
    if let Some(class) = ability.class() {
        pills.push((class.label(), class_color(class)));
    }
    if pills.is_empty() {
        return;
    }

    let children: Vec<FlexChild> = pills
        .iter()
        .map(|(label, _)| FlexChild {
            basis: Basis::Fixed(pill_width_dots(label)),
            grow: 0.0,
            shrink: 1.0,
        })
        .collect();
    let style = FlexStyle {
        direction: Direction::Row,
        justify_content: Justify::Center,
        align_items: Align::Stretch,
        gap: INTER_PILL_GAP_CELLS as i32 * 2,
    };
    let rects = flex(rect, style, &children);

    for ((pill_label, color), pill_rect) in pills.into_iter().zip(rects) {
        pill(buf, pill_rect, pill_label, color);
    }
}

/// Two equal columns: `"Damage: {n}"` left, `"Range: {n}"` right, each
/// omitted (blank) when its field is `None`.
fn fill_damage_range(buf: &mut Buffer, rect: DotRect, ability: &Ability) {
    let children = vec![
        FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
        FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
    ];
    let style = FlexStyle {
        direction: Direction::Row,
        justify_content: Justify::Start,
        align_items: Align::Stretch,
        gap: 0,
    };
    let cols = flex(rect, style, &children);

    if let Some(damage) = ability.damage() {
        draw_labeled_value(buf, cols[0].to_cell_rect(), "Damage:", &damage.to_string(), TextAlign::Left);
    }
    if let Some(range) = ability.range() {
        draw_labeled_value(buf, cols[1].to_cell_rect(), "Range:", &range.to_string(), TextAlign::Left);
    }
}

/// `"Status:"` header plus effect names in a 2-column grid
/// (effect0|effect1 / effect2|effect3 / ...).
fn fill_status(buf: &mut Buffer, rect: DotRect, effects: &[StatusEffect]) {
    let body_rows = (effects.len() as u16).div_ceil(2).max(1) as i32;
    let sections = flex(
        rect,
        FlexStyle {
            direction: Direction::Column,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        },
        &[
            FlexChild { basis: Basis::Fixed(4), grow: 0.0, shrink: 0.0 },
            FlexChild { basis: Basis::Fixed(body_rows * 4), grow: 0.0, shrink: 0.0 },
        ],
    );
    let (header_rect, body_rect) = (sections[0], sections[1]);

    draw_labeled_value(buf, header_rect.to_cell_rect(), "Status:", "", TextAlign::Left);

    let row_children: Vec<FlexChild> = (0..body_rows)
        .map(|_| FlexChild { basis: Basis::Fixed(4), grow: 0.0, shrink: 0.0 })
        .collect();
    let body_rows_rects = flex(
        body_rect,
        FlexStyle {
            direction: Direction::Column,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: 0,
        },
        &row_children,
    );

    for (i, row_rect) in body_rows_rects.into_iter().enumerate() {
        let cols = flex(
            row_rect,
            FlexStyle {
                direction: Direction::Row,
                justify_content: Justify::Start,
                align_items: Align::Stretch,
                gap: 0,
            },
            &[
                FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
                FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
            ],
        );
        if let Some(effect) = effects.get(2 * i) {
            label(buf, cols[0].to_cell_rect(), &effect.name, TextAlign::Left, text_style());
        }
        if let Some(effect) = effects.get(2 * i + 1) {
            label(buf, cols[1].to_cell_rect(), &effect.name, TextAlign::Left, text_style());
        }
    }
}

/// Word-wrapped flavor text (gray), clipped to `FLAVOR_MAX_LINES` rows with a
/// tail ellipsis.
fn fill_flavor(buf: &mut Buffer, rect: DotRect, text: &str) {
    wrapped_text(buf, rect.to_cell_rect(), text, TextAlign::Left, flavor_style(), true);
}

/// White fg for field *values* over the `Occlude`-filled card interior —
/// never `Style::default()`'s unset `Color::Reset`.
fn text_style() -> Style {
    Style::default().fg(Color::Rgb(CARD_TEXT_COLOR.r, CARD_TEXT_COLOR.g, CARD_TEXT_COLOR.b))
}

/// Muted style for field *labels* (`Cost:`/`Damage:`/`Range:`/`Status:`).
fn field_label_style() -> Style {
    Style::default().fg(Color::Rgb(FIELD_LABEL_COLOR.r, FIELD_LABEL_COLOR.g, FIELD_LABEL_COLOR.b))
}

/// Gray style for flavor text.
fn flavor_style() -> Style {
    Style::default().fg(Color::Rgb(FLAVOR_TEXT_COLOR.r, FLAVOR_TEXT_COLOR.g, FLAVOR_TEXT_COLOR.b))
}

/// Draws `"{label} {value}"` with the label in [`field_label_style`] and the
/// value in the white [`text_style`], so the two read distinctly. `align`
/// centers or left-aligns the combined run within `cell_rect`; an empty
/// `value` draws just the label.
fn draw_labeled_value(
    buf: &mut Buffer,
    cell_rect: Rect,
    label_text: &str,
    value_text: &str,
    align: TextAlign,
) {
    let label_w = label_text.chars().count() as u16;
    let value_w = value_text.chars().count() as u16;
    let combined = label_w + if value_w > 0 { 1 + value_w } else { 0 };
    let start_x = match align {
        TextAlign::Center => cell_rect.x + cell_rect.width.saturating_sub(combined) / 2,
        _ => cell_rect.x,
    };
    label(
        buf,
        Rect { x: start_x, y: cell_rect.y, width: label_w, height: cell_rect.height },
        label_text,
        TextAlign::Left,
        field_label_style(),
    );
    if value_w > 0 {
        // Draw the value with a leading ASCII space (rather than relying on the
        // untouched gap cell, which is a braille blank, not " ") so the label
        // and value read as one string when the buffer is decoded.
        label(
            buf,
            Rect {
                x: start_x + label_w,
                y: cell_rect.y,
                width: value_w + 1,
                height: cell_rect.height,
            },
            &format!(" {value_text}"),
            TextAlign::Left,
            text_style(),
        );
    }
}

/// A pill's fixed main-axis width in dots: label chars + padding each side,
/// floored at `MIN_PILL_WIDTH_CELLS` so a shrunk pill never fully chamfers
/// away.
fn pill_width_dots(label: &str) -> i32 {
    ((label.chars().count() as u16 + PILL_H_PAD_CELLS * 2).max(MIN_PILL_WIDTH_CELLS) as i32) * 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::StatusEffect;
    use crate::scenes::test_util::{lit_dot_color, rect_text};
    use ratatui::layout::Rect;

    #[test]
    fn element_color_maps_fire_to_orange() {
        assert_eq!(element_color(Element::Fire), Rgba::rgb(0xff, 0x8c, 0x00));
    }

    #[test]
    fn element_color_maps_water_to_blue() {
        assert_eq!(element_color(Element::Water), Rgba::rgb(0x1e, 0x90, 0xff));
    }

    #[test]
    fn element_color_maps_earth_to_green() {
        assert_eq!(element_color(Element::Earth), Rgba::rgb(0x2e, 0x8b, 0x57));
    }

    #[test]
    fn element_color_maps_lightning_to_yellow() {
        assert_eq!(element_color(Element::Lightning), Rgba::rgb(0xff, 0xd7, 0x00));
    }

    #[test]
    fn element_color_maps_normal_to_grey() {
        assert_eq!(element_color(Element::Normal), Rgba::rgb(0x9e, 0x9e, 0x9e));
    }

    #[test]
    fn ability_type_color_maps_attack_to_red() {
        assert_eq!(
            ability_type_color(AbilityType::Attack),
            Rgba::rgb(0xd0, 0x30, 0x30)
        );
    }

    #[test]
    fn ability_type_color_maps_buff_to_green() {
        assert_eq!(
            ability_type_color(AbilityType::Buff),
            Rgba::rgb(0x3c, 0xb3, 0x71)
        );
    }

    #[test]
    fn ability_type_color_maps_debuff_to_purple() {
        assert_eq!(
            ability_type_color(AbilityType::Debuff),
            Rgba::rgb(0x8a, 0x2b, 0xe2)
        );
    }

    #[test]
    fn class_color_maps_physical_to_tan() {
        assert_eq!(class_color(DamageClass::Physical), Rgba::rgb(0xd2, 0xb4, 0x8c));
    }

    #[test]
    fn class_color_maps_magic_to_violet() {
        assert_eq!(class_color(DamageClass::Magic), Rgba::rgb(0x94, 0x00, 0xd3));
    }

    #[test]
    fn all_palette_colors_are_opaque() {
        assert_eq!(element_color(Element::Fire).a, 0xFF);
        assert_eq!(ability_type_color(AbilityType::Attack).a, 0xFF);
        assert_eq!(class_color(DamageClass::Physical).a, 0xFF);
    }

    // ---------------------------------------------------------------- pill

    /// The capsule is a tinted OUTLINE: the overline (top edge, dot row 3) and
    /// underline (bottom edge, dot row 8) decode to `color`, and the interior
    /// (dot row 5) is transparent — not a filled chip. Empty `text` so no glyph
    /// overwrites the sampled dots. Pill is 3 cells tall (6-cell × 3-cell rect).
    #[test]
    fn pill_outline_over_underline_lit_interior_transparent() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        let rect = DotRect { x: 0, y: 0, w: 12, h: 12 };
        let color = Rgba::rgb(0x11, 0x22, 0x33);

        pill(&mut buf, rect, "", color);

        assert_eq!(lit_dot_color(&buf, 5, 3), Some(color), "overline (top edge) dot must be lit");
        assert_eq!(lit_dot_color(&buf, 5, 8), Some(color), "underline (bottom edge) dot must be lit");
        assert_eq!(lit_dot_color(&buf, 5, 5), None, "interior must be transparent (outline, not filled)");
    }

    /// The capsule's outermost corner dot is chamfered (transparent/unlit)
    /// while a mid-edge dot on the same overline row is lit — proving rounded,
    /// not square, caps.
    #[test]
    fn pill_chamfered_corner_is_unlit_while_mid_edge_is_lit() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        let rect = DotRect { x: 0, y: 0, w: 12, h: 12 };
        let color = Rgba::rgb(0x11, 0x22, 0x33);

        pill(&mut buf, rect, "", color);

        assert_eq!(
            lit_dot_color(&buf, 0, 3),
            None,
            "outermost corner dot must be chamfered (unlit)"
        );
        assert_eq!(
            lit_dot_color(&buf, 5, 3),
            Some(color),
            "mid-edge dot on the same overline row must be lit"
        );
    }

    /// `text` is drawn centered inside the capsule's cell rect.
    #[test]
    fn pill_label_centered() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        let rect = DotRect { x: 0, y: 0, w: 20, h: 4 };
        let color = Rgba::rgb(0x11, 0x22, 0x33);

        pill(&mut buf, rect, "Hi", color);

        assert_eq!(buf.cell((4, 0)).unwrap().symbol(), "H");
        assert_eq!(buf.cell((5, 0)).unwrap().symbol(), "i");
    }

    /// A zero-area `rect` draws nothing and does not panic.
    #[test]
    fn pill_zero_area_rect_draws_nothing_and_does_not_panic() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        let rect = DotRect { x: 0, y: 0, w: 0, h: 0 };
        let color = Rgba::rgb(0x11, 0x22, 0x33);

        pill(&mut buf, rect, "text", color);

        for x in 0..4 {
            assert_eq!(
                buf.cell((x, 0)).unwrap().symbol(),
                " ",
                "cell ({x},0) must remain blank"
            );
        }
    }

    // ------------------------------------------------ present_rows / layout

    /// Every field set — all five rows present.
    fn full_ability(status_effect_count: usize) -> Ability {
        Ability::new("Roar", vec![])
            .with_ability_type(AbilityType::Attack)
            .with_element(Element::Fire)
            .with_class(DamageClass::Physical)
            .with_cost(3)
            .with_damage(10)
            .with_range(5)
            .with_status_effects(
                (0..status_effect_count)
                    .map(|i| StatusEffect { name: format!("Effect{i}") })
                    .collect(),
            )
            .with_flavor("A crackling roar.")
    }

    /// Cost/range/flavor `None`, `status_effects` empty — only Pills (via
    /// `ability_type`) and DamageRange (via `damage`) remain present.
    fn sparse_ability() -> Ability {
        Ability::new("Roar", vec![]).with_ability_type(AbilityType::Attack).with_damage(10)
    }

    /// No optional field set at all — every row absent.
    fn empty_ability() -> Ability {
        Ability::new("Roar", vec![])
    }

    #[test]
    fn present_rows_full_field_returns_all_five_in_order() {
        assert_eq!(
            present_rows(&full_ability(1)),
            vec![
                TooltipRow::Cost,
                TooltipRow::Pills,
                TooltipRow::DamageRange,
                TooltipRow::Status,
                TooltipRow::Flavor,
            ]
        );
    }

    #[test]
    fn present_rows_omits_absent_fields() {
        assert_eq!(
            present_rows(&sparse_ability()),
            vec![TooltipRow::Pills, TooltipRow::DamageRange]
        );
    }

    #[test]
    fn present_rows_all_absent_ability_returns_empty() {
        assert!(present_rows(&empty_ability()).is_empty());
    }

    fn hovered_cell() -> DotRect {
        // Well away from the origin so a broken anchor (e.g. saturating at
        // 0) can't accidentally satisfy the assertion.
        DotRect { x: 200, y: 120, w: 20, h: 16 }
    }

    #[test]
    fn layout_card_width_is_fixed_and_height_sums_present_rows() {
        let layout = layout_tooltip(&full_ability(1), hovered_cell());

        assert_eq!(layout.card.w, TOOLTIP_WIDTH_CELLS as i32 * 2);

        // Cost(1) + pre-pills gap + Pills(PILL_HEIGHT_CELLS) + DamageRange(1) +
        // Status(1 + ceil(1/2)) + pre-flavor gap + Flavor(FLAVOR_MAX_LINES),
        // all in cells, plus 2x interior padding.
        let content_cells = 1
            + PRE_PILLS_GAP_CELLS as i32
            + PILL_HEIGHT_CELLS as i32
            + 1
            + (1 + 1)
            + PRE_FLAVOR_GAP_CELLS as i32
            + FLAVOR_MAX_LINES as i32;
        let expected_h = content_cells * 4 + 2 * INTERIOR_PADDING_CELLS as i32 * 4;
        assert_eq!(layout.card.h, expected_h);
    }

    #[test]
    fn layout_fewer_fields_yields_shorter_card() {
        let full = layout_tooltip(&full_ability(1), hovered_cell());
        let sparse = layout_tooltip(&sparse_ability(), hovered_cell());

        assert!(sparse.rows.len() < full.rows.len());
        assert!(
            sparse.card.h < full.card.h,
            "sparse card.h={} must be strictly shorter than full card.h={}",
            sparse.card.h,
            full.card.h
        );
    }

    #[test]
    fn layout_anchor_places_card_bottom_right_above_left_of_hovered_cell() {
        let hovered = hovered_cell();
        let layout = layout_tooltip(&full_ability(1), hovered);

        assert_eq!(
            layout.card.x + layout.card.w,
            hovered.x - ANCHOR_GAP_CELLS as i32 * 2,
            "card's right edge must sit ANCHOR_GAP_CELLS cells left of hovered's left edge"
        );
        assert_eq!(
            layout.card.y + layout.card.h,
            hovered.y - ANCHOR_GAP_CELLS as i32 * 4,
            "card's bottom edge must sit ANCHOR_GAP_CELLS cells above hovered's top edge"
        );
    }

    #[test]
    fn layout_rows_stack_without_overlap_and_flavor_gap_present() {
        let layout = layout_tooltip(&full_ability(1), hovered_cell());
        assert_eq!(layout.rows.len(), 5);

        for pair in layout.rows.windows(2) {
            let (prev_row, prev_rect) = pair[0];
            let (next_row, next_rect) = pair[1];
            let gap = next_rect.y - (prev_rect.y + prev_rect.h);
            if prev_row == TooltipRow::Status && next_row == TooltipRow::Flavor {
                assert_eq!(
                    gap,
                    PRE_FLAVOR_GAP_CELLS as i32 * 4,
                    "the single pre-flavor spacer must separate Status and Flavor"
                );
            } else if prev_row == TooltipRow::Cost && next_row == TooltipRow::Pills {
                assert_eq!(
                    gap,
                    PRE_PILLS_GAP_CELLS as i32 * 4,
                    "the single pre-pills spacer must separate Cost and Pills"
                );
            } else {
                assert_eq!(
                    gap, 0,
                    "{prev_row:?} -> {next_row:?} must be flush (no gap)"
                );
            }
        }
    }

    #[test]
    fn layout_status_height_scales_with_effect_count() {
        // Only Status present in both, so `rows[0]` is the Status rect.
        let one = Ability::new("Roar", vec![])
            .with_status_effects(vec![StatusEffect { name: "A".into() }]);
        let three = Ability::new("Roar", vec![]).with_status_effects(vec![
            StatusEffect { name: "A".into() },
            StatusEffect { name: "B".into() },
            StatusEffect { name: "C".into() },
        ]);

        let layout_one = layout_tooltip(&one, hovered_cell());
        let layout_three = layout_tooltip(&three, hovered_cell());

        assert_eq!(layout_one.rows.len(), 1);
        assert_eq!(layout_three.rows.len(), 1);
        // 1 effect -> 1 + ceil(1/2) = 2 cells = 8 dots.
        assert_eq!(layout_one.rows[0].1.h, 2 * 4);
        // 3 effects -> 1 + ceil(3/2) = 3 cells = 12 dots.
        assert_eq!(layout_three.rows[0].1.h, 3 * 4);
        assert!(layout_three.rows[0].1.h > layout_one.rows[0].1.h);
    }

    #[test]
    fn layout_all_absent_ability_no_panic_empty_rows() {
        let layout = layout_tooltip(&empty_ability(), hovered_cell());

        assert!(layout.rows.is_empty());
        assert_eq!(layout.card.w, TOOLTIP_WIDTH_CELLS as i32 * 2);
        // No content, no gap — just the doubled interior padding.
        assert_eq!(layout.card.h, 2 * INTERIOR_PADDING_CELLS as i32 * 4);
    }

    // ---------------------------------------------------------- render_tooltip

    /// An ability with only `range` set (no `damage`) — used by the
    /// Damage/Range column-omission test.
    fn range_only_ability() -> Ability {
        Ability::new("Roar", vec![]).with_range(7)
    }

    /// A buffer large enough to hold the anchored card for `hovered_cell()`
    /// (card lands roughly at cell cols 71..99, rows 19..29) with margin.
    fn tooltip_test_buf() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 110, 32))
    }

    /// Every present row's content is drawn somewhere inside the card, and
    /// `layout_tooltip` still reports all five rows present.
    #[test]
    fn render_tooltip_full_field_ability_draws_every_row_content() {
        let ability = full_ability(1);
        let mut buf = tooltip_test_buf();

        render_tooltip(&mut buf, &ability, hovered_cell());

        let layout = layout_tooltip(&ability, hovered_cell());
        assert_eq!(layout.rows.len(), 5);

        let card_text = rect_text(&buf, layout.card.to_cell_rect());
        for expected in [
            "Cost:",
            "Damage:",
            "Range:",
            "Status:",
            "A crackling roar.",
            "Attack",
            "Fire",
            "Physical",
        ] {
            assert!(
                card_text.contains(expected),
                "card text must contain {expected:?}; got {card_text:?}"
            );
        }
    }

    /// The card `Occlude`-fills its footprint: a cell that had unrelated
    /// content before the call no longer shows that content afterward.
    #[test]
    fn render_tooltip_occludes_content_beneath_the_card() {
        let ability = full_ability(1);
        let layout = layout_tooltip(&ability, hovered_cell());
        let card_rect = layout.card.to_cell_rect();
        let probe = (card_rect.x + card_rect.width / 2, card_rect.y + card_rect.height / 2);

        let mut buf = tooltip_test_buf();
        if let Some(cell) = buf.cell_mut(probe) {
            cell.set_char('Q');
        }

        render_tooltip(&mut buf, &ability, hovered_cell());

        assert_ne!(
            buf.cell(probe).unwrap().symbol(),
            "Q",
            "cell under the card must no longer show pre-existing content"
        );
    }

    /// A Range-only ability's Damage/Range row shows `"Range:"` in the right
    /// half and no `"Damage:"` anywhere in the row.
    #[test]
    fn render_tooltip_range_only_shows_right_column_only() {
        let ability = range_only_ability();
        let mut buf = tooltip_test_buf();

        render_tooltip(&mut buf, &ability, hovered_cell());

        let layout = layout_tooltip(&ability, hovered_cell());
        let (_, row_rect) = layout
            .rows
            .iter()
            .find(|(row, _)| *row == TooltipRow::DamageRange)
            .expect("DamageRange row must be present for a range-only ability");
        let row_cell_rect = row_rect.to_cell_rect();

        let left_half = Rect {
            x: row_cell_rect.x,
            y: row_cell_rect.y,
            width: row_cell_rect.width / 2,
            height: row_cell_rect.height,
        };
        let right_half = Rect {
            x: row_cell_rect.x + row_cell_rect.width / 2,
            y: row_cell_rect.y,
            width: row_cell_rect.width - row_cell_rect.width / 2,
            height: row_cell_rect.height,
        };

        assert!(
            !rect_text(&buf, left_half).contains("Damage"),
            "left column must not show Damage when damage is None"
        );
        assert!(
            rect_text(&buf, right_half).contains("Range:"),
            "right column must show Range:"
        );
    }

    /// The Fire element's pill renders with a lit dot decoding to
    /// `element_color(Element::Fire)` (orange) somewhere in the Pills row.
    #[test]
    fn render_tooltip_fire_element_pill_decodes_to_orange() {
        let ability = full_ability(1);
        let mut buf = tooltip_test_buf();

        render_tooltip(&mut buf, &ability, hovered_cell());

        let layout = layout_tooltip(&ability, hovered_cell());
        let (_, pills_rect) = layout
            .rows
            .iter()
            .find(|(row, _)| *row == TooltipRow::Pills)
            .expect("Pills row must be present");

        let orange = element_color(Element::Fire);
        let found = (pills_rect.x..pills_rect.x + pills_rect.w)
            .any(|dot_col| {
                (pills_rect.y..pills_rect.y + pills_rect.h)
                    .any(|dot_row| lit_dot_color(&buf, dot_col, dot_row) == Some(orange))
            });

        assert!(found, "Pills row must contain a lit dot decoding to Fire's orange");
    }

    /// An all-absent ability draws only the padding-only frame and must not
    /// panic.
    #[test]
    fn render_tooltip_all_absent_ability_does_not_panic() {
        let mut buf = tooltip_test_buf();
        render_tooltip(&mut buf, &empty_ability(), hovered_cell());
    }
}
