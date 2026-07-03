> # ✅ DONE! — Completed 2026-07-03

# Main Hub Navigation

> **Status: implemented.** A slice of `02-main-hub-dashboard.md`: a real, clickable-and-keyboard-navigable menu on `SceneId::MainHub`, replacing its current placeholder fill. The full dashboard vision (server-backed status, notifications, daily battle gate) remains entirely out of scope — this is navigation only.
>
> Known follow-up: the title box and menu buttons use `anchor`/`stack` (spec 26) which don't yet support a margin/inset parameter — see `28-anchor-margin-support` (queued).

## Purpose
Give the player a real way to navigate from the hub to the rest of the game — Roster, Battle, Exit — via a boxed-title, bordered-button menu, replacing the debug-only digit-key switcher as the actual in-game navigation path (the digit-key switcher itself stays, additive, for debug use).

## Scope
- `SceneId::MainHub` gets real content instead of `fill_and_label`.
- A boxed title at the top: the bundled **logo sprite** (a finished, project-owner-supplied wordmark — "AGENT BATTLEGROUND" in ornate gold lettering with decorative flourishes and a sword motif, already carrying its own title text baked into the artwork — see Decisions), inside a braille-rendered bordered frame (hollow rectangle, transparent interior — a new asset, not spec 22's opaque `BUTTON_PANEL`). No separate plain-text title label — the logo image already reads as the title.
- Three vertically-stacked menu buttons below the title, each a bordered frame (same new hollow-frame asset/technique as the title box, but interactive) with a centered plain-text label:
  - **Roster** → `Transition { target: SceneId::RosterManager, params: None }`
  - **Battle** → `Transition { target: SceneId::BattleViewer, params: None }` — direct, no Matchmaking placeholder detour, per the project owner's explicit call.
  - **Exit** → quits the application — same effect as the existing `q`/Ctrl-C keys in `crates/game/src/app.rs`'s main loop.
- A selection cursor: spec 22's existing `ICON_ARROW_RIGHT` icon, braille-rendered, prefixed immediately before whichever menu item is currently hovered/focused.
- Input, both mouse and keyboard:
  - Mouse: hovering a button's rect moves the cursor to it; clicking activates it — reusing `21-mouse-hover-input`'s `InputEvent::Mouse`.
  - Keyboard: Up/Down moves the cursor between the 3 items (wrapping, matching `24-roster-carousel`'s wraparound precedent); Enter activates the currently-cursored item.
- The existing digit-key debug switcher (`crate::scenes::scene_for_digit`) is untouched and keeps working exactly as before — this menu is an additive, real navigation path, not a replacement.

Out of scope:
- Army status overview, daily battle gate indicator, pending replay notifications, leaderboard changes — spec `02`'s fuller dashboard vision, entirely pending.
- Any transition *into* `MainHub` beyond what already exists (boot default, digit-key `1`, and whatever already transitions here).
- Matchmaking (`04-matchmaking-battle-initiation`) — "Battle" goes straight to `BattleViewer` per the project owner's explicit call; the Matchmaking-placeholder detour discussed earlier is not being built.

## Decisions (v1)
- **Logo sprite**: a project-owner-supplied finished PNG (not generated via `creature_lab` — this one came pre-made, already background-removed/transparent, already carrying the "AGENT BATTLEGROUND" wordmark baked into the art), bundled at `crates/render/src/assets/logo.png` via `include_bytes!`, rendered through `render::convert` — same technique as every other sprite in the game. Because the title text is already part of the image, this is the one title-area element that's exempt from the "words stay plain terminal text" pattern every other label in the game follows — the logo *is* the title, not chrome around a separate text label.
- **Shared interaction core, not a duplicated state machine.** `22-braille-ui-chrome`'s `Button` explicitly scoped out text-in-button rendering (it only composites a fixed icon+panel pair). Rather than duplicate its hover/press/click transition logic (which has real, already-tested subtlety — e.g. "stays `Pressed` when the pointer drags outside the rect") into a second type, `Button`'s interaction core (`ButtonState`, hit-testing, `handle_mouse`'s transition table) is factored out into something reusable independent of *how* a button paints itself, and this spec's menu buttons are a second consumer of that shared core with a different render path (bordered frame + centered text label instead of panel + icon). `crates/render/src/button.rs`'s existing tests must continue passing unmodified — this is a refactor of `Button`'s internals, not a behavior change to its public contract.
- **New asset: a hollow bordered frame** (alpha-transparent interior, opaque border only), generated the same way as spec 22's assets — a checked-in `examples/gen_*.rs` generator is the provenance, not a third-party source. Used for both the title box (static, non-interactive) and each menu button (interactive, tinted by `ButtonState` same as spec 22's panel).
- **Text label lives inside the bordered frame**, drawn via the existing `render::label` (plain text, per the braille-except-text exemption), the same "braille chrome + text label" composition `fill_and_label` already established — not a new text-rendering mechanism.
- **Layout uses `render::screen_layout`, not hand-derived `Rect` math.** The title box is placed via `anchor(area, title_size, Anchor::TopCenter)`. The 3 menu buttons' `Rect`s come from `stack(menu_container, &[button_size; 3], gap, StackAxis::Vertical)`, where `menu_container` is itself `anchor(area, menu_container_size, Anchor::Center)` (or `TopCenter`, offset below the title box — implementer's call on the exact vertical relationship between the title box and the menu group, but the *mechanism* is `anchor`/`stack`, not manual `Rect` arithmetic). This is the concrete case `26-screen-space-positioning` was built for.

## Dependencies
- `13-rendering` ✅, `21-mouse-hover-input` ✅ — mouse events this menu's hover/click consumes.
- `22-braille-ui-chrome` ✅ — the `Button` interaction core this spec's menu buttons share (via the refactor described above).
- `26-screen-space-positioning` ✅ — `anchor`/`stack` place the title box and menu buttons; a Tween-driven hover/focus-change animation (if any is added later) would use `RectTween`, though this spec's Scope doesn't currently call for one — static anchoring/stacking is sufficient for a menu that doesn't move.
- Feeds nothing further planned yet — this is the last item in the original Roster/Nav handoff's build list.
