# Local Text Model — Bundled Runtime & Model Registry

> **Status: draft (not started).** Makes the game's LLM usage real and final: a bundled llama.cpp runtime (sibling binary, like `sd-cli`), a registry of a few permissively-licensed models with a default, a download-on-need install with checksum verification, a model-selection-centric config (a specific `model_id`, switchable, BYO later), the general public API (`respond`) the mad-lib/creature-gen pipelines call, and that hookup made real end-to-end. Turns spec `70`'s `Provider::Local` from a generic "run this command" into a registry-driven, installed model. Local is the shipped default; the online providers `70` built stay in the code as advanced / BYO options, not the promoted path.

## The LLM API (naming & shape)
`70`'s public call `generate_text` is renamed **`respond`** (`llm.respond(request) -> String`) to stop implying plain prose: an LLM always returns text (tokens), and a prompt like "give me this mad-lib as JSON" returns a String that happens to be JSON-shaped, not a different type. The return type stays `String` (the model's raw response); the caller parses it (labeled lines, JSON, an enum pick) and deterministically assembles the result. There is deliberately **no** `generate_json` / no operation that returns a typed game entity — parts-not-entities (see `70`) holds regardless of the response's format.
- For prompts that need reliable structure, the `request` carries an optional output constraint (a GBNF grammar / format), which llama.cpp applies natively so the returned text conforms (this answers `70`'s constrained-decoding open question and is useful for `parts.rs`'s `ARCHETYPE` pick and any JSON-shaped response). The response is still a `String` the caller parses.
- The API stays in its own module (`text_gen`; the module may be renamed `llm` for clarity, a mechanical change). `70`'s `TextBackend` trait, transport, conformance suite, routing, and cache are unchanged except for the call rename rippling to callers.

## Purpose
The mad-lib creature-parts generation (`67`/`71`, via `70`'s `generate_text`) must work locally on the player's machine with nothing else installed. `70` built the transport (`Provider::Local` -> `SiblingBinaryTransport` spawns a sibling command, pipes the framed prompt, reads stdout). What is missing is everything about *which* model and *how it gets there*: a runtime to bundle, a specific model to run, and the install flow. This spec fills that, so a fresh install can generate a creature offline after a one-time model download.

## Scope
- The bundled runtime: llama.cpp as a sibling binary (`llm-cli`), found next to the game executable exactly as `SdCliRunner` finds `sd-cli`.
- The model registry: a manifest of 2-4 selectable models (id, display name, params, GGUF URL, SHA-256, byte size, license, recommended flags), with a default.
- The download / install flow: fetch the selected model's GGUF on selection, verify its checksum, store it under the OS data dir, and know when a model is present vs. not.
- The config rework: make config model-selection-centric — `Provider::Local` resolves a `model_id` through the registry to a concrete weights path + runtime invocation (the default path), switchable by `model_id`, with the raw `local_command` kept as the BYO escape hatch. The online providers `70` built remain as advanced / BYO options, not the default.
- The backend invocation change: build llama.cpp's real one-shot flags and pass the model weights + the framed prompt.
- The public API rename: `generate_text` -> `respond`, with an optional structured-output constraint on the request (see The LLM API above); ripple the rename to callers.
- The hookup: `67` / `71` call `respond` against the resolved local model, made real end-to-end (a mad-lib Done generates parts on the bundled local model).

Out of scope: **the in-game model-selection menu / picker UI** — the settings menu (`09-settings-model-config`) is a placeholder spec, not yet built, and this spec does not build it. This spec defaults to one model and exposes the registry + install API that `09`'s model-selection screen will later drive (see Selection below). Also out of scope: the persistent `llama-server` transport for the battle engine (a future addition for `10`, which makes many sequential calls); building/shipping the llama.cpp binaries per platform (a packaging step, like `sd-cli`'s — this spec's code assumes the sibling binary is present, as the asset pipeline already assumes `sd-cli`); the animation-weights licensing question (`needs-research/local-generation-service-integration.md`, a separate, unrelated H3 issue).

## Selection (no picker UI yet)
There is no settings menu built yet, so this spec does not gate on one. The game uses a **single default model**, `qwen3-4b-instruct`, unless a `model_id` is set via env/JSON config (the escape hatch for choosing another registry entry or a raw `local_command`). The registry holds all three entries so the architecture is multi-model from day one, but choosing between them in-game is deferred to `09-settings-model-config`'s model-selection screen (a placeholder spec, unbuilt), which will drive this registry + install API when it is built. Because there is no in-game "select" action to hang a download on yet, the default model's weights are ensured present **on first need** (downloaded and verified when first required if absent), rather than the eventual download-on-select the picker will drive.

## Bundled runtime
llama.cpp (MIT, freely redistributable) is the runtime, bundled as a sibling binary named `llm-cli`, located via `current_exe().parent()` the same way `asset_gen/runner.rs`'s `SdCliRunner` locates `sd-cli`. It shares the ggml/Vulkan toolchain the project already builds for `sd-cli`, so the same build matrix produces it per platform (Linux / macOS-Intel / macOS-ARM / Windows). A CPU build is the universal baseline; a Vulkan build is the optional accelerated path, the same GPU-optional story `sd-cli` already has. Provisioning the binary onto the player's machine is a packaging/onboarding concern; this spec's code treats it as present and reports a clear "runtime missing" error when it is not (mirroring the no-GPU handling in `66`).

## Model registry
A manifest, in code, of the selectable models. Each entry: a stable `model_id`, display name, parameter size, GGUF download URL, **SHA-256**, byte size, an SPDX license id, and the recommended per-model invocation hints (e.g. chat-template handling). License is the gate: every registry entry MUST permit both redistribution of weights and unrestricted use of outputs.

Initial registry:
- **`qwen3-4b-instruct`** (default) — Qwen3-4B-Instruct, Q4_K_M GGUF (~2.5 GB), **Apache-2.0**. Strong instruction-following at its size; already validated on disk in this repo (`experiments/creature_lab`).
- **`phi-4-mini-instruct`** — Phi-4-mini-instruct, ~3.8B, Q4_K_M (~2.5 GB), **MIT**.
- **`smollm2-1.7b-instruct`** — SmolLM2-1.7B-Instruct, Q4_K_M (~1.1 GB), **Apache-2.0**. The low-RAM / small-download option.

Llama and Gemma models are deliberately excluded from the default set: Llama's license adds "Built with Llama" attribution and derived-model naming obligations, and Gemma's requires propagating a prohibited-use policy and reserves a remote usage-restriction right. Neither is acceptable for a redistributed game's default; they may be revisited only as explicitly-flagged, license-encumbered opt-ins.

## Download / install flow
- **Download on select**, not on first battle: when the player picks a model (onboarding `01` / settings `09`), its GGUF is fetched then, so generation never stalls mid-use. The registry entry's URL is the source (HuggingFace, optionally mirrored to the project's R2 for pinning).
- **Verify**: after download, check the file's SHA-256 against the manifest; on mismatch, discard and re-download.
- **Store** under the OS data dir the game already uses (`base_data_dir()`), e.g. `<data_dir>/models/<model_id>/<file>.gguf`.
- **Presence + offline**: the install layer reports whether a given `model_id`'s weights are present. Once present (plus the bundled runtime), generation is fully local and offline. A `Provider::Local` config naming a model whose weights are absent resolves to a clear "model not downloaded" error, never a silent failure.

## Config change (model_id, not a raw command)
`model_config.rs` gains a `model_id` for the `Local` provider (env `AGENTBATTLEGROUND_MODEL_ID` / a JSON field), resolved through the registry to the concrete runtime path (the `llm-cli` sibling) and the installed weights path. The existing free-form `local_command` remains as an escape hatch for a user-supplied binary, but the normal, shipped path is `model_id` -> registry -> (runtime, weights). The `Local` config is valid when it names a resolvable, installed `model_id` or an explicit `local_command`. Switching models is a config change of `model_id`, no code change. BYO is either the `local_command` (your own binary) or `70`'s `OpenAiCompatible` backend pointed at your own endpoint. This replaces the prior config's online-provider-first framing with a local-model-first, model-selection-centric one.

## Backend invocation change
`text_gen/backend_local.rs`'s `build_invocation` currently emits `--temperature/--max-tokens/--stop` and treats the program as a magic all-in-one binary that reads the prompt from stdin. llama.cpp's `llm-cli` uses different flags, needs the model weights path, and has **no stdin-prompt flag**. The backend must therefore:
- pass the weights: `-m <weights.gguf>`;
- feed the framed prompt cross-platform: write it to a temporary prompt file and pass `-f <file>` (avoids llama-cli's missing stdin input and argv-quoting of a large prompt; superseding the current stdin-piping for the local path);
- apply the model's chat template (`--jinja`) and single-turn output flags (`-no-cnv -st --no-display-prompt`) so an instruct model returns a clean completion on stdout;
- map the sampling params to llama.cpp's real names: temperature -> `--temp`, max tokens -> `-n`, seed -> `-s`;
- optionally constrain an enumerated field with a GBNF `--grammar` (this is `70`'s "constrained decoding" open question; llama.cpp answers it natively — useful for the `ARCHETYPE` pick in `parts.rs`).

`70`'s `TextBackend` trait, conformance suite, routing (`operation.rs`), cache, and `types.rs` are unchanged; the change is the registry + a richer `Local` config + corrected argv + the prompt-file mechanism.

## Dependencies
- `70-text-generation-api` — the `Provider::Local` backend and transport this makes real; the change is additive to `backend_local.rs` / `model_config.rs`.
- `67-hatchery-definition-generation` / `71-creature-construction` — the callers whose short structured prompts this model serves.
- `09-settings-model-config` / `01-onboarding-first-run` — the screens that drive model selection + the install flow this exposes; "FLUX4" in those specs is a placeholder this registry replaces.
- `asset_gen/runner.rs` (`SdCliRunner`) — the sibling-binary pattern the runtime reuses.
- `10-battle-simulation-engine` — a future consumer that will want the persistent `llama-server` transport (out of scope here).

## Open Questions / TBDs
- Whether battles later run a persistent `llama-server` (loopback, OpenAI-compatible via `70`'s existing `OpenAiCompatible` backend) to avoid per-call weight reloads; not needed for the hatchery's occasional calls.
- Mirroring the GGUF downloads to R2 vs. pulling from HuggingFace directly (reliability/pinning vs. simplicity).
- Exact per-model recommended flags and whether GBNF grammar-constrained decoding is enabled in this first pass or deferred.
