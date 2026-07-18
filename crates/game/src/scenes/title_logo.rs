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
/// Lit white.
pub(crate) const WHITE_COLOR: Rgba = Rgba::rgb(0xEC, 0xEF, 0xF5);

/// Static material glyph -> RGB. (BATTLES' `G_GLOW` is overridden per frame.)
pub(crate) fn color_of(ch: char) -> Rgba {
    match ch {
        WHITE => WHITE_COLOR,
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

// ---- animation timing (seconds) — ONE per-beat block; each beat its own
//      [start,end); no global speed multiplier (Decision 8) ----------------
/// One animation beat: half-open time window `[start, end)` in seconds.
pub(crate) struct Beat {
    pub(crate) start: f32,
    pub(crate) end: f32,
}

impl Beat {
    pub(crate) const fn new(start: f32, end: f32) -> Self {
        Beat { start, end }
    }

    /// `t` is inside this beat's half-open window.
    pub(crate) fn contains(&self, t: f32) -> bool {
        t >= self.start && t < self.end
    }

    /// 0..1 progress across the beat, clamped (0 before start, 1 at/after end).
    pub(crate) fn progress(&self, t: f32) -> f32 {
        ((t - self.start) / (self.end - self.start)).clamp(0.0, 1.0)
    }
}

/// Held-still anticipation before the sword begins to fall: the scene shows
/// AGEN + the stone + the dark BATTLES etch for this long (sword off-screen),
/// then the drop. Added into every beat below, so the whole cascade shifts as
/// one and each beat stays independently tunable relative to it.
pub(crate) const PREROLL: f32 = 0.5;
pub(crate) const SWORD_DROP: Beat = Beat::new(PREROLL + 0.00, PREROLL + 0.18);
pub(crate) const IMPACT_SHAKE: Beat = Beat::new(PREROLL + 0.18, PREROLL + 0.30);
pub(crate) const IMPACT_DUST: Beat = Beat::new(PREROLL + 0.18, PREROLL + 0.38);
pub(crate) const BATTLES_IGNITE: Beat = Beat::new(PREROLL + 0.18, PREROLL + 0.42);
pub(crate) const SPARKLE_1: Beat = Beat::new(PREROLL + 0.46, PREROLL + 0.74);
pub(crate) const SPARKLE_2: Beat = Beat::new(PREROLL + 0.61, PREROLL + 0.89);
/// Animation is over after the last beat; hold on the still from here (b3-t2).
pub(crate) const ANIM_END: f32 = SPARKLE_2.end;

/// Shifts the WHOLE composition DOWN within the title area (Decision 3b). v1=0
/// (prototype had none). The fall start is DERIVED from it (see
/// `fall_dist_for_offset`), so the sword is always fully off-screen at t=0 for
/// ANY value.
pub(crate) const VERTICAL_OFFSET: i32 = 0;

// layout constants — the shared source of truth compute_layout consumes.
const CAP_H: i32 = 7 * SCALE; // caps are 7 shape-rows tall (28)
const MARGIN: i32 = SCALE; // (4)
const STONE_GAP: i32 = SCALE / 2; // (2)
const BODY_REACH: i32 = CAP_H + STONE_GAP + SCALE; // blade extent below cap (34)

/// Seated cap line (top of AGEN) in dots for a given vertical offset.
pub(crate) fn word_top_for(offset: i32) -> i32 {
    HILT_ABOVE + MARGIN + offset
}

/// Fall distance (dots the cap line travels from off-screen start to seat) for
/// a given offset. DERIVED (Decision 3b) from the seated cap line + the
/// sword's full below-cap height (blade body + tapering tip), so the tip
/// clears the top edge for ANY offset.
pub(crate) fn fall_dist_for_offset(offset: i32) -> i32 {
    word_top_for(offset) + BODY_REACH + BLADE_W / 2
}

/// Bottommost sword dot (blade tip) at t=0 for a given offset. Guaranteed < 0
/// (above the canvas top edge) for ANY offset — the off-screen-start invariant.
pub(crate) fn sword_tip_start_y(offset: i32) -> i32 {
    (word_top_for(offset) - fall_dist_for_offset(offset)) + BODY_REACH + BLADE_W / 2 - 1
}

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
    let cap_h = CAP_H;
    let hpad = SCALE;
    let vpad = SCALE;
    let margin = MARGIN;

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

    let word_top = word_top_for(VERTICAL_OFFSET); // ~6 cells of hilt headroom above AGEN
    let baseline = word_top + cap_h;
    let stone_gap = STONE_GAP;
    let stone_top = baseline + stone_gap;
    let stone_bottom = stone_top + stone_h;
    let canvas_h = stone_bottom + margin;

    let bl = ml + blade_left0;
    let slot_bottom = stone_top + SCALE;
    let body_reach = BODY_REACH; // cap_y+body_reach == slot_bottom when seated

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
        fall_dist: fall_dist_for_offset(VERTICAL_OFFSET), // tip starts fully off-screen
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
    draw_static_base(&mut cv, l);
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

// ---- b3-t2: animation-only additions (stub bodies — filled in by the
//      code-writer per research.md's blueprint) --------------------------

/// Sparkle ray reach, in dots (b3-t2).
pub(crate) const SPARK_MAX: f32 = 9.0;
/// Total sparkle rotation across its window, in radians (b3-t2).
pub(crate) const SPARK_ROT: f32 = 2.0;

/// One animation frame's authored canvas + its per-frame BATTLES color +
/// even-dot horizontal shake. Prototype's `Frame` (main.rs:439-443).
pub(crate) struct Frame {
    pub(crate) cv: Canvas,
    pub(crate) battles_color: Rgba,
    pub(crate) shake: (i32, i32),
}

/// Even-dot (2-dot / 1-cell) horizontal rattle during IMPACT_SHAKE, decaying;
/// (0,0) outside the window. Prototype `shake_offset` (main.rs:445-457).
pub(crate) fn shake_offset(t: f32) -> (i32, i32) {
    if IMPACT_SHAKE.contains(t) {
        let s = IMPACT_SHAKE.progress(t);
        let step = ((t - IMPACT_SHAKE.start) / 0.03) as i32;
        let amp = if s < 0.65 { 2 } else { 0 };
        let dir = if step % 2 == 0 { 1 } else { -1 };
        (dir * amp, 0)
    } else {
        (0, 0)
    }
}

/// White 4-point star (rotating+scaling). Prototype `draw_sparkle` (259-273).
fn draw_sparkle(cv: &mut Canvas, cx: i32, cy: i32, size: f32, angle: f32) {
    if size < 1.0 {
        return;
    }
    cv.rect(cx, cy, 2, 2, SPARK); // core
    for k in 0..4 {
        let a = angle + k as f32 * std::f32::consts::FRAC_PI_2;
        let (dc, ds) = (a.cos(), a.sin());
        let mut r = 1.0;
        while r <= size {
            cv.set(cx + (dc * r).round() as i32, cy + (ds * r).round() as i32, SPARK);
            r += 1.0;
        }
    }
}

/// Dust puff rising from the entry point, `d` in 0..1. Prototype `draw_dust`
/// (276-288).
fn draw_dust(cv: &mut Canvas, ex: i32, ey: i32, d: f32) {
    let dirs = [(-3, -1), (-2, -3), (-1, -4), (1, -4), (2, -3), (3, -1), (-4, 0), (4, 0)];
    let spread = 0.5 + d * 1.4;
    for (i, &(dx, dy)) in dirs.iter().enumerate() {
        // stagger a little so it isn't a perfect ring
        if (i as f32) / dirs.len() as f32 > d + 0.15 {
            continue;
        }
        let x = ex + (dx as f32 * spread).round() as i32;
        let y = ey + (dy as f32 * spread).round() as i32;
        cv.set(x, y, DUST);
    }
}

/// AGEN + stone + BATTLES (the static base shared by the still and every
/// frame). Extracted from `still_canvas` so it is not duplicated by
/// `render_frame` (see research.md's duplicate/reuse check).
fn draw_static_base(cv: &mut Canvas, l: &Layout) {
    // AGEN (static)
    let mut cx = l.ml;
    for c in ['A', 'G', 'E', 'N'] {
        cx += cv.letter(cx, l.word_top, c, WHITE) + l.gap;
    }
    // stone
    draw_stone(cv, l.stone_x, l.stone_top, l.stone_w, l.stone_h);
    // BATTLES
    draw_battles(cv, l);
}

/// The animated canvas at time `t`. Prototype `render_frame` (459-507),
/// driven by the `Beat` windows; ignite easing via
/// `engine_render::tween::ease_in_out`; fall ease `p*p`.
pub(crate) fn render_frame(l: &Layout, t: f32) -> Frame {
    let mut cv = Canvas::new(l.canvas_w, l.canvas_h);
    draw_static_base(&mut cv, l);
    // sword: falling (ease-in p*p) -> seated at word_top
    let p = SWORD_DROP.progress(t);
    let pe = p * p;
    let cap_y = l.word_top - ((1.0 - pe) * l.fall_dist as f32).round() as i32;
    draw_sword(&mut cv, l.bl, cap_y, cap_y + l.body_reach, l.slot_bottom);
    // impact dust
    if IMPACT_DUST.contains(t) {
        draw_dust(&mut cv, l.bl + BLADE_W / 2, l.stone_top, IMPACT_DUST.progress(t));
    }
    // two staggered, counter-spinning sparkles
    let sparks = [
        (&SPARKLE_1, l.spark_x - 3, l.spark_y - 2 * SCALE, SPARK_ROT),
        (&SPARKLE_2, l.spark_x + 3, l.spark_y + 2 * SCALE, -SPARK_ROT),
    ];
    for (beat, sx, sy, rot) in sparks {
        if beat.contains(t) {
            let sp = beat.progress(t);
            let size = (sp * std::f32::consts::PI).sin() * SPARK_MAX;
            draw_sparkle(&mut cv, sx, sy, size, sp * rot);
        }
    }
    // BATTLES ignite: dark etch -> gold, smoothstep (reuse tween::ease_in_out)
    let battles_color =
        ETCH_COLOR.lerp(GLOW_COLOR, engine_render::tween::ease_in_out(BATTLES_IGNITE.progress(t)));
    Frame { cv, battles_color, shake: shake_offset(t) }
}

/// Public consumable entry (b4's hub clock calls this): the composed grid at
/// time `t` + the unfloored dot-precise `DotRect`. `t >= ANIM_END` yields the
/// held still.
pub(crate) fn frame(elapsed_secs: f32) -> (Grid, DotRect) {
    let l = compute_layout();
    let f = render_frame(&l, elapsed_secs);
    let grid = canvas_to_grid(&f.cv, f.battles_color, f.shake);
    (grid, DotRect { x: 0, y: 0, w: l.canvas_w, h: l.canvas_h })
}


#[cfg(test)]
#[path = "title_logo_tests.rs"]
mod tests;
