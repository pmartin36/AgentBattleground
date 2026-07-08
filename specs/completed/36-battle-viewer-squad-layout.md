> # ✅ DONE! — Completed 2026-07-08
> Status: implemented. `BOARD_COLS`/`BOARD_ROWS` are 7×7; `pieces()` emits exactly 8 `Piece`s (3 active + 1 bench per side), reserves unrendered; row layout (bench/active/…/active/bench) and centered active columns match spec exactly; bench pieces share the same tint/scale/animation treatment as active pieces, with no `SquadRole`-derived rendering branch. `cargo test --workspace` green.

# Battle Viewer — Squad Layout

## Purpose
`18-battle-viewer-baseline` built a fixed 8×8 board with a static 6v6 layout. The squad redesign (`34-creature-attributes-data-model`: 3 active + 1 bench + 2 reserve per player) makes that layout obsolete — only 3 creatures per side actually fight, and 1 bench creature per side should be visible standing behind its team's line. This spec changes the board geometry and piece layout to match. It does NOT touch the camera (still the single existing `SideView`) — that's `37-battle-viewer-dynamic-camera`.

## Scope
- Board geometry changes from 8×8 to **7×7**.
- **3 active creatures per side** occupy the board's contested area, centered columns (analogous to `18`'s "6 pieces on cols 1-6 of 8, edges empty" — same idea, narrower).
- **1 bench creature per side**, standing on the outermost row behind its team's active row (i.e. an extra row per side beyond the active line, still part of the same 7×7 grid, same scale/rendering as active pieces — no new parallax/band mechanism).
- **2 reserves per side are not rendered at all** — no placeholder, no off-screen marker, simply absent from the piece list this scene constructs.
- Total rendered pieces: 8 (3 active + 1 bench, per side × 2 sides) — down from 12.
- Piece placement, world-position math, depth compositing, idle animation, and event-playback (`20`) all continue to work exactly as today, just over the new geometry and piece count — this spec changes `BOARD_COLS`/`BOARD_ROWS`/`pieces()`'s layout math, not the rendering pipeline itself.

Out of scope:
- Camera modes / view switching (`37`).
- Grid-line prominence varying by view (`37` — this spec's board still uses one grid-line treatment, since there's only one camera).
- Wiring real player-configured squad state (which specific creatures are active/bench/reserve, from `35`'s roster screen) into the battle viewer — this spec's piece layout remains hand-authored/hardcoded demo data, matching `18`'s existing precedent, since no cross-scene data plumbing or real battle sim exists yet to make "your actual squad" meaningful here.
- Any change to `Event`/`EventKind` (`20`) — `piece_index` still targets a stable `Piece.index`, unaffected by the smaller roster.

## Decisions (v1)
- **`BOARD_COLS = 7`, `BOARD_ROWS = 7`** replace the existing `8`/`8` constants.
- **Row layout**: Team A's bench row is the topmost row (row 0), Team A's active row is row 1; Team B's active row is row 5, Team B's bench row is the bottommost row (row 6) — mirroring `18`'s "Team A row 0 / Team B last row" convention, just with an extra row per side inserted for the bench. Rows 2-4 remain the empty contested middle, same role as `18`'s original interior rows.
- **Column layout**: 3 active (and the 1 bench, directly behind them) occupy centered columns, symmetric empty margins on both edges — the exact centered column set (e.g. cols 2-4 of 0-6) is an implementation-level call following `18`'s existing centering approach, not re-litigated here.
- **`pieces()` layout**: extended to emit 8 `Piece`s (4 per team: 3 active + 1 bench) instead of 12, using the same `Piece::new(col, row, team, index)` constructor and stable-index convention (`20`'s event-playback `piece_index` targeting) unchanged.
- **No new `SquadRole`-derived rendering distinction** — bench pieces render with the same tint/scale/animation treatment as active pieces this round (per the earlier "extra grid row, not a parallax band" call). If a future round wants the bench creature visually distinguished as "further back," that's an explicit follow-up, not built here.

## Open Questions / TBDs
- None outstanding — the geometry, row/column layout, and piece count are all decided above. Exact column centering values are an implementation detail.

## Dependencies
- `18-battle-viewer-baseline` ✅ — the board geometry / piece-layout code this spec modifies.
- `20-battle-viewer-event-playback` ✅ — its `Event`/`EventKind`/`piece_index` model is unaffected; continues to work over the new 8-piece layout unchanged.
- `34-creature-attributes-data-model` — the active/bench/reserve squad concept this layout mirrors (positionally, not yet data-wired).
- `33-scene-composite-primitive` ✅ — `composite_scene` continues to be the rendering path; this spec only changes what's fed into it (piece count/positions), not the compositor itself.
- Feeds `37-battle-viewer-dynamic-camera` — the new geometry the camera-mode spec renders from multiple angles.
