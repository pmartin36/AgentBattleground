//! AGENT BATTLES — sword-in-the-stone title logo: dot-matrix compositor + a
//! self-contained ANIMATION preview. Scratch experiment only (excluded from the
//! workspace); it *calls* the engine's public braille API and never modifies it.
//!
//! Animation (plays once, then holds): sword falls fast → slots into the T →
//! BATTLES ignites from a dark etch to gold → a white 4-point sparkle twinkles
//! on the blade. ~0.8s.
//!
//!   run:            cd experiments/title_logo && cargo run     (play once, hold)
//!   inspect a beat: FRAME=0.28 cargo run                       (dump one frame)

use engine_core::color::Rgba;
use engine_render::dots::{dots_to_grid_tinted, Dot, DotBuffer};
use engine_render::grid::Cell;
use std::io::Write;

/// Master size: native font dots × SCALE.
const SCALE: i32 = 4;

// ---- animation timeline (seconds) ------------------------------------------
const FALL_DUR: f32 = 0.18; // sword drop (fast)
const LAND: f32 = FALL_DUR; // seats at this instant
const IGNITE_START: f32 = LAND;
const IGNITE_DUR: f32 = 0.24; // BATTLES etch -> gold
const SPARK_START: f32 = 0.46; // starts AFTER the ignite finishes
const SPARK_DUR: f32 = 0.28; // sparkle grow → shrink (per glint)
const SPARK_OFFSET: f32 = 0.15; // 2nd glint starts as the 1st is shrinking
const SHAKE_DUR: f32 = 0.12; // impact shake
const DUST_DUR: f32 = 0.20; // impact dust
const ANIM_END: f32 = 0.92;
const SPARK_MAX: f32 = 9.0; // sparkle ray reach (dots)
const SPARK_ROT: f32 = 2.0; // total sparkle rotation (radians)

// ---- material glyphs (one char per lit dot) --------------------------------
const WHITE: char = '#'; // AGEN letters
const B_HI: char = 'H'; // blade specular highlight
const B_LT: char = 'L'; // blade light face
const B_DK: char = 'd'; // blade dark face
const B_SH: char = 's'; // blade shadow edge
const G_HI: char = 'G'; // gold highlight (guard)
const G_GLOW: char = 'Y'; // BATTLES — recolored per frame (etch → gold)
const G_MD: char = 'O'; // gold mid
const G_SH: char = 'o'; // gold shadow
const P: char = 'P'; // pommel
const R_LT: char = 'B'; // grip leather light
const R_DK: char = 'b'; // grip leather dark
const S_HI: char = '^'; // stone light fleck
const S_MD: char = ':'; // stone body
const S_SH: char = ','; // stone dark fleck
const DUST: char = 'u'; // impact dust
const SPARK: char = '*'; // sparkle
const EMPTY: char = ' '; // unlit -> black backdrop

// BATTLES cross-fades between these two per frame.
const ETCH_COLOR: Rgba = Rgba::rgb(0x2C, 0x2E, 0x38); // dark inset (darker than stone)
const GLOW_COLOR: Rgba = Rgba::rgb(0xFF, 0xD8, 0x48); // lit gold

/// Static material glyph -> RGB. (BATTLES' `G_GLOW` is overridden per frame.)
fn color_of(ch: char) -> Rgba {
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

// ============================ font ==========================================

/// Native bold-font base bitmaps (7 shape rows + spacer). `N`/`G` widened here.
fn glyph_dots(c: char) -> [&'static str; 8] {
    match c {
        'A' => ["·###·", "#···#", "#···#", "#####", "#···#", "#···#", "#···#", "······"],
        'B' => ["###·", "#··#", "#··#", "###·", "#··#", "#··#", "###·", "····"],
        'E' => ["####", "#···", "#···", "###·", "#···", "#···", "####", "····"],
        'G' => ["·###·", "#···#", "#····", "#··##", "#···#", "#···#", "·###·", "·····"],
        'L' => ["#··", "#··", "#··", "#··", "#··", "#··", "###", "···"],
        'N' => ["#···#", "##··#", "#·#·#", "#··##", "#···#", "#···#", "#···#", "·····"],
        'S' => ["·###", "#···", "#···", "·##·", "···#", "···#", "###·", "····"],
        'T' => ["#####", "··#··", "··#··", "··#··", "··#··", "··#··", "··#··", "······"],
        _ => ["··", "··", "··", "··", "··", "··", "··", "··"],
    }
}

/// Bold weight: smear every lit dot one column right.
fn bold_matrix(c: char) -> Vec<Vec<bool>> {
    let base: Vec<Vec<bool>> = glyph_dots(c)
        .iter()
        .map(|r| r.chars().map(|ch| ch == '#').collect::<Vec<_>>())
        .collect();
    let w = base[0].len();
    base.iter()
        .map(|row| {
            let mut out = vec![false; w + 1];
            for (col, &lit) in row.iter().enumerate() {
                if lit {
                    out[col] = true;
                    out[col + 1] = true;
                }
            }
            out
        })
        .collect()
}

fn glyph_w(c: char) -> i32 {
    bold_matrix(c)[0].len() as i32 * SCALE
}

// ============================ canvas ========================================

struct Canvas {
    w: i32,
    h: i32,
    cells: Vec<char>,
}

impl Canvas {
    fn new(w: i32, h: i32) -> Self {
        Canvas { w, h, cells: vec![EMPTY; (w * h) as usize] }
    }
    fn set(&mut self, x: i32, y: i32, ch: char) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.cells[(y * self.w + x) as usize] = ch;
        }
    }
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, ch: char) {
        for j in 0..h {
            for i in 0..w {
                self.set(x + i, y + j, ch);
            }
        }
    }
    /// Stamp a bold letter (top-left at x,y). Returns advance width in dots.
    fn letter(&mut self, x: i32, y: i32, c: char, ch: char) -> i32 {
        let m = bold_matrix(c);
        for (r, row) in m.iter().enumerate() {
            for (col, &lit) in row.iter().enumerate() {
                if lit {
                    self.rect(x + col as i32 * SCALE, y + r as i32 * SCALE, SCALE, SCALE, ch);
                }
            }
        }
        m[0].len() as i32 * SCALE
    }
    fn at(&self, x: i32, y: i32) -> char {
        self.cells[(y * self.w + x) as usize]
    }
}

/// Cheap deterministic value noise.
fn noise(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(2654435761).wrapping_add((y as u32).wrapping_mul(40503));
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^ (h >> 16)
}

// ============================ sword =========================================

const BLADE_LIGHT: i32 = SCALE;
const BLADE_DARK: i32 = SCALE;
const BLADE_W: i32 = BLADE_LIGHT + BLADE_DARK;
const GRIP_W: i32 = SCALE;
const GUARD_W: i32 = 6 * SCALE;
const GUARD_H: i32 = SCALE;
const GRIP_LEN: i32 = 2 * SCALE + 2;
const POMMEL_H: i32 = SCALE;
const NECK: i32 = SCALE / 2;
const HILT_ABOVE: i32 = NECK + GUARD_H + GRIP_LEN + POMMEL_H;

/// Blade cross-section, left→right: highlight, light, dark, shadow.
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

/// Draw the sword with `cap_y` = the letter cap line for THIS frame (it moves
/// during the fall). The blade runs full length (with a tapering tip) down to
/// `body_end`+tip, but every sword dot is clipped at `clip_bottom` — the stone
/// surface — so as the sword drops the tip slots in and disappears.
fn draw_sword(cv: &mut Canvas, bl: i32, cap_y: i32, body_end: i32, clip_bottom: i32) {
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

/// White 4-point star at (cx,cy): 4 rays of length `size` at `angle`+k·90°,
/// plus a bright core. Rotating + scaling this over time gives the twinkle.
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

/// Dust puff rising/spreading from the entry point, `d` in 0..1.
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

// ============================ layout ========================================

struct Layout {
    canvas_w: i32,
    canvas_h: i32,
    gap: i32,
    word_top: i32,
    ml: i32,
    bl: i32,
    stone_x: i32,
    stone_top: i32,
    stone_w: i32,
    stone_h: i32,
    battles_x: i32,
    battles_y: i32,
    slot_bottom: i32,
    body_reach: i32,
    fall_dist: i32,
    spark_x: i32,
    spark_y: i32,
}

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

fn compute_layout() -> Layout {
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

// ============================ stone =========================================

fn stone_edge(sw: i32, sh: i32, lx: i32, ly: i32) -> bool {
    let nick = |seed: i32, s: i32| -> i32 { (noise(seed.div_euclid(6), s) % 3 == 0) as i32 };
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

fn draw_stone(cv: &mut Canvas, sx: i32, sy: i32, sw: i32, sh: i32) {
    for ly in 0..sh {
        for lx in 0..sw {
            if !stone_edge(sw, sh, lx, ly) {
                continue;
            }
            let (x, y) = (sx + lx, sy + ly);
            let cell_n = noise(x.div_euclid(2), y.div_euclid(4));
            let base = if cell_n % 11 == 0 {
                S_HI
            } else if cell_n % 9 == 0 {
                S_SH
            } else {
                S_MD
            };
            let ch = if noise(x, y) % 20 == 0 { EMPTY } else { base };
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

// ============================ frame =========================================

struct Frame {
    cv: Canvas,
    battles_color: Rgba,
    shake: (i32, i32),
}

fn shake_offset(t: f32) -> (i32, i32) {
    // Crisp 1-cell (2-dot) HORIZONTAL rattle: an even shift keeps every glyph on
    // the cell grid, so the title jitters without smearing. Decays quickly.
    if t >= LAND && t < LAND + SHAKE_DUR {
        let s = (t - LAND) / SHAKE_DUR;
        let step = ((t - LAND) / 0.03) as i32;
        let amp = if s < 0.65 { 2 } else { 0 };
        let dir = if step % 2 == 0 { 1 } else { -1 };
        (dir * amp, 0)
    } else {
        (0, 0)
    }
}

fn render_frame(l: &Layout, t: f32) -> Frame {
    let mut cv = Canvas::new(l.canvas_w, l.canvas_h);

    // AGEN (static)
    let mut cx = l.ml;
    for c in ['A', 'G', 'E', 'N'] {
        cx += cv.letter(cx, l.word_top, c, WHITE) + l.gap;
    }
    // stone
    draw_stone(&mut cv, l.stone_x, l.stone_top, l.stone_w, l.stone_h);
    // BATTLES (glyphs are 'Y'; recolored at render from etch→gold)
    let mut bx = l.battles_x;
    for (i, c) in "BATTLES".chars().enumerate() {
        bx += cv.letter(bx, l.battles_y, c, G_GLOW);
        if i + 1 < 7 {
            bx += l.gap;
        }
    }
    // sword (falling → seated)
    let p = (t / FALL_DUR).clamp(0.0, 1.0);
    let pe = p * p; // ease-in (accelerating drop)
    let cap_y = l.word_top - ((1.0 - pe) * l.fall_dist as f32).round() as i32;
    draw_sword(&mut cv, l.bl, cap_y, cap_y + l.body_reach, l.slot_bottom);

    // impact dust
    if (LAND..LAND + DUST_DUR).contains(&t) {
        draw_dust(&mut cv, l.bl + BLADE_W / 2, l.stone_top, (t - LAND) / DUST_DUR);
    }
    // two sparkles, time-offset so the 2nd pops as the 1st is shrinking: one
    // upper-left, one lower-right of the blade, spinning opposite ways.
    let sparks = [
        (SPARK_START, l.spark_x - 3, l.spark_y - 2 * SCALE, SPARK_ROT),
        (SPARK_START + SPARK_OFFSET, l.spark_x + 3, l.spark_y + 2 * SCALE, -SPARK_ROT),
    ];
    for &(start, sx, sy, rot) in &sparks {
        if (start..start + SPARK_DUR).contains(&t) {
            let sp = (t - start) / SPARK_DUR;
            let size = (sp * std::f32::consts::PI).sin() * SPARK_MAX;
            draw_sparkle(&mut cv, sx, sy, size, sp * rot);
        }
    }

    // BATTLES ignite
    let ig = ((t - IGNITE_START) / IGNITE_DUR).clamp(0.0, 1.0);
    let ige = ig * ig * (3.0 - 2.0 * ig); // smoothstep
    let battles_color = ETCH_COLOR.lerp(GLOW_COLOR, ige);

    Frame { cv, battles_color, shake: shake_offset(t) }
}

/// Render a frame to a multi-line truecolor braille string (no header).
fn frame_to_string(cv: &Canvas, battles_color: Rgba, shake: (i32, i32)) -> String {
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
    let grid = dots_to_grid_tinted(&shape, &color);
    let (br, bg, bb) = (0x12, 0x14, 0x1A);
    let mut out = String::new();
    for row in 0..grid.rows() {
        out.push_str(&format!("\x1b[48;2;{};{};{}m", br, bg, bb));
        for col in 0..grid.cols() {
            match grid.get(col, row) {
                Cell::Glyph { ch, color } => {
                    out.push_str(&format!("\x1b[38;2;{};{};{}m{}", color.r, color.g, color.b, ch));
                }
                _ => out.push('\u{2800}'),
            }
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

fn main() {
    let l = compute_layout();

    // Single-frame dump for inspection.
    if let Ok(fr) = std::env::var("FRAME") {
        let t: f32 = fr.parse().unwrap_or(ANIM_END);
        let f = render_frame(&l, t);
        println!(
            "frame t={t}  ({}x{} dots, {}x{} cells)",
            l.canvas_w,
            l.canvas_h,
            l.canvas_w.div_euclid(2),
            l.canvas_h.div_euclid(4)
        );
        print!("{}", frame_to_string(&f.cv, f.battles_color, f.shake));
        return;
    }

    // Play once, then hold on the final still.
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b[2J\x1b[?25l"); // clear + hide cursor
    let dt = 1.0 / 30.0;
    let mut t = 0.0;
    while t <= ANIM_END {
        let f = render_frame(&l, t);
        let _ = write!(out, "\x1b[H{}", frame_to_string(&f.cv, f.battles_color, f.shake));
        let _ = out.flush();
        std::thread::sleep(std::time::Duration::from_secs_f32(dt));
        t += dt;
    }
    let f = render_frame(&l, ANIM_END);
    let _ = write!(out, "\x1b[H{}\x1b[?25h\n", frame_to_string(&f.cv, f.battles_color, f.shake));
    let _ = out.flush();
}
