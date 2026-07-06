# Engine / Game Crate Split

> # ✅ DONE! — Completed 2026-07-03
> Status: implemented, all 7 phases (A–G). Workspace is now `crates/engine/{core,render,derive,inspector}` + `crates/game`, five crates under two directories. `SceneKey`/`SceneCatalog` replace `SceneId` on the engine side (`game::scene_id::SceneId` is the sole closed enum, converted via `From`/`from_key`); `Scene`/`SceneManager`/the IPC transport/inspector-spawn logic all live in `engine-core`; digit-hotkey dispatch lives solely in `game::app`; `engine-render` ships zero bundled game content (`Button`/`FrameButton` take asset bytes as constructor parameters, `game::assets`/`game::creatures` hold this game's actual art); `CLAUDE.md`/`README.md` document the boundary. All crate moves used `git mv` (blame history verified intact via `git log --follow`). Wire format unchanged — both e2e tests pass with only mechanical type-plumbing diffs, no assertion changes.
>
> Structural refactor, not a feature. Converts the current 5-crate workspace (`scene-core`, `scene-core-derive`, `render`, `game`, `inspector`) into a clean two-product **directory** shape — `crates/engine/` (a group of sub-crates: `engine-core`, `engine-render`, `engine-derive`, `inspector`) that is reusable by any future game, and `crates/game/` that contains only this game's content. Still a multi-crate workspace throughout — this spec relocates and renames crates, it does not collapse them into one monolithic library. No behavior changes — the M1/M2 scene-switcher and inspector contracts (`14-scene-architecture.md`, `15-debug-inspector.md`) are preserved exactly; only which crate owns each piece of code (and where that crate lives on disk) moves.

## Purpose
Today the crate meant to be the shared foundation (`scene-core`) is itself entirely this game's domain content (`SceneId` hardcodes the 9 Agent Battleground scenes), the scene-switching engine loop (`SceneManager`, `Scene` trait) lives inside `game` rather than the engine, and `render` bundles this game's actual art (6 creature GIFs, logo, button/icon skin) via `include_bytes!`. This spec inverts that: after conversion, everything under `crates/engine/` knows nothing about battles, rosters, creatures, or leaderboards, and `crates/game/` knows nothing about braille compositing internals, wire framing, or Unix-socket plumbing — it only calls the engine sub-crates' public APIs and supplies its own content.

## Current Coupling (baseline, before this spec)
- `crates/scene-core/src/scene_id.rs`: closed `SceneId` enum, hardcoded to this game's 9 scenes. Depended on by `render`, `inspector`, and `game` alike — the literal "core" is game content.
- `crates/scene-core/src/ipc.rs`: every wire message (`Hello`, `CatalogEntry`, `SceneChanged`, `ApplyState`, `StateSnapshot`) carries `SceneId` as a typed Rust field.
- `SceneId` is referenced extensively across `scene-core`, `game`, `inspector`, and examples/tests — the majority of the mechanical work in this conversion. (Exact file/reference counts drift as other work lands; re-grep at implementation time rather than trust a number pinned in this document.)
- `scene-core-derive`'s generated code hardcodes `::scene_core::...` paths throughout `crates/scene-core-derive/src/lib.rs` — any crate rename must update every one of the proc-macro's emitted paths too, or every `#[derive(Inspectable)]` call site breaks. (Same caveat: count fresh, don't trust a number written here.)
- `crates/game/src/scene.rs` (`Scene` trait, `EngineCtx`, `Transition`, `InputEvent`, `NoInspect`) and `crates/game/src/manager.rs` (`SceneManager`) are the actual engine main-loop — generic scene-switching/IPC-pump/live-snapshot machinery — but live in `game`, reaching into game-specific code only via `registry::construct`/`crate::scenes::scene_for_digit`. Neither file imports anything from `render` — both depend only on `scene_core`, `ratatui`, `crossterm`, and `serde_json`.
- `crates/game/src/ipc_server.rs` (Unix-socket transport thread) and `crates/game/src/inspect.rs` (`--inspect` flag, sibling-binary discovery, spawn logic) are already 100% generic — nothing in either file is game-specific — but also live in `game`.
- `crates/render/src/creature.rs` + `creature/bundled.rs` + `assets/creatures/*.gif`: 6 concrete creatures (Ember Wolf, Frost Lizard, Shadow Cat, Stone Golem, Storm Hawk, Verdant Treant) baked into the rendering crate via `include_bytes!`.
- `crates/render/src/assets.rs`: this game's UI skin (logo, button/frame panel textures, home/arrow/dot icons) baked into the rendering crate the same way.
- Everything else in `render` (`dots.rs`, `grid.rs`, `composite.rs`, `convert.rs`, `camera.rs`, `transform.rs`, `tween.rs`, `screen_layout.rs`, `button.rs`'s state machine, `anim.rs`) and in `scene-core` (`color.rs`, `inspect.rs`) is already fully generic — confirmed via grep, zero game-domain references.
- `inspector` already depends only on `scene-core` (never `render` or `game`) and only ever touches scenes through runtime `Hello`/`CatalogEntry` payloads plus the `SceneId` Rust type for local bookkeeping (HashMap keys, dropdown selection) — it does not pattern-match on specific variants anywhere. It needs no compile-time knowledge of what scenes exist once `SceneId` is replaced with an opaque key.

## Scope
Convert the workspace to five crates under two top-level directories:
- `crates/engine/core` (package `engine-core`, lib) — `Rgba`, `Inspectable`/`FieldSchema`, wire IPC framing, `Scene`/`SceneManager`/`SceneCatalog`/`SceneKey`, the Unix-socket transport thread, and the inspector-spawn helper. Zero bundled game assets, zero knowledge of any concrete scene.
- `crates/engine/render` (package `engine-render`, lib) — the braille rendering primitives (today's `render` crate, relocated and stripped of bundled game content). Depends on `engine-core` for `Rgba`/`Inspectable`, same as `render` depends on `scene-core` today.
- `crates/engine/derive` (package `engine-derive`, proc-macro) — the `#[derive(Inspectable)]` macro, renamed from `scene-core-derive`, mechanically identical apart from its emitted crate path (`::scene_core::` → `::engine_core::`).
- `crates/engine/inspector` (package `inspector`, bin) — unchanged behavior, egui app, relocated under `engine/` and now depends on `engine-core` instead of `scene-core`. **Confirmed (not braille): the inspector stays a native egui window; "braille everywhere" applies to games built on the engine, not to the engine's own editor tooling.**
- `crates/game` (package `game`, bin) — this game's concrete `SceneId` enum, its 4 implemented scenes + registry (`SceneCatalog` impl), creature roster + bundled art, UI skin assets, digit-hotkey policy, `main.rs`/`cli.rs`. Depends on `engine-core` + `engine-render`.

This is a **relocation and rename**, not a merge: `engine-core` and `engine-render` remain two separate Cargo packages with their own `Cargo.toml`/`lib.rs`, each internally organized into modules (as they already reasonably are) — nothing from `render` is folded into `scene-core`'s file tree or vice versa. `crates/engine/` is a plain directory grouping four sibling crates, not itself a Cargo package.

Also in scope: documentation that makes the boundary durable — at minimum the project-root `CLAUDE.md` and `README.md` (Phase G); how much further to go (per-crate docs, a directory-level README) is an implementation call, not a fixed checklist.

Out of scope: any change to the M1/M2 wire protocol shape, any change to what the inspector or game can do, any new features. This is a pure crate-boundary move plus the minimum type change (`SceneId` → `SceneKey`) needed to make it possible.

## Decisions (v1)
Settled with the project owner:

1. **Scene identity on the engine side is an opaque string key, not a closed enum.**
   ```rust
   // engine_core::scene::key
   #[derive(Debug, Clone, PartialEq, Eq, Hash)]
   pub struct SceneKey(String);

   impl SceneKey {
       pub fn new(s: impl Into<String>) -> Self { Self(s.into()) }
       pub fn as_str(&self) -> &str { &self.0 }
   }
   // Serialize/Deserialize as a bare string — byte-identical wire shape to
   // today's SceneId (which already serializes via wire_name()), so the JSON
   // envelope format does not change at all.
   ```
   `game`'s existing closed `SceneId` enum (`all()`, `wire_name()`, `display_name()`, `from_wire()`) moves into `game` unchanged and gains `impl From<SceneId> for SceneKey` + `impl SceneId { fn from_key(k: &SceneKey) -> Option<Self> }`. Game keeps full compile-time exhaustiveness (adding a scene still requires touching a real `match`); `engine-core` gains zero knowledge of what scenes exist. No generics thread through `Scene`/`SceneManager`/`Message` — everything engine-side is concrete over `SceneKey`.

2. **Scene construction is dispatched through a game-supplied `SceneCatalog` trait object**, not a free function the engine calls into:
   ```rust
   // engine_core::scene::catalog
   pub trait SceneCatalog: Send {
       fn construct(&self, key: &SceneKey) -> Box<dyn Scene>;
       fn schema_for(&self, key: &SceneKey) -> FieldSchema;
       fn display_name(&self, key: &SceneKey) -> &str;
       /// Ordered catalog for Hello — replaces today's hardcoded '1'..'4' scan.
       fn catalog_keys(&self) -> Vec<SceneKey>;
       /// Cheap availability check (today's `registry::is_implemented`) —
       /// separate from `construct` so checking doesn't require building
       /// (and discarding) a scene with real `enter()` side effects.
       fn is_available(&self, key: &SceneKey) -> bool;
   }
   ```
   `SceneManager::new`/`with_scene` take `Box<dyn SceneCatalog>`. `game::registry` becomes the (sole) implementor — same panic-for-unbuilt-but-cataloged behavior as today's `unimplemented!()`, just behind the trait instead of a free function `match`.

3. **Digit-hotkey scene switching (`'1'`–`'9'`) moves entirely out of the engine.** `SceneManager::route_key` keeps only the universal quit keys (`q`, Ctrl-C) and forwarding to the active scene's `handle_input`; the digit branch is deleted. `game::app`'s input loop intercepts digit keys *before* calling `route_key`, maps them to a `SceneKey` via its own table, and calls the already-public `SceneManager::set_gameplay_transition` directly. A future game that doesn't want digit hotkeys (or wants a different scheme) needs to touch zero engine code.

4. **The engine ships zero bundled game content.** The 6 creature GIFs + `creature.rs`/`creature/bundled.rs`, and `assets.rs`'s logo/panel/icon PNGs (plus their dimension-pinned tests), all move to `game`. `Creature`/`AnimatedSprite`/`AnimationKind`/`Button`/`FrameButton` stay in `engine-render` as generic types — they take asset bytes as constructor parameters instead of reading `crate::assets::*` constants internally. `engine-render`'s own unit tests keep using synthetic in-memory buffers/pixel arrays (already the dominant pattern in `render`'s existing test suite — see `crates/render/src/lib.rs`'s `make_buf` helpers), not real game art.

## Target Crate Layout

```
crates/
  engine/                        (plain directory — not a Cargo package)
    README.md                    "these four sub-crates together are the engine"   [new]
    core/                        package: engine-core (lib)
      src/
        color.rs                  Rgba                              [from scene-core]
        inspectable.rs            Inspectable, FieldSchema, FieldTag, patch parsing [from scene-core/inspect.rs]
        ipc.rs                    Envelope, Message, framing — SceneKey-typed       [from scene-core/ipc.rs]
        scene/
          mod.rs                  Scene, EngineCtx, Transition, InputEvent, NoInspect [from game/scene.rs]
          key.rs                  SceneKey                                          [new]
          catalog.rs              SceneCatalog trait                                [new]
          manager.rs              SceneManager (route_key stripped of digit hotkeys)[from game/manager.rs]
        net/
          ipc_server.rs           Unix-socket transport thread, Event/IpcHandle     [from game/ipc_server.rs]
          inspect.rs              --inspect flag, sibling-binary discovery, spawn   [from game/inspect.rs]
    render/                       package: engine-render (lib)
      src/
        dots.rs, grid.rs, composite.rs, convert.rs, camera.rs,
        transform.rs, tween.rs, screen_layout.rs, button.rs, anim.rs
                                   (anim.rs is the sole home of AnimatedSprite;
                                    Creature/AnimationKind later moved to
                                    `game::creatures`, see `34-creature-attributes-data-model`) [from render/*, minus bundled assets]
    derive/                       package: engine-derive (proc-macro)               [renamed from scene-core-derive]
    inspector/                    package: inspector (bin), depends on engine-core  [relocated, import paths only]
  game/                           package: game (bin)
    src/
      scene_id.rs                 concrete SceneId enum + From<SceneId> for SceneKey [from scene-core/scene_id.rs, now game-owned]
      registry.rs                 SceneCatalog impl for GameCatalog                  [from game/registry.rs, retargeted]
      scenes/                     MainHub, BattleViewer, RosterManager, Leaderboard, … [unchanged]
      creatures.rs / creatures/   Creature/AnimationKind types + 6 bundled ctors + GIFs [from render/creature/bundled.rs + assets/creatures/*]
      assets.rs                   logo/panel/icon PNGs + dimension tests             [from render/assets.rs]
      app.rs                      terminal loop, digit-hotkey table, TerminalGuard   [from game/app.rs, gains digit-hotkey dispatch]
      cli.rs, main.rs             [unchanged]
```

Root `Cargo.toml` workspace `members` becomes `["crates/engine/core", "crates/engine/render", "crates/engine/derive", "crates/engine/inspector", "crates/game"]` — five crates (same count as today), regrouped under two directories so the two-product story (`engine/`, `game/`) is visible in the file tree, not just asserted in prose.

## Conversion Plan
Each phase is an independent, compiling, fully-tested checkpoint — `cargo build --workspace && cargo test --workspace` must pass at the end of every phase before starting the next, per this project's "specs must be complete, buildable units" rule applied at the phase level.

**Phase A — Introduce `SceneKey` and `SceneCatalog`, keep crate locations as-is.**
- Add `SceneKey` to `scene-core`. Add the `SceneCatalog` trait to `scene-core`.
- Change every `ipc.rs` message field from `SceneId` to `SceneKey`.
- `game::SceneId` gains `From<SceneId> for SceneKey` and `SceneId::from_key(&SceneKey) -> Option<Self>`.
- `game::registry` implements `SceneCatalog` for a new `GameCatalog` struct, wrapping today's `construct`/`schema_for`/`is_implemented` logic.
- `SceneManager` takes `Box<dyn SceneCatalog>` instead of calling `registry::construct` directly; `hello()` uses `catalog.catalog_keys()` instead of the hardcoded `['1','2','3','4']` scan.
- Update every `SceneId` reference across `scene-core`/`game`/`inspector`/examples-tests to `SceneKey` (or to game's now-internal `SceneId` where the code is genuinely game-specific, e.g. inside `scenes/*.rs`) — re-grep at the start of this phase for the actual current file/reference count rather than trusting any number from this spec's drafting time.
- `scene-core`'s own re-export of `SceneId` is deleted; `SceneId` becomes a `game`-only type for the rest of the conversion.
- This is the largest single mechanical phase — do it in one atomic pass, not interleaved with later phases, so no test breaks twice.

**Phase B — Move engine-loop code out of `game`, into `scene-core` (not `render`).**
- `game/src/scene.rs`, `game/src/manager.rs`, `game/src/ipc_server.rs`, `game/src/inspect.rs` import only `scene_core`, `ratatui`, `crossterm`, `serde_json` — never `render` — so they move directly into `scene-core` (no intermediate detour), landing at the module paths shown in the Target Crate Layout tree (`scene/mod.rs`, `scene/manager.rs`, `net/ipc_server.rs`, `net/inspect.rs`).
- Strip the digit-hotkey branch out of `SceneManager::route_key`; add the digit→`SceneKey` dispatch table to `game::app`'s input loop, calling `SceneManager::set_gameplay_transition` directly (already `pub`).
- `game` keeps only: `scenes/*.rs`, `registry.rs` (now implementing `SceneCatalog`), `cli.rs`, `app.rs`, `main.rs`.

**Phase C — Relocate and rename crates. No file-tree merging.**
- Move `crates/scene-core` → `crates/engine/core`, renaming the package `scene-core` → `engine-core`.
- Move `crates/scene-core-derive` → `crates/engine/derive`, renaming the package → `engine-derive`; update every `::scene_core::` path emitted by the macro to `::engine_core::` (grep-confirm zero `::scene_core::` remain in the crate before considering this step done — don't rely on a pre-counted occurrence total).
- Move `crates/render` → `crates/engine/render`, renaming the package `render` → `engine-render`; update its `scene_core::` imports to `engine_core::`.
- Move `crates/inspector` → `crates/engine/inspector` (package name stays `inspector`); update its `scene_core::` imports to `engine_core::`.
- `engine-core` and `engine-render` remain two distinct crates throughout — nothing from one's `src/` tree moves into the other's. Each keeps (and where helpful, tidies) its own existing module structure.
- Update root `Cargo.toml` workspace `members` and every path dependency across the workspace to the new locations/names.

**Phase D — Strip game content out of `engine-render`.**
- Move `creature/bundled.rs` + `assets/creatures/*.gif` out of `engine-render`'s `creature` module into `game` (as `game::creatures`), keeping `AnimatedSprite` in `engine-render`. (`Creature`/`AnimationKind` themselves later moved to `game::creatures` too — see `34-creature-attributes-data-model`.)
- Move `engine-render`'s `assets` module (logo/panel/icon PNGs + their dimension-pinned tests) into `game::assets`.
- Update `Button`/`FrameButton` (and any other call site that referenced `crate::assets::*` constants directly) to take asset byte slices as constructor parameters; `game` passes its own bundled bytes in.
- Confirm via `grep -rn "include_bytes!" crates/engine/` that no crate under `crates/engine/` has any remaining bundled binary content.

**Phase E — Final wiring in `game`.**
- `game::registry::GameCatalog` is the sole `SceneCatalog` impl; `game::scene_id::SceneId` is the sole closed scene enum in the workspace.
- `game::app`'s digit-hotkey table is the sole place mapping keys to scenes.
- Re-run this game's existing end-to-end tests (`game/tests/switch_e2e.rs`, `game/tests/inspector_apply_e2e.rs`) unmodified in behavior — they should still pass without their assertions changing, proving the wire contract didn't shift.

**Phase F — Inspector.**
- Confirm `crates/engine/inspector/src/{app,client}.rs` compiles cleanly against `engine_core::` (moved/renamed in Phase C); `SceneId` usages become `SceneKey` (HashMap keys, dropdown state) — expected to be close to mechanical since the inspector never pattern-matched on specific variants.
- Manually re-verify the spec-14 M1 test plan end to end: launch `game --inspect`, switch scenes from the dropdown, confirm the game screen changes and the dropdown reflects gameplay-driven switches — i.e., the sync link between inspector and game is unaffected by the crate split.
- Explicitly confirm `game`'s sibling-binary discovery (`CLAUDE.md`'s documented gotcha: `game` finds `inspector` via `current_exe().parent()/"inspector"`) still works after `inspector`'s source moves under `crates/engine/inspector/` — Cargo's flat per-profile `target/<profile>/` output layout shouldn't change regardless of source directory, but this is exactly the kind of build-topology assumption worth verifying directly rather than assuming, given it's a recurring gotcha in this repo.

**Phase G — Documentation: make the boundary explicit and durable.**
This is not cleanup after the fact — it's the mechanism that keeps the split from eroding the next time a feature gets bolted on under time pressure. Documenting the boundary is in scope; the exact file list and wording below is a starting sketch, not a checklist to satisfy literally — research/implementation should right-size it (fewer, more load-bearing docs are better than many thin ones) rather than mechanically produce one file per bullet.

- **Project-root `CLAUDE.md`**: add an "Engine / Game Boundary" section (peer to the existing "Key Constraints" section) covering, at minimum:
  - This is a two-product workspace: everything under `crates/engine/` is reusable by any future game; everything under `crates/game/` is this game's content only.
  - Rule of thumb: if a change would still make sense for a hypothetical different game built on this engine, it belongs under `crates/engine/`; if it only makes sense for Agent Battleground specifically, it belongs in `crates/game/`.
  - Hard invariants: no crate under `crates/engine/` contains `include_bytes!`-bundled game art, a closed enum of concrete scenes/creatures/pieces, or a path dependency on `crates/game`.
  - When it's unclear which crate something belongs in, ask the project owner — don't guess and place it wherever compiles.
- **Project-root `README.md`**: add an "Architecture" subsection (peer to "Tech Stack") documenting the crate layout and the same boundary rule; add this spec (`31`) to the specs table.
- Per-crate documentation (a `crates/engine/README.md` at the directory level, plus something in each of the five crates establishing what it is / what may depend on it / what it must never depend on) is worth having, but the split between root docs and per-crate docs, and how much belongs in each, is an implementation call — don't treat the specific file list above as mandatory line items to check off.

## Test Plan
- After every phase: `cargo build --workspace` and `cargo test --workspace` both green.
- After Phase D: `grep -rn "include_bytes!" crates/engine/` returns nothing.
- After Phase F: manual run of spec-14's M1 test plan (`Hello` on connect, `SwitchScene` → `SceneChanged`, gameplay-driven switch reflected in the inspector) — this is the concrete proof that "switch scenes in the inspector, and in the game, and they stay synced" still holds, since the wire format is byte-identical to before this spec.
- After Phase F: run `game --inspect` and confirm it actually spawns and connects to the sibling `inspector` binary (not just that both binaries individually build) — the concrete proof the sibling-discovery gotcha survived the move.
- Spot-check: `cargo doc -p engine-core -p engine-render --no-deps` builds with no game-specific type leaking into either crate's public API surface (manual scan of the generated docs' item list).
- After Phase G: project-root `CLAUDE.md` and `README.md` both state the engine/game boundary rule; the specs table in `README.md` includes this spec; the boundary is documented somewhere durable enough that a future contributor would find it before misplacing a new module — exact file count not prescribed.

## Risks / Concerns
- **Phase A is genuinely large** (a couple dozen files' worth of `SceneId` references, workspace-wide) — mechanical but not risk-free; doing it as one atomic commit rather than spread across phases avoids double-breaking tests.
- **The `engine-derive` proc-macro rename is a hard dependency of every later phase** — nothing else compiles until its emitted paths are updated, since `#[derive(Inspectable)]` is used throughout `scene-core`'s own types, `render`'s `transform.rs`/`camera.rs`, and every game scene.
- **`SceneCatalog::construct` still panics for a cataloged-but-unbuilt scene** (mirroring today's `unimplemented!()`), guarded by `is_available` upstream exactly as `registry::is_implemented` guards `construct` today — this spec preserves that behavior rather than changing it to a `Result`/`Option` return, to keep the phase-A diff minimal. A follow-up spec could revisit this if it's ever a real papercut.
- **No wire format changes** — `SceneKey` serializes identically to how `SceneId` already did (`wire_name()` string), so there is no protocol version bump and no risk to the inspector/game sync contract this spec is explicitly protecting.
- **Directory moves (Phase C) are still real diffs even though no logic changes** — `git mv` per crate directory, then a mechanical path/name fixup pass, keeps history traceable; a wholesale delete-and-recreate would lose blame history for no reason.

## Dependencies
- `14-scene-architecture.md` ✅ — defines `Scene`, `SceneManager`, `SceneId`/registry, and the IPC envelope this spec relocates and retypes without changing behavior.
- `15-debug-inspector.md` ✅ — `Inspectable`/`FieldSchema`/patch machinery relocates from `scene-core` into `engine-core` unchanged.
- `13-rendering.md` ✅, `22-braille-ui-chrome.md` ✅ — `render`'s contents relocate into `engine-render` (`crates/engine/render`); `Button`/`FrameButton`'s asset-parameter change (Decision 4) touches these.
- `17-creature-art-asset-pipeline.md` — the 6 bundled creatures this spec relocates from `render` to `game` are that spec's output.
- Every future non-Agent-Battleground game this engine might host — the entire point of this spec.
