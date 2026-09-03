//! Hatchery dev-only debug hotkeys: force-hatch (skip incubation on the
//! focused egg and launch the hatch sequence) and force-create-egg (append a
//! fresh `Undefined` egg to the tray). The entire module compiles only under
//! `cfg(debug_assertions)`, so neither hotkey nor its dispatch reaches a
//! release build.

use crossterm::event::KeyCode;

/// Sets the focused egg `Ready` and launches the hatch sequence on it.
pub(super) const FORCE_HATCH_KEY: KeyCode = KeyCode::Char('h');
/// Appends a fresh `Undefined` egg to the tray.
pub(super) const FORCE_CREATE_EGG_KEY: KeyCode = KeyCode::Char('e');

impl super::Hatchery {
    /// Dispatches a dev-only debug hotkey; returns whether `code` was
    /// consumed. Any other key falls through unconsumed.
    pub(super) fn handle_debug_hotkey(&mut self, code: KeyCode) -> bool {
        match code {
            FORCE_HATCH_KEY => {
                self.force_hatch_selected();
                true
            }
            FORCE_CREATE_EGG_KEY => {
                self.force_create_egg();
                true
            }
            _ => false,
        }
    }

    /// With an egg selected: records a hatch request so the next `update()`
    /// tick launches the sequence once its assets are fully generated
    /// (never forces the egg `Ready` — that would stall the clip-generation
    /// loop, which only advances an `Incubating` egg). A no-op when no egg
    /// is selected.
    fn force_hatch_selected(&mut self) {
        let Some(index) = self.selected else { return };
        if self.eggs.get(index).is_none() {
            return;
        }
        self.pending_hatch = Some(index);
    }

    /// Appends a fresh `Undefined` egg (default element, no mad-lib, no art)
    /// to the tray, keeping `art_cache`/`egg_buttons` index-aligned with
    /// `eggs`, and persists.
    fn force_create_egg(&mut self) {
        self.eggs.push(crate::player_data::Egg {
            element: crate::ability::Element::Normal,
            state: crate::player_data::EggState::Undefined,
            mad_lib: None,
            egg_art: None,
            hatchling: None,
        });
        self.art_cache.push(None);
        self.egg_buttons
            .get_mut()
            .push(engine_render::ButtonCore::new(ratatui::layout::Rect::default()));
        self.persist_eggs();
    }
}
