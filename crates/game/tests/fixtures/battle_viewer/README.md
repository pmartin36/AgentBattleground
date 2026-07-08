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
