# Hatchery redesign — task list

Tracking the roster-style hatchery rework and its shared-primitive prerequisites. Specs are in this directory; each build runs through the TDD pipeline behind the full gate (`cargo test -p game && cargo clippy -p game --all-targets -- -D warnings`).

## Done
- [x] Live-test fixes to the old inline hatchery: single starter egg, no tray mad-lib, no teal background, clean blank underlines, varied templates (`75`, shipped).
- [x] Write and settle the spec chain with zero open questions: `76` engine border, `77` tooltip primitive, `78` shared stat bars, `79` hatchery layout.

## Foundational builds (independent of each other; all block `79`)
- [ ] **76 — engine sub-cell dot placement + `draw_dot_border`.** Add sub-cell `DotRect` placement, compose the border on `rounded_rect`, migrate the roster border. Byte-identical, proven by roster's border tests.
- [ ] **77 — tooltip primitive.** Extract roster's tooltip shell to a shared game module + plain-text path; migrate the ability tooltip with no visible change.
- [ ] **78 — shared stat-bar rendering.** Hoist roster's `stat_bar` + `draw_dot_cap_box` to a shared, stats-driven module; migrate roster.

## Checkpoint
- [ ] **Verify the roster is unchanged in-game** after the three migrations, before building on top.

## Hatchery
- [ ] **79 — roster-style layout.** Egg left, STATUS/DESCRIPTION/button panel right, egg dock bottom (ring-on-hover, click-to-open). Hatch hand-off: egg to center, panel off, cede to `68`/`72`, stat bars fade in via `78` as the dock settles. Empty dock keeps the last-hatched creature shown.

## After the UX is right
- [ ] **Fix the ugly eggs.** The egg sprites look bad. Research first (`needs-research/egg-art-and-definition.md`), then a real art/generation spec, then build.
