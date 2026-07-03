# Tint Shape Invariance

> **Status: draft (not started).** Tinting a sprite/button must never change *which* braille dots are lit — only their color. Confirmed broken: rendering the same `Button` across `Idle`/`Hover`/`Pressed` states produces genuinely different braille glyphs (not just different colors) at multiple cells — e.g. one cell shows `⣻` (7 dots) in Idle, `⢻` (a different 7-dot pattern) in Hover, `⣿` (all 8 dots) in Pressed. The shape of an icon visibly shifts as its button's interaction state changes.

## Purpose
Restore (or establish, if it never actually held) the invariant that color/tint operations are pure colorization — they recolor already-decided pixels and never re-decide which pixels are "on."

## Root Cause
`dots::cell_from_dots` (the braille-glyph decision function) uses an **adaptive per-cell luma threshold**: a dot's bit is set only if its luma is `>=` the average luma of all visible (non-transparent) dots in that 2×4 cell. This threshold is recomputed fresh every time `dots_to_grid` runs — including after `tint()` has already recolored the buffer.

`tint()` is a per-channel multiply (`out = src * color / 255`, integer-truncated) applied independently per dot. A single *uniform grayscale* tint (equal R/G/B) scales every dot's luma by the same factor, which mathematically preserves the `luma_i >= avg` comparison exactly — so tinting alone shouldn't move the threshold. Two things break that in practice:

1. **Non-grayscale tints don't scale luma uniformly.** Once a layer is pre-tinted with a genuinely colored value (e.g. `PANEL_GOLD_TINT`/`ICON_AMBER_TINT`, introduced when button icons got real color), a *second*, different-hued tint applied on top does not scale every dot's luma by the same factor, because luma is a weighted sum of channels (`0.299r + 0.587g + 0.114b`) and a colored tint changes each channel by a different ratio. Two dots that started at different hues (one from the icon layer, one from the panel layer, composited into the same mixed cell) can have their *relative* luma ordering shift depending on which tint is layered on next.
2. **Integer truncation compounds across sequential `tint()` calls.** `render_tinted` (`crates/render/src/button.rs`) applies `ICON_CONTRAST_TINT`/`ICON_AMBER_TINT` to the icon layer, composites, then applies the whole `ButtonState` tint on top — two sequential truncating integer multiplies. Small rounding differences between dots whose luma is already close to the cell average can flip which side of the threshold they land on. This is most visible at `Pressed` (the darkest state, `0x8c`): low absolute luma values make a ±1 rounding error proportionally much larger, which is consistent with the observed "everything goes fully solid" result at that state.

Both mechanisms exist because **the same computation (`cell_from_dots`) is asked to decide "is this pixel part of the shape" and "how should mixed-color-and-brightness sub-pixels within one cell be dithered" using post-tint, not pre-tint, data.** Nothing today enforces that a dot's *inclusion* in the glyph mask is decided independently of any tint applied afterward.

## Scope
- Establish and enforce the invariant: **the braille glyph mask (`ch`, which dots are lit) for a given rasterized shape must be identical regardless of any tint(s) subsequently applied to it.** Only the glyph's stored `color` may change with tint.
- Concretely: move tinting to operate on the **already-braille-converted `Grid`** (one color per cell) instead of the pre-conversion `DotBuffer` (8 sub-dot colors per cell) — i.e. rasterize → decide the glyph mask once via `cell_from_dots` → *then* tint only recolors each `Cell::Glyph`'s stored `color`, structurally incapable of touching `ch`. This matches what the project owner's own mental model already assumed the pipeline did ("I thought we did the transparency pass before the tint").
- Audit and update every current caller of `dots::tint` (`crates/render/src/button.rs`'s `render_tinted`/`ICON_AMBER_TINT`/`PANEL_GOLD_TINT`, `crates/game/src/scenes/battle_viewer.rs`'s team-tint) to tint at the `Grid` level instead of the `DotBuffer` level, preserving each caller's actual color-composition intent (e.g. `Button`'s layered panel+icon compositing still needs to composite at the `DotBuffer` level *before* the mask is decided — only the *tint* step moves after).
- Verify this does not change how genuinely multi-color sprites (the bundled creature GIFs — richly shaded, real photographic-style gradients) currently render. The adaptive-luma dithering itself is not being removed — it still runs once, on the untinted composited buffer — only *tinting after that point* changes from "operates on 8 dots, can perturb the threshold" to "operates on 1 already-decided color per cell, cannot."

Out of scope:
- Changing `cell_from_dots`'s adaptive-luma algorithm itself (the dithering rule for how a single flat-shaded sprite's sub-cell antialiasing works) — that stays as-is; only *when* tint is allowed to run relative to it changes.
- `27-render-frame-caching`'s frame-to-frame rasterization caching — a related-but-separate performance concern (see that spec; also see `30-asset-decode-caching` for a narrower, distinct performance gap this investigation also surfaced).

## Decisions (v1)
- **Tint moves from `DotBuffer` to `Grid`.** `dots::tint(buf: &DotBuffer, color: Rgba) -> DotBuffer` is replaced (or supplemented, if any caller still legitimately needs pre-mask-decision compositing tint — investigate whether one exists before assuming not) by a `Grid`-level equivalent, e.g. `grid::tint_grid(grid: &Grid, color: Rgba) -> Grid`, multiply-blending only each `Cell::Glyph`'s stored `color` field and leaving `ch` and `Cell::Transparent` cells untouched.
- **Compositing (layering multiple sprites/images with depth, e.g. `Button`'s panel+icon) still happens at the `DotBuffer` level, before `dots_to_grid`.** Only the *tint* step (recoloring for interaction state / team color / etc.) moves to after. This preserves today's compositing semantics exactly — the fix is scoped to tint, not to compositing.
- **This is a breaking change to `dots::tint`'s call sites** — every current caller must be updated to call `dots_to_grid` earlier in its own pipeline (before tint) rather than after. Enumerate every call site as part of implementation (known so far: `button.rs::render_tinted`, `battle_viewer.rs`'s team-tint path) and confirm none are missed via a workspace-wide grep before considering this done.
- **Verification requirement**: the same "render across states, diff the glyph mask" technique used to *discover* this bug (render `Idle`/`Hover`/`Pressed` — or team-A/team-B for `battle_viewer.rs` — and assert the `ch` field is identical across all of them at every cell, only `color` may differ) becomes a real regression test, not just a manual check.

## Dependencies
- `13-rendering` ✅ — `dots::tint`, `cell_from_dots`, `dots_to_grid`, `Grid`/`Cell` are all defined here; this spec changes their contract.
- `22-braille-ui-chrome` ✅ — `Button`/`FrameButton`'s `render_tinted` is the primary place this bug was discovered and the primary caller needing updates.
- `18-battle-viewer-baseline` ✅ — `battle_viewer.rs`'s team-tint is another caller needing the same update.
- Related but distinct: `27-render-frame-caching` (rasterization-result caching) and `30-asset-decode-caching` (image-decode caching) — different specific waste at different pipeline layers, not the same fix as this spec.
