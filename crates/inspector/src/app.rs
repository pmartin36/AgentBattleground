//! Inspector UI state and egui app (b5-t2).
//!
//! `SwitcherState` is the pure, egui-free controller layer. All UI-relevant
//! state lives here and is updated through `apply(&Message)`. The egui draw
//! layer (`InspectorApp`) is built on top of this struct; tests only target
//! `SwitcherState`.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use scene_core::color::Rgba;
use scene_core::inspect::{FieldSchema, FieldTag};
use scene_core::ipc::{CatalogEntry, Message};
use scene_core::SceneKey;
use scene_core::{parse_path_segment, Segment};
use crate::client::InspectorClient;

/// Deliberate vertical gap (px) between the scene-selector row and the
/// field-editor section in `InspectorApp::render_body` (b1-t2). Must exceed
/// egui's default `item_spacing.y` (~3.0) so the sections read as visually
/// distinct, not jammed together.
const SECTION_GAP: f32 = 12.0;
const ACTION_ROW_PADDING: f32 = 6.0;

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

/// Per-leaf-field rendered `Response`, keyed by its full patch path.
/// Populated by `render_field_panel`; production ignores it, tests use it to
/// locate widgets / assert enabled state (b4-t4).
type FieldResponses = HashMap<String, egui::Response>;

/// Shared bordered frame used for both top-level `render_body` sections
/// (b1-t3): calling this twice with the same `ui.style()` yields two
/// byte-identical frames, so the two sections read as framed alike without
/// duplicating a `Frame::group` literal at each call site.
fn section_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::group(style)
}

/// Render `state.panel_schema`'s tree into `ui`, wiring edits to
/// `state.mark_dirty` (b4-t4: bool/int/float/string/struct/list widget arms +
/// the shared readonly gate). Renders nothing when `panel_schema` is `None`.
/// Root's own children are rendered directly — no header wraps the root itself.
fn render_field_panel(ui: &mut egui::Ui, state: &mut SwitcherState) -> FieldResponses {
    let mut out = FieldResponses::new();
    let Some(schema) = state.panel_schema.clone() else {
        return out;
    };
    for child in &schema.children {
        render_field(ui, state, child, &child.name, &mut out);
    }
    out
}

/// Wraps `render_field_panel` in a vertical scroll area (b1-t4) so a tall
/// schema (e.g. a 12-element list of multi-field structs) scrolls instead of
/// overflowing/clipping silently. Returns the `ScrollAreaOutput` so callers
/// (and tests) can inspect `content_size`/`inner_rect` to confirm a real
/// scroll clip is in the render path, not just a wide `Ui`.
fn scroll_field_panel(
    ui: &mut egui::Ui,
    state: &mut SwitcherState,
) -> egui::scroll_area::ScrollAreaOutput<FieldResponses> {
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| render_field_panel(ui, state))
}

/// Shared `Int`/`Float` field-render body: a labeled row with a `DragValue`,
/// range-clamped when `schema.range` is present. `T` is `i64` for `Int`,
/// `f64` for `Float` — the only two `Numeric` types `render_field` uses this
/// for.
fn render_numeric_field<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    dirty: bool,
    label: &str,
    readonly: bool,
    value: &mut T,
    range: Option<std::ops::RangeInclusive<T>>,
) -> egui::Response {
    ui.horizontal(|ui| {
        field_label(ui, dirty, label);
        interactive(ui, readonly, |ui| {
            let mut dv = egui::DragValue::new(value);
            if let Some(r) = range {
                dv = dv.range(r);
            }
            ui.add(dv)
        })
    })
    .inner
}

/// Recursive worker: render one field (and its descendants), appending each
/// leaf's `Response` into `out`, keyed by its full patch path. Skips
/// `schema.hidden` fields. Struct/List are containers — they are not
/// themselves editable and contribute no entry to `out`; readonly lives on
/// their leaf descendants.
fn render_field(
    ui: &mut egui::Ui,
    state: &mut SwitcherState,
    schema: &FieldSchema,
    path: &str,
    out: &mut FieldResponses,
) {
    if schema.hidden {
        return;
    }
    let label = schema.label.clone().unwrap_or_else(|| schema.name.clone());

    let dirty = state.is_dirty(path);

    match schema.tag {
        FieldTag::Bool => {
            let mut value = state
                .display_value(path)
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let resp = ui
                .horizontal(|ui| {
                    field_label(ui, dirty, label.as_str());
                    interactive(ui, schema.readonly, |ui| ui.checkbox(&mut value, ""))
                })
                .inner;
            if resp.changed() {
                state.mark_dirty(path, serde_json::json!(value));
            }
            out.insert(path.to_string(), resp);
        }
        FieldTag::Int => {
            let mut value = state
                .display_value(path)
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let range = schema.range.map(|r| (r.min as i64)..=(r.max as i64));
            let resp = render_numeric_field(ui, dirty, &label, schema.readonly, &mut value, range);
            if resp.changed() {
                state.mark_dirty(path, serde_json::json!(value));
            }
            out.insert(path.to_string(), resp);
        }
        FieldTag::Float => {
            let mut value = state
                .display_value(path)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let range = schema.range.map(|r| r.min..=r.max);
            let resp = render_numeric_field(ui, dirty, &label, schema.readonly, &mut value, range);
            if resp.changed() {
                state.mark_dirty(path, serde_json::json!(value));
            }
            out.insert(path.to_string(), resp);
        }
        FieldTag::String => {
            let mut value = state
                .display_value(path)
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_default();
            let resp = ui
                .horizontal(|ui| {
                    field_label(ui, dirty, label.as_str());
                    interactive(ui, schema.readonly, |ui| ui.text_edit_singleline(&mut value))
                })
                .inner;
            if resp.changed() {
                state.mark_dirty(path, serde_json::json!(value));
            }
            out.insert(path.to_string(), resp);
        }
        FieldTag::Struct => {
            egui::CollapsingHeader::new(label)
                .id_salt(path)
                .default_open(true)
                .show(ui, |ui| {
                    for child in &schema.children {
                        let child_path = if path.is_empty() {
                            child.name.clone()
                        } else {
                            format!("{path}.{}", child.name)
                        };
                        render_field(ui, state, child, &child_path, out);
                    }
                });
        }
        FieldTag::List => {
            let len = state
                .display_value(path)
                .and_then(|v| v.as_array())
                .map(Vec::len)
                .unwrap_or(0);
            egui::CollapsingHeader::new(label)
                .id_salt(path)
                .default_open(true)
                .show(ui, |ui| {
                    if let Some(element_schema) = schema.children.first() {
                        for k in 0..len {
                            let child_path = format!("{path}[{k}]");
                            render_field(ui, state, element_schema, &child_path, out);
                        }
                    }
                });
        }
        FieldTag::Enum => {
            let mut selected = state
                .display_value(path)
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_default();
            let before = selected.clone();
            let mut items: Vec<(String, egui::Response)> = Vec::new();
            let button = ui
                .horizontal(|ui| {
                    field_label(ui, dirty, label.as_str());
                    interactive(ui, schema.readonly, |ui| {
                        egui::ComboBox::from_id_salt(path)
                            .selected_text(selected.as_str())
                            .show_ui(ui, |ui| {
                                for v in &schema.variants {
                                    let r =
                                        ui.selectable_value(&mut selected, v.clone(), v.as_str());
                                    items.push((v.clone(), r));
                                }
                            })
                            .response
                    })
                })
                .inner;
            if selected != before {
                state.mark_dirty(path, serde_json::json!(selected));
            }
            out.insert(path.to_string(), button);
            for (v, r) in items {
                // A popup's first-ever open renders one invisible sizing pass
                // (egui's `Area` measures its own content before it can
                // report a real, interactable rect). Skip surfacing that
                // pass's item responses under the composite key so a
                // headless click-driver never targets a rect that egui's
                // one-frame-lagged hit test cannot yet resolve against.
                if r.enabled() {
                    out.insert(format!("{path}::{v}"), r);
                }
            }
        }
        FieldTag::Color => {
            let current = state.display_value(path);
            let color = read_color(current);
            let mut rgb = [color.r, color.g, color.b];
            let resp = ui
                .horizontal(|ui| {
                    field_label(ui, dirty, label.as_str());
                    interactive(ui, schema.readonly, |ui| {
                        egui::color_picker::color_edit_button_srgb(ui, &mut rgb)
                    })
                })
                .inner;
            if resp.changed() {
                let new_value = recolor_keep_alpha(current, rgb);
                state.mark_dirty(path, new_value);
            }
            out.insert(path.to_string(), resp);
        }
        FieldTag::Json => {
            // Permanent read-only fallback: Json-tagged fields (derived from
            // data-carrying enums) never become interactive and never call
            // `mark_dirty` — pretty-printed text is the whole widget.
            let text = state
                .display_value(path)
                .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
                .unwrap_or_default();
            let resp = ui
                .horizontal(|ui| {
                    field_label(ui, dirty, label.as_str());
                    interactive(ui, true, |ui| {
                        ui.add(egui::Label::new(text.clone()).sense(egui::Sense::hover()))
                    })
                })
                .inner;
            out.insert(path.to_string(), resp);
        }
    }
}

/// Shared readonly gate reused by every editable widget arm (this task and
/// b4-t5/t6/t7): draws `add` disabled when `readonly`, so a readonly leaf's
/// `Response::enabled()` is `false` and it is excluded from hit-testing —
/// a click on it can never originate a `.changed()` event.
fn interactive(
    ui: &mut egui::Ui,
    readonly: bool,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    ui.add_enabled_ui(!readonly, add).inner
}

/// Draws a leaf field's display text, tinted when the field has an
/// unsubmitted buffered edit (`SwitcherState::is_dirty`).
fn field_label(ui: &mut egui::Ui, dirty: bool, label: &str) -> egui::Response {
    if dirty {
        ui.colored_label(egui::Color32::YELLOW, label)
    } else {
        ui.label(label)
    }
}

/// Read the (r,g,b,a) of a Color field's current snapshot/dirty value.
/// Malformed/missing -> opaque black (never panics) (b4-t6).
fn read_color(current: Option<&serde_json::Value>) -> Rgba {
    current
        .and_then(|v| v.as_str())
        .and_then(|s| Rgba::from_hex(s).ok())
        .unwrap_or(Rgba::rgb(0, 0, 0))
}

/// Produce the new dirty value for a color edit: overwrite R/G/B from the
/// picker, carry the ORIGINAL alpha through untouched, re-emit "#rrggbbaa"
/// (b4-t6 — mandatory alpha-identity guarantee, point 8).
fn recolor_keep_alpha(current: Option<&serde_json::Value>, new_rgb: [u8; 3]) -> serde_json::Value {
    let a = read_color(current).a;
    let rgba = Rgba::new(new_rgb[0], new_rgb[1], new_rgb[2], a);
    serde_json::json!(rgba.to_hex())
}

/// The eframe app: owns state + the live client.
pub struct InspectorApp {
    state: SwitcherState,
    client: InspectorClient,
    /// "Apply on change" toggle (b4-t9); on by default (b2-t1).
    live_apply: bool,
}

impl InspectorApp {
    pub fn new(client: InspectorClient) -> Self {
        InspectorApp {
            state: SwitcherState::new(),
            client,
            live_apply: true,
        }
    }

    /// Drain all pending inbound messages into state.
    fn pump(&mut self) {
        loop {
            match self.client.incoming.try_recv() {
                Ok(msg) => self.state.apply(&msg),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.state.set_disconnected();
                    break;
                }
            }
        }
    }

    /// Send `ApplyState{active, dirty_patch()}` if the dirty buffer is
    /// non-empty, and mark the submit in-flight (b4-t9).
    pub fn submit(&mut self) {
        let patch = self.state.dirty_patch();
        if patch.is_empty() {
            return;
        }
        if let Some(id) = self.state.active.clone() {
            if let Err(e) = self.client.send_apply_state(id, patch) {
                eprintln!("inspector: send_apply_state failed: {e}");
            }
            self.state.begin_submit();
        }
    }

    /// Discard buffered edits; no socket I/O (b4-t9).
    pub fn revert(&mut self) {
        self.state.revert();
    }

    /// Toggle "apply on change"; sends `Subscribe{live}` only on a real
    /// transition (b4-t9).
    pub fn set_live_apply(&mut self, on: bool) {
        if on != self.live_apply {
            if let Err(e) = self.client.send_subscribe(on) {
                eprintln!("inspector: send_subscribe failed: {e}");
            }
            self.live_apply = on;
        }
    }

    /// Send the initial `Subscribe` reflecting `live_apply`'s default,
    /// exactly once at startup (b2-t1). Callers must invoke this exactly
    /// once, right after construction — it does not run automatically.
    pub fn start(&mut self) {
        if let Err(e) = self.client.send_subscribe(self.live_apply) {
            eprintln!("inspector: send_subscribe failed: {e}");
        }
    }

    /// Drain this frame's live-edit signal; while `live_apply` is on, send
    /// one `ApplyState` per edit (b4-t9).
    pub fn flush_edits(&mut self) {
        let edits = self.state.take_frame_edits();
        if !self.live_apply {
            return;
        }
        if let Some(id) = self.state.active.clone() {
            for (path, value) in edits {
                let mut patch = BTreeMap::new();
                patch.insert(path, value);
                if let Err(e) = self.client.send_apply_state(id.clone(), patch) {
                    eprintln!("inspector: send_apply_state failed: {e}");
                }
            }
        }
    }

    /// Render the scene-selector row and the field-editor section, stacked
    /// and each stretched to full width, directly into `ui` (no
    /// `egui::Panel`). Returns `(selector_response, field_editor_response)`
    /// so callers (and tests) can inspect the sections' rects.
    fn render_body(&mut self, ui: &mut egui::Ui) -> (egui::Response, egui::Response) {
        // Snapshot copy/owned values before splitting the borrow for the ComboBox.
        let connected = self.state.connected;
        let catalog_empty = self.state.catalog.is_empty();
        let selected_display = self.state.selected_name().to_string();

        let selector = section_frame(ui.style())
            .show(ui, |ui| {
                let w = ui.available_width();
                ui.set_min_width(w);

                ui.strong("Scene");
                ui.horizontal(|ui| {
                    // Status dot
                    if connected {
                        ui.colored_label(egui::Color32::GREEN, "live");
                    } else {
                        ui.colored_label(egui::Color32::GRAY, "disconnected");
                    }

                    // Dropdown — borrow disjoint fields of state inside a scoped block so
                    // the mutable borrow of `selected` ends before the Go button reads it.
                    {
                        let cat = &self.state.catalog;
                        let sel = &mut self.state.selected;

                        ui.add_enabled_ui(connected && !catalog_empty, |ui| {
                            egui::ComboBox::from_label("Scene")
                                .selected_text(selected_display.as_str())
                                .show_ui(ui, |ui| {
                                    for e in cat {
                                        ui.selectable_value(sel, Some(e.id.clone()), &e.name);
                                    }
                                });
                        });
                    } // cat and sel borrows end here

                    // Go button
                    let go_enabled = self.state.selected.is_some() && connected;
                    if ui
                        .add_enabled(go_enabled, egui::Button::new("Go"))
                        .clicked()
                    {
                        if let Some(s) = &self.state.selected {
                            if let Err(e) = self.client.send_switch(s.as_str(), None) {
                                eprintln!("inspector: send_switch failed: {e}");
                            }
                        }
                    }
                });
            })
            .response;

        ui.add_space(SECTION_GAP);

        let editor = section_frame(ui.style())
            .show(ui, |ui| {
                let w = ui.available_width();
                ui.set_min_width(w);

                ui.strong("Fields");
                scroll_field_panel(ui, &mut self.state);
            })
            .response;

        (selector, editor)
    }

    /// Submit / Revert / Apply-on-change row. Returns the Submit and Revert
    /// button responses so tests can assert their enabled state. Both buttons
    /// share one `dirty_empty` gate.
    fn render_action_row(&mut self, ui: &mut egui::Ui) -> ActionRow {
        ui.add_space(ACTION_ROW_PADDING);
        let row = ui
            .horizontal(|ui| {
                let dirty_empty = self.state.dirty_patch().is_empty();

                let submit = ui.add_enabled(!dirty_empty, egui::Button::new("Submit"));
                if submit.clicked() {
                    self.submit();
                }

                let revert = ui.add_enabled(!dirty_empty, egui::Button::new("Revert"));
                if revert.clicked() {
                    self.revert();
                }

                let mut live_apply = self.live_apply;
                if ui.checkbox(&mut live_apply, "Apply on change").changed() {
                    self.set_live_apply(live_apply);
                }

                ActionRow { submit, revert }
            })
            .inner;
        ui.add_space(ACTION_ROW_PADDING);
        row
    }

    /// Production entry point: docks the action row and the rest of the
    /// body together so the action row stays reachable regardless of
    /// field-list content height (b1-t1). The action row is pinned to a
    /// bottom panel (shown first, per egui's panel-ordering rule) and the
    /// scene-selector + field-editor body occupies the remaining central
    /// panel, which naturally caps the field list's scroll area to the
    /// remaining middle space.
    fn render(&mut self, ui: &mut egui::Ui) -> ActionRow {
        let action = egui::Panel::bottom("inspector_action_row")
            .show(ui, |ui| self.render_action_row(ui))
            .inner;
        egui::CentralPanel::default().show(ui, |ui| {
            let _ = self.render_body(ui);
        });
        action
    }
}

/// Submit / Revert button `Response`s from the action row, surfaced so tests
/// can inspect `Response::enabled()` (b2-t2). Fields are read only by tests.
#[allow(dead_code)]
struct ActionRow {
    submit: egui::Response,
    revert: egui::Response,
}

impl eframe::App for InspectorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();

        if self.state.should_exit() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let _ = self.render(ui);
        self.flush_edits();

        ui.ctx().request_repaint();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::tests::stub_schema;
    use scene_core::inspect::{FieldSchema, FieldTag};
    use scene_core::ipc::{CatalogEntry, Hello, Message, SceneChanged};

    // ── b4-t4: field-editor widget matrix (headless egui harness) ──────────

    use scene_core::inspect::Range;

    fn leaf(name: &str, tag: FieldTag) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            label: None,
            tag,
            readonly: false,
            hidden: false,
            range: None,
            children: vec![],
            variants: vec![],
        }
    }

    fn struct_schema(name: &str, children: Vec<FieldSchema>) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            label: None,
            tag: FieldTag::Struct,
            readonly: false,
            hidden: false,
            range: None,
            children,
            variants: vec![],
        }
    }

    fn list_schema(name: &str, element: FieldSchema) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            label: None,
            tag: FieldTag::List,
            readonly: false,
            hidden: false,
            range: None,
            children: vec![element],
            variants: vec![],
        }
    }

    /// An `Enum`-tagged leaf with the given variant names (b4-t5).
    fn enum_schema(name: &str, variants: Vec<&str>) -> FieldSchema {
        FieldSchema {
            name: name.to_string(),
            label: None,
            tag: FieldTag::Enum,
            readonly: false,
            hidden: false,
            range: None,
            children: vec![],
            variants: variants.into_iter().map(str::to_string).collect(),
        }
    }

    /// Runs one headless egui pass over `render_field_panel`, feeding `events`
    /// as this frame's input. Reuses the same `ctx` across calls so widget
    /// ids / focus / drag memory persist frame-to-frame like a real app loop.
    fn run_pass(
        ctx: &egui::Context,
        state: &mut SwitcherState,
        time: f64,
        events: Vec<egui::Event>,
    ) -> (FieldResponses, egui::FullOutput) {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1000.0),
            )),
            time: Some(time),
            ..Default::default()
        };
        input.events = events;

        let mut captured: Option<FieldResponses> = None;
        let output = ctx.run_ui(input, |ui| {
            captured = Some(render_field_panel(ui, state));
        });
        (
            captured.expect("render_field_panel must be invoked by run_ui's closure"),
            output,
        )
    }

    /// A single-frame press+release at `pos` — egui treats this as a click,
    /// never a drag (drags require movement across >=2 frames).
    fn click_events(pos: egui::Pos2) -> Vec<egui::Event> {
        let modifiers = egui::Modifiers::default();
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers,
            },
        ]
    }

    /// Flattens every `Shape::Text` (recursing into `Shape::Vec`) drawn this
    /// pass into its plain string content.
    fn collect_text(output: &egui::FullOutput) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(nested) => {
                    for s in nested {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// Color-aware sibling of `collect_text`: flattens every `Shape::Text`
    /// (recursing into `Shape::Vec`) into `(text, baked color)`, reading the
    /// color `field_label` bakes into the galley's first layout section
    /// (b4-t8).
    fn collect_label_colors(output: &egui::FullOutput) -> Vec<(String, egui::Color32)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Color32)>) {
            match shape {
                egui::Shape::Text(t) => {
                    let color = t
                        .galley
                        .job
                        .sections
                        .first()
                        .map(|s| s.format.color)
                        .unwrap_or(egui::Color32::PLACEHOLDER);
                    out.push((t.galley.text().to_string(), color));
                }
                egui::Shape::Vec(nested) => {
                    for s in nested {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// b1-t3: flattens every `Shape::Rect` (recursing into `Shape::Vec`) that
    /// has a visible stroke (`stroke.width > 0.0`) into its `(rect, stroke)`
    /// pair. Used to find the frame chrome painted by `egui::Frame::show`.
    fn collect_stroked_rects(output: &egui::FullOutput) -> Vec<(egui::Rect, egui::Stroke)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Stroke)>) {
            match shape {
                egui::Shape::Rect(r) if r.stroke.width > 0.0 => {
                    out.push((r.rect, r.stroke));
                }
                egui::Shape::Vec(nested) => {
                    for s in nested {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// b1-t3: finds the stroked rect (from `collect_stroked_rects`) whose
    /// bounds nearly coincide with `target` (an `egui::Response::rect`),
    /// within a small float-precision epsilon.
    fn find_matching_stroke(
        stroked: &[(egui::Rect, egui::Stroke)],
        target: egui::Rect,
    ) -> Option<egui::Stroke> {
        const EPS: f32 = 1.0;
        stroked
            .iter()
            .find(|(rect, _)| {
                (rect.min - target.min).length() < EPS && (rect.max - target.max).length() < EPS
            })
            .map(|(_, stroke)| *stroke)
    }

    /// b4-t8: a dirty field's label must be drawn with a visibly distinct
    /// (YELLOW) color at the actual egui draw call, while the same field
    /// clean is not. Discriminating on the SAME field/schema so it fails if
    /// `field_label` ever stops branching on `dirty`.
    #[test]
    fn dirty_field_row_renders_distinct_highlight() {
        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![leaf("flag", FieldTag::Bool)])));
        state.panel_snapshot = serde_json::json!({"flag": false});

        let (_, clean_output) = run_pass(&ctx, &mut state, 0.0, vec![]);
        let clean_colors = collect_label_colors(&clean_output);
        assert!(
            !clean_colors
                .iter()
                .any(|(text, color)| text == "flag" && *color == egui::Color32::YELLOW),
            "a clean field's label must not be drawn with the dirty-highlight color; got {:?}",
            clean_colors
        );

        state.mark_dirty("flag", serde_json::json!(true));
        let (_, dirty_output) = run_pass(&ctx, &mut state, 1.0, vec![]);
        let dirty_colors = collect_label_colors(&dirty_output);
        assert!(
            dirty_colors
                .iter()
                .any(|(text, color)| text == "flag" && *color == egui::Color32::YELLOW),
            "a dirty field's label must be drawn with a visibly distinct (YELLOW) highlight; got {:?}",
            dirty_colors
        );
    }

    /// A bool field's checkbox click marks its path dirty with the flipped value.
    #[test]
    fn bool_checkbox_toggle_marks_dirty() {
        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![leaf("flag", FieldTag::Bool)])));
        state.panel_snapshot = serde_json::json!({"flag": false});

        let (responses, _) = run_pass(&ctx, &mut state, 0.0, vec![]);
        let rect = responses
            .get("flag")
            .expect("a Bool field must render a leaf Response keyed by its path \"flag\"")
            .rect;
        let center = rect.center();

        run_pass(&ctx, &mut state, 1.0, click_events(center));

        assert!(
            state.is_dirty("flag"),
            "clicking the bool checkbox must mark its path dirty"
        );
        assert_eq!(
            state.display_value("flag"),
            Some(&serde_json::json!(true)),
            "toggling the checkbox must flip false -> true in the dirty overlay"
        );
    }

    /// A readonly field's widget is disabled and a click on it never marks
    /// the field dirty (round-2 MEDIUM-1 fix).
    #[test]
    fn readonly_field_widget_is_disabled_and_uneditable() {
        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        let mut readonly_field = leaf("locked_count", FieldTag::Int);
        readonly_field.readonly = true;
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![readonly_field])));
        state.panel_snapshot = serde_json::json!({"locked_count": 5});

        let (responses, _) = run_pass(&ctx, &mut state, 0.0, vec![]);
        let resp = responses
            .get("locked_count")
            .expect("a readonly field must still render a leaf Response");
        assert!(
            !resp.enabled(),
            "a readonly field's widget must render disabled (Response::enabled() == false)"
        );
        let center = resp.rect.center();

        run_pass(&ctx, &mut state, 1.0, click_events(center));

        assert!(
            !state.is_dirty("locked_count"),
            "a click on a readonly field's widget must never call mark_dirty"
        );
    }

    /// `schema.label` wins when present; a field with no label falls back to
    /// its bare Rust field name (round-2 LOW-1 fix).
    #[test]
    fn label_falls_back_to_field_name() {
        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        let mut labeled = leaf("custom_field", FieldTag::Bool);
        labeled.label = Some("Custom Label".to_string());
        let bare = leaf("raw_name", FieldTag::Bool);
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![labeled, bare])));
        state.panel_snapshot = serde_json::json!({"custom_field": false, "raw_name": false});

        let (_, output) = run_pass(&ctx, &mut state, 0.0, vec![]);
        let texts = collect_text(&output);

        assert!(
            texts.iter().any(|t| t == "Custom Label"),
            "a field with schema.label = Some(..) must render that label text; drawn text: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "raw_name"),
            "a field with schema.label = None must render its bare field name; drawn text: {texts:?}"
        );
    }

    /// A list field renders exactly N element foldouts (N = current array
    /// length from panel_snapshot), and editing one element's nested field
    /// produces the indexed patch path `list[k].sub` — never touching a
    /// sibling element's path.
    #[test]
    fn list_renders_n_elements_and_indexed_edit() {
        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        let element = struct_schema("item", vec![leaf("nested", FieldTag::Bool)]);
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![list_schema("items", element)])));
        state.panel_snapshot = serde_json::json!({"items": [{"nested": false}, {"nested": true}]});

        let (responses, _) = run_pass(&ctx, &mut state, 0.0, vec![]);

        assert!(
            responses.contains_key("items[0].nested"),
            "element 0's nested field must render at patch path \"items[0].nested\"; got keys {:?}",
            responses.keys().collect::<Vec<_>>()
        );
        assert!(
            responses.contains_key("items[1].nested"),
            "element 1's nested field must render at patch path \"items[1].nested\"; got keys {:?}",
            responses.keys().collect::<Vec<_>>()
        );
        assert!(
            !responses.contains_key("items[2].nested"),
            "exactly N=2 element foldouts must render for a 2-length array, no 3rd element"
        );

        let center = responses["items[1].nested"].rect.center();
        run_pass(&ctx, &mut state, 1.0, click_events(center));

        assert!(
            state.is_dirty("items[1].nested"),
            "editing element 1's nested field must mark its indexed patch path dirty"
        );
        assert_eq!(
            state.display_value("items[1].nested"),
            Some(&serde_json::json!(false)),
            "element 1's nested bool (true) must flip to false"
        );
        assert!(
            !state.is_dirty("items[0].nested"),
            "editing element 1 must not mark element 0's path dirty"
        );
    }

    /// A Float field with a `range` clamps a drag past its bounds to the
    /// bound rather than letting the committed value overshoot.
    #[test]
    fn float_dragvalue_clamps_to_range() {
        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        let mut power = leaf("power", FieldTag::Float);
        power.range = Some(Range { min: 0.0, max: 10.0 });
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![power])));
        state.panel_snapshot = serde_json::json!({"power": 5.0});

        let (responses, _) = run_pass(&ctx, &mut state, 0.0, vec![]);
        let center = responses
            .get("power")
            .expect("a Float field must render a leaf Response")
            .rect
            .center();

        // Frame 2: press over the widget (no movement yet -> not a drag).
        run_pass(
            &ctx,
            &mut state,
            1.0,
            vec![
                egui::Event::PointerMoved(center),
                egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );

        // Frame 3: drag far to the right — egui only *decides* a gesture is a
        // drag (vs. a click) once movement exceeds the click threshold, which
        // requires a frame after the press with no press/release event in it.
        let far = egui::Pos2::new(center.x + 5000.0, center.y);
        run_pass(&ctx, &mut state, 1.05, vec![egui::Event::PointerMoved(far)]);

        // Frame 4: release.
        run_pass(
            &ctx,
            &mut state,
            1.1,
            vec![egui::Event::PointerButton {
                pos: far,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
        );

        assert!(
            state.is_dirty("power"),
            "dragging the float field must mark it dirty"
        );
        let value = state
            .display_value("power")
            .and_then(|v| v.as_f64())
            .expect("power's dirty value must be numeric");
        assert!(
            (0.0..=10.0).contains(&value),
            "dragging far past the schema range's max (10) must clamp the committed value into [0, 10], got {value}"
        );
        assert!(
            value > 5.0,
            "dragging to the right from a starting value of 5.0 must increase it, got {value}"
        );
    }

    /// Selecting a different Enum variant marks only that field's path
    /// dirty with the new variant name; a sibling Float field is unaffected
    /// (spec-15 point-5's UI half: `Piece.team`'s edit must not cascade).
    #[test]
    fn enum_select_variant_marks_only_that_path_dirty() {
        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        let team = enum_schema("team", vec!["A", "B"]);
        let power = leaf("power", FieldTag::Float);
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![team, power])));
        state.panel_snapshot = serde_json::json!({"team": "A", "power": 5.0});

        let (responses, _) = run_pass(&ctx, &mut state, 0.0, vec![]);
        let center = responses
            .get("team")
            .expect("an Enum field must render a leaf Response (the ComboBox button) keyed by its path \"team\"")
            .rect
            .center();

        // Click the ComboBox button to open its popup. Whether the popup's
        // items render this same frame or need a follow-up frame is an
        // egui-internal timing detail, so re-render (no new events) a few
        // times until the "B" option's composite-keyed Response appears.
        let (mut responses, _) = run_pass(&ctx, &mut state, 1.0, click_events(center));
        let mut t = 1.0;
        while !responses.contains_key("team::B") && t < 1.3 {
            t += 0.05;
            let (r, _) = run_pass(&ctx, &mut state, t, vec![]);
            responses = r;
        }
        let variant_b_center = responses
            .get("team::B")
            .expect("opening the \"team\" ComboBox must render a \"B\" option Response keyed \"team::B\"")
            .rect
            .center();

        run_pass(&ctx, &mut state, t + 0.05, click_events(variant_b_center));

        assert!(
            state.is_dirty("team"),
            "selecting variant \"B\" must mark \"team\" dirty"
        );
        assert_eq!(
            state.display_value("team"),
            Some(&serde_json::json!("B")),
            "the enum field's dirty value must be the newly selected variant name"
        );
        assert!(
            !state.is_dirty("power"),
            "selecting a new enum variant must not mark the sibling float field dirty"
        );
    }

    // ── b4-t6: Color widget arm (RGB-only, alpha bit-identical) ────────────

    /// MANDATORY (point 8): editing R/G/B through the widget's edit code
    /// path must carry the pre-edit alpha byte through bit-identical, for
    /// two distinct non-0xFF/non-0x00 alphas (proves it's a carry-through,
    /// not a hard-coded constant).
    #[test]
    fn recolor_keep_alpha_carries_original_alpha_bit_identical() {
        let current = serde_json::json!("#00000080");
        let out = recolor_keep_alpha(Some(&current), [0xc8, 0x1e, 0x1e]);
        let hex = out
            .as_str()
            .expect("recolor_keep_alpha must emit a JSON string value");
        let rgba = Rgba::from_hex(hex).expect("recolor_keep_alpha must emit a valid \"#rrggbbaa\" string");
        assert_eq!(
            rgba.a, 0x80,
            "alpha must be carried through bit-identical, got {:#04x}",
            rgba.a
        );
        assert_eq!((rgba.r, rgba.g, rgba.b), (0xc8, 0x1e, 0x1e));

        let current2 = serde_json::json!("#1122337f");
        let out2 = recolor_keep_alpha(Some(&current2), [0x01, 0x02, 0x03]);
        let hex2 = out2.as_str().expect("JSON string value");
        let rgba2 = Rgba::from_hex(hex2).expect("valid \"#rrggbbaa\" string");
        assert_eq!(
            rgba2.a, 0x7f,
            "a second, different original alpha must also be carried through unchanged"
        );
        assert_eq!((rgba2.r, rgba2.g, rgba2.b), (0x01, 0x02, 0x03));
    }

    /// Missing or malformed current value falls back to opaque black —
    /// never panics.
    #[test]
    fn read_color_falls_back_to_opaque_black_on_missing_or_malformed() {
        assert_eq!(
            read_color(None),
            Rgba::rgb(0, 0, 0),
            "a missing value must fall back to opaque black"
        );
        let garbage = serde_json::json!("garbage");
        assert_eq!(
            read_color(Some(&garbage)),
            Rgba::rgb(0, 0, 0),
            "a malformed hex value must fall back to opaque black, never panic"
        );
    }

    /// A `Color`-tagged leaf renders an INTERACTIVE (enabled) widget at its
    /// path — distinguishes the real widget arm from the old read-only
    /// placeholder shared with `Json`.
    #[test]
    fn color_leaf_renders_enabled_interactive_widget() {
        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![leaf("tint", FieldTag::Color)])));
        state.panel_snapshot = serde_json::json!({"tint": "#c81e1e80"});

        let (responses, _) = run_pass(&ctx, &mut state, 0.0, vec![]);
        let resp = responses
            .get("tint")
            .expect("a Color field must render a leaf Response keyed by its path \"tint\"");
        assert!(
            resp.enabled(),
            "a non-readonly Color field must render an INTERACTIVE widget, not the old read-only placeholder"
        );
    }

    // ── b4-t7: Json read-only fallback widget arm ──────────────────────────

    /// A data-carrying enum field (derive-tagged `Json`, `readonly: true`)
    /// renders as non-interactive, pretty-printed JSON text, and no
    /// interaction on it can ever mark it dirty.
    #[test]
    fn json_field_renders_noninteractive_pretty_printed() {
        use scene_core::Inspectable as _;

        // `Empty` is never constructed in this test — it exists so `Payload`
        // is a realistic multi-variant data-carrying enum, not a red flag.
        #[allow(dead_code)]
        #[derive(serde::Serialize, scene_core::Inspectable)]
        enum Payload {
            Loaded { hp: u32, mana: u32 },
            Empty,
        }

        #[derive(scene_core::Inspectable)]
        struct Carrier {
            payload: Payload,
        }

        let ctx = egui::Context::default();
        let mut state = SwitcherState::new();
        state.panel_schema = Some(Rc::new(Carrier::schema()));
        state.panel_snapshot = Carrier {
            payload: Payload::Loaded { hp: 5, mana: 9 },
        }
        .snapshot();

        let (responses, output) = run_pass(&ctx, &mut state, 0.0, vec![]);
        let resp = responses
            .get("payload")
            .expect("a Json field must render a leaf Response keyed by its path \"payload\"");
        assert!(
            !resp.enabled(),
            "a Json-tagged field's widget must render disabled (Response::enabled() == false)"
        );

        let expected = serde_json::to_string_pretty(state.display_value("payload").unwrap())
            .expect("value must serialize");
        let texts = collect_text(&output);
        assert!(
            texts.contains(&expected),
            "the Json fallback arm must render the value PRETTY-PRINTED (multi-line), got texts: {texts:?}, expected: {expected:?}"
        );

        let center = resp.rect.center();
        run_pass(&ctx, &mut state, 1.0, click_events(center));

        assert!(
            !state.is_dirty("payload"),
            "a click on the Json fallback widget must never mark its path dirty"
        );
        assert!(
            !state.dirty_patch().contains_key("payload"),
            "a Json field must never appear in dirty_patch()'s output"
        );
    }

    /// Like `stub_schema` but with `field_count` synthetic `Bool` children, so
    /// distinct schemas are actually distinct in shape (not four identical stubs).
    fn stub_schema_with_fields(name: &str, field_count: usize) -> FieldSchema {
        let mut s = stub_schema(name);
        s.children = (0..field_count)
            .map(|i| FieldSchema {
                name: format!("f{i}"),
                label: None,
                tag: FieldTag::Bool,
                readonly: false,
                hidden: false,
                range: None,
                children: vec![],
                variants: vec![],
            })
            .collect();
        s
    }

    fn four_scene_hello() -> Message {
        Message::Hello(Hello {
            scenes: vec![
                CatalogEntry {
                    id: SceneKey::new("MainHub"),
                    name: "Main Hub".to_string(),
                    schema: stub_schema_with_fields("MainHub", 1),
                },
                CatalogEntry {
                    id: SceneKey::new("BattleViewer"),
                    name: "Battle Viewer".to_string(),
                    schema: stub_schema_with_fields("BattleViewer", 2),
                },
                CatalogEntry {
                    id: SceneKey::new("RosterManager"),
                    name: "Roster".to_string(),
                    schema: stub_schema_with_fields("RosterManager", 3),
                },
                CatalogEntry {
                    id: SceneKey::new("Leaderboard"),
                    name: "Leaderboard".to_string(),
                    schema: stub_schema_with_fields("Leaderboard", 4),
                },
            ],
            active: SceneKey::new("MainHub"),
        })
    }

    /// A second, disjoint Hello (different scene set) — used to prove a fresh
    /// Hello REBUILDS `schema_cache` rather than merging into it.
    fn two_scene_hello_distinct() -> Message {
        Message::Hello(Hello {
            scenes: vec![
                CatalogEntry {
                    id: SceneKey::new("RosterManager"),
                    name: "Roster".to_string(),
                    schema: stub_schema_with_fields("RosterManager2", 5),
                },
                CatalogEntry {
                    id: SceneKey::new("Leaderboard"),
                    name: "Leaderboard".to_string(),
                    schema: stub_schema_with_fields("Leaderboard2", 6),
                },
            ],
            active: SceneKey::new("Leaderboard"),
        })
    }

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

    /// When the stub server closes the socket, pump() must propagate the EOF
    /// through the reader-thread channel to state.should_exit() == true.
    /// RED: set_disconnected() does not yet set should_exit, so the loop times out.
    #[test]
    fn pump_sets_should_exit_on_eof() {
        use crate::client::connect;
        use scene_core::ipc::{write_frame, Envelope};
        use std::os::unix::net::UnixListener;
        use std::time::Duration;

        let path = std::env::temp_dir().join(format!(
            "inspector-app-test-eof-{}.sock",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind tmp socket");
        let path2 = path.clone();

        // Stub server: write a Hello, then drop the stream (EOF).
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let env = Envelope::new(0, None, four_scene_hello());
            let _ = write_frame(&mut stream, &env);
            // stream drops here → EOF on the client side
        });

        let client = connect(&path2).expect("connect must succeed");
        let mut app = InspectorApp::new(client);

        // Loop pump() until should_exit flips or 2 s timeout.
        let mut exited = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            app.pump();
            if app.state.should_exit() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = std::fs::remove_file(&path2);

        assert!(
            exited,
            "app.state.should_exit() must become true within 2 s of socket close"
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

    use scene_core::ipc::StateSnapshot;

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

    // ── b4-t9: Submit / Revert / Apply-on-change wired to the wire protocol ──

    /// Test harness: accepts one connection, forwards every frame the client
    /// sends to the returned receiver, and hands back a cloned write half so
    /// the test can push server -> client replies (Ack/StateSnapshot)
    /// directly, without a second background thread racing the assertions.
    fn stub_app_harness(
        tag: &str,
    ) -> (
        InspectorApp,
        std::sync::mpsc::Receiver<scene_core::ipc::Envelope>,
        std::os::unix::net::UnixStream,
        std::path::PathBuf,
    ) {
        use crate::client::connect;
        use scene_core::ipc::read_frame;
        use std::os::unix::net::UnixListener;

        let path = std::env::temp_dir().join(format!(
            "inspector-app-test-b4t9-{}-{}.sock",
            std::process::id(),
            tag,
        ));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind tmp socket");

        let (frame_tx, frame_rx) = std::sync::mpsc::channel();
        let (stream_tx, stream_rx) = std::sync::mpsc::sync_channel(1);

        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let write_half = stream.try_clone().expect("clone stream");
            stream_tx.send(write_half).ok();
            let mut read_half = stream;
            while let Ok(env) = read_frame(&mut read_half) {
                if frame_tx.send(env).is_err() {
                    break;
                }
            }
        });

        let client = connect(&path).expect("connect must succeed");
        let app = InspectorApp::new(client);
        let server_write = stream_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("server must accept and hand back a write half");

        (app, frame_rx, server_write, path)
    }

    // ── b1-t1: full-width stacked layout (replaces `egui::Panel::right`) ────

    /// `render_body` must lay out the scene-selector row and the
    /// field-editor section stacked (selector above editor), each stretched
    /// to (most of) the viewport width — not a narrow docked side-panel
    /// strip. This is the exact user-reported bug: `egui::Panel::right`
    /// leaves the selector row confined to a narrow top-left strip.
    #[test]
    fn body_sections_are_full_width_and_stacked() {
        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("layout");
        app.state.apply(&four_scene_hello());

        let ctx = egui::Context::default();
        let viewport_w = 1000.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(viewport_w, 1000.0),
            )),
            time: Some(0.0),
            ..Default::default()
        };

        let mut captured: Option<(egui::Response, egui::Response)> = None;
        let _ = ctx.run_ui(input, |ui| {
            captured = Some(app.render_body(ui));
        });
        let (selector, editor) =
            captured.expect("render_body must be invoked by run_ui's closure");

        let min_w = 0.9 * viewport_w;
        assert!(
            selector.rect.width() >= min_w,
            "selector row must span (most of) the full viewport width, got {} of {}",
            selector.rect.width(),
            viewport_w
        );
        assert!(
            editor.rect.width() >= min_w,
            "field-editor section must span (most of) the full viewport width, got {} of {}",
            editor.rect.width(),
            viewport_w
        );
        assert!(
            editor.rect.min.y >= selector.rect.max.y,
            "field-editor section must be stacked below the selector row, not beside it \
             (selector bottom {}, editor top {})",
            selector.rect.max.y,
            editor.rect.min.y
        );

        let _ = std::fs::remove_file(&path);
    }

    // ── b1-t2: deliberate vertical gap between the two sections ─────────────

    /// The vertical gap between the selector row's bottom and the
    /// field-editor section's top must be bounded below by `SECTION_GAP`, a
    /// real, deliberate value — not egui's ~3px default `item_spacing.y` and
    /// not a token 0.
    #[test]
    fn body_sections_have_deliberate_vertical_gap() {
        const {
            assert!(
                SECTION_GAP >= 8.0,
                "SECTION_GAP must be a deliberate value clearly above egui's \
                 default item_spacing.y (~3.0)"
            );
        }

        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("gap");
        app.state.apply(&four_scene_hello());

        let ctx = egui::Context::default();
        let viewport_w = 1000.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(viewport_w, 1000.0),
            )),
            time: Some(0.0),
            ..Default::default()
        };

        let mut captured: Option<(egui::Response, egui::Response)> = None;
        let _ = ctx.run_ui(input, |ui| {
            captured = Some(app.render_body(ui));
        });
        let (selector, editor) =
            captured.expect("render_body must be invoked by run_ui's closure");

        let gap = editor.rect.min.y - selector.rect.max.y;
        assert!(
            gap >= SECTION_GAP,
            "vertical gap between selector and field-editor sections must be \
             >= SECTION_GAP ({SECTION_GAP}), got {gap}"
        );

        let _ = std::fs::remove_file(&path);
    }

    // ── b1-t3: consistent framing between the two sections ──────────────────

    /// Both top-level sections must be enclosed by a visibly stroked frame,
    /// and those two frames must be identical (same stroke width and color)
    /// — a sameness contract, never a specific pixel value (spec explicitly
    /// defers exact styling to implementer judgment).
    #[test]
    fn both_sections_share_identical_frame_styling() {
        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("frame-style");
        app.state.apply(&four_scene_hello());

        let ctx = egui::Context::default();
        let viewport_w = 1000.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(viewport_w, 1000.0),
            )),
            time: Some(0.0),
            ..Default::default()
        };

        let mut captured: Option<(egui::Response, egui::Response)> = None;
        let output = ctx.run_ui(input, |ui| {
            captured = Some(app.render_body(ui));
        });
        let (selector, editor) =
            captured.expect("render_body must be invoked by run_ui's closure");

        let stroked = collect_stroked_rects(&output);

        let selector_stroke = find_matching_stroke(&stroked, selector.rect).expect(
            "the scene-selector section must be enclosed by a visibly stroked frame",
        );
        let editor_stroke = find_matching_stroke(&stroked, editor.rect).expect(
            "the field-editor section must be enclosed by a visibly stroked frame",
        );

        assert!(
            selector_stroke.width > 0.0 && editor_stroke.width > 0.0,
            "both sections' frames must have a non-zero stroke width; got selector={:?}, editor={:?}",
            selector_stroke,
            editor_stroke
        );
        assert_eq!(
            selector_stroke.width, editor_stroke.width,
            "both sections must share the same frame stroke width"
        );
        assert_eq!(
            selector_stroke.color, editor_stroke.color,
            "both sections must share the same frame stroke color"
        );

        let _ = std::fs::remove_file(&path);
    }

    // ── b1-t4: scroll container around the field-editor content ────────────

    /// A schema at least as large as `BattleViewer`'s (a 12-element list of
    /// multi-field structs), rendered into a height-constrained viewport,
    /// must produce a laid-out content height that exceeds the visible
    /// scroll viewport — proving `scroll_field_panel` puts a real
    /// `egui::ScrollArea` in the render path (a plain `Ui` would just clip
    /// silently, with `content_size ≈ inner_rect`).
    #[test]
    fn field_panel_scrolls_when_content_exceeds_viewport() {
        let element = struct_schema(
            "piece",
            vec![
                leaf("a", FieldTag::Int),
                leaf("b", FieldTag::Float),
                leaf("c", FieldTag::Bool),
            ],
        );
        let mut state = SwitcherState::new();
        state.panel_schema = Some(Rc::new(struct_schema("Root", vec![list_schema("pieces", element)])));

        let pieces: Vec<serde_json::Value> = (0..12)
            .map(|_| serde_json::json!({"a": 0, "b": 0.0, "c": false}))
            .collect();
        state.panel_snapshot = serde_json::json!({ "pieces": pieces });

        let ctx = egui::Context::default();
        let viewport = egui::vec2(400.0, 150.0);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, viewport)),
            time: Some(0.0),
            ..Default::default()
        };

        let mut captured: Option<egui::scroll_area::ScrollAreaOutput<FieldResponses>> = None;
        let _ = ctx.run_ui(input, |ui| {
            captured = Some(scroll_field_panel(ui, &mut state));
        });
        let out = captured.expect("scroll_field_panel must be invoked by run_ui's closure");

        assert!(
            out.content_size.y > viewport.y,
            "a 12-element list's full content height ({}) must exceed the \
             constrained viewport height ({})",
            out.content_size.y,
            viewport.y
        );
        assert!(
            out.content_size.y > out.inner_rect.height(),
            "content must overflow the clipped visible viewport, proving a real \
             egui::ScrollArea is in the render path (got content_size.y={}, \
             inner_rect.height()={})",
            out.content_size.y,
            out.inner_rect.height()
        );
        assert!(
            !out.inner.is_empty(),
            "scroll_field_panel must still return the wrapped panel's field Responses"
        );
    }

    // ── b1-t1: action row stays reachable when the field list overflows ────

    /// The Submit/Revert action row must be reachable (its rendered
    /// `Response` rects must lie entirely within the visible screen rect)
    /// through the real production wiring (`app.render`, not `render_body`
    /// alone), even when a 12-element list-of-structs field panel (at least
    /// as large as `BattleViewer`'s) overflows a deliberately small
    /// viewport. Regression guard for the unbounded-`ScrollArea` bug that
    /// could push the action row below the visible window.
    #[test]
    fn action_row_stays_within_viewport_when_field_list_overflows() {
        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("action-row-pin");
        app.state.apply(&four_scene_hello());

        let element = struct_schema(
            "piece",
            vec![
                leaf("a", FieldTag::Int),
                leaf("b", FieldTag::Float),
                leaf("c", FieldTag::Bool),
            ],
        );
        app.state.panel_schema = Some(Rc::new(struct_schema("Root", vec![list_schema("pieces", element)])));
        let pieces: Vec<serde_json::Value> = (0..12)
            .map(|_| serde_json::json!({"a": 0, "b": 0.0, "c": false}))
            .collect();
        app.state.panel_snapshot = serde_json::json!({ "pieces": pieces });

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let input = egui::RawInput {
            screen_rect: Some(screen_rect),
            time: Some(0.0),
            ..Default::default()
        };

        // Bottom-panel height negotiation needs one settle frame (egui
        // remembers last frame's measured content height when placing this
        // frame's panel) before the allocated rect reflects the padding
        // added by ACTION_ROW_PADDING — mirrors the settle-frame pattern
        // used elsewhere in this file for popups.
        let mut captured: Option<ActionRow> = None;
        for t in [0.0, 1.0] {
            let mut input = input.clone();
            input.time = Some(t);
            let _ = ctx.run_ui(input, |ui| {
                captured = Some(app.render(ui));
            });
        }
        let row = captured.expect("app.render must be invoked by run_ui's closure");

        assert!(
            row.submit.rect.min.y >= screen_rect.min.y && row.submit.rect.max.y <= screen_rect.max.y,
            "Submit button rect ({:?}) must lie entirely within the screen rect ({:?}) \
             even when the field list overflows",
            row.submit.rect,
            screen_rect
        );
        assert!(
            row.revert.rect.min.y >= screen_rect.min.y && row.revert.rect.max.y <= screen_rect.max.y,
            "Revert button rect ({:?}) must lie entirely within the screen rect ({:?}) \
             even when the field list overflows",
            row.revert.rect,
            screen_rect
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Calls `app.pump()` in a loop until `pred` is true or `timeout` elapses.
    /// Returns whether `pred` became true.
    fn pump_until(
        app: &mut InspectorApp,
        mut pred: impl FnMut(&InspectorApp) -> bool,
        timeout: std::time::Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            app.pump();
            if pred(app) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Submitting a dirty field sends exactly one `ApplyState` whose `patch`
    /// is exactly the dirty overlay and whose `id` is the active scene, and
    /// marks the submit in-flight.
    #[test]
    fn submit_sends_apply_state_with_only_dirty_patch_and_marks_awaiting() {
        use scene_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("submit");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        assert!(
            pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2)),
            "app must observe Hello before submit"
        );

        app.state.mark_dirty("f0", serde_json::json!(true));
        app.submit();

        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ApplyState must arrive at the stub after submit");
        match env.body {
            Message::ApplyState(a) => {
                assert_eq!(
                    a.id,
                    SceneKey::new("MainHub"),
                    "submit's ApplyState must target the active scene"
                );
                let mut expected = BTreeMap::new();
                expected.insert("f0".to_string(), serde_json::json!(true));
                assert_eq!(
                    a.patch, expected,
                    "submit's ApplyState.patch must contain exactly the dirty overlay"
                );
            }
            other => panic!("expected ApplyState, got {:?}", other),
        }
        assert!(
            app.state.awaiting_submit,
            "submit must mark the submit in-flight (awaiting_submit)"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Submitting with an empty dirty buffer sends nothing over the socket.
    #[test]
    fn submit_with_empty_dirty_sends_nothing() {
        use scene_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("submit-empty");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2));

        app.submit();

        match frame_rx.recv_timeout(Duration::from_millis(200)) {
            Err(_) => {} // expected: nothing arrived
            Ok(env) => panic!(
                "submit with an empty dirty buffer must send nothing, got {:?}",
                env.body
            ),
        }

        let _ = std::fs::remove_file(&path);
    }

    /// After a Submit, feeding an `Ack` then a `StateSnapshot` for the active
    /// scene clears the dirty buffer and refreshes the displayed value from
    /// the new snapshot.
    #[test]
    fn ack_then_state_snapshot_after_submit_clears_dirty_and_refreshes() {
        use scene_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("submit-reply");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2));

        app.state.mark_dirty("f0", serde_json::json!(true));
        app.submit();
        let _ = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ApplyState must arrive after submit");

        write_frame(&mut server_write, &Envelope::new(1, None, Message::Ack)).expect("write Ack");
        write_frame(
            &mut server_write,
            &Envelope::new(
                2,
                None,
                Message::StateSnapshot(StateSnapshot {
                    id: SceneKey::new("MainHub"),
                    snapshot: serde_json::json!({"f0": true}),
                }),
            ),
        )
        .expect("write StateSnapshot");

        let cleared = pump_until(
            &mut app,
            |a| a.state.dirty_patch().is_empty(),
            Duration::from_secs(2),
        );
        assert!(
            cleared,
            "the dirty buffer must clear once Ack + StateSnapshot arrive after submit"
        );
        assert_eq!(
            app.state.display_value("f0"),
            Some(&serde_json::json!(true)),
            "display_value must reflect the refreshed StateSnapshot once the dirty overlay clears"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Revert with no prior submit performs no socket I/O and clears the
    /// dirty buffer.
    #[test]
    fn revert_sends_nothing_and_clears_dirty() {
        use scene_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("revert");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2));

        app.state.mark_dirty("f0", serde_json::json!(true));
        app.revert();

        assert!(
            app.state.dirty_patch().is_empty(),
            "revert must clear the dirty buffer"
        );

        match frame_rx.recv_timeout(Duration::from_millis(200)) {
            Err(_) => {} // expected: nothing arrived
            Ok(env) => panic!("revert must perform no socket I/O, got {:?}", env.body),
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── b2-t2: gate Revert on dirty_empty (mirror Submit's existing gate) ──

    /// The Revert button must render disabled while the dirty buffer is
    /// empty and enabled once a field has been marked dirty — mirroring
    /// Submit's existing `dirty_empty` gate exactly (round-2 issue 6).
    #[test]
    fn revert_button_disabled_when_dirty_empty_enabled_when_dirty() {
        let (mut app, _frame_rx, _server_write, path) = stub_app_harness("revert-gate");
        app.state.apply(&four_scene_hello());

        let ctx = egui::Context::default();

        let mut captured: Option<ActionRow> = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1000.0),
                )),
                time: Some(0.0),
                ..Default::default()
            },
            |ui| {
                captured = Some(app.render_action_row(ui));
            },
        );
        let row =
            captured.expect("render_action_row must be invoked by run_ui's closure");
        assert!(
            !row.revert.enabled(),
            "Revert must render disabled when the dirty buffer is empty"
        );

        app.state.mark_dirty("f0", serde_json::json!(true));

        let mut captured: Option<ActionRow> = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1000.0),
                )),
                time: Some(1.0),
                ..Default::default()
            },
            |ui| {
                captured = Some(app.render_action_row(ui));
            },
        );
        let row =
            captured.expect("render_action_row must be invoked by run_ui's closure");
        assert!(
            row.revert.enabled(),
            "Revert must render enabled once a field is marked dirty"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Enabling "apply on change" sends exactly one `Subscribe{live:true}` on
    /// the transition (a repeat call does not resend), and a subsequent edit
    /// sends its own `ApplyState` without waiting for Submit.
    #[test]
    fn set_live_apply_sends_subscribe_once_then_edit_sends_own_apply_state() {
        use scene_core::ipc::{write_frame, Envelope};
        use std::time::Duration;

        let (mut app, frame_rx, mut server_write, path) = stub_app_harness("live-apply");

        write_frame(&mut server_write, &Envelope::new(0, None, four_scene_hello()))
            .expect("write Hello");
        pump_until(&mut app, |a| a.state.connected, Duration::from_secs(2));

        // live_apply now defaults to true (b2-t1); toggle it off first so the
        // subsequent ON transition below is a real transition to observe.
        app.set_live_apply(false);
        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Subscribe must arrive on the OFF transition from the true default");
        match env.body {
            Message::Subscribe(s) => assert!(!s.live, "Subscribe.live must be false"),
            other => panic!("expected Subscribe, got {:?}", other),
        }

        app.set_live_apply(true);
        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Subscribe must arrive on the ON transition");
        match env.body {
            Message::Subscribe(s) => assert!(s.live, "Subscribe.live must be true"),
            other => panic!("expected Subscribe, got {:?}", other),
        }

        // Re-enabling an already-on toggle must not resend (the "once" contract).
        app.set_live_apply(true);
        match frame_rx.recv_timeout(Duration::from_millis(200)) {
            Err(_) => {} // expected: nothing arrived
            Ok(env) => panic!(
                "re-enabling an already-on live_apply must not resend Subscribe, got {:?}",
                env.body
            ),
        }

        app.state.mark_dirty("f0", serde_json::json!(true));
        app.flush_edits();

        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("a live edit must send its own ApplyState");
        match env.body {
            Message::ApplyState(a) => {
                assert_eq!(a.id, SceneKey::new("MainHub"));
                let mut expected = BTreeMap::new();
                expected.insert("f0".to_string(), serde_json::json!(true));
                assert_eq!(
                    a.patch, expected,
                    "the live ApplyState must carry exactly the edited field"
                );
            }
            other => panic!("expected ApplyState, got {:?}", other),
        }

        let _ = std::fs::remove_file(&path);
    }

    // ── b2-t1: auto-apply default + startup Subscribe ──────────────────────

    /// `InspectorApp::new` must default `live_apply` to `true` — the
    /// "Apply on change" checkbox starts checked (spec: auto-apply on by
    /// default).
    #[test]
    fn new_defaults_live_apply_to_true() {
        let (app, _frame_rx, _server_write, path) = stub_app_harness("default-live-apply");

        assert!(
            app.live_apply,
            "InspectorApp::new must default live_apply to true"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Calling `start()` once at startup must result in exactly one framed
    /// `Subscribe{live: true}` arriving at the connected server — proving the
    /// wire message is actually sent, not just that the field defaults to
    /// true.
    #[test]
    fn start_sends_subscribe_live_true_at_startup() {
        use std::time::Duration;

        let (mut app, frame_rx, _server_write, path) = stub_app_harness("start-subscribe");

        app.start();

        let env = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Subscribe must arrive at the server after start()");
        match env.body {
            Message::Subscribe(s) => assert!(
                s.live,
                "start() must send Subscribe{{live:true}} reflecting the true default"
            ),
            other => panic!("expected Subscribe, got {:?}", other),
        }

        let _ = std::fs::remove_file(&path);
    }
}
