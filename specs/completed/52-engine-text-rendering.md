> # ✅ DONE! — Completed 2026-07-13
> Status: implemented via the tdd-pipeline, shipped to `main`.

# Engine Text Rendering

## Status

Done (shipped to `main`). Replaces the engine's single centered `label` helper with an **alignment- and style-aware** text API, and adds a **wrapped/ellipsized** multi-line helper for static display text. Engine-level and reusable. Foundational to the roster detail panel (`48`) and ability tooltip (`49`) despite its higher number — same situation as `40-flex-layout-primitive`, a late-numbered foundational primitive.

## Purpose

`engine_render::label` today draws exactly one **centered** line and sets only a **foreground color** — no left/right alignment, no text-style modifiers (underline, bold…), no wrapping. The roster redesign needs left-aligned section headers, left-aligned **underlined** ability names, and wrapped-with-ellipsis body text. Rather than scatter bespoke `Buffer` writes across game scenes, expand the engine text API so every current and future caller inherits alignment + style, and **migrate the existing callers onto it** so there is one text path, not two that drift.

## Scope

- Replace `label` with an alignment + `Style` aware signature; migrate all existing call sites.
- Add `wrapped_text` — a multi-line word-wrap + optional tail-ellipsis helper for static display text.

**Out of scope:** the interactive editor's text rendering (`50` owns its own per-line draw, soft-wrap, cursor, and scroll — editors **scroll, they don't ellipsize**, so `50` does not use `wrapped_text`); Markdown rendering; rich runs (multiple styles within one line).

## API

In `crates/engine/render` (extend `lib.rs` or a `text.rs` submodule re-exported from it).

```rust
pub enum TextAlign { Left, Center, Right }
```

**Single-line — the replacement for `label`:**
```rust
pub fn label(buf: &mut Buffer, area: Rect, text: &str, align: TextAlign, style: Style);
```
- Draws one line, placed horizontally per `align` within `area`, vertically centered (as today).
- `style` is a ratatui `Style`, so it carries the fg color **and** any modifiers (`UNDERLINED`, `BOLD`, …). A color is still required (the existing `Color::Reset` illegibility caveat stands).
- Hard-truncates to `area` width, no wrap (unchanged).

**Multi-line wrapped display text:**
```rust
pub fn wrapped_text(buf: &mut Buffer, area: Rect, text: &str, align: TextAlign, style: Style, ellipsis: bool);
```
- Word-wraps `text` to `area` width (breaking an over-long token by character), top-down, one wrapped row per line, each row aligned per `align`.
- Clips to `area` height. When `ellipsis` is set and the text overflows the available rows, the **last visible row ends with `…`** — a single tail ellipsis for the whole block, not per line.
- Callers: the details-panel instructions preview (`48`, `Left`, `ellipsis: true`) and the tooltip flavor block (`49`, capped to its 2-row area).

## Migration

Every existing `label(buf, area, text, color)` becomes `label(buf, area, text, TextAlign::Center, Style::default().fg(color))` — behavior-identical for the ~dozen current call sites (all centered, color-only). No parallel legacy helper is kept; there is exactly one `label`. Centered output must be byte-identical after migration, so roster/post-battle golden fixtures should not change — regenerate only if a diff appears, and treat any diff as a migration bug to investigate, not rubber-stamp.

## Decisions (v1)

- One text path: `label` gains `TextAlign` + `Style`; **all callers migrate**, no old/new split.
- Style is a full `Style` (not a bare color) so underline/bold flow through the same call.
- `wrapped_text` is the shared static-text wrap/ellipsis helper; the editor (`50`) does **not** use it.
- No rich (multi-style) runs within a single line this pass.

## Testing Guidance

- `label` with `Left`/`Center`/`Right` places text at the expected x within `area` (decode rendered cells).
- `label` with an `UNDERLINED` style yields underlined cells (decode the cell `Style`).
- A migrated centered call is byte-identical to the old output (spot-check against a fixture).
- `wrapped_text` wraps a long paragraph to the expected row count; `ellipsis: true` + overflow ends the last visible row in `…`; a fitting block has none.
- Alignment is applied per wrapped row.

## Open Questions / TBDs

None outstanding. Rich multi-style runs are deferred (not currently needed).

## Dependencies

- Builds on the ratatui `Buffer`/`Style` text layer and the existing `label` (`13-rendering` ✅).
- Consumed by `48-roster-detail-panel-redesign` and `49-ability-hover-tooltip`. Independent of `50`/`51`.
