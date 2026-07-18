# Menu Button Cleanup — Borders, Colors, Settings Button, Intro Fade-in

## Purpose
Polish to the main hub's nav buttons: fix the errant border dots, add a
**Settings** button + a (blank) Settings scene, recolor the buttons to match the
new title palette (`61-animated-title-logo`) — gold for inactive, white for the
active/selected button — and, on the first-run title intro, **hide the buttons
until the sword seats, then fade them in**.

## Scope
- Fix the button border: eliminate the **4 missing dots at the corners** and the
  **4 stray dots just outside** the border.
- Add a **Settings** nav button, second-from-bottom (order: Roster, Battle,
  Settings, Exit).
- Add a **Settings scene**: blank except a working **home button** (reuse the
  roster/post-battle home button), navigating back to the hub.
- Recolor buttons: **inactive = gold, active/selected = white** (border + label),
  matched to the title.
- On the **first-run title intro**, hide the nav buttons during the sword drop
  and **fade them in** once the sword seats (coordinated with `61`'s animation).

Out of scope:
- Any real Settings content (model config etc. is `09-settings-model-config`);
  this ships a blank shell.
- Changing other scenes' buttons beyond the shared widget/color change they
  inherit.

## Background (grounding)
- Hub buttons are `engine_render::Button` (`crates/engine/render/src/button.rs`),
  whose **border is a background PNG** — `assets::FRAME_PANEL` (`frame_panel.png`)
  rasterized by `sprite_to_dots` and **stretch-fit** to the button rect
  (`render_button`, `button.rs:~232`). The rounded corners are baked into the
  PNG, so the "4 missing corner / 4 extra outside" dots are artifacts of
  stretching a fixed raster to an arbitrary size — not a procedural bug.
  (Contrast: the *procedural* dotted border `ui_primitives::rounded_rect` — used
  by the roster Edit button, battle menu, tooltips — has its own corner-chamfer
  logic at `ui_primitives.rs:55-80`, but that is **not** what the hub buttons
  use.)
- Colors: the sprite is multiplied by `PANEL_GOLD_TINT (#c9a03c)` then a per-state
  tint (`Idle #c8c8c8`, `Hover #ffffff`, `Pressed #8c8c8c`); the label is a
  constant `#f0f0f0` across all states (`button.rs:26-72`). The widget has only
  mouse states (`Idle/Hover/Pressed`) — **no keyboard "selected" state**; the hub
  shows keyboard selection with a separate arrow icon and a `cursor_index`, not
  by recoloring (`main_hub.rs`).
- Hub button geometry is **hard-coded to 3**: `[Button; 3]`, `button_rects() ->
  [Rect; 3]`, `MENU_H = 3*BUTTON_H + 2*MENU_GAP` (`main_hub.rs:26,181,78`).
  `activate(index)` (`main_hub.rs:248`) is the single nav dispatch. Iteration,
  `cursor_index` wrap, render, and input already use `self.buttons.len()`.
- `SceneId::Settings` **already exists** (`scene_id.rs:15`) but is unimplemented:
  `registry.rs` `construct`/`schema_for` hit `unimplemented!()` and it's absent
  from `IMPLEMENTED_SCENES`. The home button is `home_button::draw_home_button`
  (`crates/game/src/scenes/home_button.rs`, `pub(crate)`) driven by a
  `ButtonCore`; roster wires "click home → `Transition` to `MainHub`"
  (`roster_manager/mod.rs:532`). `Leaderboard` (`scenes/leaderboard.rs`) is the
  minimal `Scene` skeleton to model a blank scene on.

## Decisions (v1)

- **Decision 1 — Draw the button border procedurally, dot-precise; drop the
  stretched PNG.** Replace the `FRAME_PANEL` sprite border on `Button` with a
  procedurally-drawn **dotted rounded border** placed dot-precisely (threading
  `DotRect` unfloored, never round-tripped through a cell-floored `Rect`), so
  corner dots are always present and no dot lands outside the button rect at any
  size. This is the durable fix — a fixed raster stretched to arbitrary button
  sizes will always drift a dot at the corners/edges. The dotted (dashed) look
  in the current art is preserved, now generated at exact dot positions.
  **Verification is by decoding the actual rendered dots**
  (`engine_render::decode_braille_cell` / `test_util` dot helpers), asserting: at
  each corner the expected dot is lit (no gap), and no lit dot exists outside the
  border rect — not by eyeballing or comparing `Rect` fields. The target look is
  the existing dotted rounded frame in the current buttons, just clean.

- **Decision 2 — Settings button, second-from-bottom.** Hub nav order becomes
  **Roster, Battle, Settings, Exit**. Bump the hub's hard-coded 3 → 4: the
  `[Button; 4]` array (add a `.label("Settings")` entry at index 2), `button_rects
  -> [Rect; 4]`, and `MENU_H = 4*BUTTON_H + 3*MENU_GAP`. `activate` gains
  `2 => Transition to SceneId::Settings`, and the quit arm moves to `3`. Update
  any hard-coded button count in `main_hub_tests.rs`.

- **Decision 3 — Blank Settings scene with a home button.** New `Settings` scene
  struct implementing `Scene`, modeled on `Leaderboard` but carrying a
  `RefCell<ButtonCore>` home button: `render` calls `home_button::draw_home_button`
  at the same top-right placement the roster uses; `handle_input`/mouse returns
  `Transition` to `SceneId::MainHub` on a home hit. Otherwise blank (a
  `fill_and_label`-style body is fine). Wire it up: `registry.rs` `construct`
  (`SceneId::Settings => Box::new(Settings::default())`), the matching
  `schema_for` arm, add to `IMPLEMENTED_SCENES`, import + export in
  `scenes/mod.rs`. Update the two registry tests that currently assume Settings
  is unimplemented (`registry.rs:118` `construct_unimplemented_scene_panics`, and
  `:295`).

- **Decision 4 — Inactive gold, active white; colors reused from the title.** A
  button is **active** when it is **either keyboard-selected (`cursor_index`) or
  mouse-hovered**; the active button renders its border and label in the title's
  **white** (`#ECEFF5`, the AGEN white), and every other button in the title's
  **gold** (`#FFD848`, the BATTLES glow gold). Both values are reused from
  `61-animated-title-logo`'s palette rather than newly chosen. This requires
  **driving button color from selection state**, which the widget currently
  lacks: the hub tells the button whether it is the active one (a
  `selected`/`focused` flag or a per-render `ButtonColors` override), since today
  only mouse `Hover` turns a button white and keyboard selection was arrow-only.
  Label color stops being a constant `#f0f0f0` and follows the same gold/white
  rule.

- **Decision 5 — Drop the selection arrow.** The left-of-button `ICON_ARROW_RIGHT`
  cursor indicator (`main_hub.rs` `cursor_rect` / render) is **removed** — the
  white active-button color now conveys selection, so the arrow is redundant.
  `cursor_index` stays (it still drives which button is active/white and where
  Enter navigates); only the drawn arrow goes away.

- **Decision 6 — Buttons hidden during the drop, then fade in.** During the
  **first-run title intro** (`61-animated-title-logo`), the nav buttons are
  **hidden while the sword falls** and **fade in** (opacity 0 → full) only after
  the sword **seats** (61's sword-drop end, ~`0.18s`). The fade is its own
  per-beat window `[BTN_FADE_START, BTN_FADE_END]` (default ~`0.30s`–`0.62s`,
  tunable one-liners in the same spirit as 61's per-beat timing), read off the
  hub's animation clock that `61` introduces. Implemented as an **alpha ramp** on
  the button colors (border + label, `a`: 0 → 255) composited via `draw_grid`'s
  translucent-glyph blending — not a color-lerp toward the backdrop, so it fades
  cleanly over whatever is behind. Buttons are **non-interactive until the fade
  completes** (no clicking an invisible button). On **non-first-run** hub entries
  (where `61` shows the held still, no animation), buttons appear immediately at
  full opacity — no fade. The Settings button (Decision 2) and the gold/white
  colors (Decision 4) fade in with the rest.

  Seam note: this depends on `61` having landed first — specifically the hub's
  `elapsed` animation clock and the sword-seat time constant. `62` reads those;
  it does not re-implement the title animation.

## Open Questions / TBDs
- Border, active-state, arrow, and palette opens are all resolved (procedural
  border; active = keyboard or hover; arrow dropped; palette reused from the
  title logo).
- **Fade window is a taste call** (Decision 6) — default `~0.30–0.62s` has the
  buttons rising during BATTLES' ignite; pushing it later (after the sparkles) is
  a one-line tune once we see it in the real hub.

## Implementation notes (decomposition constraints)
These guide the task breakdown so the pipeline's file-size and reuse checks pass:

- **Split `button_tests.rs` first.** `crates/engine/render/src/button_tests.rs`
  is already ~1088 lines — over the 1000-line file-size budget — and Decisions 1
  and 4 add border-decode and color/alpha test cases to the button suite. The
  decomposition MUST include a **dedicated, behavior-preserving split of
  `button_tests.rs`** (mechanically partitioned by concern — state machine /
  hit-test / render+color — into sibling files, no assertion changes) as its
  **own task in the first bucket**, with the new border/color/alpha test tasks
  **depending on it**. New tests land in the post-split files, never back into an
  over-budget one.

- **Share the home-button geometry — do not duplicate.** The Settings scene's
  home button (Decision 3) must reuse the roster's placement by **hoisting the
  shared helper** (`home_dot_rect` / `home_rect`, currently private in
  `roster_manager`) to a location both scenes call — not by copying the
  constants. The two home buttons must stay aligned by construction; a duplicated
  constant drifts the moment either side is retuned.

- **Keep `main_hub_tests.rs` under budget.** It is ~775 lines and several tasks
  add cases; removing the arrow-cursor test (Decision 5) frees room, but keep the
  additions lean (split the file if it would cross 1000) so it does not silently
  breach.

## Dependencies
- `completed/45-button-widget-unification` — the `Button` widget whose border and
  color logic this changes.
- `completed/25-main-hub-navigation` / `02-main-hub-dashboard` — the hub nav this
  extends (Settings button + 4-button geometry).
- `61-animated-title-logo` — the palette (gold/white) the buttons align to, **and**
  the hub animation clock + sword-seat time that Decision 6's button fade-in reads
  from. **62 must build after 61 lands** (it consumes 61's hub animation state).
- `completed/13-rendering` — the dot pipeline + `decode_braille_cell` the border
  fix is verified against.
- `09-settings-model-config` — the real Settings content this blank shell is a
  placeholder for (future).
