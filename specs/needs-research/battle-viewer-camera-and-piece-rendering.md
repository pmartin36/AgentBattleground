# Battle Viewer — Camera Angles, Perspective, and Piece Rendering Rework

> **Status: DRAFT — needs research/discussion before this becomes a real spec.** `37-battle-viewer-dynamic-camera` shipped (all tasks GREEN, gate clean) but the actual visual result doesn't match what the project owner expected at all — this document captures the gap and the follow-up asks as of 2026-07-06, for a future session to dig into. Per the project owner's explicit call: **the perspective-projection question stays open here, to be discussed further before any design work starts** — don't run this through the TDD pipeline as-is.

## What shipped vs. what was expected

`37`'s three camera modes (Sideline/OverShoulder/TopDown) all shipped as simple 2D projections (per spec 37's own "stylized, not physically-accurate perspective" call) — but the project owner's actual read of the result: **the views basically look the same as each other**, not distinct angles.

- **Over-the-shoulder**: expected ~30° above horizon. What shipped reads as ~80° above horizon — essentially indistinguishable from top-down.
- **Sideline**: expected ~10° above horizon, and — this is a real direction correction, not just an angle tweak — **viewed broadside** (camera looking perpendicular to the line connecting the two teams, so the teams read as separated left/right or along a different screen axis than today), not from behind one team looking toward the other. The current `SideView` camera looks down the *same axis* over-the-shoulder does (behind one team, toward the other) — that's the wrong axis entirely for a true "sideline" read, not just the wrong pitch.
- **Top-down**: confirmed fine as-is.

## Open question — deliberately unresolved (discuss before designing)

**How far does "perspective projection" actually need to go?** `16-world-space-and-camera` is built on a deliberate architectural decision: world space is continuous **2D**, board-agnostic, and a `Camera` is just two pure functions (`project`/`depth_key`) over that 2D position — no 3D position, no camera orientation, no perspective divide anywhere in the engine. Two very different scopes are on the table:

1. **Cheaper pseudo-perspective (2.5D)** — keep the 2D world-space model, fake depth via scale/screen-position tricks (farther things render smaller/higher on screen) without real 3D math. Smaller lift, consistent with this project's "stylized, not physically accurate" approach everywhere else (creature art, board chrome, etc.).
2. **Full 3D perspective** — real 3D world positions, a camera with position + orientation, actual perspective-divide projection math, genuine foreshortening from any angle. This is a rewrite of the engine's core spatial model (`WorldPos`, the `Camera` trait's signature, `16`'s own decisions), not a BattleViewer-local change.

The project owner wants to **discuss this further** before committing either way — this is the central design question for whoever picks this up.

## Action items (from the project owner, verbatim intent preserved)

1. **Fix the camera angles** — per the corrected pitches/direction above (30°/10°/broadside-sideline). Blocked on the perspective-scope question above, since the right way to "fix an angle" differs a lot between a 2.5D trick and real 3D camera orientation.
2. **Perspective projection** — see Open Question above.
3. **Grid lines were completely invisible in over-the-shoulder and sideline**, not faint. Spec 37's Decision was explicitly "faint... not gone" — worth checking first whether this is a straightforward bug in the just-shipped dimming logic (e.g. `GRID_LINE_COLOR_DIM` too close to background, or lines landing off-screen/miscomputed under these two camera modes specifically) rather than something that needs the bigger perspective rework to fix. **Possibly a quick, separate bug fix rather than part of the research-heavy work.**
4. **Real creature art instead of the stand-in wizard sprite.** Clarified: a **placeholder-quality swap is enough for now** — swap the wizard for real bundled creature art (the same 6 creatures `crates/game/src/creatures.rs` already bundles) in the existing hardcoded demo battle layout. Does **not** need to reflect the player's actual chosen squad — that would require cross-scene squad-selection data wiring, explicitly out of scope here.
5. **The bench piece isn't visible in ANY camera view.** `36-battle-viewer-squad-layout` put it on its own outermost grid row (row 0 / row 6 of the 7×7 board), same scale/rendering as active pieces — worth checking whether this is a camera-framing bug (rows getting clipped/off-screen under the current — possibly-also-broken — camera math) before assuming it needs new design work.
6. **Sprites should always be billboarded** (always face the camera, never skew/rotate with the projection). Not actually broken this round (the shipped cameras are simple enough that this was trivially true), but flagged now as a hard requirement for whenever real camera angles/perspective land — a naive perspective transform applied directly to sprite rasterization would break this.
7. **Replace per-team sprite tint with a team-colored "contact shadow" blob.** Instead of the current multiply-blend tint on the whole sprite, render a blob-shaped shadow under the piece, in the team's color, fading toward the edges of the cell/square it occupies. Clarified: **team colors stay the existing pale-gold/pale-mint palette** — just moved from a sprite tint to this new shadow treatment, not a literal red/blue palette change. Visibility rule: the shadow only shows when a piece is at rest, fully settled in a cell — during a move (mid-tween), it fades out, then fades back in once the piece lands in its new cell. (Exact fade timing/curve — e.g. tied to the move tween's own duration vs. a fixed separate fade — is a "digging" detail for the research pass, not decided here.)

## Next steps when resuming
1. Have the perspective-scope discussion with the project owner (2.5D vs. full 3D) before any design work — this gates almost everything else in items 1-2.
2. Triage items 3 and 5 (invisible grid lines, missing bench) as likely-simple bugs in the already-shipped `37`/`36` code — worth a quick look before folding them into the bigger rework, since they may not need new design at all.
3. Item 4 (real creature art) and item 7 (team-color contact shadow) are reasonably well-specified already and could plausibly be scoped as their own smaller spec(s) independent of the camera/perspective question, if the project owner wants to unblock some progress while the bigger perspective question is still being decided.
4. Item 6 (billboarding) needs no action until real camera angles/perspective exist — just don't forget it when that work starts.

## Dependencies / related specs
- `37-battle-viewer-dynamic-camera` — the just-shipped feature this document is a follow-up to; its "stylized, not physically-accurate perspective" and "faint grid lines" decisions are exactly what's being revisited/debugged here.
- `36-battle-viewer-squad-layout` — the 7×7/bench-row geometry the missing-bench symptom (item 5) sits on top of.
- `16-world-space-and-camera` — owns the 2D-world-space/`Camera`-trait decision the open perspective question would need to revisit if "full 3D" is chosen.
- `23-piece-identity-data-model` / `crates/game/src/creatures.rs` — the bundled real creature art (item 4) already exists here, built for the roster screen; this would be its first reuse in the Battle Viewer.
- `13-rendering` — sprite tinting (multiply-blend) and the binary-alpha dot compositor the new contact-shadow treatment (item 7) needs to work within (no translucency support today — a soft-fading blob shadow may itself need new compositor capability, worth flagging during research).
