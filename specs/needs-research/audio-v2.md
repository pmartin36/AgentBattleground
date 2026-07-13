# Audio v2 — expressive effects (needs research)

> **Status: parked pending research.** The deferred capabilities of the engine audio subsystem, beyond the `57-engine-audio-api` mid-scope v1 (music loop + concurrent SFX + Music/SFX buses + volume/mute + linear fades). Don't run this through the TDD pipeline as-is — each item below needs a scope decision (does *this* game want it?) before it becomes a real spec.

## Purpose
V1 ships the primitives an atmospheric game needs day one. `kira` gives most of the following essentially for free (they are built-in track effects / tweenable parameters), so v2 is largely about *exposing* capability through a clean API and deciding how much this game actually wants — not about new infrastructure.

## Deferred capabilities

- **Pitch / playback-rate bending.** `SoundHandle::set_playback_rate(rate, fade)` — kira's tweenable rate change (coupled pitch+speed, the tape/slow-mo effect). Cheap to add. Decide the use cases (tension ramps, hit-pitch variation) before speccing. True *independent* pitch-shift (pitch without speed) is a separate, heavier path via `signalsmith-stretch` (C++ FFI) — almost certainly not needed.
- **Filters / EQ.** kira `effect::filter` (LP/HP/BP, tweenable cutoff) and `effect::eq_filter` on a bus — the classic "muffle the music when a menu/pause opens" is a one-line cutoff tween. Likely the highest-value v2 item.
- **Reverb / delay / distortion / compressor.** kira `effect::{reverb, delay, distortion, compressor}` as per-bus effects. Atmospheric reverb on a bus is easy; decide whether it's wanted globally or per-scene.
- **Random-SFX variation.** A small game-side helper: pick a clip from a pool and jitter volume/pitch within a range on each `play_sfx` so repeated sounds don't feel identical. Trivial once pitch (above) exists; belongs in `crates/game`, not the engine.
- **Stereo panning / spatial audio.** kira `effect::panning_control` (per-track pan) and spatial tracks + a listener (distance attenuation). Only worth it if the battle viewer wants positional sound tied to on-screen creature location.
- **Disk streaming.** `StreamingSoundData` for long tracks loaded from disk rather than `include_bytes!`. Only relevant once music comes from files outside the binary; introduces the static-vs-streaming type split v1 deliberately skipped.
- **Easing curves on fades.** V1 fades are linear; kira supports easing (`OutPowi`, etc.). Expose `Fade { dur, easing }` if linear proves too mechanical.
- **Sidechain ducking.** True signal-driven ducking (music dips under VO/SFX via a compressor keyed off another bus) is not built into kira — v1's simple volume-tween ducking covers ~90% of cases. If a headline feature needs real sidechain, that is the point to evaluate the **FMOD** Rust bindings (`fmod-oxide`), which give full FMOD Studio parity (adaptive music, sidechain, snapshots) on native desktop as an escape hatch.
- **Adaptive / interactive music.** kira's clock system gives sample-accurate scheduling to hand-build state-driven layering/transitions (combat ↔ explore ↔ tension). No middleware authoring tool exists in Rust short of the FMOD binding. Scope only if reactive music becomes a design goal.

## Dependencies
- Extends `57-engine-audio-api` — every item above builds on v1's global-singleton mixer, buses, and `SoundHandle`.
