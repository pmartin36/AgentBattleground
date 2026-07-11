//! Spec 51's `PromptEditor` popup state container (b1-t1): two `TextEditor`s
//! (agent input + instructions), the close (X) `ButtonCore`, focus tracking,
//! and the write-debounce/dirty flag. Layout/render land in b2, input
//! routing + write-through land in b3.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ratatui::layout::Rect;

use engine_render::{ButtonCore, Sizing, TextEditor, TextEditorConfig};

/// Fraction of the screen width/height the popup occupies (b2-t1).
#[allow(dead_code)] // consumed by b2-t1's centered-geometry layout
const POPUP_W_FRAC: f32 = 0.8;
#[allow(dead_code)] // consumed by b2-t1's centered-geometry layout
const POPUP_H_FRAC: f32 = 0.8;
/// Minimum popup height, in cells (b2-t1).
#[allow(dead_code)] // consumed by b2-t1's centered-geometry layout
const POPUP_MIN_H: u16 = 12;
/// Idle time after the last instructions-editor edit before the pending
/// write is flushed to disk (b3-t2).
#[allow(dead_code)] // consumed by b3-t2's debounced write-through
const WRITE_DEBOUNCE: Duration = Duration::from_millis(300);
/// Cap on the agent input's grow-with-content row count (b3-t3).
const AGENT_INPUT_MAX_ROWS: u16 = 6;

/// Which of the two editors currently receives keyboard input (b3-t1's
/// Tab-cycle). Defaults to `Instructions` on open.
#[allow(dead_code)] // read by b3-t1's modal input routing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopupFocus {
    AgentInput,
    Instructions,
}

#[allow(dead_code)] // most fields consumed by b2 (layout/render) and b3 (input/write-through)
pub(super) struct PromptEditor {
    creature_index: usize,
    creature_name: String,
    instructions_base: Option<PathBuf>,
    agent_input: RefCell<TextEditor>,
    instructions: RefCell<TextEditor>,
    close_button: RefCell<ButtonCore>,
    focus: PopupFocus,
    dirty: bool,
    debounce: Duration,
}

impl PromptEditor {
    pub(super) fn new(creature_index: usize, name: &str, base: Option<&Path>) -> Self {
        let seed = crate::instructions::read_instructions_maybe(base, name).unwrap_or_default();

        let mut instructions = TextEditor::new(TextEditorConfig {
            sizing: Sizing::Fixed,
            submit_on_enter: false,
            placeholder: String::new(),
        });
        instructions.set_text(&seed);

        let agent_input = TextEditor::new(TextEditorConfig {
            sizing: Sizing::Grow {
                max_rows: AGENT_INPUT_MAX_ROWS,
            },
            submit_on_enter: true,
            placeholder: "Prompt agent to update".to_string(),
        });

        Self {
            creature_index,
            creature_name: name.to_string(),
            instructions_base: base.map(Path::to_path_buf),
            agent_input: RefCell::new(agent_input),
            instructions: RefCell::new(instructions),
            close_button: RefCell::new(ButtonCore::new(Rect::default())),
            focus: PopupFocus::Instructions,
            dirty: false,
            debounce: Duration::ZERO,
        }
    }

    /// Test-only accessor for the seeded instructions-editor text
    /// (private-field tests live in this module; kept for symmetry with
    /// `instructions_text` mentioned in research.md, in case a later task's
    /// tests need to live outside this module).
    #[cfg(test)]
    fn instructions_text(&self) -> String {
        self.instructions.borrow().text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp base dir (pid + monotonic counter), mirroring
    /// `crate::instructions`'s own test helper (instructions.rs:105).
    fn temp_base_dir(tag: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "game-prompt-editor-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ))
    }

    #[test]
    fn seed_matches_file() {
        let base = temp_base_dir("seed-matches");
        let name = "Ember Wolf";
        crate::instructions::write_instructions_in(&base, name, "# known md")
            .expect("seed write should succeed");

        let popup = PromptEditor::new(0, name, Some(&base));

        assert_eq!(popup.instructions_text(), "# known md");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_file_seeds_empty() {
        let base = temp_base_dir("seed-missing");
        let name = "Nonexistent Creature";

        let popup = PromptEditor::new(0, name, Some(&base));

        assert_eq!(popup.instructions_text(), "");

        let _ = std::fs::remove_dir_all(&base);
    }
}
