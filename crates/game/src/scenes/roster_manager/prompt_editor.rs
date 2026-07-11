//! Minimal placeholder for spec 51's `PromptEditor` popup (b3-t1
//! SCOPE_QUESTION, research.md). Holds only the target creature index so
//! b3-t1's Edit-button click can flip a scene-owned `Option<PromptEditor>`
//! to `Some` without pre-building spec 51's real popup (TextEditors, X
//! button, debounce land there).

#[allow(dead_code)] // creature_index consumed by spec 51's real popup
pub(super) struct PromptEditor {
    creature_index: usize,
}

impl PromptEditor {
    pub(super) fn new(creature_index: usize) -> Self {
        Self { creature_index }
    }
}
