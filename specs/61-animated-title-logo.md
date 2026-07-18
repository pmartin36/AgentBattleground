# Animated Title Logo — Sword in the Stone

## Purpose
Replace the main hub's static bundled PNG title (`assets::LOGO`, drawn in
`main_hub.rs`) with a procedural, animated **AGENT BATTLES** logo: the word
AGEN in white, the **T rendered as a sword** that drops and slots into a stone
slab reading BATTLES. It plays once when the hub opens, then holds on the
finished still. This is the first thing the player sees each session and is
meant to set tone.

The full still and animation were designed and locked in the reference
prototype `experiments/title_logo/` (analogous to how `13-rendering` references
`experiments/ascii_test/`). That prototype is the source of truth for exact
geometry, palette, and timing; this spec captures the decisions and the port
into the game.

## Scope
- A procedural sword-in-stone logo composited entirely in the braille dot
  pipeline — every glyph (AGEN, BATTLES) is braille dot art, no ASCII, honouring
  the "braille is universal except text" invariant.
- A play-once intro animation (sword fall → seat → BATTLES ignite → sparkles),
  driven by the hub scene's `update(dt)`/`render`.
- A single **vertical-offset** knob to shift the whole logo down, and **per-beat
  timing** (each beat's own start/end) exposed as named constants so every bit
  is retimed independently — all easy one-line tuning points.
- Porting the prototype's widened **N** and redrawn **G** glyphs into the shared
  game font `braille_name.rs`.
- Replacing (and deleting) the PNG logo in the main hub with this.

Out of scope:
- A separate splash/intro scene (project owner chose: animate in the hub).
- Sound (sword `shing`/impact clang) — future, via `57-engine-audio-api`.
- Any change to the engine dot pipeline — the logo only *calls* the public API
  (`DotBuffer`, `dots_to_grid_tinted`, `draw_grid`, `Rgba::lerp`).
- Onboarding first-run title treatment (`01-onboarding-first-run`).

## Decisions (v1)

- **Decision 1 — Lives in the main hub, replacing the PNG.** `main_hub.rs`'s
  title box renders this procedural logo instead of `assets::LOGO`. The hub
  scene gains an animation clock (accumulated `elapsed` in `update(dt)`) and is
  no longer fully static for the first ~0.9s after entry. Project owner's call
  over a dedicated splash scene. The bundled `assets::LOGO` PNG and its asset
  declaration are **deleted** — this procedural logo fully replaces it (no PNG
  fallback).

- **Decision 2 — The locked still.** AGEN in white bold dot-font; the **T is a
  sword** whose hilt (brown grip + gold pommel + gold **guard = the T
  crossbar**, floating a "neck" above the caps so it clears the N) sits above
  the caps and whose **beveled steel blade is the T's stem**, dropping through
  the word and wedging a short slot into the top of a **textured gray stone
  slab**. BATTLES is emblazoned across the slab in a saturated gold
  (`#FFD848`), snapped to the cell grid so it renders crisp. Exact dot
  geometry, cross-sections, stone texture (cell-level light/dark patches +
  sparse explicit black-hole grain + rough hewn edges + a thin crack), and the
  full palette are as locked in `experiments/title_logo/` and ported verbatim.

- **Decision 3 — Fixed size; small-terminal handling deferred.** The logo is a
  fixed `SCALE = 4` (letter shape 7 dots × 4; whole logo ≈ **94 × 24 cells**),
  placed **dot-precisely** (the `DotRect` threaded unfloored from composition to
  `draw_grid`, per the alignment invariant — never round-tripped through a
  cell-floored `Rect`). It is NOT scaled to fit. On terminals too narrow for it
  the logo simply overflows/clips for now — the game has a broader
  small-resolution problem to solve holistically, and this logo is not the place
  to special-case it. No `MIN_COLS` logic in v1.

- **Decision 3b — One vertical-offset knob; sword always starts off-screen.**
  A single constant shifts the whole composition **down** within the title
  area. The fall's start position is *derived* from the final seated position
  plus this offset plus the sword's own height, so the sword always begins fully
  **above the visible top edge** no matter how far down the logo is shifted —
  moving the logo never leaves the sword starting mid-screen.

- **Decision 4 — Animation beats + timing (play once, then hold).** Driven by
  scene elapsed seconds; ~0.9s total:
  1. **Fall** `0.00–0.18s` — the full sword (tip included) accelerates
     (ease-in) from just above the top edge and slots into the T; the blade is
     clipped at the stone surface so the tip buries as it seats.
  2. **Impact** at `0.18s` — a crisp **1-cell (2-dot) horizontal** rattle
     (even-dot shift preserves glyph grid-alignment, so nothing smears) that
     decays over ~0.12s, plus a **dust** puff rising from the entry (~0.20s).
  3. **BATTLES ignite** `0.18–0.42s` — each letter cross-fades (smoothstep)
     from a dark inset **etch** color (darker than the stone, so it reads as
     engraved before it lights) up to the gold glow.
  4. **Sparkles** — **two** white 4-point stars that grow-rotate-shrink,
     time-staggered so the second pops as the first fades: #1 upper-left of the
     blade at `0.46s` (spins one way), #2 lower-right at `0.61s` (spins the
     other). Then hold on the final still.

- **Decision 5 — Port the widened N and redrawn G into `braille_name.rs`.** The
  prototype widened **N** (5-wide, M/W-style diagonal — the base N was hard to
  read) and redrew **G** (spur pulled right so the bold smear stops welding it
  to the stem). These go into the shared game name font, so all consumers
  (creature names, post-battle band) inherit them. The font's proportional-
  width and any snapshot/golden tests are re-verified and re-baselined after
  the change; a visual pass confirms names still read well.

- **Decision 6 — Rendering path.** Composited into a `DotBuffer` of
  `Dot::Lit(Rgba)`; converted with `dots_to_grid_tinted(uniform_white_shape,
  color_buffer)` so every authored dot stays lit (the adaptive-luma rule in
  plain `dots_to_grid` culls darker dots and haloes the letters — a bug found
  and avoided in the prototype). Grain/holes are authored as explicit unlit
  dots. All of this is **game-side** content under `crates/game/`; nothing in
  `crates/engine/` changes.

- **Decision 7 — Plays once per app launch; no skip.** The intro runs **once
  when the experience is launched** — the first time the hub is shown in a given
  process — and does **not** replay when returning to the hub from another scene
  (roster / battle / etc.) in the same session. **Every fresh launch plays it
  again.** The gate is a **session-scoped, per-process flag** (e.g. a process
  `static` "intro played this launch" the hub checks-and-sets on first entry) —
  **not** a persisted first-run-ever marker and **no `first_run_complete`-style
  file** (the earlier persisted-first-run implementation was wrong for this and
  is replaced; any stray marker file is deleted). There is **no skip** control;
  the animation is short and unskippable by design. *(The first pass shipped a
  persisted first-run flag — a mis-spec — and also left a title-frame border
  around the logo; both are corrected by `63-title-intro-fixes`.)*

- **Decision 8 — Per-beat timing, independently tunable.** There is **no** global
  speed multiplier. Instead every beat is defined by its own **start and end**
  (seconds) as named constants in one block, so any single bit can be retimed
  without touching the others. The v1 values (from the locked prototype):

  | beat | start | end |
  |------|-------|-----|
  | sword drop | `0.00` | `0.18` |
  | impact shake | `0.18` | `0.30` |
  | impact dust | `0.18` | `0.38` |
  | BATTLES ignite | `0.18` | `0.42` |
  | sparkle #1 (upper-left) | `0.46` | `0.74` |
  | sparkle #2 (lower-right) | `0.61` | `0.89` |

  Each beat reads its own two constants; the sword-drop end is when it seats,
  which is also when shake/dust/ignite begin. Shifting, say, the twinkle later
  is a one-line change to its start/end and affects nothing else.

## Open Questions / TBDs
- **Small-terminal handling is deferred, not solved** (Decision 3) — it rides on
  the game's broader small-resolution effort, tracked separately.
- **Sound** — sword `shing` on the fall + a clang on impact are future work,
  captured in `specs/needs-research/sound-title-and-ui-sfx.md` and gated on
  `57-engine-audio-api`.

## Dependencies
- `02-main-hub-dashboard` — the scene this logo lives in and animates within.
- `completed/25-main-hub-navigation` ✅ — the hub scene structure/lifecycle the
  animation clock plugs into.
- `completed/13-rendering` ✅ — the braille dot pipeline (`DotBuffer`,
  `dots_to_grid_tinted`, `draw_grid`) the logo is built on; unchanged.
- `crates/game/src/braille_name.rs` — the shared name font that gains the
  widened N and redrawn G (Decision 5).
- `experiments/title_logo/` — the locked reference prototype for the still and
  animation (geometry, palette, timing).
- `57-engine-audio-api` — future sound (out of scope for v1).
