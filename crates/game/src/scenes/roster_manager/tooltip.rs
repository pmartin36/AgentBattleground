//! Ability hover tooltip (spec 49). This module lands incrementally:
//! b1-t1 = scaffold + tunable constants + pill color palette only.
//! No rendering/geometry lives here yet (b2).

// Constants and palette fns below are consumed by b2 (geometry/pill
// rendering) and b3 (wiring) — not yet called from any production path.
// Suppress dead_code until that wiring lands.
#![allow(dead_code)]

use crate::ability::{AbilityType, DamageClass, Element};
use engine_core::color::Rgba;

/// Tooltip card width (spec:26). Placeholder — tunable.
pub(super) const TOOLTIP_WIDTH_CELLS: u16 = 28;
/// Interior `.inset` padding applied to the card's content area (spec:26).
pub(super) const INTERIOR_PADDING_CELLS: u16 = 1;
/// Corner radius (dots) for the pill capsule — deeper than BattleMenu's
/// standard chamfer (2) so the ends read as rounded caps.
pub(super) const PILL_CORNER_RADIUS_DOTS: usize = 3;
/// Pill row height — one text line.
pub(super) const PILL_HEIGHT_CELLS: u16 = 1;
/// Gap between adjacent pills in the pill row.
pub(super) const INTER_PILL_GAP_CELLS: u16 = 1;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
