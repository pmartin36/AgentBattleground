# Leaderboard & Social

## Purpose
Rankings and the ability to look outward — see how others are doing, scout their armies, and find replays worth watching.

## Scope
- Global player leaderboard
- Player profile browsing (public info)
- Access to shared replays from the leaderboard context

## Key Details

### Leaderboard
A ranked list of all players on the server. Ranking criteria TBD (win rate, total wins, ELO-style rating, etc.). The leaderboard is the primary competitive reference point.

### Player Profile Browsing
From the leaderboard, a player can view another player's public profile:
- Username
- Ranking / record
- Army overview (piece names, possibly visuals — public info TBD)
- Shared replays associated with that player

This is the "scouting" mechanic — you can study top players' armies and replays to inform your own skill editing.

### Challenge Entry Point
From a player's profile, you should be able to initiate a direct challenge (routes to Matchmaking & Battle Initiation).

### Shared Replays
The leaderboard context surfaces shared replays, either globally or per-player. These are browsable and launchable into the Battle Viewer.

## Open Questions / TBDs
- What ranking algorithm? Win/loss ratio? ELO? Power level?
- What army info is public vs. private? (skills might be private, piece names/visuals public)
- Is there any social feature beyond browsing — messaging, following, etc.? (probably not in v1)
- How often is the leaderboard updated?

## Dependencies
- `11-server-backend` — leaderboard and player profile data hosted here
- `04-matchmaking-battle-initiation` — challenge entry point
- `07-replay-browser` — shared replay browsing
- `05-battle-viewer` — replay playback
