> # ✅ DONE! — Completed 2026-07-10
> Status: implemented via the tdd-pipeline (7 tasks GREEN, 0 escalations). `Button`/`FrameButton` collapsed into one builder-style `Button` — `new(rect, background)` plus `.icon()`/`.label()`/`.colors()`/`.dot_offset_down()` — with a per-state `ButtonColors`/`StateColors` scheme that recolors background, icon, and label together; `FrameButton` deleted. Every call site migrated (roster icon buttons, main-hub + Battle Menu text buttons); the Battle Menu "Finish Battle" button is the first per-state-colors consumer, carrying a real `FINISH_SCHEME`. `ButtonColors::default()` reproduces the prior appearance, enforced byte-for-byte by a `diff_dots` lossless gate against pre-migration fixtures rendered from a shared scenario module. Full workspace gate green on an independent re-run: 893 tests pass, `cargo clippy --workspace --all-targets -- -D warnings` clean.

# Unified Button Widget — `engine-render`

## Purpose

Collapse `engine-render`'s two button widgets into **one** `Button`, and give it a **configurable per-state color scheme** that recolors every layer — background, icon, label — together.

Two problems, one change:
1. **The split is fake.** `Button`'s `panel` and `FrameButton`'s `frame` are both just background image bytes; `Button`'s `icon` and `FrameButton`'s `label` are both just foreground content (one image, one string). Both wrap the same `ButtonCore` as flat siblings — neither is a kind of the other. The names describe the *asset* (solid panel vs hollow frame), not the widget, and `FrameButton` misleadingly reads as a subtype of `Button`.
2. **The label doesn't move with the button.** Today the background tints per `ButtonState`, but the label is pinned to one constant color, so a hover/press animates half the button. Making the label a per-state-colored layer — alongside the background and icon — fixes this by construction.

## Scope

- **One `Button` type** in `crates/engine/render/src/button.rs`, replacing both `Button` and `FrameButton`. `FrameButton` is deleted. `ButtonCore` (rect + state machine + hit-test) and `ButtonState` (Idle/Hover/Pressed) are unchanged.
- **Builder-style construction.** `Button::new(rect, background)` is the only required pair; icon, label, colors, and the sub-cell nudge are optional builder methods:
  ```rust
  Button::new(rect, background)          // rect + a background asset (&'static [u8])
      .icon(bytes)                       // optional foreground image
      .label("Finish Battle")            // optional text (impl Into<String>)
      .colors(scheme)                    // optional per-state color scheme
      .dot_offset_down(dots)             // optional sub-cell render nudge (today's Button::set_dot_offset_down)
  ```
- **A per-state color scheme** covering all three layers:
  ```rust
  /// Colors for one ButtonState. `background`/`icon` are multiply tints fed to
  /// `dots::tint` (same semantics as today's `ButtonState::tint_color()`);
  /// `label` is the absolute foreground color of the text.
  pub struct StateColors { pub background: Rgba, pub icon: Rgba, pub label: Rgba }

  /// Per-state color scheme for a Button. `Default` reproduces the current
  /// appearance exactly (see Decisions), so an un-`.colors()`-ed button is
  /// visually unchanged.
  pub struct ButtonColors { pub idle: StateColors, pub hover: StateColors, pub pressed: StateColors }
  ```
- **`render` composites in one pipeline**: background (state-tinted) → optional icon (state-tinted) → optional label (state-colored), reusing the existing `sprite_to_dots → composite_dots → tint → dots_to_grid_tinted → draw_grid` path and the `dot_down` nudge. A button with neither icon nor label renders just its background; a button with both renders both (permitted, though no current caller needs it).
- **Migrate every call site** off the removed constructors:
  - Roster's `home` / `left` / `right` — icon buttons → `Button::new(rect, panel).icon(...)`, keeping their `dot_offset_down`. Roster's `current_dot` uses `ButtonCore` directly and is untouched.
  - Main-hub's 3 buttons — text buttons → `Button::new(rect, FRAME_PANEL).label(...)`.
  - Battle Menu's "Finish Battle" — text button → `Button::new(rect, FRAME_PANEL).label(...).colors(<coordinated scheme>)`. **This is the spec's proof consumer**: the first button to opt into a custom scheme, and the reason the feature exists.
- **A committed test-support module** in `engine-render` holding the lossless-gate fixture **scenarios** — the exact per-state rects and background/icon/label asset-byte choices each fixture is rendered from — as shared constants/helpers. Referenced by *both* the fixture-capture step and the post-migration verify step so they render from byte-identical inputs by construction (not by re-spelling constants). This module is created and its fixtures captured **before any change to `button.rs`**, and it is a **prerequisite (explicit dependency) of the unified-`Button` implementation and every call-site migration** (see *Sequencing* and *Validation*).
- **`lib.rs` re-exports** updated: export `Button`, `ButtonCore`, `ButtonState`, `ButtonColors`, `StateColors`; drop `FrameButton`.

## Decisions (v1)

### One widget named `Button`
Removing the second type *is* the naming fix — no rename of a surviving type is needed. `Button` is the honest name for "a background asset + optional foreground content over a `ButtonCore`." Content (icon vs label vs both) becomes orthogonal options, not a type distinction.

### Builder for the optional parts
`new(rect, background)` takes only what every button must have. Icon, label, colors, and nudge are `self`-consuming builder methods returning `Button`, matching this codebase's value-semantics fluent style (`DotRect::inset` chaining). Avoids a constructor with a run of `Option` arguments and reads left-to-right at the call site.

### Per-state colors cover every layer, together
The scheme carries a color for the **background**, **icon**, and **label** per state. This generalizes the original label-only complaint into the real invariant: a button's whole appearance moves as one on hover/press. Background and icon are multiply tints (preserving current semantics); the label is an absolute color (text is drawn directly, not through `dots::tint`).

### `Default` is a lossless reproduction of today's look
`ButtonColors::default()` yields, per state, exactly what ships now:
- `background` tint = today's `ButtonState::tint_color()` (Idle `0xc8c8c8`, Hover `0xffffff`, Pressed `0x8c8c8c`),
- `icon` tint = today's icon tint path,
- `label` = today's `FrameButton::LABEL_COLOR`.

So every migrated existing button is byte-for-byte unchanged until it calls `.colors(...)`. Enforced, not asserted (see Validation).

### `ButtonState` stays the single source of interaction state
The state machine, transition table, and `tint_color()` constant are unchanged. `ButtonColors::default().*.background` simply mirrors `tint_color()`'s values so behavior is identical; a future cleanup could have `default()` call `tint_color()` directly, but that is not required here.

### Where it lives
`crates/engine/render/src/button.rs` — a reusable UI widget, per `CLAUDE.md`'s engine/game boundary. No game-specific asset bytes enter the engine; backgrounds/icons remain caller-supplied `&'static [u8]`.

### Sequencing: fixtures first, everything else depends on them
The lossless gate captures the **pre-migration** render as its oracle, so the fixture-scenario module and its captured fixtures **must be created and committed before any code in `crates/engine/render/src/button.rs` changes** — while the current `Button` (3-arg `new`) and `FrameButton` still compile. This is a hard ordering constraint, not a soft preference: capturing after the rewrite would render against the new API (compile failure) or capture post-migration output (a meaningless "regression" check that enshrines whatever the migration produced).

Therefore the fixture work is the **first, foundational unit of the whole feature**, and the unified-`Button` implementation *and every call-site migration* declare an **explicit dependency** on it. A decomposition MUST encode this as a real dependency edge in the task graph — file-touch write-serialization does not order them, because the fixture task touches `tests`/test-support paths while the rewrite touches `button.rs`.

## Out of Scope

- Renaming `ButtonCore` / `ButtonState` or any asset constant.
- Any **visual redesign** of the roster or main-hub buttons — the default scheme reproduces them exactly; only the Battle Menu button opts into new colors.
- New interaction states beyond Idle/Hover/Pressed, or keyboard/focus activation of buttons (mouse-driven, per current behavior).
- Icon-`Button` asset content, the `ui_primitives`/occlude work, or the Battle Menu's panel/layout (spec `05`).
- A recursive/nested layout or slot system for button content — one background, at most one icon, at most one label.

## Validation

### Unit (engine-render)
- Builder: an icon-only, a label-only, a both, and a custom-`.colors()` button each produce the expected internal config.
- `render`: background is always drawn; icon is drawn iff `.icon()` was set; label is drawn iff `.label()` was set (verified by decoding the rendered `Buffer`).
- Per-state coloring: driving the button Idle→Hover→Pressed changes the rendered background tint, icon tint, **and** label color, each to the scheme's value for that state.
- `ButtonColors::default()` returns the current per-state values named in *Decisions*.

### Lossless migration gate (reusing spec `40`'s `diff_dots`)

**Shared scenarios, referenced not re-spelled.** The inputs each fixture renders from — the rect, the background/icon/label asset bytes, and the `ButtonState` — are defined once in the committed test-support module (see *Scope*/*Sequencing*), and *both* the capture step and the post-migration verify step render by referencing those same constants. There is no throwaway generator that gets deleted, and no second hand-copy of the rects/assets; divergence between "what was captured" and "what is re-rendered to compare" is impossible by construction.

**Captured first, against the old code.** Per *Sequencing*, the fixtures are captured while today's `Button`/`FrameButton` still exist and committed before any `button.rs` change — capturing the *old* render is the entire point of the oracle.

**The assertion.** Capture a representative icon `Button` and text `FrameButton` at each of Idle/Hover/Pressed as committed fixture `Buffer`s. After migration, assert `diff_dots(fixture, actual).is_match()` for the default-scheme equivalents at every state — deterministic proof that collapsing the two widgets into one changed nothing visible. A one-time manual render confirmation precedes committing the fixtures (project visual-verification discipline: a fixture captured from a wrong render just enshrines it).

### Migration acceptance
- Roster's and main-hub's existing test suites pass unchanged.
- Battle Menu: its existing tests pass, plus a new test asserting the "Finish Battle" **label color differs between Idle and Hover** — the coordination this whole spec delivers.

## Dependencies

- `22-braille-ui-chrome` ✅ — the button render/tint contract and mouse-transition state machine this generalizes.
- `21-mouse-hover-input` ✅ — `ButtonCore` hover/press states driving the per-state colors.
- `24-roster-carousel` ✅ / `25-main-hub-navigation` ✅ — the `Button`/`FrameButton` call sites migrated here.
- `40-flex-layout-primitive` ✅ — `diff_dots`/`decode_braille_cell`, reused as the lossless-migration gate.
- `13-rendering` ✅ — the dot pipeline (`sprite_to_dots`/`composite_dots`/`tint`/`dots_to_grid_tinted`/`draw_grid`) the render path composes.
