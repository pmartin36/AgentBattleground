use super::*;

/// b7-t1: pre-`flex()` migration golden-fixture gate. Freezes the CURRENT
/// (pre-migration) `RosterManager` render at 4 deterministic scenarios into
/// committed fixtures under `crates/game/tests/fixtures/roster/`, decoded
/// through the exact `decode_braille_cell`/`diff_dots` channel the b8
/// `flex()` migration is re-checked against (b8-t9). Text labels are NOT
/// covered here — `decode_braille_cell` only sees braille dot cells
/// (research.md SCOPE NOTE); text positioning stays covered by this file's
/// existing `rect_text`-based assertions elsewhere.
#[cfg(test)]
mod golden_fixture_tests {
    use super::*;
    use crate::scenes::test_util::{
        buffer_to_art, key_event, load_roster_fixture, render_to_buffer, serialize_braille_buffer,
    };
    use crossterm::event::KeyCode;
    use engine_core::scene::EngineCtx;
    use engine_render::diff_dots;

    /// (fixture name, render width, render height, scene-mutation fn).
    type Scenario = (&'static str, u16, u16, fn(&mut RosterManager));

    /// The 4 deterministic scenarios: fixture name, render dims, and the
    /// mutation applied to a fresh `RosterManager::new()` before render.
    fn scenarios() -> Vec<Scenario> {
        fn rest(_scene: &mut RosterManager) {}
        fn at_index_2(scene: &mut RosterManager) {
            scene.current_index = 2;
        }
        fn mid_slide(scene: &mut RosterManager) {
            scene.handle_input(key_event(KeyCode::Right));
            let mut ctx = EngineCtx;
            scene.update(&mut ctx, Duration::from_millis(75)); // ~25% of the 300ms SLIDE_DUR
        }

        vec![
            ("rest_40x20", 40, 20, rest as fn(&mut RosterManager)),
            ("rest_80x30", 80, 30, rest as fn(&mut RosterManager)),
            ("index2_80x30", 80, 30, at_index_2 as fn(&mut RosterManager)),
            ("midslide_80x30", 80, 30, mid_slide as fn(&mut RosterManager)),
        ]
    }

    /// For each of the 4 scenarios, the CURRENT render must dot-for-dot
    /// match its committed fixture (`diff_dots(&fixture, &actual).is_match()`).
    /// Pre-migration this is a freeze (green by construction once the
    /// fixtures are generated); b8-t9 re-runs this SAME assertion against
    /// the `flex()`-migrated code, so this test is the enforced acceptance
    /// oracle for the whole b8 bucket.
    ///
    /// Run with `UPDATE_ROSTER_FIXTURES=1` to (re)generate the 4
    /// `*.fixture` + `*.preview.txt` files from the current render — do the
    /// manual visual pass over the previews (recorded in
    /// `crates/game/tests/fixtures/roster/README.md`) BEFORE committing
    /// regenerated fixtures.
    #[test]
    fn roster_golden_fixtures_match_pre_migration_baseline() {
        let generate = std::env::var("UPDATE_ROSTER_FIXTURES").is_ok();

        for (name, w, h, build) in scenarios() {
            let mut scene = RosterManager::new();
            build(&mut scene);
            let actual = render_to_buffer(&scene, w, h);

            if generate {
                let fixture_path =
                    format!("{}/tests/fixtures/roster/{name}.fixture", env!("CARGO_MANIFEST_DIR"));
                let preview_path = format!(
                    "{}/tests/fixtures/roster/{name}.preview.txt",
                    env!("CARGO_MANIFEST_DIR")
                );
                std::fs::write(&fixture_path, serialize_braille_buffer(&actual))
                    .unwrap_or_else(|e| panic!("failed to write {fixture_path}: {e}"));
                std::fs::write(&preview_path, buffer_to_art(&actual))
                    .unwrap_or_else(|e| panic!("failed to write {preview_path}: {e}"));
                continue;
            }

            let fixture = load_roster_fixture(name);
            let diff = diff_dots(&fixture, &actual);
            assert!(
                diff.is_match(),
                "scenario {name:?}: current render diverges from the committed \
                 pre-migration fixture ({} dot mismatch(es) of {} compared); \
                 regenerate with UPDATE_ROSTER_FIXTURES=1 only if the divergence \
                 is intentional (re-run the manual visual pass first)",
                diff.mismatches.len(),
                diff.dots_compared
            );
        }
    }
}

/// b8-t9: enforces the 3 render-time nudge constants the `flex()` migration
/// deleted (b8-t3/t4/t5) never come back — dot-native positioning must
/// absorb such offsets into the computed `DotRect`, not a bolted-on
/// constant.
#[cfg(test)]
mod nudge_constant_removal_tests {
    /// Fragment-assembles each needle at runtime so this test's own source
    /// text doesn't contain the identifiers literally (else it would find
    /// its own mention and guard nothing).
    #[test]
    fn nudge_constants_absent_after_flex_migration() {
        let src = concat!(
            include_str!("mod.rs"),
            include_str!("borders.rs"),
            include_str!("chrome.rs"),
            include_str!("details_panel.rs"),
            include_str!("dot_row.rs"),
            include_str!("layout.rs"),
            include_str!("sprite_name.rs"),
            include_str!("stat_bar.rs"),
            include_str!("regression_tests.rs"),
        );
        for (a, b) in [
            ("ARROW_NUDGE_DOWN", "_DOTS"),
            ("HOME_NUDGE_UP", "_DOTS"),
            ("DOT_SLOT_DOWN", "_DOTS"),
        ] {
            let needle = format!("{a}{b}");
            assert!(
                !src.contains(&needle),
                "{needle} was reintroduced — the flex() migration deleted this \
                 render-time nudge (spec 40, b8-t3/t4/t5); dot-native \
                 positioning must absorb the offset into the computed DotRect, \
                 not a bolted-on constant"
            );
        }
    }
}

/// Precision-safety regression (mirrors `draw_dot_border`'s sub-cell test):
/// `render_stat_bars`/`render_dot_row` take a `DotRect` and honor its
/// sub-cell remainder, so a band positioned one DOT off a cell boundary
/// renders its content one dot lower — not snapped back to the nearest cell.
/// Guards the same class of bug that made the details-panel border land 2
/// dots off from `stat_bar`'s before the `draw_dot_border` fix: an
/// intermediate `.to_cell_rect()` in the draw chain silently discarding the
/// sub-cell offset. Today every value feeding these bands is cell-aligned, so
/// nothing exercises the offset in the live render — these tests inject the
/// remainder deliberately to prove the plumbing carries it.
#[cfg(test)]
mod sub_cell_precision_tests {
    use super::*;
    use crate::scenes::test_util::topmost_lit_dot_row;

    fn buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    /// A 1-dot sub-cell y offset on the stat-bar band moves the rendered
    /// border/fill down exactly 1 dot — decoded from the actual braille
    /// buffer, not the input geometry.
    #[test]
    fn render_stat_bars_honors_sub_cell_y_offset() {
        let scene = RosterManager::new();
        let base = RosterManager::cell_rect_to_dots(Rect::new(0, 0, 40, 6));
        let shifted = engine_render::DotRect { y: base.y + 1, ..base };

        let mut a = buf(40, 8);
        let mut b = buf(40, 8);
        scene.render_stat_bars(&mut a, base);
        scene.render_stat_bars(&mut b, shifted);

        // First slice's left border edge column: its topmost lit dot must sit
        // exactly one dot lower in the shifted render.
        let slices = crate::scenes::stat_bar::stat_slice_parts(base.to_cell_rect());
        let (outline, _fill, _label) = slices[0];
        let col = outline.left();
        let top_a = topmost_lit_dot_row(&a, col, outline.top());
        let top_b = topmost_lit_dot_row(&b, col, outline.top());
        assert_eq!(
            top_b, top_a + 1,
            "a 1-dot sub-cell y offset must move the stat bar's content down exactly 1 dot \
             (cell-aligned top {top_a}, shifted top {top_b}) — not snap back to the cell"
        );
    }

    /// A 1-dot sub-cell y offset on the dot-row band moves the rendered dot
    /// slot down exactly 1 dot.
    #[test]
    fn render_dot_row_honors_sub_cell_y_offset() {
        let scene = RosterManager::new();
        let base = RosterManager::cell_rect_to_dots(Rect::new(0, 0, 40, 4));
        let shifted = engine_render::DotRect { y: base.y + 1, ..base };

        let mut a = buf(40, 6);
        let mut b = buf(40, 6);
        scene.render_dot_row(&mut a, base);
        scene.render_dot_row(&mut b, shifted);

        let slots = RosterManager::dot_slots(base.to_cell_rect());
        let slot = slots[scene.current_index]; // the filled slot at rest
        let col = slot.x + slot.width / 2;
        let top_a = topmost_lit_dot_row(&a, col, slot.y);
        let top_b = topmost_lit_dot_row(&b, col, slot.y);
        assert_eq!(
            top_b, top_a + 1,
            "a 1-dot sub-cell y offset must move the dot-row content down exactly 1 dot \
             (cell-aligned top {top_a}, shifted top {top_b}) — not snap back to the cell"
        );
    }
}
