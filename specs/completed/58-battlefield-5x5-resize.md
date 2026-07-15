> # ✅ DONE! — Completed 2026-07-15
> Status: implemented. `BOARD_COLS`/`BOARD_ROWS` are 5×5; bench rows derive from `MIDLINE_ROW` (1/3 at 5×5, reproducing 2/4 at 7×7); `OVER_SHOULDER_CAMERA_DEPTH` derives from `BOARD_ROWS * 10/7` (~7.14, reproducing 10.0 at 7×7); the `board_size_constants_are_5x5` tripwire and the 5×5-sized `grid.rs` `geom()` fixture are updated; `no_two_pieces_share_a_cell_across_teams` and `bench_rows_straddle_the_midline_on_distinct_rows` added (Decision 9) — the former verified to FAIL under the old bench rule (7 unique cells, not 8) and pass under the new one. All three camera goldens re-baselined after a visual pass. `cargo test --workspace` green (1268 passed, 0 failed); `cargo clippy --workspace --all-targets` clean.
>
> Layout verified by decoding rendered dots, not by reading coordinates: Top-Down shows 5 grid columns with Team A's trio on row 1 at cols 1–3, bench at col 5 outside the grid, Team B mirrored on row 3, and row 2 as the empty contested middle. Both perspective cameras auto-refit and fill the frame (Decision 7 confirmed empirically). Bench slack under Over-the-shoulder is 1 dot (was 13 at 7×7) — measured, not clipped; see Decision 6a.

# Battlefield 5×5 Resize

## Purpose
`36-battle-viewer-squad-layout` sized the board 7×7 to hold 3 active + 1 bench per side. The bench has since moved off the drawn grid entirely (`BENCH_COL`, one column past the board's far edge), which means the grid itself no longer has to carry the bench — two of its seven rows exist only as empty framing. This spec resizes the board to **5×5**, the smallest grid that still holds a 3-wide active line with symmetric margins and a contested middle row.

Resizing exposes two latent defects that this spec also fixes, because 5×5 is unreachable without them:

- **The bench-row rule is not actually encoded.** `TEAM_A_BENCH_ROW` is a hardcoded `2` while `TEAM_B_BENCH_ROW` is `BOARD_ROWS - 3`. Both yield the intended 2/4 at 7×7 by coincidence, not by rule. At 5 rows they both compute to row 2 — and since `BENCH_COL` is shared, both teams' bench pieces would occupy the identical cell. The real rule is **midline ∓ 1**, which this spec encodes.
- **`OVER_SHOULDER_CAMERA_DEPTH` is an absolute literal (`10.0`) expressing a relative intent.** Its own doc comment defines it as "a comfortable margin past `BOARD_ROWS`," and records a shipped bug when that margin was too thin. Held at `10.0` against a 5-row board the margin silently grows from 3 to 5, flattening the shot. This spec derives it.

Both fixes reproduce today's 7×7 values exactly, so neither is a behavior change at the current size — they are the same rule, finally written down.

## Scope
- Board geometry changes from **7×7 to 5×5** (`BOARD_COLS`, `BOARD_ROWS`).
- `TEAM_A_BENCH_ROW` / `TEAM_B_BENCH_ROW` re-derived from the board midline rather than one hardcoded literal and one unrelated offset.
- `OVER_SHOULDER_CAMERA_DEPTH` re-derived from `BOARD_ROWS`, preserving its tuned standoff margin.
- Tests updated to the new size, including the deliberate `board_size_constants_are_7x7` tripwire.
- `36-battle-viewer-squad-layout`'s stale row-layout description corrected (see Decision 7).

Out of scope:
- The battle simulation engine (`10`) — it does not exist yet; no coordinates, positions, or bounds checks anywhere in `crates/`. Nothing to resize.
- Piece count, squad composition, tint/scale/animation treatment, event playback (`20`), and the `Piece`/`Event` models — all unchanged. This spec moves where pieces stand, not what they are.
- `SIDELINE_CAMERA_DEPTH` (see Decision 6) and every other geometry/grid/sizing consumer, all of which already derive from the board constants and need no edit.
- Wiring real squad state into the viewer — still hand-authored demo data, unchanged from `36`.

## Decisions (v1)

- **Decision 1 — `BOARD_COLS = 5`, `BOARD_ROWS = 5`** replace `7`/`7` as the single source of truth. Every existing consumer already derives from these constants; no other production value is edited to accommodate the new size.

- **Decision 2 — Board layout at 5×5.** Each team's active line is its own back row; bench sits one column past the board's far edge, at two rows straddling the midline:

  ```
       c0  c1  c2  c3  c4 | c5
  r0    .   A   A   A   . |
  r1    .   .   .   .   . | A-bench    ─┐
  r2    .   .   .   .   . |             ├─ contested middle
  r3    .   .   .   .   . | B-bench    ─┘
  r4    .   B   B   B   . |
  ```

- **Decision 3 — Bench rows are midline ∓ 1**, derived: `TEAM_A_BENCH_ROW = BOARD_ROWS / 2 - 1`, `TEAM_B_BENCH_ROW = BOARD_ROWS / 2 + 1`. Yields 1/3 at 5×5 and reproduces today's 2/4 at 7×7. This replaces the hardcoded `2` and the `BOARD_ROWS - 3` offset, which agreed with the rule only at 7. Vertical symmetry (`A_bench + B_bench == BOARD_ROWS - 1`) holds for any odd `BOARD_ROWS`.

- **Decision 4 — `BENCH_COL = BOARD_COLS` is unchanged** (col 5 at 5×5): flush past the last drawn column, not separated by a gap. The constant's existing rationale — bench reading as "behind the field" under Sideline and "off to the side" under Over-the-shoulder, because each camera maps the column axis to a different screen axis — is size-independent and survives the resize intact.

- **Decision 5 — Each team's active row is its own BACK row**: `TEAM_A_ROW = 0`, `TEAM_B_ROW = BOARD_ROWS - 1` → rows 0 and 4. Everything between the two lines is contested ground (rows 1–3), with each team's bench inside that span, off-grid on `BENCH_COL`.

  This *changes* the previous rule rather than carrying it over. The old `TEAM_A_ROW = 1` / `TEAM_B_ROW = BOARD_ROWS - 2` inset the lines one cell, leaving an empty framing row behind each team. At 7 rows that still left 3 rows between the teams, so it read fine; at 5 it leaves only the single midline row, which reads as both squads bunched in the middle of the board with a row wasted behind each. An inset that was invisible at 7×7 is not invisible at 5×5 — carrying the rule over unexamined because it "needed no edit" was the error.

  Columns keep their existing rule with no edit: `COL_MARGIN = (BOARD_COLS - 3) / 2` gives `ACTIVE_COLS = [1, 2, 3]` with symmetric single-column margins.

- **Decision 6 — Camera depths.** `OVER_SHOULDER_CAMERA_DEPTH` becomes `BOARD_ROWS * 10/7` → `~7.14` at 5×5, `10.0` at 7×7 (today's value exactly). The tuned quantity is neither the absolute depth nor the absolute margin, but the **near/far distance ratio** `depth / (depth - BOARD_ROWS)` — which is what the constant's own doc identifies as the thing that blows up the near/far sprite-size ratio. Holding that ratio at the 7-row board's `10/3 ≈ 3.33` makes the shot's perspective character size-invariant. Measured, not reasoned: pinning depth `10.0` gives ratio 2.0 and pinning margin `3.0` gives 2.67, and **both put the bench sprite's leftmost dot at dot-column 0 with zero slack**; the ratio-derived depth restores slack while reproducing 7×7 exactly. `SIDELINE_CAMERA_DEPTH` stays the absolute `-4.0`: it is a fixed standoff *before* the near edge at world-0, so its distance to the board is already board-size-independent (measured: 45 dots of slack, unaffected by the resize). `BOARD_CENTER_COL` (`BOARD_COLS / 2`) re-derives to `2.5` on its own.

- **Decision 6a — Known tightness, accepted.** Over-the-shoulder bench slack is inherently narrower at 5×5 than at 7×7 (measured leftmost chromatic dot: 13 at 7×7, 1 at 5×5). Cause is geometric, not camera tuning: `BENCH_COL` sits one full column outside the board, which is 1/5 of the board's width at 5×5 versus 1/7 at 7×7, so the bench projects relatively further toward the frame edge no matter where the camera stands. Nothing is clipped — verified by decoding the rendered dots, whose left-edge histogram (`0, 3, 2, 1, 2, …`) is a sprite taper, not the abrupt vertical cut a truncation produces. Accepted for now. The robust fix, if this ever does clip, is to fit on the pieces' **sprite extents** rather than `board_world_corners()`'s bench *centers* — deliberately out of scope here, since it would re-frame every camera and re-baseline every golden.

- **Decision 7 — Overall framing is unaffected, and this is load-bearing.** Sideline and Over-the-shoulder use `FitMode::ViewportFit`, whose `fit_perspective_geometry` auto-fits scale from `board_world_corners()` — which already includes both bench positions. A smaller board therefore re-fits to fill the viewport rather than rendering smaller. Camera depth changes the *perspective character* (taper/foreshortening), not the apparent size. Top-Down's `FitMode::ExactFit` likewise derives its cell sizing from the constants. No fit code is edited.

- **Decision 8 — The `board_size_constants_are_7x7` tripwire is updated, not deleted.** It exists to force anyone changing the board size to confront the decision deliberately; it is renamed to `board_size_constants_are_5x5` and re-asserted against `5`. Tests that hardcode 7-derived expectations (dot-grid extents, `board_rect` values, world positions assuming rows/cols 5–6 exist) are updated to the new size.

- **Decision 9 — Bench-uniqueness gets a real test.** The collision this resize would have caused was invisible to the suite: the piece-uniqueness test keys on `(team, col, row)`, so two teams' pieces sharing a cell passed, and the symmetry assertion `A_bench + B_bench == BOARD_ROWS - 1` holds at `2 + 2 == 4`. A test asserting the two bench pieces occupy distinct cells — keyed on `(col, row)` alone, ignoring team — is added so the defect class is caught by CI rather than by eye.

## Open Questions / TBDs
- None. Size, layout, bench rule, and camera derivation are all decided above.

## Dependencies
- `36-battle-viewer-squad-layout` ✅ — the 7×7 geometry and row/column layout this spec resizes and corrects.
- `18-battle-viewer-baseline` ✅ — the board/piece-layout code being modified.
- `39-battle-viewer-camera-perspective-rework` ✅ / `41-battle-viewer-perspective-camera-rework` ✅ — established `BENCH_COL` off-grid and the camera constants Decision 6 re-derives.
- `42-engine-camera-kind-api-and-free-roam-camera` ✅ — the `FitMode` auto-fit behavior Decision 7 relies on.
- `20-battle-viewer-event-playback` ✅ — unaffected; `piece_index` targeting works unchanged over the same 8 pieces.
