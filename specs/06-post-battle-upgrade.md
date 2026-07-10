# Post-Battle & Upgrade Flow

## Purpose
The moment after a battle ends — results, reflection, and the reward loop. Winning unlocks an upgrade opportunity for one of your pieces.

> **Slice carved out:** the win-side **results screen UI** (outcome banner, per-creature level/XP/stamina columns, spoils row) plus the `Creature` XP field and the `Exhaustion → Stamina` rename now live in `46-post-battle-results-screen.md`. What remains below is the upgrade flow, the loss debrief, draw handling, and replay finalization.

## Scope
- Results screen (win/loss/draw)
- Win: piece upgrade flow
- Loss: debrief
- Replay finalization and upload

## Key Details

### Results Screen
Displays the outcome clearly. Should reference the battle that just concluded — key moments, which pieces performed, which didn't.

### Win: Upgrade Flow
When you win, you have an opportunity to upgrade one of your pieces. The upgrade mechanic is the primary progression loop and is intentionally evocative of Pokemon evolution:
- The player selects which piece to upgrade (or declines)
- The upgrade involves some LLM-driven "magic" — customizing the piece's visuals, expanding its abilities, giving it more personality
- The specific details of what upgrades can do are TBD, but the intent is that each upgrade makes a piece more unique and more powerful
- Over time, pieces should feel like they have a history
- "Expanding its abilities" now has a concrete shape to expand into: `34-creature-attributes-data-model`'s up-to-4 ability slots (each with up to 4 modifier tags) and stat growth. `34` also adds a `level` field to every piece (shown on the roster screen) with no leveling/XP mechanic behind it yet — that mechanic is intended to live here, still undesigned (see Open Questions)

### Loss: Debrief
No upgrade, but the player gets a debrief — a summary of what happened in the battle. The intent is to help the player think about how to adjust their skills. (The real learning happens by watching the replay in the Battle Viewer.)

### Replay Finalization
After every battle, the replay is saved locally and uploaded to the server in condensed format. The server then makes it available for download by the opponent (or as a shared replay if the player opts in).

## Open Questions / TBDs
- Can you decline an upgrade and save it for later?
- What exactly can upgrades modify? (visuals, skills, stats — all TBD)
- Who or what generates the upgrade content? (local LLM? server-side?)
- Is there a draw state, and if so, does it yield an upgrade?
- How many upgrades can a piece receive in total?
- The level/XP mechanic itself: how much a win is worth, whether losses grant partial progress, what a level-up actually changes about a piece's stats — entirely undesigned; `34` only adds the `level` field for the roster screen to display.

## Dependencies
- `34-creature-attributes-data-model` — the stats/level/ability/modifier shape this spec's upgrade flow and future leveling mechanic act on.
- `05-battle-viewer` — player often comes here directly after watching the battle
- `03-army-skill-editing` — post-upgrade, player returns here to adjust skills
- `11-server-backend` — replay upload
- `12-data-model-sync` — replay and piece data format
