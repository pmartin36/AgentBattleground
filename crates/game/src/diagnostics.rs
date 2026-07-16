//! Lint vocabulary of problems: the six `DiagnosticKind`s and the
//! `Diagnostic` a mention/prompt problem is reported as. `mention.rs` owns
//! *resolution* (deciding which kind, if any, applies); this module owns the
//! *kinds themselves* and their single source of player-facing copy
//! (`DiagnosticKind::message`). b1-t4 is the only emitter; b3-t3 renders
//! `Diagnostic::message` and must not re-derive copy per kind.

use std::ops::Range;

use crate::mention::{resolve_mention, ResolveError, Vocabulary};

/// The prompt word budget. A deliberate PROXY for directive count (spec:29)
/// — tunable; `PromptTooLong.limit` is always fed from here.
pub const WORD_LIMIT: usize = 500;

/// Exactly six — spec:47-54. Do not extend without a spec change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// The whole-prompt word-count budget was exceeded. Whole-file: no span.
    PromptTooLong { words: usize, limit: usize },
    /// `<x>:<y>` shape whose `<x>` is not `self`/`ally`/`enemy`.
    UnknownTarget { token: String },
    /// Target is known, selector is invalid for it.
    BadSelectorForTarget { token: String },
    /// Names an ability that exists on the roster, but not on the creature
    /// being edited.
    NotAnAbility { token: String },
    /// Names something that is not among the bounded roster at all (or a
    /// typo) — spec:96's default case.
    CreatureNotOnRoster { token: String },
    /// Names a roster creature that is in `Reserve` — not `Active`/`Bench`.
    CreatureNotFielded { token: String },
}

impl DiagnosticKind {
    /// The single source of per-kind player-facing copy.
    pub fn message(&self) -> String {
        match self {
            DiagnosticKind::PromptTooLong { words, limit } => {
                format!("Instructions are {words} words, over the {limit}-word limit.")
            }
            DiagnosticKind::UnknownTarget { token } => {
                if token.is_empty() {
                    "A target is missing before \":\" (expected self/ally/enemy).".to_string()
                } else {
                    format!("\"{token}\" is not a known target (self/ally/enemy).")
                }
            }
            DiagnosticKind::BadSelectorForTarget { token } => {
                if token.is_empty() {
                    "A selector is missing after \":\".".to_string()
                } else {
                    format!("\"{token}\" is not a valid selector for this target.")
                }
            }
            DiagnosticKind::NotAnAbility { token } => {
                format!("\"{token}\" is not one of this creature's abilities.")
            }
            DiagnosticKind::CreatureNotOnRoster { token } => {
                format!("\"{token}\" is not on your roster.")
            }
            DiagnosticKind::CreatureNotFielded { token } => {
                format!("\"{token}\" is in reserve, not active or bench.")
            }
        }
    }
}

/// spec:56-62 shape. Fields stay `pub`; `message` is DERIVED from `kind` —
/// always build via `Diagnostic::new` so the two can never drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    /// Byte range of the offending token; `None` for whole-file kinds
    /// (`PromptTooLong`).
    pub span: Option<Range<usize>>,
    pub message: String,
}

impl Diagnostic {
    /// The ONLY constructor callers use. Fills `message` from
    /// `kind.message()` so the two can never drift.
    pub fn new(kind: DiagnosticKind, span: Option<Range<usize>>) -> Self {
        let message = kind.message();
        Self { kind, span, message }
    }
}

/// A word character for the mention-start boundary test (spec:107).
/// Unicode-aware ON PURPOSE (see research.md CLEANLINESS) — the token
/// grammar itself is ASCII-only, but "is the preceding char part of a
/// word" must reject `café@x.com` too, not just ASCII emails.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// End byte offset of the token body starting at `start` (just past `@`),
/// or `None` when no valid body starts there. Grammar (spec:108, exact):
/// `[A-Za-z_][A-Za-z0-9_-]*` then optionally `:` + `[a-z-]+`.
fn token_end(text: &str, start: usize) -> Option<usize> {
    let rest = text.get(start..)?;
    let first = *rest.as_bytes().first()?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let name_len =
        rest.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-').count();
    let mut end = start + name_len;
    if let Some(after) = text[end..].strip_prefix(':') {
        let sel_len = after.bytes().take_while(|b| b.is_ascii_lowercase() || *b == b'-').count();
        if sel_len > 0 {
            end += 1 + sel_len; // ':' + selector; non-empty only
        }
    }
    Some(end)
}

/// Byte ranges of every mention token in `text`, in ascending source order.
/// Each range slices back to the EXACT token (`&text[span]`).
fn mention_spans(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut idx = 0;
    while let Some(off) = text[idx..].find('@') {
        let at = idx + off;
        let preceded_by_word = text[..at].chars().next_back().is_some_and(is_word_char);
        idx = at + 1; // default: resume just past this '@'
        if preceded_by_word {
            continue; // foo@bar.com
        }
        if let Some(end) = token_end(text, at + 1) {
            out.push(at..end);
            idx = end;
        }
    }
    out
}

/// Every problem in `text`, against `vocab`. Order is deterministic:
/// `PromptTooLong` (if any) first, then one diagnostic per offending
/// mention OCCURRENCE in ascending source order. spec:64.
pub fn lint(text: &str, vocab: &Vocabulary) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let words = text.split_whitespace().count();
    if words > WORD_LIMIT {
        out.push(Diagnostic::new(DiagnosticKind::PromptTooLong { words, limit: WORD_LIMIT }, None));
    }
    for span in mention_spans(text) {
        if let Err(ResolveError::Diagnostic(kind)) = resolve_mention(&text[span.clone()], vocab) {
            out.push(Diagnostic::new(kind, Some(span)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint_test_fixture::vocab;

    fn all_kinds() -> Vec<DiagnosticKind> {
        vec![
            DiagnosticKind::PromptTooLong { words: 500, limit: 400 },
            DiagnosticKind::UnknownTarget { token: "foo".to_string() },
            DiagnosticKind::BadSelectorForTarget { token: "most-hp".to_string() },
            DiagnosticKind::NotAnAbility { token: "Frost_Bite".to_string() },
            DiagnosticKind::CreatureNotOnRoster { token: "Volt_Scorpion".to_string() },
            DiagnosticKind::CreatureNotFielded { token: "Verdant_Treant".to_string() },
        ]
    }

    /// All six kinds' messages are pairwise distinct — no two problems read
    /// the same to the player.
    #[test]
    fn messages_are_pairwise_distinct() {
        let kinds = all_kinds();
        let messages: Vec<String> = kinds.iter().map(|k| k.message()).collect();
        for i in 0..messages.len() {
            for j in (i + 1)..messages.len() {
                assert_ne!(
                    messages[i], messages[j],
                    "kinds {:?} and {:?} produced identical messages",
                    kinds[i], kinds[j]
                );
            }
        }
    }

    /// spec:99's own words: the off-roster vs. in-reserve distinction must
    /// be legible in the copy itself, not just the variant name.
    #[test]
    fn creature_not_on_roster_message_says_not_on_roster() {
        let kind = DiagnosticKind::CreatureNotOnRoster { token: "Volt_Scorpion".to_string() };
        assert!(
            kind.message().contains("not on your roster"),
            "message was: {:?}",
            kind.message()
        );
    }

    #[test]
    fn creature_not_fielded_message_says_in_reserve() {
        let kind = DiagnosticKind::CreatureNotFielded { token: "Verdant_Treant".to_string() };
        assert!(kind.message().contains("in reserve"), "message was: {:?}", kind.message());
    }

    /// Every mention-kind's message contains its offending token, so the
    /// player can tell which token the problem is about.
    #[test]
    fn mention_kind_messages_contain_their_token() {
        for (token, kind) in [
            ("foo", DiagnosticKind::UnknownTarget { token: "foo".to_string() }),
            ("most-hp", DiagnosticKind::BadSelectorForTarget { token: "most-hp".to_string() }),
            ("Frost_Bite", DiagnosticKind::NotAnAbility { token: "Frost_Bite".to_string() }),
            (
                "Volt_Scorpion",
                DiagnosticKind::CreatureNotOnRoster { token: "Volt_Scorpion".to_string() },
            ),
            (
                "Verdant_Treant",
                DiagnosticKind::CreatureNotFielded { token: "Verdant_Treant".to_string() },
            ),
        ] {
            assert!(
                kind.message().contains(token),
                "message for {:?} should contain {:?}",
                kind,
                token
            );
        }
    }

    /// An empty target (`@:frozen`, reachable only by a direct caller —
    /// unreachable from b1-t4's lexer) must not emit copy that quotes an
    /// empty string; it must read as a missing target.
    #[test]
    fn unknown_target_empty_token_does_not_quote_empty_string() {
        let kind = DiagnosticKind::UnknownTarget { token: String::new() };
        let msg = kind.message();
        assert!(!msg.contains("\"\""), "message quotes an empty token: {:?}", msg);
        assert!(msg.to_lowercase().contains("target"), "message was: {:?}", msg);
    }

    /// An empty selector (`@self:`, `@enemy:`, reachable only by a direct
    /// caller) must not emit copy that quotes an empty string; it must read
    /// as a missing selector.
    #[test]
    fn bad_selector_for_target_empty_token_does_not_quote_empty_string() {
        let kind = DiagnosticKind::BadSelectorForTarget { token: String::new() };
        let msg = kind.message();
        assert!(!msg.contains("\"\""), "message quotes an empty token: {:?}", msg);
        assert!(msg.to_lowercase().contains("selector"), "message was: {:?}", msg);
    }

    /// `Diagnostic::new` derives `message` from `kind` — the two can never
    /// drift because there is only one constructor.
    #[test]
    fn diagnostic_new_derives_message_from_kind() {
        let kind = DiagnosticKind::CreatureNotOnRoster { token: "Volt_Scorpion".to_string() };
        let expected = kind.message();
        let diag = Diagnostic::new(kind, None);
        assert_eq!(diag.message, expected);
        assert_eq!(diag.span, None);
    }

    // --- b1-t4: lexer + lint --------------------------------------------
    //
    // Fixture is the ONE shared `crate::lint_test_fixture` (b1-t3 iteration
    // 3 / scope FINDING 3) — do NOT redefine `roster`/`vocab` locally here.
    // Do NOT touch `Ember_Wolf.md` (gitignored) or `demo_roster()`.

    // -- word limit (`lint`) --

    #[test]
    fn lint_at_word_limit_has_no_diagnostic() {
        let text = vec!["word"; WORD_LIMIT].join(" ");
        assert_eq!(lint(&text, &vocab()), vec![]);
    }

    #[test]
    fn lint_over_word_limit_emits_prompt_too_long() {
        let text = vec!["word"; WORD_LIMIT + 1].join(" ");
        let diags = lint(&text, &vocab());
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].kind,
            DiagnosticKind::PromptTooLong { words: WORD_LIMIT + 1, limit: WORD_LIMIT }
        );
        assert_eq!(diags[0].span, None);
    }

    // -- lexer (`mention_spans` directly) --

    /// Meaningful: were the `@` scanned, `@bar` would resolve to
    /// `CreatureNotOnRoster` — this genuinely pins the non-word-char rule.
    #[test]
    fn mention_spans_email_is_not_scanned() {
        assert_eq!(mention_spans("foo@bar.com"), Vec::<Range<usize>>::new());
    }

    #[test]
    fn mention_spans_scans_start_of_file_and_delimited_mentions() {
        assert_eq!(mention_spans("@Howl").len(), 1);
        assert_eq!(mention_spans("(@Howl)").len(), 1);
        assert_eq!(mention_spans("`@Howl` here").len(), 1);
    }

    /// The dangling colon is left OUT of the token — this is what makes the
    /// half-typed-mention exemption work with no special case in `lint`.
    #[test]
    fn mention_spans_excludes_dangling_colon() {
        let text = "Use @enemy: now";
        let spans = mention_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].clone()], "@enemy");
    }

    #[test]
    fn mention_spans_rejects_invalid_token_starts() {
        for text in ["@123abc", "@-foo", "email @ home", "just @"] {
            assert_eq!(mention_spans(text), Vec::<Range<usize>>::new(), "{:?} should yield no spans", text);
        }
    }

    #[test]
    fn mention_spans_slice_back_to_exact_token() {
        let text = "see @Volt_Scorpion now";
        let spans = mention_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].clone()], "@Volt_Scorpion");
    }

    /// A char-indexed implementation slices garbage or panics here; a
    /// byte-indexed one does not shift.
    #[test]
    fn mention_spans_not_shifted_by_preceding_multibyte_utf8() {
        let text = "café — @Volt_Scorpion";
        let spans = mention_spans(text);
        assert_eq!(spans.len(), 1);
        assert_eq!(&text[spans[0].clone()], "@Volt_Scorpion");
    }

    /// Pins the Unicode-aware `is_word_char`; an ASCII-only form regresses
    /// this (would falsely lex `@x` out of `café@x.com`).
    #[test]
    fn mention_spans_unicode_word_char_before_at_suppresses_token() {
        assert_eq!(mention_spans("café@x.com"), Vec::<Range<usize>>::new());
    }

    // -- lexer x resolver (`lint`) --

    /// Trap: `@Howl` is a VALID ability, so asserting through `lint` on it
    /// proves nothing (observationally identical to "code span skipped").
    /// This uses a BAD mention inside backticks so scanning is actually
    /// exercised end-to-end.
    #[test]
    fn lint_scans_inside_code_spans_not_skip() {
        let diags = lint("`@Volt_Scorpion`", &vocab());
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].kind,
            DiagnosticKind::CreatureNotOnRoster { token: "Volt_Scorpion".to_string() }
        );
    }

    #[test]
    fn lint_trailing_half_typed_mention_yields_no_diagnostic() {
        assert_eq!(lint("Use @enemy: now", &vocab()), vec![]);
    }

    #[test]
    fn lint_clean_text_has_no_diagnostics() {
        let text = "Use @Ember_Fang then @Howl. If @self:frozen retreat, else target @enemy:least-hp.";
        assert_eq!(lint(text, &vocab()), vec![]);
    }

    #[test]
    fn lint_one_bad_mention_yields_one_diagnostic_with_matching_span() {
        let text = "Cast @Frost_Bite now";
        let diags = lint(text, &vocab());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].kind, DiagnosticKind::NotAnAbility { token: "Frost_Bite".to_string() });
        let span = diags[0].span.clone().unwrap();
        assert_eq!(&text[span], "@Frost_Bite");
    }

    /// The same bad mention 3x -> 3 diagnostics with 3 distinct, ascending
    /// spans. Pins the contract b4-t2's per-occurrence tinting depends on.
    #[test]
    fn lint_does_not_dedupe_repeated_bad_mentions() {
        let text = "@Volt_Scorpion @Volt_Scorpion @Volt_Scorpion";
        let diags = lint(text, &vocab());
        assert_eq!(diags.len(), 3);
        let starts: Vec<usize> = diags.iter().map(|d| d.span.clone().unwrap().start).collect();
        assert!(starts.windows(2).all(|w| w[0] < w[1]), "spans must be ascending: {:?}", starts);
    }

    #[test]
    fn lint_orders_prompt_too_long_first_then_ascending_mentions() {
        let mut text = vec!["word"; WORD_LIMIT + 1].join(" ");
        text.push_str(" @Volt_Scorpion");
        let diags = lint(&text, &vocab());
        assert!(matches!(diags[0].kind, DiagnosticKind::PromptTooLong { .. }));
        assert_eq!(diags[0].span, None);
        let starts: Vec<usize> = diags[1..].iter().map(|d| d.span.clone().unwrap().start).collect();
        assert!(starts.windows(2).all(|w| w[0] < w[1]), "spans must be ascending: {:?}", starts);
    }

    /// Completes the sixth-variant coverage the deliverable names, together
    /// with `PromptTooLong` + `CreatureNotOnRoster` above.
    #[test]
    fn lint_mixed_kinds_in_source_order() {
        let text = "@foo:burn then @self:most-hp then @Frost_Bite then @Verdant_Treant";
        let diags = lint(text, &vocab());
        let kinds: Vec<&DiagnosticKind> = diags.iter().map(|d| &d.kind).collect();
        assert_eq!(
            kinds,
            vec![
                &DiagnosticKind::UnknownTarget { token: "foo".to_string() },
                &DiagnosticKind::BadSelectorForTarget { token: "most-hp".to_string() },
                &DiagnosticKind::NotAnAbility { token: "Frost_Bite".to_string() },
                &DiagnosticKind::CreatureNotFielded { token: "Verdant_Treant".to_string() },
            ]
        );
    }
}
