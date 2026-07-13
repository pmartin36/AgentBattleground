> # ✅ DONE! — Completed 2026-07-13
> Status: implemented via the tdd-pipeline, shipped to `main`.

# `@` Mention Authoring

## Status

Done (shipped to `main`). Adds an inline `@` autocomplete to the engine `TextEditor` plus this game's mention vocabulary, so authors can reference creatures, the creature's own abilities, and dynamic battle targets from within the instructions Markdown. **Authoring + token storage only** — actually *resolving* a mention at battle time ("who has the most HP") is the battle engine's job, deferred to `10-battle-simulation-engine`.

## Purpose

Battle instructions read like "when @self:frozen, use @Douse" or "attack @enemy:lowest-hp". Typing `@` should pop an autocomplete of valid references and insert a structured, readable token into the file. The token is a stable, parseable reference (not just free text) that survives external hand-editing and that `10` will later resolve against live battle state.

## Architecture — engine mechanism, game vocabulary

- **Engine (`TextEditor`, `crates/engine/render`):** a generic inline autocomplete. Configurable trigger char (`@`). On trigger it opens a small popup **anchored at the caret**, captures the query the author keeps typing, asks a caller-supplied **candidate provider** for matching candidates, shows them (highlightable list), and on accept inserts the chosen candidate's `insert_text` (replacing the `@query`). The engine knows nothing about creatures/abilities — it only drives the popup + insertion. Reusable by any game.
- **Game (`crates/game`):** a `MentionProvider` that, given the current creature + query string, returns candidates. This is where the vocabulary lives.

### Provider contract (engine-side shape)
```rust
struct MentionCandidate {
    display: String,
    insert_text: String,
    category: &'static str,
    /// true  = accepting inserts `insert_text` and KEEPS the popup open,
    ///         re-querying with the new text (the two-stage target→selector
    ///         flow: picking `enemy` inserts `enemy:` and stays open).
    /// false = terminal: accepting inserts and closes.
    continues: bool,
}
trait MentionProvider { fn candidates(&self, query: &str) -> Vec<MentionCandidate>; }
```
The editor is handed a `MentionProvider` (and the trigger char). The prompt-editor popup (`51`) constructs the game provider for the creature being edited and passes it in.

## Autocomplete behavior (engine)

- `@` opens the popup at the caret; each subsequent char extends the query and re-filters (provider called with the query); `Backspace` past the `@` closes it.
- `Up/Down` move the highlight; `Enter`/`Tab`/click **accept**; `Esc` dismisses (leaving the literal `@query` typed).
- **Two-stage flow.** Accepting a `continues` candidate (a target — `self`/`ally`/`enemy`) inserts its `insert_text` = the target plus an auto-appended **`:`**, and KEEPS the popup open, re-querying so the provider now offers only the **selectors valid for that target** (self → statuses; ally/enemy → `most-hp`/`least-hp`/`highest-damage`/status). Accepting a terminal candidate (a selector, ability, creature, or a bare target) inserts and closes. The caret lands after the inserted text.

## Mention vocabulary (game) + token grammar

The token is readable `@`-prefixed text stored verbatim in the Markdown. Categories the provider offers:

1. **Targets** (keywords): `@self`, `@ally`, `@enemy` — valid bare, or qualified via a `:` selector (the `:` is auto-inserted when you pick the target — the two-stage flow above).
2. **Qualified targets** — `@<target>:<selector>` (one token). **`@self` takes only a status** (self is a single creature, so `most-hp`/`least-hp`/`highest-damage` are meaningless for it); **`@ally`/`@enemy` take any selector** (they pick one out of a group). Selectors (v1, hyphenated):
   - `most-hp`, `least-hp` — **ally/enemy only**
   - `highest-damage` (highest attack damage) — **ally/enemy only**
   - a **status** from `55`: `burn`, `frozen`, `shocked`, `rooted` — **any target**, e.g. `@self:frozen` ("am I frozen"), `@enemy:frozen` ("an enemy that is frozen").
3. **Own abilities** — the current creature's abilities by name: `@Douse`, `@Kick`.
4. **Specific creatures** — a roster creature by name (spaces→underscores, matching the instructions filename convention): `@Ember_Wolf`.

Grammar summary (what `10` will parse; defined here so authoring emits it consistently):
```
mention   := "@" ( "self" (":" status)?
                 | ("ally" | "enemy") (":" selector)?
                 | name )
selector  := "most-hp" | "least-hp" | "highest-damage" | status
status    := "burn" | "frozen" | "shocked" | "rooted"
name      := an ability name (own) or a creature name (roster), underscored
```

## Scope

- **In:** the engine autocomplete mechanism; the game `MentionProvider` (the vocabulary above); inserting the token text; wiring the provider into the prompt-editor's instructions field (`51`); the grammar definition.
- **Out:** resolving a mention against battle state (`10`); mention token *rendering* as a distinct styled chip (v1 leaves the inserted `@token` as plain text — distinct styling is a later polish); the `/` (or other) trigger; horizontal scroll.

## Decisions (v1)

- Engine drives a generic `@`-triggered autocomplete over a caller-supplied `MentionProvider`; the vocabulary is game-side.
- Tokens are readable `@`-text stored verbatim in the Markdown (hand-editable, round-trips); resolution deferred to `10`.
- v1 vocabulary: self/ally/enemy (± `:selector`, self=status-only), own abilities, roster creatures — per the grammar above.
- Two-stage autocomplete: pick target → `:` auto-appears → selectors filtered by target.
- Statuses/selectors reference the `55` enums.

## Open Questions

- **Creature vs. ability name collision:** both are bare `@Name` (owner: both fine as bare). The provider lists both, categorized so the author picks the right one via arrow-key nav; `10` disambiguates by context (ability = the creature's own list; creature = roster).
- Distinct in-editor **token styling** (chip) — deferred polish.
- Final selector spellings — `most-hp`/`least-hp`/`highest-damage` (pin exact wording during build; owner used both "least" and "lowest").
- Separator is `:` (`@enemy:lowest-hp`); revisit if the owner prefers `-` throughout (`@enemy-lowest-hp`).

## Testing Guidance (headless)

- Typing `@` opens the popup; typing filters (provider called with the query); `Esc` leaves literal text; `Enter` inserts the candidate's `insert_text` and closes.
- The game provider returns the expected categories for a creature: its own abilities, roster creatures, `@self/@ally/@enemy`, and (after a `:`) the target-filtered selectors (`@self:` → statuses only; `@enemy:` → `most-hp`/`least-hp`/`highest-damage`/status).
- Two-stage accept: accepting the `enemy` target inserts `@enemy:` and keeps the popup open (`continues`); then accepting `lowest-hp` yields the single token `@enemy:lowest-hp`; reading the file back yields it verbatim (round-trips).
- Grammar: the emitted tokens match the grammar (a small parser-shape test, even though full resolution is out of scope).

## Dependencies

- Extends `50-engine-text-editing-primitives` (+ `53` caret); the popup is a new inline overlay in the editor.
- Needs `55-combat-status-and-element-enums` (status/element vocabulary) and `47-ability-and-instructions-data-model` (abilities, creature names).
- Wired into `51-prompt-editor-popup` (the instructions field gets the provider).
- Feeds `10-battle-simulation-engine` (resolution) and the `03-army-skill-editing` skill-file vision.
