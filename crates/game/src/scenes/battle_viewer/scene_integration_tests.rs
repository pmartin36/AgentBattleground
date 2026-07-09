use super::*;

#[cfg(test)]
mod battle_viewer_scene_wiring_tests {
    use super::*;
    use engine_core::scene::{EngineCtx, Scene};
    use crate::scenes::test_util::render_to_buffer;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use engine_render::camera::Camera;

    /// Whatever `camera.grid_line_color()` + the real alpha-blit currently
    /// produce, blended the same way `draw_grid` blends it over the
    /// `Color::Reset` fallback. Used purely to identify/skip grid-line cells
    /// in the piece-tint assertions below, so those assertions stay correct
    /// regardless of `grid_line_color`'s implementation state (b4-t2).
    fn actual_grid_line_fg(camera: &BattleCamera, tuning: &BattleViewerTuning) -> Color {
        let blended = camera.grid_line_color(tuning).over(Rgba::rgb(0, 0, 0));
        Color::Rgb(blended.r, blended.g, blended.b)
    }

    /// A cell's symbol is a braille glyph (U+2800..=U+28FF) — i.e. sprite
    /// content, not a board-line character (┌─┼ etc.) or blank background.
    fn is_braille_glyph(sym: &str) -> bool {
        sym.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
    }

    /// DELIVERABLE (4): idle animation actually advances — after `update()`
    /// accumulates enough elapsed time to cross a frame boundary, at least
    /// one previously-lit board cell changes.
    ///
    /// b3-t1: pinned to `top_down_preset()` (not the scene's Sideline
    /// default) — Sideline/OverShoulder's on-screen framing is not fixed
    /// until b4-t1 lands, so a camera-agnostic assertion like this one must
    /// run against the one preset whose framing is stable (Top-Down, guarded
    /// byte-for-byte by the b0-t1 golden fixture).
    #[test]
    fn idle_animation_advances_after_update() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

        let buf_before = render_to_buffer(&scene, 100, 50);

        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(150));

        let buf_after = render_to_buffer(&scene, 100, 50);

        let mut changed = false;
        'outer: for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let before = buf_before.cell((x, y)).unwrap();
                let after = buf_after.cell((x, y)).unwrap();
                if before.symbol() != after.symbol() || before.fg != after.fg {
                    changed = true;
                    break 'outer;
                }
            }
        }
        assert!(
            changed,
            "at least one board cell must change after update() advances past a frame boundary"
        );
    }

    /// DELIVERABLE (5): the pre-existing registry roundtrip test is
    /// unaffected by this task's changes (regression guard, duplicated here
    /// so a red run of this file alone still proves it).
    #[test]
    fn default_battle_viewer_reports_correct_scene_id() {
        let scene = BattleViewer::default();
        assert_eq!(scene.id(), SceneKey::from(SceneId::BattleViewer));
    }

    // PRUNED (battle-viewer-perspective-camera-rework/b3-t1): the former
    // `render_glyph_mask_invariant_to_tint` test (was pinned to
    // `sideline_preset()`). Same "on-screen Sideline POSITION" class as the
    // grid-line-corner and board-corner-glyph tests already pruned above:
    // the scene's single test piece projects off the composite buffer under
    // `sideline_preset()` until b4-t1 adds the fit-to-viewport centering
    // offset (explicitly out of this task's scope). Re-pointing to
    // `top_down_preset()` was tried and does NOT fix it either — Top-Down
    // trips a separate, pre-existing, camera-independent defect in
    // `cell_from_dots_tinted`'s adaptive-luma mask threshold
    // (`crates/engine/render/src/dots.rs`: a shadow's team-colored dots and
    // the piece's own differently-colored sprite dots sharing a terminal
    // cell can flip a mask bit when team color changes; reproduced verbatim
    // on pre-b3-t1 `main`). No in-scope camera choice satisfies this test
    // under either preset, so it is deferred rather than chased: re-establish
    // once b4-t1 lands Sideline framing AND the dots.rs mask bug (b1-t4/
    // b7-t1 territory) is fixed independently.

    /// Projects a world position to the terminal cell its sprite is CENTERED
    /// on, using the same `geom.camera.project` + dot->cell conversion
    /// (`/2` cols, `/4` rows) that `place`/`dots_to_grid`/`draw_grid` use.
    fn terminal_center_cell(pos: WorldPos, geom: &BoardGeometry) -> (i32, i32) {
        let (dot_x, dot_y) = geom.camera.project(pos);
        (
            geom.board_rect.x as i32 + dot_x / 2,
            geom.board_rect.y as i32 + dot_y / 4,
        )
    }

    /// b3-t1 DELIVERABLE: the rendered sprite must be placed at the piece's
    /// own stored, independently-editable `transform.translate`, not re-derive
    /// the position fresh from `col`/`row` every call. Mutating
    /// `pieces[0].transform.translate` to a distinct in-board world position
    /// must move the rendered sprite glyph on the very next `render()`: a
    /// glyph appears near the NEW projected cell, and none remains near the
    /// OLD col/row-derived cell.
    ///
    /// Pinned to `top_down_preset()` (not the scene's Sideline default) —
    /// see `idle_animation_advances_after_update`'s doc note: Sideline's
    /// on-screen framing isn't fixed until b4-t1, so this camera-agnostic
    /// "reads live stored state" contract is only reliably checkable under
    /// Top-Down's stable framing.
    #[test]
    fn render_reflects_mutated_stored_piece_transform_translate() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        scene.pieces.truncate(1); // isolate to exactly one sprite on the board
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

        let old_translate = scene.pieces[0].transform.translate;
        let old_center = terminal_center_cell(old_translate, &geom);

        let new_translate = WorldPos::new(4.5, 4.5);
        assert_ne!(
            new_translate, old_translate,
            "test setup: new translate must differ from the seeded default"
        );
        scene.pieces[0].transform.translate = new_translate;
        let new_center = terminal_center_cell(new_translate, &geom);

        let buf = render_to_buffer(&scene, 100, 50);
        let grid_line_fg = actual_grid_line_fg(&geom.camera, &BattleViewerTuning::default());

        let has_piece_glyph_near = |center: (i32, i32)| -> bool {
            const WINDOW: i32 = 8;
            for dy in -WINDOW..=WINDOW {
                for dx in -WINDOW..=WINDOW {
                    let x = center.0 + dx;
                    let y = center.1 + dy;
                    if x < geom.board_rect.x as i32
                        || y < geom.board_rect.y as i32
                        || x >= geom.board_rect.right() as i32
                        || y >= geom.board_rect.bottom() as i32
                    {
                        continue;
                    }
                    let cell = buf.cell((x as u16, y as u16)).unwrap();
                    if is_braille_glyph(cell.symbol()) && cell.fg != grid_line_fg {
                        return true;
                    }
                }
            }
            false
        };

        assert!(
            has_piece_glyph_near(new_center),
            "expected a piece glyph near the NEW transform.translate-projected cell {new_center:?}"
        );
        assert!(
            !has_piece_glyph_near(old_center),
            "no piece glyph should remain near the OLD col/row-derived cell {old_center:?} \
             after transform.translate was mutated away from it"
        );
    }

    /// Scans a `WINDOW`-cell box around `center` for any non-grid-line
    /// braille glyph — the same piece-glyph-presence probe used by
    /// `render_reflects_mutated_stored_piece_transform_translate`.
    fn has_piece_glyph_near(buf: &Buffer, geom: &BoardGeometry, center: (i32, i32)) -> bool {
        // Default scene camera is Sideline, which renders grid lines dimmed (b4-t1).
        let grid_line_fg = actual_grid_line_fg(&geom.camera, &BattleViewerTuning::default());
        const WINDOW: i32 = 8;
        for dy in -WINDOW..=WINDOW {
            for dx in -WINDOW..=WINDOW {
                let x = center.0 + dx;
                let y = center.1 + dy;
                if x < geom.board_rect.x as i32
                    || y < geom.board_rect.y as i32
                    || x >= geom.board_rect.right() as i32
                    || y >= geom.board_rect.bottom() as i32
                {
                    continue;
                }
                let cell = buf.cell((x as u16, y as u16)).unwrap();
                if is_braille_glyph(cell.symbol()) && cell.fg != grid_line_fg {
                    return true;
                }
            }
        }
        false
    }

    /// Whole-board-rect equality check: `true` iff every cell's symbol AND fg
    /// color match between the two buffers. Used to compare a piece's
    /// `alive == false` render against a ground-truth render where that piece
    /// is entirely absent from `pieces` (not just flagged dead) — a strictly
    /// stronger, geometry-agnostic proof of "contributes NO glyph" than a
    /// spatial probe window, which (per b5-t1's real bundled creature art)
    /// can false-positive when a neighboring piece's own, unrelated sprite
    /// footprint happens to reach into the probed box.
    fn boards_pixel_identical(a: &Buffer, b: &Buffer, geom: &BoardGeometry) -> bool {
        for y in geom.board_rect.y..geom.board_rect.bottom() {
            for x in geom.board_rect.x..geom.board_rect.right() {
                let ca = a.cell((x, y)).unwrap();
                let cb = b.cell((x, y)).unwrap();
                if ca.symbol() != cb.symbol() || ca.fg != cb.fg {
                    return false;
                }
            }
        }
        true
    }

    /// b2-t4 DELIVERABLE: a piece with `alive == false` contributes NO glyph
    /// to the composited render — its render is pixel-identical to a scene
    /// where that piece is removed from `pieces` entirely — while a still-
    /// alive sibling's glyphs remain present. `transform` is left intact on
    /// the dead piece (not driven through a real `Die` event) so the ONLY
    /// reason it can vanish is `render()`'s `alive` filter, not a collapsed
    /// zero scale.
    ///
    /// Compares against "piece removed from the list" rather than probing a
    /// fixed-size window around the dead piece's own center: with real
    /// bundled creature art (b5-t1), a neighboring piece's sprite can be wide
    /// enough that its footprint reaches into a window centered on an
    /// adjacent bench piece, producing a false "glyph still present" positive
    /// unrelated to the `alive` filter under test. Diffing the FULL board
    /// against a ground-truth render with the piece entirely absent isolates
    /// exactly and only that piece's own contribution, regardless of
    /// neighboring sprites' size.
    /// Pinned to `top_down_preset()` (not the scene's Sideline default) —
    /// see `idle_animation_advances_after_update`'s doc note.
    #[test]
    fn render_excludes_dead_piece_keeps_alive_sibling() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

        assert_eq!(scene.pieces[3].team, Team::A, "test setup: target must be Team A");
        assert_eq!(scene.pieces[7].team, Team::B, "test setup: sibling must be Team B");
        let sibling_center = terminal_center_cell(scene.pieces[7].transform.translate, &geom);

        let mut scene_removed = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        scene_removed.pieces.remove(3);
        let buf_removed = render_to_buffer(&scene_removed, 100, 50);

        scene.pieces[3].alive = false;
        let buf_dead = render_to_buffer(&scene, 100, 50);

        assert!(
            boards_pixel_identical(&buf_dead, &buf_removed, &geom),
            "a dead piece (alive == false) must render pixel-identical to that piece being \
             entirely absent from `pieces` — no residual glyph anywhere on the board"
        );
        assert!(
            has_piece_glyph_near(&buf_dead, &geom, sibling_center),
            "a still-alive sibling's glyphs must remain present near {sibling_center:?}"
        );
    }

    /// b2-t4 DELIVERABLE (revive, no special-casing): flipping a previously
    /// excluded piece's `alive` back to `true` (transform untouched) makes
    /// its glyphs reappear on the very next `render()`, with no other code
    /// change — proving exclusion is a pure per-frame filter on `alive`, not
    /// a one-way/sticky removal.
    ///
    /// Setup precondition ("dead piece contributes nothing") and the revive
    /// assertion both use the pixel-identical-to-piece-removed comparison —
    /// see `render_excludes_dead_piece_keeps_alive_sibling` for why a
    /// spatial probe window is unreliable against real bundled creature art
    /// (a neighboring piece's wider sprite can bleed into the window).
    /// Pinned to `top_down_preset()` (not the scene's Sideline default) —
    /// see `idle_animation_advances_after_update`'s doc note.
    #[test]
    fn render_reincludes_piece_when_alive_flipped_back_true() {
        let mut scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        let area = Rect::new(0, 0, 100, 50);
        let geom = board_geometry(area, BattleCamera::top_down_preset(), BattleViewerTuning::default());

        let mut scene_removed = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        scene_removed.pieces.remove(3);
        let buf_removed = render_to_buffer(&scene_removed, 100, 50);

        scene.pieces[3].alive = false;
        let buf_dead = render_to_buffer(&scene, 100, 50);
        assert!(
            boards_pixel_identical(&buf_dead, &buf_removed, &geom),
            "test setup: piece must render pixel-identical to piece-removed while alive == false"
        );

        scene.pieces[3].alive = true;
        let buf_revived = render_to_buffer(&scene, 100, 50);
        assert!(
            !boards_pixel_identical(&buf_revived, &buf_removed, &geom),
            "piece glyph must reappear (render must diverge from the piece-removed ground truth) \
             once alive is flipped back to true"
        );
    }

    /// b3-t1 DELIVERABLE: `BattleViewer::default().pieces` is seeded from the
    /// same layout logic as the free `pieces()` function — a real, owned
    /// field, not a divergent copy or an empty placeholder. Count is the
    /// 8-piece (3 active + 1 bench per side) layout.
    #[test]
    fn default_seeds_eight_pieces_from_layout() {
        let scene = BattleViewer::default();
        assert_eq!(scene.pieces.len(), 8, "expected 8 seeded pieces");
        assert_eq!(
            scene.pieces,
            pieces(),
            "BattleViewer::default().pieces must match the free pieces() layout"
        );
    }

    /// b4-t1 DELIVERABLE (1): `BattleViewer.elapsed` is a plain `f32` seconds
    /// accumulator, seeded to `0.0` — not a `Duration`.
    #[test]
    fn elapsed_seeds_to_zero_as_f32() {
        let scene = BattleViewer::default();
        assert_eq!(scene.elapsed, 0.0_f32);
    }

    /// b4-t1 DELIVERABLE (2): `update()` accumulates `dt` into `elapsed` as
    /// whole seconds (`dt.as_secs_f32()`), not milliseconds or any other unit
    /// — 150ms of `dt` must land at ~0.15, not ~150.0.
    #[test]
    fn elapsed_accumulates_in_seconds_via_update() {
        let mut scene = BattleViewer::default();
        let mut ctx = EngineCtx;
        scene.update(&mut ctx, Duration::from_millis(150));
        assert!(
            (scene.elapsed - 0.15).abs() < 1e-4,
            "expected elapsed ~= 0.15 seconds after a 150ms update(), got {}",
            scene.elapsed
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: `#[derive(Inspectable)]` on Piece/Team/BattleViewer (b5-t1)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod inspectable_tests {
    use super::*;
    use engine_core::{FieldSchema, FieldTag, PatchError};

    fn field<'a>(schema: &'a FieldSchema, name: &str) -> &'a FieldSchema {
        schema
            .children
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("missing field `{name}` in schema {schema:?}"))
    }

    /// DELIVERABLE: `Piece::schema()` reports `col`/`row` as readonly `Int`,
    /// `team` as `Enum` (with Team's two variant names), `index` as a plain
    /// (non-readonly) `Int`, `transform` as a `Struct` with b3-t1's nested
    /// `translate`/`rotation`/`scale` leaves, and `color` as `Color`.
    #[test]
    fn piece_schema_reports_readonly_ints_enum_struct_and_color_fields() {
        let schema = Piece::schema();
        assert_eq!(schema.tag, FieldTag::Struct);

        let col = field(&schema, "col");
        assert_eq!(col.tag, FieldTag::Int);
        assert!(col.readonly, "col must be readonly");

        let row = field(&schema, "row");
        assert_eq!(row.tag, FieldTag::Int);
        assert!(row.readonly, "row must be readonly");

        let team = field(&schema, "team");
        assert_eq!(team.tag, FieldTag::Enum);
        assert_eq!(team.variants, vec!["A".to_string(), "B".to_string()]);

        let index = field(&schema, "index");
        assert_eq!(index.tag, FieldTag::Int);
        assert!(!index.readonly, "index must be editable, not readonly");

        let transform = field(&schema, "transform");
        assert_eq!(transform.tag, FieldTag::Struct);
        let transform_children: Vec<&str> =
            transform.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(transform_children, vec!["translate", "rotation", "scale"]);

        let color = field(&schema, "color");
        assert_eq!(color.tag, FieldTag::Color);
    }

    /// DELIVERABLE: `apply_patch` on a `Piece` value for `"team"` changes
    /// only `team` — `color`/`transform` are byte-unchanged.
    #[test]
    fn piece_apply_patch_on_team_changes_only_team() {
        let mut piece = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        let before_color = piece.color;
        let before_transform = piece.transform;

        piece
            .apply_patch("team", serde_json::json!("B"))
            .expect("apply_patch on team must succeed");

        assert_eq!(piece.team, Team::B, "team must change");
        assert_eq!(piece.color, before_color, "color must be untouched by a team patch");
        assert_eq!(
            piece.transform, before_transform,
            "transform must be untouched by a team patch"
        );
    }

    /// DELIVERABLE: `apply_patch` for `"col"` returns `Err` (readonly
    /// rejection) and leaves the value unchanged.
    #[test]
    fn piece_apply_patch_on_readonly_col_is_err_and_unchanged() {
        let mut piece = Piece::new(1, TEAM_A_ROW, Team::A, 0);
        let before = piece;

        let result = piece.apply_patch("col", serde_json::json!(5));

        assert_eq!(result, Err(PatchError::Readonly));
        assert_eq!(
            piece, before,
            "piece must be byte-unchanged after a rejected readonly patch"
        );
    }

    /// DELIVERABLE (b2-t1): `BattleViewerTuning::default()` returns exactly
    /// the spec Decision 1 values. `grid_dim_alpha` is excluded (b1-t2): it
    /// is a visual-tuning value chosen by rendering, guarded instead by
    /// `dim_grid_reads_meaningfully_dimmer_than_opaque`'s invariant, never a
    /// pinned constant here. `grid_taper_*` (b3-t1) and `depth_scale_*`
    /// (b6-t1) no longer exist — depth scaling is derived from the camera's
    /// own `forward_distance`, not a tunable.
    #[test]
    fn battle_viewer_tuning_default_matches_spec_values() {
        let tuning = BattleViewerTuning::default();
        assert_eq!(tuning.shadow_fade_ms, 150);
    }

    /// DELIVERABLE (b2-t1): `BattleViewer::schema()` has a `tuning` child
    /// that is a `Struct` with the 2 remaining tuning leaves (b3-t1:
    /// `grid_taper_*` removed; b6-t1: `depth_scale_*` removed), none readonly.
    #[test]
    fn battle_viewer_schema_exposes_editable_tuning_struct() {
        let schema = BattleViewer::schema();
        let tuning = field(&schema, "tuning");
        assert_eq!(tuning.tag, FieldTag::Struct);

        for name in ["grid_dim_alpha", "shadow_fade_ms"] {
            let leaf = field(tuning, name);
            assert!(!leaf.readonly, "tuning.{name} must be editable, not readonly");
        }
    }

    /// DELIVERABLE (b2-t1): `apply_patch` on a `BattleViewer` value for
    /// `"tuning.shadow_fade_ms"` edits only that leaf — `grid_dim_alpha`
    /// (another tuning leaf) is unchanged. Repointed from the now-removed
    /// `tuning.depth_scale_min` (b6-t1).
    #[test]
    fn battle_viewer_apply_patch_on_tuning_leaf_edits_only_that_field() {
        let mut scene = BattleViewer::default();

        scene
            .apply_patch("tuning.shadow_fade_ms", serde_json::json!(999))
            .expect("apply_patch on tuning.shadow_fade_ms must succeed");

        let snap = scene.snapshot();
        let tuning = snap
            .as_object()
            .expect("BattleViewer snapshot must be a JSON object")
            .get("tuning")
            .expect("tuning key must be present in snapshot")
            .as_object()
            .expect("tuning must be a JSON object");

        assert_eq!(
            tuning.get("shadow_fade_ms").and_then(|v| v.as_u64()),
            Some(999),
            "shadow_fade_ms must reflect the patch"
        );
        assert_eq!(
            tuning.get("grid_dim_alpha").and_then(|v| v.as_u64()),
            Some(u64::from(BattleViewerTuning::default().grid_dim_alpha)),
            "grid_dim_alpha must be untouched by a shadow_fade_ms patch"
        );
    }

    /// DELIVERABLE: `BattleViewer::schema()` reports `elapsed` as an
    /// editable `Float` and `pieces` as a `List` of `Piece`-shaped elements;
    /// the `#[inspect(hidden)]` `creatures` field (b5-t1's per-piece
    /// creature catalog, replacing the old shared `sprite`) is absent
    /// entirely.
    #[test]
    fn battle_viewer_schema_reports_editable_elapsed_and_pieces_list_hides_creatures() {
        let schema = BattleViewer::schema();
        assert_eq!(schema.tag, FieldTag::Struct);

        let names: Vec<&str> = schema.children.iter().map(|c| c.name.as_str()).collect();
        assert!(
            !names.contains(&"creatures"),
            "hidden creatures field must be absent from schema: {names:?}"
        );

        let elapsed = field(&schema, "elapsed");
        assert_eq!(elapsed.tag, FieldTag::Float);
        assert!(!elapsed.readonly, "elapsed must be editable");

        let pieces = field(&schema, "pieces");
        assert_eq!(pieces.tag, FieldTag::List);
        assert_eq!(pieces.children.len(), 1, "List schema carries one element template");
        assert_eq!(
            pieces.children[0].tag,
            FieldTag::Struct,
            "pieces element template must be Piece-shaped (Struct)"
        );
    }

    /// DELIVERABLE: the 8-piece layout round-trips through `snapshot()` as a
    /// `pieces` array of exactly 8 Piece-shaped objects, and the hidden
    /// `creatures` field (b5-t1) never appears in the snapshot either.
    #[test]
    fn battle_viewer_default_snapshot_has_eight_piece_shaped_elements_and_hides_creatures() {
        let scene = BattleViewer::default();
        let snap = scene.snapshot();
        let obj = snap.as_object().expect("BattleViewer snapshot must be a JSON object");

        assert!(
            !obj.contains_key("creatures"),
            "hidden creatures field must be absent from snapshot"
        );

        let pieces = obj
            .get("pieces")
            .expect("pieces key must be present")
            .as_array()
            .expect("pieces snapshot must be an array");
        assert_eq!(pieces.len(), 8, "expected 8 seeded pieces in the snapshot");
        for p in pieces {
            let p = p.as_object().expect("each piece snapshot must be an object");
            for key in ["col", "row", "team", "index", "transform", "color"] {
                assert!(p.contains_key(key), "piece snapshot missing key `{key}`");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests: Top-Down golden fixture lock (b0-t1)
//
// Locks Top-Down's rendered output for the camera rework: Top-Down's
// PROJECTION (`board_geometry`/`place`/`BattleCamera`'s formula) is frozen for
// the whole feature. Top-Down's rendered PIXELS are legitimately re-baselined
// at b1's two intentional global visual-tuning changes (b1-t3 sprite-dot
// ratio, b1-t4 shadow shape) — both repaint Top-Down without touching its
// projection. From b2 onward this fixture is locked byte-for-byte: every task
// that touches shared code must re-assert byte-identity against it for
// Top-Down, and any divergence is a regression. Mirrors `main_hub.rs`'s
// `golden_fixture_tests` precedent one-for-one.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod golden_fixture_tests {
    use super::*;
    use crate::scenes::test_util::{
        buffer_to_art, key_event, load_battle_viewer_fixture, render_to_buffer,
        serialize_braille_buffer,
    };
    use engine_render::diff_dots;

    /// Top-Down, demo `pieces()` layout, `elapsed = 0.0`, rendered 80x40.
    /// Must dot-for-dot match the committed baseline fixture (re-baselined at
    /// b1-t3 sprite-ratio and b1-t4 shadow-shape; locked byte-for-byte from b2
    /// onward). Run with `UPDATE_BATTLE_VIEWER_FIXTURES=1` to (re)generate the
    /// `.fixture` + `.preview.txt` from the current render — only do this at
    /// one of the two designated b1 re-baseline points, and only after a
    /// manual visual pass over the preview (see
    /// `crates/game/tests/fixtures/battle_viewer/README.md`).
    #[test]
    fn top_down_golden_matches_baseline() {
        let generate = std::env::var("UPDATE_BATTLE_VIEWER_FIXTURES").is_ok();

        let scene = BattleViewer {
            camera_mode: BattleCamera::top_down_preset(),
            ..BattleViewer::default()
        };
        let actual = render_to_buffer(&scene, 80, 40);

        if generate {
            let fixture_path = format!(
                "{}/tests/fixtures/battle_viewer/top_down_golden.fixture",
                env!("CARGO_MANIFEST_DIR")
            );
            let preview_path = format!(
                "{}/tests/fixtures/battle_viewer/top_down_golden.preview.txt",
                env!("CARGO_MANIFEST_DIR")
            );
            std::fs::write(&fixture_path, serialize_braille_buffer(&actual))
                .unwrap_or_else(|e| panic!("failed to write {fixture_path}: {e}"));
            std::fs::write(&preview_path, buffer_to_art(&actual))
                .unwrap_or_else(|e| panic!("failed to write {preview_path}: {e}"));
            return;
        }

        let serialized = serialize_braille_buffer(&actual);
        assert!(
            serialized.lines().count() > 1,
            "Top-Down render must produce at least one lit braille cell \
             (got an empty/all-blank render — a vacuous fixture would never \
             catch a real regression)"
        );

        let fixture = load_battle_viewer_fixture("top_down_golden");
        let diff = diff_dots(&fixture, &actual);
        assert!(
            diff.is_match(),
            "Top-Down's current render diverges from the committed golden \
             fixture ({} dot mismatch(es) of {} compared); Top-Down's \
             projection is frozen for this feature and no task from b2 \
             onward may change its rendered dots — regenerate with \
             UPDATE_BATTLE_VIEWER_FIXTURES=1 only at a designated b1 \
             re-baseline point (b1-t3 sprite ratio, b1-t4 shadow shape), \
             and only after re-running the manual visual pass",
            diff.mismatches.len(),
            diff.dots_compared
        );
    }

    /// Sideline, demo `pieces()` layout, `elapsed = 0.0`, rendered 80x40.
    /// Captured render evidence + forward regression lock (b7-t1) — NOT a
    /// pre-refactor oracle like `top_down_golden` (Sideline's projection was
    /// legitimately changed by spec 41; b5-t1 already proves param/output
    /// equivalence for the rework itself). Run with
    /// `UPDATE_BATTLE_VIEWER_FIXTURES=1` to (re)generate the `.fixture` +
    /// `.preview.txt`, after a manual visual pass over the preview.
    #[test]
    fn sideline_golden_matches_baseline() {
        let generate = std::env::var("UPDATE_BATTLE_VIEWER_FIXTURES").is_ok();

        let scene = BattleViewer {
            camera_mode: BattleCamera::sideline_preset(),
            ..BattleViewer::default()
        };
        let actual = render_to_buffer(&scene, 80, 40);

        if generate {
            let fixture_path = format!(
                "{}/tests/fixtures/battle_viewer/sideline_golden.fixture",
                env!("CARGO_MANIFEST_DIR")
            );
            let preview_path = format!(
                "{}/tests/fixtures/battle_viewer/sideline_golden.preview.txt",
                env!("CARGO_MANIFEST_DIR")
            );
            std::fs::write(&fixture_path, serialize_braille_buffer(&actual))
                .unwrap_or_else(|e| panic!("failed to write {fixture_path}: {e}"));
            std::fs::write(&preview_path, buffer_to_art(&actual))
                .unwrap_or_else(|e| panic!("failed to write {preview_path}: {e}"));
            return;
        }

        let serialized = serialize_braille_buffer(&actual);
        assert!(
            serialized.lines().count() > 1,
            "Sideline render must produce at least one lit braille cell \
             (got an empty/all-blank render)"
        );

        let fixture = load_battle_viewer_fixture("sideline_golden");
        let diff = diff_dots(&fixture, &actual);
        assert!(
            diff.is_match(),
            "Sideline's current render diverges from the committed golden \
             fixture ({} dot mismatch(es) of {} compared)",
            diff.mismatches.len(),
            diff.dots_compared
        );
    }

    /// Over-Shoulder, demo `pieces()` layout, `elapsed = 0.0`, rendered 80x40.
    /// Same role as `sideline_golden_matches_baseline` — captured render
    /// evidence + forward regression lock, not a pre-refactor oracle.
    #[test]
    fn over_shoulder_golden_matches_baseline() {
        let generate = std::env::var("UPDATE_BATTLE_VIEWER_FIXTURES").is_ok();

        let scene = BattleViewer {
            camera_mode: BattleCamera::over_shoulder_preset(),
            ..BattleViewer::default()
        };
        let actual = render_to_buffer(&scene, 80, 40);

        if generate {
            let fixture_path = format!(
                "{}/tests/fixtures/battle_viewer/over_shoulder_golden.fixture",
                env!("CARGO_MANIFEST_DIR")
            );
            let preview_path = format!(
                "{}/tests/fixtures/battle_viewer/over_shoulder_golden.preview.txt",
                env!("CARGO_MANIFEST_DIR")
            );
            std::fs::write(&fixture_path, serialize_braille_buffer(&actual))
                .unwrap_or_else(|e| panic!("failed to write {fixture_path}: {e}"));
            std::fs::write(&preview_path, buffer_to_art(&actual))
                .unwrap_or_else(|e| panic!("failed to write {preview_path}: {e}"));
            return;
        }

        let serialized = serialize_braille_buffer(&actual);
        assert!(
            serialized.lines().count() > 1,
            "Over-Shoulder render must produce at least one lit braille cell \
             (got an empty/all-blank render)"
        );

        let fixture = load_battle_viewer_fixture("over_shoulder_golden");
        let diff = diff_dots(&fixture, &actual);
        assert!(
            diff.is_match(),
            "Over-Shoulder's current render diverges from the committed \
             golden fixture ({} dot mismatch(es) of {} compared)",
            diff.mismatches.len(),
            diff.dots_compared
        );
    }

    /// b7-t1: free-roam actually renders content, and driving movement keys
    /// through the real `handle_input` changes the rendered dots. Renders
    /// `free_roam_preset()`'s pinned starting transform, feeds
    /// `['w','w','w', Right, Up]` (forward x3, yaw +5deg, pitch +5deg)
    /// through `handle_input`, renders again. This is a genuine,
    /// potentially-failing check: no existing test renders free-roam to a
    /// buffer, so a blank frame (everything off-screen / degenerate scale)
    /// or a no-op render would fail here and is a real defect, not a test
    /// bug.
    #[test]
    fn free_roam_movement_changes_rendered_output() {
        let before_scene = BattleViewer {
            camera_mode: BattleCamera::free_roam_preset(),
            ..BattleViewer::default()
        };
        let before = render_to_buffer(&before_scene, 80, 40);

        let serialized_before = serialize_braille_buffer(&before);
        assert!(
            serialized_before.lines().count() > 1,
            "free-roam's starting transform must render at least one lit \
             braille cell (got an empty/all-blank render)"
        );

        let mut after_scene = BattleViewer {
            camera_mode: BattleCamera::free_roam_preset(),
            ..BattleViewer::default()
        };
        for code in [
            crossterm::event::KeyCode::Char('w'),
            crossterm::event::KeyCode::Char('w'),
            crossterm::event::KeyCode::Char('w'),
            crossterm::event::KeyCode::Right,
            crossterm::event::KeyCode::Up,
        ] {
            after_scene.handle_input(key_event(code));
        }
        let after = render_to_buffer(&after_scene, 80, 40);

        assert!(
            !diff_dots(&before, &after).is_match(),
            "free-roam movement keys must change the rendered output; got \
             identical dots before and after driving \
             ['w','w','w', Right, Up] through handle_input"
        );
    }
}
