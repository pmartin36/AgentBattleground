//! Example scenes for the scene-switcher.
//!
//! ⚠ M1 PLACEHOLDERS. Each scene paints a single solid color plus its name so
//! a scene switch is visibly obvious — that is their whole purpose. They are
//! NOT representative of real game scenes or the real renderer. See
//! `specs/13-rendering.md` (renderer) and `specs/14-scene-architecture.md`
//! (scene model). Real scenes (battle viewer, roster manager, …) replace these.

pub mod battle_viewer;
pub mod leaderboard;
pub mod main_hub;
pub mod roster_manager;

pub use battle_viewer::BattleViewer;
pub use leaderboard::Leaderboard;
pub use main_hub::MainHub;
pub use roster_manager::RosterManager;

use scene_core::scene_id::SceneId;

/// Global dev keybind map: number keys 1–4 → the four implemented scenes.
/// Single source of truth so the mapping is never copy-pasted into each scene.
pub(crate) fn scene_for_digit(c: char) -> Option<SceneId> {
    match c {
        '1' => Some(SceneId::MainHub),
        '2' => Some(SceneId::BattleViewer),
        '3' => Some(SceneId::RosterManager),
        '4' => Some(SceneId::Leaderboard),
        _ => None,
    }
}

/// Shared render helper: fills `area` with `color` then draws `name` centered.
/// Called by every example scene's `render` implementation.
pub(crate) fn fill_and_label(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    color: scene_core::color::Rgba,
    name: &str,
) {
    render::fill(frame.buffer_mut(), area, color);
    render::label(frame.buffer_mut(), area, name);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Scene;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;
    use scene_core::scene_id::SceneId;

    /// Render a scene into a fresh TestBackend and return the resulting buffer.
    fn render_scene_to_buffer(scene: &dyn Scene, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                scene.render(f, area);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    // ------------------------------------------------------------------ colors

    /// All four scene COLOR consts must be distinct — guards against copy-paste
    /// bugs where two scenes are accidentally given the same fill color.
    #[test]
    fn all_scene_colors_are_distinct() {
        use std::collections::HashSet;
        let colors = [MainHub::COLOR, Leaderboard::COLOR];
        let mut seen: HashSet<(u8, u8, u8)> = HashSet::new();
        for (i, c) in colors.iter().enumerate() {
            assert!(
                seen.insert((c.r, c.g, c.b)),
                "scene color #{i} (r={}, g={}, b={}) is a duplicate — each scene must declare a unique COLOR",
                c.r, c.g, c.b
            );
        }
    }

    // ----------------------------------------------------------------- fill + id

    /// For each scene: `id()` returns the matching `SceneId`, and `render()` fills
    /// cell (0,0) with the braille glyph ⣿ in the scene's declared COLOR foreground.
    macro_rules! scene_fill_and_id_test {
        ($test_name:ident, $scene_ty:ty, $expected_id:expr) => {
            #[test]
            fn $test_name() {
                let scene = <$scene_ty>::default();

                assert_eq!(
                    scene.id(),
                    $expected_id,
                    "{} id() must return {:?}",
                    stringify!($scene_ty),
                    $expected_id
                );

                let buf = render_scene_to_buffer(&scene, 40, 10);
                let cell = buf
                    .cell((0, 0))
                    .expect("cell (0,0) must exist in a 40x10 buffer");
                let expected_fg = Color::Rgb(
                    <$scene_ty>::COLOR.r,
                    <$scene_ty>::COLOR.g,
                    <$scene_ty>::COLOR.b,
                );
                assert_eq!(
                    cell.symbol(),
                    "⣿",
                    "{} render must fill cell (0,0) with braille glyph ⣿",
                    stringify!($scene_ty)
                );
                assert_eq!(
                    cell.fg,
                    expected_fg,
                    "{} render cell (0,0) fg must match its declared COLOR",
                    stringify!($scene_ty)
                );
            }
        };
    }

    scene_fill_and_id_test!(
        main_hub_fills_with_color_and_correct_id,
        MainHub,
        SceneId::MainHub
    );
    scene_fill_and_id_test!(
        leaderboard_fills_with_color_and_correct_id,
        Leaderboard,
        SceneId::Leaderboard
    );

    // ------------------------------------------------------------------- label

    /// MainHub's render must place the display name "Main Hub" on the center row.
    #[test]
    fn main_hub_center_row_contains_display_name() {
        let scene = MainHub;
        let (w, h) = (40u16, 10u16);
        let buf = render_scene_to_buffer(&scene, w, h);
        let center_y = h / 2;
        let row_text: String = (0..w)
            .map(|x| buf.cell((x, center_y)).unwrap().symbol().to_string())
            .collect();
        let trimmed = row_text.trim();
        assert!(
            trimmed.contains("Main Hub"),
            "center row (y={center_y}) of MainHub render must contain the display name 'Main Hub', got: {trimmed:?}"
        );
    }
}
