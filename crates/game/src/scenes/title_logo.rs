//! Procedural sword-in-stone title logo compositor (spec 61).
//!
//! Ported from `experiments/title_logo/src/main.rs`. This module only calls
//! the public engine dot API; nothing under `crates/engine/` changes.
//!
//! temporary — the `#![allow(dead_code)]` below is removed in b4-t1 once
//! `main_hub` wires this module in as a real caller.
#![allow(dead_code)]

use engine_core::color::Rgba;
use engine_render::{DotRect, Grid};
use engine_render::dots::{dots_to_grid_tinted, Dot, DotBuffer};

/// Master size: native font dots × SCALE.
pub(crate) const SCALE: i32 = 4;

// ---- material glyphs (one char per lit dot) --------------------------------
/// AGEN letters material glyph.
pub(crate) const WHITE: char = '#';
/// Blade specular highlight.
pub(crate) const B_HI: char = 'H';
/// Blade light face.
pub(crate) const B_LT: char = 'L';
/// Blade dark face.
pub(crate) const B_DK: char = 'd';
/// Blade shadow edge.
pub(crate) const B_SH: char = 's';
/// Gold highlight (guard).
pub(crate) const G_HI: char = 'G';
/// BATTLES — recolored per frame (etch → gold).
pub(crate) const G_GLOW: char = 'Y';
/// Gold mid.
pub(crate) const G_MD: char = 'O';
/// Gold shadow.
pub(crate) const G_SH: char = 'o';
/// Pommel.
pub(crate) const P: char = 'P';
/// Grip leather light.
pub(crate) const R_LT: char = 'B';
/// Grip leather dark.
pub(crate) const R_DK: char = 'b';
/// Stone light fleck.
pub(crate) const S_HI: char = '^';
/// Stone body.
pub(crate) const S_MD: char = ':';
/// Stone dark fleck.
pub(crate) const S_SH: char = ',';
/// Impact dust.
pub(crate) const DUST: char = 'u';
/// Sparkle.
pub(crate) const SPARK: char = '*';
/// Unlit backdrop.
pub(crate) const EMPTY: char = ' ';

// BATTLES cross-fades between these two per frame.
/// Dark inset (darker than stone).
pub(crate) const ETCH_COLOR: Rgba = Rgba::rgb(0x2C, 0x2E, 0x38);
/// Lit gold.
pub(crate) const GLOW_COLOR: Rgba = Rgba::rgb(0xFF, 0xD8, 0x48);

/// Static material glyph -> RGB. (BATTLES' `G_GLOW` is overridden per frame.)
pub(crate) fn color_of(ch: char) -> Rgba {
    match ch {
        WHITE => Rgba::rgb(0xEC, 0xEF, 0xF5),
        B_HI => Rgba::rgb(0xF6, 0xFA, 0xFF),
        B_LT => Rgba::rgb(0xC6, 0xD2, 0xE2),
        B_DK => Rgba::rgb(0x6C, 0x7A, 0x8E),
        B_SH => Rgba::rgb(0x3E, 0x49, 0x59),
        G_HI => Rgba::rgb(0xFF, 0xDE, 0x7C),
        G_GLOW => GLOW_COLOR,
        G_MD => Rgba::rgb(0xE0, 0xAE, 0x38),
        G_SH => Rgba::rgb(0x9A, 0x71, 0x1C),
        P => Rgba::rgb(0xF0, 0xCB, 0x60),
        R_LT => Rgba::rgb(0x8A, 0x59, 0x32),
        R_DK => Rgba::rgb(0x4E, 0x31, 0x1A),
        S_HI => Rgba::rgb(0x80, 0x83, 0x8E),
        S_MD => Rgba::rgb(0x5C, 0x5F, 0x6A),
        S_SH => Rgba::rgb(0x3C, 0x3E, 0x48),
        DUST => Rgba::rgb(0xA8, 0x9E, 0x8C),
        SPARK => Rgba::rgb(0xFF, 0xFF, 0xFF),
        _ => Rgba::rgb(0xFF, 0x00, 0xFF),
    }
}

// ---- sword geometry (dot units) --------------------------------------------
pub(crate) const BLADE_LIGHT: i32 = SCALE;
pub(crate) const BLADE_DARK: i32 = SCALE;
pub(crate) const BLADE_W: i32 = BLADE_LIGHT + BLADE_DARK;
pub(crate) const GRIP_W: i32 = SCALE;
pub(crate) const GUARD_W: i32 = 6 * SCALE;
pub(crate) const GUARD_H: i32 = SCALE;
pub(crate) const GRIP_LEN: i32 = 2 * SCALE + 2;
pub(crate) const POMMEL_H: i32 = SCALE;
pub(crate) const NECK: i32 = SCALE / 2;
pub(crate) const HILT_ABOVE: i32 = NECK + GUARD_H + GRIP_LEN + POMMEL_H;

/// Pinned layout anchors, all in dot units. Derived by `compute_layout`.
pub(crate) struct Layout {
    pub(crate) canvas_w: i32,
    pub(crate) canvas_h: i32,
    pub(crate) gap: i32,
    pub(crate) word_top: i32,
    pub(crate) ml: i32,
    pub(crate) bl: i32,
    pub(crate) stone_x: i32,
    pub(crate) stone_top: i32,
    pub(crate) stone_w: i32,
    pub(crate) stone_h: i32,
    pub(crate) battles_x: i32,
    pub(crate) battles_y: i32,
    pub(crate) slot_bottom: i32,
    pub(crate) body_reach: i32,
    pub(crate) fall_dist: i32,
    pub(crate) spark_x: i32,
    pub(crate) spark_y: i32,
}

/// Advance width, in dots, of `c` in the bold name font at `SCALE`. Sources
/// its matrix from `crate::braille_name::bold_matrix`.
pub(crate) fn glyph_w(c: char) -> i32 {
    crate::braille_name::bold_matrix(c)[0].len() as i32 * SCALE
}

/// Total width, in dots, of `word` rendered via `glyph_w` with `gap` dots
/// between each letter.
fn word_width(word: &str, gap: i32) -> i32 {
    let mut w = 0;
    for (i, c) in word.chars().enumerate() {
        w += glyph_w(c);
        if i + 1 < word.chars().count() {
            w += gap;
        }
    }
    w
}

/// Derives the whole-logo layout (canvas size + every anchor position) from
/// `SCALE`, per-glyph widths, and the sword-geometry constants.
pub(crate) fn compute_layout() -> Layout {
    let gap = SCALE;
    let cap_h = 7 * SCALE;
    let hpad = SCALE;
    let vpad = SCALE;
    let margin = SCALE;

    let agen_w = glyph_w('A') + gap + glyph_w('G') + gap + glyph_w('E') + gap + glyph_w('N');
    let blade_left0 = agen_w + gap;
    let guard_left0 = blade_left0 + (BLADE_W - GUARD_W) / 2;
    let sword_right0 = guard_left0 + GUARD_W - 1;
    let agent_right0 = sword_right0.max(agen_w);

    let battles_w = word_width("BATTLES", gap);
    let stone_w = battles_w + 2 * hpad;
    let stone_h = cap_h + 2 * vpad;

    let anchor0 = agent_right0 / 2;
    let stone_x0 = anchor0 - stone_w / 2;
    let min_x = 0.min(guard_left0).min(stone_x0);
    let shift = margin - min_x;
    let ml = shift;
    let stone_x = stone_x0 + shift;
    let stone_cx = stone_x + stone_w / 2;
    let right = (ml + sword_right0).max(stone_x + stone_w);
    let canvas_w = right + margin;

    let word_top = HILT_ABOVE + margin; // ~6 cells of hilt headroom above AGEN
    let baseline = word_top + cap_h;
    let stone_gap = SCALE / 2;
    let stone_top = baseline + stone_gap;
    let stone_bottom = stone_top + stone_h;
    let canvas_h = stone_bottom + margin;

    let bl = ml + blade_left0;
    let slot_bottom = stone_top + SCALE;
    let body_reach = cap_h + stone_gap + SCALE; // cap_y+body_reach == slot_bottom when seated

    let mut battles_x = stone_cx - battles_w / 2;
    battles_x -= battles_x.rem_euclid(2);
    let mut battles_y = stone_top + (stone_h - cap_h) / 2;
    battles_y -= battles_y.rem_euclid(4);

    Layout {
        canvas_w,
        canvas_h,
        gap,
        word_top,
        ml,
        bl,
        stone_x,
        stone_top,
        stone_w,
        stone_h,
        battles_x,
        battles_y,
        slot_bottom,
        body_reach,
        fall_dist: word_top + body_reach + 2, // tip starts just above the top edge
        spark_x: bl + BLADE_W / 2,
        spark_y: word_top + cap_h / 2, // mid-blade
    }
}

/// Char/material authoring buffer at dot resolution.
pub(crate) struct Canvas {
    w: i32,
    h: i32,
    cells: Vec<char>,
}

impl Canvas {
    pub(crate) fn new(w: i32, h: i32) -> Self {
        Canvas { w, h, cells: vec![EMPTY; (w * h) as usize] }
    }

    pub(crate) fn set(&mut self, x: i32, y: i32, ch: char) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.cells[(y * self.w + x) as usize] = ch;
        }
    }

    pub(crate) fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, ch: char) {
        for j in 0..h {
            for i in 0..w {
                self.set(x + i, y + j, ch);
            }
        }
    }

    /// Stamp a bold letter (top-left at x,y). Returns advance width in dots.
    pub(crate) fn letter(&mut self, x: i32, y: i32, c: char, ch: char) -> i32 {
        let m = crate::braille_name::bold_matrix(c);
        for (r, row) in m.iter().enumerate() {
            for (col, &lit) in row.iter().enumerate() {
                if lit {
                    self.rect(x + col as i32 * SCALE, y + r as i32 * SCALE, SCALE, SCALE, ch);
                }
            }
        }
        m[0].len() as i32 * SCALE
    }

    pub(crate) fn at(&self, x: i32, y: i32) -> char {
        self.cells[(y * self.w + x) as usize]
    }
}

/// Cheap deterministic value noise. Ported from the prototype
/// (`experiments/title_logo/src/main.rs` `noise`, 166-171).
fn noise(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(2654435761).wrapping_add((y as u32).wrapping_mul(40503));
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^ (h >> 16)
}

/// Straight top edge (swallows the blade cleanly); hewn sides/bottom via
/// `noise`. Ported from the prototype `stone_edge` (389-403).
fn stone_edge(sw: i32, sh: i32, lx: i32, ly: i32) -> bool {
    let nick = |seed: i32, s: i32| -> i32 { noise(seed.div_euclid(6), s).is_multiple_of(3) as i32 };
    // top edge stays straight (swallows the blade cleanly); sides + bottom hew.
    let bot = nick(lx, 22);
    let lft = nick(ly, 33);
    let rgt = nick(ly, 44);
    if ly >= sh - bot || lx < lft || lx >= sw - rgt {
        return false;
    }
    let c = SCALE;
    lx + ly >= c
        && (sw - 1 - lx) + ly >= c
        && lx + (sh - 1 - ly) >= c
        && (sw - 1 - lx) + (sh - 1 - ly) >= c
}

/// Textured gray stone slab: cell-level light/dark patches, sparse `EMPTY`
/// grain holes, hewn side/bottom edges, straight top edge, wandering crack.
/// Ported from the prototype `draw_stone` (405-435).
pub(crate) fn draw_stone(cv: &mut Canvas, sx: i32, sy: i32, sw: i32, sh: i32) {
    for ly in 0..sh {
        for lx in 0..sw {
            if !stone_edge(sw, sh, lx, ly) {
                continue;
            }
            let (x, y) = (sx + lx, sy + ly);
            let cell_n = noise(x.div_euclid(2), y.div_euclid(4));
            let base = if cell_n.is_multiple_of(11) {
                S_HI
            } else if cell_n.is_multiple_of(9) {
                S_SH
            } else {
                S_MD
            };
            let ch = if noise(x, y).is_multiple_of(20) { EMPTY } else { base };
            cv.set(x, y, ch);
        }
    }
    let mut cxp = sx + sw / 4;
    for j in 1..sh - 1 {
        if stone_edge(sw, sh, cxp - sx, j) {
            cv.set(cxp, sy + j, EMPTY);
        }
        match noise(cxp, sy + j) % 4 {
            0 => cxp -= 1,
            3 => cxp += 1,
            _ => {}
        }
    }
}

/// Stamps "BATTLES" across the slab in the gold-glow material (`G_GLOW`),
/// starting at `l.battles_x`/`l.battles_y` (cell-grid-snapped). Ported from
/// the prototype `render_frame`'s BATTLES loop (471-476).
pub(crate) fn draw_battles(cv: &mut Canvas, l: &Layout) {
    let mut bx = l.battles_x;
    let word = "BATTLES";
    let n = word.chars().count();
    for (i, c) in word.chars().enumerate() {
        bx += cv.letter(bx, l.battles_y, c, G_GLOW);
        if i + 1 < n {
            bx += l.gap;
        }
    }
}

/// Blade cross-section, left→right: highlight, light, dark, shadow. Ported
/// from the prototype `blade_xsect` (experiments/title_logo/src/main.rs
/// 187-196). Length == `BLADE_W`.
///
fn blade_xsect() -> Vec<char> {
    let mut v = Vec::with_capacity(BLADE_W as usize);
    for i in 0..BLADE_LIGHT {
        v.push(if i < 2 { B_HI } else { B_LT });
    }
    for i in 0..BLADE_DARK {
        v.push(if i >= BLADE_DARK - 2 { B_SH } else { B_DK });
    }
    v
}

/// Draws the sword — pommel/grip/guard hilt floating `NECK` above `cap_y`,
/// blade body + tapering tip below, every dot clipped at `clip_bottom` (the
/// stone surface). Ported from the prototype `draw_sword`
/// (experiments/title_logo/src/main.rs 202-255).
pub(crate) fn draw_sword(cv: &mut Canvas, bl: i32, cap_y: i32, body_end: i32, clip_bottom: i32) {
    let xsect = blade_xsect();
    let grip_x = bl + (BLADE_W - GRIP_W) / 2;
    let guard_bottom = cap_y - NECK;
    let guard_top = guard_bottom - GUARD_H;
    let grip_top = guard_top - GRIP_LEN;
    let pommel_top = grip_top - POMMEL_H;
    // pommel + grip
    cv.rect(grip_x - 1, pommel_top + 1, GRIP_W + 2, POMMEL_H - 1, P);
    cv.rect(grip_x, pommel_top, GRIP_W, 1, G_HI);
    for row in 0..GRIP_LEN {
        for i in 0..GRIP_W {
            let ch = if i < GRIP_W / 2 { R_LT } else { R_DK };
            cv.set(grip_x + i, grip_top + row, ch);
        }
    }
    // guard (horizontal sheen, lit from the left)
    let g_left = bl + (BLADE_W - GUARD_W) / 2;
    for j in 0..GUARD_H {
        for i in 0..GUARD_W {
            let ch = if i < GUARD_W / 4 {
                G_HI
            } else if i < GUARD_W * 7 / 10 {
                G_MD
            } else {
                G_SH
            };
            cv.set(g_left + i, guard_top + j, ch);
        }
    }
    // blade body, clipped at the stone surface
    for y in guard_bottom..body_end {
        if y >= clip_bottom {
            break;
        }
        for (i, &ch) in xsect.iter().enumerate() {
            cv.set(bl + i as i32, y, ch);
        }
    }
    // tapering tip below the body (visible mid-fall, buried when seated)
    let steps = BLADE_W / 2;
    for s in 0..steps {
        let y = body_end + s;
        if y >= clip_bottom {
            break;
        }
        for (i, &ch) in xsect.iter().enumerate() {
            let ii = i as i32;
            if ii >= s && ii < BLADE_W - s {
                cv.set(bl + ii, y, ch);
            }
        }
    }
}

/// Builds the seated static-still composition: AGEN + stone + BATTLES +
/// seated sword, no dust/sparkle/shake (animation-only concerns). Port of
/// the prototype `render_frame` (462-481, `p==1`).
pub(crate) fn still_canvas(l: &Layout) -> Canvas {
    let mut cv = Canvas::new(l.canvas_w, l.canvas_h);

    // AGEN (static)
    let mut cx = l.ml;
    for c in ['A', 'G', 'E', 'N'] {
        cx += cv.letter(cx, l.word_top, c, WHITE) + l.gap;
    }
    // stone
    draw_stone(&mut cv, l.stone_x, l.stone_top, l.stone_w, l.stone_h);
    // BATTLES
    draw_battles(&mut cv, l);
    // sword, seated (cap at word_top, tip buried at slot_bottom)
    draw_sword(&mut cv, l.bl, l.word_top, l.word_top + l.body_reach, l.slot_bottom);

    cv
}

/// Splits `cv` into a uniform-white SHAPE `DotBuffer` and a per-dot COLOR
/// `DotBuffer`. For every non-EMPTY canvas dot (shifted by `shake`, clipped
/// to the canvas): `color_of(ch)`, with `G_GLOW` overridden by
/// `battles_color`. Port of the prototype `frame_to_string` (512-528).
pub(crate) fn build_buffers(cv: &Canvas, battles_color: Rgba, shake: (i32, i32)) -> (DotBuffer, DotBuffer) {
    let (sdx, sdy) = shake;
    let white = Dot::Lit(Rgba::rgb(0xFF, 0xFF, 0xFF));
    let mut shape = DotBuffer::new(cv.w as usize, cv.h as usize);
    let mut color = DotBuffer::new(cv.w as usize, cv.h as usize);
    for y in 0..cv.h {
        for x in 0..cv.w {
            let ch = cv.at(x, y);
            if ch == EMPTY {
                continue;
            }
            let col = if ch == G_GLOW { battles_color } else { color_of(ch) };
            let (px, py) = (x + sdx, y + sdy);
            if px >= 0 && py >= 0 && px < cv.w && py < cv.h {
                shape.set(px as usize, py as usize, white);
                color.set(px as usize, py as usize, Dot::Lit(col));
            }
        }
    }
    (shape, color)
}

/// `build_buffers` followed by `dots_to_grid_tinted`. Port of the prototype
/// `frame_to_string` (529). The seam b3-t2's `frame()` reuses per-frame.
pub(crate) fn canvas_to_grid(cv: &Canvas, battles_color: Rgba, shake: (i32, i32)) -> Grid {
    let (shape, color) = build_buffers(cv, battles_color, shake);
    dots_to_grid_tinted(&shape, &color)
}

/// Public still entry: the seated composition rendered via the tinted path
/// (Decision 6 — no luma-halo culling), paired with its dot-precise,
/// unfloored `DotRect` (never derived via `to_cell_rect`).
pub(crate) fn compose_still() -> (Grid, DotRect) {
    let l = compute_layout();
    let cv = still_canvas(&l);
    let grid = canvas_to_grid(&cv, GLOW_COLOR, (0, 0));
    (grid, DotRect { x: 0, y: 0, w: l.canvas_w, h: l.canvas_h })
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_render::dots::{dots_to_grid, dots_to_grid_tinted};
    use engine_render::Cell;

    /// Counts total lit dot bits across every `Cell::Glyph` in `g` (sum of
    /// `count_ones` on each glyph's braille mask, `ch - U+2800`).
    fn count_lit_dots(g: &Grid) -> u32 {
        let mut n = 0u32;
        for r in 0..g.rows() {
            for c in 0..g.cols() {
                if let Cell::Glyph { ch, .. } = g.get(c, r) {
                    n += (ch as u32 - 0x2800).count_ones();
                }
            }
        }
        n
    }

    /// DELIVERABLE: `compute_layout()` produces the prototype's verbatim-
    /// ported canvas size — 188×94 dots, which floors to 94×23 cells (NOT
    /// 96 dots / 24 rows; see research.md's REFINED verdict). The spec's
    /// "≈94×24" is satisfied at 94×23 — assert `cols == 94` exactly and
    /// `rows` within `23..=24`, never `rows == 24` exactly.
    #[test]
    fn compute_layout_canvas_dims_are_188x94_dots() {
        let layout = compute_layout();
        assert_eq!(layout.canvas_w, 188, "canvas width must be 188 dots");
        assert_eq!(layout.canvas_h, 94, "canvas height must be 94 dots");

        let cols = layout.canvas_w / 2;
        let rows = layout.canvas_h / 4;
        assert_eq!(cols, 94, "canvas width must floor to 94 cells");
        assert!(
            (23..=24).contains(&rows),
            "canvas height must floor to ~24 cells (94 dots -> 23), got {rows}"
        );
    }

    /// DELIVERABLE: pinned anchor positions land where the prototype ported
    /// them, including the cell-grid snap invariants on the BATTLES origin
    /// (Decision-pinned: battles_x even, battles_y a multiple of 4).
    #[test]
    fn compute_layout_pinned_anchors() {
        let layout = compute_layout();
        assert_eq!(layout.gap, 4);
        assert_eq!(layout.word_top, 24);
        assert_eq!(layout.stone_top, 54);
        assert_eq!(layout.stone_h, 36);
        assert_eq!(layout.slot_bottom, 58);

        assert_eq!(layout.battles_x % 2, 0, "battles_x must be cell-grid snapped (even)");
        assert_eq!(
            layout.battles_y.rem_euclid(4),
            0,
            "battles_y must be cell-grid snapped (multiple of 4)"
        );

        // Animation-only fields must also be populated with sane, in-canvas
        // values (exercised here; exact values are b3's contract).
        assert!(layout.fall_dist > 0, "fall_dist must be positive");
        assert!(
            layout.spark_x >= 0 && layout.spark_x < layout.canvas_w,
            "spark_x must land inside the canvas"
        );
        assert!(
            layout.spark_y >= 0 && layout.spark_y < layout.canvas_h,
            "spark_y must land inside the canvas"
        );
    }

    /// Font-reuse guard: the scaled glyph stamping must consume b1-t1's
    /// widened N through `braille_name::bold_matrix`, not a stale local
    /// copy — stamping "AGEN" must actually light cells, and glyph_w('N')
    /// must reflect the widened (5-wide base -> 6-wide bold -> 24 dots at
    /// SCALE=4) letterform.
    #[test]
    fn font_reuse_stamps_widened_n_through_bold_matrix() {
        let mut cv = Canvas::new(200, 100);
        cv.letter(0, 0, 'A', WHITE);
        cv.letter(30, 0, 'G', WHITE);
        cv.letter(60, 0, 'E', WHITE);
        cv.letter(90, 0, 'N', WHITE);

        let mut any_lit = false;
        'outer: for y in 0..100 {
            for x in 0..200 {
                if cv.at(x, y) != EMPTY {
                    any_lit = true;
                    break 'outer;
                }
            }
        }
        assert!(any_lit, "stamping AGEN must light at least one canvas cell");
        assert_eq!(
            glyph_w('N'),
            24,
            "widened N (b1-t1) must be 24 dots wide at SCALE=4"
        );
    }

    /// DELIVERABLE (b2-t2): the stone rect is textured, not a solid fill —
    /// contains lit stone-body material cells AND sparse EMPTY grain/crack
    /// holes (grain authored as unlit dots, not skipped).
    #[test]
    fn stone_slab_is_textured_and_holed() {
        let l = compute_layout();
        let mut cv = Canvas::new(l.canvas_w, l.canvas_h);
        draw_stone(&mut cv, l.stone_x, l.stone_top, l.stone_w, l.stone_h);

        let mut stone_body = 0;
        let mut empty_holes = 0;
        for y in l.stone_top..(l.stone_top + l.stone_h) {
            for x in l.stone_x..(l.stone_x + l.stone_w) {
                match cv.at(x, y) {
                    S_HI | S_MD | S_SH => stone_body += 1,
                    EMPTY => empty_holes += 1,
                    _ => {}
                }
            }
        }
        assert!(stone_body > 0, "stone rect must contain textured stone-body cells");
        assert!(
            empty_holes > 0,
            "stone rect must contain sparse EMPTY grain/crack holes (not a solid fill)"
        );
    }

    /// DELIVERABLE (b2-t2): BATTLES is stamped in gold (`G_GLOW` -> `GLOW_COLOR`)
    /// at the layout's cell-grid-snapped origin.
    #[test]
    fn battles_is_stamped_gold_and_cell_snapped() {
        let l = compute_layout();
        assert_eq!(l.battles_x % 2, 0, "battles_x must be cell-grid snapped (even)");
        assert_eq!(
            l.battles_y.rem_euclid(4),
            0,
            "battles_y must be cell-grid snapped (multiple of 4)"
        );

        let mut cv = Canvas::new(l.canvas_w, l.canvas_h);
        draw_battles(&mut cv, &l);

        let mut glow_cells = 0;
        for y in l.battles_y..(l.battles_y + 7 * SCALE) {
            for x in 0..l.canvas_w {
                if cv.at(x, y) == G_GLOW {
                    glow_cells += 1;
                }
            }
        }
        assert!(
            glow_cells > 0,
            "draw_battles must light G_GLOW cells on the BATTLES rows"
        );
        assert_eq!(
            color_of(G_GLOW),
            GLOW_COLOR,
            "G_GLOW must map to gold in the static still"
        );
    }

    /// Guard on the ported edge rule: the slab's top edge stays straight
    /// (not hewn away) across the mid-span, so it cleanly swallows the blade.
    #[test]
    fn stone_top_edge_is_straight_across_mid_span() {
        let l = compute_layout();
        let mid_lo = l.stone_w / 4;
        let mid_hi = l.stone_w - l.stone_w / 4;
        let mut any_intact = false;
        for lx in mid_lo..mid_hi {
            if stone_edge(l.stone_w, l.stone_h, lx, 0) {
                any_intact = true;
            }
        }
        assert!(
            any_intact,
            "stone top edge must remain intact (straight) across the mid-span"
        );
    }

    /// DELIVERABLE (b2-t3): with the sword seated (cap at the final letter
    /// line), no sword material is drawn at or below the stone-surface clip
    /// line (`slot_bottom`) — every dot is clipped as the tip buries.
    #[test]
    fn sword_clipped_at_stone_surface() {
        let l = compute_layout();
        let mut cv = Canvas::new(l.canvas_w, l.canvas_h);
        draw_sword(&mut cv, l.bl, l.word_top, l.word_top + l.body_reach, l.slot_bottom);

        for y in l.slot_bottom..l.canvas_h {
            for x in 0..l.canvas_w {
                assert_eq!(
                    cv.at(x, y),
                    EMPTY,
                    "sword material must not appear at/below the stone-surface clip line (x={x}, y={y})"
                );
            }
        }
    }

    /// DELIVERABLE (b2-t3): the guard renders as a wide horizontal crossbar
    /// above the caps, while the blade body within the word rows is the
    /// narrow stem — the T shape.
    #[test]
    fn guard_is_crossbar_above_caps() {
        let l = compute_layout();
        let mut cv = Canvas::new(l.canvas_w, l.canvas_h);
        draw_sword(&mut cv, l.bl, l.word_top, l.word_top + l.body_reach, l.slot_bottom);

        let mut max_lit_above_caps = 0;
        for y in 0..l.word_top {
            let count = (0..l.canvas_w).filter(|&x| cv.at(x, y) != EMPTY).count() as i32;
            max_lit_above_caps = max_lit_above_caps.max(count);
        }
        assert!(
            max_lit_above_caps >= GUARD_W,
            "guard crossbar must span at least GUARD_W ({GUARD_W}) lit dots on some row above the caps (word_top), got {max_lit_above_caps}"
        );

        let stem_row = l.word_top + SCALE;
        let stem_count = (0..l.canvas_w).filter(|&x| cv.at(x, stem_row) != EMPTY).count() as i32;
        assert!(
            stem_count > 0 && stem_count <= BLADE_W,
            "blade stem row inside the word body must be narrow (<= BLADE_W = {BLADE_W}), got {stem_count}"
        );
    }

    /// DELIVERABLE (b2-t3): blade cross-section material is present within
    /// the word-body rows — the T stem.
    #[test]
    fn blade_material_present_in_word_body() {
        let l = compute_layout();
        let mut cv = Canvas::new(l.canvas_w, l.canvas_h);
        draw_sword(&mut cv, l.bl, l.word_top, l.word_top + l.body_reach, l.slot_bottom);

        let cap_h = 7 * SCALE;
        let mut blade_material_found = false;
        for y in l.word_top..(l.word_top + cap_h) {
            for x in 0..l.canvas_w {
                if matches!(cv.at(x, y), B_HI | B_LT | B_DK | B_SH) {
                    blade_material_found = true;
                }
            }
        }
        assert!(
            blade_material_found,
            "blade cross-section material must be present within the word-body rows [word_top, word_top+7*SCALE)"
        );
    }

    /// DELIVERABLE (b2-t4): the tinted conversion (uniform-white shape +
    /// per-dot color) preserves every authored dot exactly — no luma-halo
    /// culling — while the same color buffer run through plain
    /// `dots_to_grid` culls some of the dark blade-shadow / dark-stone dots
    /// that share a 2x4 block with brighter dots. This is the direct guard
    /// against the bug Decision 6 calls out.
    #[test]
    fn still_dark_dot_survives_tinted_conversion() {
        let l = compute_layout();
        let cv = still_canvas(&l);

        let mut authored = 0u32;
        for y in 0..l.canvas_h {
            for x in 0..l.canvas_w {
                if cv.at(x, y) != EMPTY {
                    authored += 1;
                }
            }
        }
        assert!(authored > 0, "still_canvas must author at least one lit dot");

        let (shape, color) = build_buffers(&cv, GLOW_COLOR, (0, 0));
        let tinted = dots_to_grid_tinted(&shape, &color);
        let plain = dots_to_grid(&color);

        let lit_tinted = count_lit_dots(&tinted);
        let lit_plain = count_lit_dots(&plain);

        assert_eq!(
            lit_tinted, authored,
            "tinted conversion must preserve every authored dot (no luma culling)"
        );
        assert!(
            lit_plain < lit_tinted,
            "plain dots_to_grid must cull some dark dots that the tinted path preserves (tinted={lit_tinted}, plain={lit_plain})"
        );
    }

    /// DELIVERABLE (b2-t4): `compose_still` returns the logo's genuine
    /// dot-precise extent (188x94), never reconstructed from the floored
    /// or ceiling-rounded grid — the CLAUDE.md #5 unfloored-DotRect
    /// invariant b4-t1 depends on.
    #[test]
    fn compose_still_returns_unfloored_dotprecise_rect() {
        let (grid, rect) = compose_still();

        assert_eq!(rect, DotRect { x: 0, y: 0, w: 188, h: 94 });
        assert_eq!(grid.cols(), 94);
        assert_eq!(grid.rows(), 24);
        assert_ne!(
            rect.h,
            grid.rows() as i32 * 4,
            "rect.h must be the genuine dot extent (94), not grid-reconstructed (96)"
        );
    }

    /// Sanity: the composed still actually renders something (not an
    /// all-transparent grid).
    #[test]
    fn compose_still_grid_nonempty() {
        let (grid, _rect) = compose_still();
        let mut any_glyph = false;
        'outer: for r in 0..grid.rows() {
            for c in 0..grid.cols() {
                if matches!(grid.get(c, r), Cell::Glyph { .. }) {
                    any_glyph = true;
                    break 'outer;
                }
            }
        }
        assert!(any_glyph, "compose_still must render at least one lit glyph cell");
    }
}
