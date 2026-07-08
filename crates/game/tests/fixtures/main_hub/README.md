# Main Hub golden fixtures (b1-t2)

Pre-`flex()`-migration braille-dot render freeze for `MainHub`, captured from
`crates/game/src/scenes/main_hub.rs`'s
`golden_fixture_tests::main_hub_golden_fixtures_match_pre_migration_baseline`
test before any b2/b3/b4 `flex()` migration code was written.

## Scenarios

- `rest_120x50` — `MainHub::default()`, rendered 120x50 (wide rest).
- `narrow_40x20` — `MainHub::default()`, rendered 40x20 (narrow: exercises
  `title_size`'s width-floor `.max(20)` and height-cap clamps).
- `cursor_exit_120x50` — `MainHub::default()` with `cursor_index = 2`,
  rendered 120x50 (cursor on the Exit button).

## Files

- `{name}.fixture` — serialized `Buffer` (via `serialize_braille_buffer`),
  loaded back by `load_main_hub_fixture` / `deserialize_braille_buffer`, and
  compared dot-for-dot against the live render via `diff_dots`.
- `{name}.preview.txt` — human-readable braille-art dump of the same render
  (via `buffer_to_art`), for manual eyeballing.

Only braille dot cells are captured/gated — `decode_braille_cell` returns
`None` for text cells, so the "Roster"/"Battle"/"Exit" button labels are not
represented in these fixtures.

## Manual visual confirmation

All 3 `*.preview.txt` files were visually reviewed before these fixtures were
committed: title logo box centered top, three stacked menu-button frames
(Roster/Battle/Exit, top to bottom) near the bottom of the screen.
`narrow_40x20` shows the same layout compressed to fit the narrow viewport,
confirming the title/menu clamp paths render sane at small dimensions.
`cursor_exit_120x50` shows the cursor arrow (`⠰⠆`) to the left of the Exit
button only, versus left of the Roster button in `rest_120x50` — confirming
`button_rects()[2]`'s geometry. Confirmed correct; no divergence found before
commit.
