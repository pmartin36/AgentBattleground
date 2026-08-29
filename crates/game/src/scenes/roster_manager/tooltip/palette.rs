//! Pill color palette for the ability hover tooltip (spec 49): maps each
//! `Element`/`AbilityType`/`DamageClass` to the tint of its pill.
#![allow(dead_code)]

use crate::ability::{AbilityType, DamageClass};
use engine_core::color::Rgba;

pub(super) use crate::scenes::palette::element_color;

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
    use crate::ability::Element;

    #[test]
    fn ability_type_color_maps_attack_to_red() {
        assert_eq!(ability_type_color(AbilityType::Attack), Rgba::rgb(0xd0, 0x30, 0x30));
    }

    #[test]
    fn ability_type_color_maps_buff_to_green() {
        assert_eq!(ability_type_color(AbilityType::Buff), Rgba::rgb(0x3c, 0xb3, 0x71));
    }

    #[test]
    fn ability_type_color_maps_debuff_to_purple() {
        assert_eq!(ability_type_color(AbilityType::Debuff), Rgba::rgb(0x8a, 0x2b, 0xe2));
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
