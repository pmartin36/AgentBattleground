//! b3-t1: the Edit button in the details panel's Instructions header opens
//! a scene-owned `Option<PromptEditor>` on a completed click (spec 48
//! Testing Guidance line 87). See research.md's blueprint — the edit
//! button is not yet wired into `render`/`handle_input`, so these are RED
//! until the code-writer wires them (mirrors selection_tests.rs's RED
//! framing).

use super::*;
use crate::scenes::test_util::{has_non_space, mouse_event, render_to_buffer};
use ratatui::crossterm::event::{MouseButton, MouseEventKind};

fn edit_button_cell_rect(area: Rect) -> Rect {
    RosterManager::panel_interior_regions(area)
        .edit_button
        .to_cell_rect()
}

fn edit_button_center(area: Rect) -> (u16, u16) {
    let cell = edit_button_cell_rect(area);
    (cell.x + cell.width / 2, cell.y + cell.height / 2)
}

/// A completed click (Moved/Down/Up) on the Edit button's slot flips
/// `prompt_editor` from `None` to `Some`.
#[test]
fn edit_button_click_opens_prompt_editor() {
    let mut scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    assert!(scene.prompt_editor.is_none());

    // Render once so the button's hit rect is set for this frame (handle_input
    // hit-tests against the PREVIOUS frame's render, per the arrow/home
    // button pattern — selection_tests.rs::click_current_dot_selects).
    let _ = render_to_buffer(&scene, w, h);

    let (cx, cy) = edit_button_center(area);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

    assert!(
        scene.prompt_editor.is_some(),
        "a completed click on the Edit button must open the prompt editor"
    );
}

/// The Edit button renders non-blank content into the Instructions
/// header's right slot (label "Edit").
#[test]
fn edit_button_renders_with_label() {
    let scene = RosterManager::new();
    let (w, h) = (80u16, 30u16);
    let area = Rect::new(0, 0, w, h);
    let buf = render_to_buffer(&scene, w, h);

    let cell = edit_button_cell_rect(area);
    assert!(
        has_non_space(&buf, cell),
        "the Edit button must render non-blank content in the instructions header's right slot"
    );
}
