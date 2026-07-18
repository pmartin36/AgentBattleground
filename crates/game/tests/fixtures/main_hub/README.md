# Main Hub golden fixtures (b1-t2, re-baselined b4-t1, b2-t2)

Braille-dot render freeze for `MainHub`, captured from
`crates/game/src/scenes/main_hub.rs`'s
`golden_fixture_tests::main_hub_golden_fixtures_match_pre_migration_baseline`
test. b4-t1 re-baselined all 3 fixtures to the procedural sword-in-stone logo
(`title_logo::frame`), held on its SETTLED still (`elapsed = 2.0`, past
`title_logo::ANIM_END`) — the bundled PNG logo render they used to freeze is
retired. b2-t2 re-baselined the 3 button-border shapes to `Button`'s
procedural dotted rounded border (spec 62 Decision 1), replacing the
pre-migration stretched-`FRAME_PANEL`-raster border.

## Scenarios

- `rest_120x50` — `MainHub::default()` with `elapsed = 2.0`, rendered 120x50
  (wide rest).
- `narrow_40x20` — `MainHub::default()` with `elapsed = 2.0`, rendered 40x20
  (narrow: the title box is now a FIXED 96x26 cells, so on a 40x20 viewport it
  overflows the screen and visually overlaps the menu — expected per Decision
  3's fixed-size, non-aspect-fit placement; `draw_grid`/`draw_dots` clip to
  the buffer bounds, so nothing panics or wraps).
- `cursor_exit_120x50` — `MainHub::default()` with `cursor_index = 2` and
  `elapsed = 2.0`, rendered 120x50 (cursor on the Exit button).

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

All 3 `*.preview.txt` files were visually reviewed before the current
fixtures were committed. The procedural sword-in-stone logo (stone slab, gold
"BATTLES", seated sword forming the "AGEN...T" T-cross) fills the title box,
centered top, with three stacked menu-button frames (Roster/Battle/Exit, top
to bottom) near the bottom of the screen. `narrow_40x20` shows the fixed-size
logo overflowing the small viewport and overlapping the menu (expected — see
Scenarios above); the button labels still render legibly inside their own
rects. `cursor_exit_120x50` shows the cursor arrow (`⠰⠆`) to the left of the
Exit button only, versus left of the Roster button in `rest_120x50` —
confirming `button_rects()[2]`'s geometry. Each button frame is a fully
corner-connected rounded rectangle (`⡎...⢱` top, `⢇...⡸` bottom, no
corner gaps) — `Button`'s procedural border (b2-t2). Confirmed correct; no
divergence found before commit.
