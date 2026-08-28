
## asset_gen tests share a global content-hashed temp root (no per-test isolation)

- Found by: feature asset-generation-api, task b2-t1 (validator).
- Severity: med.
- Observation: `asset_path` (crates/game/src/asset_gen/operations.rs:64) writes every generated
  asset to `std::env::temp_dir()/abg_assets/{request-content-hash}.{ext}`, and `materialize_image`
  (operations.rs:355) saves it with a non-atomic `image::save` (truncate-in-place). Any two tests
  that build a byte-identical `ImageRequest` therefore share one on-disk file with no isolation and
  race the truncating write against concurrent reads (`generate_animation`'s `image::open`),
  producing `Io("unexpected end of file")`. Concretely reproduced between compose.rs:193
  `image_req(1,None)` and operations.rs:482 `req(1,None)` (both seed 1) at
  `/tmp/abg_assets/832dbcea236bd16e.png`, 8/20 forced-co-schedule runs; the full `cargo test -p game`
  gate is a low-rate probabilistic flake.
- Immediate mitigation (owned by task b2-t1 test-writer): give compose.rs fixtures request content
  not already claimed by operations.rs so no two tests hash to the same path.
- Durable fix (unowned): isolate the asset root per test/process (e.g. a test-scoped temp dir), so
  identical request content no longer implies a shared global file. Touches the frozen operations.rs.

## Feature-gate red at baseline: MainHub debug-grid overlay test fails deterministically

- Found by: feature asset-generation-api, task b2-t1 (validator).
- Severity: high (forces the feature GATE_COMMAND `cargo test -p game` to exit 101 for every task
  in the feature; no asset-gen task can turn the gate green).
- Owner: unassigned. Predates the feature entirely, so no asset-generation-api task owns it; needs
  its own task.
- Observation: `app::tests::render_frame_applies_debug_grid_overlay_globally_across_scenes`
  (crates/game/src/app.rs:214) fails deterministically, in isolation and in the full suite, with:
  `panicked at crates/game/src/app.rs:279: scene SceneKey("MainHub") must render at least one
  plain-text cell for this guard to be meaningful`. The test renders MainHub's FIRST frame at 40x20
  and requires >=1 plain-text cell in `buf_before`; MainHub's first frame produces none (its menu
  labels Roster/Battle/Settings/Exit only settle to plain text after its intro fade), so the guard
  aborts. Reproduced failing at the pre-feature commit af1a903 and at both feature commits
  (f0fc774, d7a2a6d) — neither of which touches app.rs or scenes. Unrelated to asset_gen.
- Fix direction: the fix is in app.rs / MainHub, not in any asset_gen file. Either the test's
  first-frame assumption is stale (render after the intro settles, or seed a settled scene) or
  MainHub's frame-0 render legitimately shows no text and the guard must be relaxed. Determine which
  before touching either; it is a test-side stale-assumption in shape (the deliverable it guards,
  the global debug-grid overlay, still works for Leaderboard).

## Load resolves only the idle clip into a sprite; still/attack handles stay unresolved

- Found by: feature player-data-store, task b2-t2 (validator).
- Severity: low.
- Owner: unassigned. No planned task in player-data-store owns it; needs its own task once the
  runtime animation model grows the missing slots.
- Observation: spec 69 (specs/69-player-data-store.md:39) says load "resolves the handles back into
  sprites (still image, idle clip, attack clip)" for all three handles. The runtime animation model
  exposes exactly one `AnimationKind` (`Idle`, crates/game/src/creatures.rs:21) and no "still" slot;
  its only consumers read `AnimationKind::Idle` (battle_viewer/mod.rs:139, sizing.rs:310,
  post_battle/columns.rs:170, roster_manager/sprite_name.rs:73). `creature_from_persisted`
  (crates/game/src/player_data/convert.rs:32) therefore resolves ONLY the idle `ClipAsset` into an
  Idle sprite and preserves `still`/`attack` losslessly as handles on the runtime `Creature`
  (round-trip intact) without decoding them.
- Does NOT relax b2-t2's DELIVERABLE: the deliverable's "resolved sprites match handle-referenced
  frames" clause is met via the idle clip (verified: idle_clip_resolves_into_matching_sprite asserts
  frame count + first-frame red-pixel decode).
- Fix direction: full still/attack sprite resolution needs `AnimationKind::Attack` + a still slot +
  their runtime consumers (battle-animation work), then extend `creature_from_persisted` to resolve
  those two handles too.
