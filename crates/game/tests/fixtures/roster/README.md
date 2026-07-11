# Roster golden fixtures (b7-t1)

Pre-`flex()`-migration braille-dot render freeze for `RosterManager`, captured
from `crates/game/src/scenes/roster_manager.rs`'s
`golden_fixture_tests::roster_golden_fixtures_match_pre_migration_baseline`
test before any b8 `flex()` migration code was written.

## Scenarios

- `rest_40x20` — `RosterManager::new()`, rendered 40x20 (narrow rest).
- `rest_80x30` — `RosterManager::new()`, rendered 80x30 (wide rest).
- `index2_80x30` — `RosterManager::new()` with `current_index = 2`, rendered 80x30.
- `midslide_80x30` — `RosterManager::new()`, `Right` nav input, `update(75ms)`
  (~25% of the 300ms slide), rendered 80x30 (mid-slide-transition frame).

## Files

- `{name}.fixture` — serialized `Buffer` (via `serialize_braille_buffer`),
  loaded back by `load_roster_fixture` / `deserialize_braille_buffer`, and
  compared dot-for-dot against the live render via `diff_dots`.
- `{name}.preview.txt` — human-readable braille-art dump of the same render
  (via `buffer_to_art`), for manual eyeballing.

Only braille dot cells are captured/gated — `decode_braille_cell` returns
`None` for text cells, so creature name, "LVL n", ability text, "HOME", and
role labels are not represented in these fixtures. Text positioning stays
covered by `roster_manager.rs`'s existing `rect_text`-based assertions.

## Manual visual confirmation

All 4 `*.preview.txt` files were visually reviewed before these fixtures were
committed, against the current roster layout: sprite centered above its stat
cluster, 4 stat bars (STR/DEX/INT/VIT) with the dot-cluster role indicator
below, HOME button top-right, and Active/Bench/Reserve role-label dot slots
along the bottom row. The right-hand details panel (b1-b3 redesign) shows a
"Stamina" label + banded bar, an "Abilities" header + underline + 2x2 ability
grid, and an "Instructions" header + underline + "Edit" button + wrapped
preview text — all inside the bordered right-third, none of it bleeding past
its own row into a neighboring region. `rest_40x20`'s narrower terminal
collapses the Instructions header/Edit button/preview bands entirely (no
room); the panel border stays intact with no stray dots painted into it.
`midslide_80x30` shows the outgoing sprite/dot-cluster sliding off the right
edge and the incoming creature's partially entering from the left, with the
entire details panel interior suppressed (no stamina/ability/instructions
content, no bar, no edit button), consistent with the mid-slide (~25% of
`SLIDE_DUR`) transition frame. Confirmed correct; no divergence found before
commit.
