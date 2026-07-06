# Main Hub / Dashboard

## Purpose
The resting state of the game. Everything the player needs to understand their current situation and navigate to any other scene.

## Scope
- Army status overview
- Daily battle availability indicator
- Pending replay downloads
- Notifications
- Navigation hub for all other scenes

## Key Details

### Army Status Overview
A glanceable summary of your 6 pieces — names, visuals, and high-level health/readiness. "Readiness" now has a concrete meaning: whether a creature is available (active/bench) or injured and recovering (`34-creature-attributes-data-model`'s exhaustion/injury state) — this overview is a summary, not the full per-creature exhaustion/ability detail (that's the Roster screen, `35-roster-screen-stats-abilities-squad`).

### Daily Battle Gate
One initiated battle per day. The hub makes the current status obvious: available, or time until reset. This is the primary pacing mechanic.

### Pending Replay Downloads
When the player comes online, the server pushes down any replays of battles fought against their army since last login. The hub surfaces these as a notification/queue so the player knows there's new tape to watch.

### Notifications
- New replays available
- Battle results (if they stepped away)
- Leaderboard changes (TBD scope)
- Shared replays from other players

### Navigation
The hub is the root of the navigation tree. All other scenes are reachable from here.

## Open Questions / TBDs
- What does the hub look like visually? Full ASCII layout TBD by design agent.
- Do notifications persist or clear on view?
- Is there an "offline mode" where the hub functions without server connectivity?

## Dependencies
- `34-creature-attributes-data-model` — the exhaustion/injury state the Army Status Overview summarizes.
- `11-server-backend` — replay downloads and notification data come from server on connect
- `03-army-skill-editing` — navigates to army management
- `04-matchmaking-battle-initiation` — navigates to battle initiation
- `07-replay-browser` — navigates to replay browser
- `08-leaderboard-social` — navigates to leaderboard
