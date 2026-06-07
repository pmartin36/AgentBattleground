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
- TBD: display preferences, playback speed defaults, notification settings, etc.

## Open Questions / TBDs
- Which local models besides FLUX4 are supported?
- How is the model used at runtime — does the game shell out to a local binary, use an API interface, or something else?
- Are online model API keys stored locally only, or synced to server?
- Can different pieces use different models? (probably not in v1)

## Dependencies
- `01-onboarding-first-run` — initial model setup shares this logic
- `10-battle-simulation-engine` — model config is consumed here at battle time
