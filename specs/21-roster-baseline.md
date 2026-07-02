# Roster — Baseline

> **Status: draft (not started).** A minimal, presentation-only slice of `03-army-skill-editing`: a view of the player's 6-piece army — a sprite and a placeholder label per piece — reachable today via the existing debug digit-key (`3`) or `--scene ArmyEditor` CLI boot flag. No stats, abilities, skill editing, or persistence. Mirrors how `18-battle-viewer-baseline` proved out rendering before any battle rules existed.

## Purpose
Give the player's army a visible home before any of spec 03's real content (skill files, upgrade history, stats/abilities) exists.

## Scope
- The scene mapped to `SceneId::ArmyEditor` gets real content instead of the current placeholder fill. Its `display_name()` changes from `"Army Editor"` to `"Roster"` (the enum variant name itself, `ArmyEditor`, is unchanged — this is a user-facing label change only).
- Six placeholder army-slot entries, each showing a sprite and a plain-text placeholder label (`"Piece 1"` .. `"Piece 6"`).
- A simple layout — six independent slots (e.g. equal-width columns via ordinary `ratatui::layout::Layout` splitting). No board, world-space, or camera model is needed here: reuse `render::convert` (the tier-1 image→braille-in-an-area function, already built and validated), not `BattleViewer`'s dot-compositor/camera pipeline — there's no "world" to place things in, just independent UI slots.

Out of scope:
- Real piece data (stats, abilities, skill files, upgrade history) — spec 03's fuller vision, entirely pending.
- Persistence / save-load of army composition.
- Any editing capability (rename, reorder, replace a piece) — this is read-only.
- Navigation *to* this scene via an in-game menu — that's `Main Hub — Navigation Baseline` (a separate, not-yet-written spec, built after this one). For this build, the existing debug digit-key `3` (`crate::scenes::scene_for_digit`) and the `--scene ArmyEditor` CLI boot flag (`crates/game/src/cli.rs`) already reach it — no new navigation plumbing needed to demo this spec.

## Decisions (v1)
- Every slot reuses the bundled `wizard.gif` asset (same one `BattleViewer` uses) — no new art, no per-slot visual distinction needed (there's only one army here, no opposing team to differentiate via tint/mirror the way `18-battle-viewer-baseline` does).
- Labels are placeholder text (`"Piece 1"` .. `"Piece 6"`), not real piece names or stats — plain terminal text, not braille, per `CLAUDE.md`'s "braille is universal except text" exemption for labels.
- Idle animation (playing the GIF's frame loop) is optional for this baseline — implementer's call. If included, reuse the exact per-index phase-stagger pattern `18-battle-viewer-baseline` already established (`PIECE_STAGGER`-style offset) rather than inventing a new one.

## Dependencies
- `13-rendering` ✅ — `render::convert`, used directly; no camera/world-space dependency needed.
- Feeds `03-army-skill-editing` — this is army-*viewing*'s baseline; skill editing, stats, and abilities remain entirely pending there.
- Feeds the not-yet-written Main Hub navigation spec — the eventual menu will link here.
