//! Lossless-migration gate (bucket b5): asserts the post-migration,
//! default-scheme unified `Button` renders (icon-only and label-only) are
//! dot-for-dot identical, at each of Idle/Hover/Pressed, to the pre-migration
//! icon-`Button`/text-`FrameButton` fixtures captured in b1-t1. Inputs come
//! ONLY from the shared scenario module — never re-spelled — so they are
//! byte-identical to the capture step by construction.
//!
//! This is a post-migration gate, not a RED-first unit: it is expected to
//! PASS against the current, fully-migrated `Button`. A failure here means a
//! genuine lossless-migration regression.

#[path = "support/button_scenarios.rs"]
mod scenarios;

use engine_render::Button;
use ratatui::buffer::Buffer;
use scenarios::{
    icon_bytes, panel_bytes, frame_bytes, make_buf, set_state, load_fixture, state_tag,
    ButtonState, ICON_RECT, LABEL, TEXT_RECT,
};

fn render_icon_button(state: ButtonState) -> Buffer {
    let mut b = Button::new(ICON_RECT, panel_bytes()).icon(icon_bytes());
    set_state(&mut b, state);
    let mut buf = make_buf(16, 8);
    b.render(&mut buf);
    buf
}

fn render_text_button(state: ButtonState) -> Buffer {
    let mut b = Button::new(TEXT_RECT, frame_bytes()).label(LABEL);
    set_state(&mut b, state);
    let mut buf = make_buf(24, 6);
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

#[test]
fn verify_text_button_matches_fixture_idle() {
    let state = ButtonState::Idle;
    assert_matches(
        &format!("text_{}", state_tag(state)),
        &render_text_button(state),
    );
}

#[test]
fn verify_text_button_matches_fixture_hover() {
    let state = ButtonState::Hover;
    assert_matches(
        &format!("text_{}", state_tag(state)),
        &render_text_button(state),
    );
}

#[test]
fn verify_text_button_matches_fixture_pressed() {
    let state = ButtonState::Pressed;
    assert_matches(
        &format!("text_{}", state_tag(state)),
        &render_text_button(state),
    );
}
