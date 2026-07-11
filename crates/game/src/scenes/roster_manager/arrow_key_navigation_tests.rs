use super::*;
use crate::scenes::test_util::key_event;
use crossterm::event::KeyCode;

/// Right arrow from the last index (5) wraps to 0.
#[test]
fn right_arrow_wraps_from_last_to_zero() {
    let mut scene = RosterManager::new();
    scene.current_index = 5;
    let transition = scene.handle_input(key_event(KeyCode::Right));
    assert_eq!(scene.current_index, 0, "right arrow at index 5 must wrap to 0");
    assert!(transition.is_none(), "arrow keys must not produce a Transition");
}

/// Left arrow from index 0 wraps to the last index (5).
#[test]
fn left_arrow_wraps_from_zero_to_last() {
    let mut scene = RosterManager::new();
    scene.current_index = 0;
    let transition = scene.handle_input(key_event(KeyCode::Left));
    assert_eq!(scene.current_index, 5, "left arrow at index 0 must wrap to 5");
    assert!(transition.is_none(), "arrow keys must not produce a Transition");
}

/// Right arrow from a middle index advances by 1 (non-wrap case).
#[test]
fn right_arrow_advances_without_wrap() {
    let mut scene = RosterManager::new();
    scene.current_index = 2;
    let transition = scene.handle_input(key_event(KeyCode::Right));
    assert_eq!(scene.current_index, 3, "right arrow at index 2 must advance to 3");
    assert!(transition.is_none(), "arrow keys must not produce a Transition");
}

/// A key unrelated to navigation leaves `current_index` unchanged.
#[test]
fn unrelated_key_leaves_index_unchanged() {
    let mut scene = RosterManager::new();
    scene.current_index = 2;
    let transition = scene.handle_input(key_event(KeyCode::Char('q')));
    assert_eq!(scene.current_index, 2, "unrelated key must not change current_index");
    assert!(transition.is_none(), "unrelated key must not produce a Transition");
}
