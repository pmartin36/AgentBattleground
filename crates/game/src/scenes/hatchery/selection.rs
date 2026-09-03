//! Single source-of-truth selection/edit-mode state for the hatchery scene.
//! `Hatchery::selected` names the master egg shown large; `mode`
//! distinguishes browsing (a hover target within the tray) from editing
//! (which blank is receiving input). `enter_edit` is the only constructor of
//! the `Editing` variant and always sets `selected` in the same call, so
//! "editing implies `selected.is_some()`" holds by construction — callers
//! read the edited egg through `editing_egg` rather than a second index.

use std::cell::RefCell;
use std::time::Duration;

use engine_render::{Sizing, TextEditor, TextEditorConfig};

use super::mad_lib::MadLibTemplate;

/// Browsing tracks a hover target independent of `selected`; editing tracks
/// which blank editor is receiving input. The edited egg is always
/// `selected` — see `Hatchery::editing_egg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HatcheryMode {
    Browsing { hover: usize },
    Editing { active_blank: usize },
}

/// One empty, single-line, fixed-size `TextEditor` per `t`'s blanks, with no
/// placeholder text.
fn build_blank_editors(t: &MadLibTemplate) -> Vec<RefCell<TextEditor>> {
    t.blank_labels()
        .map(|_label| {
            RefCell::new(TextEditor::new(TextEditorConfig {
                sizing: Sizing::Fixed,
                submit_on_enter: false,
                placeholder: String::new(),
            }))
        })
        .collect()
}

impl super::Hatchery {
    /// Sets the master-egg selection without entering edit mode.
    pub(crate) fn select(&mut self, index: usize) {
        self.selected = Some(index);
        self.mode = HatcheryMode::Browsing { hover: index };
    }

    /// Selects egg `index` and enters edit mode for it: builds one blank
    /// editor per its template's blanks, with the first blank active.
    pub(crate) fn enter_edit(&mut self, index: usize) {
        self.selected = Some(index);
        self.mode = HatcheryMode::Editing { active_blank: 0 };
        self.blank_editors = build_blank_editors(super::mad_lib::select_template(index));
    }

    /// Leaves edit mode back to browsing (hovering the previously-selected
    /// egg) and clears the blank editors.
    pub(crate) fn exit_edit(&mut self) {
        self.mode = HatcheryMode::Browsing { hover: self.selected.unwrap_or(0) };
        self.blank_editors.clear();
    }

    /// The egg index currently under edit, or `None` while browsing.
    pub(crate) fn editing_egg(&self) -> Option<usize> {
        matches!(self.mode, HatcheryMode::Editing { .. }).then(|| self.selected).flatten()
    }

    /// Ticks every blank editor's blink accumulator while editing; a no-op
    /// with no blank editors.
    pub(crate) fn tick_blank_editors(&self, dt: Duration) {
        for editor in &self.blank_editors {
            editor.borrow_mut().tick(dt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::mad_lib;
    use super::super::Hatchery;
    use super::HatcheryMode;

    /// `select` records the master-egg selection but stays in `Browsing` —
    /// selecting an egg for the large detail view is not the same as opening
    /// it for edit.
    #[test]
    fn select_sets_selection_without_entering_edit_mode() {
        let mut scene = Hatchery::new();

        scene.select(2);

        assert_eq!(scene.selected, Some(2), "select must record the chosen index");
        assert!(
            matches!(scene.mode, HatcheryMode::Browsing { .. }),
            "select must not enter Editing, got {:?}",
            scene.mode
        );
        assert!(scene.blank_editors.is_empty(), "select must not build any blank editors");
    }

    /// `enter_edit` selects the egg, switches to `Editing` starting on the
    /// first blank, and builds exactly one editor per the egg's template
    /// blank count.
    #[test]
    fn enter_edit_selects_egg_sets_editing_mode_and_builds_one_editor_per_blank() {
        let mut scene = Hatchery::new();

        scene.enter_edit(0);

        assert_eq!(scene.selected, Some(0), "enter_edit must select the edited egg");
        assert!(
            matches!(scene.mode, HatcheryMode::Editing { active_blank: 0 }),
            "enter_edit must start editing on the first blank, got {:?}",
            scene.mode
        );
        let expected = mad_lib::select_template(0).blank_count();
        assert_eq!(
            scene.blank_editors.len(),
            expected,
            "enter_edit must build exactly one editor per template blank"
        );
    }

    /// `exit_edit` returns to `Browsing` and clears the blank editors built
    /// by `enter_edit`.
    #[test]
    fn exit_edit_returns_to_browsing_and_clears_blank_editors() {
        let mut scene = Hatchery::new();
        scene.enter_edit(1);

        scene.exit_edit();

        assert!(
            matches!(scene.mode, HatcheryMode::Browsing { .. }),
            "exit_edit must return to Browsing, got {:?}",
            scene.mode
        );
        assert!(scene.blank_editors.is_empty(), "exit_edit must clear the blank editors");
    }

    /// `editing_egg` is `None` while browsing and reports the edited index
    /// once `enter_edit` is called.
    #[test]
    fn editing_egg_is_none_in_browsing_and_some_selected_in_editing() {
        let mut scene = Hatchery::new();
        assert_eq!(scene.editing_egg(), None, "a fresh scene starts in Browsing with no editing egg");

        scene.enter_edit(3);

        assert_eq!(scene.editing_egg(), Some(3), "editing_egg must report the egg under edit");
    }
}
