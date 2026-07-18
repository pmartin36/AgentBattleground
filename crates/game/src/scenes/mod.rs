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
pub mod post_battle;

pub(crate) mod bars;
pub(crate) mod close_button;
pub(crate) mod home_button;
pub(crate) mod title_logo;

#[cfg(test)]
mod test_util;

pub use battle_viewer::BattleViewer;
pub use leaderboard::Leaderboard;
pub use main_hub::MainHub;
pub use roster_manager::RosterManager;
pub use post_battle::PostBattle;

/// Shared render helper: fills `area` with `color` then draws `name` centered.
/// Called by every example scene's `render` implementation.
pub(crate) fn fill_and_label(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    color: engine_core::color::Rgba,
    name: &str,
) {
    engine_render::fill(frame.buffer_mut(), area, color);
    // Dark, near-black text — every current caller's fill color (e.g.
    // Leaderboard's bright amber) is light enough that dark text reads
    // clearly; this is a placeholder helper (see module doc), not worth a
    // per-caller contrast computation.
    let color = engine_core::color::Rgba::rgb(0x10, 0x10, 0x10);
    engine_render::label(
        frame.buffer_mut(),
        area,
        name,
        engine_render::TextAlign::Center,
        ratatui::style::Style::default().fg(ratatui::style::Color::Rgb(color.r, color.g, color.b)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_id::SceneId;
    use engine_core::scene::Scene;
    use engine_core::SceneKey;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;

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
                    SceneKey::from($expected_id),
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
        leaderboard_fills_with_color_and_correct_id,
        Leaderboard,
        SceneId::Leaderboard
    );
}
