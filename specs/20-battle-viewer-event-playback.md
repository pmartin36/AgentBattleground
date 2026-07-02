# Battle Viewer — Event Playback

> **Status: draft (not started).** The first concrete slice of `05-battle-viewer`'s stage 2 (replay-driven viewer): scripted playback of two universal battle events — piece movement and piece death — animated on top of the static battlefield (`18-battle-viewer-baseline`). Attack/take-damage events and a real replay file format are explicitly NOT part of this spec (see *Scope*).

## Purpose
Prove the viewer can animate a battle over time — pieces gliding between cells, pieces dying — independent of any real combat rules or replay file format, continuing `05-battle-viewer`'s build approach of building presentation before rules.

## Scope
- An in-memory `Vec<Event>` playback model, hand-authored directly in the scene for this build (not loaded from any file/wire format)
- `Move` events: a piece glides from its current position to a target cell
- `Die` events: a piece plays a death animation and becomes inert (not removed from the piece list)
- A playback clock that can animate multiple overlapping events at once

Out of scope:
- **Attack / take-damage events.** Their visual shape depends on combat mechanics that don't exist yet (ranged? melee? AoE? something else) — `10-battle-simulation-engine`. Picking a visual now risks guessing wrong and having to redo it once combat is actually designed.
- **A real replay file format.** How a battle's events get serialized, stored, and transmitted is `12-data-model-sync`'s concern, deliberately decoupled from this spec — the same way `16-world-space-and-camera` decouples board cells from world position. This spec's internal `Event` list only needs to be *representable* by a future serialized format, not identical to it.
- Playback controls (play/pause/step/speed/jump-to-turn) — `05-battle-viewer`'s later scope.
- Any real battle simulation producing these events — this build's event list is hand-scripted, proving playback only.

## Decisions (v1)
- **Event shape**:
  ```rust
  struct Event { turn: u32, start_time: f32, duration: f32, kind: EventKind }
  enum EventKind {
      Move { piece_index: usize, to: (u16, u16) },
      Die { piece_index: usize },
  }
  ```
  `start_time`/`duration` are elapsed seconds, matching `BattleViewer.elapsed`'s existing unit — they drive the actual smooth interpolation. `turn` is a separate, discrete grouping tag: this game is turn-based (`10-battle-simulation-engine`), and `05-battle-viewer`'s future playback controls ("step forward/back by turn", "jump to turn N") need a discrete unit to navigate by that cannot be reconstructed from continuous time alone once multiple turns' events overlap in wall-clock time for pacing. Multiple events may share the same `turn` while still having different `start_time`s (staggered within the turn) — `turn` does not replace the clock, it labels which turn produced each event.
- **`piece_index` targets `Piece.index`** (a piece's own stable identity field), not its position in `BattleViewer.pieces` — stays valid regardless of future removal/reordering. Independent of `Piece.team` (ownership) — resolving which piece an event affects never needs to know or infer which side owns it.
- **`Move` carries only a destination**, not a redundant "from". `EventKind::Move` MUST NOT gain a `from` field. The glide interpolates from wherever the piece's `Transform.translate` actually is when the event starts, via the existing `Tween`/`ease_in_out` utility (`16-world-space-and-camera`, built but previously unused by any shipped scene). Remembering that starting position for the duration of a multi-frame tween is transient, scene-internal runtime bookkeeping (e.g. a small cache populated the frame an event's window begins), not part of the authored `Event` data — the same way `18-battle-viewer-baseline` keeps per-frame render state separate from the data it derives from.
- **Gameplay truth commits instantly at move start; world position lerps to catch up (cosmetic)** — this is `16-world-space-and-camera`'s already-decided model ("a move commits instantly in gameplay... the world position lerps to catch up"), applied here: the moment a `Move` event's window begins, the piece's `col`/`row` update to `to` immediately. `transform.translate` continues to visually glide toward the new cell's center over the event's `duration` — the gap between instantly-updated gameplay truth and still-catching-up render truth is exactly the point of the cosmetic lerp.
- **Events may overlap in time.** The playback clock evaluates "which events are active at the current elapsed time" every frame and drives every affected piece simultaneously — not a strict one-event-at-a-time assumption. A real battle will very likely want simultaneous multi-piece actions within a single turn.
- **Death**: `Piece` gains `pub alive: bool` (defaults `true`). A `Die` event animates `Transform.scale` to zero via `Tween` over the event's duration; once `alive == false` the piece is excluded from rendering. It is NOT removed from `BattleViewer.pieces` — keeps `Vec` length/indices stable and trivially supports flipping `alive` back to `true` later (e.g. a hypothetical revive mechanic) with no special-casing.
- **The event sequence is hand-authored/hardcoded directly in the scene** for this build, mirroring how the static 6v6 `pieces()` layout is hardcoded — no file loading, no wire format.

## Dependencies
- `18-battle-viewer-baseline` ✅ — the static board/piece rendering this animates on top of.
- `16-world-space-and-camera` ✅ — the `Tween`/`ease_in_out` utility this is the first real consumer of.
- Feeds `05-battle-viewer` — this is stage 2's move/death slice; attack/take-damage events and a real replay file format remain pending there.
- Blocked-on, for the REST of stage 2 (not this spec): `10-battle-simulation-engine` (attack/damage visual shape), `12-data-model-sync` (real replay file format).
