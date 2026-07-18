//! Lossless-migration gate (bucket b5): asserts the post-migration,
//! default-scheme unified `Button` icon render is dot-for-dot identical, at
//! each of Idle/Hover/Pressed, to the pre-migration icon-`Button` fixture
//! captured in b1-t1. Inputs come ONLY from the shared scenario module —
//! never re-spelled — so they are byte-identical to the capture step by
//! construction.
//!
//! This is a post-migration gate, not a RED-first unit: it is expected to
//! PASS against the current, fully-migrated `Button`. A failure here means a
//! genuine lossless-migration regression.
//!
//! The equivalent text/frame-button cases were removed in menu-button-cleanup
//! b2-t2: `Button`'s frame border is now drawn procedurally (a rounded dotted
//! ring, spec 62 Decision 1) instead of the pre-migration stretched-raster
//! `FRAME_PANEL` border, so a byte-identical match against the pre-migration
//! fixture is no longer the correct invariant for frame/text buttons — the
//! shape difference is intentional, not a regression. Frame-border
//! correctness is covered instead by `button_frame_tests.rs` (corner-lit,
//! no-stray-dot, per-state tint) and `button_glyph_mask_tests.rs`. The icon
//! case (solid `BUTTON_PANEL` body) stays on the unchanged stretch-fit path
//! and remains a valid lossless gate.

#[path = "support/button_scenarios.rs"]
mod scenarios;

use engine_render::Button;
use ratatui::buffer::Buffer;
use scenarios::{icon_bytes, panel_bytes, make_buf, set_state, load_fixture, state_tag, ButtonState, ICON_RECT};

fn render_icon_button(state: ButtonState) -> Buffer {
    let mut b = Button::new(ICON_RECT, panel_bytes()).icon(icon_bytes());
    set_state(&mut b, state);
    let mut buf = make_buf(16, 8);
    b.render(&mut buf);
    buf
}

fn assert_matches(name: &str, buf: &Buffer) {
    let fixture = load_fixture(name);
    let diff = engine_render::diff_dots(&fixture, buf);
    assert!(
        diff.is_match(),
        "post-migration default-scheme render for {name} must byte-match the \
         committed b1-t1 fixture; mismatches: {:?}",
        diff.mismatches
    );
}

#[test]
fn verify_icon_button_matches_fixture_idle() {
    let state = ButtonState::Idle;
    assert_matches(
        &format!("icon_{}", state_tag(state)),
        &render_icon_button(state),
    );
}

#[test]
fn verify_icon_button_matches_fixture_hover() {
    let state = ButtonState::Hover;
    assert_matches(
        &format!("icon_{}", state_tag(state)),
        &render_icon_button(state),
    );
}

#[test]
fn verify_icon_button_matches_fixture_pressed() {
    let state = ButtonState::Pressed;
    assert_matches(
        &format!("icon_{}", state_tag(state)),
        &render_icon_button(state),
    );
}

