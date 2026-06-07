# Replay Browser

## Purpose
A library of battles to watch. Players use this to study opponents, review their own performance, and discover shared replays from the broader community.

## Scope
- Browse and play your downloaded replays
- Browse and download shared/public replays
- Launch replays in the Battle Viewer

## Key Details

### Your Replays
When a player connects to the server, any replays of battles fought against their army since last login are downloaded automatically. These replays are surfaced in the browser and then purged from the server. Local storage is the player's responsibility after download.

### Shared Replays
Players can opt to "share" a replay, which keeps it hosted on the server for others to download. Shared replays are browsable by any player. This is the community tape-sharing mechanic — useful for high-level play, interesting match-ups, or teaching moments.

### Replay Metadata
Each replay should show enough context to decide whether to watch:
- Participants (player names)
- Date/time
- Outcome
- Army names or piece counts (TBD)
- Whether it's been watched before

### Launching into Battle Viewer
Selecting a replay launches the Battle Viewer in replay mode. Playback controls are handled there.

## Open Questions / TBDs
- Is there a cap on local replay storage?
- Can shared replays be taken down by the original uploader?
- Is there any curation or ranking of shared replays (most viewed, etc.)?
- Can you share a replay of a battle you lost?

## Dependencies
- `05-battle-viewer` — all replay playback happens here
- `11-server-backend` — shared replay hosting, download-on-connect flow
- `12-data-model-sync` — replay file format
