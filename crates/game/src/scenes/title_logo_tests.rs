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

/// Bottommost lit sword-material dot's y (over the canvas bounds described
/// by `l`), or `i32::MIN` if no sword material is present. Sword material is
/// any of the blade/hilt/gold/leather glyphs (b3-t2 fall/seat probes).
fn max_sword_dot_y(cv: &Canvas, l: &Layout) -> i32 {
    let mut max_y = i32::MIN;
    for y in 0..l.canvas_h {
        for x in 0..l.canvas_w {
            if matches!(
                cv.at(x, y),
                B_HI | B_LT | B_DK | B_SH | G_HI | G_MD | G_SH | P | R_LT | R_DK
            ) {
                max_y = max_y.max(y);
            }
        }
    }
    max_y
}

/// Counts cells matching material glyph `ch` over the canvas bounds
/// described by `l` (b3-t2 dust/sparkle presence probes).
fn count_material(cv: &Canvas, l: &Layout, ch: char) -> u32 {
    let mut n = 0u32;
    for y in 0..l.canvas_h {
        for x in 0..l.canvas_w {
            if cv.at(x, y) == ch {
                n += 1;
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

/// DELIVERABLE (b3-t1): the six per-beat `Beat` constants + `ANIM_END`
/// match the Decision-8 timing table exactly, ported as start/end pairs
/// (not durations).
#[test]
fn timing_block_matches_spec_table() {
    assert_eq!((SWORD_DROP.start, SWORD_DROP.end), (0.00, 0.18), "sword drop");
    assert_eq!((IMPACT_SHAKE.start, IMPACT_SHAKE.end), (0.18, 0.30), "impact shake");
    assert_eq!((IMPACT_DUST.start, IMPACT_DUST.end), (0.18, 0.38), "impact dust");
    assert_eq!((BATTLES_IGNITE.start, BATTLES_IGNITE.end), (0.18, 0.42), "battles ignite");
    assert_eq!((SPARKLE_1.start, SPARKLE_1.end), (0.46, 0.74), "sparkle #1");
    assert_eq!((SPARKLE_2.start, SPARKLE_2.end), (0.61, 0.89), "sparkle #2");
    assert_eq!(ANIM_END, 0.89, "ANIM_END must be the last beat's end");
}

/// `Beat::contains`/`Beat::progress` are half-open `[start,end)`:
/// `contains(start)` is true, `contains(end)` is false, `progress` runs
/// 0..1 across the window (verified against the real `SWORD_DROP` window).
#[test]
fn beat_contains_and_progress_are_half_open() {
    assert!(SWORD_DROP.contains(0.0), "contains(start) must be true");
    assert!(!SWORD_DROP.contains(0.18), "contains(end) must be false (half-open)");
    assert_eq!(SWORD_DROP.progress(0.0), 0.0, "progress(start) must be 0");
    assert_eq!(SWORD_DROP.progress(0.18), 1.0, "progress(end) must be 1");
    assert!(
        (SWORD_DROP.progress(0.09) - 0.5).abs() < 0.01,
        "progress(midpoint) must be ~0.5, got {}",
        SWORD_DROP.progress(0.09)
    );
}

/// DELIVERABLE (b3-t1): for ANY vertical offset, the derived sword-tip
/// start position is above the title-area top edge (fully off-screen at
/// t=0), including negative offsets.
#[test]
fn sword_starts_offscreen_for_any_vertical_offset() {
    for offset in (-8..=200).step_by(4) {
        let tip_y = sword_tip_start_y(offset);
        assert!(
            tip_y < 0,
            "sword tip start y must be off-screen (<0) for offset={offset}, got {tip_y}"
        );
    }
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

// ---- b3-t2: beat rendering (fall, impact shake, dust, ignite, sparkles, hold) ----

/// DELIVERABLE (b3-t2): mid-fall (t=0.09, inside SWORD_DROP) the sword's
/// bottommost material dot stays above the stone surface; once seated
/// (t == SWORD_DROP.end) it reaches down to the stone-surface clip line.
#[test]
fn fall_sword_is_above_stone_midfall() {
    let l = compute_layout();

    let mid = render_frame(&l, 0.09);
    let mid_max_y = max_sword_dot_y(&mid.cv, &l);
    assert!(
        mid_max_y < l.stone_top,
        "mid-fall (t=0.09) sword must stay above the stone surface, got max_y={mid_max_y}, stone_top={}",
        l.stone_top
    );

    let seated = render_frame(&l, SWORD_DROP.end);
    let seated_max_y = max_sword_dot_y(&seated.cv, &l);
    assert!(
        seated_max_y >= l.slot_bottom - 1,
        "seated sword (t={}) must reach the stone surface (slot_bottom={}), got max_y={seated_max_y}",
        SWORD_DROP.end,
        l.slot_bottom
    );
}

/// DELIVERABLE (b3-t2): the impact shake is an even (2-dot / 1-cell)
/// horizontal rattle confined to IMPACT_SHAKE's window — nonzero for at
/// least one sampled t inside the window, (0,0) outside it, and always
/// even so the shifted dots stay column-grid-aligned (no smear).
#[test]
fn impact_shake_is_even_and_windowed() {
    let mut any_nonzero = false;
    for i in 0..=12 {
        let t = IMPACT_SHAKE.start + (i as f32) * (IMPACT_SHAKE.end - IMPACT_SHAKE.start) / 12.0;
        let (dx, _dy) = shake_offset(t);
        assert_eq!(dx.rem_euclid(2), 0, "shake dx must stay even at t={t}, got {dx}");
        if dx != 0 {
            any_nonzero = true;
        }
    }
    assert!(any_nonzero, "shake must be nonzero for at least one sampled t inside the window");

    assert_eq!(shake_offset(0.10), (0, 0), "shake must be (0,0) before the window");
    assert_eq!(shake_offset(0.35), (0, 0), "shake must be (0,0) after the window");
}

/// DELIVERABLE (b3-t2): the dust puff renders only inside IMPACT_DUST's
/// window (0.18-0.38).
#[test]
fn dust_present_only_in_dust_window() {
    let l = compute_layout();
    assert!(
        count_material(&render_frame(&l, 0.28).cv, &l, DUST) > 0,
        "dust must be present mid-window (t=0.28)"
    );
    assert_eq!(
        count_material(&render_frame(&l, 0.09).cv, &l, DUST),
        0,
        "no dust before the window (t=0.09)"
    );
    assert_eq!(
        count_material(&render_frame(&l, 0.50).cv, &l, DUST),
        0,
        "no dust after the window (t=0.50)"
    );
}

/// DELIVERABLE (b3-t2): BATTLES cross-fades from the dark etch color to
/// gold across BATTLES_IGNITE — exactly ETCH_COLOR before the window,
/// strictly bracketed (every channel) mid-window, exactly GLOW_COLOR at
/// or after the window ends.
#[test]
fn battles_strictly_between_etch_and_gold_midignite() {
    let l = compute_layout();

    let before = render_frame(&l, 0.10).battles_color;
    assert_eq!(before, ETCH_COLOR, "battles_color before ignite starts must be ETCH_COLOR");

    let after = render_frame(&l, BATTLES_IGNITE.end).battles_color;
    assert_eq!(after, GLOW_COLOR, "battles_color at/after ignite ends must be GLOW_COLOR");

    let mid = render_frame(&l, 0.30).battles_color;
    assert_ne!(mid, ETCH_COLOR, "battles_color mid-ignite must not equal ETCH_COLOR");
    assert_ne!(mid, GLOW_COLOR, "battles_color mid-ignite must not equal GLOW_COLOR");
    assert!(
        mid.r > ETCH_COLOR.r && mid.r < GLOW_COLOR.r,
        "mid-ignite red channel must be strictly bracketed, got {}",
        mid.r
    );
    assert!(
        mid.g > ETCH_COLOR.g && mid.g < GLOW_COLOR.g,
        "mid-ignite green channel must be strictly bracketed, got {}",
        mid.g
    );
    assert!(
        mid.b > ETCH_COLOR.b && mid.b < GLOW_COLOR.b,
        "mid-ignite blue channel must be strictly bracketed, got {}",
        mid.b
    );
}

/// DELIVERABLE (b3-t2): each sparkle draws SPARK material only inside its
/// own window, staggered — none outside either window.
#[test]
fn sparkle_present_in_each_sparkle_window() {
    let l = compute_layout();
    assert!(
        count_material(&render_frame(&l, 0.60).cv, &l, SPARK) > 0,
        "sparkle #1 window must draw SPARK material (t=0.60)"
    );
    assert!(
        count_material(&render_frame(&l, 0.80).cv, &l, SPARK) > 0,
        "sparkle #2 window must draw SPARK material (t=0.80)"
    );
    assert_eq!(
        count_material(&render_frame(&l, 0.30).cv, &l, SPARK),
        0,
        "no sparkle outside either window (t=0.30)"
    );
}

/// DELIVERABLE (b3-t2): after ANIM_END every beat window is past, so
/// `frame(t)` holds on exactly the composed still (grid AND DotRect equal).
#[test]
fn frame_holds_on_still_after_anim_end() {
    let still = compose_still();
    assert_eq!(frame(ANIM_END), still, "frame(ANIM_END) must equal compose_still() (grid + rect)");
    assert_eq!(frame(2.0), still, "frame(t far past ANIM_END) must equal compose_still() (grid + rect)");
}
