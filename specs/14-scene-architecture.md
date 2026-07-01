> # ✅ DONE! — Completed 2026-07-01

# Scene Architecture & Debug Scene Switcher

> **Status: implemented (M1 — scene switching, built & validated).** Foundational architecture spec. Defines what a "scene" is in the engine, how scenes are registered and switched in memory, the debug IPC channel, and the debug **Scene Switcher** that drives scene transitions from the external inspector.
>
> **Milestone scope.** This spec covers **M1: scene switching only** — list scenes, pick one, switch the running game to it, see the screen change. **Editing a scene's field values (the inspector form, `ApplyState`, schema-driven widgets) is M2 and lives in `15-debug-inspector`.** Where M2 shares a one-way-door decision with M1 (notably the IPC envelope), it is settled here; everything else editing-related is deferred to spec 15 and flagged for confirmation.

## Purpose
Establish the runtime structure the whole client is built on: a set of full-screen game modes ("scenes"), exactly one active at a time, switchable at runtime. The same structure that powers normal in-game navigation is the structure the debug tooling drives. M1 delivers the scene model, the registry, the in-memory `SceneManager`, the IPC channel, the debug Scene Switcher, and four example scenes used to prove switching works end-to-end.

## Scope (M1)
- The `Scene` concept and trait (engine-side contract every scene implements)
- `SceneId` and the scene registry (the source of truth the switcher reads)
- The `SceneManager` (owns the active scene, processes transitions in memory)
- The main loop model (tick rate, command draining, transition processing)
- The debug IPC channel: Unix domain socket, framing, **envelope**, and the M1 message subset
- The debug Scene Switcher: catalog → dropdown → "Go" → transition
- A minimal renderer path (solid braille fill + label) so example scenes can draw
- Four example scenes used to validate switching end-to-end

**Deferred to `15-debug-inspector` (M2):** the `Inspectable` derive, per-scene field schema, field widgets, `ApplyState`/`StateSnapshot`/`Subscribe`/`RequestSnapshot` messages, validation policy, live value sync. The trait reserves a hook for it; the envelope reserves room for it.

Out of scope entirely: the full image/sprite renderer (`13-rendering`), the real content of each game scene (`01`–`09`).

---

## Resolved Decisions
Settled with the project owner:

- **One active scene at a time.** Overlays (e.g. the Battle Viewer tutorial layer) are a future layering extension, not part of the base switcher.
- **Continuous main loop, fixed timestep, 30 FPS.** Scenes receive a real `dt`. (Animation and live updates depend on this; not event-driven redraw.)
- **Fresh-construct on switch.** Switching to a scene builds a new instance (state resets). Scene caching is a later optimization; noted where MainHub/BattleViewer will want it.
- **Transition precedence.** Transitions are processed only at top-of-frame. A debug `SwitchScene` overrides a gameplay-requested transition pending the same frame. `enter()` may not request a transition (a scene returns one from `update`/`handle_input`).
- **`EngineCtx` services.** Scenes get `{ renderer, clock, input, rng }`. Scenes do **not** get the IPC handle — the main loop mediates all debug commands.
- **Linux only for now.** Transport is a Unix domain socket. A portable abstraction (Windows named pipes) is deferred.
- **Debug builds only.** The IPC socket is compiled in and opened only under `--inspect` on a debug build. Release builds contain no inspector surface.
- **Canonical color type is `Rgba`** (`u8` r/g/b/a), wire-encoded as `"#rrggbbaa"`. Alpha is carried even though a solid fill is opaque, so the type is shared with the compositing renderer (`13-rendering`).

---

## Key Details

### What a Scene Is
A **scene** is a self-contained, full-screen game mode that owns its own state, update logic, input handling, and rendering. The scene set maps directly to the existing game segments:

| SceneId | Spec |
|---|---|
| `Onboarding` | 01 |
| `MainHub` | 02 |
| `ArmyEditor` | 03 |
| `Matchmaking` | 04 |
| `BattleViewer` | 05 |
| `PostBattle` | 06 |
| `ReplayBrowser` | 07 |
| `Leaderboard` | 08 |
| `Settings` | 09 |

Exactly one scene is **active** (updated + rendered + receiving input) at any time. Switching scenes is the fundamental navigation primitive: normal gameplay switches scenes (hub → battle viewer), and the debug switcher switches scenes the same way — it is a debug *trigger* on top of the real transition path, not a parallel system.

### The `Scene` Trait
Every scene implements one trait. Illustrative shape:

```rust
pub trait Scene {
    fn id(&self) -> SceneId;

    /// Called once when this scene becomes active. `params` is an optional
    /// JSON blob (e.g. which replay to open) supplied by the transition.
    fn enter(&mut self, ctx: &mut EngineCtx, params: Option<JsonValue>);

    /// Per-frame logic. `dt` is the frame delta. May request a transition.
    fn update(&mut self, ctx: &mut EngineCtx, dt: Duration) -> Option<Transition>;

    /// Draw via the renderer into the given area.
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Handle a single input event. May request a transition.
    fn handle_input(&mut self, ev: InputEvent) -> Option<Transition>;

    /// Called once when this scene is being torn down.
    fn exit(&mut self, ctx: &mut EngineCtx);

    // --- M2 hook (see spec 15) ---
    // fn inspect(&mut self) -> &mut dyn Inspectable;
}
```

`Transition { target: SceneId, params: Option<JsonValue> }` is the request to switch. A scene returns it from `update`/`handle_input` to navigate; the debug channel injects the same `Transition` to force a switch. The `inspect()` method is reserved for M2 and intentionally omitted from M1.

### `SceneId` and the Registry
`SceneId` is a closed enum (scenes are code, so adding one is a recompile — acceptable). On the wire, ids are the variant **string** name (`"BattleViewer"`). The **registry** maps each id to a constructor and a human-readable display name:

```rust
pub enum SceneId { Onboarding, MainHub, ArmyEditor, /* … */ Settings }

impl SceneId {
    pub fn all() -> &'static [SceneId];       // enumerates the catalog
    pub fn display_name(self) -> &'static str;
    pub fn wire_name(self) -> &'static str;   // string id for IPC
    pub fn construct(self) -> Box<dyn Scene>; // fresh instance
}
```

The registry is the **single source of truth** for "what scenes exist." The Scene Switcher's dropdown is generated from `SceneId::all()` — never hand-maintained. `SceneId` lives in the shared `scene-core` crate so the inspector knows the same catalog at compile time; the game additionally sends the live catalog on connect so the inspector reflects the actual running build.

### `SceneManager` and the Main Loop
The `SceneManager` owns the single active scene and processes transitions in memory:

```rust
pub struct SceneManager {
    active: Box<dyn Scene>,
    pending: Option<Transition>,
}
```

Per frame, at a fixed 30 FPS timestep, the main loop:
1. Drains the inbound debug-command channel (a `SwitchScene` sets `pending`, overriding any prior).
2. Polls input → `active.handle_input(ev)` (may set `pending`).
3. `active.update(ctx, dt)` (may set `pending` only if not already set by debug — debug wins).
4. If `pending`: `active.exit()` → `target.construct()` → `enter(params)` → swap `active` → emit `SceneChanged`.
5. `active.render(frame, area)`.

All scene state lives in memory, owned by `SceneManager`; switches do no disk I/O and are O(1) (construct + `enter`).

### IPC Channel
**Transport.** A Unix domain socket. The **game is the server** (binds + listens); the **inspector is the client** (connects). Created only under `--inspect` on a debug build.

- Path: `$XDG_RUNTIME_DIR/agent-battleground/inspect-<pid>.sock` (fallback `/tmp/agent-battleground-inspect-<pid>.sock`), printed on startup and passed to the spawned inspector.
- Permissions: `0600`. Removed on clean exit.
- **Single client.** At most one inspector connected at a time; a second concurrent connection is refused with an `Error`.
- **Spawn-only launch.** The game spawns the inspector and hands it the path. (Connecting an inspector to a *different*, already-running game is deferred — the path is per-pid.)
- **Reconnect to a live game.** If the inspector drops while the game is still running, the game continues headless and a newly launched inspector can connect to the same per-pid socket; the game resends `Hello` (stateless — no buffered or replayed history).
- **Game exit closes the inspector.** The socket is per-pid, so when the game process exits there is nothing to reconnect to: the connected inspector gets EOF and exits. (A stable, discoverable socket path that survives game restarts — letting the inspector persist and re-attach across launches — is a future item.)

**Framing.** Length-prefixed JSON: a 4-byte little-endian unsigned length, then that many bytes of UTF-8 JSON = one envelope. Binary-safe, newline-agnostic, human-readable. Max framed size **16 MB**; larger is a `BadFrame` error.

**Threading.** A dedicated **IPC thread** owns the socket and blocks on it. It bridges to the main loop with two lock-free channels:
- Inbound `Command` (`mpsc` → main loop).
- Outbound `Event` (main loop → IPC thread → socket).

The main loop only touches the channels; the socket never blocks rendering. Inbound commands are drained at top-of-frame (step 1 above).

**Envelope** (shared by M1 and M2 — this is the one-way-door decision settled here):

```json
{ "type": "SwitchScene", "seq": 12, "reply_to": null, "payload": { … } }
```

- `seq` — monotonic per sender.
- `reply_to` — for a reply (`Ack`/`Error`), the originating command's `seq`; for an unsolicited push (`Hello`, gameplay-driven `SceneChanged`), `null`.

**M1 message set:**

Game → Inspector:
- `Hello` — `{ "scenes": [ { "id": "BattleViewer", "name": "Battle Viewer" }, … ], "active": "MainHub" }`. **The handshake**: sent by the game immediately when an inspector connects (and resent on every reconnect). It is the inspector's initial state dump — the full scene catalog (so the switcher dropdown is populated, not hardcoded) and which scene is currently active (so the dropdown shows the right selection). The inspector renders nothing scene-specific until it receives this. *(M2 adds a per-scene `schema` field carrying each scene's editable fields.)*
- `SceneChanged` — `{ "id": "BattleViewer" }`. Sent after any switch, debug- or gameplay-driven. *(M2 adds the new scene's `snapshot`.)*
- `Ack` / `Error` — `Error` carries `{ "code": "...", "message": "..." }`.

Inspector → Game:
- `SwitchScene` — `{ "target": "BattleViewer", "params": null }`. `params` is reserved now (present, optional) so adding scene-entry arguments later needs no envelope change.

**Error codes (M1):** `BadFrame`, `UnknownType`, `UnknownScene`. *(M2 adds `NotActive`, `BadField`.)*

**Flow (M1):** inspector connects → `Hello` → populate dropdown + show active scene → user picks a scene, clicks **Go** → `SwitchScene` → game switches → `SceneChanged` (+ `Ack`) → inspector updates the selection. The screen visibly changes.

### Minimal Renderer Path (for Example Scenes)
The full renderer is `13-rendering`. M1 needs only:
- **`fill(area, rgba)`** — fill an area with fully-lit braille cells (`U+28FF`) at a 24-bit foreground (alpha opaque for a fill). Reuses the cell technique validated in `ascii_test/downrez.rs` (flat color → fully-lit cells), bypassing image conversion.
- A **centered text label** (plain ratatui text, not braille) drawn over the fill, showing the scene's display name — so a switch is unmistakable on screen.

This keeps example scenes honest to the braille style without depending on the asset pipeline. Lives in a `render` crate (future home of spec 13), minimal for now.

### Example Scenes
Four minimal scenes prove switching. Each renders a full-screen solid braille fill plus its name label:

| Scene | Fill color |
|---|---|
| `MainHub` | deep blue |
| `BattleViewer` | crimson red |
| `ArmyEditor` | green |
| `Leaderboard` | amber |

These are stand-ins for the real scenes (`02`, `05`, `03`, `08`) so names and ids are real. Switching between them must visibly recolor the screen and change the label. (Editable per-scene fields are added in M2/spec 15; M1 scenes hold only their fixed color + name.)

### Crate Layout
- `scene-core` (shared): `SceneId`, registry, `Scene` trait, IPC envelope/message types. Depended on by **both** game and inspector so neither drifts. *(M2 adds the `Inspectable` trait + derive here.)*
- `render`: the braille renderer (M1: `fill` + label; grows into spec 13).
- `game`: engine, `SceneManager`, main loop, concrete scenes, IPC server thread.
- `inspector`: the egui app (spec 15), IPC client.

`ascii_test/` stays a separate scratch prototype; code is ported into `render` as needed.

### Launch Model
`game --inspect`:
1. Binds the Unix socket and starts the IPC thread.
2. Spawns the inspector process (sibling binary in the same target dir), passing the socket path.
3. Runs normally; the inspector connects and the channel goes live.

Rejected (or no-op with a warning) on release builds. The game owns the terminal (ratatui alt-screen); the inspector is its own OS window (separate process). They never share a terminal.

### Performance Notes
- Scene state owned in memory; switches do no disk I/O.
- IPC off-thread; the render loop only drains lock-free channels.
- M1 traffic is tiny (catalog + switch commands).

### Security Notes
The socket exists only in debug builds under `--inspect`, adding **no attack surface to shipped builds** — consistent with the project's hard sandboxing constraint. The socket carries no untrusted input; it is a local developer channel (`0600`, per-pid path).

---

## Test Plan (M1)
1. Launch `game --inspect`; inspector spawns and connects; `Hello` lists the four example scenes.
2. Select `BattleViewer`, click **Go** → screen turns crimson, label reads "Battle Viewer"; inspector selection updates from `SceneChanged`.
3. Switch to `MainHub` → screen turns blue, label updates.
4. Trigger a gameplay-driven switch in the game → inspector receives an unsolicited `SceneChanged` and updates its selection.
5. **Automated:** a headless socket test client sends `SwitchScene` + reads `SceneChanged`/`Ack` and asserts the returned `active`/`id` — no GUI required.

---

## Open Questions — confirm before build
Defaults shown; flag any to flip. **Switcher-domain (could affect M1 code):**
- **Envelope shape** (above): `seq` + `reply_to`, 16 MB cap, listed error codes — confirm, since M2 inherits it.
- **`params` on `SwitchScene`** reserved-but-unused in M1 — confirm we want it present now.
- **Workspace layout** (`scene-core` / `render` / `game` / `inspector`) and inspector-as-sibling-binary discovery — confirm.
- **Scene caching:** fresh-construct for v1; does any M1 example scene need warm state across switches? (Default: no.)

**Deferred to `15-debug-inspector` (M2) — listed here for traceability, not decided in this spec:**
- `Inspectable` derive + field schema; canonical patch-path grammar (`a.b[2].c`); nesting depth (structs + read-only `Vec`; maps/`Option` deferred).
- Validation policy on apply (default: per-field best-effort — clamp numerics to range, skip type-mismatches, `Error` the rejected paths; valid fields still apply).
- Live value sync cadence and backpressure (default: coalesce to latest, ≤10 Hz, never drop `Ack`/`Error`).
- Edit target = active scene only; pre-staging via `SwitchScene.params`.
- Enum support (C-like only for v1); data-carrying enums → read-only JSON.

## Dependencies
- `13-rendering` — provides the real renderer; M1 uses only a `fill` + label subset.
- `12-data-model-sync` — scene state mirroring persistent data must stay consistent with the data model (not exercised by example scenes).
- Consumed by `15-debug-inspector` — the inspector GUI is the client of the catalog and IPC envelope defined here; M2 extends both.
- Every game-segment spec (`01`–`09`) eventually implements `Scene`.
