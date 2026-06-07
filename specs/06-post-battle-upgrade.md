# Post-Battle & Upgrade Flow

## Purpose
The moment after a battle ends — results, reflection, and the reward loop. Winning unlocks an upgrade opportunity for one of your pieces.

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

## Dependencies
- `05-battle-viewer` — player often comes here directly after watching the battle
- `03-army-skill-editing` — post-upgrade, player returns here to adjust skills
- `11-server-backend` — replay upload
- `12-data-model-sync` — replay and piece data format
