# Title Intro Fixes — Drop the Frame, Per-Launch Gating, Fade Robustness

## Purpose
Three fixes to the shipped animated title (`61-animated-title-logo`) and its
button fade-in (`62-menu-button-cleanup`), found on first real review in the hub:
a leftover **title-frame border** drawn around the logo, the intro gated to
**first-run-ever** instead of **per-launch** (so it never replays), and making
the **button fade-in robust** to the no-intro / already-seated case so buttons
can't get stuck hidden.

## Scope
- Remove the leftover title frame (the `FRAME_PANEL` border box around the logo).
- Replace the persisted first-run gating with a **per-process (per-launch)**
  session flag; remove the now-unused `first_run` persistence + its marker file.
- Make the button fade-in **level-based on the animation clock** so buttons are
  visible whenever the intro isn't currently animating.

Out of scope:
- The still art, animation geometry/palette/timing (all `61`) and the button
  border/colors/Settings scene (all `62`) — this only fixes the three
  integration defects, changing no visual design.

## Background (grounding)
- **Leftover frame:** `main_hub.rs` `render()` calls `self.draw_title_frame(buf,
  title)` (`main_hub.rs:270`), which stretches `assets::FRAME_PANEL` to fill the
  title rect, then draws the logo into `title_interior(title)` (inset by the
  frame thickness). `61` deleted the `LOGO` PNG but not this **frame box**, so a
  stray border rings the standalone logo.
- **Gating:** `main_hub.rs:254` uses `first_run::is_first_run()` /
  `mark_first_run_done()` (a persisted `first_run_complete` marker under
  `instructions::base_data_dir`). This is first-run-**ever**, not per-launch — and
  the marker got created during the pipeline's own build/test runs, so even the
  first genuine launch saw "not first run" → no animation. `first_run.rs` is used
  **only** by the title.
- **Fade:** `62` Decision 6 fades the nav buttons in after the sword seats. If
  that is edge-triggered on the seat instant, an already-seated entry (hub
  return, or a launch where the intro isn't playing) never fires it → buttons
  could stay hidden. The animation clock is `main_hub.elapsed: f32`, advanced in
  `update(dt)` and read by `title_logo::frame(elapsed)`; on a non-intro entry
  `elapsed` is set straight to `title_logo::ANIM_END`.

## Decisions (v1)

- **Decision 1 — Drop the title frame.** Remove the `draw_title_frame` call
  (`main_hub.rs:270`) and the method, and give the logo the **full title rect**
  (drop the `title_interior` frame-inset for the logo). No `FRAME_PANEL` border
  around the title. The `FRAME_PANEL` asset itself stays (still used elsewhere;
  and note `62` may have moved the *buttons* to a procedural border). **Verify by
  decoding dots** that no border ring surrounds the logo.

- **Decision 2 — Per-launch (per-process) intro gating.** Replace the persisted
  first-run check at the hub-enter point with a **process-scoped session flag** —
  a module `static` "intro played this launch" (e.g. `AtomicBool`, default false
  at process start). On hub enter: if not yet played → `elapsed = 0.0` and set the
  flag; else → `elapsed = ANIM_END`. Effect: the intro plays **once per app
  launch** (first hub entry in the process), **not** when returning to the hub
  from roster/battle in the same session, and **replays on every fresh launch**.
  Remove the `first_run` module, its `mark_first_run_done`/`is_first_run` calls,
  and delete the stray `first_run_complete` marker (and gitignore it if tracked).
  If nothing else consumes `first_run`, delete the module rather than leave dead
  code; a future onboarding flow (`01-onboarding-first-run`) can reintroduce
  persistence when it actually needs it.

- **Decision 3 — Fade-in is level-based on the clock, never edge-triggered — and
  the return-to-menu case is handled by the SAME path.** The nav buttons' opacity
  is a **pure function of `elapsed`** (not of any "sword just seated" event). One
  formula covers **both** hub-entry cases, with no branch for "did the intro
  play":
  - **Case A — first hub entry of the launch** (intro plays, `elapsed` starts at
    `0`): buttons fully hidden while `elapsed < seat_time`, alpha ramps `0→255`
    across the fade window (`62` Decision 6's `BTN_FADE_START..END`), fully opaque
    after.
  - **Case B — return to the hub from roster/battle/etc.** (intro does NOT play;
    Decision 2 sets `elapsed = ANIM_END`): `elapsed` is already past the fade
    window, so the very same formula yields **full opacity on the first frame** —
    buttons just appear, no fall, no seat, nothing to wait on.

  The whole point: there is **no seat-edge event to miss**, so Case B cannot leave
  the buttons stuck hidden. This is the failure mode called out on review, and it
  must be verified for **both** cases: **(A)** first launch entry hides-then-fades
  the buttons in; **(B)** entering the hub from another scene shows all buttons at
  full opacity on the first rendered frame. Repairs/confirms `62` Decision 6.

## Open Questions / TBDs
- None material. (Minor: whether to keep the `first_run` module dormant for future
  onboarding vs. delete it now — Decision 2 deletes it if unused.)

## Dependencies
- `61-animated-title-logo` — the title/animation + the hub animation clock this
  fixes the gating and frame of.
- `62-menu-button-cleanup` — the button fade-in this hardens; **63 must build
  after 62 lands** (both edit `main_hub.rs`).
- `completed/25-main-hub-navigation` / `02-main-hub-dashboard` — the hub scene.
- `completed/13-rendering` — the dot pipeline + `decode_braille_cell` the frame
  removal is verified against.
