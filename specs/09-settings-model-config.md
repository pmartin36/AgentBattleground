# Settings & Model Configuration

## Purpose
Player-controlled configuration. Most importantly, which AI model powers their battles — a choice with real consequences for how the game plays.

## Scope
- Model selection and management
- Account settings
- Game preferences

## Key Details

### Model Selection
The model the player chooses runs their battle simulation locally. This is a meaningful choice — different models will interpret and execute skills differently, at different speeds and quality levels.

**Recommended: Local Model (FLUX4)**
- The game defaults to recommending FLUX4 (or equivalent capable local model)
- Auto-setup is supported: if not installed, the game offers to download and configure it
- Local models mean battles can be run without internet after opponent data is downloaded
- Initial model setup happens during onboarding but can be changed here

**Online Models**
- Players may alternatively use Claude, OpenAI, or other API-compatible models
- Player supplies their own API key
- Latency and cost implications are the player's responsibility
- The game should surface a clear warning about API cost implications

### Account Settings
- Change password
- View username / account ID (needed for challenge-by-username)
- Account deletion (TBD)

### Game Preferences
- **Audio**: master volume, per-bus volume (Music, SFX), and mute. These map directly onto the `engine-audio` subsystem's API (`engine_audio::set_master_volume`, `set_bus_volume(Bus::{Music, Sfx}, ..)`, `set_muted`) from `57-engine-audio-api`. That spec holds these values **in memory only** (runtime state on the audio backend); this settings screen owns the **persisted, user-facing** control — it reads the saved levels on startup and pushes them into `engine-audio`, and writes them back when the player changes them. Persistence needs a config-file layer, which does not exist in the codebase yet (only `logging.rs` uses the OS data dir today) — establishing that layer is part of this spec's scope.
- TBD: display preferences, playback speed defaults, notification settings, etc.

## Open Questions / TBDs
- Which local models besides FLUX4 are supported?
- How is the model used at runtime — does the game shell out to a local binary, use an API interface, or something else?
- Are online model API keys stored locally only, or synced to server?
- Can different pieces use different models? (probably not in v1)
- Config-file persistence format/location for game preferences (incl. audio levels) — reuse the `directories`-resolved OS data dir that logging already uses?

## Dependencies
- `01-onboarding-first-run` — initial model setup shares this logic
- `10-battle-simulation-engine` — model config is consumed here at battle time
- `57-engine-audio-api` — supplies the audio subsystem whose volume/mute this screen persists and drives (v1 keeps those values in memory only; persistence lives here)
