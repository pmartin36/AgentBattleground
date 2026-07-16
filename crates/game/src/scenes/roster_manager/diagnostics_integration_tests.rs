//! b4-t1: lint-on-reload wiring + the `[!!]` header badge + its hover card,
//! integrated end-to-end through `RosterManager::render`/`handle_input`/
//! `update` (spec 60). b4-t2 (bottom of this file): the SAME wiring inside
//! the prompt-editor popup — inline `DIAGNOSTIC_BG` span tint, the editor's
//! own `[!!]` badge on the file-path row, and its hover card. This file
//! tests WIRING only — badge drawing itself (`draw_badge`), the warning
//! card's content/pluralization/layout, and span mapping are
//! `diagnostics_ui_tests.rs`'s job (b3-t2/b3-t3/b3-t4), not re-verified
//! here.
//!
//! RED until the code-writer: (1) calls `diagnostics::lint` inside
//! `reload_instructions` and bumps `lint_runs`; (2) draws the badge via
//! `diagnostics_ui::header_badge_slot` + `draw_badge` in `render`, recording
//! `badge_hit_rect`; (3) hit-tests `badge_hit_rect` on `Mouse(Moved)` into
//! `hovered_badge`; (4) renders `render_warning_card` in the overlay pass
//! while `hovered_badge` (research.md's blueprint). All fixtures run at the
//! canonical 80x30 (research.md b4-t1 FINDING 1: the badge slot is 0-margin
//! at 80 cols, and the card's exact columns are NEVER hardcoded — always
//! derived via `diagnostics_ui::header_badge_slot`/`layout_warning_card`).
//! Tests use a **temp base dir**, never the real repo (spec 51's
//! convention), mirroring `instructions_cache_tests.rs`.

use super::*;
use crate::scenes::test_util::{key_event, lit_dot_color, mouse_event, rect_text, render_to_buffer};
use engine_core::scene::EngineCtx;
use engine_render::SELECTION_BG;
use ratatui::crossterm::event::{KeyCode, MouseButton, MouseEventKind};
use ratatui::style::Color;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Unique per-test temp base dir, mirroring `instructions_cache_tests.rs`'s
/// own helper.
fn temp_base_dir(tag: &str) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "game-roster-diagnostics-integration-test-{}-{}-{}",
        std::process::id(),
        tag,
        n
    ))
}

/// `demo_roster()`'s first entry (`instructions_cache_tests::CREATURE_0`) —
/// abilities "Ember Fang"/"Howl".
const CREATURE_0: &str = "Ember Wolf";
/// `demo_roster()`'s second entry — owns "Frost Bite", not an ability of
/// `CREATURE_0`.
const CREATURE_1: &str = "Frost Lizard";

/// research.md's verified 0-diagnostic fixture.
const CLEAN_TEXT: &str = "Use `@Ember_Fang` on `@enemy:frozen`.";
/// research.md's verified 1-diagnostic fixture: `Volt_Scorpion` is bundled
/// by `creatures::all()` but off the 6-slot `demo_roster()` ->
/// `CreatureNotOnRoster`.
const ONE_DIAG_TEXT: &str = "Send in @Volt_Scorpion now.";
/// research.md's verified 2-diagnostic fixture: adds `@Frost_Bite` (owned by
/// `CREATURE_1`, not `CREATURE_0`) -> `NotAnAbility`.
const TWO_DIAG_TEXT: &str = "Send in @Volt_Scorpion and use @Frost_Bite.";

const W: u16 = 80;
const H: u16 = 30;

/// Independently recomputes the diagnostics `text` must lint to for the
/// CURRENT creature, WITHOUT reading `scene.diagnostics` — so a test
/// asserting against this can't be trivially satisfied by an unwired,
/// always-empty cache.
fn expected_diagnostics(scene: &RosterManager, text: &str) -> Vec<crate::diagnostics::Diagnostic> {
    let vocab =
        crate::mention::Vocabulary::new(&scene.creatures[scene.current_index], &scene.creatures);
    crate::diagnostics::lint(text, &vocab)
}

fn regions() -> panel_layout::PanelRegions {
    RosterManager::panel_interior_regions(Rect::new(0, 0, W, H))
}

/// `rect_text` with `Dot::Occlude`'s U+2800 blank-fill collapsed to spaces
/// and runs of whitespace normalized, mirroring `diagnostics_ui_tests.rs`'s
/// own `row_text` — a raw `rect_text(..).contains(msg)` fails on the fill
/// padding at a message's wrap point, not on content (validator.md
/// iteration 1 finding).
fn row_text(buf: &Buffer, rect: Rect) -> String {
    rect_text(buf, rect).replace('\u{2800}', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A flagged instructions file renders the `[!!]` badge in `WARNING_COLOR`
/// at the derived `header_badge_slot` (spec:127, research.md FINDING 1 —
/// never a hardcoded rect).
#[test]
fn flagged_instructions_render_badge_in_header_gap() {
    let base = temp_base_dir("badge-gap");
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let scene = RosterManager::new_with_instructions_base(base.clone());

    let buf = render_to_buffer(&scene, W, H);
    let slot = diagnostics_ui::header_badge_slot(regions().instructions_header, "Instructions")
        .expect("the badge slot must exist at 80 columns (research.md FINDING 1)");

    assert!(
        rect_text(&buf, slot).contains(diagnostics_ui::BADGE_TEXT),
        "expected {:?} in the header badge slot, got {:?}",
        diagnostics_ui::BADGE_TEXT,
        rect_text(&buf, slot)
    );
    let fg = crate::scenes::test_util::sample_fg(&buf, slot);
    let expected = Color::Rgb(
        diagnostics_ui::WARNING_COLOR.r,
        diagnostics_ui::WARNING_COLOR.g,
        diagnostics_ui::WARNING_COLOR.b,
    );
    assert_eq!(fg, Some(expected), "badge must render in WARNING_COLOR");

    let _ = std::fs::remove_dir_all(&base);
}

/// A clean instructions file writes no cell into the badge slot (spec:217 —
/// the cell is genuinely blank, not merely "no tooltip").
#[test]
fn clean_instructions_write_no_badge_cell() {
    let base = temp_base_dir("badge-absent");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let scene = RosterManager::new_with_instructions_base(base.clone());

    let buf = render_to_buffer(&scene, W, H);
    let slot = diagnostics_ui::header_badge_slot(regions().instructions_header, "Instructions")
        .expect("the badge slot must exist at 80 columns (research.md FINDING 1)");

    assert!(
        !crate::scenes::test_util::has_non_space(&buf, slot),
        "a clean instructions file must write no cell in the badge slot"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Hovering the drawn badge shows the warning card with the pluralized
/// count header and every diagnostic's message.
#[test]
fn hovering_badge_shows_card_with_every_diagnostic() {
    let base = temp_base_dir("hover-card");
    crate::instructions::write_instructions_in(&base, CREATURE_0, TWO_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());

    let _ = render_to_buffer(&scene, W, H);
    let badge = scene
        .badge_hit_rect
        .borrow()
        .expect("badge_hit_rect must be Some after rendering a flagged file");

    let (cx, cy) = (badge.x + badge.width / 2, badge.y + badge.height / 2);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    assert!(scene.hovered_badge, "a Moved event inside the badge rect must set hovered_badge");

    let buf = render_to_buffer(&scene, W, H);

    let diags = expected_diagnostics(&scene, TWO_DIAG_TEXT);
    assert_eq!(diags.len(), 2, "setup: TWO_DIAG_TEXT must lint to exactly 2 diagnostics");
    let anchor = RosterManager::cell_rect_to_dots(badge);
    let layout = diagnostics_ui::layout_warning_card(anchor, &diags);

    let header_text = row_text(&buf, layout.rows[0].to_cell_rect());
    assert!(header_text.contains("2 issues"), "card header must read \"2 issues\", got {:?}", header_text);
    for (diag, row) in diags.iter().zip(&layout.rows[1..]) {
        let text = row_text(&buf, row.to_cell_rect());
        assert!(
            text.contains(&diag.message),
            "expected diagnostic message {:?} in its row, got {:?}",
            diag.message,
            text
        );
    }

    let _ = std::fs::remove_dir_all(&base);
}

/// Moving the cursor off the badge clears `hovered_badge` and the card
/// stops rendering.
#[test]
fn moving_off_badge_hides_card() {
    let base = temp_base_dir("hover-off");
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());

    let _ = render_to_buffer(&scene, W, H);
    let badge = scene
        .badge_hit_rect
        .borrow()
        .expect("badge_hit_rect must be Some after rendering a flagged file");
    let (cx, cy) = (badge.x + badge.width / 2, badge.y + badge.height / 2);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    assert!(scene.hovered_badge, "setup: hovering the badge must set hovered_badge");

    scene.handle_input(mouse_event(MouseEventKind::Moved, 0, 0));

    assert!(!scene.hovered_badge, "moving off the badge must clear hovered_badge");

    let buf = render_to_buffer(&scene, W, H);
    let diags = expected_diagnostics(&scene, ONE_DIAG_TEXT);
    let anchor = RosterManager::cell_rect_to_dots(badge);
    let layout = diagnostics_ui::layout_warning_card(anchor, &diags);
    // The card's full footprint legitimately overlaps unconditional details-panel
    // content (spec:180 — "covering the creature art is accepted"), so a blank-cell
    // check over the whole footprint is unsatisfiable even when the card is
    // correctly suppressed (validator.md iteration 1 finding). Narrow to a
    // diagnostics-only signal: the count-header row's text, which nothing else on
    // this screen produces.
    let count_text = diagnostics_ui::count_header(diags.len());
    let header_text = row_text(&buf, layout.rows[0].to_cell_rect());
    assert!(
        !header_text.contains(&count_text),
        "the warning card's count header {:?} must not render once hovered_badge is cleared, got {:?}",
        count_text,
        header_text
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Lint runs exactly once at construction and never again from repeated
/// renders (spec:223's explicit call-count assertion).
#[test]
fn lint_does_not_run_per_render() {
    let base = temp_base_dir("lint-once");
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let scene = RosterManager::new_with_instructions_base(base.clone());

    assert_eq!(scene.lint_runs, 1, "construction must lint exactly once");

    for _ in 0..5 {
        let _ = render_to_buffer(&scene, W, H);
    }

    assert_eq!(scene.lint_runs, 1, "render must never invoke lint, however many times it runs");

    let _ = std::fs::remove_dir_all(&base);
}

/// Closing the prompt-editor popup re-lints the (possibly externally
/// edited) reloaded file exactly once.
#[test]
fn lint_reruns_when_popup_closes() {
    let base = temp_base_dir("lint-popup-close");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    let before = scene.lint_runs;

    let idx = scene.current_index;
    let name = scene.creatures[idx].name().to_string();
    scene.prompt_editor =
        Some(prompt_editor::PromptEditor::new(idx, &name, Some(&base), &scene.creatures));

    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("edit write should succeed");
    scene.handle_input(key_event(KeyCode::Esc));

    assert_eq!(scene.lint_runs, before + 1, "closing the popup must re-lint exactly once");
    let diags = expected_diagnostics(&scene, ONE_DIAG_TEXT);
    assert_eq!(scene.diagnostics, diags, "diagnostics must reflect the reloaded file's content");

    let _ = std::fs::remove_dir_all(&base);
}

/// Settling a navigation slide re-lints the INCOMING creature's file
/// exactly once.
#[test]
fn lint_reruns_on_slide_settle() {
    let base = temp_base_dir("lint-slide-settle");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    crate::instructions::write_instructions_in(&base, CREATURE_1, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    let before = scene.lint_runs;
    let mut ctx = EngineCtx;

    scene.navigate(Direction::Right);
    scene.update(&mut ctx, RosterManager::SLIDE_DUR + Duration::from_millis(1));

    assert_eq!(scene.lint_runs, before + 1, "a settled slide must re-lint exactly once");
    let diags = expected_diagnostics(&scene, ONE_DIAG_TEXT);
    assert_eq!(
        scene.diagnostics, diags,
        "diagnostics must reflect the INCOMING creature's file, not the outgoing one"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// The badge is present (and hit-testable) while the details panel is
/// resting, but disappears the instant a slide is in flight — the details
/// panel itself is suppressed during a slide (mirrors
/// `ability_hover_tests.rs`'s suppression contract for `ability_hit_rects`).
#[test]
fn badge_absent_while_slide_in_flight() {
    let base = temp_base_dir("badge-slide");
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    crate::instructions::write_instructions_in(&base, CREATURE_1, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());

    let _ = render_to_buffer(&scene, W, H);
    assert!(
        scene.badge_hit_rect.borrow().is_some(),
        "setup: badge_hit_rect must be Some while at rest with a flagged file"
    );

    scene.navigate(Direction::Right);
    let _ = render_to_buffer(&scene, W, H);

    assert!(
        scene.badge_hit_rect.borrow().is_none(),
        "badge_hit_rect must be None while a slide is in flight"
    );
    let slot = diagnostics_ui::header_badge_slot(regions().instructions_header, "Instructions")
        .expect("the badge slot must exist at 80 columns (research.md FINDING 1)");
    let buf = render_to_buffer(&scene, W, H);
    assert!(
        !crate::scenes::test_util::has_non_space(&buf, slot),
        "no badge cell may be written while a slide is in flight"
    );

    let _ = std::fs::remove_dir_all(&base);
}

// ── b4-t1 iteration 2 delta: relint on a completed select-and-swap ─────────
// research.md FINDING 2 (routed back by scope/bucket-b4.md): `toggle_selection`'s
// swap arm changes WHICH creature `current_index` points at without calling
// `reload_instructions`, so `current_instructions`/`diagnostics`/the header
// badge keep describing the outgoing creature. RED until the code-writer adds
// `self.reload_instructions();` to the swap arm ONLY (`mod.rs:308-317`) — select
// and deselect must NOT relint (spec:201's per-render economy).

/// The FINDING-2 regression: completing a swap (Space with a different
/// creature already selected) re-lints exactly once and `diagnostics`/
/// `current_instructions` describe the SWAPPED-IN creature, not the outgoing
/// one.
#[test]
fn swap_reloads_diagnostics_for_swapped_in_creature() {
    let base = temp_base_dir("swap-relint");
    // CLEAN_TEXT mentions `@Ember_Fang`, one of CREATURE_0's OWN abilities —
    // linting it against CREATURE_1's vocabulary would itself flag
    // `NotAnAbility`, so CREATURE_1's baseline uses mention-free text (no `@`
    // tokens at all -> trivially clean under any vocabulary, `diagnostics.rs:147`).
    const NO_MENTION_TEXT: &str = "Attack the nearest enemy.";
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    crate::instructions::write_instructions_in(&base, CREATURE_1, NO_MENTION_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());

    // Baseline: view CREATURE_1 (Frost Lizard), which is clean.
    scene.current_index = 1;
    scene.reload_instructions();
    assert!(scene.diagnostics.is_empty(), "setup: CREATURE_1's file must be clean");
    let before = scene.lint_runs;

    // Select CREATURE_0 (Ember Wolf, flagged, sitting at slot 0) while the
    // cursor stays at CREATURE_1's slot (1, the baseline view), then complete
    // the swap — `current_index` is untouched by the swap, so afterward slot 1
    // holds the swapped-in CREATURE_0 and `current_index == 1` displays it.
    scene.selected_index = Some(0);
    scene.handle_input(key_event(KeyCode::Char(' ')));

    assert_eq!(scene.selected_index, None, "a completed swap must clear the selection");
    assert_eq!(
        scene.lint_runs,
        before + 1,
        "a completed swap must re-lint exactly once for the swapped-in creature"
    );
    assert_eq!(
        scene.current_instructions, ONE_DIAG_TEXT,
        "current_instructions must reload to the swapped-in creature's file"
    );
    let diags = expected_diagnostics(&scene, ONE_DIAG_TEXT);
    assert!(!diags.is_empty(), "setup: the swapped-in creature's file must be flagged");
    assert_eq!(
        scene.diagnostics, diags,
        "diagnostics must describe the swapped-in creature, not the creature that used to be here"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// The economy guard: a plain select or deselect changes neither the
/// displayed creature nor the roster order, so neither may relint.
#[test]
fn select_and_deselect_do_not_relint() {
    let base = temp_base_dir("select-deselect-no-relint");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    let before = scene.lint_runs;

    scene.handle_input(key_event(KeyCode::Char(' '))); // select the current creature
    assert_eq!(scene.selected_index, Some(0), "setup: select must set selected_index");
    assert_eq!(scene.lint_runs, before, "selecting must not relint");

    scene.handle_input(key_event(KeyCode::Char(' '))); // deselect (same index)
    assert_eq!(scene.selected_index, None, "setup: deselect must clear selected_index");
    assert_eq!(scene.lint_runs, before, "deselecting must not relint");

    let _ = std::fs::remove_dir_all(&base);
}

// ── b4-t2: prompt-editor badge, inline tint, and hover card ────────────────
// RED until the code-writer wires: (1) `DiagnosticsState::relint` into
// `PromptEditor::new` and `flush_pending`; (2) `draw_badge_in`/`render_card`
// into `PromptEditor::render`, split via `diagnostics_ui::split_path_row`;
// (3) `set_hover` into `handle_input`'s `Moved` branch (research.md b4-t2's
// blueprint). Badge drawing, card layout, and span mapping themselves are
// NOT re-verified here (owned by `diagnostics_ui_tests.rs` / `render.rs`'s
// own module) — this file tests wiring only, per bucket b4's convention.

/// Opens the popup for the CURRENT creature, mirroring
/// `prompt_editor_modal_tests.rs:40`'s `open_popup` (private-field
/// construction; this file predates that helper's extraction, see
/// `lint_reruns_when_popup_closes` above for the same inline shape).
fn open_editor_popup(scene: &mut RosterManager, base: &std::path::Path) {
    let idx = scene.current_index;
    let name = scene.creatures[idx].name().to_string();
    scene.prompt_editor =
        Some(prompt_editor::PromptEditor::new(idx, &name, Some(base), &scene.creatures));
}

/// Every buffer cell whose `bg == bg`, concatenated in reading (row-major)
/// order — decodes the REAL rendered buffer, not the decoration list.
fn cells_tinted(buf: &Buffer, bg: Color) -> String {
    let area = buf.area;
    let mut s = String::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = buf.cell((x, y)).unwrap();
            if cell.bg == bg {
                s.push_str(cell.symbol());
            }
        }
    }
    s
}

/// The deliverable's named correctness check: `DIAGNOSTIC_BG` must differ
/// from the imported `SELECTION_BG`, never a re-declared literal.
#[test]
fn diagnostic_bg_differs_from_selection_bg() {
    assert_ne!(
        diagnostics_ui::DIAGNOSTIC_BG,
        SELECTION_BG,
        "DIAGNOSTIC_BG must be distinguishable from the editor's own selection tint"
    );
}

/// Headline case: editing in a bad `@mention` tints exactly that span's
/// cells with `DIAGNOSTIC_BG`, once the debounced re-lint has run.
#[test]
fn editing_bad_mention_tints_exactly_its_span() {
    let base = temp_base_dir("editor-tint-span");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    for c in " @Volt_Scorpion.".chars() {
        scene.handle_input(key_event(KeyCode::Char(c)));
    }
    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(400));

    let buf = render_to_buffer(&scene, W, H);
    let tinted = cells_tinted(&buf, diagnostics_ui::DIAGNOSTIC_BG);
    assert_eq!(
        tinted, "@Volt_Scorpion",
        "exactly the bad mention token's cells must be tinted DIAGNOSTIC_BG, got {tinted:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// The `[!!]` badge renders on the file-path row's reserved slot
/// (`split_path_row`), never a hardcoded rect.
#[test]
fn badge_renders_on_file_path_row() {
    let base = temp_base_dir("editor-badge-row");
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    let buf = render_to_buffer(&scene, W, H);
    let layout = prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, W, H), 1);
    let row = diagnostics_ui::split_path_row(layout.file_path);
    let slot = row.badge.expect("the badge slot must exist at 80 columns");

    assert!(
        rect_text(&buf, slot).contains(diagnostics_ui::BADGE_TEXT),
        "expected {:?} in the editor's badge slot, got {:?}",
        diagnostics_ui::BADGE_TEXT,
        rect_text(&buf, slot)
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// A clean instructions file writes no cell into the editor's badge slot.
#[test]
fn clean_file_writes_no_badge_cell() {
    let base = temp_base_dir("editor-badge-absent");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    let buf = render_to_buffer(&scene, W, H);
    let layout = prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, W, H), 1);
    let row = diagnostics_ui::split_path_row(layout.file_path);
    let slot = row.badge.expect("the badge slot must exist at 80 columns");

    assert!(
        !crate::scenes::test_util::has_non_space(&buf, slot),
        "a clean instructions file must write no cell in the editor's badge slot"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// FINDING 2's regression guard: the badge slot's reservation must be
/// UNCONDITIONAL, so `fit_path_text`'s available width — and therefore the
/// rendered path text itself — never changes when a diagnostic
/// appears/clears. Uses a SINGLE scene/file path throughout (the file path
/// itself never changes as the user types into the instructions body) and
/// decodes the REAL buffer before and after, so the only variable between
/// the two captures is whether a diagnostic exists. (Iteration 1 compared
/// `split_path_row(x) == split_path_row(x)` on the same input — a
/// tautology; see validator.md.)
#[test]
fn path_text_rect_does_not_move_when_a_diagnostic_appears() {
    let base = temp_base_dir("editor-path-stable");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    let layout = prompt_editor::PromptEditor::compute_layout(Rect::new(0, 0, W, H), 1);
    let row = diagnostics_ui::split_path_row(layout.file_path);
    let badge_slot = row.badge.expect("the badge slot must exist at 80 columns");

    let clean_buf = render_to_buffer(&scene, W, H);
    assert!(
        !crate::scenes::test_util::has_non_space(&clean_buf, badge_slot),
        "setup: the clean file must write no cell in the badge slot"
    );
    let clean_path_text = rect_text(&clean_buf, row.path);

    for c in " @Volt_Scorpion.".chars() {
        scene.handle_input(key_event(KeyCode::Char(c)));
    }
    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(400));

    let flagged_buf = render_to_buffer(&scene, W, H);
    assert!(
        crate::scenes::test_util::has_non_space(&flagged_buf, badge_slot),
        "setup: the now-flagged file must write the badge into its slot"
    );
    let flagged_path_text = rect_text(&flagged_buf, row.path);

    assert_eq!(
        clean_path_text, flagged_path_text,
        "the file-path text rendered into the reserved `path` rect must be identical before \
         and after a diagnostic appears — the badge slot's width must not grow/shrink and \
         shift the path text: clean={clean_path_text:?} flagged={flagged_path_text:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Hovering the editor's drawn badge shows the clamped warning card with the
/// pluralized count and every diagnostic's message.
#[test]
fn hovering_editor_badge_shows_clamped_card() {
    let base = temp_base_dir("editor-hover-card");
    crate::instructions::write_instructions_in(&base, CREATURE_0, TWO_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    let _ = render_to_buffer(&scene, W, H);
    let badge = scene
        .prompt_editor
        .as_ref()
        .unwrap()
        .diagnostics
        .badge_hit_rect
        .get()
        .expect("editor badge_hit_rect must be Some after rendering a flagged file");

    let (cx, cy) = (badge.x + badge.width / 2, badge.y + badge.height / 2);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    assert!(
        scene.prompt_editor.as_ref().unwrap().diagnostics.hovered_badge,
        "a Moved event inside the editor badge rect must set hovered_badge"
    );

    let buf = render_to_buffer(&scene, W, H);
    let diags = expected_diagnostics(&scene, TWO_DIAG_TEXT);
    assert_eq!(diags.len(), 2, "setup: TWO_DIAG_TEXT must lint to exactly 2 diagnostics");
    let anchor = RosterManager::cell_rect_to_dots(badge);
    let layout = diagnostics_ui::layout_warning_card(anchor, &diags);

    let header_text = row_text(&buf, layout.rows[0].to_cell_rect());
    assert!(header_text.contains("2 issues"), "card header must read \"2 issues\", got {header_text:?}");
    for (diag, row) in diags.iter().zip(&layout.rows[1..]) {
        let text = row_text(&buf, row.to_cell_rect());
        assert!(
            text.contains(&diag.message),
            "expected diagnostic message {:?} in its row, got {:?}",
            diag.message,
            text
        );
    }

    assert_eq!(layout.card.x, 0, "at this anchor the card must be left-clamped (unclamped x = -50)");
    let mid_dot_row = layout.card.y + layout.card.h / 2;
    assert_eq!(
        lit_dot_color(&buf, 0, mid_dot_row).map(|c| (c.r, c.g, c.b)),
        Some((super::tooltip::CARD_BORDER_COLOR.r, super::tooltip::CARD_BORDER_COLOR.g, super::tooltip::CARD_BORDER_COLOR.b)),
        "the card's left border must decode as lit at dot col 0 — clamped, not merely comparing rect fields"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Moving off the editor's badge clears `hovered_badge`.
#[test]
fn moving_off_editor_badge_hides_card() {
    let base = temp_base_dir("editor-hover-off");
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    let _ = render_to_buffer(&scene, W, H);
    let badge = scene
        .prompt_editor
        .as_ref()
        .unwrap()
        .diagnostics
        .badge_hit_rect
        .get()
        .expect("editor badge_hit_rect must be Some after rendering a flagged file");
    let (cx, cy) = (badge.x + badge.width / 2, badge.y + badge.height / 2);
    scene.handle_input(mouse_event(MouseEventKind::Moved, cx, cy));
    assert!(
        scene.prompt_editor.as_ref().unwrap().diagnostics.hovered_badge,
        "setup: hovering the editor badge must set hovered_badge"
    );

    scene.handle_input(mouse_event(MouseEventKind::Moved, 0, 0));

    assert!(
        !scene.prompt_editor.as_ref().unwrap().diagnostics.hovered_badge,
        "moving off the editor badge must clear hovered_badge"
    );

    let _ = std::fs::remove_dir_all(&base);
}

// ── b4-t2 iteration 2 delta: the two `@`-mention accept paths ──────────────
// research.md (iteration 2): `mark_dirty` never restarts on either mention
// accept path (Tab discards `handle_key`'s return; click's `Changed` is
// destroyed inside `handle_mouse`'s `bool`), so the tint/write/relint chain
// never runs on an accept — falsifying this task's headline "tints exactly
// those spans" deliverable. RED until the code-writer replaces `mark_dirty`
// with `poll_instructions` (polling `TextEditor::revision()`).

/// Scans the WHOLE scene buffer for the first row containing `needle` as a
/// contiguous run of single-char-symbol cells — mirrors
/// `engine_render`'s own `text_editor::mention_tests::find_candidate_row`,
/// generalized because the popup's on-screen position isn't fixed here.
fn find_text_row(buf: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let area = buf.area;
    let needle_chars: Vec<char> = needle.chars().collect();
    for y in area.top()..area.bottom() {
        let row_chars: Vec<char> = (area.left()..area.right())
            .map(|x| buf.cell((x, y)).unwrap().symbol().chars().next().unwrap_or(' '))
            .collect();
        if let Some(idx) = row_chars
            .windows(needle_chars.len().max(1))
            .position(|w| w == needle_chars.as_slice())
        {
            return Some((area.left() + idx as u16, y));
        }
    }
    None
}

/// Headline regression: Tab-accepting a mention candidate must tint exactly
/// its (post-accept) span. Derives the Reserve-slot creature rather than
/// hardcoding an index (spec:210) — its mention resolves to the real
/// `CreatureNotFielded` diagnostic (`mention.rs`). The query chars are typed
/// and FLUSHED (via `update(400ms)`) before Tab, so `dirty` is genuinely
/// `false` at the moment of accept — otherwise a leftover `dirty` from
/// typing masks the bug this test targets (Tab's own `Changed` is discarded,
/// `prompt_editor.rs`'s Tab branch).
#[test]
fn tab_accepting_a_mention_tints_the_accepted_span() {
    let base = temp_base_dir("editor-tab-accept-tint");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    let reserve = scene.creatures[crate::squad_role::ACTIVE_SLOTS + crate::squad_role::BENCH_SLOTS]
        .name()
        .to_string();
    let prefix: String = reserve.chars().take(4).collect(); // "Verd" of "Verdant Treant"
    for c in format!(" @{prefix}").chars() {
        scene.handle_input(key_event(KeyCode::Char(c)));
    }
    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(400)); // settle: dirty -> false before the accept

    scene.handle_input(key_event(KeyCode::Tab));
    scene.update(&mut ctx, Duration::from_millis(400));

    let buf = render_to_buffer(&scene, W, H);
    let token = format!("@{}", crate::instructions::underscore_name(&reserve));
    let tinted = cells_tinted(&buf, diagnostics_ui::DIAGNOSTIC_BG);
    assert_eq!(
        tinted, token,
        "Tab-accepting the mention must tint exactly its span, got {tinted:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// The path no game-side `EditorEvent` can observe at all: `handle_mouse`
/// returns `bool`, so a click-accepted mention's `Changed` is destroyed
/// inside the engine before `PromptEditor` ever sees it. Settles `dirty` to
/// `false` (via `update(400ms)`) before the click for the same reason as the
/// Tab test above.
#[test]
fn click_accepting_a_mention_tints_the_accepted_span() {
    let base = temp_base_dir("editor-click-accept-tint");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    let reserve = scene.creatures[crate::squad_role::ACTIVE_SLOTS + crate::squad_role::BENCH_SLOTS]
        .name()
        .to_string();
    let prefix: String = reserve.chars().take(4).collect();
    for c in format!(" @{prefix}").chars() {
        scene.handle_input(key_event(KeyCode::Char(c)));
    }
    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(400)); // settle: dirty -> false before the click

    let buf = render_to_buffer(&scene, W, H);
    let (cx, cy) = find_text_row(&buf, &reserve)
        .unwrap_or_else(|| panic!("candidate row for {reserve:?} must be drawn"));
    scene.handle_input(mouse_event(MouseEventKind::Down(MouseButton::Left), cx, cy));
    scene.update(&mut ctx, Duration::from_millis(400));

    let buf2 = render_to_buffer(&scene, W, H);
    let token = format!("@{}", crate::instructions::underscore_name(&reserve));
    let tinted = cells_tinted(&buf2, diagnostics_ui::DIAGNOSTIC_BG);
    assert_eq!(
        tinted, token,
        "click-accepting the mention must tint exactly its span, got {tinted:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// The sharpest "wrong cells" case: accepting a mention BEFORE an existing
/// bad span shifts every column right of it. If decorations don't move with
/// the edit, the surviving tint paints a stale, shifted (wrong) run instead
/// of following `@Volt_Scorpion`. Settles `dirty` to `false` before Tab for
/// the same reason as the headline test above.
#[test]
fn tab_accepting_a_mention_moves_an_existing_tint() {
    let base = temp_base_dir("editor-tab-accept-shifts-tint");
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    let before_buf = render_to_buffer(&scene, W, H);
    assert_eq!(
        cells_tinted(&before_buf, diagnostics_ui::DIAGNOSTIC_BG),
        "@Volt_Scorpion",
        "setup: ONE_DIAG_TEXT's bad mention must already be tinted"
    );

    // Insert a clean CREATURE_0 ability mention at the buffer start, followed
    // by a space so the accepted mention cannot merge with "Send" into one
    // greedy alnum token (`diagnostics.rs:100 token_end` has no word-boundary
    // check other than the leading `@`) — every column at/after it still
    // shifts right, in isolation from that unrelated tokenizer behavior.
    scene.handle_input(key_event(KeyCode::Home));
    scene.handle_input(key_event(KeyCode::Char(' ')));
    scene.handle_input(key_event(KeyCode::Home));
    for c in "@How".chars() {
        scene.handle_input(key_event(KeyCode::Char(c)));
    }
    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(400)); // settle: dirty -> false before the accept

    scene.handle_input(key_event(KeyCode::Tab)); // accepts "@Howl", producing
    // "@Howl Send in @Volt_Scorpion now." — the trailing space keeps "@Howl"
    // and "Send" as separate tokens, isolating the +N shift under test.
    scene.update(&mut ctx, Duration::from_millis(400));

    let buf = render_to_buffer(&scene, W, H);
    let tinted = cells_tinted(&buf, diagnostics_ui::DIAGNOSTIC_BG);
    assert_eq!(
        tinted, "@Volt_Scorpion",
        "the tint must follow the shifted `@Volt_Scorpion` span, not paint a stale run: got {tinted:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// The write-loss half of the same root cause: a Tab-accepted mention must
/// still reach disk once the debounce elapses. Settles `dirty` to `false`
/// before Tab for the same reason as the headline test above.
#[test]
fn tab_accepted_mention_is_written_to_disk() {
    let base = temp_base_dir("editor-tab-accept-write");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    for c in " @How".chars() {
        scene.handle_input(key_event(KeyCode::Char(c)));
    }
    let mut ctx = EngineCtx;
    scene.update(&mut ctx, Duration::from_millis(400)); // settle: dirty -> false before the accept

    scene.handle_input(key_event(KeyCode::Tab)); // accepts "@Howl"
    scene.update(&mut ctx, Duration::from_millis(400));

    let on_disk = crate::instructions::read_instructions_in(&base, CREATURE_0)
        .expect("instructions file must be readable");
    assert!(
        on_disk.contains("@Howl"),
        "a Tab-accepted mention must reach disk after the debounce elapses, got {on_disk:?}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// spec:223: lint runs exactly once at popup construction and never again
/// from repeated renders.
#[test]
fn editor_lint_does_not_run_per_render() {
    let base = temp_base_dir("editor-lint-once");
    crate::instructions::write_instructions_in(&base, CREATURE_0, ONE_DIAG_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);

    assert_eq!(
        scene.prompt_editor.as_ref().unwrap().diagnostics.lint_runs,
        1,
        "opening the popup on an already-flagged file must lint exactly once"
    );

    for _ in 0..5 {
        let _ = render_to_buffer(&scene, W, H);
    }

    assert_eq!(
        scene.prompt_editor.as_ref().unwrap().diagnostics.lint_runs,
        1,
        "render must never invoke the editor's lint, however many times it runs"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Lint is debounced on `WRITE_DEBOUNCE`, not run per keystroke: a burst of
/// edits under the debounce contributes exactly one re-lint.
#[test]
fn editor_lint_is_debounced_not_per_keystroke() {
    let base = temp_base_dir("editor-lint-debounced");
    crate::instructions::write_instructions_in(&base, CREATURE_0, CLEAN_TEXT)
        .expect("seed write should succeed");
    let mut scene = RosterManager::new_with_instructions_base(base.clone());
    open_editor_popup(&mut scene, &base);
    let before = scene.prompt_editor.as_ref().unwrap().diagnostics.lint_runs;

    let mut ctx = EngineCtx;
    for c in " @Volt_Scorpion.".chars() {
        scene.handle_input(key_event(KeyCode::Char(c)));
        scene.update(&mut ctx, Duration::from_millis(50)); // < WRITE_DEBOUNCE (300ms)
    }
    assert_eq!(
        scene.prompt_editor.as_ref().unwrap().diagnostics.lint_runs,
        before,
        "a burst of edits under the debounce must not re-lint yet"
    );

    scene.update(&mut ctx, Duration::from_millis(400));
    assert_eq!(
        scene.prompt_editor.as_ref().unwrap().diagnostics.lint_runs,
        before + 1,
        "the whole debounced burst must contribute exactly one re-lint"
    );

    let _ = std::fs::remove_dir_all(&base);
}
