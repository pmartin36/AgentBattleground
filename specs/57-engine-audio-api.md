# Engine Audio API

## Purpose
The game has zero audio infrastructure today — no sound library, no playback, nothing. A terminal UI constrains only *visual* output; the process is an ordinary native binary and can open the system audio device like any other. This spec adds a durable, content-agnostic audio subsystem at the engine layer: looping background music, concurrent one-shot sound effects, a small mixer (Music/SFX buses with volume + mute), and smooth fades — everything an atmospheric game needs on day one.

This is squarely a cross-cutting mechanism per CLAUDE.md's engine/game boundary rule: any future game built on this engine will want the same thing, and every caller (present and future) should be able to make a sound without wiring up the subsystem itself. Real-time effects beyond fades (pitch bending, filter sweeps, reverb/delay, spatial panning, random-SFX variation, disk streaming, adaptive music) are deliberately deferred — see `needs-research/audio-v2`.

## Scope
- **Engine** (`crates/engine/audio`, new crate `engine-audio`): a [`kira`](https://crates.io/crates/kira)-backed playback subsystem exposed as a **process-global singleton** — free functions over a `OnceLock` mixer. Covers device init with graceful silent fallback, a Music bus and an SFX bus under a master track, looping music with arbitrary loop points, fire-and-forget concurrent SFX, per-bus + master volume, mute, and linear fades. Includes a decode-once PCM cache keyed on the source bytes' pointer, mirroring `engine-render`'s `asset_cache`.
- **Engine** (`crates/engine/audio/Cargo.toml`, workspace `Cargo.toml`): new `kira` dependency (default `cpal` backend); new workspace member `crates/engine/audio`.
- **Game** (`crates/game/src/sounds.rs`, new): sound files as `&'static [u8]` `include_bytes!` consts — the audio parallel of `game::assets` art consts. The bytes const *is* the sound's identity; there is no handle/ID/enum type.
- **Game** (`crates/game/src/main.rs`): calls `engine_audio::init()` once at startup (right after `engine_core::logging::init`), before the alternate screen is entered.
- **Game** (`crates/game/src/scenes/main_hub.rs`): the audible proof, wired into the Main Hub — `UI_CONFIRM` plays on the SFX bus inside `MainHub::handle_input()` on cursor Up/Down and on Enter/Space select. `handle_input` receives no `EngineCtx`, which is exactly why the global-singleton access pattern (Decision 1) is required. This proves the pipeline end-to-end with audible output — not just a headless test. (`play_music` is part of the engine-audio API and is exercised by the crate's own tests; no scene currently plays background music.)

Out of scope (deferred to `needs-research/audio-v2`, confirmed with the project owner):
- **Real-time pitch/playback-rate bending** — kira supports it (`set_playback_rate`, tweenable), but it's a v2 expressive feature, not a mid-scope need. `SoundHandle` in v1 exposes only `stop` and `set_volume`.
- **Filters / EQ / reverb / delay / distortion / compressor** — all built into kira as per-track effects; v1 ships no effect chain on the buses.
- **Spatial audio and stereo panning** — the battlefield could later position sound in space; not v1.
- **Random-SFX variation helpers** (pitch/volume jitter, clip pools to avoid repetition) — a small game-side helper on top of the v1 API; deferred so v1 stays a clean primitive.
- **Disk streaming** (`StreamingSoundData`). V1 loads every sound fully into memory as `StaticSoundData` — bundled `include_bytes!` bytes are already in memory, so streaming buys nothing until sounds come from disk. The static-vs-streaming split is a v2 concern.
- **Persisted volume/mute.** V1 holds master/bus volume and mute as in-memory runtime state only. The persisted, user-facing control is owned by `09-settings-model-config`, which has already been updated (in this same change, as a documentation cross-reference — not a code task) to reference this audio subsystem; no config-file layer exists in the codebase yet regardless.
- **Tying audio to battle event playback** (`20-battle-viewer-event-playback`) — "which sound plays on which battle event" is game content authored later against this API, not part of the primitive.

## Decisions (v1)

### 1. New `engine-audio` crate, on `kira`, exposed as a process-global singleton
A whole rendering subsystem (with its own external dep `image`) got its own crate `engine-render` rather than becoming a module of `engine-core`. Audio is the direct parallel: a distinct subsystem with an external dep (`kira`/`cpal`). So it is a new crate `crates/engine/audio`, added to the workspace `members`, sitting beside `engine-render` — not a submodule of core.

`kira` (chosen over `rodio`) because it is purpose-built "audio for games": it gives seamless music **loop regions** (an intro that plays once, then a body that loops), a real **mixer with sub-tracks/buses**, and **tweenable** volume/fades — exactly this spec's mid-scope surface, and the growth path for v2's effects — all built in rather than hand-rolled. Pin the version (`kira` is pre-1.0; minor bumps have been breaking).

The subsystem is reached as **free functions over a `OnceLock` global**, the same way `engine_render::asset_cache::decoded()` is reached today. Reasoning, not analogy: audio and assets are the two "callable-from-anywhere content subsystems," and keying the choice to the existing `asset_cache` precedent keeps them consistent. The global also (a) structurally dissolves kira's "the stream stops if its owner drops" footgun — a `static` never drops, so the device stream lives for the whole process with no handle to thread — and (b) lets `Scene::handle_input`/`render` (which receive no `EngineCtx`) trigger a sound with zero plumbing, so a click-on-keypress needs no stash-and-replay dance. A thin `EngineCtx::audio()` accessor pointing at the same global can be added later if a scene wants explicit access; that is additive and not part of v1.

```rust
// crates/engine/audio/src/lib.rs — the entire v1 public surface
pub fn init();                                              // call once at startup; idempotent; degrades silently

pub fn play_sfx(sound: &'static [u8]) -> SoundHandle;       // fire-and-forget one-shot on the SFX bus
pub fn play_music(sound: &'static [u8], opts: MusicOpts) -> SoundHandle; // looping, on the Music bus
pub fn stop_music(fade: Fade);                             // fade the current music out

pub fn set_master_volume(amplitude: f32, fade: Fade);      // 0.0..=1.0
pub fn set_bus_volume(bus: Bus, amplitude: f32, fade: Fade);
pub fn set_muted(muted: bool);
pub fn is_muted() -> bool;

pub enum Bus { Music, Sfx }
```

### 2. Graceful silent fallback, baked into the backend — safe by default for every caller
Device init legitimately fails on headless CI, over SSH, or in a locked-down container. Audio is non-essential, so `init()` must never panic or propagate a hard error. The backend is an enum:

```rust
static BACKEND: OnceLock<Mutex<Backend>> = OnceLock::new();

enum Backend {
    Active(Active),  // kira AudioManager + master/Music/Sfx TrackHandles + current-music handle + mute/volume state
    Silent,          // device init failed; every op is a no-op that logs once at WARN
}
```

`init()` (and lazy first-use) attempts `AudioManager::new(...)`; on `Err` it installs `Backend::Silent` and every `play_*`/`set_*` becomes a safe no-op. Per CLAUDE.md's "safe by default for every current and future caller" rule, this lives **inside the subsystem**, not as a per-caller `if audio_available` check each call site must remember. Calling any API before `init()` lazily initializes the backend, so ordering mistakes degrade to silence, never a crash.

### 3. Sound assets are `&'static [u8]`, decoded once, cached on the source pointer
Sound files are `include_bytes!` consts in `crates/game/src/sounds.rs`, the audio twin of `game::assets`:

```rust
// crates/game/src/sounds.rs
pub const UI_CONFIRM: &[u8] = include_bytes!("sounds/ui_confirm.ogg");
```

`engine-audio` holds a decode-once cache mirroring `asset_cache.rs` exactly, including its load-bearing safety invariant — **the key is always `bytes.as_ptr() as usize`, never the decoded object's address** (a `'static` slice lives in rodata for the whole process and can't be freed/reused; a decoded heap allocation's address can be, which is why keying on it would be unsound):

```rust
static SOUND_CACHE: OnceLock<Mutex<HashMap<usize, StaticSoundData>>> = OnceLock::new();

fn sound_data(bytes: &'static [u8]) -> StaticSoundData {
    let key = bytes.as_ptr() as usize; // source-bytes pointer, the only safe identity
    // decode via StaticSoundData::from_cursor(...) once; clone on every subsequent call.
    // kira: cloning StaticSoundData shares the same sample buffer — no extra memory per replay.
}
```

Both SFX and music decode to `StaticSoundData`; firing the same SFX repeatedly clones a shared buffer (cheap). Formats: `.ogg`/`.wav`/`.flac`/`.mp3` decode out of the box via kira's default Symphonia features. Bundled first-party assets that fail to decode `panic` as an invariant (`.expect("bundled first-party sound must decode")`), matching how `asset_cache::decoded` treats bundled art.

### 4. Two buses under master; volume + mute in memory
The mixer is a master track with two sub-tracks: **Music** and **SFX**. `play_sfx` routes to the SFX bus, `play_music` to the Music bus, so the game can duck all SFX or all music at once with a single `set_bus_volume`. Master/bus volume are `0.0..=1.0` amplitudes at the API surface (converted to kira `Decibels`/`Volume` internally). `set_muted(true)` drops the master to silence via a tween and remembers the pre-mute level, restored on `set_muted(false)`. All of this is in-memory runtime state on `Active`; persistence is `09-settings-model-config`'s job (Decision 8).

### 5. Looping music with arbitrary loop points; one logical track at a time
`play_music` plays a `StaticSoundData` on the Music bus with a loop region. `MusicOpts::loop_region: None` loops the whole track; `Some(3.5..)` plays an intro once then seamlessly loops the body — kira's `loop_region` headline feature. Starting new music while music is already playing **crossfades**: the outgoing track fades out over `fade_in` while the new one fades in, so scene transitions don't hard-cut the score. `stop_music(fade)` fades the current track to silence.

```rust
pub struct MusicOpts {
    pub loop_region: Option<Range<f64>>, // seconds; None = loop whole track
    pub fade_in: Fade,
    pub volume: f32,                     // 0.0..=1.0, relative to the Music bus
}
```

### 6. One-shot SFX: fire-and-forget, concurrent, auto-mixed
`play_sfx(bytes)` plays a one-shot on the SFX bus and returns a `SoundHandle` the caller may ignore. Many may overlap; kira sums all live voices for us — we never hand-mix samples. The returned handle exposes only what mid scope needs:

```rust
pub struct SoundHandle { /* wraps the kira handle; no-op methods when Silent */ }
impl SoundHandle {
    pub fn stop(&mut self, fade: Fade);
    pub fn set_volume(&mut self, amplitude: f32, fade: Fade);
    // set_playback_rate (pitch), filters, panning → v2 (needs-research/audio-v2)
}
```

### 7. Fades are a first-class type; v1 tweens are linear
Every volume change and music transition takes a `Fade` so audio never hard-jumps unless asked. V1 uses linear tweens; kira's easing curves are exposed in v2.

```rust
pub struct Fade { pub dur: Duration }
impl Fade {
    pub const NONE: Fade = Fade { dur: Duration::ZERO };
    pub fn ms(ms: u64) -> Fade { Fade { dur: Duration::from_millis(ms) } }
}
```

### 8. Wiring, and the trusted-code / sandboxing boundary
`crates/game/src/main.rs` calls `engine_audio::init()` once, right after `engine_core::logging::init` and before the alternate screen — mirroring the existing startup order (log path is printed to stdout before raw mode). Init happens before raw mode specifically so any ALSA device-probe warnings cpal emits to stderr can't corrupt the first rendered frame.

"Reachable from anywhere" means **trusted engine/game Rust**, not untrusted input. Per Key Constraint 1, opponent skill files and the LLM battle path produce *data*, never executed code — so nothing untrusted can name, select, or inject a sound. Audio is driven exclusively by trusted scene/engine code; the spec pins that audio is never triggered by untrusted input.

## Where the code lives
| Decision | Crate | Files |
|---|---|---|
| 1. `engine-audio` crate, global singleton, public API | **Engine** | `crates/engine/audio/src/lib.rs` (new) |
| 1. `kira` dep + new workspace member | **Engine** | `crates/engine/audio/Cargo.toml` (new), `Cargo.toml` (workspace) |
| 2. Backend enum + graceful silent fallback | **Engine** | `crates/engine/audio/src/backend.rs` (new) |
| 3. Decode-once PCM cache (`bytes.as_ptr()` key) | **Engine** | `crates/engine/audio/src/cache.rs` (new) |
| 4–7. Buses, volume/mute, music loop/crossfade, SFX, fades | **Engine** | `crates/engine/audio/src/backend.rs`, `lib.rs` |
| 3. Sound-byte consts | **Game** | `crates/game/src/sounds.rs` (new), `crates/game/src/sounds/*.ogg` (new) |
| 8. `engine_audio::init()` at startup | **Game** | `crates/game/src/main.rs` |
| Scope. Click SFX in `handle_input` | **Game** | `crates/game/src/scenes/main_hub.rs` |

Hard invariants respected (per CLAUDE.md / `31-engine-game-crate-split`): `engine-audio` contains **no `include_bytes!`-bundled sound files**, **no closed enum of concrete sounds**, and **no path dependency on `crates/game`** — exactly the rules that keep `engine-render` content-free. The bytes, and the choice of which sound plays when, live in `crates/game`.

## Testing Guidance (headless, no audio device)
CI has no audio device, so the default test path exercises the `Silent` backend — which is itself the most important thing to prove safe:
- `init()` on a machine with no audio device returns normally and installs `Backend::Silent`; every subsequent `play_sfx`/`play_music`/`stop_music`/`set_*` is a no-op that does **not** panic.
- Calling `play_sfx` **before** `init()` lazily initializes and still never panics (ordering-independence).
- Decode cache: two `play_sfx` calls with the same `&'static [u8]` perform exactly **one** real decode — assert via a `#[cfg(test)]` decode-recompute counter sampled as a before/after delta (never an absolute — the cache is process-global and shared across concurrent tests), the established `asset_cache` pattern.
- `set_muted(true)` then `is_muted()` returns `true`; `set_muted(false)` restores the prior master amplitude (assert the stored pre-mute level round-trips).
- `MusicOpts { loop_region: Some(3.5..), .. }` constructs a kira loop region with the expected start (unit-level, no device).

**Verification requirements (beyond the automated gate — passing tests ≠ working software):**
- On a real machine with audio, manually confirm and record the in-app audible behavior: a one-shot SFX fires on a menu keypress with no perceptible latency, and several overlapping SFX mix without clipping/dropping. Not "done" until heard. (Music playback — seamless loop points and crossfade — is implemented in `engine-audio` and covered by the crate's headless unit tests; it is not wired into a scene, so it is not part of the in-app manual check.)
- Confirm the one wired-in scene sound is audible in the real app (`cargo run -p game`), not merely present in a test.

## Open Questions / TBDs
None outstanding for v1 — library (`kira`), access pattern (global singleton), scope (music + concurrent SFX + Music/SFX buses + volume/mute + linear fades), and in-memory (not persisted) volume were confirmed with the project owner before writing this spec. Retention of every deferred capability is captured in `needs-research/audio-v2`, not left implicit here. The initial set of actual sound-asset files is game content produced separately; this spec defines the pipeline and proves it with one real sound.

## Dependencies
- `31-engine-game-crate-split` ✅ — this spec's engine/game placement follows that split directly: the whole subsystem is a content-free engine crate; only the sound bytes and `main.rs` wiring are game-crate changes.
- `30-asset-decode-caching` ✅ / `32-static-asset-rasterization-caching` ✅ — the decode-once, pointer-keyed cache (Decision 3) mirrors `asset_cache.rs`, including its safety invariant.
- `43-engine-app-logging-and-panic-handling` ✅ — `init()` is wired at the same startup chokepoint in `main.rs`, right after `logging::init`; the "hold the resource for process life" constraint is the same one `LoggingHandle` documents (here dissolved by the `static`).
- `14-scene-architecture` ✅ — the access-pattern decision (global singleton vs `EngineCtx`-threaded) is made against this spec's `EngineCtx` and `Scene` trait; a future `EngineCtx::audio()` accessor would extend it additively.
- `09-settings-model-config` — owns the future persisted, user-facing master-volume / mute control that this spec surfaces only in memory; that spec has already been updated (a documentation cross-reference, not part of this code pipeline) to reference the audio subsystem.
