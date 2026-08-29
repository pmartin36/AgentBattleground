> # ✅ DONE! — Completed 2026-08-29

# Text Generation API

> **Status: done.** The single in-game API for producing text from the configured model: a prompt goes in, generated text comes back. One API any feature calls; it routes to the player's configured backend (a local model subprocess, or an online provider over HTTP) and manages the job lifecycle. Game-specific (`crates/game/`), structured to mirror the asset-generation API (`66`). This spec defines the runtime model interface that `09-settings-model-config` and `10-battle-simulation-engine` both leave as an open question.

## Purpose
Give every feature one uniform way to ask the configured model for text, so no feature re-wires model invocation for itself. The API owns the job lifecycle, backend routing, and the transport to the local subprocess or online provider. It is the counterpart to `66` for text: `66` turns prompts into art, this turns prompts into text.

The two features that need it are the battle simulation engine (`10`, deciding each turn's actions) and the hatchery (`67`, meta-generating mad-lib templates and reading a creature's traits from the finished sentence). Both currently have nothing to call.

## The model builds parts; the game assembles (load-bearing)
The model never emits a finished game entity. It returns interpretive text: a short phrase, a classification, a weighting, a chosen name. Deterministic game code then assembles the actual entity (a creature's stats, abilities, and starting attack; a turn's resolved move) from those parts plus fixed game rules.

Concretely, a creature is not "generated as JSON by the model." The game holds a fixed stat-point budget; the model reads the mad-lib description and yields a *weighting* over stats (and leanings such as element or class); deterministic code distributes the fixed budget by that weighting and selects abilities and the one starting attack by rule. The model influences the shape, never authors the result.

This is why the API is text-only and has no structured-output operation: a model emitting the finished structure would be non-reproducible, unbalanced, and authoritative over game state, all of which this design refuses. Parsing the model's small parts and assembling the entity is caller code, and it is deterministic.

## Scope
- The API surface and its request/response types: freeform text completion.
- The two backends: a local model via subprocess, and online providers (Claude, OpenAI, and API-compatible) over HTTP.
- Backend selection from the player's `09` model configuration, including the online API key.
- The async job lifecycle: submit, poll, success/error/timeout, never a silent stall (same lifecycle machinery as `66`).
- The safe-transport boundary: what this API guarantees, and what prompt sanitizing/output handling it leaves to the caller.

Out of scope: which model is recommended, onboarding/auto-setup, and where the API key is stored (`09`); battle prompt framing, skill-file sanitizing, and turn-output validation (`10`); mad-lib prompt design and the deterministic creature assembly (`67`). This API is deliberately content-free: it moves a prompt to a model and text back, and knows nothing about battles or creatures.

## The API
One operation, returning a job handle that resolves to the model's text output, mirroring `66`'s handle-based lifecycle:

```
generate_text(TextRequest) -> String
```

Run the assembled prompt and return the model's completion as text. Callers that need a small structured part (one of an allowed set, a number, a short list) prompt for exactly that and parse it themselves, then assemble and validate the result in their own code. There is no operation that returns the model's own JSON as game data.

## Request / response shapes
- **TextRequest** the prompt (a system frame plus the user content the caller has already assembled), sampling params (temperature, max tokens, stop sequences), and an optional seed. The caller owns prompt construction; the API does not inject or wrap content.
- **Response** the completion string, or a structured error on model failure or timeout.
- **ResolvedModelConfig** the single resolved configuration the API is constructed with: the selected provider, the online API key or the local model command, and a **stable model identity** (a short string naming the model/provider). This is one shared foundational type, defined once and depended on by the backends and the cache; it is not re-declared per backend. The cache-key model identity (below) is this type's identity field. The config is authoritative over *routing only*, never over game state.

## Uniform behavior
- **Async job lifecycle** submit, then poll on a fixed interval, resolving to success, a real error, or an explicit timeout; a caller is never left with no signal. It mirrors the *shape* of `66`'s lifecycle (a serial-queue submit/poll/timeout) as its own `text_gen` module with a text-specific work unit (a backend call) and error type. It does **not** reuse `66`'s `JobQueue`/`JobStatus` in place: those are coupled to the `sd-cli` subprocess invocation and asset_gen's concrete error, and the online backend is an HTTP call, not a subprocess. This API therefore edits no asset_gen file. (A future streaming mode is an open question below; the baseline is a single resolved response.)
- **Backend routing** the API is given a **resolved model configuration** (the selected provider plus the online API key or the local model command) and routes on it at submit time. It does not read a settings file: persisting, editing, and choosing that config is `09`'s job, and `09`'s config-file layer does not exist yet, so the config is injected into the API rather than loaded by it. Switching models is `09` handing the API a different resolved config; no call site changes.
- **Caching** unlike `66`, text generation does not cache by default: a battle turn's context is unique and its output is intentionally non-deterministic, so caching would be wrong. Caching is opt-in per request (keyed by the model identity from `ResolvedModelConfig`, the prompt, the params, and the seed) for the deterministic, seed-pinned cases such as regenerating the same mad-lib template. The model identity in the key comes from `ResolvedModelConfig`, so the cache depends on that one type, not on either backend.

## Backends
Both implement one `TextBackend` trait, the text analogue of `66`'s `RecipeBackend`. The API really produces text; it does not defer generation to a caller.

### Local model (subprocess)
- The configured local model (spec `09`'s recommended local model or an equivalent), invoked out-of-process the way `66` drives `sd-cli`: a sibling binary or a local model server the game talks to over a loopback socket. Which of those two is the concrete transport is an open question below.
- Runs fully on the player's machine, so battles need no network once opponent data is downloaded (the `09`/`10` local-first premise).

### Online providers (HTTP)
- Three provider categories, each a `Provider` variant with its own tested request/response shaping behind the same `TextBackend`: Claude, OpenAI, and OpenAI-API-compatible (a distinct category, an OpenAI-shaped API at a caller-supplied base URL). Selected in `09`, using the player's own API key. The API sends the request over HTTPS.
- The player owns latency and cost; `09` surfaces the cost warning. This API does not add its own retries that would silently multiply billed calls.

## Sandboxing boundary (critical)
Constraint #1 (LLM sandboxing) is satisfied jointly, and this spec draws the line:
- This API guarantees **safe transport**: it feeds a prompt to a model and returns text. It exposes no tool-use, function-calling, or filesystem/shell access to the model, and it never evaluates model output as code or commands. The output is always inert text.
- The caller owns **content safety**: sanitizing untrusted input (opponent skill files) and wrapping it in a constrained prompt frame before it reaches a `TextRequest`, and parsing/validating the returned text into legal game state. This is `10`'s sandboxing section and stays there; the API cannot know which parts of a prompt are untrusted.

Stated plainly: the API makes the model unable to *act*; the caller makes the prompt safe to *send* and the output safe to *use*. Combined with the parts-not-entities rule above, the model is never authoritative over game state.

**Backend conformance (inherited by default, not opt-in).** Safe transport is not re-derived in each backend. Output safety is structural: the `TextBackend` trait returns an inert `String` the API never evaluates, so every present and future backend inherits it from the signature alone. Request-side restraint (no tool-use/function-calling exposed to the model; the model given no filesystem or shell access beyond the one explicitly configured local command) is enforced by a **shared backend-conformance test suite that every `TextBackend` implementation must pass**.

Enforcement is structural, not per-backend memory. Every backend is constructed through one routing function `backend_for(ResolvedModelConfig)`, keyed on a closed `Provider` enum; nothing else constructs a backend. A single conformance test iterates **every** `Provider` variant through `backend_for` and runs the shared harness on each result. Adding a backend is a new `Provider` variant plus a `backend_for` arm, which the variant-iterating test covers automatically, so a backend cannot ship without passing the suite. This is the class-of-problem guard the project rule asks for: every future implementor gets it by default, not by remembering to opt in.

## Placement
`crates/game/`, alongside `asset_gen`. The API, its lifecycle, and its backends are this game's, structured to mirror `66`. The local backend drives its model out-of-process via the sibling-binary/loopback pattern the game already uses for the inspector and `sd-cli`.

## Consumers
- `10-battle-simulation-engine` proposes each active piece's turn action as text over sanitized skill-file prompts; the engine parses that into structured turn data and validates it against the rules. The engine is the authority; the model advises.
- `67-hatchery-definition-generation` meta-generates varied mad-lib templates, and reads the completed sentence into traits (a stat weighting, element/class leanings, a name). Deterministic hatchery code then allocates the fixed stat budget and assembles the abilities and the one starting attack.
- Any later feature needing model text calls this rather than invoking a model directly, and assembles its own result from the returned text.

## Open Questions / TBDs
- Local transport: a one-shot sibling binary per call (like `sd-cli`) versus a persistent local model server over a loopback socket. The persistent server avoids reloading model weights per call, which matters when a battle makes many sequential calls.
- Constrained decoding: whether the local backend should offer grammar- or enum-constrained sampling so a caller's small enumerated part (pick one of N) parses reliably. Even with it, the final entity is still assembled by deterministic caller code; this only makes the model's parts easier to parse.
- Streaming: whether the battle viewer wants token-by-token streaming for a live feel, or a single resolved response per turn is enough. Baseline is a single response.
- Concurrency: whether more than one text job may run at once against a single local model, given weight-loading and memory cost. Default assumption is a serial queue, as with `66`.
- Shared lifecycle extraction: whether to later factor `66`'s and this API's separate serial-queue lifecycles into one generic job lifecycle (generic over work unit and error, possibly engine-level), replacing the deliberate shape-duplication this first pass accepts to avoid refactoring committed asset_gen code.

## Dependencies
- `09-settings-model-config` the model choice, provider, and API key this API routes on, handed to the API as a resolved config (this spec does not read or persist settings). This spec resolves `09`'s open question of how the model is used at runtime.
- `10-battle-simulation-engine` the primary consumer; this spec resolves `10`'s open question of the exact LLM interface. Skill-file sanitizing and turn-output validation stay in `10`.
- `67-hatchery-definition-generation` consumes `generate_text` for templates and trait reading; the deterministic creature assembly lives there.
- `66-asset-generation-api` the sibling API whose job-lifecycle machinery and out-of-process pattern this one reuses; the two together are the game's generation layer.
