# Creature Audio Generation (needs research)

> **Status: needs research.** A dedicated local audio-generation model that produces a distinct, short sound per creature — primarily a signature "select" sound that plays when you pick a creature in the roster, and potentially idle/attack SFX. This is the audio path after the free option was ruled out: MiniMax H3 (the animation model) emits an audio track, but a real test found it indistinct — see Motivation. Not scheduled; this decides whether a good local creature-audio path exists and what it costs.

## Motivation (why not just use H3's audio)
H3 is a video model that also emits an audio latent, and the pipeline already generates it (`--audio-vae`). A live probe generated three clips and measured/heard the audio: it is real and loud (mean −6 dB for attacks, −12.9 dB for an idle — so it does vary a little with the action), but on listening, a beetle attack, a beetle idle, and a fire-wolf attack all "sound kind of similar." It reads as generic motion/foley weakly conditioned on the prompt, not a creature-specific sound. It is also a fixed ~0.9s regardless of clip length. Conclusion: reusing H3's baked audio as per-creature SFX would give every creature roughly the same whoosh — not worth building. Distinct creature audio needs a model built for sound effects.

## Goal
Given a creature's generated identity (its description / element / archetype from `71`/`67`), produce a short, distinct sound that:
- reads as *that* creature (a dragon roars, a mouse squeaks) — the H3 failure was indistinctness, so distinctness across creatures is the bar;
- is a good length for a UI/gameplay cue (a fraction of a second to a couple of seconds);
- plays at the right moment — the primary ask is a **roster "select" sound** (fires when a creature is selected/focused in the roster), with idle-loop ambience and attack SFX as possible extensions.

## What to research
1. **Model candidates.** Local text-to-audio / text-to-SFX models capable of short, distinct sound effects from a text prompt — e.g. Stable Audio Open, AudioLDM 2, AudioGen, and any newer permissively-licensed option. For each: does it actually produce *distinct* sounds across different creature prompts (test it, the way the H3 audio was tested), what length/quality, and what is the license (redistribution of weights + use of outputs must be permissive, per the same bar the text/image registry uses — reference `73`'s license gate).
2. **Runtime / bundling.** Can it run natively as a bundled sibling binary with no external apps, matching the `sd-cli`/`llm-cli` pattern (`73`, `66`)? Audio models may not fit `stable-diffusion.cpp`; determine the runtime (a ggml-based audio runner, ONNX, a small dedicated binary) and whether it is redistributable and self-contained (CPU baseline + optional GPU). Weight size and download flow (the deferred model-download/registry work would extend to cover it).
3. **Prompt derivation.** What text produces a good creature sound — the creature's flavor description directly, or a sound-focused prompt derived from it (element/archetype → "a crackling electric strike", "a low earth rumble"). This is a `71`-style "parts" question: the model builds the sound; deterministic code picks the prompt.
4. **When it generates.** Generate-on-definition (alongside the still/clips during incubation, stored as a per-creature asset like the sprite/clips) vs. generate-on-first-select. Storage mirrors `66`'s per-creature asset handles.
5. **Playback + triggers.** Wire the stored sound through `engine-audio` (`57`, the SFX bus) at the trigger points: roster select (primary), idle loop, attack. Volume/one-shot vs. loop behavior.

## Open questions
- Is a genuinely distinct, controllable local SFX model available under a permissive license today, or is this blocked the way H3's animation licensing is?
- Roster-select sound only (smallest, clearest win) vs. the fuller idle + attack SFX set.
- Whether the audio is a per-creature generated asset (varies per creature, needs storage + generation time) or a smaller set of element/archetype-keyed sounds selected deterministically (cheaper, less unique) — the same "generated vs. rule-selected" fork `71` settled for stats/attacks.

## Dependencies
- `57-engine-audio-api` — the SFX bus this plays through.
- `66-asset-generation-api` / `74` — the bundled-native-runtime + per-creature-asset pattern this would extend to audio; the deferred model download/registry flow would need an audio entry.
- `73-local-text-model` — the license gate and bundled-sibling-binary pattern to mirror.
- `71-creature-construction` / `67-hatchery-definition-generation` — the creature description that drives the sound prompt.
- `24-roster-carousel` / roster detail views — where the select-sound triggers.
