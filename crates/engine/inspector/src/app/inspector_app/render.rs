use super::*;
use crate::app::fields::{scroll_field_panel, section_frame};

/// Deliberate vertical gap (px) between the scene-selector row and the
/// field-editor section in `InspectorApp::render_body` (b1-t2). Must exceed
/// egui's default `item_spacing.y` (~3.0) so the sections read as visually
/// distinct, not jammed together.
const SECTION_GAP: f32 = 12.0;
const ACTION_ROW_PADDING: f32 = 6.0;

impl InspectorApp {
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
    pub(super) fn render_action_row(&mut self, ui: &mut egui::Ui) -> ActionRow {
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
    pub(super) fn render(&mut self, ui: &mut egui::Ui) -> ActionRow {
        let action = egui::Panel::bottom("inspector_action_row")
            .show(ui, |ui| self.render_action_row(ui))
            .inner;
        egui::CentralPanel::default().show(ui, |ui| {
            let _ = self.render_body(ui);
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_fixtures::*;
    use crate::app::fields::FieldResponses;
    use engine_core::inspect::FieldTag;
    use std::rc::Rc;

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

}
