# Flex Layout Primitive — `engine-render`

## Purpose
A dot-native flex-style layout primitive for `engine-render`, solving the class of problem `26-screen-space-positioning`'s `anchor`/`stack` can't: alignment within a computed sub-region, proportional grow/shrink, content-driven (intrinsic) child sizing, and sub-cell (braille-dot) precision — without bolted-on per-caller nudge fields. Motivated directly by `specs/needs-research/flex-style-layout-primitive.md`, itself written after the `38-roster-screen-layout-corrections` saga, where every one of these problems was solved by hand, per call site, with a fresh named constant and a paragraph of doc comment defending it.

This spec ships `flex()` and its supporting types in `engine-render`, **and** proves them by fully re-implementing `crates/game/src/scenes/roster_manager.rs`'s entire positioning surface on top of the primitive — the real screen that motivated this work in the first place, not a synthetic stand-in for it. The migration is a **strictly lossless refactor**: today's already-shipped, `38`-corrected visual output is the target, reproduced exactly, with the file's own existing test suite as the acceptance oracle. No new layout/visual decisions get made here — a real desired change to Roster's look is a separate, later spec. Validating an abstraction only against tests written by the same hand that designed it is exactly how `35`'s original layout shipped wrong in the first place; re-deriving an already-correct, independently-specified baseline is a much stronger bar.

## Scope
- A new module in `engine-render` (`crates/engine/render/src/flex.rs`) exporting: `DotRect`, `Direction`, `Justify`, `Align`, `Basis`, `FlexChild`, `FlexStyle`, `flex()`, `DotRectTween`.
- A single-axis-per-call flex solver operating entirely in **dot units** (1 dot = 1 sub-cell braille pixel, 2 wide × 4 tall per terminal cell) — CSS-flexbox-equivalent semantics: basis sizing (fixed or intrinsic/content-driven), proportional grow **and** shrink, main-axis `justify_content`, cross-axis `align_items`, `gap`.
- `DotRect::to_cell_rect()` / `DotRect::cell_remainder()` — the sole place dot-space output collapses into a whole-cell `ratatui::Rect`. Everything upstream of this stays dot-precise.
- `DotRect::inset()` — a padding/margin helper (see *Decisions*), needed once real insets (`EDGE_MARGIN`, the sprite's 4-sided render insets, the details panel's asymmetric left shift) are expressed in dot units.
- `DotRectTween` — a dot-native sibling of `RectTween` (`26-screen-space-positioning`), so `flex()` output can be animated (slide-in/out, etc.) without cell-rounding jitter mid-transition.
- Primitive-level test coverage, independent of Roster, re-deriving each concrete pain point named in the needs-research doc as a synthetic scenario: top-align within an available band; cross-axis centering against a computed sub-region's actual bounding box (via two nested `flex()` calls); edge-anchored resize (a fixed sibling + one grow child, pinning the far edge while the grow child's near edge moves); gap combined with non-`Start` justification. This exists so a *second* future consumer of `flex()` has a spec to read that isn't just "however Roster happened to use it."
- **`decode_braille_cell` + `diff_dots`/`DotDiff`** in `engine-render` — a deterministic, dot-by-dot comparison utility between two rendered `ratatui::Buffer`s, decoding every cell's braille glyph back into its 8-dot lit/unlit bitmask and reading its one shared color. This is what makes the Roster migration's "lossless" claim (see below) an enforced, automated gate instead of a manual eyeball check.
- **Consolidation of `roster_manager.rs`'s duplicated buffer-inspection test helpers** (`rect_text` ×5, `braille_mask` ×2, `sample_fg` ×2, `region_cells` ×1 — all private, near-identical, per-test-module copies) into `crates/game/src/scenes/test_util.rs`, which already exists and already deduplicated this file's render/event test helpers but missed these. `braille_mask` is rebuilt on top of the new shared `decode_braille_cell`, rather than staying its own separate hand-rolled decoder.
- **A debug cell-boundary gridline overlay + a global toggle keybinding** — a post-composite render pass (in `engine-render`) that marks every braille cell's 2-dot-wide × 4-dot-tall boundary directly on the terminal output, toggled by a single new engine-level keybinding that applies regardless of which scene is active.
- **Full replacement of `roster_manager.rs`'s entire positioning surface** — `layout`, `dot_bands`, `dot_cluster_rects`, `dot_cluster_group_bounds`, `dot_slots`, `arrow_rects`, `home_rect`, `details_panel_rects`, `stat_slice_parts`, and the `ARROW_NUDGE_DOWN_DOTS`/`HOME_NUDGE_UP_DOTS`/`DOT_SLOT_DOWN_DOTS` render-time nudges — onto `flex()`/`DotRect`/`DotRect::inset()`. This includes the file's 2 existing `anchor`/`stack` call sites (`dot_cluster_rects`, `stat_slice_parts`): one consistent primitive for the whole file, not two positioning systems left side by side. Drawing/rendering logic (`render_sprite`, `render_stat_bars`, `draw_dot_box`/`draw_dot_border`/`draw_dot_cap_box`, `draw_dot_slot`'s glyph compositing) is unchanged — only how positions/sizes are *computed* changes.

## Decisions (v1)

### Granularity: dot-native, locked in
Every size, gap, and position `flex()` deals with is in dot units. `Rect`/cells are strictly an *output* concern — `DotRect::to_cell_rect()` is the only conversion point. This directly eliminates the class of bug spec 38 hit ("a cell-level zero gap doesn't guarantee a dot-level zero gap") and the `Button::set_dot_offset_down`-style bolted-on nudge fields, by construction rather than by convention/comment discipline.

Nested `flex()` calls stay dot-precise end to end: an outer call's child `DotRect` is passed directly as an inner call's `container`, with no intermediate rounding to cells. Rounding to cells happens exactly once, at the final `to_cell_rect()` call a renderer makes right before constructing a `ratatui::Rect`-based type (`Button::new`, `draw_grid`'s `area` param, a hit-test rect).

### Output type: `DotRect`, not `ratatui::Rect`
```rust
/// A rect in dot units — 1 dot = 1 sub-cell braille pixel
/// (2 wide × 4 tall dots per terminal cell).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DotRect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }

impl DotRect {
    /// Floor to the containing whole-cell `Rect` (x/w ÷ 2, y/h ÷ 4).
    pub fn to_cell_rect(self) -> ratatui::layout::Rect;

    /// Sub-cell remainder after flooring to `to_cell_rect()` — (dx, dy) in
    /// 0..2 / 0..4 — for precision render nudges. This is what replaces
    /// `Button::set_dot_offset_down`-style bolted fields: the offset is a
    /// real value baked into the computed position, not a second constant
    /// a caller must remember to apply on top.
    pub fn cell_remainder(self) -> (i32, i32);

    /// Shrink `self` inward by the given dot amounts on each edge — the
    /// padding/margin equivalent for a container passed into `flex()`, and
    /// for a leaf `DotRect` that needs a render-target inset independent of
    /// its layout-computed slot (e.g. Roster's `EDGE_MARGIN`, the sprite's
    /// 4-sided asymmetric render inset, the details panel's extra left
    /// shift). Deliberately a plain `DotRect` method, not a `FlexStyle`
    /// field — `flex()` itself only ever distributes children along one
    /// axis; insetting a container/child is a separate, composable step
    /// callers apply before/after a `flex()` call, not a second concern
    /// baked into the solver.
    pub fn inset(self, left: i32, right: i32, top: i32, bottom: i32) -> DotRect;
}
```
Callers needing a plain cell `Rect` for hit-testing or an existing `Rect`-typed constructor call `.to_cell_rect()`. Callers needing full sub-cell precision (compositing directly into a `DotBuffer`) use the `DotRect` as-is.

### Composition model: flat, composable functions
No recursive `Node`/tree/builder type in v1. `flex()` takes a container + a slice of `FlexChild` + a `FlexStyle`, and returns one `DotRect` per child — matching `stack()`'s existing flat shape, just far more capable per call. Nesting (e.g. `dot_row` → 3 role clusters → N slots each) is achieved the same way `dot_cluster_rects` already composes `anchor` + `stack` today: call `flex()` once for the outer level, then call `flex()` again on each resulting `DotRect` for the level below. This keeps the same stateless, immediate-mode contract spec 26 already commits to ("computed fresh each frame... not cached/mutable layout state") — `flex()` is a pure function, safe to call every frame, never panics.

### Sizing: `Basis` (fixed or intrinsic) + proportional `grow`/`shrink`
```rust
/// A child's main-axis base size before grow/shrink is applied.
pub enum Basis {
    /// Exact dot size on the main axis.
    Fixed(i32),
    /// Content-driven: given the container's main-axis extent (the most
    /// space this child could ever be offered), returns this child's
    /// natural (main, cross) size in dots. Invoked once per `flex()` call,
    /// in child order, before grow/shrink distribution — this is what lets
    /// something like the sprite's `fit_dot_dims` aspect-fit plug directly
    /// into a `flex()` call instead of being computed as a manual pre-step.
    Intrinsic(Box<dyn Fn(i32) -> (i32, i32)>),
}

pub struct FlexChild {
    pub basis: Basis,
    /// > 0.0: this child claims a proportional share of leftover main-axis
    /// space beyond the sum of all bases + gaps (CSS flex-grow). `0.0`
    /// (default): never grows.
    pub grow: f32,
    /// > 0.0: this child compresses proportionally, weighted by
    /// `shrink * basis` (the CSS flex-shrink convention), when total
    /// basis + gaps exceeds the container. `0.0` (default): never shrinks
    /// below its basis. A child with `shrink == 0.0` for every child and an
    /// over-committed container clips at the container edge, matching
    /// `anchor`/`stack`'s existing `Rect::intersection`-based clamp
    /// behavior today.
    pub shrink: f32,
}
```
Solve order: (1) resolve each child's basis — `Fixed` as given, `Intrinsic` invoked with the container's main-axis extent; (2) sum bases + `gap * (n-1)`; (3) if the container has leftover space and any child has `grow > 0`, distribute the leftover proportionally by grow weight; if the total instead overflows the container and any child has `shrink > 0`, distribute the deficit proportionally by `shrink * basis`, clamped so no child's final size goes negative; (4) position every child along the main axis per `justify_content`; (5) align every child on the cross axis per `align_items`.

Both grow and shrink are full proportional (CSS-parity) rather than a single designated filler child — deliberately more general than any single pain point in this session strictly required, chosen for CSS fidelity now rather than a narrower model that would need revisiting the first time two siblings both need to grow.

### API vocabulary: CSS Flexbox terms
```rust
pub enum Direction { Row, Column }

/// Main-axis distribution of children after basis+grow/shrink sizing.
pub enum Justify { Start, Center, End, SpaceBetween, SpaceAround, SpaceEvenly }

/// Cross-axis alignment of each child within the container's cross extent.
pub enum Align { Start, Center, End, Stretch }

pub struct FlexStyle {
    pub direction: Direction,
    pub justify_content: Justify,
    pub align_items: Align,
    /// Dots, between adjacent children only — never before the first or
    /// after the last child.
    pub gap: i32,
}

/// Pure function — no cached state, safe to call fresh every frame (same
/// contract `anchor`/`stack` already have). Never panics; degrades via the
/// same saturating/clamping approach `anchor`/`stack` use for undersized
/// containers or oversized children.
pub fn flex(container: DotRect, style: FlexStyle, children: &[FlexChild]) -> Vec<DotRect>;
```
Chosen over ratatui-`Constraint`-flavored naming or extending this codebase's own `Anchor`/`StackAxis` vocabulary: CSS flexbox terms are the most universally recognized, and match the needs-research doc's own working vocabulary throughout.

### ratatui's `Layout`/`Constraint`: naming inspiration only, not wired in
Evaluated directly (see needs-research doc's next-steps #1): ratatui's `Layout` splits one axis of a **cell-space** `Rect` per call, using `Length`/`Percentage`/`Ratio`/`Min`/`Max`/`Fill` constraints — and does **not** solve cross-axis alignment (callers get the container's full cross extent back) or intrinsic/content-driven sizing, two of this primitive's explicit must-haves. Reusing its actual solver (e.g. by scaling dot values into a synthetic coordinate space and feeding them through `Layout::split`) would bend an API not designed for that unit system while still leaving both must-haves to build from scratch. `flex()` is a custom solver; only `Fill`'s "compete for leftover space" concept carries over conceptually into `grow`.

### Animation: `DotRectTween`
```rust
/// Dot-native sibling of `RectTween` (`26-screen-space-positioning`) —
/// interpolates between two `DotRect`s (typically an off-screen "from" and
/// a `flex()`-computed resting "to") over `dur`. Same per-field
/// `Tween`-delegation contract as `RectTween`, at dot instead of cell
/// precision — avoids the cell-rounding jitter of re-flooring to a whole
/// cell every intermediate animation frame.
pub struct DotRectTween { /* x, y, w, h: Tween */ }
impl DotRectTween {
    pub fn new(from: DotRect, to: DotRect, dur: Duration) -> Self;
    pub fn at(&self, elapsed: Duration) -> DotRect;
}
```
`flex()` itself has no awareness of animation, exactly as `anchor`/`stack` don't today: it only ever computes the resting position for the current frame's inputs. A scene wanting a sprite to slide out of view computes `flex()`'s resting `DotRect` as the tween's `to`, builds an off-screen `from`, and renders `DotRectTween::at(elapsed)` instead of the raw `flex()` output while a transition is in flight — the same pattern `roster_manager.rs`'s existing slide transition and `RectTween` already use, just at dot precision. Because `flex()`'s output is a plain value (not an owned/cached layout tree), a scene can trivially override, offset, or ignore it for any single child on any given frame without `flex()` needing to know.

### Relationship to `anchor`/`stack`: permanent coexistence (policy), full replacement within Roster (this spec's exception)
`anchor`/`stack` are not deprecated as a general policy and are not force-migrated project-wide. `main_hub.rs`'s 3-button menu — the cleanest, simplest existing consumer — stays exactly as it is; there is no mandate that every `anchor`/`stack` call site everywhere must move to `flex()` just because `flex()` now exists. Within `roster_manager.rs` specifically, both of its existing `anchor`/`stack` call sites (`dot_cluster_rects`, `stat_slice_parts`) DO move to `flex()`, per *Scope* above — a one-file, one-time consistency call (one positioning primitive per file, not two), not a reversal of the general coexistence policy.

### Roster migration: lossless refactor, full replacement, existing tests as the oracle
`roster_manager.rs`'s entire positioning surface (see *Scope*) is re-implemented on `flex()`/`DotRect`/`DotRect::inset()`. Concretely:
- **Acceptance bar**: the file's existing test suite (5 `#[cfg(test)]` modules, ~2,960 lines) passes. A test may be *updated* only when its assertion targets an internal implementation detail that's genuinely gone (e.g. a test asserting `ARROW_NUDGE_DOWN_DOTS`'s exact value, once that constant no longer exists) — never because the rendered/visual result it checks (a rect's position, a border's extent, a gap's size) changed. If reproducing today's exact output through `flex()` turns out to require a compromise, that is a signal to revisit this spec's API, not license to quietly change Roster's visuals here.
- **Expected deletions**: `ARROW_NUDGE_DOWN_DOTS`, `HOME_NUDGE_UP_DOTS`, and `DOT_SLOT_DOWN_DOTS`'s manual `off_x`/`off_y` nudge — all three exist purely to bolt sub-cell precision onto a cell-granularity result; dot-native `flex()` output should make each unnecessary by construction. If any of the three turns out to still be needed, that's a real finding the implementation should surface, not silently work around.
- **Not expected to change**: constants governing *drawn content* rather than *position* (`STAT_BAR_HUG_CAP_DOTS`, `BORDER_THICKNESS`, `CHAMFER`, color constants, `STAT_DISPLAY_CAP`, etc.) — these belong to `render_stat_bars`/`draw_dot_box`/`draw_dot_border`, which are explicitly out of scope (see *Out of Scope*).
- **Insets carry over via `DotRect::inset()`**: `EDGE_MARGIN`, `DETAILS_LEFT_SHIFT`, `STAT_BAR_LEFT_MARGIN`/`STAT_BAR_DETAILS_MARGIN`, and the sprite's `SPRITE_INSET_LEFT`/`RIGHT`/`TOP`/`BOTTOM` all become dot-unit `.inset(...)` calls (cell-unit constants convert once, at the top, by multiplying by 2/4) rather than hand-written `saturating_sub` arithmetic scattered through `layout()`.

### Deterministic dot-by-dot comparison: `decode_braille_cell` + `diff_dots`
```rust
/// Decodes the braille glyph at `(x, y)` in `buf` into its 8-dot lit/unlit
/// bitmask (bit k set = dot k, in the same (dx, dy, bit) order as
/// `dots.rs`'s `DOTS` table) plus that cell's one shared foreground color.
/// `None` for a non-braille cell (space/blank — nothing lit).
pub fn decode_braille_cell(buf: &Buffer, x: u16, y: u16) -> Option<(u8, Rgba)>;

/// One point of divergence between two rendered buffers, at braille-dot
/// granularity.
pub struct DotMismatch {
    pub cell: (u16, u16),  // column, row in the buffer
    pub dot: (u8, u8),     // (0..2, 0..4) position within the cell's 2x4 block
    pub expected_lit: bool,
    pub actual_lit: bool,
    /// `None` when either side reports this dot unlit — there's no color
    /// to compare in that case.
    pub expected_color: Option<Rgba>,
    pub actual_color: Option<Rgba>,
}

pub struct DotDiff { pub mismatches: Vec<DotMismatch>, pub dots_compared: usize }
impl DotDiff {
    pub fn is_match(&self) -> bool { self.mismatches.is_empty() }
}

/// Decodes every cell of `expected` and `actual` via `decode_braille_cell`
/// and compares dot-for-dot, literally — every one of the 8 dots in every
/// cell, not a cell-level or string-level comparison. Buffers of differing
/// size are compared over their shared (min width, min height) region;
/// every dot outside that region on the larger buffer is reported as a
/// mismatch (content one side has that the other doesn't).
pub fn diff_dots(expected: &Buffer, actual: &Buffer) -> DotDiff;
```
Operates on `ratatui::Buffer` — the actual on-screen output every scene's `render()` produces — rather than a raw pre-collapse `DotBuffer`, per *Decisions* on output granularity above: this is both the thing worth snapshotting for a golden/regression check, and as precise as the pipeline itself ever gets past compositing (one color per cell, per `13-rendering`). Lives in `engine-render`: a generic braille-buffer-diffing utility is exactly the kind of cross-cutting mechanism any future game on this engine would also want for golden-test validation, not Agent-Battleground-specific.

### Consolidating existing buffer-inspection test helpers
`crates/game/src/scenes/test_util.rs` already exists and already deduplicated this file's render/event test helpers (`render_to_buffer`, `key_event`, `mouse_event`, `has_non_space`) but missed the buffer/braille-inspection ones. This spec finishes that consolidation: `rect_text`, `braille_mask`, `sample_fg`, and `region_cells` move from their 5/2/2/1 private per-test-module copies in `roster_manager.rs` into `test_util.rs` as single shared functions, with `braille_mask` rebuilt as a thin wrapper over the new `decode_braille_cell` rather than its own separate hand-rolled decode. Purely a test-code refactor — doesn't touch what any test asserts, so it's orthogonal to the lossless-refactor promise about Roster's actual rendered output.

### Debug cell-boundary gridline overlay
Braille cells are 2 dots wide × 4 dots tall (8 is the dot *count* per cell, not a spacing value on either axis) — the overlay marks every dot at `x % 2 == 0` (left edge of each cell) and every dot at `y % 4 == 0` (top edge of each cell), directly on the composited output.

Rather than a flat darken (which is invisible wherever the underlying pixel is already dark — most of any terminal's background, most of the time), the overlay uses **adaptive contrast**: for each boundary dot, read the underlying color (or an assumed black for a currently-transparent dot, so the grid is visible over blank background too), compute its luma (`dots.rs`'s existing `luma()` function), and blend it toward black if it's already bright or toward white if it's already dark, by a fixed `GRID_CONTRAST_BLEND` fraction (`0.25`, matching the originally-proposed 25% figure, applied adaptively rather than in one fixed direction). This guarantees the grid line is visible against any content or background, always.

Toggled by a single new global keybinding, applied as a final post-composite pass over the whole terminal `Buffer` after any scene's `render()` runs — every current and future scene gets it automatically; no scene opts in individually, per this project's "inherited by default, not opt-in" rule for cross-cutting mechanisms. The grid-drawing pass itself lives in `engine-render` (cross-cutting, not game-specific); exactly which existing module captures the toggle keypress (this codebase has no prior runtime debug-toggle pattern to slot into — the debug inspector is a separate out-of-process egui tool over IPC, confirmed not reusable here) is an implementation detail for the research phase, resolved against wherever top-level input dispatch already lives.

### Where it lives
`crates/engine/render` — per `CLAUDE.md`'s engine/game boundary rule, this is a cross-cutting rendering mechanism any future game built on this engine would also need, not Agent-Battleground-specific content. Not an open question, confirming per the needs-research doc.

## Out of Scope
- Any new visual/layout decision for Roster beyond reproducing its current, already-`38`-corrected appearance exactly — this is a computation refactor, not a redesign. A genuinely desired future change to how Roster looks is a separate spec.
- Migrating any scene *other than* Roster onto `flex()` (e.g. `main_hub.rs`) — no other scene is touched by this spec.
- Deprecating or replacing `anchor`/`stack` as a general policy — permanent coexistence outside Roster, per *Decisions* above.
- A recursive `Node`/tree/builder layout API — v1 is flat `flex()` calls composed by feeding one call's output into another's `container`.
- `Row`-reverse / `Column`-reverse (RTL-style reversed main-axis) direction variants.
- Wiring ratatui's actual `Layout::split` solver internals — only its constraint *vocabulary* informed naming; the dot-native solver is custom, per *Decisions* above.
- Procedural dot-drawing helpers (bordered boxes, thin lines — `draw_dot_box`, `draw_dot_border`, `draw_dot_cap_box`, `battle_viewer.rs::draw_board_lines`). This spec computes *positions and sizes* only; how content is painted within a `flex()`-computed `DotRect` is unchanged and out of scope here, even though it's currently duplicated per-scene and would benefit from similar consolidation.
- Sub-cell RGBA / translucent-alpha compositing — unrelated, inherited constraint from `13-rendering`, untouched.
- The out-of-process egui debug inspector (`14-scene-architecture`/`15-debug-inspector`) — a separate tool over IPC, unrelated to and untouched by the new in-terminal debug gridline overlay. The exact key chosen for the new toggle is a low-stakes implementation detail, not pinned here.

## Validation
Two tiers: primitive-level (independent of any one consumer) and the Roster migration itself (the real-world proof).

### Tier 1 — primitive-level, in `engine-render`'s own test module
Synthetic test scenarios, each directly re-deriving one concrete pain point from the needs-research doc — kept independent of Roster specifically so a future second consumer has a spec/test suite to read, not just "however Roster happened to use it":
- **Top-align within an available band**: a fixed-height child in a taller container with `align_items: Start` lands flush at the container's near edge, with a `flex-grow` sibling absorbing the remainder — the "top-align the stat bars within their available band" case.
- **Cross-axis centering against a computed sub-region**: two nested `flex()` calls — an inner call producing a group of unevenly-sized children (mirroring dot-cluster + label widths), an outer call centering a third element (mirroring the flanking arrows) against the *inner call's actual returned bounding box*, not the outer container's full width.
- **Edge-anchored resize**: a `Fixed` sibling pinned at the container's far edge (`Align`/`Justify::End`) plus one `grow: 1.0` child immediately before it — shrinking the fixed sibling's size (simulating "grow the sprite, keep the baseline fixed") never moves the far-edge sibling, only the grow child's near edge moves.
- **Gap combined with non-`Start` justification**: `justify_content: Center` (or `SpaceBetween`) with a nonzero `gap` produces the exact expected spacing between every adjacent pair, at multiple container widths — the "more gap between role clusters" case, generalized.
- **Solver invariants** (property-style, not tied to a specific scenario): the sum of returned child extents plus gaps never exceeds the container's main-axis extent unless every `shrink` is `0.0`; grow distribution is exactly proportional to weight for at least 3 children with distinct weights; shrink distribution is exactly proportional to `shrink * basis` for an over-committed container; `Intrinsic` is invoked exactly once per child per `flex()` call.
- `DotRect::to_cell_rect()`/`cell_remainder()` round-trip: `to_cell_rect()` composed with `cell_remainder()` reconstructs the original `DotRect` exactly, for a range of dot positions including non-cell-aligned ones.
- `DotRectTween` mirrors `RectTween`'s existing test shape (hits endpoints, delegates to `Tween` per field, monotonic slide-in composition with `flex()` output as the resting target) at dot instead of cell precision.
- `decode_braille_cell` round-trips a known dot pattern correctly (every one of the 8 bit positions, both isolated and combined) and returns `None` for a blank cell.
- `diff_dots` reports zero mismatches for two identical buffers, reports the exact expected mismatch set (cell + dot position) for a buffer with a deliberately altered single dot and a deliberately altered single cell's color, and correctly reports every out-of-bounds dot as a mismatch when comparing buffers of different sizes.
- The debug gridline overlay: every boundary dot (`x % 2 == 0` or `y % 4 == 0`) is modified in the expected direction (lightened if the underlying/assumed color was dark, darkened if it was light) for both a fully-blank buffer (proving it paints over transparent background, not just existing content) and a buffer with existing bright content at a boundary position.

### Tier 2 — Roster migration acceptance
- `roster_manager.rs`'s full existing test suite passes, per the lossless-refactor rule in *Decisions*.
- `ARROW_NUDGE_DOWN_DOTS`, `HOME_NUDGE_UP_DOTS`, `DOT_SLOT_DOWN_DOTS` are deleted from the file (or the implementation records why one couldn't be, as a real finding).
- **Golden fixtures + `diff_dots`, the enforced deterministic gate**: before any migration code is written, the pre-`flex()` implementation is rendered at a fixed, representative set of scenarios (informed by the existing test suite's own scenarios — at minimum: rest state at both 40- and 80-column widths, a non-zero `current_index`, and one mid-slide-transition frame) and each `Buffer` is committed as a fixture. After migration, the same scenarios are re-rendered and `diff_dots(fixture, actual).is_match()` is asserted for every one — the deterministic replacement for a manual eyeball check, and what CI actually enforces. A manual render pass is still done once, as a first confirmation before committing the fixtures themselves (per this project's visual-work verification discipline — passing tests alone don't establish "looks right," and a fixture captured from an already-wrong render would just enshrine the wrong result).

## Dependencies
- `26-screen-space-positioning` ✅ / `28-anchor-margin-support` ✅ — `anchor`/`stack`/`anchor_with_margin`/`RectTween`, which this coexists alongside outside Roster per *Decisions* above; `RectTween`'s shape is what `DotRectTween` mirrors.
- `13-rendering` ✅ — the dot pipeline (`DotBuffer`, `Dot`, `dots_to_grid`) `flex()`'s dot-unit output composes with directly; `to_cell_rect()` is the boundary back to the `Grid`/`draw_grid` cell-space this pipeline already outputs into.
- `specs/needs-research/flex-style-layout-primitive.md` — the research doc this spec resolves; motivated by `35-roster-screen-stats-abilities-squad` / `38-roster-screen-layout-corrections`.
- `35-roster-screen-stats-abilities-squad` / `38-roster-screen-layout-corrections` — the shipped, already-corrected Roster layout this spec re-implements losslessly; not extended or changed in intent, only in how it's computed.
