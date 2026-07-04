# Mouse Hover Regression — Investigation

> **Status: NEEDS RESEARCH. Not a spec yet, no fix designed.** ("Draft" is reserved elsewhere in `specs/` for a fully-specified, ready-to-implement spec that just hasn't been built yet — this is earlier-stage than that, hence living in `specs/needs-research/` instead.) This document exists to preserve investigation context across a reboot/session loss, per the project owner's explicit request (2026-07-04). Continue from "Next steps" below.

## Symptom (as reported by the project owner)
- Button hover highlighting (the `ButtonState::Idle` → `Hover` color/tint change) stopped working.
- Click (`Down`/`Up`) still correctly changes tint (`Hover`/`Pressed` colors are visibly distinct on click).
- It was **working** when tested mid-way through `32-static-asset-rasterization-caching`'s implementation (project owner explicitly said things looked good then). It was **broken** by the time of the final test, after the whole spec completed. Project owner is not 100% sure they weren't running a stale binary during the "mid-spec, working" test.
- Reproduces identically in **two different terminal emulators**: ghostty (tab) and Cosmic Terminal.
- Held mouse hover for an extended period with zero transition — not an intermittent/dropped-single-event issue.
- Project owner separately noted the mouse "feels sluggish globally" right now, which they attribute to running many concurrent Claude Code sessions on the same machine — mentioned as a caveat, not a diagnosis.

## What's confirmed / ruled out (as of 2026-07-04, this session)
- `ButtonCore::handle_mouse` (`crates/engine/render/src/button.rs`) — the actual Idle/Hover/Pressed state machine — is **byte-for-byte unchanged** across every commit in specs 27, 29, 30, 31, and 32. Diffed the full range; the only changes to `button.rs` are test-fixture signature updates (`&panel_bytes()` → `panel_bytes()`, from the `'static`-bytes refactor spec 32 required) — zero changes to production logic.
- `crates/game/src/app.rs` (the real input-polling loop: `while event::poll(...) { match event::read()? { Event::Mouse(me) => ... } }`) has **zero commits touching it** across the entire spec 27–32 range (`git log --oneline 4acd3ae..e8c9a76 -- crates/game/src/app.rs` returns empty).
- `crossterm`'s `EnableMouseCapture` (called once at app startup) sends `?1000h` (click), `?1002h` (drag-motion), **`?1003h` (any-motion, i.e. bare hover with no button held)**, plus SGR extended-coordinate modes. Confirmed by reading crossterm 0.28.1's actual source (`~/.cargo/registry/.../crossterm-0.28.1/src/event.rs`). So the app is correctly requesting full hover-motion reporting from the terminal at the protocol level — this hasn't changed and isn't misconfigured.
- Wrote and ran a temporary end-to-end test: constructed a real `MainHub::default()`, rendered once, dispatched a real synthetic `MouseEvent::Moved` inside a button's rect via `scene.handle_input(...)` (the same call `app.rs` makes), rendered again, and diffed the actual painted foreground color at that cell. **The color genuinely changed** (Idle → Hover) — the full in-process code path is correct. (Test was reverted after confirming — it was diagnostic only, not a permanent regression test, since it didn't find anything broken.)
- The state machine explains why click-without-hover still works even if `Moved` never arrives: `MouseEventKind::Down(MouseButton::Left)` transitions to `Pressed` from **either** `Idle` or `Hover` (`button.rs`, `handle_mouse`'s `Down` arm) — a button never needs to have passed through `Hover` first to show the `Pressed` tint on click.

## Leading hypothesis
Given the above, the regression is very unlikely to be in this repo's Rust code — everything reachable by `git blame` across the whole spec-27–32 range that could plausibly cause this has been checked and is unchanged/correct. The remaining candidates, roughly in order of likelihood:
1. **System/terminal-level mouse-motion delivery under load.** The project owner's own aside ("mouse feels sluggish globally... too many Claude Code sessions") may not be a coincidental side-note — heavy concurrent CPU load can cause a terminal emulator, PTY layer, or compositor to throttle/drop high-frequency bare-motion mouse events while still reliably delivering lower-frequency, discrete click/release events. This would explain: reproducing across two different terminal emulators (a system-level bottleneck sits below both), click always working, and hover never transitioning even after a long hold (if motion events are being dropped entirely rather than just delayed).
2. **Stale binary during the "mid-spec, working" test.** The project owner flagged this uncertainty themselves. If true, hover may never have actually been re-verified working against current code at all during this session — the "it broke" framing could be comparing against a much older, pre-session baseline instead of a real in-session regression.
3. (Less likely, but not fully excluded) Something about terminal capability/config for `?1003h` in these two specific terminals — considered less likely than (1) since two different, actively-maintained terminal emulators (ghostty, Cosmic Terminal) would need to share the exact same limitation, and both are modern enough to be expected to support SGR any-motion tracking.

## Next steps
1. **Isolate at the crossterm layer, bypassing all game code.** Build a tiny standalone binary/example that does nothing but `execute!(stdout, EnableMouseCapture)` then `loop { println!("{:?}", event::read()?) }`. Run it, hover over the terminal without clicking for several seconds, and check whether *any* `Event::Mouse(MouseEvent { kind: MouseEventKind::Moved, .. })` lines appear.
   - If they **never** appear → conclusively a terminal/OS/system-level mouse-reporting issue, not this repo's code. Nothing to fix here; may resolve itself when system load drops, or may need a terminal-side mouse-tracking setting change (environment-specific, not a code fix).
   - If they **do** appear here but the game still doesn't react → surprising given everything already verified above, but would mean there's a real bug in `app.rs`'s event loop or `SceneManager::handle_input`'s forwarding that wasn't caught by the checks above — re-open investigation into those two specifically.
2. After reboot: re-test hover in the game fresh (in case it was purely a stale-binary or transient-load artifact) before doing anything else — this alone may resolve it with no code change needed.
3. If still broken after a clean reboot + fresh build + retest, revisit this doc and escalate to a real fix-oriented spec once the actual root cause (not just "ruled out our own code") is identified.

## Relevant file/commit references
- `crates/engine/render/src/button.rs` — `ButtonCore::handle_mouse`, unchanged.
- `crates/game/src/app.rs` — the real input loop, unchanged across specs 27–32.
- `crates/game/src/scenes/main_hub.rs` — `handle_input`, dispatches mouse events to each button + tracks `cursor_index`.
- `crates/engine/core/src/scene/manager.rs` — `SceneManager::handle_input`, trivial forward to `self.active.handle_input(ev)`.
- Spec 32 commit range for reference: `4acd3ae` (spec added) .. `e8c9a76` (final commit, test-only diff, confirmed via `git show e8c9a76 -- crates/game/src/scenes/main_hub.rs`).
