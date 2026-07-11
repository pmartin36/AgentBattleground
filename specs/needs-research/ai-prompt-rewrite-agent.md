# AI Prompt-Rewrite Agent (needs research)

> **Status: parked pending research.** This is the real behavior behind the "Prompt agent to update" input in the prompt editor popup (`51-prompt-editor-popup`), which ships that input as a **no-op stub**. Unnumbered until scoped, per the `needs-research/` convention.

## Purpose

Let the player type a natural-language instruction ("make him more aggressive", "focus on defending the back line") and have an LLM **rewrite the creature's battle-instructions Markdown file**, with the editor reloading to show the result. This is the in-game half of `03-army-skill-editing`'s authoring flow, applied to the per-creature instructions file defined in `47-ability-and-instructions-data-model`.

## Why it's parked

It pulls in several unresolved, security-sensitive decisions that shouldn't be guessed:

- **Model & config.** Local (FLUX4) vs. online (Claude/OpenAI) per `09-settings-model-config`; who picks, and the default. Latency/UX implications differ sharply.
- **The file/UI tool.** The core open question the owner raised: does the LLM get a **tool to read/write the instructions file and refresh the UI**, or does the app own the file I/O and pass only text to/from the model? A model-driven filesystem tool is the more powerful design but collides directly with the hard sandboxing constraint.
- **Sandboxing (hard constraint).** Per CLAUDE.md constraint 1 and `10-battle-simulation-engine`, the LLM must not act outside the game directory, and untrusted input must be sanitized. Any filesystem-capable tool must be constrained to `creature_instructions/` and nothing else.
- **Apply / discard UX.** Does the rewrite overwrite the file directly (leveraging spec 51's live write-through) or land in a diff/preview the player accepts or rejects? What happens to un-accepted edits already in the editor?
- **Async / streaming UX.** The rewrite is a network/inference round-trip inside a modal. Spinner, streaming into the editor, cancel, failure handling.
- **Prompt construction.** What context the model gets: the current instructions, the creature's abilities/stats (spec 47 data), the user's instruction, and house rules for the output format (must stay valid Markdown).

## Research questions to resolve before speccing

1. Model-driven file tool vs. app-owned I/O — and how each is sandboxed to `creature_instructions/`.
2. Overwrite-live vs. preview/accept, and how it interacts with spec 51's debounced write-through.
3. Streaming vs. blocking, cancellation, and failure/retry UX inside the modal.
4. Prompt/context assembly and output-format enforcement (valid Markdown, size bounds).
5. Local vs. online model selection and defaults (ties to `09`).

## Dependencies / relationships

- Replaces the stub in `51-prompt-editor-popup`.
- Constrained by `10-battle-simulation-engine` (sandboxing) and `09-settings-model-config` (model choice).
- Operates on files from `47-ability-and-instructions-data-model`; part of the `03-army-skill-editing` authoring vision.
