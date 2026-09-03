//! Shared stat-bar rendering (b1-t1): the stats-driven, caller-controlled
//! renderer both the roster and (later) the hatchery draw their 4 labeled,
//! capped stat bars through. Mirrors `detail_panel`'s shape — free
//! functions + `pub(crate)` items, data+geometry passed in, never a scene
//! `self` — so nothing here depends on `RosterManager`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use engine_render::DotRect;
use engine_render::dots::{dots_to_grid, Dot, DotBuffer};
use engine_core::color::Rgba;
use crate::stats::StatKind;

/// Lit-dot colour for filled stat-bar segments.
pub(crate) const STAT_BAR_COLOR: Rgba = Rgba::rgb(0x4a, 0xd0, 0x8a);
/// v1 fill cap for the stat bars: a stat value >= this cap paints a
/// full-length bar. Spec 35 explicitly defers the exact cap as an
/// implementation detail; this value keeps every `demo_roster()` stat
/// (range 8..34) partially-filled with clearly distinct lengths.
pub(crate) const STAT_DISPLAY_CAP: u32 = 40;
/// Height (in cells) of each stat-bar OUTLINE. 3 cells = 12 dot rows: the
/// green fill occupies exactly the MIDDLE cell (dot rows 4-7, see
/// `stat_slice_parts`), and a rounded `STAT_BAR_HUG_CAP_DOTS`-thick grey
/// cap sits directly above and below it — the top cell's bottom
/// `STAT_BAR_HUG_CAP_DOTS` dots, and the bottom cell's top
/// `STAT_BAR_HUG_CAP_DOTS` dots — with 1-dot left/right sides connecting
/// them. Because the fill is confined to its own single cell and the caps
/// live in the cells directly above/below it, no braille cell ever
/// contains both a border dot and a fill dot, so the border always
/// renders as a complete, crisp shape at any fill amount.
pub(crate) const STAT_BAR_OUTLINE_H: u16 = 3;
/// Height (in rows) of the label row at the bottom of each stat-bar slice.
pub(crate) const STAT_LABEL_H: u16 = 1;
/// Thickness (in dots) of the grey cap directly above/below the fill — see
/// `STAT_BAR_OUTLINE_H`.
const STAT_BAR_HUG_CAP_DOTS: usize = 2;
/// Gap (in cells) between adjacent stat-bar slices.
const STAT_BAR_GAP: u16 = 1;
/// Reserved blank cells on the LEFT of `stat_bar`, before the first bar
/// slice, so the leftmost bar has a deliberate left margin off the screen
/// edge rather than starting flush at `area.x`. Mirrors how
/// `STAT_BAR_DETAILS_MARGIN` reserves space on the right; the 4 slices
/// divide the width remaining between the two margins, so adding this
/// narrows each bar by a few dots rather than widening the group.
pub(crate) const STAT_BAR_LEFT_MARGIN: u16 = 2;
/// Reserved blank cells on the RIGHT of `stat_bar` that the 4 slices never
/// occupy, so the rightmost bar keeps genuine horizontal clearance from
/// the details panel's left border.
const STAT_BAR_DETAILS_MARGIN: u16 = 4;

/// Screen chrome that is NOT stat-bar-specific (shared with other bordered
/// elements) — passed in by the caller rather than redefined here, so no
/// same-value constant is duplicated across owners.
#[derive(Clone, Copy)]
pub(crate) struct StatBarChrome {
    pub border_color: Rgba,
    pub label_color: Rgba,
    pub h_thickness: usize,
    pub chamfer: usize,
}

/// The one arithmetic site for a stat value's fill length, in dot-columns
/// out of `dot_cols` (pre-round, so a caller tweening between two values can
/// still interpolate the raw `f32` before rounding to a dot count): `value`
/// scaled against `STAT_DISPLAY_CAP`, clamped to `0.0..=1.0`, times
/// `dot_cols`. Shared by the roster (which rounds and eases it) and the
/// hatchery (which rounds it directly).
pub(crate) fn stat_fill_scaled(value: u32, dot_cols: usize) -> f32 {
    (value as f32 / STAT_DISPLAY_CAP as f32).clamp(0.0, 1.0) * dot_cols as f32
}

/// Scales `c`'s alpha by `opacity` — the sole mechanism `draw_stat_bars`
/// uses to fade lit dots. At `opacity == 1.0` this returns `c` byte-
/// identical (`(255.0 * 1.0).round() as u8 == 255`), so `draw_grid`'s
/// opaque-overwrite path fires; at a lower opacity it returns a
/// translucent color that blends via `Rgba::over`.
fn with_opacity(c: Rgba, opacity: f32) -> Rgba {
    Rgba { a: (c.a as f32 * opacity).round() as u8, ..c }
}

/// Converts a cell-quantized `Rect` to its dot-precise `DotRect` (1 cell =
/// 2 dots wide, 4 dots tall). Inlined here rather than shared with
/// `RosterManager::cell_rect_to_dots` because that helper is `pub(super)`
/// to the roster module and coupling this shared component to
/// `RosterManager` is forbidden.
fn cell_rect_to_dots(r: Rect) -> DotRect {
    DotRect { x: r.x as i32 * 2, y: r.y as i32 * 4, w: r.width as i32 * 2, h: r.height as i32 * 4 }
}

/// The 4 stat slices across `stat_bar` (`StatKind::ALL` order, left->right),
/// each as `(outline_rect, fill_interior_rect, label_rect)` — sole source
/// of stat-bar geometry. Built on `engine_render::flex()`/`DotRect` — a Row
/// `flex()` with `Justify::Start`/`Align::Stretch` over cell-floored
/// `Basis::Fixed` slices, never hand-rolled x-accumulation — computing a
/// slice width that FILLS the width remaining between the left/right
/// margins (rather than centering a fixed-size group). Each slice reserves
/// its bottom `STAT_LABEL_H` row for the label (immediately below the
/// outline, no gap row); the rows above become the outline. `fill` is the
/// middle interior cell of the outline (inset a full cell on every side);
/// the caller draws the border box around the full outline and lights THIS
/// `fill` cell, so the top/bottom border and the fill land in separate
/// braille cells and neither overwrites the other.
pub(crate) fn stat_slice_parts(stat_bar: Rect) -> Vec<(Rect, Rect, Rect)> {
    let n = StatKind::ALL.len();
    // Reserve `STAT_BAR_LEFT_MARGIN` cells before the first slice and
    // `STAT_BAR_DETAILS_MARGIN` cells after the last, so the leftmost bar
    // clears the screen edge and the rightmost bar clears the details
    // panel's left border. The 4 slices divide the width remaining between
    // those two margins, so both margins narrow each bar rather than
    // widening the group past `stat_bar`.
    let container = cell_rect_to_dots(stat_bar).inset(
        STAT_BAR_LEFT_MARGIN as i32 * 2,
        STAT_BAR_DETAILS_MARGIN as i32 * 2,
        0,
        0,
    );
    // Slice width MUST stay floored at CELL granularity before ×2 to
    // dots — computing `(container.w - gap_dots*(n-1))/n` directly in
    // dots rounds differently and would silently change the layout.
    let usable_cells = container.w / 2;
    let slice_w_cells =
        (usable_cells - STAT_BAR_GAP as i32 * (n as i32 - 1)).max(0) / n as i32;
    let children: Vec<engine_render::FlexChild> = (0..n)
        .map(|_| engine_render::FlexChild {
            basis: engine_render::Basis::Fixed(slice_w_cells * 2),
            grow: 0.0,
            shrink: 0.0,
        })
        .collect();
    let slices = engine_render::flex(
        container,
        engine_render::FlexStyle {
            direction: engine_render::Direction::Row,
            justify_content: engine_render::Justify::Start,
            align_items: engine_render::Align::Stretch,
            gap: STAT_BAR_GAP as i32 * 2,
        },
        &children,
    );

    slices
        .into_iter()
        .map(|s| s.to_cell_rect())
        .map(|s| {
            // Fixed, compact outline height at the TOP of the slice (so the
            // bar sits level with the details box top), then the label on
            // the row immediately below it (no gap row). Any slice height
            // beyond that is deliberate bottom breathing room. All
            // saturating/clamped so a too-short slice degrades gracefully.
            let outline_h = STAT_BAR_OUTLINE_H.min(s.height);
            let outline = Rect::new(s.x, s.y, s.width, outline_h);
            let label_h = STAT_LABEL_H.min(s.height.saturating_sub(outline_h));
            // Label sits on the row IMMEDIATELY below the outline — no
            // blank gap row between the bar and its label.
            let label_y = (s.y + outline_h).min(s.bottom().saturating_sub(label_h));
            let label = Rect::new(s.x, label_y, s.width, label_h);
            // `fill` is exactly the outline's MIDDLE cell — inset one full
            // cell on every side — so it never shares a braille cell with
            // the hug caps drawn directly above/below it (see
            // `STAT_BAR_OUTLINE_H`).
            let fill = Rect::new(
                outline.x.saturating_add(1),
                outline.y.saturating_add(1),
                outline.width.saturating_sub(2),
                outline.height.saturating_sub(2),
            );
            (outline, fill, label)
        })
        .collect()
}

/// Display label for `kind`'s slice — an exhaustive `match` over
/// `StatKind`, mirroring `Stats::value`'s discipline (single stat list, no
/// second enumeration to drift out of sync). `pub(crate)` so callers beyond
/// the roster reuse the same mapping instead of hardcoding a second one.
pub(crate) fn stat_label(kind: StatKind) -> &'static str {
    match kind {
        StatKind::Strength => "STR",
        StatKind::Dexterity => "DEX",
        StatKind::Intelligence => "INT",
        StatKind::Vitality => "VIT",
    }
}

/// Like `engine_render::draw_dot_border`'s underlying `rounded_rect`, but
/// with an asymmetric edge thickness: left/right sides stay `h_thickness`
/// dots thick, while the top/bottom caps are `v_thickness` dots thick —
/// e.g. the stat bars' 2-dot hug caps, which a uniform border thickness
/// can't produce (it would need either a 2-dot-thick border on every side,
/// or a 1-dot cap that isn't what was asked for). Same single-dot
/// `chamfer` corner clip as `engine_render::draw_dot_border` — the clip
/// only compares distance-from-edge on each axis independently, so it
/// reads identically rounded regardless of this box's differing
/// horizontal/vertical thickness.
#[allow(clippy::too_many_arguments)] // sole caller is `draw_stat_bars`; a rect + thickness/chamfer/color params struct would only move the count, not reduce it.
pub(crate) fn draw_dot_cap_box(
    dots: &mut DotBuffer,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
    h_thickness: usize,
    v_thickness: usize,
    chamfer: usize,
    color: Rgba,
) {
    for row in top..=bottom {
        for col in left..=right {
            let d_left = col - left;
            let d_right = right - col;
            let d_top = row - top;
            let d_bottom = bottom - row;

            let in_border = d_left < h_thickness
                || d_right < h_thickness
                || d_top < v_thickness
                || d_bottom < v_thickness;
            if !in_border {
                continue;
            }

            let clipped = (d_left < chamfer && d_top < chamfer && d_left + d_top < chamfer)
                || (d_right < chamfer && d_top < chamfer && d_right + d_top < chamfer)
                || (d_left < chamfer && d_bottom < chamfer && d_left + d_bottom < chamfer)
                || (d_right < chamfer && d_bottom < chamfer && d_right + d_bottom < chamfer);
            if clipped {
                continue;
            }

            dots.set(col, row, Dot::Lit(color));
        }
    }
}

/// Draws 4 side-by-side outlined, labeled stat bars (STR/DEX/INT/VIT,
/// `StatKind::ALL` order) into `rect` — no `col_offset`, so it never
/// travels with an in-flight sprite slide. Geometry comes solely from
/// `stat_slice_parts`. Per slice the border box AND the proportional
/// `STAT_BAR_COLOR` fill are built into ONE `DotBuffer` spanning the
/// `outline` rect and drawn with a single `draw_grid` (non-text chrome, so
/// they render through the dot pipeline): a rounded "hug" bracket (via
/// `draw_dot_cap_box`) wraps just the fill's own cell — `STAT_BAR_HUG_CAP_DOTS`-thick
/// grey caps directly above/below it, 1-dot left/right sides connecting
/// them, with `chrome`'s chamfered corner — and the fill lights `fill` —
/// the outline's own middle cell — with `fill_dots(kind, fill_dot_cols)`
/// drawable dots. Because `fill` never shares a braille cell with the caps
/// (see `STAT_BAR_OUTLINE_H`), the border always renders as a complete,
/// crisp bracket at any fill amount, including zero. A plain-text
/// `stat_label(kind)` sits on the row immediately beneath.
///
/// `opacity` scales every lit dot's Rgba alpha (border caps + green fill)
/// before the `dots_to_grid`/`draw_grid` translucent-blend path — at
/// `opacity == 1.0` this is a byte-identical opaque draw (alpha stays
/// `0xFF`); text labels via `chrome.label_color` are never alpha-scaled
/// (opacity fades dots, not text).
///
/// `rect` (the stat-bar band) is honored at DOT precision, not floored to
/// the nearest cell first — the same sub-cell placement technique
/// `draw_dot_border` uses. The band's whole-cell footprint (`to_cell_rect`)
/// drives `stat_slice_parts`' cell-granular slice/outline/label geometry,
/// while the band's sub-cell remainder `(dx, dy)` offsets every slice's
/// DOT content uniformly inside its buffer before the single per-slice
/// `draw_grid`, so the whole band's true dot position survives the floor.
/// Labels are text, so they render at cell granularity (offset by the same
/// floored origin) — text can't be placed sub-cell.
pub(crate) fn draw_stat_bars(
    buf: &mut Buffer,
    rect: DotRect,
    fill_dots: impl Fn(StatKind, usize) -> usize,
    opacity: f32,
    chrome: StatBarChrome,
) {
    let cell_rect = rect.to_cell_rect();
    let (dxr, dyr) = rect.cell_remainder();
    let (dx, dy) = (dxr as usize, dyr as usize);
    for ((outline, fill, label), kind) in
        stat_slice_parts(cell_rect).into_iter().zip(StatKind::ALL)
    {
        let dot_cols = outline.width as usize * 2;
        let dot_rows = outline.height as usize * 4;
        if dot_cols > 0 && dot_rows > 0 && fill.width > 0 && fill.height > 0 {
            // Buffer sized to include the sub-cell remainder; all content
            // is drawn OFFSET by `(dx, dy)` within it, so the outline's
            // true position survives the eventual cell floor.
            let mut dots = DotBuffer::new(dot_cols + dx, dot_rows + dy);

            // Fill-cell dot bounds within the outline's dot grid (the
            // middle cell — see `stat_slice_parts`), shifted by `(dx, dy)`.
            let fx0 = dx + (fill.x - outline.x) as usize * 2;
            let fy0 = dy + (fill.y - outline.y) as usize * 4;
            let fill_dot_cols = fill.width as usize * 2;
            let fill_dot_rows = fill.height as usize * 4;

            // Rounded hug bracket: `STAT_BAR_HUG_CAP_DOTS`-thick caps
            // directly above/below the fill's own cell, 1-dot left/right
            // sides spanning that same hugged range, chamfered corners.
            let hug_top = fy0.saturating_sub(STAT_BAR_HUG_CAP_DOTS);
            let hug_bottom = (fy0 + fill_dot_rows + STAT_BAR_HUG_CAP_DOTS)
                .min(dot_rows + dy)
                .saturating_sub(1);
            draw_dot_cap_box(
                &mut dots,
                dx,
                hug_top,
                dx + dot_cols - 1,
                hug_bottom,
                chrome.h_thickness,
                STAT_BAR_HUG_CAP_DOTS,
                chrome.chamfer,
                with_opacity(chrome.border_color, opacity),
            );

            let n = fill_dots(kind, fill_dot_cols);
            let fill_color = with_opacity(STAT_BAR_COLOR, opacity);
            for row in fy0..fy0 + fill_dot_rows {
                for col in 0..n {
                    dots.set(fx0 + col, row, Dot::Lit(fill_color));
                }
            }

            let grid = dots_to_grid(&dots);
            let draw_area = Rect {
                x: outline.x,
                y: outline.y,
                width: grid.cols() as u16,
                height: grid.rows() as u16,
            };
            engine_render::draw_grid(buf, draw_area, &grid);
        }

        engine_render::label(
            buf,
            label,
            stat_label(kind),
            engine_render::TextAlign::Center,
            ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(
                chrome.label_color.r,
                chrome.label_color.g,
                chrome.label_color.b,
            )),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use crate::scenes::test_util::braille_mask;

    /// The render area every case below uses — large enough for the 4-slice
    /// bar band, small enough to keep dot scans cheap.
    fn area() -> Rect {
        Rect::new(0, 0, 40, 6)
    }

    fn cell_rect_to_dots(r: Rect) -> DotRect {
        DotRect { x: r.x as i32 * 2, y: r.y as i32 * 4, w: r.width as i32 * 2, h: r.height as i32 * 4 }
    }

    fn dummy_chrome() -> StatBarChrome {
        StatBarChrome {
            border_color: Rgba::rgb(0x88, 0x88, 0x88),
            label_color: Rgba::rgb(0xff, 0xff, 0xff),
            h_thickness: 1,
            chamfer: 1,
        }
    }

    /// A fill closure that returns `n` drawable dots for `kind` and 0 for
    /// every other stat — isolates a single bar's fill so a scan for the
    /// rightmost green-dominant cell in the whole render area unambiguously
    /// finds that one bar, without needing this module's own slice geometry.
    fn only_kind_fill_n(kind: StatKind, n: usize) -> impl Fn(StatKind, usize) -> usize + Copy {
        move |k, _cols| if k == kind { n } else { 0 }
    }

    /// Rightmost cell within `rect` whose fg is green-dominant — the
    /// `STAT_BAR_COLOR` fill blends green-dominant, the grey border does
    /// not, mirroring `roster_manager::stat_bar_tests`'s own
    /// `rightmost_green` probe.
    fn rightmost_green(buf: &Buffer, rect: Rect) -> Option<(u16, u16)> {
        (rect.left()..rect.right()).rev().find_map(|x| {
            (rect.top()..rect.bottom()).find_map(|y| match buf.cell((x, y)).unwrap().fg {
                Color::Rgb(r, g, b) if g > r && g > b => Some((x, y)),
                _ => None,
            })
        })
    }

    fn channel_sum(c: Color) -> u32 {
        match c {
            Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
            _ => panic!("expected an Rgb color, got {c:?}"),
        }
    }

    /// (a1) At `opacity == 1.0`, a lit fill cell's fg is the exact, undimmed
    /// `STAT_BAR_COLOR` — draw_grid's opaque overwrite path, not a blend.
    #[test]
    fn opacity_one_is_byte_identical_full_color() {
        let rect = cell_rect_to_dots(area());
        let mut buf = Buffer::empty(area());
        draw_stat_bars(&mut buf, rect, only_kind_fill_n(StatKind::Dexterity, 6), 1.0, dummy_chrome());

        let (x, y) = rightmost_green(&buf, area())
            .expect("a non-zero fill closure must paint a green-dominant cell");
        let expected = Color::Rgb(STAT_BAR_COLOR.r, STAT_BAR_COLOR.g, STAT_BAR_COLOR.b);
        assert_eq!(
            buf.cell((x, y)).unwrap().fg,
            expected,
            "at opacity 1.0 the fill cell must be the exact, undimmed STAT_BAR_COLOR"
        );
    }

    /// (a2) A fractional opacity blends the SAME lit fill cell darker
    /// against the buffer's background, while the lit-dot glyph mask stays
    /// identical — opacity changes color/alpha only, never which dots light.
    #[test]
    fn fractional_opacity_dims_fill_without_changing_lit_mask() {
        let rect = cell_rect_to_dots(area());
        let fill = only_kind_fill_n(StatKind::Dexterity, 6);

        let mut full = Buffer::empty(area());
        draw_stat_bars(&mut full, rect, fill, 1.0, dummy_chrome());
        let mut dim = Buffer::empty(area());
        draw_stat_bars(&mut dim, rect, fill, 0.5, dummy_chrome());

        let (x, y) = rightmost_green(&full, area())
            .expect("a non-zero fill closure must paint a green-dominant cell at opacity 1.0");

        let full_mask =
            braille_mask(&full, x, y).expect("full-opacity fill cell must be a painted glyph");
        let dim_mask = braille_mask(&dim, x, y).expect("dimmed fill cell must be a painted glyph");
        assert_eq!(
            full_mask, dim_mask,
            "opacity must not change which dots light, only their color (full={full_mask:#04x}, dim={dim_mask:#04x})"
        );

        let full_sum = channel_sum(full.cell((x, y)).unwrap().fg);
        let dim_sum = channel_sum(dim.cell((x, y)).unwrap().fg);
        assert!(
            dim_sum < full_sum,
            "a fractional opacity (0.5) must blend the fill darker over the buffer's background \
             (full={full_sum}, dim={dim_sum})"
        );
    }

    /// (b) With no `RosterManager` involved, a larger per-stat fill closure
    /// return paints that bar's green strictly farther right than a smaller
    /// one — driven purely by the passed fill input.
    #[test]
    fn larger_fill_input_paints_farther_right() {
        let rect = cell_rect_to_dots(area());

        let mut low = Buffer::empty(area());
        draw_stat_bars(&mut low, rect, only_kind_fill_n(StatKind::Dexterity, 4), 1.0, dummy_chrome());
        let mut high = Buffer::empty(area());
        draw_stat_bars(&mut high, rect, only_kind_fill_n(StatKind::Dexterity, 20), 1.0, dummy_chrome());

        let low_col = rightmost_green(&low, area()).map(|(x, _)| x);
        let high_col = rightmost_green(&high, area()).map(|(x, _)| x);
        assert!(low_col.is_some(), "a non-zero fill input must paint green");
        assert!(high_col.is_some(), "a larger fill input must also paint green");
        assert!(
            high_col.unwrap() > low_col.unwrap(),
            "a larger per-stat fill input ({high_col:?}) must paint strictly farther right than a smaller one ({low_col:?})"
        );
    }
}
