> # ✅ DONE! — Completed 2026-07-02

# Screen-Space Positioning

> **Status: implemented.** Reusable primitives for placing UI elements (a sprite, a `22-braille-ui-chrome` `Button`, a text label) in screen space — terminal-cell coordinates, the same space `ratatui::layout::Rect` already uses — without every scene hand-deriving its own `Rect` arithmetic. `22-braille-ui-chrome` explicitly scoped generic anchoring/layout helpers out at the time ("each consuming scene computes its own button `Rect`s... via ordinary `ratatui::layout::Layout`"); this spec adds that capability now that more than one scene needs it. Not a scope reopening of `22` (already shipped and unchanged) — new capability.
>
> Lands as `render::screen_layout`: `anchor()` (9-position grid, pure function, saturating/clamped), `stack()` (vertical/horizontal, gap-spaced), `RectTween` (animates a `Rect` via 4 independent `Tween` channels, reusing `crate::tween::Tween` unmodified).

## Purpose
Give any scene a small, reusable vocabulary for "where does this go on screen" — anchoring to a corner/edge/center, stacking a group of elements with spacing, and (building on top of those) animating an element's position over time — instead of each scene re-deriving `Rect` splits and offset math from scratch.

## Scope
- **Anchoring**: given a container `Rect` and an element's size, compute the element's `Rect` anchored to a named position — top-left, top-center, top-right, center, bottom-center, etc.
- **Stacking**: given a container `Rect`, a list of element sizes, and a gap, compute each element's `Rect` stacked vertically or horizontally in order.
- **Animated positioning**: a thin wrapper pairing the above with the existing `Tween`/`ease_in_out` (`16-world-space-and-camera`) to animate an element's `Rect` from one screen position to another over time (e.g. "slide in from off-screen-left to its anchored resting position") — reusing `Tween`, not reimplementing interpolation.
- Pure screen-space (terminal-cell `Rect` math). No relationship to `16-world-space-and-camera`'s `WorldPos`/`Camera`/board-placement model — this is UI layout, not board/sprite world placement.

Out of scope:
- Any specific scene's actual layout (Roster's carousel, Main Hub's menu, etc.) — this spec is the reusable primitive; consuming scenes' specs decide how they use it.
- Wrapping/reflow, scrolling containers, z-ordering — no current consumer needs them.

## Decisions (v1)
- **Lives in `render`** (alongside `Button`, `convert`, `tint`) — not `scene-core` (no dependency direction change) and not `game` (every scene, not just one, is a potential consumer).
- **Pure functions over a stateful layout system.** Anchoring/stacking are computed fresh each frame from a container `Rect` + sizes, not cached/mutable layout state — matching how `ratatui::layout::Layout` itself already works, and how `18-battle-viewer-baseline`'s `BoardGeometry` is recomputed per frame rather than cached.
- **Animated positioning is additive, not required.** A scene that just needs static anchoring uses the anchoring functions directly; the `Tween`-based helper is for scenes that also want motion (e.g. a slide transition). Building this doesn't force every screen-space consumer to animate.

## Dependencies
- `13-rendering` ✅ — the `Rect`/dot-pipeline this positions elements within.
- `16-world-space-and-camera` ✅ — `Tween`/`ease_in_out`, reused unmodified for the animated-positioning helper.
- `22-braille-ui-chrome` ✅ — a `Button`'s `Rect` is exactly the kind of element this spec's anchoring/stacking helpers are meant to compute.
- Intended to feed `24-roster-carousel` and `25-main-hub-navigation` — but neither of those specs' text references this yet. Once this spec is built and independently verified, `24` and `25` get their own text updates (a real edit to those spec files, not a verbal aside) and are re-run through a fresh `tdd-pipeline` invocation — full deconstruct + validate, not a resume or a patched prompt — so the decomposition validator gets a genuine chance to review the updated plan.
