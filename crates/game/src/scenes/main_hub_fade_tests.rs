//! b4-t3 RED tests — first-run fade-in + input gating for the hub's nav
//! buttons (Decision 6). Kept out of `main_hub_tests.rs` (already at the
//! 1000-line budget) as its own `#[path]` sibling, mirroring `button.rs`'s
//! split-test-module pattern.

use super::*;
use crate::scenes::test_util::{mouse_event, render_to_buffer};
use engine_render::decode_braille_cell;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};

const W: u16 = 120;
const H: u16 = 50;

fn hub_at(elapsed: f32) -> MainHub {
    MainHub { elapsed, ..Default::default() }
}

/// Renders `scene` once (populating each `Button`'s hit-test rect via
/// `set_rect`, mirroring `main_hub_tests.rs::mouse_input_tests::render_once`)
/// and returns the fixed `area` used by `button_rects`.
fn render_once(scene: &MainHub) -> Rect {
    render_to_buffer(scene, W, H);
    Rect::new(0, 0, W, H)
}

fn center(rect: Rect) -> (u16, u16) {
    (rect.x + rect.width / 2, rect.y + rect.height / 2)
}

/// Any lit (non-space) cell inside `rect`.
fn any_painted(buf: &Buffer, rect: Rect) -> bool {
    (rect.top()..rect.bottom())
        .any(|y| (rect.left()..rect.right()).any(|x| buf.cell((x, y)).unwrap().symbol() != " "))
}

/// Decision 6: while `elapsed` is inside `title_logo::SWORD_DROP` (i.e.
/// before the fade window even starts), the nav buttons must be fully
/// invisible — no border, no label, no background dots at all.
#[test]
fn buttons_hidden_during_sword_drop() {
    let scene = hub_at(0.1);
    let buf = render_to_buffer(&scene, W, H);
    let rects = MainHub::button_rects(Rect::new(0, 0, W, H));

    for (i, rect) in rects.iter().enumerate() {
        assert!(
            !any_painted(&buf, *rect),
            "button {i} rect {rect:?} must paint no dots at elapsed={} \
             (inside SWORD_DROP, before the fade window starts)",
            scene.elapsed
        );
    }
}

/// Mid-fade, a button's border must be lit but translucent — its decoded
/// color must differ from the fully-opaque gold it renders at rest (see
/// `main_hub_tests.rs::active_color_tests`), proving the ramp actually
/// blends rather than snapping straight to opaque.
#[test]
fn buttons_partial_alpha_mid_fade() {
    let scene = hub_at(0.46);
    let buf = render_to_buffer(&scene, W, H);
    let rects = MainHub::button_rects(Rect::new(0, 0, W, H));
    let gold = crate::scenes::title_logo::GLOW_COLOR;

    // Button 1 (Battle) is not the cursored button (cursor_index defaults to
    // 0), so at full opacity it renders opaque gold.
    let rect = rects[1];
    let (bx, by) = (rect.x + rect.width / 2, rect.y);
    let (_, color) = decode_braille_cell(&buf, bx, by).unwrap_or_else(|| {
        panic!(
            "button 1 border cell ({bx},{by}) must be lit mid-fade (elapsed={})",
            scene.elapsed
        )
    });
    assert_ne!(
        color, gold,
        "mid-fade border color must be a translucent blend, not opaque gold {gold:?}"
    );
}

/// Decision 3 (b1-t3): a return-to-hub entry starts at
/// `elapsed == title_logo::ANIM_END` (t2's per-process gating on a non-first
/// entry). The VERY FIRST rendered frame — no `update` calls — must paint
/// all 4 nav buttons at full opacity (border color == the opaque base color)
/// and the input gate must already be open. Guards against the "buttons
/// stuck hidden after returning to the hub" regression.
#[test]
fn return_to_hub_first_frame_is_fully_opaque_and_interactive() {
    let scene = hub_at(crate::scenes::title_logo::ANIM_END);
    let buf = render_to_buffer(&scene, W, H);
    let rects = MainHub::button_rects(Rect::new(0, 0, W, H));
    let white = crate::scenes::title_logo::WHITE_COLOR;
    let gold = crate::scenes::title_logo::GLOW_COLOR;

    for (i, rect) in rects.iter().enumerate() {
        let (bx, by) = (rect.x + rect.width / 2, rect.y);
        let (_, color) = decode_braille_cell(&buf, bx, by).unwrap_or_else(|| {
            panic!(
                "button {i} border cell ({bx},{by}) must be lit on the very \
                 first return-to-hub frame (elapsed={})",
                scene.elapsed
            )
        });
        let expected = if i == 0 { white } else { gold };
        assert_eq!(
            color, expected,
            "button {i} must be fully opaque on the very first return-to-hub \
             frame (elapsed={})",
            scene.elapsed
        );
    }

    assert!(
        scene.buttons_interactive(),
        "return-to-hub first frame (elapsed={}) must already be interactive",
        scene.elapsed
    );
}

/// Decision 6: buttons are non-interactive until the fade completes — a
/// completed click during the pre-fade window must not activate anything.
#[test]
fn click_during_pre_fade_returns_no_transition() {
    let mut scene = hub_at(0.1);
    let area = render_once(&scene);
    let rects = MainHub::button_rects(area);
    let (cx, cy) = center(rects[0]); // Roster — would transition if interactive

    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
    let transition =
        scene.handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy));

    assert!(
        transition.is_none(),
        "a completed click during the pre-fade window (elapsed={}) must not transition",
        scene.elapsed
    );
    assert!(
        !scene.quit_requested(),
        "a completed click during the pre-fade window must not request quit"
    );
}
