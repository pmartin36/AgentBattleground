//! Slide transition (b5-t1): navigating slides the outgoing creature's
//! group off-screen in the direction of travel while the incoming
//! creature's group slides in from the opposite edge, eased via
//! `engine_render::tween`. Timings below assume the blueprint's documented
//! `SLIDE_DUR = 300ms` (research.md b5-t1): 75ms/225ms/425ms total elapsed
//! land at ~25%/~75%/past-100% progress.

use super::*;
use engine_core::scene::EngineCtx;
use crate::scenes::test_util::{key_event, render_to_buffer};
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;

/// Leftmost non-space column across every row of `rect`, if any. Used to
/// track the SPRITE region's slide position (b1-t3: only the sprite is
/// offset during a slide; name/dot-row are static and no longer a valid
/// signal for slide progress).
fn leftmost_non_space_in_rect(buf: &Buffer, rect: Rect) -> Option<u16> {
    (rect.left()..rect.right()).find(|&x| {
        (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
    })
}

/// The sprite band the slide tests measure. The arrow buttons flank the
/// dot-cluster group and so now paint in the MIDDLE columns of the frame,
/// but they live entirely within `dot_row`'s row range — disjoint from
/// (directly below) the sprite's rows — so scanning `layout(area).sprite`
/// never picks up an arrow glyph, and no column-narrowing is needed. (The
/// details-panel border shares the sprite's rightmost column at rest, but
/// the tests measure the LEFTMOST painted column, which is always sprite
/// content, so that right-edge border cell never interferes.)
fn sprite_measure_rect(area: Rect) -> Rect {
    RosterManager::layout(area).sprite
}

/// A right-nav slides the outgoing creature's SPRITE out to the left and
/// the incoming creature's SPRITE in from the right, eased over time, and
/// settles with only the incoming creature's sprite painted at its
/// resting column. Per b1-t3, name/dot-row no longer travel with the
/// slide (they update immediately with `current_index`, unchanged
/// columns throughout) — only the sprite region is exercised here.
#[test]
fn right_nav_slide_animates_and_settles() {
    let (w, h) = (80u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    let sprite_rect = sprite_measure_rect(area);

    // Resting (no-slide) column of each creature's sprite, rendered
    // standalone.
    let out_rest_left = {
        let baseline = RosterManager::new(); // index 0: Ember Wolf
        let buf = render_to_buffer(&baseline, w, h);
        leftmost_non_space_in_rect(&buf, sprite_rect)
            .expect("Ember Wolf's sprite must paint at rest")
    };
    let in_rest_left = {
        let mut baseline = RosterManager::new();
        baseline.current_index = 1; // Frost Lizard
        let buf = render_to_buffer(&baseline, w, h);
        leftmost_non_space_in_rect(&buf, sprite_rect)
            .expect("Frost Lizard's sprite must paint at rest")
    };

    let mut ctx = EngineCtx;
    let mut scene = RosterManager::new();
    let t = scene.handle_input(key_event(KeyCode::Right));
    assert!(t.is_none(), "arrow keys must not produce a Transition");
    assert_eq!(
        scene.current_index, 1,
        "current_index must update immediately on nav (b4 contract), even though a slide starts"
    );

    // Instant of trigger (no update yet): outgoing sprite still at rest.
    let buf0 = render_to_buffer(&scene, w, h);
    assert_eq!(
        leftmost_non_space_in_rect(&buf0, sprite_rect),
        Some(out_rest_left),
        "immediately after nav, the outgoing creature's sprite (Ember Wolf) must still be painted at its resting column"
    );

    // ~25% progress: outgoing sprite has slid measurably left of rest.
    scene.update(&mut ctx, Duration::from_millis(75));
    let buf1 = render_to_buffer(&scene, w, h);
    let out_mid_left = leftmost_non_space_in_rect(&buf1, sprite_rect)
        .expect("outgoing sprite must still be partially on-screen at ~25% progress");
    assert!(
        out_mid_left < out_rest_left,
        "outgoing sprite's painted column ({out_mid_left}) must have moved left of its resting column ({out_rest_left})"
    );

    // ~75% progress: incoming sprite has slid in from the right, not yet settled.
    scene.update(&mut ctx, Duration::from_millis(150)); // total elapsed 225ms
    let buf2 = render_to_buffer(&scene, w, h);
    let in_mid_left = leftmost_non_space_in_rect(&buf2, sprite_rect)
        .expect("incoming sprite must be partially visible at ~75% progress");
    assert!(
        in_mid_left > in_rest_left,
        "incoming sprite's painted column ({in_mid_left}) must still be offset right of its resting column ({in_rest_left}) mid-transition"
    );

    // Past the slide duration: only the incoming sprite remains, at rest.
    scene.update(&mut ctx, Duration::from_millis(200)); // total elapsed 425ms
    let buf3 = render_to_buffer(&scene, w, h);
    assert_eq!(
        leftmost_non_space_in_rect(&buf3, sprite_rect),
        Some(in_rest_left),
        "once settled, the incoming sprite must render at its exact resting column"
    );
}

/// Mirror of the right-nav case: a left-nav slides the outgoing
/// creature's sprite out to the right and the incoming creature's sprite
/// in from the left.
#[test]
fn left_nav_slide_animates_and_settles() {
    let (w, h) = (80u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    let sprite_rect = sprite_measure_rect(area);

    let out_rest_left = {
        let baseline = RosterManager::new(); // index 0: Ember Wolf
        let buf = render_to_buffer(&baseline, w, h);
        leftmost_non_space_in_rect(&buf, sprite_rect)
            .expect("Ember Wolf's sprite must paint at rest")
    };
    let in_rest_left = {
        let mut baseline = RosterManager::new();
        baseline.current_index = 5; // Shadow Cat (left-wrap from 0)
        let buf = render_to_buffer(&baseline, w, h);
        leftmost_non_space_in_rect(&buf, sprite_rect)
            .expect("Shadow Cat's sprite must paint at rest")
    };

    let mut ctx = EngineCtx;
    let mut scene = RosterManager::new();
    let t = scene.handle_input(key_event(KeyCode::Left));
    assert!(t.is_none(), "arrow keys must not produce a Transition");
    assert_eq!(
        scene.current_index, 5,
        "left nav from index 0 must wrap current_index to 5 immediately, even though a slide starts"
    );

    let buf0 = render_to_buffer(&scene, w, h);
    assert_eq!(
        leftmost_non_space_in_rect(&buf0, sprite_rect),
        Some(out_rest_left),
        "immediately after nav, the outgoing creature's sprite (Ember Wolf) must still be painted at its resting column"
    );

    scene.update(&mut ctx, Duration::from_millis(75));
    let buf1 = render_to_buffer(&scene, w, h);
    let out_mid_left = leftmost_non_space_in_rect(&buf1, sprite_rect)
        .expect("outgoing sprite must still be partially on-screen at ~25% progress");
    assert!(
        out_mid_left > out_rest_left,
        "outgoing sprite's painted column ({out_mid_left}) must have moved right of its resting column ({out_rest_left}) for a left-nav exit"
    );

    scene.update(&mut ctx, Duration::from_millis(150)); // total elapsed 225ms
    let buf2 = render_to_buffer(&scene, w, h);
    let in_mid_left = leftmost_non_space_in_rect(&buf2, sprite_rect)
        .expect("incoming sprite must be partially visible at ~75% progress");
    assert!(
        in_mid_left < in_rest_left,
        "incoming sprite's painted column ({in_mid_left}) must still be offset left of its resting column ({in_rest_left}) mid-transition"
    );

    scene.update(&mut ctx, Duration::from_millis(200)); // total elapsed 425ms
    let buf3 = render_to_buffer(&scene, w, h);
    assert_eq!(
        leftmost_non_space_in_rect(&buf3, sprite_rect),
        Some(in_rest_left),
        "once settled, the incoming sprite must render at its exact resting column"
    );
}

/// Per b1-t3/b2-t1: during an active slide, at the SAME `current_index`,
/// the name rect and dot-row rect paint identical COLUMNS whether or not
/// a slide is active — only the sprite region's painted columns differ
/// (still slides). This is the shared layout contract every b2 rendering
/// task depends on. The name's fg COLOUR, however, DOES change during an
/// active slide (b2-t1): it cross-fades toward the background and back,
/// keyed off the same `Slide`/`elapsed` window (colour-only, position
/// never moves). Sampled at 200ms (~67% progress) — unambiguously past
/// the 150ms/50% prev/current cross-fade boundary, so `current_index`'s
/// name is definitely the one shown, still mid-fade-in.
#[test]
fn name_and_dot_row_do_not_slide_but_sprite_does() {
    fn painted_columns(buf: &Buffer, rect: Rect) -> std::collections::BTreeSet<u16> {
        (rect.left()..rect.right())
            .filter(|&x| {
                (rect.top()..rect.bottom()).any(|y| buf.cell((x, y)).unwrap().symbol() != " ")
            })
            .collect()
    }
    fn first_painted_fg(buf: &Buffer, rect: Rect) -> Option<ratatui::style::Color> {
        (rect.top()..rect.bottom())
            .flat_map(|y| (rect.left()..rect.right()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let cell = buf.cell((x, y))?;
                if cell.symbol() != " " { Some(cell.fg) } else { None }
            })
    }
    fn channel_sum(c: ratatui::style::Color) -> u32 {
        match c {
            ratatui::style::Color::Rgb(r, g, b) => r as u32 + g as u32 + b as u32,
            _ => 0,
        }
    }

    let (w, h) = (80u16, 20u16);
    let area = Rect::new(0, 0, w, h);
    let l = RosterManager::layout(area);
    let mut ctx = EngineCtx;

    // Mid-slide render: nav right from index 0 -> 1, sample at ~67%
    // progress (200ms of the 300ms SLIDE_DUR).
    let mut mid_slide_scene = RosterManager::new();
    mid_slide_scene.handle_input(key_event(KeyCode::Right));
    mid_slide_scene.update(&mut ctx, Duration::from_millis(200));
    let mid_slide_buf = render_to_buffer(&mid_slide_scene, w, h);
    assert_eq!(mid_slide_scene.current_index, 1, "slide must not change current_index a second time");

    // No-slide render at the SAME resting current_index (1).
    let mut rest_scene = RosterManager::new();
    rest_scene.current_index = 1;
    let rest_buf = render_to_buffer(&rest_scene, w, h);

    assert_eq!(
        painted_columns(&mid_slide_buf, l.name),
        painted_columns(&rest_buf, l.name),
        "name rect's painted columns must be identical during an active slide vs. at rest — name no longer travels with col_offset"
    );
    assert_eq!(
        painted_columns(&mid_slide_buf, l.dot_row),
        painted_columns(&rest_buf, l.dot_row),
        "dot-row rect's painted columns must be identical during an active slide vs. at rest — dots no longer travel with col_offset"
    );
    assert_ne!(
        painted_columns(&mid_slide_buf, l.sprite),
        painted_columns(&rest_buf, l.sprite),
        "sprite rect's painted columns must differ during an active slide vs. at rest — the sprite is the only region that still slides"
    );

    let mid_name_fg = first_painted_fg(&mid_slide_buf, l.name)
        .expect("name rect must paint at least one cell mid-slide");
    let rest_name_fg = first_painted_fg(&rest_buf, l.name)
        .expect("name rect must paint at least one cell at rest");
    assert!(
        channel_sum(mid_name_fg) < channel_sum(rest_name_fg),
        "name fg during an active slide ({mid_name_fg:?}, sum={}) must be darker (closer to background) than its resting colour ({rest_name_fg:?}, sum={}) — b2-t1's colour cross-fade",
        channel_sum(mid_name_fg), channel_sum(rest_name_fg)
    );
}

/// A second nav fired before the first nav's slide has settled must be
/// ignored (research.md's SCOPE_QUESTION default) — `current_index`
/// reflects only the first nav.
#[test]
fn nav_ignored_during_active_slide() {
    let mut ctx = EngineCtx;
    let mut scene = RosterManager::new();
    scene.handle_input(key_event(KeyCode::Right));
    assert_eq!(scene.current_index, 1, "first nav must update current_index immediately");

    // Still well within the slide's transition window.
    scene.update(&mut ctx, Duration::from_millis(50));

    scene.handle_input(key_event(KeyCode::Right));
    assert_eq!(
        scene.current_index, 1,
        "a nav fired while a slide transition is active must be ignored until it settles"
    );
}
