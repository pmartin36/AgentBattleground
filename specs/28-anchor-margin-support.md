# Anchor Margin Support

> **Status: draft (not started).** `render::screen_layout::anchor()` (`26-screen-space-positioning`) only places elements flush against a container's edges or exactly centered — there's no way to inset an element by a margin. This bit `24-roster-carousel`'s home/arrow buttons, which are flush against the screen edges with zero breathing room (a real, already-shipped visual issue flagged by the project owner while reviewing spec 25). This spec adds margin support and fixes Roster's buttons to use it. Not a reopening of `24`/`26`'s done status — new capability plus a bug fix, same pattern as `19`/`20`'s relationship to `15`/`18`.

## Purpose
Let a screen-space element be inset from its anchored edge by a margin (in terminal cells), instead of only flush-to-edge or exactly-centered, and use that to fix Roster's edge-flush buttons.

## Scope
- `anchor()` gains margin support. Exact API shape is an implementer call between two reasonable options — extending the existing signature with an additional parameter (e.g. `anchor(container, size, pos, margin: (u16, u16))`) vs. an additive `anchor_with_margin(...)` function alongside the existing zero-margin `anchor()` — but whichever is chosen, **every existing call site of `anchor()`/`stack()` in `main_hub.rs` (spec 25, done) must continue compiling and behaving identically** (title box and menu container are intentionally centered/flush today and must stay that way unless a margin is explicitly threaded through). Margin only affects `Near`/`Far`-aligned axes (a `Center`-aligned axis has no "edge" to inset from — margin on that axis is a no-op, not an error).
- `crates/game/src/scenes/roster_manager.rs`'s `home_rect()` and `arrow_rects()` — currently hand-rolled `Rect` math placing buttons exactly flush against `area`'s edges — are rewritten to call the new margin-aware `anchor()` instead, with a real inset (not zero — that would be a no-op fix). This is the actual bug fix motivating this spec.
- `RosterManager`'s existing tests (render position assertions for the arrow/home buttons) are updated to expect the new inset position, not deleted — the "buttons render beside/at their target position" contract still holds, just at an inset position instead of flush.

Out of scope:
- Retrofitting `MainHub`'s title box/menu container (spec 25, done) with a margin — they're `Center`/`TopCenter`-anchored, not corner-flush, so this issue doesn't apply there today. If a future change makes it apply, that's its own follow-up.
- Any inspector/`Inspectable` exposure of margin or other screen-space positioning parameters — explicitly punted by the project owner (screen-space `Rect`s are still computed by pure functions from constants, not stored owned fields, so there's nothing to expose yet; making positioning inspectable is a larger, deliberately deferred change).

## Decisions (v1)
- **A real, non-zero default margin for Roster's buttons** — 1 terminal cell inset from each relevant edge is the concrete target (a visually meaningful gap given each cell is a 2×4 braille dot block), applied to `home_rect()` (inset from top and right) and `arrow_rects()` (inset from left/right respectively). Implementer may adjust the exact cell count if 1 reads as insufficient once rendered, but "still flush (0 margin)" is not an acceptable outcome — that's the bug being fixed.
- **Margin is expressed in whole terminal cells** (`u16`), matching every other screen-space measurement in this codebase (`Rect`, `anchor`, `stack` all already use cell units) — no sub-cell/pixel unit introduced.

## Dependencies
- `26-screen-space-positioning` ✅ — extends `anchor()` directly.
- `24-roster-carousel` ✅ — `home_rect()`/`arrow_rects()` are the concrete bug this spec fixes.
- `25-main-hub-navigation` ✅ — must not regress; its existing `anchor()`/`stack()` call sites are the compatibility bar this spec's API choice is checked against.
