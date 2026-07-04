# Typed Mutation for Engine-Owned Props (e.g. Transform)

> **Status: RESEARCHED, awaiting go-ahead — not yet acted on.** ("Draft" is reserved elsewhere in `specs/` for a fully-specified, ready-to-implement spec — this hasn't been promoted to that yet, hence `specs/needs-research/`.) Research is largely done (see below); this is captured here rather than executed because the project owner wants to circle back after resolving the mouse-hover regression first. Preserved across a reboot/session loss per the project owner's explicit request (2026-07-04).

## The original question
Project owner's framing: in **game** code, setting something like `pieces[0].transform.translate.x = 5` should be the norm — not a stringly-typed `apply_patch("pieces[0].transform.translate.x", json!(5.0))` call. Not asking for this on every prop, specifically "engine-y" props like `Transform`. The inspector (a genuinely separate, cross-process tool) can keep doing whatever it needs to (string paths are basically unavoidable there, since it works from a runtime-discovered `FieldSchema`, not compile-time Rust types).

## Research findings (a `fork` sub-agent investigated this, 2026-07-04 — see full report in conversation history if resumable, summarized here)
Grepped every `.apply_patch(` call site in the workspace. Result: **there is exactly one production call site** — `engine-core/src/scene/manager.rs:216`, inside the `ApplyState` IPC handler. This one is genuinely cross-process (the path string arrives off the wire from the separate `inspector` binary), so it has no choice but to be stringly-typed.

Every *other* `apply_patch` call site is either:
- The derive macro's own test suite (`inspect.rs`, `derive_inspectable.rs`, `transform.rs`'s own tests) — necessarily testing string-path parsing with strings, that's what those tests are for.
- Two tests in `battle_viewer.rs` (`piece_apply_patch_on_team_changes_only_team`, `piece_apply_patch_on_readonly_col_is_err_and_unchanged`) that are **specifically testing the patch protocol's field-isolation and readonly-rejection behavior** — using `apply_patch` there is the correct tool, not a shortcut around typed access.

Confirmed the actual in-process game logic **already** does plain typed field access — `BattleViewer::drive_events` (the event-driven playback that moves pieces during a battle) does things like `piece.transform.translate = target;` and `piece.col = to.0;` directly. `Transform`, `WorldPos`, `Vec2`, and `Piece` are all plain-public-field structs. So `pieces[0].transform.translate.x = 5.0` **already works today, with zero new machinery** — it's already the established norm everywhere in this codebase except the one necessary wire boundary.

## Conclusion
There is essentially nothing to build here. The architecture already does the right thing; the string-path `apply_patch` mechanism is correctly scoped to exactly the one place (the wire protocol) where it's unavoidable.

## Proposed next step (small, low-risk, not yet done)
Add a short guardrail to `CLAUDE.md`, in the same style as the existing "Engine / Game Boundary" section, to prevent future drift (e.g. someone reaching for `apply_patch` out of habit in new game code instead of a plain field write):

> **In-process code mutates engine-owned types (`Transform`, etc.) via direct typed field access — never construct an `apply_patch` string path for something reachable by assignment. `apply_patch` is reserved for the wire/inspector protocol and tests of that protocol itself.**

This is documentation only — no code changes, no spec needed beyond this note. Safe to do any time; not blocked on anything else.

## Next steps when resuming
1. Confirm the project owner still wants the `CLAUDE.md` addition above (or wants wording changed).
2. Add it (one `Edit` call, trivial), commit, done. No pipeline/spec execution needed — there's no code to write.
