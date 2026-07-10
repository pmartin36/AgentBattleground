//! TRANSITIONAL capture/assert test for the button-widget-unification
//! lossless migration (b1-t1). Renders TODAY'S icon `Button` (3-arg
//! constructor) and text `FrameButton` from the shared scenario module and
//! asserts each matches its committed fixture — green against the
//! UNMODIFIED `button.rs`.
//!
//! Fixtures are (re)generated with `UPDATE_SNAPSHOTS=1` (mirrors
//! `tests/golden.rs`'s pattern), then committed. This file is DELETED by
//! b3-t1 once `Button::new`'s signature changes (the durable scenario
//! module + committed fixtures survive; b5-t1 adds the new-API verify test
//! against the same fixtures).

#[path = "support/button_scenarios.rs"]
mod scenarios;

use engine_render::{Button, FrameButton};
use ratatui::buffer::Buffer;

use scenarios::{
    buffer_to_art, deserialize_braille_buffer, fixture_path, icon_bytes, load_fixture, panel_bytes,
    frame_bytes, make_buf, serialize_braille_buffer, set_state, state_tag, ButtonState, ICON_RECT,
    LABEL, TEXT_RECT,
};

fn render_icon_button(state: ButtonState) -> Buffer {
    let mut b = Button::new(ICON_RECT, panel_bytes(), icon_bytes());
    set_state(&mut b, state);
    let mut buf = make_buf(16, 8);
    b.render(&mut buf);
    buf
}

fn render_text_frame_button(state: ButtonState) -> Buffer {
    let mut b = FrameButton::new(TEXT_RECT, frame_bytes(), LABEL);
    set_state(&mut b, state);
    let mut buf = make_buf(24, 6);
    b.render(&mut buf);
    buf
}

/// Generate (`UPDATE_SNAPSHOTS=1`) or assert one fixture. Returns the
/// rendered `Buffer` so callers can print `buffer_to_art` for the one-time
/// manual eyeball confirmation.
fn generate_or_assert(name: &str, buf: &Buffer) {
    let path = fixture_path(name);
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::write(&path, serialize_braille_buffer(buf))
            .unwrap_or_else(|e| panic!("failed to write fixture {path}: {e}"));
        return; // Fixture written — test passes.
    }
    let fixture = load_fixture(name);
    let diff = engine_render::diff_dots(&fixture, buf);
    assert!(
        diff.is_match(),
        "today's render for {name} must byte-match its committed fixture \
         (regenerate with UPDATE_SNAPSHOTS=1 if this divergence is intentional); \
         mismatches: {:?}",
        diff.mismatches
    );
}

#[test]
fn baseline_icon_button_matches_fixture_idle() {
    generate_or_assert("icon_idle", &render_icon_button(ButtonState::Idle));
}

#[test]
fn baseline_icon_button_matches_fixture_hover() {
    generate_or_assert("icon_hover", &render_icon_button(ButtonState::Hover));
}

#[test]
fn baseline_icon_button_matches_fixture_pressed() {
    generate_or_assert("icon_pressed", &render_icon_button(ButtonState::Pressed));
}

#[test]
fn baseline_text_frame_button_matches_fixture_idle() {
    generate_or_assert("text_idle", &render_text_frame_button(ButtonState::Idle));
}

#[test]
fn baseline_text_frame_button_matches_fixture_hover() {
    generate_or_assert("text_hover", &render_text_frame_button(ButtonState::Hover));
}

#[test]
fn baseline_text_frame_button_matches_fixture_pressed() {
    generate_or_assert("text_pressed", &render_text_frame_button(ButtonState::Pressed));
}

/// Guards the scenario module's own serialize/deserialize round trip,
/// independent of any button render.
#[test]
fn serialize_braille_buffer_round_trips_through_decode() {
    for state in scenarios::STATES {
        let buf = render_icon_button(state);
        let round_tripped = deserialize_braille_buffer(&serialize_braille_buffer(&buf));
        let diff = engine_render::diff_dots(&buf, &round_tripped);
        assert!(
            diff.is_match(),
            "round-tripped buffer must diff_dots-match the original exactly for state {:?}; \
             mismatches: {:?}",
            state, diff.mismatches
        );
    }
}

/// Prints the human-eyeball braille-art preview for all six scenarios, for
/// the one-time manual render-confirmation (spec line 96) BEFORE fixtures
/// are committed. Run with `cargo test --test button_migration_baseline \
/// print_all_scenarios_for_manual_eyeball -- --ignored --nocapture`.
#[test]
#[ignore]
fn print_all_scenarios_for_manual_eyeball() {
    for state in scenarios::STATES {
        println!("--- icon_{} ---", state_tag(state));
        println!("{}", buffer_to_art(&render_icon_button(state)));
        println!("--- text_{} ---", state_tag(state));
        println!("{}", buffer_to_art(&render_text_frame_button(state)));
    }
}
