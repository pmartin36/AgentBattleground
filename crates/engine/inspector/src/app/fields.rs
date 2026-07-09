use std::collections::HashMap;

use engine_core::color::Rgba;
use engine_core::inspect::{FieldSchema, FieldTag};

use super::state::SwitcherState;

/// Per-leaf-field rendered `Response`, keyed by its full patch path.
/// Populated by `render_field_panel`; production ignores it, tests use it to
/// locate widgets / assert enabled state (b4-t4).
pub(crate) type FieldResponses = HashMap<String, egui::Response>;

/// Shared bordered frame used for both top-level `render_body` sections
/// (b1-t3): calling this twice with the same `ui.style()` yields two
/// byte-identical frames, so the two sections read as framed alike without
/// duplicating a `Frame::group` literal at each call site.
pub(crate) fn section_frame(style: &egui::Style) -> egui::Frame {
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
pub(crate) fn scroll_field_panel(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_fixtures::*;
    use engine_core::inspect::Range;
    use std::rc::Rc;

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
        use engine_core::Inspectable as _;

        // `Empty` is never constructed in this test — it exists so `Payload`
        // is a realistic multi-variant data-carrying enum, not a red flag.
        #[allow(dead_code)]
        #[derive(serde::Serialize, engine_core::Inspectable)]
        enum Payload {
            Loaded { hp: u32, mana: u32 },
            Empty,
        }

        #[derive(engine_core::Inspectable)]
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
}
