# Server / Backend

## Purpose
The minimal shared infrastructure that connects players. The server's job is coordination, not computation. All simulation runs on the client.

## Scope
- User authentication
- Matchmaking
- Replay storage and distribution
- Leaderboard

## Key Details

### Hosting Stack
- **Compute**: Fly.io — managed platform, deploy the API server (persistent process), attach a small Postgres for accounts/leaderboard/matchmaking state, custom domain support. Free tier covers hobby traffic; ~$5-10/mo beyond that.
- **Replay storage**: Cloudflare R2 — S3-compatible object storage, no egress fees (critical for replay downloads at scale), free tier is generous. Clients never hit R2 directly — the API generates signed URLs or proxies downloads.

### Design Philosophy
The server does as little as possible. No battle simulation, no LLM calls, no heavy processing. It stores small structured data in Postgres and replay files in R2, and routes requests between clients.

### Authentication
- Username + password
- Session token management
- Password reset TBD

### Matchmaking
Three modes:
1. **Automatic**: find a valid opponent of similar power level (algorithm TBD). Returns opponent's public piece/skill data for local download.
2. **Challenge by username**: direct player-to-player challenge. Flow (sync vs. async) TBD.
3. **Bot fallback**: when no valid human opponent is available (new player, no opponents at power level, played everyone recently). Server returns bot skill data.

Same-opponent frequency limiting: a player cannot be matched against the same opponent too frequently. Threshold TBD.

### Replay Storage & Distribution
**Auto-download replays**: when an opponent runs a battle against your army, the server stores the replay. When you next connect, it pushes those replays to you and then purges them. You cannot miss a replay that happened while you were offline — it waits until you connect.

**Shared replays**: a player can mark a replay as shared. Shared replays persist on the server until the uploader removes them or a storage limit is hit. Any player can browse and download shared replays.

### Leaderboard
A ranked list of all registered players. Updated after each battle result is reported. Ranking algorithm TBD. Accessible to all players.

### Player Profiles (Public Data)
Each player has a public profile: username, ranking, battle record, and shared replays. Skill files are NOT public — only the player has access to their own skills.

## Open Questions / TBDs
- What language/framework for the server? (should be simple — Go, Python, or Rust are all reasonable)
- How are battle results reported? (client reports outcome after local simulation)
- Is there any anti-cheat concern with self-reported results?
- Storage limits for shared replays
- How does the server distribute bot skill files?

## Dependencies
- `04-matchmaking-battle-initiation` — primary client of matchmaking endpoints
- `07-replay-browser` — primary client of replay distribution
- `08-leaderboard-social` — primary client of leaderboard and profile data
- `12-data-model-sync` — all data formats exchanged between client and server
