//! Spoils band (b4-t6): renders the "SPOILS" label plus a Row of bordered
//! spoil-item boxes (candy icon + description) into the band `DotRect`
//! `bands()` already computes. `spoil_item_layouts` is a PURE function so
//! tests read exact frame/icon/desc rects without re-deriving geometry —
//! mirrors `columns::column_layouts` — see research.md.

use ratatui::buffer::Buffer;

use engine_render::{Align, Basis, Direction, DotRect, FlexChild, FlexStyle, Justify};

use super::PostBattle;

/// Width (dots) reserved for the "SPOILS" label, left of the items row.
pub(super) const SPOILS_LABEL_W_DOTS: i32 = 14;
/// Dot gap between the label and items, and between adjacent items.
pub(super) const SPOILS_GAP_DOTS: i32 = 2;
/// Spoil box border thickness, in dots.
pub(super) const BOX_THICKNESS: usize = 1;
/// Spoil box corner chamfer radius, in dots.
pub(super) const BOX_RADIUS: usize = 2;
/// Width (dots) reserved for the candy icon, left of the description.
pub(super) const ICON_W_DOTS: i32 = 10;
/// Dot gap between the icon and the description text.
pub(super) const ICON_TEXT_GAP_DOTS: i32 = 1;

/// One spoil item's computed sub-rects, all dot-precise — the shared
/// geometry both `render` and its tests consume, never re-derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SpoilItemLayout {
    pub frame: DotRect,
    pub icon: DotRect,
    pub desc: DotRect,
}

/// Pure geometry: splits `band` into a label slot (`SPOILS_LABEL_W_DOTS`)
/// and an items slot (Row flex, `count` evenly-dividing boxes, gap
/// `SPOILS_GAP_DOTS`), then each box into `frame` / `icon` (`ICON_W_DOTS`)
/// / `desc` (grows). Sole source of spoil-item geometry for `render` and
/// tests.
pub(super) fn spoil_item_layouts(band: DotRect, count: usize) -> Vec<SpoilItemLayout> {
    let row_style = FlexStyle {
        direction: Direction::Row,
        justify_content: Justify::Start,
        align_items: Align::Stretch,
        gap: SPOILS_GAP_DOTS,
    };
    let top_children = [
        FlexChild { basis: Basis::Fixed(SPOILS_LABEL_W_DOTS), grow: 0.0, shrink: 0.0 },
        FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
    ];
    let top = engine_render::flex(band, row_style, &top_children);
    let items_slot = top[1];

    if count == 0 {
        return vec![];
    }
    let item_children: Vec<FlexChild> =
        (0..count).map(|_| FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 }).collect();
    let items = engine_render::flex(items_slot, row_style, &item_children);

    let inner_style = FlexStyle {
        direction: Direction::Row,
        justify_content: Justify::Start,
        align_items: Align::Stretch,
        gap: ICON_TEXT_GAP_DOTS,
    };
    let inner_children = [
        FlexChild { basis: Basis::Fixed(ICON_W_DOTS), grow: 0.0, shrink: 0.0 },
        FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
    ];

    items
        .into_iter()
        .map(|frame| {
            let inner = frame.inset(
                BOX_THICKNESS as i32,
                BOX_THICKNESS as i32,
                BOX_THICKNESS as i32,
                BOX_THICKNESS as i32,
            );
            let inner_rects = engine_render::flex(inner, inner_style, &inner_children);
            let icon = inner_rects[0];
            let desc = inner_rects[1];
            SpoilItemLayout { frame, icon, desc }
        })
        .collect()
}

/// Renders the "SPOILS" label (left) and `scene.spoils`' bordered boxes
/// (right, candy icon + description each) into `band`. Reads only
/// `scene.spoils` — no mutation, no `elapsed` (the band is static).
pub(super) fn render(scene: &PostBattle, buf: &mut Buffer, band: DotRect) {
    let row_style = FlexStyle {
        direction: Direction::Row,
        justify_content: Justify::Start,
        align_items: Align::Stretch,
        gap: SPOILS_GAP_DOTS,
    };
    let top_children = [
        FlexChild { basis: Basis::Fixed(SPOILS_LABEL_W_DOTS), grow: 0.0, shrink: 0.0 },
        FlexChild { basis: Basis::Fixed(0), grow: 1.0, shrink: 0.0 },
    ];
    let top = engine_render::flex(band, row_style, &top_children);
    let label_slot = top[0];
    if label_slot.w > 0 && label_slot.h > 0 {
        engine_render::label(buf, label_slot.to_cell_rect(), "SPOILS", PostBattle::LABEL_COLOR);
    }

    let layouts = spoil_item_layouts(band, scene.spoils.len());
    for (layout, spoil) in layouts.iter().zip(scene.spoils.iter()) {
        let frame = layout.frame;
        if frame.w <= 0 || frame.h <= 0 {
            continue;
        }
        let frame_buf = engine_render::rounded_rect(
            frame.w as usize,
            frame.h as usize,
            BOX_THICKNESS,
            BOX_RADIUS,
            PostBattle::FRAME_COLOR,
            engine_render::dots::Dot::Transparent,
        );
        super::columns::blit_dots(buf, frame, &frame_buf);

        let icon = layout.icon;
        if icon.w > 0 && icon.h > 0 {
            engine_render::draw_asset(buf, icon.to_cell_rect(), spoil.icon);
        }

        let desc = layout.desc;
        if desc.w > 0 && desc.h > 0 {
            engine_render::label(buf, desc.to_cell_rect(), &spoil.description, PostBattle::LABEL_COLOR);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    /// A fresh scene + a `band` sized to a plausible spoils-band buffer
    /// (whole-cell aligned, mirrors `columns.rs`'s `scene_and_buffer`).
    fn scene_and_buffer(w_cells: u16, h_cells: u16) -> (PostBattle, Buffer, DotRect) {
        let scene = PostBattle::new();
        let buf = Buffer::empty(Rect::new(0, 0, w_cells, h_cells));
        let band = DotRect { x: 0, y: 0, w: w_cells as i32 * 2, h: h_cells as i32 * 4 };
        (scene, buf, band)
    }

    /// The "SPOILS" label renders somewhere in the band's left region.
    #[test]
    fn spoils_band_renders_label_text() {
        let (scene, mut buf, band) = scene_and_buffer(80, 10);
        render(&scene, &mut buf, band);
        let text = crate::scenes::test_util::rect_text(&buf, band.to_cell_rect());
        assert!(
            text.contains("SPOILS"),
            "spoils band must render the \"SPOILS\" label, got {text:?}"
        );
    }

    /// `spoil_item_layouts` produces exactly `count` layouts whose frames lie
    /// within `band` and are pairwise x-disjoint (mirrors
    /// `columns::four_frames_are_horizontally_separated`).
    #[test]
    fn spoil_item_layouts_are_horizontally_separated() {
        let band = DotRect { x: 0, y: 0, w: 160, h: 40 };
        let layouts = spoil_item_layouts(band, 2);

        assert_eq!(layouts.len(), 2, "must produce exactly `count` layouts");
        for (i, l) in layouts.iter().enumerate() {
            assert!(
                l.frame.x >= band.x && l.frame.x + l.frame.w <= band.x + band.w,
                "item {i} frame must lie within band, got {:?}",
                l.frame
            );
        }
        assert!(
            layouts[0].frame.x + layouts[0].frame.w <= layouts[1].frame.x,
            "item 0 frame (right={}) must not overlap item 1 frame (left={})",
            layouts[0].frame.x + layouts[0].frame.w,
            layouts[1].frame.x
        );
    }

    /// Rendering the default scene's 2 seeded spoils paints exactly 2
    /// chamfered box borders — a lit `FRAME_COLOR` dot at each box's
    /// top-edge midpoint.
    #[test]
    fn spoils_band_renders_exactly_two_boxes() {
        let (scene, mut buf, band) = scene_and_buffer(80, 10);
        render(&scene, &mut buf, band);
        let layouts = spoil_item_layouts(band, scene.spoils.len());
        assert_eq!(layouts.len(), 2, "default scene must seed exactly 2 spoils");

        for (i, l) in layouts.iter().enumerate() {
            let color = crate::scenes::test_util::lit_dot_color(
                &buf,
                l.frame.x + l.frame.w / 2,
                l.frame.y,
            );
            assert_eq!(
                color,
                Some(PostBattle::FRAME_COLOR),
                "box {i} must have a lit FRAME_COLOR dot at its top-edge midpoint"
            );
        }
    }

    /// Each spoil box shows at least one lit braille cell inside its icon
    /// slot (the candy icon has transparent corners, so assert ANY lit, not
    /// all).
    #[test]
    fn each_spoil_box_shows_candy_icon() {
        let (scene, mut buf, band) = scene_and_buffer(80, 10);
        render(&scene, &mut buf, band);
        let layouts = spoil_item_layouts(band, scene.spoils.len());

        for (i, l) in layouts.iter().enumerate() {
            let cell_rect = l.icon.to_cell_rect();
            let has_lit = (cell_rect.top()..cell_rect.bottom())
                .flat_map(|y| (cell_rect.left()..cell_rect.right()).map(move |x| (x, y)))
                .any(|(x, y)| engine_render::decode_braille_cell(&buf, x, y).is_some());
            assert!(has_lit, "box {i} icon slot must show at least one lit braille cell");
        }
    }

    /// Each spoil box shows its own description string: item 0 -> "Spoil 1",
    /// item 1 -> "Spoil 2" (per `PostBattle::seed`).
    #[test]
    fn each_spoil_box_shows_its_description() {
        let (scene, mut buf, band) = scene_and_buffer(80, 10);
        render(&scene, &mut buf, band);
        let layouts = spoil_item_layouts(band, scene.spoils.len());

        for (i, l) in layouts.iter().enumerate() {
            let text = crate::scenes::test_util::rect_text(&buf, l.desc.to_cell_rect());
            let expected = &scene.spoils[i].description;
            assert!(
                text.contains(expected.as_str()),
                "box {i} desc slot must show {expected:?}, got {text:?}"
            );
        }
    }
}
