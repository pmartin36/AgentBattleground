//! Shared color mappings reachable by every scene.
#![allow(dead_code)]

use crate::ability::Element;
use engine_core::color::Rgba;

/// Maps an [`Element`] to its color. Exhaustive match — a future variant
/// fails to compile rather than silently falling through.
pub(crate) fn element_color(element: Element) -> Rgba {
    match element {
        Element::Fire => Rgba::rgb(0xff, 0x8c, 0x00),
        Element::Ice => Rgba::rgb(0x7d, 0xd8, 0xff),
        Element::Earth => Rgba::rgb(0x2e, 0x8b, 0x57),
        Element::Lightning => Rgba::rgb(0xff, 0xd7, 0x00),
        Element::Normal => Rgba::rgb(0x9e, 0x9e, 0x9e),
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
    fn element_color_maps_ice_to_ice_blue() {
        assert_eq!(element_color(Element::Ice), Rgba::rgb(0x7d, 0xd8, 0xff));
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
}
