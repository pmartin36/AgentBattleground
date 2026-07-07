# Flex-Style Layout Primitive for `engine-render`

> **Status: DRAFT — needs research/discussion before this becomes a real spec.** Motivated directly by the Roster screen's layout-correction saga (`38-roster-screen-layout-corrections` and its several follow-up fine-tuning rounds) as of 2026-07-07 — this document captures why that process was so much more manual/error-prone than it should have been, and what a real fix might look like. Not scoped or decided yet; needs discussion before design work starts. The project owner expects more UI-heavy work coming, so this is worth investing in properly rather than continuing to hand-roll layout math per scene.

## The pain, concretely (from this session)
Fixing the Roster screen's layout took many rounds of precise, hand-computed nudges, each requiring the same kind of reasoning a flex/constraint layout system exists specifically to automate:
- "Top-align the stat bars within their available band" — expressed today as an absolute y-offset computed from everything above it, recomputed by hand every time something upstream changes.
- "Center the arrows relative to the dot-cluster group's actual visual extent (dots + labels), not the full-width band" — a cross-axis centering-within-a-computed-subregion problem, currently solved with bespoke per-element math (`dot_cluster_group_bounds`).
- "Grow the sprite upward by N dots, keeping its baseline (bottom edge) fixed" — an edge-anchored resize, which today means carefully reasoning about which inset constant to shrink without disturbing an unrelated existing gap.
- "Move the details panel left by 2 dots" / "move the stat bars up by 3 dots" — small sub-cell (dot-level, not whole-terminal-cell) nudges that don't map cleanly onto the existing `Rect`-based (cell-granularity) layout model at all, forcing ad hoc dot-level offsets bolted onto otherwise cell-level code.
- "The label should be snug against the bar" — turned out to be a dot-vs-cell rounding bug: a cell-level "zero gap" doesn't guarantee a dot-level zero gap if a border's edge dot lands at the wrong position within its cell.

None of this is exotic — it's exactly what `justify-content`/`align-items`/`gap`/`flex-grow` (or equivalent) exist to express declaratively in any real UI layout system. Right now every scene (`roster_manager.rs`, `main_hub.rs`, etc.) hand-rolls absolute positioning math instead.

## What exists today
- **`26-screen-space-positioning`**'s `anchor`/`stack` primitives (`engine-render`) — already used in several places in this codebase (`RosterManager`'s dot-cluster grouping, `MainHub`'s title/menu layout). These give basic single-point anchoring and simple fixed-size stacking with a gap, but no alignment-within-available-space-with-grow, no edge-pinned-resize, no mixed fixed/flexible children in one flow.
- **ratatui's own `Layout`/`Constraint` API** (already a dependency of this project) is a real, mature 1D constraint-based layout system (`Constraint::{Length, Percentage, Min, Max, Fill}` etc., splitting a `Rect` along one axis) — this is directly relevant prior art already in the dependency tree and worth evaluating as a foundation or reference before designing something from scratch. Not yet investigated in this project — flag as the first research step.
- Everything else (the vast majority of actual positioning in `roster_manager.rs`) is hand-computed `Rect` arithmetic in `layout()` and sibling functions.

## Open questions — genuinely undecided, needs discussion
1. **Cell-level or dot-level?** This project's layout today operates at terminal-cell granularity (`ratatui::layout::Rect`), but braille rendering is 2×4 dots per cell — several of this session's real, concrete asks ("move up 3 dots," "grow 5 dots upward keeping baseline fixed") are sub-cell precision that a cell-granularity layout system can't express natively. Does a real fix need to operate in dot units internally (a bigger shift — dot-resolution containers, only rasterizing to cells at the boundary) or is a cell-level flex system with an escape hatch for dot-level fine adjustment within a cell-sized slot sufficient? This is the central architectural question.
2. **Does this replace `anchor`/`stack`, or sit alongside them?** `anchor`/`stack` are already working, tested, and used in multiple shipped scenes. A full replacement is a bigger, riskier undertaking than introducing a new, more capable primitive and migrating call sites opportunistically as scenes get touched anyway. Leaning toward the latter, but not decided.
3. **Data-dependent child sizes.** Some layout children have a "natural size" that depends on runtime content, not a static number — e.g. the creature sprite's size comes from `fit_dot_dims` fitting an actual image's aspect ratio into an available box. A real flex system needs a way to express "this child's size is computed from its content, constrained by the available space" (closer to CSS's intrinsic sizing / `flex-basis: auto` than a fixed `Constraint::Length`), not just fixed-or-percentage children.
4. **API shape.** CSS-flexbox-like (`direction`, `justify_content`, `align_items`, `gap`, per-child `grow`/`shrink`/`basis`) is the obvious reference model, but should be evaluated against what ratatui's `Layout`/`Constraint` already offers before inventing new vocabulary — reusing established, familiar terminology (whether CSS's or ratatui's own) is probably better than a third bespoke naming scheme.
5. **Where does it live?** `crates/engine/render` — this is unambiguously a cross-cutting rendering mechanism per the project's own engine/game boundary rule, not `crates/game`-specific. Not really an open question, just confirming it up front since it affects scoping.

## Why this matters now
The project owner explicitly flagged that more UI-heavy work is coming. Every future screen built the way `roster_manager.rs` was built (hand-rolled absolute positioning, iteratively corrected via many rounds of "move this N dots" feedback) pays the same tax this session just paid. A real, general layout primitive is exactly the kind of investment that gets more valuable the more UI work follows it — worth doing properly rather than continuing to patch per-scene math.

## Next steps when resuming
1. Actually evaluate ratatui's `Layout`/`Constraint` system hands-on (small throwaway prototype, same validate-before-committing approach used elsewhere in this project) — determine how far it goes toward solving this and where it falls short (likely: cell-granularity only, no intrinsic/content-dependent sizing).
2. Resolve the cell-vs-dot granularity question (Open Question 1) — probably the single biggest scoping decision.
3. Design a minimal first version scoped against THIS session's actual concrete pain points (top-align within available space, cross-axis centering against a computed sub-region, edge-anchored resize, gap-with-alignment) rather than a maximal, speculative general system — validate it by re-deriving the Roster screen's actual layout with it as a proof of concept.
4. Decide the migration story for `anchor`/`stack` call sites (Open Question 2).
5. Once scoped, this becomes a real numbered spec with concrete decisions, not a needs-research doc.

## Dependencies / related specs
- `26-screen-space-positioning` ✅ — the existing `anchor`/`stack` primitives this would extend or sit alongside.
- `13-rendering` ✅ — the dot pipeline; if this ends up operating at dot granularity, it composes directly with `DotBuffer`/`Grid` the same way every other rendering primitive in this codebase does.
- `35-roster-screen-stats-abilities-squad` / `38-roster-screen-layout-corrections` — the concrete, real-world case study motivating this document; a good validation target for a first version.
- Any future UI-heavy scene work the project owner mentioned as the reason this is worth prioritizing.
