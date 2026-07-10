# Large-File Split Plan

Not a spec — this doesn't go through the TDD pipeline (project owner's explicit call: one-off mechanical refactor, no new behavior). Parked here as a reference so the work is captured without blocking on it. Target: every file under ~1000 lines. Execute in the order listed; run the full gate (`cargo build --workspace --all-targets && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`) after each step, whether within a file's own multi-step split or after a single-file mechanical move.

## Overview

| File | Now | Action | Largest resulting file |
|---|---|---|---|
| `crates/engine/render/src/button.rs` | 1177 | Move test tail to sibling file | ~363 (prod) / ~815 (tests) |
| `crates/engine/render/src/camera.rs` | 1018 | Move test tail to sibling file | ~390 (prod) / ~630 (tests) |
| `crates/engine/inspector/src/app.rs` | 2599 | Module-tree split (4 files) | ~940 |
| `crates/game/src/scenes/roster_manager.rs` | 4759 | Module-tree split (9 files) | ~880 |
| `crates/game/src/scenes/battle_viewer.rs` | 5150 | Module-tree split (9 files) | ~920 |

Recommended order: `button.rs` and `camera.rs` first (trivial, near-zero risk), then `inspector/app.rs` (cleanest dependency graph), then `roster_manager.rs`, then `battle_viewer.rs` last (biggest, most test modules).

Recurring pattern across the three module-tree splits: a struct's private fields must stay in the file that defines the struct (Rust privacy cascades to descendant modules, not siblings), so functions relocated out to sibling files need `pub(super)`; each also gets one dedicated test-only file for whole-scene integration/regression suites that don't belong to a single narrower topic — a deliberate, named exception to "tests live with their code," not an oversight.

---

## 1. `button.rs` (1177 → trivial split)

Lines 1-363 are production code (`ButtonState`, `ButtonCore`, `render_tinted`, `Button`, `FrameButton`). Lines 365-1177 (~70% of the file) are four `#[cfg(test)]` modules plus shared PNG fixtures, touching only `super::*`, with zero coupling to each other beyond that and physically contiguous at the file's tail.

**Action:** move lines 365-1177 verbatim into a sibling file (e.g. `button_tests.rs`, included via `#[cfg(test)] #[path = "button_tests.rs"] mod button_tests;`). Zero logic changes — every test byte-for-byte identical, just relocated. Drops `button.rs` to ~363 lines.

## 2. `camera.rs` (1018 → trivial split)

Same shape as `button.rs`: `WorldPos`/`Camera` trait/`DepthAxis`/`axis_values`/`SideView`/`OrthographicCamera`/`PerspectiveCamera`/`FreeRoamCamera`/`AnyCamera` (~388 lines of production code) followed by one large `#[cfg(test)] mod tests` (~630 lines) covering all five camera kinds' projection/depth-key/anchor/elevation behavior plus `AnyCamera`'s delegation.

**Action:** same move as `button.rs` — relocate the test module to a sibling file. Only 18 lines over target, and the camera kinds are cross-tested together (comparative "AnyCamera delegates correctly" assertions), so a topic-based split would fragment cohesive coverage for no real benefit. (Note: this file was mid-refactor — spec 42's `AnyCamera`/`FreeRoamCamera` work — when first profiled; it has since landed, so the 1018-line count above is final, not transitional.)

## 3. `inspector/app.rs` (2599 → 4-file module tree)

| File | Contents | Est. lines |
|---|---|---|
| `app/mod.rs` | doc + `mod` decls + re-exports | ~25 |
| `app/state.rs` | `SwitcherState` (the pure, egui-free controller layer) + `navigate` + its own reducer tests | ~630 |
| `app/fields.rs` | `FieldResponses`, `section_frame`, `render_field_panel`, `scroll_field_panel`, `render_numeric_field`, `render_field`, `interactive`, `field_label`, `read_color`, `recolor_keep_alpha` + widget/color/json/scroll tests | ~880 |
| `app/inspector_app.rs` | `InspectorApp`, `ActionRow`, the `eframe::App` impl, both consts + all lifecycle/layout tests | ~940 |
| `app/test_fixtures.rs` (test-only) | `leaf`, `struct_schema`, `list_schema`, `four_scene_hello`, `stub_schema_with_fields`, `two_scene_hello_distinct` — fixtures genuinely shared across 2+ of the modules above | ~120 |

Clean DAG: `state.rs` has no outgoing deps on the other two; `fields.rs` depends only on `state.rs`; `inspector_app.rs` depends on both.

**Complications:**
- `section_frame`/`scroll_field_panel` need `pub(crate)` — `InspectorApp::render_body` calls both, a real production cross-module call, not just test sharing.
- Splitting `InspectorApp` further (e.g. render methods vs. lifecycle methods into separate sibling files) would require widening its private fields to `pub(crate)`/`pub(super)` — not worth it at ~940 lines; left as one file.
- `render_field`'s 189-line match (9 arms) is the single largest inherently-hard-to-split unit — a genuine future refactor of its own, not a module-boundary problem, out of scope here.

## 4. `roster_manager.rs` (4759 → 9-file module tree)

| File | Contents | Est. lines |
|---|---|---|
| `roster_manager/mod.rs` | `RosterManager`, `Slide`, `Direction`, `Default`, `impl Scene` (dispatches into submodules), `new`, `navigate`, `blink_on`, `toggle_selection`, `active_slide`, `slide_offsets` + `tests`, `selection_tests`, `slide_transition_tests`, `arrow_key_navigation_tests` | ~880 |
| `roster_manager/layout.rs` | `RosterLayout`, `cell_rect_to_dots`, `top_bands_dots`, `left_col_dots`, `right_col_dots`, `layout`, `dot_bands`, `dot_cluster_rects`, `dot_cluster_group_bounds`, `dot_slots`, `details_panel_rects` + `layout_tests` | ~880 |
| `roster_manager/chrome.rs` | `arrow_rects`/`arrow_dot_rects`, `home_dot_rect`/`home_rect` + `arrow_button_tests`, `home_button_tests` | ~570 |
| `roster_manager/sprite_name.rs` | `render_sprite`, `name_display`, `render_name`, `render_level` + `sprite_and_name_render_tests`, `level_render_tests` | ~410 |
| `roster_manager/stat_bar.rs` | `stat_fill_dots`, `stat_slice_parts`, `stat_label`, `render_stat_bars` + `stat_bar_tests` | ~700 |
| `roster_manager/borders.rs` | `draw_dot_box`, `draw_dot_cap_box`, `draw_dot_border` (shared primitive used by stat bars AND the details panel) + `draw_dot_border_tests`, `details_panel_border_tests` | ~410 |
| `roster_manager/details_panel.rs` | `exhaustion_text`, `render_exhaustion`, `ability_lines`, `render_ability_list` + `exhaustion_render_tests`, `ability_list_render_tests` | ~320 |
| `roster_manager/dot_row.rs` | `draw_dot_slot`, `render_dot_row` + `dot_row_render_tests`, `dot_row_cluster_tests` | ~370 |
| `roster_manager/regression_tests.rs` (test-only) | `golden_fixture_tests`, `nudge_constant_removal_tests`, `sub_cell_precision_tests` — whole-scene acceptance checks spanning multiple topics | ~170 |

Dependency direction: `mod.rs` → `{chrome, sprite_name, stat_bar, dot_row, details_panel, borders}` → `layout.rs` (acyclic; `layout.rs`/`borders.rs` are leaves).

**Complications:**
- Heavy `pub(super)` promotion needed — most of this file's functions are called from `render()`/`handle_input()` in `mod.rs`, so nearly everything moving out needs it: `cell_rect_to_dots`, `layout`, `top_bands_dots`, `left_col_dots`, `right_col_dots`, `dot_bands`, `dot_cluster_rects`, `dot_cluster_group_bounds`, `dot_slots`, `details_panel_rects`, `arrow_dot_rects`, `home_dot_rect`, `render_sprite`, `render_name`, `render_level`, `render_stat_bars`, `render_dot_row`, `render_exhaustion`, `render_ability_list`, `draw_dot_border`, `draw_dot_cap_box`.
- A few constants are genuinely cross-cutting (`BORDER_COLOR`, `DOT_LABEL_COLOR`, `STAT_BAR_OUTLINE_H`/`STAT_LABEL_H`) — keep these in `mod.rs` rather than chasing `pub(super)` across every consumer.
- **`nudge_constant_removal_tests` does `include_str!("roster_manager.rs")`** to grep the file's own source for banned constant names — this silently stops covering anything once the file is split into pieces. Must be rewritten to `include_str!` every resulting submodule file.
- `sub_cell_precision_tests`/`golden_fixture_tests` cut across 3+ topics by design (whole-scene acceptance, not unit tests of one topic) — parked in `regression_tests.rs` as a deliberate exception.

## 5. `battle_viewer.rs` (5150 → 9-file module tree)

| File | Contents | Depends on | Est. lines |
|---|---|---|---|
| `battle_viewer/piece.rs` | `Team`, `Piece`, `pieces()`, `world_pos_for_cell`, row/col layout consts, `TEAM_A/B_COLOR` + `piece_layout_tests` | *(nothing here)* | ~270 |
| `battle_viewer/camera.rs` | `BattleCamera` + `Camera` impl + presets, `OVER_SHOULDER_*`/`SIDELINE_CAMERA_DEPTH`, `BOARD_CENTER_COL` + `battle_camera_tests`, `camera_migration_tests`, `handle_input_camera_tests` | *(nothing here)* | ~760 |
| `battle_viewer/playback.rs` | `Event`, `EventKind`, `demo_events()`, `drive_events` (as `impl BattleViewer`, `pub(super)`) + 5 event-driving test mods | `piece.rs` | ~820 |
| `battle_viewer/geometry.rs` | `BoardGeometry`, `FramedCamera`, `board_geometry()`, `fit_perspective_geometry`, `board_world_corners`, `dot_bbox`, `BattleViewerTuning` + `board_geometry_tests` | `camera.rs` | ~570 |
| `battle_viewer/grid.rs` | `draw_board_lines`, `rasterize_grid_line`, `plot_dot_segment`, `GRID_LINE_COLOR` + `draw_board_lines_tests` | `geometry.rs`, `camera.rs` | ~460 |
| `battle_viewer/sizing.rs` | `piece_elapsed`, `sprite_base_dot_rows(_width_fill)`, `depth_scale_factor`, `depth_scaled_transform`, `build_draws` (as `impl BattleViewer`, `pub(super)`) + 6 sizing/anchor test mods | `camera.rs`, `geometry.rs`, `piece.rs` | ~920 (tightest) |
| `battle_viewer/shadow.rs` | `SHADOW_WIDTH_RATIO`, `shadow_alpha`, `shadow_buffers` (as `impl BattleViewer`, `pub(super)`) + `contact_shadow_tests` | `piece.rs`, `geometry.rs`, `camera.rs` | ~320 |
| `battle_viewer/mod.rs` | `BattleViewer` struct + fields + `Default`, `default_camera_mode`, `piece_sprite`, `drawable_pieces`, `nudge_free_roam`, `impl Scene`, `BOARD_COLS`/`BOARD_ROWS`, `mod` declarations | everything above | ~380 |
| `battle_viewer/scene_integration_tests.rs` (test-only) | `battle_viewer_scene_wiring_tests`, `inspectable_tests`, `golden_fixture_tests` | reads everything via `super::*` | ~800 |

**Execution order within this file** (leaf-first): `piece.rs` and `camera.rs` (independent, either order) → `playback.rs` → `geometry.rs` → `grid.rs` → `sizing.rs` → `shadow.rs` → `scene_integration_tests.rs` → `mod.rs` cleanup.

**Complications:**
- `pub(super)` needed on: `BattleCamera::with_scale_dots`, `BattleViewer::shadow_buffers`, `BattleViewer::build_draws`, `BattleViewer::drive_events`.
- `BattleViewer`'s private fields (`elapsed`, `pieces`, `events`, `camera_mode`, `settled_events`, etc.) must stay defined in `mod.rs` — nearly every test module across every proposed file constructs `BattleViewer { field: ..., ..Default::default() }` literals touching them directly, which only works via privacy cascading to descendants.
- `sizing.rs` lands at ~920, the tightest — if it grows, `bench_visibility_tests` is the easiest thing to peel into `shadow.rs`, but that requires promoting the shared `is_chromatic` test helper out of same-file-sibling access into a proper shared test helper.

## Open question
Commit granularity for whichever file(s) get tackled: one commit per extracted module (more, smaller, reviewable diffs) vs. one commit per whole-file split (bigger, but the file only exists in a "half-migrated" state within a single commit). Not decided — ask before starting whichever file comes up first.
