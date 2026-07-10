# Post-Battle Results Screen

## Status

Pending. Implementable slice of `06-post-battle-upgrade.md` — this spec covers **only the results screen UI** shown immediately after a battle (outcome banner, per-creature progress columns, spoils row) plus the two data-model changes that screen needs. The upgrade flow, defeat debrief content, and replay upload remain in `06`.

## Purpose

The screen the player lands on when a battle ends (reached today via the Battle Viewer's "Finish Battle" button → `SceneId::PostBattle`). It shows the outcome, each participating creature's progress (level, XP gained, remaining stamina), and the spoils earned from the fight. It is the visible front end of the progression loop; the actual upgrade/assignment mechanics build on top of it later.

The scene is currently a bare placeholder (`crates/game/src/scenes/post_battle.rs` fills a solid color + label). This spec replaces that with the real screen.

## Reference Mock

Top-to-bottom: a large "VICTORY" banner, a row of 4 creature columns (portrait → level → XP bar → stamina bar), and a bottom "SPOILS" band with a left label and a horizontal row of bordered spoil items (icon + description).

## Data-Model Changes

These are prerequisites and land as part of this spec (a complete, buildable unit).

### 1. Add XP to `Creature`

`crates/game/src/creatures.rs`:
- Add field `xp: u32` to `Creature` (persistent progress toward the next level), a `with_xp(u32)` builder, and an `xp()` getter. Default `0` in `Creature::new`.
- Add a module constant `pub const XP_TO_NEXT_LEVEL: u32 = 100;` — a **shared placeholder cap**, the same for every creature, **never incremented on rollover** this pass (per-creature / level-scaled thresholds and the actual level-up are deferred; see Open Questions).
- In `demo_roster()`, seed distinct placeholder `xp` values on the first four entries such that, combined with the scene's placeholder gained amounts (below), **at least two of the four columns cross `XP_TO_NEXT_LEVEL` and visibly roll the bar over**, and at least two do not.

There is **no** persistent "gained this battle" field on `Creature`. The gained delta is transient battle output; until the battle engine is wired, it lives as placeholder scene state (see Scene State). If/where it later needs a home is out of scope.

### 2. Rename `Exhaustion` → `Stamina` (with inverted semantic)

The stored meter is renamed and its numeric sense inverted, so the value the code carries *is* stamina remaining (avoids a permanent "stamina = 100 − stored" trap and matches this screen's label).

- `crates/game/src/exhaustion.rs` → `crates/game/src/stamina.rs`; type `Exhaustion` → `Stamina`; module decl in `lib.rs` updated.
- Semantics flip: `percent: u8` now means **stamina remaining, 0..=100**. `Stamina::default()` is **100** (fully rested). The **injured** state is entered when `percent` reaches **0** (was: 100).
- Transitions become drains that **subtract**: `apply_damage_exhaustion` / `apply_ability_use_exhaustion` → `drain_from_damage(amount)` / `drain_from_ability_use(cost)` (subtract, clamp at 0, set `injured_until` when it hits 0). Recovery restores toward 100.
- `Creature`: field/builder/getter `exhaustion`/`with_exhaustion`/`exhaustion()` → `stamina`/`with_stamina`/`stamina()`.
- `crates/game/src/scenes/roster_manager/details_panel.rs`: `exhaustion_text`/`render_exhaustion`/`exhaustion_rect` and the `"Exhaustion: {percent}%"` string → `"Stamina: {percent}%"` (the injured "days remain" line is unchanged in wording). This also updates the roster preview golden fixtures under `crates/game/tests/fixtures/roster/` — regenerate them.
- Update all tests referencing the old names/semantics (e.g. a creature built with `drain_from_damage(42)` now reads `58%`, not `42%`).

The threshold **coloring** below is scoped to this scene's stamina bar only; the roster's stamina text keeps its existing style.

## Layout

Screen split top-to-bottom (all chrome via the dot pipeline per CLAUDE.md rule 4):

1. **Title band** — height = the minimum to fit the braille glyphs (`braille_name::GLYPH_H` = 8 dots = 2 cells) **plus a 1-cell margin above and below** (≈ 4 cells). `braille_name::draw_name(buf, band, "VICTORY", TITLE_COLOR)` (auto-centers). A **Home button** (reuse `engine_render::Button` + `assets::ICON_HOME` + `assets::BUTTON_PANEL`, exactly as RosterManager) sits inset in the **top-right corner**, drawn over the band (the centered title leaves the corner free).
2. **Creature area** — the vertical space between the title and spoils bands.
3. **Spoils band** — the bottom **25%** of the screen height.

### Creature area

4 columns evenly dividing the width (a Row `flex` over `DotRect` with a small inter-column gap). Reserve a **1-dot margin around every portrait** for the selection glow ring, so columns keep a consistent layout whether or not selected.

Each column is a Column `flex`, top-to-bottom:
- **Portrait** (grows to fill): a chamfered box frame (`ui_primitives::rounded_rect`, `thickness: 1`, `corner_radius: 2`, transparent fill) around the creature's **idle animation**. Render the sprite with the RosterManager idiom: `creature.animation(AnimationKind::Idle)` → aspect-fit within the dot-precise inner rect (bottom-pinned, horizontally centered) → `sprite.dots_at(self.elapsed, w, h)` → sub-cell placement. The loop advances because `update()` accumulates `dt` into `elapsed`.
- **Level line** (1 cell): plain text `"LVL {n}"` via `engine_render::label` (`n = creature.level()`).
- **XP row**: a short `"XP"` label + a horizontal bar (label fixed-width, bar grows). See XP Bar.
- **Stamina row**: a short `"STA"` label + a horizontal bar. See Stamina Bar.

Labels are plain terminal text (rule 4). In narrow columns they truncate via `label`.

### Spoils band

- Left: plain-text `"SPOILS"` label.
- Right: a horizontal Row `flex` of the spoil items (this pass: **2**), with a gap.
- Each spoil item: a chamfered bordered box (`ui_primitives::rounded_rect`, transparent fill) containing a **candy icon** on the left (`engine_render::draw_asset` with `assets::ICON_SPOIL_CANDY`) and a **description** on the right (plain text). Description format is per-spoil-type and **deferred** — use placeholder strings (`"Spoil 1"`, `"Spoil 2"`) for now.

## XP Bar

Model on RosterManager's stat bar (fraction → lit dot-columns; border and fill in separate braille cells so the border stays crisp at any fill; `Tween` off `elapsed`). Fill color `XP_BAR_COLOR`.

Animation, per column `i`:
- `start = creature.xp()`, `gained = xp_gained[i]` (placeholder scene state), `end = start + gained`.
- A value `v` eases `start → end` over `XP_ANIM_DUR` (≈ 1.2 s) via `Tween`, keyed off `elapsed`. It plays **once on entry** (because `enter` resets `elapsed` to 0) and rests at `end` afterward.
- Displayed fill fraction = `(v mod XP_TO_NEXT_LEVEL) / XP_TO_NEXT_LEVEL`. When `v` crosses a multiple of `XP_TO_NEXT_LEVEL` the bar **rolls over** (fills to full, wraps to empty, keeps filling). The displayed **level number is not incremented** this pass, and `XP_TO_NEXT_LEVEL` is not changed — the level-up moment ("extra UI work") is a separate future spec.

## Stamina Bar

Same bar structure, **static** (no animation this pass — there is no live combat trigger). Fill fraction = `creature.stamina().percent() / 100`. Fill **color by remaining percent**:
- `> 50` → green
- `15..=50` → yellow
- `< 15` → red

`demo_roster()` gives the four shown creatures **distinct** stamina percents spanning all three bands (e.g. 80 / 45 / 60 / 10) so every color path renders.

## Selection Glow

A `selected_index: usize` marks one creature; **hardcoded to `0`** this pass (no selection interaction yet — spoils are visual-only). The marker is a **slowly pulsing "shadow" ring** one dot outside the selected portrait's frame border:
- A 1-dot ring of `Dot::Lit` around the frame's outer perimeter (dot pipeline), drawn only for the selected column.
- Its color **pulses** over `elapsed` between a dim and a brighter shade (lerp of `TITLE_COLOR` toward background / toward bright), on a slow period (≈ 1.0 s), **quantized into a few discrete steps** (≈ 4–6) so it reads as "every couple of frames" rather than a perfectly smooth fade.

## Outcome

Add `enum Outcome { Victory, Defeat }`; the scene stores it, **hardcoded to `Victory`** (battle outcome is not wired). Only the title text/color branch on it: Victory → `"VICTORY"` in `TITLE_COLOR` (amber `0xffbf00`, the scene's existing `COLOR`); Defeat → `"DEFEAT"` in `DEFEAT_COLOR` (a TBD red constant). The full Defeat layout/debrief stays in `06` — this pass only needs the banner to switch cleanly.

## Scene State (`PostBattle`)

Replace the unit struct. Fields (all non-scalars `#[inspect(hidden)]`, per the RosterManager convention):
- `outcome: Outcome` (hardcoded `Victory`)
- `creatures: Vec<Creature>` — `creatures::demo_roster()` truncated to the first 4 (the 3 Active + 1 Bench that fight; battle→squad wiring is deferred, so this placeholder is the only real source)
- `xp_gained: [u32; 4]` — placeholder per-column gained deltas (chosen with the seeded `xp` so two columns roll over)
- `elapsed: Duration`
- `selected_index: usize` (0)
- `spoils: Vec<Spoil>` — 2 placeholders, where `struct Spoil { icon: &'static [u8], description: String }` (both candy for now)
- `home_button: RefCell<engine_render::Button>`

Trait impl:
- `enter`: (re)build `creatures`/`xp_gained`/`spoils`, reset `elapsed = 0` so the XP fill and glow restart each visit.
- `update`: `elapsed += dt`.
- `render(&self, ...)`: title band + home button; 4 columns; spoils band. Widgets use the set-rect-then-render dance (`set_rect`/`set_dot_offset_down`/`render`).
- `handle_input`: mouse → home button `handle_mouse`; on hit return `Transition { target: SceneId::MainHub.into(), .. }`. `Esc` also → `MainHub`.
- `inspect`: return `self`.

## Assets

- Bundle `crates/game/src/assets/icon_spoil_candy.png` (already created: 64×64, transparent background) as `pub const ICON_SPOIL_CANDY: &[u8]` in `crates/game/src/assets.rs`, following the existing `include_bytes!` + decode-test pattern.

## Constants (placeholders — tunable)

`XP_TO_NEXT_LEVEL = 100`, `XP_ANIM_DUR ≈ 1.2s`, spoils band `25%`, glow period `≈ 1.0s` / `~5` steps, title `TITLE_COLOR = 0xffbf00`, `DEFEAT_COLOR` TBD red, `XP_BAR_COLOR` TBD, stamina green/yellow/red TBD.

## Testing Guidance

Verify **rendered behavior**, not just structure (CLAUDE.md). Where two dot elements' alignment matters, decode actual dots (`engine_render::decode_braille_cell`; reuse `scenes/test_util.rs` helpers) — never compare `Rect`/`DotRect` fields alone.

- Title: `"VICTORY"` paints lit cells in the title band; switching `Outcome::Defeat` paints `"DEFEAT"`.
- Each of the 4 columns renders a non-empty portrait, a `"LVL n"` line, an XP bar, and a stamina bar.
- Stamina color: a creature at 80/45/10% yields the green/yellow/red fill respectively (decode the fill color).
- XP rollover: for a column whose `start + gained > XP_TO_NEXT_LEVEL`, sampling the fill mid-animation shows it near full then reset toward empty, and at rest the fraction equals `(end mod XP_TO_NEXT_LEVEL)/XP_TO_NEXT_LEVEL`; the level number is unchanged.
- Selection glow: the ring is present around column 0 and absent around the others; two `elapsed` samples a half-period apart give different ring colors (proves the pulse).
- Spoils: exactly 2 boxes, each with candy-icon cells and a description string.
- Home button click and `Esc` each return a `Transition` to `MainHub`.
- Data model: `Stamina::default().percent() == 100`; a drained stamina reads the reduced value and hits injured at 0; roster details renders `"Stamina: {n}%"`; `Creature::xp()`/`with_xp` round-trip.

## Out of Scope / Open Questions (deferred)

- Real spoil types, descriptions, icons, and the **spoil → creature assignment** interaction (this pass is visual-only; `selected_index` is fixed).
- The **level-up moment**: what the UI does when XP rolls the bar over (the deferred "extra UI work"); whether `XP_TO_NEXT_LEVEL` becomes per-creature / level-scaled; where `xp_gained` ultimately comes from (battle engine) and whether it persists on `Creature`.
- Full **Defeat** layout + debrief content (stays in `06`).
- **Replay** finalization/upload (stays in `06`).
- Wiring the columns to the **actual battle squad** instead of `demo_roster()` (needs battle→roster data plumbing; see `36`).
- Balancing the placeholder numbers (`XP_TO_NEXT_LEVEL`, stamina bands, recovery).

## Dependencies

- `06-post-battle-upgrade.md` — the parent design this slices; upgrade flow / defeat debrief / replay remain there.
- `34-creature-attributes-data-model.md` — the `Creature` shape this extends (XP field, Stamina rename).
- `completed/13-rendering.md`, `completed/22-braille-ui-chrome.md`, `completed/40-flex-layout-primitive.md` — dot pipeline, chrome primitives (`ui_primitives`), flex layout.
- `completed/24-roster-carousel.md` / `35-roster-screen-stats-abilities-squad.md` — the portrait/idle, stat-bar, and Home-button patterns reused here.
- `45-button-widget-unification.md` — if landed first, use the unified button; otherwise the existing `engine_render::Button`.
