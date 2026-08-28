# Animation model weights — redistribution licensing (needs decision)

> **Status: parked pending decision.** `64-creature-animation-pipeline` runs natively on
> `stable-diffusion.cpp` (no Python), so there is no runtime or packaging blocker. The one remaining
> pre-ship item is whether the model weights may be bundled and redistributed inside a shipped game.
> This gates shipping `64` and anything depending on it for a player-facing capability (e.g.
> `65-hatchery`); it does not gate continued development.

## The decision
May the animation stack's weights be bundled/redistributed in a distributed build (free or commercial)?
The stack: the MiniMax H3 diffusion transformer (+ its GGUF re-quantizations), the Qwen3-VL-32B text
encoder, and the Turbo LoRA.

## Current license terms
- **MiniMax H3** (MiniMax H3 Community License) — use, distribution, and *outputs* are restricted to
  the "Applicable Territory," which **excludes the EU, the UK, South Korea, and the United States**.
  Redistribution additionally requires attaching the agreement and a NOTICE file and binding every
  downstream recipient to equally-protective terms. Commercial use is free below $20M/yr revenue;
  above that, prior written authorization is required. This is the blocker.
- **GGUF re-quantizations** of H3 — inherit the H3 Community License; no additional freedom.
- **Turbo LoRA** — declared Apache-2.0.
- **Qwen3-VL-32B** encoder — Apache-2.0.

The permissive components do not lift the H3 base-license restriction. For a build distributed in any
excluded territory (which includes the US), shipping the H3 weights, and arguably using their outputs,
is not permitted under the current license.

## Open sub-questions
- Whether a bake-outputs-only pipeline (ship the finished frame assets, not the weights) clears the
  license, given the outputs clause. Needs a direct read, possibly counsel.
- No-GPU fallback and cross-platform/cross-hardware validation are tracked in `64` itself, not here.

## Dependencies
- Gates shipping (not development) of `64-creature-animation-pipeline` and dependents (`65-hatchery`).
