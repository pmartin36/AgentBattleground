use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use engine_core::inspect::FieldSchema;
use engine_core::ipc::{CatalogEntry, Message};
use engine_core::SceneKey;
use engine_core::{parse_path_segment, Segment};

/// Pure UI state — the testable controller layer (no egui, no client).
pub struct SwitcherState {
    pub catalog: Vec<CatalogEntry>, // from Hello; empty until connected
    pub active: Option<SceneKey>,   // game's current scene
    pub selected: Option<SceneKey>, // dropdown selection
    pub connected: bool,            // true once Hello seen; false on disconnect
    pub should_exit: bool,          // true once set_disconnected() is called
    /// Every catalog entry's schema, keyed by scene id. Populated by `Hello` (b4-t1).
    pub schema_cache: HashMap<SceneKey, FieldSchema>,
    /// The active scene's schema — the field-editor panel skeleton (b4-t1).
    /// `Rc`-wrapped so `render_field_panel`'s per-frame clone (required to
    /// avoid holding an immutable borrow into `state` while also passing
    /// `state` mutably to `render_field`) is an O(1) refcount bump, not a
    /// deep tree clone repeated every frame under continuous repaint.
    pub panel_schema: Option<Rc<FieldSchema>>,
    /// The active scene's live values, from `Hello`/`SceneChanged.snapshot` (b4-t2).
    pub panel_snapshot: serde_json::Value,
    /// Buffered local edits not yet submitted, keyed by patch path (b4-t3).
    /// Reads (`display_value`) prefer this overlay over `panel_snapshot`;
    /// an incoming `StateSnapshot` only ever touches `panel_snapshot`, never
    /// this field — that disjointness is the no-clobber guarantee.
    pub dirty: BTreeMap<String, serde_json::Value>,
    /// True from `begin_submit()` until the matching `Ack`/`StateSnapshot`
    /// reply is observed by the reducer; gates the post-submit dirty-clear
    /// so an unprompted `StateSnapshot` still can't clobber a live edit (b4-t9).
    pub awaiting_submit: bool,
    /// Per-frame live-edit signal for "apply on change" mode, drained by
    /// `InspectorApp::flush_edits` each frame. Separate from the persistent
    /// `dirty` buffer, which still feeds Submit (b4-t9).
    pub frame_edits: Vec<(String, serde_json::Value)>,
}

impl SwitcherState {
    pub fn new() -> Self {
        SwitcherState {
            catalog: Vec::new(),
            active: None,
            selected: None,
            connected: false,
            should_exit: false,
            schema_cache: HashMap::new(),
            panel_schema: None,
            panel_snapshot: serde_json::Value::Null,
            dirty: BTreeMap::new(),
            awaiting_submit: false,
            frame_edits: Vec::new(),
        }
    }

    /// Reducer: updates state for one inbound message.
    /// Hello  -> catalog + active + selected + connected=true
    /// SceneChanged -> active=Some(id), selected=Some(id), panel rebuilt from
    ///   schema_cache (schema) + sc.snapshot (values) (b4-t2)
    /// StateSnapshot -> if `id == active`, replace `panel_snapshot` wholesale;
    ///   otherwise ignored entirely. If `awaiting_submit` is set, also clears
    ///   `dirty` + resets the flag (b4-t9 Submit reply) — atomically with the
    ///   refresh so a client never observes "cleared but not yet refreshed".
    ///   When the flag is unset, `dirty` is never touched — the no-clobber
    ///   guarantee for in-progress local edits (b4-t3).
    /// Ack -> no-op; it carries no state, so it must not race ahead of the
    ///   paired StateSnapshot's refresh (b4-t9 — see code-writer.md deviation).
    /// Error / SwitchScene -> no-op (M1)
    pub fn apply(&mut self, msg: &Message) {
        match msg {
            Message::Hello(h) => {
                self.catalog = h.scenes.clone();
                self.active = Some(h.active.clone());
                self.selected = Some(h.active.clone());
                self.connected = true;

                self.schema_cache.clear();
                for entry in &h.scenes {
                    self.schema_cache.insert(entry.id.clone(), entry.schema.clone());
                }
                self.panel_schema = self.schema_cache.get(&h.active).cloned().map(Rc::new);
            }
            Message::SceneChanged(sc) => {
                self.active = Some(sc.id.clone());
                self.selected = Some(sc.id.clone());
                self.panel_schema = self.schema_cache.get(&sc.id).cloned().map(Rc::new);
                self.panel_snapshot = sc.snapshot.clone();
            }
            Message::StateSnapshot(ss) => {
                if self.active.as_ref() == Some(&ss.id) {
                    self.panel_snapshot = ss.snapshot.clone();
                    if self.awaiting_submit {
                        self.dirty.clear();
                        self.awaiting_submit = false;
                    }
                }
                // Non-matching id, or the dirty overlay: left untouched.
            }
            Message::Ack => {
                // Deliberately does not clear `dirty`/`awaiting_submit` here (b4-t9
                // deviation — see code-writer.md): an `Ack` carries no state, so
                // clearing on it alone can race ahead of the paired `StateSnapshot`
                // that actually refreshes `panel_snapshot`, transiently showing a
                // stale/missing value. Only the active-scene `StateSnapshot` arm
                // clears the buffer, atomically with the refresh it performs.
            }
            Message::Error(_) | Message::SwitchScene(_) | Message::ApplyState(_) | Message::Subscribe(_) => {
                // no-op here; ApplyState/Subscribe handling lands in later tasks.
            }
        }
    }

    /// Drop connected flag (catalog may remain for greyed display).
    /// disconnect => exit
    pub fn set_disconnected(&mut self) {
        self.connected = false;
        self.should_exit = true;
    }

    /// Returns true when the app should exit (game connection was lost).
    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    /// Returns the catalog display name for the current selection, or "-".
    pub fn selected_name(&self) -> &str {
        if let Some(id) = &self.selected {
            for entry in &self.catalog {
                if entry.id == *id {
                    return &entry.name;
                }
            }
        }
        "-"
    }

    // ── b4-t3: dirty-overlay buffer ──────────────────────────────────────────
    // Public API consumed by the field-editor widgets (b4-t4+) and the submit
    // path (b4-t9); allow dead_code on each until that wiring lands.

    /// Buffer a local edit at `path`. Unconditional; readonly gating is b4-t4.
    /// Also records the edit onto `frame_edits` for the "apply on change"
    /// live signal, drained by `InspectorApp::flush_edits` each frame (b4-t9).
    pub fn mark_dirty(&mut self, path: &str, value: serde_json::Value) {
        self.dirty.insert(path.to_string(), value.clone());
        self.frame_edits.push((path.to_string(), value));
    }

    /// True if `path` has a buffered (not yet submitted/reverted) edit.
    pub fn is_dirty(&self, path: &str) -> bool {
        self.dirty.contains_key(path)
    }

    /// Buffered value at `path` if dirty, else navigated from `panel_snapshot`.
    pub fn display_value(&self, path: &str) -> Option<&serde_json::Value> {
        if let Some(v) = self.dirty.get(path) {
            return Some(v);
        }
        navigate(&self.panel_snapshot, path)
    }

    /// Snapshot of exactly the buffered edits, keyed by path.
    pub fn dirty_patch(&self) -> BTreeMap<String, serde_json::Value> {
        self.dirty.clone()
    }

    /// Discard all buffered edits. Also resets `awaiting_submit` so a
    /// mid-flight revert can't be clobbered by a late stray `Ack` (b4-t9).
    pub fn revert(&mut self) {
        self.dirty.clear();
        self.awaiting_submit = false;
    }

    /// Mark a Submit as in-flight (b4-t9): sets `awaiting_submit`, gating the
    /// reducer's post-reply dirty-clear.
    pub fn begin_submit(&mut self) {
        self.awaiting_submit = true;
    }

    /// Drain and return this frame's live-edit signal (b4-t9), leaving
    /// `frame_edits` empty for the next frame.
    pub fn take_frame_edits(&mut self) -> Vec<(String, serde_json::Value)> {
        std::mem::take(&mut self.frame_edits)
    }
}

/// Read-only navigator: walks `path` (dotted fields + `[N]` indices, via the
/// shared grammar) into `root`, returning `None` on any missing/mismatched
/// segment rather than panicking.
fn navigate<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    let mut rest = path;
    loop {
        let (segment, tail) = parse_path_segment(rest).ok()?;
        cur = match segment {
            Segment::Field(field) => cur.get(field)?,
            Segment::Index(index) => cur.get(index)?,
        };
        match tail {
            Some(t) => rest = t,
            None => return Some(cur),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_fixtures::*;
    use engine_core::ipc::SceneChanged;

    /// New state: empty catalog, no active/selected, not connected.
    #[test]
    fn new_state_is_empty_and_disconnected() {
        let s = SwitcherState::new();
        assert!(s.catalog.is_empty(), "catalog must start empty");
        assert_eq!(s.active, None, "active must start None");
        assert_eq!(s.selected, None, "selected must start None");
        assert!(!s.connected, "connected must start false");
    }

    /// Hello sets catalog, marks active, pre-selects it, marks connected.
    #[test]
    fn hello_populates_catalog_and_selects_active() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        assert_eq!(s.catalog.len(), 4, "catalog must have 4 scenes after Hello");
        assert_eq!(s.active, Some(SceneKey::new("MainHub")), "active must be MainHub");
        assert_eq!(s.selected, Some(SceneKey::new("MainHub")), "selected must be pre-set to active");
        assert!(s.connected, "connected must be true after Hello");
    }

    /// SceneChanged updates both active and selected (covers unsolicited switches).
    #[test]
    fn scene_changed_updates_selection() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("BattleViewer"),
            snapshot: serde_json::Value::Null,
        }));
        assert_eq!(s.active, Some(SceneKey::new("BattleViewer")), "active must track SceneChanged");
        assert_eq!(s.selected, Some(SceneKey::new("BattleViewer")), "selected must mirror active on SceneChanged");
    }

    /// selected_name returns the catalog entry's name for the selected scene.
    #[test]
    fn selected_name_reflects_catalog() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        // Hello pre-selects MainHub; catalog entry name is "Main Hub".
        assert_eq!(s.selected_name(), "Main Hub", "selected_name must return catalog name");
    }

    /// set_disconnected flips connected to false; catalog may remain.
    #[test]
    fn set_disconnected_clears_connected() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        assert!(s.connected, "must be connected after Hello");
        s.set_disconnected();
        assert!(!s.connected, "must be disconnected after set_disconnected");
    }

    // ── b5-t3: connection lifecycle ───────────────────────────────────────────

    /// should_exit is false on a fresh SwitcherState.
    #[test]
    fn should_exit_false_on_new() {
        let s = SwitcherState::new();
        assert!(!s.should_exit(), "should_exit must be false on new state");
    }

    /// should_exit stays false after a Hello (not a disconnect event).
    #[test]
    fn should_exit_false_after_hello() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        assert!(!s.should_exit(), "should_exit must remain false after Hello");
    }

    /// set_disconnected must flip should_exit to true.
    /// RED: set_disconnected() does not yet set should_exit.
    #[test]
    fn set_disconnected_sets_should_exit() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.set_disconnected();
        assert!(
            s.should_exit(),
            "set_disconnected must set should_exit to true"
        );
    }
    // ── b4-t1: schema cache + panel skeleton ────────────────────────────────

    /// Fresh state: cache empty, no panel schema yet.
    #[test]
    fn new_state_has_empty_schema_cache_and_no_panel() {
        let s = SwitcherState::new();
        assert!(s.schema_cache.is_empty(), "schema_cache must start empty");
        assert_eq!(s.panel_schema, None, "panel_schema must start None");
    }

    /// Hello caches every catalog entry's schema, keyed by scene id, by value.
    #[test]
    fn hello_caches_every_entry_schema() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());

        assert_eq!(s.schema_cache.len(), 4, "schema_cache must have one entry per catalog scene");
        assert_eq!(
            s.schema_cache.get(&SceneKey::new("MainHub")),
            Some(&stub_schema_with_fields("MainHub", 1)),
            "cached schema for MainHub must equal its source entry's schema"
        );
        assert_eq!(
            s.schema_cache.get(&SceneKey::new("BattleViewer")),
            Some(&stub_schema_with_fields("BattleViewer", 2)),
            "cached schema for BattleViewer must equal its source entry's schema"
        );
        assert_eq!(
            s.schema_cache.get(&SceneKey::new("RosterManager")),
            Some(&stub_schema_with_fields("RosterManager", 3)),
            "cached schema for RosterManager must equal its source entry's schema"
        );
        assert_eq!(
            s.schema_cache.get(&SceneKey::new("Leaderboard")),
            Some(&stub_schema_with_fields("Leaderboard", 4)),
            "cached schema for Leaderboard must equal its source entry's schema"
        );
    }

    /// panel_schema is set to the active scene's schema specifically (not an
    /// arbitrary cache entry) — MainHub's schema has 1 field, distinct from
    /// the other three, so a wrong pick is caught.
    #[test]
    fn hello_sets_panel_schema_to_active() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        assert_eq!(
            s.panel_schema.as_deref(),
            Some(&stub_schema_with_fields("MainHub", 1)),
            "panel_schema must equal the active (MainHub) scene's cached schema"
        );
    }

    /// A second, disjoint Hello fully rebuilds schema_cache — no stale entries
    /// survive from the first Hello, and panel_schema tracks the new active scene.
    #[test]
    fn second_hello_rebuilds_schema_cache() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&two_scene_hello_distinct());

        assert_eq!(
            s.schema_cache.len(),
            2,
            "schema_cache must be rebuilt from scratch, not merged (stale entries must be gone)"
        );
        assert!(
            !s.schema_cache.contains_key(&SceneKey::new("MainHub")),
            "MainHub's stale cache entry from the first Hello must be gone"
        );
        assert_eq!(
            s.schema_cache.get(&SceneKey::new("Leaderboard")),
            Some(&stub_schema_with_fields("Leaderboard2", 6)),
            "Leaderboard's cache entry must be replaced by the second Hello's schema"
        );
        assert_eq!(
            s.panel_schema.as_deref(),
            Some(&stub_schema_with_fields("Leaderboard2", 6)),
            "panel_schema must track the second Hello's active scene (Leaderboard)"
        );
    }

    // ── b4-t2: SceneChanged rebuilds the panel from the schema cache ────────

    /// Fresh state: panel_snapshot starts as Value::Null.
    #[test]
    fn new_state_panel_snapshot_is_null() {
        let s = SwitcherState::new();
        assert_eq!(
            s.panel_snapshot,
            serde_json::Value::Null,
            "panel_snapshot must start as Value::Null"
        );
    }

    /// SceneChanged for a cached scene swaps panel_schema to that scene's
    /// cached schema (never from the message, which has no `schema` field)
    /// and stores the message's snapshot into panel_snapshot.
    #[test]
    fn scene_changed_rebuilds_panel_from_cache() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        let snap = serde_json::json!({"k": 1});
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("BattleViewer"),
            snapshot: snap.clone(),
        }));

        assert_eq!(
            s.panel_schema.as_deref(),
            Some(&stub_schema_with_fields("BattleViewer", 2)),
            "panel_schema must swap to BattleViewer's cached schema on SceneChanged"
        );
        assert_ne!(
            s.panel_schema.as_deref(),
            Some(&stub_schema_with_fields("MainHub", 1)),
            "panel_schema must not remain the previously-active scene's schema"
        );
        assert_eq!(
            s.panel_snapshot, snap,
            "panel_snapshot must equal the SceneChanged message's snapshot"
        );
    }

    /// A second SceneChanged back to the original scene swaps panel_schema
    /// back and updates panel_snapshot again — proves a real cache re-lookup
    /// on every SceneChanged, not a one-time/stale value.
    #[test]
    fn scene_changed_back_swaps_panel() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("BattleViewer"),
            snapshot: serde_json::json!({"k": 1}),
        }));
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("MainHub"),
            snapshot: serde_json::json!({"k": 2}),
        }));

        assert_eq!(
            s.panel_schema.as_deref(),
            Some(&stub_schema_with_fields("MainHub", 1)),
            "panel_schema must swap back to MainHub's cached schema"
        );
        assert_eq!(
            s.panel_snapshot,
            serde_json::json!({"k": 2}),
            "panel_snapshot must track the latest SceneChanged's snapshot"
        );
    }

    /// SceneChanged for a SceneKey absent from schema_cache leaves panel_schema
    /// as None (defensive no-panic path) rather than panicking or keeping a
    /// stale Some value.
    #[test]
    fn scene_changed_uncached_id_leaves_panel_none() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        // Settings is never sent in four_scene_hello()'s catalog.
        assert!(
            !s.schema_cache.contains_key(&SceneKey::new("Settings")),
            "test precondition: Settings must be absent from schema_cache"
        );

        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("Settings"),
            snapshot: serde_json::json!({"k": 3}),
        }));

        assert_eq!(
            s.panel_schema, None,
            "panel_schema must be None for a SceneChanged whose id has no cache entry"
        );
    }

    // ── b4-t3: dirty-overlay buffer + StateSnapshot no-clobber ──────────────

    use engine_core::ipc::StateSnapshot;

    /// Fresh state: no dirty entries at all.
    #[test]
    fn new_state_has_empty_dirty_buffer() {
        let s = SwitcherState::new();
        assert!(s.dirty_patch().is_empty(), "dirty_patch() must start empty");
        assert!(!s.is_dirty("anything"), "is_dirty must be false with no edits");
    }

    /// mark_dirty buffers the edit; is_dirty and display_value both see it.
    #[test]
    fn mark_dirty_makes_field_dirty_and_display_returns_edit() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("MainHub"),
            snapshot: serde_json::json!({"a": 1}),
        }));

        s.mark_dirty("a", serde_json::json!(42));

        assert!(s.is_dirty("a"), "field must be dirty after mark_dirty");
        assert_eq!(
            s.display_value("a"),
            Some(&serde_json::json!(42)),
            "display_value must return the buffered edit for a dirty path"
        );
    }

    /// A clean (non-dirty) path's display_value navigates panel_snapshot via
    /// the shared path grammar, including nested field and index segments.
    #[test]
    fn display_value_falls_back_to_snapshot_for_clean_nested_path() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("MainHub"),
            snapshot: serde_json::json!({
                "a": {"b": 5},
                "list": [{"x": 1}, {"x": 2}],
            }),
        }));

        assert_eq!(
            s.display_value("a.b"),
            Some(&serde_json::json!(5)),
            "display_value must navigate a dotted field path into panel_snapshot"
        );
        assert_eq!(
            s.display_value("list[1].x"),
            Some(&serde_json::json!(2)),
            "display_value must navigate an indexed-then-field path into panel_snapshot"
        );
        assert_eq!(
            s.display_value("missing.path"),
            None,
            "display_value must return None for a path absent from panel_snapshot, not panic"
        );
    }

    /// A StateSnapshot for the active scene updates a clean field's value but
    /// leaves a dirty field's buffered edit untouched (spec 15 line 47).
    #[test]
    fn state_snapshot_active_id_no_clobber_of_dirty_but_updates_clean() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("MainHub"),
            snapshot: serde_json::json!({"dirty_field": 1, "clean_field": 2}),
        }));
        s.mark_dirty("dirty_field", serde_json::json!(99));

        s.apply(&Message::StateSnapshot(StateSnapshot {
            id: SceneKey::new("MainHub"),
            snapshot: serde_json::json!({"dirty_field": 5, "clean_field": 20}),
        }));

        assert_eq!(
            s.display_value("dirty_field"),
            Some(&serde_json::json!(99)),
            "a dirty field's buffered edit must survive an incoming StateSnapshot for the active scene"
        );
        assert_eq!(
            s.display_value("clean_field"),
            Some(&serde_json::json!(20)),
            "a clean field must pick up the incoming StateSnapshot's new value"
        );
    }

    /// A StateSnapshot for a different (non-active) scene id is ignored
    /// entirely: it must not corrupt panel_snapshot or clobber a dirty edit.
    #[test]
    fn state_snapshot_other_id_is_ignored() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("MainHub"),
            snapshot: serde_json::json!({"dirty_field": 1, "clean_field": 2}),
        }));
        s.mark_dirty("dirty_field", serde_json::json!(99));

        s.apply(&Message::StateSnapshot(StateSnapshot {
            id: SceneKey::new("BattleViewer"),
            snapshot: serde_json::json!({"dirty_field": 500, "clean_field": 999}),
        }));

        assert_eq!(
            s.display_value("dirty_field"),
            Some(&serde_json::json!(99)),
            "a StateSnapshot for a non-active scene must not clobber a dirty edit"
        );
        assert_eq!(
            s.panel_snapshot,
            serde_json::json!({"dirty_field": 1, "clean_field": 2}),
            "a StateSnapshot for a non-active scene must leave panel_snapshot untouched"
        );
    }

    /// revert() clears the dirty buffer; display_value falls back to
    /// panel_snapshot again.
    #[test]
    fn revert_clears_dirty_and_display_falls_back_to_snapshot() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("MainHub"),
            snapshot: serde_json::json!({"a": 1}),
        }));
        s.mark_dirty("a", serde_json::json!(42));

        s.revert();

        assert!(s.dirty_patch().is_empty(), "revert must clear the dirty buffer");
        assert!(!s.is_dirty("a"), "is_dirty must be false after revert");
        assert_eq!(
            s.display_value("a"),
            Some(&serde_json::json!(1)),
            "display_value must fall back to panel_snapshot after revert"
        );
    }

    /// dirty_patch() returns exactly the marked paths/values, nothing else.
    #[test]
    fn dirty_patch_returns_exactly_dirty_entries() {
        let mut s = SwitcherState::new();
        s.apply(&four_scene_hello());
        s.apply(&Message::SceneChanged(SceneChanged {
            id: SceneKey::new("MainHub"),
            snapshot: serde_json::json!({"a": 1, "b": 2, "c": 3}),
        }));
        s.mark_dirty("a", serde_json::json!(10));
        s.mark_dirty("c", serde_json::json!(30));

        let mut expected = BTreeMap::new();
        expected.insert("a".to_string(), serde_json::json!(10));
        expected.insert("c".to_string(), serde_json::json!(30));

        assert_eq!(
            s.dirty_patch(),
            expected,
            "dirty_patch must return exactly the marked paths/values, nothing else"
        );
    }
}
