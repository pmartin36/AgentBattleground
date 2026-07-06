# Roster Screen — Stats, Abilities & Squad

## Purpose
`24-roster-carousel` built a one-at-a-time carousel showing just a name and sprite. This spec is the redesign that makes the roster screen actually useful: stat bars, level, exhaustion/injury status, a full ability list, and the active/bench/reserve squad structure from `34-creature-attributes-data-model` — plus the interaction to reassign squad roles by swapping two creatures' positions. Builds directly on `24`'s carousel (arrows, home button, dot row, slide transition) rather than replacing it.

## Scope
- **Name** at top, unchanged position, rendered as a **hand-drawn braille dot-matrix font** (not plain terminal text, not the photo/image conversion pipeline) — validated against a plain-text baseline and a block-letter banner alternative in a throwaway prototype (`experiments/roster_name_banner/`); the project owner picked the braille dot-font decisively after running all three. Fades out/in (rather than sliding with the rest of the group) during a switch — see *Transition Choreography* for what "fade" actually means for dot-rendered content.
- **Level** displayed below the name (e.g. "LVL 59"), plain text.
- **4 stat bars** (STR/DEX/INT/VIT), upper-left, each a horizontal bar whose fill length is proportional to the stat value. On switch, bars animate (lerp) from the outgoing creature's values to the incoming creature's — they do NOT snap instantly and do NOT slide off-screen with the sprite.
- **Creature sprite**, below the stat bars, continues to use the existing slide transition from `24` unchanged.
- **Exhaustion / injury display**, upper-right: normally "Exhaustion: N%"; when the creature is in `34`'s injured state, this switches to a recovery-remaining display (e.g. "Exhausted: N days remain") instead of a percentage. Disappears during a switch (see *Transition Choreography*).
- **Full ability list**, right side, below exhaustion: all of the creature's abilities (up to 4) shown simultaneously and fully expanded — each ability's description and modifier tags are always visible, not progressively disclosed. Disappears during a switch (see *Transition Choreography*).
- **Dot row**, bottom, now visually grouped by squad role: 3 dots (active) — gap — 1 dot (bench) — gap — 2 dots (reserve), each cluster with a small text label underneath ("Active" / "Bench" / "Reserve"). The filled/brighter dot still marks which creature is currently being viewed (unchanged from `24`), independent of the clustering, which marks role.
- **Select-and-swap interaction**: clicking the current creature's dot, or pressing Space, marks it "selected" (the same dot begins blinking — no new visual element). Navigating (arrows/keys) to a different creature and selecting again (click/Space) swaps the two creatures' positions in the roster — which, per `34`'s purely-positional role rule, immediately changes both creatures' active/bench/reserve role as a side effect of the reorder. Selecting the same already-selected creature again cancels the selection (stops blinking) without swapping.
- **Left/right nav, home button**: unchanged from `24`.

Out of scope:
- Any UI for the mid-battle, skill-driven bench swap (`34` already scopes this out — no design exists).
- Editing stats, abilities, or modifiers themselves (read-only display, same as `24`'s read-only stance on name/sprite).
- Any numeric stat value shown as text (bars only, per the reference sketch) — the exact bar-fill scale (what stat value == a full bar) is an implementation-level v1 call, not a design decision requiring further input.

## Transition Choreography (switching creatures)
Per the project owner's explicit direction, a switch is NOT a uniform slide of everything:
- **Sprite**: keeps `24`'s existing slide transition unchanged (outgoing slides off in the direction of travel, incoming slides in from the opposite edge).
- **Name**: fades out as the switch begins, then fades back in showing the incoming creature's name once the transition settles — never travels with the sprite (no slide). "Fade" here means a **color lerp toward the background color**, not true alpha/opacity — the engine's dot compositor is binary lit/unlit per cell (`13-rendering`'s deferred sub-cell translucency), so nothing in this renderer supports a literal alpha fade. Every lit dot's color tweens toward the background color and back, the same technique already used for tint/hover-state animations elsewhere in the engine — this applies whether the name is rendered as plain text or (see below) as braille dots.
- **Stat bars**: numerically lerp from the outgoing creature's values to the incoming creature's, in place — no fade, no slide, just the fill length animating.
- **Exhaustion display and ability list**: disappear during the transition (no cross-fade animation specified — they're simply not shown mid-transition) and reappear, populated with the new creature's data, once the transition settles.
- **Dot row**: the filled/current-index dot moves as it does today (`24`); the role-cluster grouping and labels are static UI chrome, unaffected by the transition.

## Decisions (v1)
- **Creature name is a hand-drawn braille dot-matrix font, a new documented exception to `13-rendering`'s "braille is universal except text" rule** — same category of exception as the Main Hub logo (`25-main-hub-navigation`), but for a different reason: the logo is one static bundled image, while this is per-creature dynamic text. The exception holds here because `13-rendering`'s "braille cannot render legible Latin glyphs at this resolution" finding was established against the **photo/sprite conversion pipeline** (adaptive luma-threshold, one averaged color per cell — tuned for organic creature silhouettes, not typography); it does not apply to letterforms **hand-authored directly as lit/unlit dot patterns**, bypassing image conversion entirely — the same low-level technique `battle_viewer.rs`'s `draw_board_lines` already uses for grid lines (`DotBuffer`/`Dot::Lit`/`dots_to_grid`, no `AnimatedSprite`/`convert` involved). Validated in `experiments/roster_name_banner/`'s `braille_name` prototype against a plain-text baseline and a block-letter-banner alternative; the project owner ran all three themselves and chose braille decisively.
- **Font is proportional-width, not monospace.** Each letter's dot-width matches its actual shape (e.g. "L" narrower, "M"/"W"/"T"/"A" wider), with a small fixed dot-gap between letters — forcing every letter into the same fixed-width column was tried first, read as cramped/illegible, and was corrected (see `experiments/roster_name_banner/src/lib.rs`'s `braille_font` module, and the project memory this produced). This is a hard requirement, not a style preference — the monospace version was materially harder to read.
- **Bold/italic styling is explicitly deferred**, not part of this spec's initial build. The project owner wants the regular-weight proportional font shipped first; bold/italic hand-drawn letterform variants (already prototyped in `experiments/roster_name_banner/`'s `braille_name_bold`/`braille_name_italic`) are expected to be a lighter follow-up on top of the same font-data structure, not a redesign — but are not required for this spec's done-criterion.
- **Consumes `34`'s `RosterEntry`-shaped data** (or whatever `34` ultimately names its game-side wrapper type) via `crate::creatures::all()` or its successor, the same way `RosterManager` already consumes `crate::creatures::all()` for `Creature`.
- **Fade is a new transition primitive alongside the existing slide.** `24`'s `Slide` (prev_index/dir/start, driven by `elapsed`) stays as-is for the sprite; the name's fade and the bars' lerp are additional per-frame computations keyed off the same `elapsed`/`Slide` timing window, not a second independent transition-state machine.
- **Bar-fill scale** is an implementation call (e.g. a fixed display cap), documented in code, not re-litigated here.
- **Role labels are static plain text** (per the braille-except-text rule) positioned under each dot cluster via `anchor`/`stack` (`26-screen-space-positioning`), not a new UI primitive.
- **"Selected" blink** reuses the existing filled/unfilled dot asset pair, toggling visibility on a timer — no new dot asset needed.

## Open Questions / TBDs
- Exact bar-fill display cap (implementation detail, not blocking).
- Whether the ability list needs to scroll/paginate if a creature's abilities+modifiers text is long enough to overflow the panel (no current bundled creature content is long enough to test this; flag if it comes up during implementation).
- Bold/italic name styling — prototyped, deferred, not required for this spec (see Decisions above). A future follow-up can add it without redesigning the font data.

## Reference Prototype
`experiments/roster_name_banner/` — throwaway crate validating the name-treatment decision above: plain-label baseline, block-letter banner, and the winning braille dot-font (`braille_name`, plus deferred `braille_name_bold`/`braille_name_italic` variants), all rendered through the real `engine-render` primitives via a `ratatui::backend::TestBackend` buffer. Not shipped code — a reference for implementing the real font-data module against, same role `experiments/ascii_test/` plays for `13-rendering`.

## Dependencies
- `24-roster-carousel` ✅ — the carousel, slide transition, dot row, arrows, and home button this spec extends rather than replaces.
- `34-creature-attributes-data-model` — stats/level/exhaustion/abilities/modifiers/squad-role data this screen renders.
- `22-braille-ui-chrome` ✅, `26-screen-space-positioning` ✅ — layout/positioning primitives reused for the new panels.
- `16-world-space-and-camera` ✅ — `Tween`/`ease_in_out`, reused for the bar-lerp and name-fade animations, same as `24`'s slide.
