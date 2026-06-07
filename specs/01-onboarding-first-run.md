# Onboarding & First Run

## Purpose
The entry point into the game. Guides a new player from zero to their first battle, establishing identity, local tooling, and their initial army.

## Scope
- Account creation (username + password)
- Model selection and auto-setup
- Initial piece generation
- Guided first bot match (tutorial)

## Key Details

### Account Creation
Players register with a username and password. This identity is tied to their army, battle record, and leaderboard ranking on the server.

### Model Selection
- The game strongly recommends a local model (FLUX4 is the default recommendation)
- Auto-detect if a compatible local model is already installed; if not, offer to download and configure it automatically
- Players may alternatively select an online model (Claude, OpenAI, etc.) with their own API credentials
- Model choice persists and can be changed later in Settings

### Initial Piece Generation
- Every new player starts with 6 pieces — blank slates with no customization
- The generation step should feel like a meaningful moment (not just a loading screen), even if the pieces are generic at this stage
- Details of piece generation magic are TBD

### First Bot Match (Tutorial)
- The player's first initiated battle is always against a bot
- The battle serves as a hands-on tutorial for the Battle Viewer
- Each other scene has its own tutorial overlay; onboarding funnels the player through the first one

## Open Questions / TBDs
- What does piece generation look like visually during onboarding?
- How much does onboarding explain about skill editing before the first match?
- Bot difficulty for tutorial match

## Dependencies
- `10-battle-simulation-engine` — bot opponent runs through same engine
- `11-server-backend` — account creation hits the server
- `09-settings-model-config` — model selection shares logic with Settings
