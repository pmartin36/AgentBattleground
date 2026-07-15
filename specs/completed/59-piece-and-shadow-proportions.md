> # ✅ DONE! — Completed 2026-07-15
> Status: implemented. `WIDTH_FILL_RATIO` is `0.77` (was `0.92`) and is now `pub(super)`; `shadow.rs`'s `SHADOW_WIDTH_RATIO` is defined AS `WIDTH_FILL_RATIO` (was an independent `0.55`), so ring and creature agree by construction. All three camera goldens re-baselined after a visual pass. `cargo test --workspace` green (1268 passed, 0 failed); `cargo clippy --workspace --all-targets` clean.

# Battle Viewer — Piece and Shadow Proportions

## Purpose
`58-battlefield-5x5-resize` shrank the board to 5×5. Because the board auto-fits the viewport (`58` Decision 7), fewer columns spanning the same screen makes every cell larger — and creatures, sized as a fraction of a cell, scaled up with it. Measured linear scale-up from the rendered dots, 7×7 → 5×5: **1.21× Top-Down, 1.32× Over-the-shoulder, 1.5× Sideline** (Sideline being the default camera, and the largest jump).

No ratio changed in `58` — the zoom did. But the result read as too big, and the resize also broke the relationship between a creature and the contact ring under it. This spec retunes both.

## Scope
- `WIDTH_FILL_RATIO` retuned for the 5×5 cell size.
- `SHADOW_WIDTH_RATIO` re-defined in terms of `WIDTH_FILL_RATIO` instead of being an independently tuned constant.
- Camera goldens re-baselined.

Out of scope:
- `SPRITE_DOT_RATIO` (Top-Down's separate height-based sizing path) — see Open Questions.
- Board geometry, camera placement, and piece layout — all `58`.
- Any change to the sizing *mechanism* (per-piece `local_dots_per_world_unit`, aspect-derived height, `MAX_SPRITE_DOT_DIMENSION` clamp). This spec changes two ratios, not how they are applied.

## Decisions (v1)

- **Decision 1 — `WIDTH_FILL_RATIO = 0.77`** (was `0.92`). Project owner's call: the 7×7 creatures read too small and the unadjusted 5×5 creatures too big, so the target is between the two. Derived rather than eyeballed: apparent size scales with the ratio, and area with its square, so landing Sideline's 1.5× jump near the ~1.25× midpoint gives `0.92 × √(1.25²/1.5²) ≈ 0.77`, then confirmed by rendering.

  This supersedes the previous "width fills the base of the cell (project owner's explicit ask)" note on the constant. That ask was made against 7×7's smaller cells; at 5×5 a near-exact fill is what reads as oversized. The constant is no longer a cell-fill — it is a tuned fraction.

- **Decision 2 — `SHADOW_WIDTH_RATIO` IS `WIDTH_FILL_RATIO`**, not a separate value (was `0.55`, with a comment calling it "deliberately smaller… so the shadow reads as a mark under the creature's feet, not another shape competing with it"). The ring reads as the creature's own footprint on its cell, so the two must agree by construction — as one definition, not two constants that happen to be tuned to compatible values. Two independent values drift the moment either side is retuned, which is exactly what the resize did: a 0.92-wide creature on a 0.55-wide ring, at a cell size where the gap became obvious. `WIDTH_FILL_RATIO` is `pub(super)` and `shadow.rs` imports it directly (`sizing` is not re-exported by the module).

- **Decision 3 — The ring stays elevation-squashed.** Only its width definition changes; height remains `width × sin(elevation)`, so an oblique camera still flattens the annulus and Top-Down still keeps it round. Unchanged from the existing mechanism.

## Open Questions / TBDs
- **Top-Down's ring and creature still do not agree, by construction.** Decision 2 ties the ring to `WIDTH_FILL_RATIO`, which governs Sideline/Over-the-shoulder only; Top-Down sizes creatures from `SPRITE_DOT_RATIO` (a *height* ratio, with width derived via aspect). So under Top-Down the ring is `WIDTH_FILL_RATIO` of a cell while the creature's width is whatever `SPRITE_DOT_RATIO × aspect` produces — visibly different in the re-baselined Top-Down golden, where the ring encircles the creature rather than matching it. Two sizing paths for the same object is the underlying issue; unifying them was not in this spec's scope and is worth an explicit follow-up.

## Dependencies
- `58-battlefield-5x5-resize` ✅ — the resize whose enlarged cells this retunes against; its Decision 7 (auto-fit) is why the cells grew at all.
- `13-rendering` ✅ / `33-scene-composite-primitive` ✅ — the dot/composite pipeline both ratios feed; unchanged.
