//! The roster's Hatchery entry button renders and, on a completed click,
//! transitions to the Hatchery scene.

use super::*;
use crate::scenes::test_util::{has_non_space, mouse_event, render_to_buffer};
use ratatui::crossterm::event::{MouseButton, MouseEventKind};

fn hatchery_button_center(area: Rect) -> (u16, u16) {
    let rect = RosterManager::hatchery_button_rect(area);
    (rect.x + rect.width / 2, rect.y + rect.height / 2)
}

/// `render()` paints the Hatchery button with non-blank content.
#[test]
fn hatchery_button_renders_with_label() {
    let scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let buf = render_to_buffer(&scene, w, h);

    let area = Rect::new(0, 0, w, h);
    let rect = RosterManager::hatchery_button_rect(area);
    assert!(
        has_non_space(&buf, rect),
        "the Hatchery button must paint at least one non-space cell within its rect"
    );
}

/// A completed click (Moved+Down+Up, all inside the Hatchery button's rect)
/// returns a `Transition` to `Hatchery` with no params.
#[test]
fn hatchery_button_click_transitions_to_hatchery() {
    let mut scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    // Render once so the button's rect is set for this frame (handle_input
    // hit-tests against the previous frame's render, per the home-button
    // pattern).
    let _ = render_to_buffer(&scene, w, h);

    let area = Rect::new(0, 0, w, h);
    let (cx, cy) = hatchery_button_center(area);

    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
    let t = scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

    let t = t.expect("a completed click on the Hatchery button must return a Transition");
    assert_eq!(
        t.target,
        SceneKey::from(SceneId::Hatchery),
        "Hatchery button must transition to Hatchery"
    );
    assert!(t.params.is_none(), "Hatchery button transition must carry no params");
}
