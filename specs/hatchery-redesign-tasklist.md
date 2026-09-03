# Hatchery redesign — task list

Tracking the roster-style hatchery rework and its shared-primitive prerequisites. Specs are in this directory; each build runs through the TDD pipeline behind the full gate (`cargo test -p game && cargo clippy -p game --all-targets -- -D warnings`).

## Done
- [x] Live-test fixes to the old inline hatchery: single starter egg, no tray mad-lib, no teal background, clean blank underlines, varied templates (`75`, shipped).
- [x] Write and settle the spec chain with zero open questions: `76` engine border, `77` tooltip primitive, `78` shared stat bars, `79` hatchery layout.

## Foundational builds (independent of each other; all block `79`)
- [x] **76 — engine sub-cell dot placement + `draw_dot_border`.** Added `draw_dots_at` + `draw_dot_border` to the engine, migrated the roster border onto them, `draw_dot_box` deleted. Roster border tests pass unchanged (byte-identical); added direct sub-cell precision tests.
- [x] **77 — tooltip primitive.** Shared `scenes/tooltip` module (`layout`/`draw_frame`/`render_text`); roster ability tooltip migrated onto it, local `shell.rs` gone, all 40 tooltip tests pass unchanged.
- [x] **78 — shared stat-bar rendering.** Shared `scenes/stat_bar` module (`draw_stat_bars` + `draw_dot_cap_box` + opacity fade); roster's `render_stat_bars` delegates, cap box moved out of roster, 21 stat-bar tests pass unchanged.

## Checkpoint
- [ ] **Verify the roster is unchanged in-game** after the three migrations, before building on top. (Awaiting owner check: border, ability tooltip, stat bars.)

## Hatchery
- [ ] **79 — roster-style layout.** Egg left, STATUS/DESCRIPTION/button panel right, egg dock bottom (ring-on-hover, click-to-open). Hatch hand-off: egg to center, panel off, cede to `68`/`72`, stat bars fade in via `78` as the dock settles. Empty dock keeps the last-hatched creature shown.

## After the UX is right
- [ ] **Fix the ugly eggs.** The egg sprites look bad. Research first (`needs-research/egg-art-and-definition.md`), then a real art/generation spec, then build.
