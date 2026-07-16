# Instructions Diagnostics

## Status

Pending. Flags problems in a creature's battle-instructions file and surfaces them in the roster details panel (`48`) and the prompt-editor popup (`51`).

## Purpose

A player writes battle instructions as free Markdown with `@` mentions (`56`). Two things silently go wrong today:

1. The file grows past the point where the model reliably follows all of it.
2. An `@` mention names something that does not exist — a creature off the roster, an ability the creature doesn't have, a selector invalid for its target. Nothing catches this: `is_valid_mention` (`crates/game/src/mention.rs:154`) checks grammar shape only, `is_name` accepts any colon-free token, and **nothing calls it** — it is dead code today.

This spec adds a linter over the instructions text, a `[!!]` badge where problems exist, and a hover card listing them.

## Why a word count (and why 500)

**The evidence says instruction *count* drives degradation, not length.** ManyIFEval measures prompt-level accuracy (every instruction satisfied) against instruction-level accuracy (the fraction satisfied) as instruction count rises:

| Model | prompt-level n=1 → n=10 | instruction-level n=1 → n=10 |
|---|---|---|
| Claude 3.5 Sonnet | 0.95 → 0.48 | 0.95 → 0.93 |
| GPT-4o | 0.94 → 0.21 | 0.94 → 0.85 |
| Gemma2-9B | 0.91 → 0.04 | 0.91 → 0.74 |
| Llama3.1-8B | 0.82 → 0.02 | 0.82 → 0.72 |

The failure mode is *not* "the creature breaks" — per-instruction compliance stays at 72–93%. The model keeps following most directives and reliably drops a few. For the ~7B-class local model this game recommends, ~10 directives already means a ~2–4% chance of following all of them.

**Directive count is nonetheless rejected as the metric**, because it is not robustly computable: a player may write one unbroken paragraph, and counting imperatives in free prose is a heuristic that silently undercounts. A metric that only works on well-formatted files is worse than an honest proxy. Word count is trivially computable, legible to the player, and correlates in practice — nobody writes 900 words containing two rules.

**500 is engineering judgment, not a citable finding.** No study measures a natural-language behavioral-directive blob at this scale. The only direct word anchor in the literature is AgentIF's *"when instruction length exceeds 6,000 words, the ISR scores of all models are nearly 0"* — an order of magnitude above this regime and useless for calibration. No Anthropic or OpenAI guidance states a length above which instructions get ignored. Long-context work (lost-in-the-middle, NoLiMa, RULER) measures recall, not adherence, and starts degrading above this scale.

500 words ≈ 13 directives at this game's observed prose density (`Ember_Wolf.md`: 1004 words / 26 bullets ≈ 39 words per directive), landing inside the band where a small local model visibly drops rules. **Pin it as tunable; the rigorous answer is a sweep against the actual local model, deferred.**

Sources: ManyIFEval [2509.21051](https://arxiv.org/abs/2509.21051) · IFScale [2507.11538](https://arxiv.org/abs/2507.11538) · WildIFEval [2503.06573](https://arxiv.org/abs/2503.06573) · AgentIF [2505.16944](https://arxiv.org/abs/2505.16944).

## Architecture — one vocabulary, two consumers

`GameMentionProvider` (`crates/game/src/mention.rs:36`) already snapshots exactly the context a linter needs: the edited creature's ability names and the roster's creature names. **Extract a `Vocabulary` that both the autocomplete and the linter consume**, so the offered candidates and the accepted tokens cannot drift apart.

```rust
pub struct Vocabulary {
    ability_names: Vec<String>,              // the edited creature's own
    roster: Vec<(String, SquadRole)>,        // name + positional role
}

pub enum DiagnosticKind {
    PromptTooLong { words: usize, limit: usize },
    UnknownTarget { token: String },          // @foo:burn
    BadSelectorForTarget { token: String },   // @self:most-hp
    NotAnAbility { token: String },           // @Frost_Bite on Ember Wolf
    CreatureNotOnRoster { token: String },    // @Volt_Scorpion
    CreatureNotFielded { token: String },     // on roster, but Reserve
}

pub struct Diagnostic {
    pub kind: DiagnosticKind,
    /// Byte range in the source. `None` for whole-file diagnostics
    /// (`PromptTooLong`).
    pub span: Option<Range<usize>>,
    pub message: String,
}

pub fn lint(text: &str, vocab: &Vocabulary) -> Vec<Diagnostic>;
```

### Grammar layer must return a *reason*, not a bool

`is_valid_mention` (`mention.rs:154`) returns a bare `bool`, and it **already rejects** both `@foo:burn` (`_ => false`, :161) and `@self:most-hp` (`"self" => is_status(selector)`, :158). So two of the six diagnostics — `UnknownTarget` and `BadSelectorForTarget` — live *inside* what the current grammar layer discards wholesale. A resolver that delegates grammar to `is_valid_mention` therefore **cannot** distinguish them, and "resolve_mention layers on top of is_valid_mention" is not implementable as stated.

The grammar layer is reworked to return a reason:

```rust
pub enum GrammarError { UnknownTarget, BadSelectorForTarget, Malformed }
pub struct ParsedMention { /* target/selector/name, as parsed */ }

/// Context-free parse. Returns WHY it failed, not just that it did.
pub fn parse_mention(token: &str) -> Result<ParsedMention, GrammarError>;

/// Context-aware. Parses, then checks names/roles against `vocab`.
pub fn resolve_mention(token: &str, vocab: &Vocabulary) -> Result<Resolved, DiagnosticKind>;

/// Kept as a thin wrapper so its existing tests (`mention_grammar_tests.rs`)
/// keep passing unchanged: `parse_mention(token).is_ok()`.
pub fn is_valid_mention(token: &str) -> bool;
```

**Resolution is the single entry point `10` will reuse.** Spec `10` needs exactly `resolve_mention` to resolve a mention against live battle state — build it as shared resolution, not a lint-only hack.

### Roster roles

**Only `Active` and `Bench` are valid mention targets. Anything else is flagged, unconditionally.**

`ROSTER_SIZE` is 6 (3 Active, 1 Bench, 2 Reserve — `squad_role.rs`), but **8 creatures are bundled**. So two distinct diagnostics, both always emitted:

- `CreatureNotOnRoster` — named creature is not among the 6 (e.g. `@Volt_Scorpion`, or a typo).
- `CreatureNotFielded` — on the roster but in **Reserve**, so it cannot appear in a battle.

Two kinds rather than one so the message can be specific ("not on your roster" vs. "in reserve, not active or bench"); the flagging rule itself is identical for both. A player writing instructions that anticipate a future squad change still gets warned — that is intended, not a false positive.

`SquadRole` derives from roster position — never a stored field.

## Lexing

The real file wraps mentions in Markdown code spans (`` `@Howl` ``), so **code spans and fenced blocks are scanned, not skipped** — that is the established authoring convention.

- A mention starts at an `@` **preceded by a non-word character** (or start-of-file). This excludes `foo@bar.com` without needing an email special case.
- Token: `@` then `[A-Za-z_][A-Za-z0-9_-]*`, optionally `:` then `[a-z-]+`.
- A trailing bare `@target:` (the intermediate two-stage form) is **not** diagnosed — it is a half-typed token, not an error.

## Rendering

### Badge

`[!!]` — plain amber text (`WARNING_COLOR`), one row, rendered via `engine_render::label`.

Rule 4 note: the badge is **HUD copy**, on the text side of the braille rule, alongside the panel's existing "Instructions" / "Abilities" / "Edit" / "STR" labels. A braille glyph was prototyped and rejected: at a 2×2-cell budget the icon area is 4×8 dots (dots are square; a cell is 2×4, so 2×2 cells is a 1:2 *tall* box, not a square), which cannot hold a legible warning triangle — a triangle rasterized 4 dots wide degenerates into a column that flares at the base. `⚠` itself is rejected: it is East-Asian-Ambiguous width, so terminals disagree on one cell vs two and a two-cell render would shift the layout. `[!!]` is ASCII, always exactly 4 cells.

- **Details panel** — in the Instructions header row, in the gap between the "Instructions" label and the Edit button (~9 cells free; verified in place). Absent entirely when there are no diagnostics.
- **Prompt editor** — on the file-path row beneath the editor.

### Hover card

Reuses the ability tooltip's look via a **shared shell extracted inside the game crate** (`roster_manager/tooltip/`), per owner decision — spec `49` declared the tooltip bespoke and non-engine, and that holds.

- Extract the frame + anchor + `Dot::Occlude` fill + interior `flex` into a content-agnostic shell; the ability card and the warning card both call it. This kills the duplication without promoting it to an engine primitive.
- `TOOLTIP_WIDTH_CELLS = 36`, anchored **up-left**, matching the ability card. Verified in place: from the details-panel badge the card lands at cols 42–78 / rows 8–16, floating over the creature art and covering the VIT stat label. That is accepted — it is transient, `Occlude` keeps it legible, and the ability card already behaves this way.
- **The shell adds edge clamping**, which spec `49` deliberately skipped. The details-panel badge does not need it (verified on-screen), but the **editor badge does**: anchored near the popup's bottom-left, a 36-wide card up-left would run off-screen.
- Contents: a count header (`"2 issues"`), then one wrapped entry per diagnostic.

### Editor inline marking

Bad mention spans get a **background tint** on their cells.

**This cannot be braille.** A braille dot cannot share a cell with a text glyph — the cell holds the letter. (This is why the existing pills are 3 cells tall: `tooltip/pills.rs`.) So no squiggle, no underline dots; cell styling is the only mechanism, and the selection highlight already sets the precedent.

**Engine change (`crates/engine/render/src/text_editor/`).** `draw_selection` (`render.rs:221`) already maps a `(line,col)→(line,col)` span onto wrapped rows and tints those cells, with a hardcoded `SELECTION_BG` and a hardcoded span source. Generalize it into a span-decoration pass:

```rust
pub struct Decoration { pub span: ((usize, usize), (usize, usize)), pub bg: Color }
impl TextEditor {
    pub fn set_decorations(&mut self, decos: Vec<Decoration>);
    /// Hit-test for hover -> index into the decoration list.
    pub fn decoration_at(&self, pos: (usize, usize)) -> Option<usize>;
}
```

Selection becomes one caller of the pass, diagnostics another. The *mechanism* is engine-side so any future game's editor inherits it; the *content* stays game-side, mirroring how `MentionProvider` already splits.

**Required exports (`crates/engine/render/src/lib.rs`).** `lib.rs:38` currently re-exports only `EditorEvent, MentionCandidate, MentionProvider, Sizing, TextEditor, TextEditorConfig`. This task **must also export `Decoration` and `SELECTION_BG`** and its `TOUCHES` must include `lib.rs`:

- `SELECTION_BG` is `pub(super)` at `render.rs:19` and unreachable from `crates/game`. The game picks `DIAGNOSTIC_BG` and must not collide with it — a caller choosing a decoration colour legitimately needs to know the selection colour. Without the export the only way to satisfy "`DIAGNOSTIC_BG != SELECTION_BG`" is to **duplicate the literal `Color::Rgb(0x2f, 0x4f, 0x6f)` game-side**, which silently breaks the moment `SELECTION_BG` is retuned. Export it; never re-declare it.
- Spell the span as explicit `(usize, usize)` tuples, not the private `Pos` alias (`text_editor/mod.rs:25`), so game-side code and engine-side code name the same type.

**Precedence is engine behavior, tested engine-side.** "An active selection over a decorated span still renders `SELECTION_BG`" is a property of the pass itself, so it is asserted in `text_editor/render.rs`'s own test module where `SELECTION_BG` is in scope — not from the game crate.

"Only engine change" means only engine *behavior* change. The exports above are part of this task, not a separate one.

## Module placement (file-size budget)

`docs/large-file-split-plan.md` sets a **~1000-line target per file**, and states that the splits it lists are **explicitly not pipeline work** — *"project owner's explicit call: one-off mechanical refactor, no new behavior... parked here so the work is captured without blocking on it."*

**This feature therefore performs no splits.** Do not add a split/refactor task for `details_panel.rs`, `prompt_editor.rs`, or anything else — that is parked work and out of scope here. Instead, the budget is respected by putting the new code in a new module:

- **All new diagnostics rendering lives in `crates/game/src/scenes/roster_manager/diagnostics_ui.rs`**, with its own sibling test file `diagnostics_ui_tests.rs` — badge rendering, the warning card's row content, and their tests.
- **`details_panel.rs` (946 lines, 54 of headroom) and `prompt_editor.rs` (1032 lines, already over) gain call sites only** — a handful of lines each, no test growth. This is a hard requirement, not a best-effort assumption: both files colocate their tests, so any test written into them lands in the file itself.
- The shared tooltip shell extraction lives under `tooltip/` (`mod.rs` is 756 lines; put the shell in a new `tooltip/shell.rs` rather than growing `mod.rs`).

## Scope

- **In:** `Vocabulary` extraction; `parse_mention` + `resolve_mention`; the six `DiagnosticKind`s; `lint`; the `[!!]` badge in both surfaces; the shared tooltip shell + clamping; the engine span-decoration pass + its exports; hover wiring.
- **Out:** resolving mentions against live battle state (`10`); conflict detection between directives; auto-fix / quick-fix; a hard cap or any block on saving; watching the file for external edits (`03`); severity levels beyond warning; **any file split/refactor from `docs/large-file-split-plan.md`**.

## Decisions (v1)

- **Word count, not directive count** — 500 words, tunable. A proxy by choice; the reasoning and its limits are recorded above.
- **Warning-only severity.** No error tier; every diagnostic is amber.
- Badge is `[!!]` amber **text**, not a braille glyph (rationale above).
- Tooltip shell extracted **inside the game crate**, not promoted to engine; gains clamping.
- Card stays 36 cells, up-left anchor, matching the ability tooltip; covering the creature art is accepted.
- Inline marking is a **background tint**; braille is impossible over text.
- Span decoration is an **engine** mechanism; diagnostics vocabulary is **game**.
- Code spans are scanned; `@` must follow a non-word char.
- **The grammar layer returns a reason (`parse_mention -> Result<_, GrammarError>`), not a bool** — `UnknownTarget`/`BadSelectorForTarget` are otherwise unreachable, since today's `is_valid_mention` already discards both. `is_valid_mention` survives as a thin wrapper so its tests keep passing.
- **Count header pluralizes** (`1 issue` / `2 issues`) — previously a TBD, now decided.
- **No file splits.** New rendering goes in a new `diagnostics_ui.rs`; `details_panel.rs`/`prompt_editor.rs` get call sites only. The split plan is parked, owner-directed, non-pipeline work.
- **Only `Active`/`Bench` creatures are valid `@` targets.** Reserve and off-roster are both flagged, with no exception for anticipated squad changes.
- **`Ember_Wolf.md` is left at 1004 words and stays permanently flagged** — it is the deliberate validation case. At 1004 words / 26 bullets with `@Volt_Scorpion` and Reserve-slot mentions present, it exercises `PromptTooLong` and the mention diagnostics against real content rather than a fixture. Do **not** trim it to silence the badge.
- `resolve_mention` is built as the shared resolver `10` will consume.

## Constants (placeholders — tunable)

- `WORD_LIMIT: usize = 500` — the RECOMMENDED count, advisory only; never enforced (hence a warning, not an error). Message reads "over the recommended 500", never "limit".
- `BADGE_TEXT: &str = "[!!]"`
- `WARNING_COLOR: Rgba = Rgba::rgb(0xff, 0xbf, 0x00)` (the existing amber)
- `DIAGNOSTIC_BG: Color` — distinct from `SELECTION_BG` so a selected bad span stays distinguishable.
- Lint debounce: reuse `WRITE_DEBOUNCE` (~300 ms).

## When lint runs

- **Details panel:** when `current_instructions` reloads — at construction, on slide-settle, and when the popup closes. Never per-frame.
- **Editor:** on `EditorEvent::Changed`, debounced on the same timer as the existing write-through.

Lint is a scan of ~1k words; cost is not a concern, but it must not run per-render.

## Testing Guidance

- 500 words → no diagnostic; 501 → `PromptTooLong`.
- `@Ember_Fang` on Ember Wolf → clean. `@Frost_Bite` on Ember Wolf → `NotAnAbility`.
- Roster roles: an **Active**-slot creature → clean; a **Bench**-slot creature → clean; a **Reserve**-slot creature → `CreatureNotFielded`; `@Volt_Scorpion` (bundled but off the 6-slot roster) → `CreatureNotOnRoster`. Drive this off `squad_role(index)` so the boundary moves if `ACTIVE_SLOTS`/`BENCH_SLOTS` change — never hardcode index 4.
- `@foo:burn` → `UnknownTarget` (target is not one of `self`/`ally`/`enemy`).
- `@self:most-hp` → `BadSelectorForTarget`; `@self:frozen` → clean.
- All six `DiagnosticKind` variants have at least one case. `parse_mention` returns the distinct `GrammarError` for `@foo:burn` vs `@self:most-hp` — a bool cannot carry this, which is why the grammar layer was reworked.
- `is_valid_mention` keeps its current behavior: every existing assertion in `mention/mention_grammar_tests.rs` passes unchanged against the `parse_mention(t).is_ok()` wrapper.
- `foo@bar.com` → no diagnostic. `` `@Howl` `` (backticked) → parsed and resolved.
- A trailing `@enemy:` → no diagnostic (half-typed, not an error).
- No diagnostics → **no badge cell is written** at the header position (assert the cell is blank, not merely that no tooltip shows).
- Badge fg decodes to `WARNING_COLOR`.
- Card: assert one entry per diagnostic and the count header; assert a cell under the card decodes to the card, not the panel beneath (`Occlude`).
- Count header pluralizes at the boundary: exactly 1 diagnostic → `"1 issue"`; 2 → `"2 issues"`.
- Clamping: a badge near the screen's left edge yields a card whose `x >= 0`.
- Decoration pass: a decorated span tints exactly its cells; an active selection over the same span still renders `SELECTION_BG`.
- Lint does not run per-render (assert call count across N renders).
- Tests assert against a **temp base dir**, never the real repo (`51`'s convention).

## Manual verification (not an automated task)

`Ember_Wolf.md` is gitignored, so **no automated test may depend on it** — every test uses a temp base dir. Its validation value is therefore a **manual check by the owner**, recorded here so it isn't mistaken for covered:

> Open the roster on Ember Wolf. The `[!!]` badge is present in the Instructions header. Hovering it shows `PromptTooLong` (1004 words, over the recommended 500) alongside the mention diagnostics its text triggers.

## Open Questions / TBDs

- The `WORD_LIMIT` sweep against the real local model, if the guess ever proves wrong in play.

## Dependencies

- Needs `47-ability-and-instructions-data-model` (instructions IO, `Ability`), `56-at-mention-authoring` (`GameMentionProvider`, grammar, `underscore_name`), `55-combat-status-and-element-enums` (statuses), `23`/`34` (`Creature`), `squad_role` (`ROSTER_SIZE`, `SquadRole`).
- Modifies `48-roster-detail-panel-redesign` (badge in the Instructions header), `49-ability-hover-tooltip` (shell extraction), `51-prompt-editor-popup` (badge + inline marks), `54-text-editor-selection-clipboard-scrollbar-undo` (generalizing `draw_selection`).
- Uses `40-flex-layout-primitive`, `52-engine-text-rendering`, `13`/`22` (`rounded_rect`, `Dot::Occlude`).
- Feeds `10-battle-simulation-engine` (`resolve_mention` is the shared resolver).
