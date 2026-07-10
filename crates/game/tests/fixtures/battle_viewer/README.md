# Battle Viewer golden fixtures (b0-t1)

Pre-camera-rework braille-dot render freeze for `BattleViewer`, captured from
`crates/game/src/scenes/battle_viewer.rs`'s
`golden_fixture_tests::top_down_golden_matches_baseline` test before any
b1-b7 perspective-camera-rework code was written. This is the acceptance
oracle for the whole feature: no task in `battle-viewer-perspective-camera-rework`
may change Top-Down's rendered dots.

## Scenarios

- `top_down_golden` — `BattleViewer::default()` with `camera_mode:
  BattleCamera::top_down_preset()`, demo `pieces()` layout, `elapsed = 0.0`,
  rendered 80x40.
- `sideline_golden` — `BattleViewer::default()` with `camera_mode:
  BattleCamera::sideline_preset()`, demo `pieces()` layout, `elapsed = 0.0`,
  rendered 80x40. Added at b7-t1 (`engine-camera-kind-api-free-roam`), NOT a
  pre-refactor oracle — Sideline's projection was legitimately changed by
  spec 41. This is captured render evidence plus a forward regression lock
  from b7-t1 onward; the actual proof that spec 42 left Sideline's
  param/output behavior unchanged is `battle_viewer.rs`'s b5-t1 preset
  param/output-equivalence tests, re-run by this feature's gate.
- `over_shoulder_golden` — same role as `sideline_golden`, with
  `camera_mode: BattleCamera::over_shoulder_preset()`.

## Files

- `{name}.fixture` — serialized `Buffer` (via `serialize_braille_buffer`),
  loaded back by `load_battle_viewer_fixture` / `deserialize_braille_buffer`,
  and compared dot-for-dot against the live render via `diff_dots`.
- `{name}.preview.txt` — human-readable braille-art dump of the same render
  (via `buffer_to_art`), for manual eyeballing.

## Manual visual confirmation

`top_down_golden.preview.txt` was visually reviewed before this fixture was
committed: an 8x4 grid of board cells (Top-Down camera), 6 creature sprites
placed inside their cells (one cluster near the top rows, two clusters near
the bottom rows), no blank/empty render. Confirmed correct; no divergence
found before commit.

`sideline_golden.preview.txt` and `over_shoulder_golden.preview.txt` were
visually reviewed before commit (b7-t1): both show a perspective board grid
with converging gridlines and creature sprite clusters visible mid-frame, no
blank/empty render. Confirmed correct; no divergence found before commit.

`sideline_golden.preview.txt` and `over_shoulder_golden.preview.txt` were
regenerated at b2-t1 (`engine-camera-shots-vs-kinds-consolidation`) for the
canonical-rotation (Option B) correction: `PerspectiveCamera::cam_space` was
reverted to the plain, unnegated rotation, which horizontally mirrors these
two shots relative to their prior baseline. Both regenerated previews still
show a perspective board grid with converging gridlines and creature sprite
clusters visible mid-frame, no blank/empty render — the same content,
left-right flipped. `top_down_golden` is unaffected (unchanged fixture,
verified via `git diff`). Independent human visual sign-off on the
regenerated previews is b2-t2's deliverable, not this one.
