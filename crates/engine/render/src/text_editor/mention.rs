//! `@`-mention autocomplete: contract + storage (b1-t1) and the popup state
//! machine driven from `TextEditor::dispatch_key` (b1-t2).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{EditorEvent, Pos, TextEditor};

/// One candidate offered by a [`MentionProvider`] for the current query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionCandidate {
    pub display: String,
    pub insert_text: String,
    pub category: &'static str,
    /// `true` = accepting inserts `insert_text` and KEEPS the popup open
    /// (two-stage target->selector flow); `false` = terminal: accepting
    /// inserts and closes.
    pub continues: bool,
}

/// Source of `@`-mention candidates for a [`TextEditor`]. Engine-reusable;
/// concrete game vocabularies implement this (e.g. `crates/game/src/mention.rs`).
pub trait MentionProvider {
    fn candidates(&self, query: &str) -> Vec<MentionCandidate>;
}

impl TextEditor {
    /// Attach a mention autocomplete provider + trigger char. Stored as
    /// `Some((provider, trigger))`. Mirrors `set_clipboard`.
    pub fn set_mention_provider(&mut self, provider: Box<dyn MentionProvider>, trigger: char) {
        self.mention_provider = Some((provider, trigger));
    }

    /// Whether a `@`-mention popup is currently open on this editor. Lets an
    /// embedding modal (which may bind its own key, e.g. Esc, over the
    /// editor) know the editor wants first refusal on that key while the
    /// popup is capturing it.
    pub fn mention_active(&self) -> bool {
        self.mention_popup.is_some()
    }
}

/// Open mention-autocomplete popup state: `anchor` is the logical position
/// of the trigger char, `candidates` is the provider's current result for
/// the derived query, `highlight` indexes into `candidates`. Fields are
/// `pub(super)` so b1-t3's render/mouse code (and tests) can read them.
/// b1-t2.
pub(super) struct MentionPopup {
    pub(super) anchor: Pos,
    pub(super) candidates: Vec<MentionCandidate>,
    pub(super) highlight: usize,
    /// On-screen cell rect of the candidate list, set by `draw_mention_popup`
    /// during `render()`; `None` until the popup has been rendered at least
    /// once. Read back by `mention_mouse_accept` for click hit-testing. b1-t3.
    pub(super) list_rect: Option<ratatui::layout::Rect>,
}

// b1-t2: popup state machine (open/query/nav/accept/dismiss), dispatched
// from `TextEditor::dispatch_key`'s two hooks. Bodies are unimplemented
// stubs owned by the test-writer so `mention_tests.rs` compiles and fails
// at the assertion/panic level (not a missing-symbol build error); the
// code-writer fills these in against research.md's blueprint.
impl TextEditor {
    /// The configured trigger char, if a provider is attached.
    pub(super) fn mention_trigger(&self) -> Option<char> {
        self.mention_provider.as_ref().map(|(_, trigger)| *trigger)
    }

    /// Current query: chars from `anchor` (exclusive of the trigger char)
    /// through the caret, on the anchor's line.
    pub(super) fn mention_query(&self) -> String {
        let Some(popup) = self.mention_popup.as_ref() else {
            return String::new();
        };
        let (anchor_line, anchor_col) = popup.anchor;
        if self.cursor_line != anchor_line || self.cursor_col <= anchor_col {
            return String::new();
        }
        let line: Vec<char> = self.lines[anchor_line].chars().collect();
        let start = anchor_col + 1;
        let end = self.cursor_col.min(line.len());
        line.get(start..end)
            .map(|s| s.iter().collect())
            .unwrap_or_default()
    }

    /// Query the attached provider, or an empty result when none is
    /// attached (defensive; callers only reach here when a provider is
    /// known to be set).
    fn mention_query_provider(&self, query: &str) -> Vec<MentionCandidate> {
        self.mention_provider
            .as_ref()
            .map(|(provider, _)| provider.candidates(query))
            .unwrap_or_default()
    }

    /// Open the popup at the caret (one position after the just-typed
    /// trigger char) and issue the initial empty-query request.
    pub(super) fn mention_open(&mut self) {
        let anchor: Pos = (self.cursor_line, self.cursor_col - 1);
        let candidates = self.mention_query_provider("");
        self.mention_popup = Some(MentionPopup {
            anchor,
            candidates,
            highlight: 0,
            list_rect: None,
        });
    }

    /// Move the highlight by `delta`, wrapping around the candidate list.
    pub(super) fn mention_move_highlight(&mut self, delta: isize) {
        let Some(popup) = self.mention_popup.as_mut() else {
            return;
        };
        let len = popup.candidates.len();
        if len == 0 {
            return;
        }
        let new = (popup.highlight as isize + delta).rem_euclid(len as isize);
        popup.highlight = new as usize;
    }

    /// Accept the highlighted candidate: replace the `(anchor, caret)` span
    /// with its `insert_text` (via `replace_selection`), then close the
    /// popup (terminal) or re-open + re-query at the same anchor
    /// (`continues`).
    pub(super) fn mention_accept(&mut self) -> EditorEvent {
        let Some(popup) = self.mention_popup.as_ref() else {
            return EditorEvent::None;
        };
        let anchor = popup.anchor;
        let candidate = popup.candidates.get(popup.highlight).cloned();
        let Some(candidate) = candidate else {
            self.mention_popup = None;
            return EditorEvent::None;
        };
        let caret = (self.cursor_line, self.cursor_col);
        self.set_selection(anchor, caret);
        self.replace_selection(&candidate.insert_text);
        if candidate.continues {
            self.mention_popup = Some(MentionPopup {
                anchor,
                candidates: Vec::new(),
                highlight: 0,
                list_rect: None,
            });
            self.mention_sync_after_edit();
        } else {
            self.mention_popup = None;
        }
        EditorEvent::Changed
    }

    /// Re-derive the query after a buffer edit and re-query the provider,
    /// clamping the highlight; closes the popup if the caret has moved off
    /// the anchor's span.
    pub(super) fn mention_sync_after_edit(&mut self) {
        let Some(popup) = self.mention_popup.as_ref() else {
            return;
        };
        let anchor = popup.anchor;
        if self.cursor_line != anchor.0 || self.cursor_col <= anchor.1 {
            self.mention_popup = None;
            return;
        }
        let query = self.mention_query();
        let candidates = self.mention_query_provider(&query);
        let Some(popup) = self.mention_popup.as_mut() else {
            return;
        };
        let len = candidates.len();
        popup.candidates = candidates;
        popup.highlight = if len == 0 {
            0
        } else {
            popup.highlight.min(len - 1)
        };
    }

    /// First refusal while the popup is open. `Some(event)` = fully
    /// handled (Up/Down nav, Enter/Tab accept, Esc dismiss, empty-query
    /// Backspace close, or a movement key that dismisses); `None` = fall
    /// through to normal dispatch (printable chars to insert, non-empty
    /// Backspace to shrink).
    pub(super) fn mention_handle_open(&mut self, key: KeyEvent) -> Option<EditorEvent> {
        match key.code {
            KeyCode::Up => {
                self.mention_move_highlight(-1);
                Some(EditorEvent::None)
            }
            KeyCode::Down => {
                self.mention_move_highlight(1);
                Some(EditorEvent::None)
            }
            KeyCode::Enter | KeyCode::Tab => Some(self.mention_accept()),
            KeyCode::Esc => {
                self.mention_popup = None;
                Some(EditorEvent::None)
            }
            KeyCode::Backspace => {
                if self.mention_query().is_empty() {
                    self.mention_popup = None;
                }
                None
            }
            KeyCode::Char(_) if !key.modifiers.contains(KeyModifiers::CONTROL) => None,
            _ => {
                // Left/Right/Home/End/Delete/Ctrl-combos/other: the query
                // span is no longer being extended — dismiss and pass
                // through to normal handling.
                self.mention_popup = None;
                None
            }
        }
    }

    /// Post-edit hook, called after every `dispatch_key_inner`: re-query if
    /// the popup is still open, else open it if `key` was the (non-Ctrl)
    /// trigger char. No-op when no provider is attached — this guard is
    /// real logic (not a stub) so editors that never call
    /// `set_mention_provider` are byte-for-byte unaffected.
    pub(super) fn mention_after_key(&mut self, key: KeyEvent) {
        let Some(trigger) = self.mention_trigger() else {
            return;
        };
        if self.mention_popup.is_some() {
            self.mention_sync_after_edit();
            return;
        }
        if let KeyCode::Char(c) = key.code {
            if c == trigger && !key.modifiers.contains(KeyModifiers::CONTROL) {
                self.mention_open();
            }
        }
    }

    /// Hit-test a `Down(Left)` mouse cell against the popup's last-rendered
    /// candidate-list rect (`list_rect`); on a hit, sets `highlight` to the
    /// clicked row and accepts through `mention_accept()` (the same
    /// span-replace/close-or-continue path as keyboard Enter/Tab). Returns
    /// `false` (no mutation) when there is no popup, it hasn't been rendered
    /// yet (`list_rect` is `None`), or the cell falls outside the list — the
    /// `mention_popup.is_none()` guard is real (not stubbed) so editors with
    /// no popup open are byte-for-byte unaffected, matching
    /// `mention_after_key`'s no-provider guard. b1-t3.
    pub(super) fn mention_mouse_accept(&mut self, column: u16, row: u16) -> bool {
        if self.mention_popup.is_none() {
            return false;
        }
        let Some(list_rect) = self.mention_popup.as_ref().and_then(|p| p.list_rect) else {
            return false;
        };
        if column < list_rect.x
            || column >= list_rect.x + list_rect.width
            || row < list_rect.y
            || row >= list_rect.y + list_rect.height
        {
            return false;
        }
        let idx = (row - list_rect.y) as usize;
        let Some(popup) = self.mention_popup.as_mut() else {
            return false;
        };
        if idx >= popup.candidates.len() {
            return false;
        }
        popup.highlight = idx;
        self.mention_accept();
        true
    }
}
