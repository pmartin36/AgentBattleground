//! Ability hover tooltip (spec 49). Composition root: owns the card content
//! ([`layout_tooltip`]/[`present_rows`]) and the overlay render
//! ([`render_tooltip`] + the per-row fillers), and pulls the pill capsule
//! ([`pills`]), its color palette ([`palette`]), and the card chrome
//! (`crate::scenes::tooltip`: anchor math, edge clamping, frame) from
//! sibling/shared modules.
#![allow(dead_code)]

mod palette;
mod pills;

use crate::ability::{Ability, StatusKind};
use crate::scenes::tooltip::{self, RowSpec};
use engine_core::color::Rgba;
use engine_render::{flex, label, wrapped_text, Align, Basis, Direction, DotRect, FlexChild, FlexStyle, Justify, TextAlign};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use palette::{ability_type_color, class_color, element_color};
use pills::{pill, pill_width_dots, INTER_PILL_GAP_CELLS, PILL_HEIGHT_CELLS};

/// Tooltip card width (spec:26). Placeholder — tunable. Wide enough that the
/// full three-pill row (ability_type/element/class, each padded
/// `PILL_H_PAD_CELLS` cells a side) fits without the pill-row `flex` shrink
/// kicking in — shrinking a pill can floor its cell width down to exactly
/// its text width, leaving no un-overwritten margin for the tinted capsule
/// to show through.
pub(super) const TOOLTIP_WIDTH_CELLS: u16 = 36;

/// Max flavor-text lines the card reserves (spec:33 clips flavor to 2 rows).
pub(super) const FLAVOR_MAX_LINES: u16 = 2;
/// Spacer height (cells) inserted only before a present Flavor row, and only
/// when at least one row precedes it.
pub(super) const PRE_FLAVOR_GAP_CELLS: u16 = 1;

/// Card body text color — legible fg over the `Occlude`-filled interior
/// (matches `details_panel`'s white). Used for field *values*.
pub(super) const CARD_TEXT_COLOR: Rgba = Rgba::rgb(0xff, 0xff, 0xff);
/// Field *label* color (`Cost:`/`Damage:`/`Range:`/`Status:`) — a muted steel
/// blue, stylistically distinct from the white values beside them.
pub(super) const FIELD_LABEL_COLOR: Rgba = Rgba::rgb(0x8f, 0xa8, 0xc8);
/// Flavor-text color — gray, de-emphasized under the field rows.
pub(super) const FLAVOR_TEXT_COLOR: Rgba = Rgba::rgb(0x9e, 0x9e, 0x9e);

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

/// Computes the tooltip card's outer `DotRect` (fixed width, content-driven
/// height) and each present row's rect via [`tooltip::layout`], anchored so
/// the card's bottom-right corner sits `ANCHOR_GAP_CELLS` cells above-left of
/// `hovered_cell`'s top-left corner, clamped to the screen origin. A
/// `gap_above_cells` of `PRE_FLAVOR_GAP_CELLS` is attached to Flavor when
/// Flavor is present and at least one row precedes it; the gap never appears
/// in the returned `rows`.
pub(super) fn layout_tooltip(ability: &Ability, hovered_cell: DotRect) -> TooltipLayout {
    let rows = present_rows(ability);
    // Flavor is always the last present row (fixed order); the gap only
    // applies when at least one row precedes it.
    let pre_flavor_gap = rows.len() > 1 && matches!(rows.last(), Some(TooltipRow::Flavor));

    let row_specs: Vec<RowSpec> = rows
        .iter()
        .map(|&row| RowSpec {
            height_cells: row_height_cells(row, ability),
            gap_above_cells: if pre_flavor_gap && row == TooltipRow::Flavor {
                PRE_FLAVOR_GAP_CELLS
            } else {
                0
            },
        })
        .collect();

    let layout = tooltip::layout(hovered_cell, &row_specs, TOOLTIP_WIDTH_CELLS);

    let rows_out = rows.into_iter().zip(layout.rows).collect();

    TooltipLayout { card: layout.card, rows: rows_out }
}

/// Draws the whole tooltip overlay — chamfered `Occlude` frame plus every
/// present row's content — into `buf`, anchored per [`layout_tooltip`] off
/// `hovered_cell`. A zero-area card (all-absent ability) draws nothing and
/// does not panic.
pub(super) fn render_tooltip(buf: &mut Buffer, ability: &Ability, hovered_cell: DotRect) {
    let layout = layout_tooltip(ability, hovered_cell);
    if !tooltip::draw_frame(buf, layout.card) {
        return;
    }

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
fn fill_status(buf: &mut Buffer, rect: DotRect, effects: &[StatusKind]) {
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
            label(buf, cols[0].to_cell_rect(), effect.label(), TextAlign::Left, text_style());
        }
        if let Some(effect) = effects.get(2 * i + 1) {
            label(buf, cols[1].to_cell_rect(), effect.label(), TextAlign::Left, text_style());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::{AbilityType, DamageClass, Element, StatusKind};
    use crate::scenes::test_util::{lit_dot_color, rect_text};
    use crate::scenes::tooltip::{ANCHOR_GAP_CELLS, INTERIOR_PADDING_CELLS};
    use ratatui::layout::Rect;

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
                    .map(|i| {
                        [StatusKind::Burn, StatusKind::Frozen, StatusKind::Shocked, StatusKind::Rooted]
                            [i % 4]
                    })
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

        // Cost(1) + Pills(PILL_HEIGHT_CELLS) + DamageRange(1) +
        // Status(1 + ceil(1/2)) + pre-flavor gap + Flavor(FLAVOR_MAX_LINES),
        // all in cells, plus 2x interior padding.
        let content_cells = 1
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
            hovered.x + hovered.w / 2 - ANCHOR_GAP_CELLS as i32 * 2,
            "card's right edge must sit ANCHOR_GAP_CELLS cells left of the hovered cell's center"
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
        let one = Ability::new("Roar", vec![]).with_status_effects(vec![StatusKind::Burn]);
        let three = Ability::new("Roar", vec![]).with_status_effects(vec![
            StatusKind::Burn,
            StatusKind::Frozen,
            StatusKind::Shocked,
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
            "Burn",
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
