#[cfg(test)]
mod render_tests {
    use super::super::*;
    use crate::scenes::test_util::render_to_buffer;
    use engine_render::decode_braille_cell;

    /// b5-t2 deliverable: the title box paints real content (frame + logo,
    /// non-space cells) and the bare display-name text "Main Hub" appears
    /// nowhere in the rendered buffer — the logo is the title, not a label.
    #[test]
    fn main_hub_title_box_paints_and_has_no_text() {
        let (w, h) = (120u16, 50u16);
        let area = Rect::new(0, 0, w, h);
        let buf = render_to_buffer(&MainHub::default(), w, h);
        let title = MainHub::title_rect(area);

        let painted = (title.top()..title.bottom()).any(|y| {
            (title.left()..title.right())
                .any(|x| buf.cell((x, y)).unwrap().symbol() != " ")
        });
        assert!(
            painted,
            "title rect {title:?} must contain at least one non-space painted cell"
        );

        let full_text: String = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !full_text.contains("Main Hub"),
            "rendered buffer must not contain the bare display-name text \"Main Hub\", got:\n{full_text}"
        );
    }

    /// b4-t1 deliverable: the 4 menu buttons render their exact label text
    /// on the center row of their respective `button_rects`, top-to-bottom
    /// in order Roster, Battle, Settings, Exit. `elapsed` is pinned past
    /// `title_logo::ANIM_END` (b4-t3's fade window has completed) since
    /// buttons paint no dots at all during the first-run fade-in.
    #[test]
    fn main_hub_renders_four_menu_button_labels() {
        let (w, h) = (120u16, 50u16);
        let area = Rect::new(0, 0, w, h);
        let scene = MainHub { elapsed: 2.0, ..Default::default() };
        let buf = render_to_buffer(&scene, w, h);
        let rects = MainHub::button_rects(area).to_vec();
        let labels = ["Roster", "Battle", "Settings", "Exit"];
        assert_eq!(
            rects.len(),
            labels.len(),
            "button_rects() must return exactly one rect per label (4: Roster, \
             Battle, Settings, Exit)"
        );

        for (rect, label) in rects.iter().zip(labels.iter()) {
            let y = rect.y + rect.height / 2;
            let row_text: String = (rect.left()..rect.right())
                .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                .collect();
            assert!(
                row_text.contains(label),
                "button rect {rect:?} center row must contain label {label:?}, got {row_text:?}"
            );
        }

        assert!(
            rects.windows(2).all(|w| w[0].y < w[1].y),
            "button rects must be strictly ordered top-to-bottom Roster, Battle, Settings, Exit"
        );
    }

    /// b4-t1 Decision 5: the selection arrow is removed — no dots may paint
    /// in the column band immediately left of ANY button rect, regardless of
    /// `cursor_index` (`cursor_index` still exists as pure selection state,
    /// it just has no visual effect until b4-t2 recolors the active button).
    #[test]
    fn no_cursor_arrow_paints_left_of_any_button() {
        let scene = MainHub {
            cursor_index: 1,
            ..Default::default()
        };
        let (w, h) = (120u16, 50u16);
        let area = Rect::new(0, 0, w, h);
        let buf = render_to_buffer(&scene, w, h);

        let rects = MainHub::button_rects(area);
        let cursor_w: u16 = 2;
        let cursor_gap: u16 = 1;
        let band_painted = |rect: Rect| -> bool {
            let x0 = rect.x.saturating_sub(cursor_gap + cursor_w);
            let x1 = rect.x.saturating_sub(cursor_gap);
            (rect.top()..rect.bottom())
                .any(|y| (x0..x1).any(|x| buf.cell((x, y)).unwrap().symbol() != " "))
        };

        for rect in rects {
            assert!(
                !band_painted(rect),
                "no arrow dots may paint left of button rect {rect:?} — the selection \
                 arrow is removed (Decision 5)"
            );
        }
    }

    /// b1-t1 deliverable (Decision 1): after the leftover title frame is
    /// dropped, no rectangular border ring frames the logo — every cell in
    /// the outermost 1-cell margin band of the title rect (`title_rect`'s
    /// own edge cells: top/bottom rows, left/right columns) is unlit.
    #[test]
    fn no_border_ring_surrounds_the_logo() {
        let (w, h) = (120u16, 50u16);
        let area = Rect::new(0, 0, w, h);
        let scene = MainHub { elapsed: 2.0, ..Default::default() };
        let buf = render_to_buffer(&scene, w, h);
        let title = MainHub::title_rect(area);

        let band_cells = (title.left()..title.right())
            .map(|x| (x, title.top()))
            .chain((title.left()..title.right()).map(|x| (x, title.bottom() - 1)))
            .chain((title.top()..title.bottom()).map(|y| (title.left(), y)))
            .chain((title.top()..title.bottom()).map(|y| (title.right() - 1, y)));

        for (x, y) in band_cells {
            assert!(
                decode_braille_cell(&buf, x, y).is_none(),
                "title margin band cell ({x},{y}) must be unlit — no border ring may \
                 frame the logo (Decision 1)"
            );
        }
    }
}

/// b4-t2 deliverable: active button (keyboard-selected via `cursor_index`,
/// or mouse-hovered — both collapse to `cursor_index`) paints border+label
/// white; every other button paints border+label gold. Opaque throughout
/// (fade is b4-t3's concern; `elapsed` is pinned past `ANIM_END` here so
/// this test is fade-independent).
#[cfg(test)]
mod active_color_tests {
    use super::super::*;
    use crate::scenes::test_util::render_to_buffer;
    use engine_core::color::Rgba;
    use engine_render::decode_braille_cell;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    /// Spec-pinned active (white) color — `title_logo`'s `#ECEFF5`.
    const WHITE: Rgba = Rgba::rgb(0xEC, 0xEF, 0xF5);

    fn rgb(c: Rgba) -> Color {
        Color::Rgb(c.r, c.g, c.b)
    }

    /// Top-center border cell of `rect`, matching the engine's own
    /// `button_frame_tests.rs::border_cell` convention.
    fn border_cell(rect: Rect) -> (u16, u16) {
        (rect.x + rect.width / 2, rect.y)
    }

    /// The fg color of the first non-space cell on `rect`'s center row (the
    /// label glyph), panicking if the button painted no label there.
    fn label_fg(buf: &Buffer, rect: Rect) -> Color {
        let y = rect.y + rect.height / 2;
        (rect.left()..rect.right())
            .find_map(|x| {
                let cell = buf.cell((x, y)).unwrap();
                (cell.symbol() != " ").then_some(cell.fg)
            })
            .unwrap_or_else(|| panic!("button rect {rect:?} must paint a label glyph on its center row"))
    }

    #[test]
    fn active_button_border_and_label_white_others_gold() {
        let scene = MainHub {
            cursor_index: 2,
            elapsed: 2.0,
            ..Default::default()
        };
        let (w, h) = (120u16, 50u16);
        let buf = render_to_buffer(&scene, w, h);
        let rects = MainHub::button_rects(Rect::new(0, 0, w, h));
        let gold = crate::scenes::title_logo::GLOW_COLOR;

        for (i, rect) in rects.iter().enumerate() {
            let (bx, by) = border_cell(*rect);
            let (_, border_color) = decode_braille_cell(&buf, bx, by)
                .unwrap_or_else(|| panic!("button {i} top-border cell ({bx},{by}) must be lit"));
            let label_color = label_fg(&buf, *rect);

            let expected = if i == scene.cursor_index { WHITE } else { gold };
            assert_eq!(
                border_color, expected,
                "button {i} border must be {}, got {border_color:?} (cursor_index={})",
                if i == scene.cursor_index { "white" } else { "gold" },
                scene.cursor_index
            );
            assert_eq!(
                label_color,
                rgb(expected),
                "button {i} label must be {}, got {label_color:?} (cursor_index={})",
                if i == scene.cursor_index { "white" } else { "gold" },
                scene.cursor_index
            );
        }
    }
}

#[cfg(test)]
mod keyboard_input_tests {
    use super::super::*;
    use crate::scenes::test_util::key_event;
    use crossterm::event::KeyCode;

    /// `elapsed` is pinned past `title_logo::ANIM_END` (b4-t3's fade window
    /// has completed) so keyboard dispatch is interactive — fade gating
    /// itself is covered in `main_hub_fade_tests.rs`.
    fn hub_at(cursor_index: usize) -> MainHub {
        MainHub {
            cursor_index,
            elapsed: 2.0,
            ..Default::default()
        }
    }

    /// Up at index 0 wraps to 3 (last index, now Exit).
    #[test]
    fn up_wraps_from_zero_to_three() {
        let mut scene = hub_at(0);
        let transition = scene.handle_input(key_event(KeyCode::Up));
        assert_eq!(scene.cursor_index, 3, "Up at index 0 must wrap to 3");
        assert!(transition.is_none(), "arrow-key navigation must not transition");
    }

    /// Down at index 3 wraps to 0.
    #[test]
    fn down_wraps_from_three_to_zero() {
        let mut scene = hub_at(3);
        let transition = scene.handle_input(key_event(KeyCode::Down));
        assert_eq!(scene.cursor_index, 0, "Down at index 3 must wrap to 0");
        assert!(transition.is_none(), "arrow-key navigation must not transition");
    }

    /// Mid-range steps do not wrap: Down 0->1, Up 1->0.
    #[test]
    fn mid_range_steps_do_not_wrap() {
        let mut scene = hub_at(0);
        scene.handle_input(key_event(KeyCode::Down));
        assert_eq!(scene.cursor_index, 1, "Down at index 0 must step to 1");

        scene.handle_input(key_event(KeyCode::Up));
        assert_eq!(scene.cursor_index, 0, "Up at index 1 must step to 0");
    }

    /// Enter at cursor_index 0 (Roster) transitions to SceneId::RosterManager.
    #[test]
    fn enter_on_roster_transitions_to_roster_manager() {
        let mut scene = hub_at(0);
        let transition = scene
            .handle_input(key_event(KeyCode::Enter))
            .expect("Enter on Roster must return a Transition");
        assert_eq!(transition.target, SceneKey::from(SceneId::RosterManager));
        assert!(!scene.quit_requested(), "Roster activation must not request quit");
    }

    /// Enter at cursor_index 1 (Battle) transitions to SceneId::BattleViewer.
    #[test]
    fn enter_on_battle_transitions_to_battle_viewer() {
        let mut scene = hub_at(1);
        let transition = scene
            .handle_input(key_event(KeyCode::Enter))
            .expect("Enter on Battle must return a Transition");
        assert_eq!(transition.target, SceneKey::from(SceneId::BattleViewer));
        assert!(!scene.quit_requested(), "Battle activation must not request quit");
    }

    /// Enter at cursor_index 2 (Settings) transitions to SceneId::Settings and
    /// does not request quit.
    #[test]
    fn enter_on_settings_transitions_to_settings() {
        let mut scene = hub_at(2);
        let transition = scene
            .handle_input(key_event(KeyCode::Enter))
            .expect("Enter on Settings must return a Transition");
        assert_eq!(transition.target, SceneKey::from(SceneId::Settings));
        assert!(!scene.quit_requested(), "Settings activation must not request quit");
    }

    /// Enter at cursor_index 3 (Exit) requests quit and returns no Transition.
    #[test]
    fn enter_on_exit_sets_quit_requested_and_no_transition() {
        let mut scene = hub_at(3);
        let transition = scene.handle_input(key_event(KeyCode::Enter));
        assert!(transition.is_none(), "Exit activation must not return a Transition");
        assert!(scene.quit_requested(), "Exit activation must set quit_requested");
    }

    /// Space activates the cursored item identically to Enter.
    #[test]
    fn space_on_roster_transitions_to_roster_manager() {
        let mut scene = hub_at(0);
        let transition = scene
            .handle_input(key_event(KeyCode::Char(' ')))
            .expect("Space on Roster must return a Transition");
        assert_eq!(transition.target, SceneKey::from(SceneId::RosterManager));
        assert!(!scene.quit_requested(), "Roster activation must not request quit");
    }

    /// An unrelated key changes nothing and requests nothing.
    #[test]
    fn unrelated_key_is_noop() {
        let mut scene = hub_at(0);
        let transition = scene.handle_input(key_event(KeyCode::Char('x')));
        assert_eq!(scene.cursor_index, 0, "unrelated key must not move the cursor");
        assert!(transition.is_none(), "unrelated key must not transition");
        assert!(!scene.quit_requested(), "unrelated key must not request quit");
    }
}

#[cfg(test)]
mod mouse_input_tests {
    use super::super::*;
    use crate::scenes::test_util::{mouse_event, render_to_buffer};
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    use engine_render::ButtonState;

    const W: u16 = 120;
    const H: u16 = 50;

    /// Renders `scene` once (populating each `Button`'s rect via
    /// `set_rect`, per b5-t6 research's "render before dispatch" note) and
    /// returns the fixed `area` used by `button_rects`.
    fn render_once(scene: &MainHub) -> Rect {
        render_to_buffer(scene, W, H);
        Rect::new(0, 0, W, H)
    }

    /// A hub seated past `title_logo::ANIM_END`, i.e. past b4-t3's fade
    /// window, so mouse dispatch is interactive for these click/hover
    /// cases — fade gating itself is covered in `main_hub_fade_tests.rs`.
    fn interactive_hub() -> MainHub {
        MainHub { elapsed: 2.0, ..Default::default() }
    }

    fn center(rect: Rect) -> (u16, u16) {
        (rect.x + rect.width / 2, rect.y + rect.height / 2)
    }

    /// A `Moved` landing inside the Battle button's rect (index 1) sets
    /// `cursor_index` to 1 and produces no transition.
    #[test]
    fn moved_inside_battle_sets_cursor_index_without_transition() {
        let mut scene = interactive_hub();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[1]);

        let transition = scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));

        assert_eq!(
            scene.cursor_index, 1,
            "Moved inside the Battle button's rect must set cursor_index to 1"
        );
        assert!(transition.is_none(), "hover must not produce a Transition");
        assert_eq!(
            scene.buttons[1].borrow().state(),
            ButtonState::Hover,
            "the Battle button itself must report Hover state after the Moved event"
        );
    }

    /// A completed click (Moved+Down+Up, all inside the Settings button's
    /// rect) returns the same Transition Enter produces at cursor_index 2.
    #[test]
    fn click_on_settings_transitions_to_settings() {
        let mut scene = interactive_hub();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[2]);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            cx,
            cy,
        ));
        let transition = scene
            .handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy))
            .expect("a completed click on Settings must return a Transition");

        assert_eq!(transition.target, SceneKey::from(SceneId::Settings));
        assert!(!scene.quit_requested(), "Settings activation must not request quit");
    }

    /// A completed click (Moved+Down+Up, all inside the Exit button's rect)
    /// fires the quit signal via `activate`, with no transition.
    #[test]
    fn click_on_exit_requests_quit() {
        let mut scene = interactive_hub();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area).to_vec();
        let (cx, cy) = center(*rects.last().expect("button_rects must be non-empty"));

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            cx,
            cy,
        ));
        let transition = scene.handle_input(mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            cx,
            cy,
        ));

        assert!(
            transition.is_none(),
            "a completed click on Exit must not return a Transition"
        );
        assert!(
            scene.quit_requested(),
            "a completed click on Exit must set quit_requested"
        );
    }

    /// A completed click on the Roster button returns the same Transition
    /// Enter produces at cursor_index 0 — the shared `activate` dispatch.
    #[test]
    fn click_on_roster_transitions_to_roster_manager() {
        let mut scene = interactive_hub();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[0]);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            cx,
            cy,
        ));
        let transition = scene
            .handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy))
            .expect("a completed click on Roster must return a Transition");

        assert_eq!(transition.target, SceneKey::from(SceneId::RosterManager));
        assert!(!scene.quit_requested(), "Roster activation must not request quit");
    }

    /// A completed click on the Battle button returns the same Transition
    /// Enter produces at cursor_index 1.
    #[test]
    fn click_on_battle_transitions_to_battle_viewer() {
        let mut scene = interactive_hub();
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[1]);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            cx,
            cy,
        ));
        let transition = scene
            .handle_input(mouse_event(MouseEventKind::Up(MouseButton::Left), cx, cy))
            .expect("a completed click on Battle must return a Transition");

        assert_eq!(transition.target, SceneKey::from(SceneId::BattleViewer));
        assert!(!scene.quit_requested(), "Battle activation must not request quit");
    }

    /// b4-t2 deliverable: hovering a different button moves the active
    /// (white) color to the hovered button — `cursor_index` is the single
    /// active predicate, so a `Moved` sync is enough, no separate hover
    /// color path. Reuses `active_color_tests`' border-cell convention.
    #[test]
    fn hover_moves_white_to_hovered_button() {
        use engine_core::color::Rgba;
        use engine_render::decode_braille_cell;

        const WHITE: Rgba = Rgba::rgb(0xEC, 0xEF, 0xF5);
        fn border_cell(rect: Rect) -> (u16, u16) {
            (rect.x + rect.width / 2, rect.y)
        }

        let mut scene = MainHub {
            elapsed: 2.0,
            ..Default::default()
        };
        let area = render_once(&scene);
        let rects = MainHub::button_rects(area);
        let (cx, cy) = center(rects[1]);

        scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
        assert_eq!(scene.cursor_index, 1, "Moved onto button 1 must set cursor_index to 1");

        let buf = render_to_buffer(&scene, W, H);
        let (bx0, by0) = border_cell(rects[0]);
        let (_, color0) = decode_braille_cell(&buf, bx0, by0)
            .expect("button 0 top-border cell must be lit");
        let (bx1, by1) = border_cell(rects[1]);
        let (_, color1) = decode_braille_cell(&buf, bx1, by1)
            .expect("button 1 top-border cell must be lit");

        assert_eq!(color1, WHITE, "hovered button 1 must be white, got {color1:?}");
        assert_ne!(color0, WHITE, "non-hovered button 0 must not be white, got {color0:?}");
    }

    /// A click sequence completed at a point outside all 3 button rects is a
    /// no-op: cursor_index unchanged, no transition, no quit.
    #[test]
    fn click_outside_all_buttons_is_noop() {
        let mut scene = MainHub {
            cursor_index: 0,
            elapsed: 2.0,
            ..Default::default()
        };
        let _area = render_once(&scene);
        // Top-left corner of the screen — outside the centered title/menu.
        let (ox, oy) = (0u16, 0u16);

        scene.handle_input(mouse_event(MouseEventKind::Moved, ox, oy));
        scene.handle_input(mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            ox,
            oy,
        ));
        let transition = scene.handle_input(mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            ox,
            oy,
        ));

        assert_eq!(
            scene.cursor_index, 0,
            "a click sequence outside all button rects must not move cursor_index"
        );
        assert!(transition.is_none(), "a click outside all buttons must not transition");
        assert!(
            !scene.quit_requested(),
            "a click outside all buttons must not request quit"
        );
    }
}

#[cfg(test)]
mod layout_tests {
    use super::super::*;
    use engine_render::{flex, Align, Basis, Direction, FlexChild, FlexStyle, Justify};

    /// Fixed screen area used by every case in this module.
    fn area() -> Rect {
        Rect::new(0, 0, 120, 50)
    }

    /// `title_rect` is exactly a single-child `flex()` Row (Justify::Center,
    /// Align::Start) over `cell_rect_to_dots(area)` inset by 1 cell (4 dots)
    /// off the top edge, `.to_cell_rect()`'d — the mechanism, not
    /// hand-derived arithmetic.
    #[test]
    fn title_rect_is_top_center_anchor() {
        let a = area();
        let (w, h) = MainHub::title_size(a);
        let child = FlexChild {
            basis: Basis::Intrinsic(Box::new(move |_main| (w as i32 * 2, h as i32 * 4))),
            grow: 0.0,
            shrink: 0.0,
        };
        let style = FlexStyle {
            direction: Direction::Row,
            justify_content: Justify::Center,
            align_items: Align::Start,
            gap: 0,
        };
        let container = MainHub::cell_rect_to_dots(a);
        let expected = flex(container, style, std::slice::from_ref(&child))[0].to_cell_rect();
        assert_eq!(MainHub::title_rect(a), expected);
    }

    /// The title box's top edge sits flush at the top of `area` — the former
    /// 1-cell top margin was removed (title moved up 1 cell).
    #[test]
    fn title_rect_is_flush_at_top() {
        let a = area();
        let title = MainHub::title_rect(a);
        assert_eq!(
            title.y, a.y,
            "title box top edge must sit flush at the top of area (no top margin)"
        );
    }

    /// b4-t1: the title box is now a FIXED size (the procedural logo's own
    /// cell dims + a 1-cell border each side), independent of the render
    /// area — supersedes the old aspect-fit "large fraction of the screen"
    /// contract (LOGO_ASPECT/TITLE_W_FRAC/TITLE_H_MAX_FRAC are retired).
    #[test]
    fn title_size_is_fixed_regardless_of_area() {
        let small = MainHub::title_size(Rect::new(0, 0, 40, 20));
        let large = MainHub::title_size(Rect::new(0, 0, 120, 50));
        assert_eq!(
            small, large,
            "title box size must be fixed, independent of the render area"
        );
        assert_eq!(
            small,
            (96, 26),
            "title box size must equal the logo's fixed cell dims (94x24, \
             div_ceil of compute_layout()'s 188x94 dot canvas) + a 1-cell \
             border each side"
        );
    }

    /// `menu_container`'s output for the two b1 fixture geometries (120x50
    /// and 40x20, covering both x-parity paths) — a direct value assertion
    /// beyond the two existing property-only tests below, which would not
    /// catch an off-by-margin/axis slip in the inset/Justify::End formula.
    #[test]
    fn menu_container_matches_pre_migration_rect() {
        // MENU_BOTTOM_MARGIN is now 4 (was 2): the menu group sits 2 cells
        // higher, so each y drops by 2 (33->31, 3->1).
        assert_eq!(MainHub::menu_container(Rect::new(0, 0, 120, 50)), Rect::new(50, 31, 20, 15));
        assert_eq!(MainHub::menu_container(Rect::new(0, 0, 40, 20)), Rect::new(10, 1, 20, 15));
    }

    /// The menu sits near the BOTTOM of the screen (Exit close to the
    /// bottom edge), not dead-center — the fix for the "menu floats in the
    /// middle with a huge empty gap above the tiny title" complaint.
    #[test]
    fn menu_container_is_anchored_near_the_bottom() {
        let a = area();
        let container = MainHub::menu_container(a);
        let title = MainHub::title_rect(a);
        assert!(
            container.bottom() > a.bottom() * 3 / 4,
            "menu container bottom {} must be in the bottom quarter of the screen (bottom={})",
            container.bottom(),
            a.bottom()
        );
        assert!(
            container.y > title.bottom(),
            "menu container must sit below the title box, with the freed vertical space between them"
        );
    }

    /// `button_rects` is exactly a `flex()` mechanism (Column, 3 Intrinsic
    /// children, `gap: MENU_GAP*4`, Justify::Start, Align::Start) applied
    /// over `cell_rect_to_dots(menu_container(area))` — proves the 3 button
    /// rects come from the flex mechanism applied to `menu_container`'s own
    /// output, not independently derived (research.md b3-t2 blueprint).
    #[test]
    fn button_rects_match_stack_of_menu_container() {
        let a = area();
        let container = MainHub::cell_rect_to_dots(MainHub::menu_container(a));
        let child = || FlexChild {
            basis: Basis::Intrinsic(Box::new(|_main| {
                (MainHub::BUTTON_H as i32 * 4, MainHub::BUTTON_W as i32 * 2)
            })),
            grow: 0.0,
            shrink: 0.0,
        };
        let children = [child(), child(), child(), child()];
        let style = FlexStyle {
            direction: Direction::Column,
            justify_content: Justify::Start,
            align_items: Align::Start,
            gap: MainHub::MENU_GAP as i32 * 4,
        };
        let expected: Vec<Rect> =
            flex(container, style, &children).iter().map(|r| r.to_cell_rect()).collect();
        assert_eq!(MainHub::button_rects(a).as_slice(), expected.as_slice());
    }

    /// The 4 button rects are strictly ordered top-to-bottom (index 0
    /// Roster, 1 Battle, 2 Settings, 3 Exit) and non-overlapping.
    #[test]
    fn button_rects_are_ordered_and_non_overlapping() {
        let rects = MainHub::button_rects(area()).to_vec();
        assert_eq!(rects.len(), 4, "button_rects() must return exactly 4 rects");

        for pair in rects.windows(2) {
            assert!(pair[0].y < pair[1].y, "each rect must be above the next: {pair:?}");
            assert!(
                pair[0].bottom() <= pair[1].y,
                "consecutive rects must not overlap: {pair:?}"
            );
        }
    }

    /// `menu_container`'s height must be at least the stacked group's total
    /// height (4 buttons + 3 gaps) — guards against the container being too
    /// short for the button column.
    #[test]
    fn menu_container_height_fits_four_buttons() {
        let container = MainHub::menu_container(area());
        assert!(
            container.height >= 4 * MainHub::BUTTON_H + 3 * MainHub::MENU_GAP,
            "menu_container height {} must fit 4 buttons + 3 gaps",
            container.height
        );
    }
}

#[cfg(test)]
mod animation_clock_tests {
    use super::super::*;
    use crate::scenes::test_util::{render_to_buffer, serialize_braille_buffer};
    use engine_core::scene::EngineCtx;

    /// b4-t1 deliverable: the rendered logo changes across successive
    /// `update(dt)` calls over ~0.9s from scene entry (the intro plays),
    /// then holds on the same still once past `title_logo::ANIM_END`
    /// (buffers at ~1.0s and ~2.0s must be identical).
    #[test]
    fn hub_logo_animates_then_holds() {
        let (w, h) = (120u16, 50u16);
        let mut scene = MainHub::default();
        let mut ctx = EngineCtx;

        // Sample DURING the animation (after the pre-roll, mid sword-drop) and
        // then well past ANIM_END — both derived from the constants so this
        // survives PREROLL tuning.
        let pre = crate::scenes::title_logo::PREROLL;
        let anim_end = crate::scenes::title_logo::ANIM_END;
        let buf_start = render_to_buffer(&scene, w, h); // t=0, pre-roll held
        scene.update(&mut ctx, Duration::from_secs_f32(pre + 0.10)); // mid sword-drop
        let buf_mid = render_to_buffer(&scene, w, h);
        scene.update(&mut ctx, Duration::from_secs_f32(anim_end)); // now well past ANIM_END
        let buf_settled_a = render_to_buffer(&scene, w, h);
        scene.update(&mut ctx, Duration::from_secs_f32(1.0)); // further past
        let buf_settled_b = render_to_buffer(&scene, w, h);

        assert_ne!(
            serialize_braille_buffer(&buf_start),
            serialize_braille_buffer(&buf_mid),
            "hub render ~0.30s into entry must differ from the entry frame (t=0) \
             — the logo must animate, not sit static"
        );
        assert_eq!(
            serialize_braille_buffer(&buf_settled_a),
            serialize_braille_buffer(&buf_settled_b),
            "hub render must hold on the same still once past ANIM_END \
             (~1.0s and ~2.0s renders must be identical)"
        );
    }
}

#[cfg(test)]
mod render_timing_tests {
    use super::super::*;
    use crate::scenes::test_util::render_to_buffer;
    use std::time::Instant;

    /// Spec 30 done-criterion, closed by spec 32: re-measure `MainHub::render()`
    /// after the rasterize-cache fix (b3-t1/b4-t1/b6-t1) and confirm it is a
    /// small fraction of the 33ms (30fps) frame budget, in BOTH profiles.
    /// Ignored by default (it is a measurement, run explicitly and recorded as
    /// evidence; it also keeps the debug gate fast).
    ///
    /// Measured (2026-07-04, this machine): release avg ~0.26-2.5ms, debug avg
    /// ~4.0ms — both a small fraction of the budget. Pre-spec-32, debug avg was
    /// ~161ms (`engine_render::convert()` re-rasterizing `logo.png` every
    /// frame); spec 32's shared rasterize cache (`asset_cache`) closed that
    /// gap, so the debug bound is now asserted too. b4-t1 replaced the PNG
    /// render with `title_logo::frame()`, which recomposes procedurally
    /// (no `asset_cache` rasterization involved for the logo).
    #[test]
    #[ignore = "timing measurement; run explicitly (see spec 30/32 done-criterion)"]
    fn main_hub_render_is_a_small_fraction_of_frame_budget() {
        const N: u32 = 50;
        /// Flake-resistant ceiling well under 33ms; measured release avg ~2.5ms.
        const RELEASE_BUDGET_MS: f64 = 10.0;
        /// Full 30fps frame budget; ~8x headroom over the measured ~4ms debug
        /// avg, but still catches a regression back toward the pre-spec-32
        /// ~161ms (uncached re-rasterization).
        const DEBUG_BUDGET_MS: f64 = 33.0;

        let scene = MainHub::default();
        for _ in 0..3 {
            let _ = render_to_buffer(&scene, 120, 50);
        } // warmup
        let t = Instant::now();
        for _ in 0..N {
            let _ = render_to_buffer(&scene, 120, 50);
        }
        let avg_ms = t.elapsed().as_secs_f64() * 1000.0 / f64::from(N);

        let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
        println!("MainHub::render() avg = {avg_ms:.3} ms over {N} ({profile})");

        if cfg!(debug_assertions) {
            assert!(
                avg_ms < DEBUG_BUDGET_MS,
                "debug MainHub::render() avg {avg_ms:.3}ms must be a small fraction \
                 of the 33ms frame budget (ceiling {DEBUG_BUDGET_MS}ms; pre-spec-32 \
                 this was ~161ms uncached)"
            );
        } else {
            assert!(
                avg_ms < RELEASE_BUDGET_MS,
                "release MainHub::render() avg {avg_ms:.3}ms must be a small fraction \
                 of the 33ms frame budget (ceiling {RELEASE_BUDGET_MS}ms)"
            );
        }
    }
}

/// Golden-fixture gate. Freezes the reference `MainHub` render at 3
/// deterministic scenarios into committed fixtures under
/// `crates/game/tests/fixtures/main_hub/`, decoded through the exact
/// `decode_braille_cell`/`diff_dots` channel live code must match
/// dot-for-dot. Mirrors `roster_manager.rs`'s `golden_fixture_tests`
/// precedent.
#[cfg(test)]
mod golden_fixture_tests {
    use super::super::*;
    use crate::scenes::test_util::{
        buffer_to_art, load_main_hub_fixture, render_to_buffer, serialize_braille_buffer,
    };
    use engine_render::diff_dots;

    /// (fixture name, render width, render height, scene-mutation fn).
    type Scenario = (&'static str, u16, u16, fn(&mut MainHub));

    /// The 3 deterministic scenarios: fixture name, render dims, and the
    /// mutation applied to a fresh `MainHub::default()` before render. Each
    /// sets `elapsed` past `title_logo::ANIM_END` so the fixtures freeze the
    /// SETTLED procedural logo still, not an animation frame.
    fn scenarios() -> Vec<Scenario> {
        fn rest(scene: &mut MainHub) {
            scene.elapsed = 2.0;
        }
        fn cursor_exit(scene: &mut MainHub) {
            scene.cursor_index = 3;
            scene.elapsed = 2.0;
        }

        vec![
            ("rest_120x50", 120, 50, rest as fn(&mut MainHub)),
            ("narrow_40x20", 40, 20, rest as fn(&mut MainHub)),
            ("cursor_exit_120x50", 120, 50, cursor_exit as fn(&mut MainHub)),
        ]
    }

    /// For each of the 3 scenarios, the CURRENT render must dot-for-dot
    /// match its committed fixture (`diff_dots(&fixture, &actual).is_match()`).
    /// b4-t1 re-baselines these fixtures to the procedural sword-in-stone
    /// logo held on its SETTLED still (`elapsed = 2.0`, past
    /// `title_logo::ANIM_END`), replacing the retired PNG render.
    ///
    /// Run with `UPDATE_MAIN_HUB_FIXTURES=1` to (re)generate the 3
    /// `*.fixture` + `*.preview.txt` files from the current render — do the
    /// manual visual pass over the previews (recorded in
    /// `crates/game/tests/fixtures/main_hub/README.md`) BEFORE committing
    /// regenerated fixtures.
    #[test]
    fn main_hub_golden_fixtures_match_pre_migration_baseline() {
        let generate = std::env::var("UPDATE_MAIN_HUB_FIXTURES").is_ok();

        for (name, w, h, build) in scenarios() {
            let mut scene = MainHub::default();
            build(&mut scene);
            let actual = render_to_buffer(&scene, w, h);

            if generate {
                let fixture_path = format!(
                    "{}/tests/fixtures/main_hub/{name}.fixture",
                    env!("CARGO_MANIFEST_DIR")
                );
                let preview_path = format!(
                    "{}/tests/fixtures/main_hub/{name}.preview.txt",
                    env!("CARGO_MANIFEST_DIR")
                );
                std::fs::write(&fixture_path, serialize_braille_buffer(&actual))
                    .unwrap_or_else(|e| panic!("failed to write {fixture_path}: {e}"));
                std::fs::write(&preview_path, buffer_to_art(&actual))
                    .unwrap_or_else(|e| panic!("failed to write {preview_path}: {e}"));
                continue;
            }

            let fixture = load_main_hub_fixture(name);
            let diff = diff_dots(&fixture, &actual);
            assert!(
                diff.is_match(),
                "scenario {name:?}: current render diverges from the committed \
                 pre-migration fixture ({} dot mismatch(es) of {} compared); \
                 regenerate with UPDATE_MAIN_HUB_FIXTURES=1 only if the divergence \
                 is intentional (re-run the manual visual pass first)",
                diff.mismatches.len(),
                diff.dots_compared
            );
        }
    }
}

#[cfg(test)]
mod first_run_gate_tests {
    use super::super::*;
    use engine_core::scene::EngineCtx;

    /// b1-t2 deliverable: `MainHub::enter` gates the intro on a process-
    /// scoped `INTRO_PLAYED` latch, not on persisted filesystem/env-var
    /// state. Single test owns the latch end-to-end (process-global) so it
    /// can't race any other test — no other test in this binary calls
    /// `enter` or `reset_intro_played_for_test`.
    #[test]
    fn first_entry_in_process_animates_then_second_entry_holds_still() {
        reset_intro_played_for_test();

        // (a) first entry of the process: intro must play from t=0.
        let mut first = MainHub::default();
        let mut ctx = EngineCtx;
        first.enter(&mut ctx, None);
        assert_eq!(
            first.elapsed, 0.0,
            "first entry of the process must start the intro at t=0"
        );

        // (b) a later entry in the SAME process: must seat straight on the
        // held final still, no animation, no filesystem/env-var involved.
        let mut second = MainHub::default();
        second.enter(&mut ctx, None);
        assert!(
            second.elapsed >= crate::scenes::title_logo::ANIM_END,
            "a later entry in the same process must seat elapsed at/after \
             ANIM_END (held still), got {}",
            second.elapsed
        );
    }
}
