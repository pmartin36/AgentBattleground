//! Game `@`-mention vocabulary: implements `engine_render::MentionProvider`
//! (b1-t1) over this game's data — targets, per-target selectors/statuses,
//! the edited creature's own abilities, and the roster's creature names
//! (spec 56, b2-t1).

use engine_render::{MentionCandidate, MentionProvider};

use crate::ability::StatusKind;
use crate::creatures::Creature;
use crate::diagnostics::DiagnosticKind;
use crate::instructions::underscore_name;
use crate::squad_role::{squad_role, SquadRole, ROSTER_SIZE};

/// The four status kinds, in display order. `ability::StatusKind` has no
/// `all()`/iterator, so this enumerates the variants explicitly (research
/// b2-t1) rather than hardcoding their string labels anywhere.
const STATUSES: [StatusKind; 4] =
    [StatusKind::Burn, StatusKind::Frozen, StatusKind::Shocked, StatusKind::Rooted];

/// Ranking selectors valid for `ally`/`enemy` targets only (not `self`).
const RANKING_SELECTORS: [&str; 3] = ["most-hp", "least-hp", "highest-damage"];

/// Catch-all selector for `ally`/`enemy` (not `self`): matches a target
/// afflicted with ANY status effect, as opposed to a specific one.
const ANY_STATUS_SELECTOR: &str = "any-status";

/// Target keywords, in emit order.
const TARGETS: [&str; 3] = ["self", "ally", "enemy"];

/// Case-insensitive prefix match; an empty `query` matches everything.
fn matches(key: &str, query: &str) -> bool {
    query.is_empty() || key.to_ascii_lowercase().starts_with(&query.to_ascii_lowercase())
}

/// The tokens that exist for one creature being authored: its own ability
/// names plus the roster's creature names paired with their positional
/// role. Shared by `GameMentionProvider` (offers them) and the linter
/// (accepts them) so the two cannot drift. (b1-t1)
pub struct Vocabulary {
    ability_names: Vec<String>,
    /// Bounded to `ROSTER_SIZE`: entries beyond it are off-roster by
    /// definition, so `squad_role` is never consulted past its last
    /// meaningful index.
    roster: Vec<(String, SquadRole)>,
    /// Every BOUNDED-roster creature's ability display names (same
    /// `.take(ROSTER_SIZE)` bound as `roster`). Present solely so a roster
    /// creature's ability (not the edited one's own) is decidable as
    /// `NotAnAbility` rather than collapsing into the off-roster default.
    roster_ability_names: Vec<String>,
}

impl Vocabulary {
    /// Snapshot `creature`'s abilities and `roster`'s names, truncated to
    /// `ROSTER_SIZE` before pairing each entry with `squad_role(index)`.
    pub fn new(creature: &Creature, roster: &[Creature]) -> Self {
        let ability_names =
            creature.abilities().iter().map(|a| a.description().to_string()).collect();
        let roster_ability_names = roster
            .iter()
            .take(ROSTER_SIZE)
            .flat_map(|c| c.abilities().iter().map(|a| a.description().to_string()))
            .collect();
        let roster = roster
            .iter()
            .take(ROSTER_SIZE)
            .enumerate()
            .map(|(i, c)| (c.name().to_string(), squad_role(i)))
            .collect();
        Self { ability_names, roster, roster_ability_names }
    }

    /// `token` (underscored) names one of the edited creature's own
    /// abilities. Returns its index into `abilities()`.
    pub fn ability_index(&self, token: &str) -> Option<usize> {
        self.ability_names.iter().position(|name| underscore_name(name) == token)
    }

    /// The roster index + role of the roster creature `token` names, or
    /// `None` when it is not on the (bounded) roster.
    pub fn creature_entry(&self, token: &str) -> Option<(usize, SquadRole)> {
        self.roster
            .iter()
            .position(|(name, _)| underscore_name(name) == token)
            .map(|i| (i, self.roster[i].1))
    }

    /// `token` (underscored) names an ability belonging to SOME bounded
    /// roster creature (not necessarily the edited one).
    pub fn roster_has_ability(&self, token: &str) -> bool {
        self.roster_ability_names.iter().any(|name| underscore_name(name) == token)
    }
}

/// `@`-mention vocabulary for one creature being authored: its own
/// abilities plus the full roster's creature names, snapshotted as owned
/// `String`s (no borrows) so the provider is `'static` and boxable.
pub struct GameMentionProvider {
    vocab: Vocabulary,
}

impl GameMentionProvider {
    /// Snapshot the current creature's ability names + the roster's
    /// creature names.
    pub fn new(creature: &Creature, roster: &[Creature]) -> Self {
        Self { vocab: Vocabulary::new(creature, roster) }
    }

    /// Stage-1 (bare `@query`, no `:` yet): targets, own abilities, roster
    /// creatures — filtered by `query`.
    fn stage1(&self, query: &str) -> Vec<MentionCandidate> {
        let mut out = Vec::new();

        for target in TARGETS {
            if matches(target, query) {
                out.push(MentionCandidate {
                    display: target.to_string(),
                    insert_text: format!("@{}:", target),
                    category: "target",
                    continues: true,
                });
            }
        }

        for name in &self.vocab.ability_names {
            let token = underscore_name(name);
            if matches(&token, query) {
                out.push(MentionCandidate {
                    display: name.clone(),
                    insert_text: format!("@{}", token),
                    category: "ability",
                    continues: false,
                });
            }
        }

        for (name, _role) in &self.vocab.roster {
            let token = underscore_name(name);
            if matches(&token, query) {
                out.push(MentionCandidate {
                    display: name.clone(),
                    insert_text: format!("@{}", token),
                    category: "creature",
                    continues: false,
                });
            }
        }

        out
    }

    /// Stage-2 (`query` is `<target>:<sel_query>`): selectors valid for
    /// `target`, filtered by `sel_query`. Empty when `target` is not one of
    /// `self`/`ally`/`enemy`.
    fn stage2(&self, target: &str, sel_query: &str) -> Vec<MentionCandidate> {
        if !TARGETS.contains(&target) {
            return Vec::new();
        }

        let mut out = Vec::new();

        if target != "self" {
            for selector in RANKING_SELECTORS {
                if matches(selector, sel_query) {
                    out.push(MentionCandidate {
                        display: selector.to_string(),
                        insert_text: format!("@{}:{}", target, selector),
                        category: "selector",
                        continues: false,
                    });
                }
            }
            if matches(ANY_STATUS_SELECTOR, sel_query) {
                out.push(MentionCandidate {
                    display: ANY_STATUS_SELECTOR.to_string(),
                    insert_text: format!("@{}:{}", target, ANY_STATUS_SELECTOR),
                    category: "selector",
                    continues: false,
                });
            }
        }

        for status in STATUSES {
            let keyword = status.label().to_ascii_lowercase();
            if matches(&keyword, sel_query) {
                out.push(MentionCandidate {
                    display: keyword.clone(),
                    insert_text: format!("@{}:{}", target, keyword),
                    category: "status",
                    continues: false,
                });
            }
        }

        out
    }
}

impl MentionProvider for GameMentionProvider {
    fn candidates(&self, query: &str) -> Vec<MentionCandidate> {
        match query.split_once(':') {
            Some((target, sel_query)) => self.stage2(target, sel_query),
            None => self.stage1(query),
        }
    }
}

/// Why a token failed the context-free grammar parse. Each context-ful
/// variant carries the EXACT offending substring, borrowed from the token:
/// the parser already has it in scope, so no caller re-derives it. (b1-t2 /
/// b1-t3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarError<'a> {
    /// `@foo:burn` — carries the offending TARGET (`"foo"`).
    UnknownTarget { target: &'a str },
    /// `@self:most-hp` — carries the offending SELECTOR (`"most-hp"`). The
    /// target is VALID here; naming it would be a lie (this shipped once).
    BadSelectorForTarget { selector: &'a str },
    /// Not a mention token at all: `@Ember Wolf`, `self`, `@`, `""`.
    Malformed,
}

/// A grammar-valid token, as parsed. Borrows `token`: no allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsedMention<'a> {
    /// `@self` (selector None) / `@enemy:most-hp` (selector Some).
    Target { target: &'a str, selector: Option<&'a str> },
    /// `@Ember_Fang` — an underscored name. Ability vs creature is
    /// context-dependent; b1-t3 decides against the `Vocabulary`.
    Name(&'a str),
}

/// Context-free parse (spec 56) — the token shape `10` will parse. Returns
/// WHY the token failed, not just that it did. Validates a COMPLETE, final
/// mention token; intermediate two-stage forms (e.g. `@enemy:`, emitted by a
/// `continues` target candidate) are NOT final and are rejected as
/// `BadSelectorForTarget`/`Malformed` (never silently accepted).
pub fn parse_mention(token: &str) -> Result<ParsedMention<'_>, GrammarError<'_>> {
    let Some(body) = token.strip_prefix('@') else { return Err(GrammarError::Malformed) };
    match body.split_once(':') {
        Some((target, selector)) => {
            let ok = match target {
                "self" => is_status(selector),
                "ally" | "enemy" => is_selector(selector),
                _ => return Err(GrammarError::UnknownTarget { target }),
            };
            if ok {
                Ok(ParsedMention::Target { target, selector: Some(selector) })
            } else {
                Err(GrammarError::BadSelectorForTarget { selector })
            }
        }
        None if TARGETS.contains(&body) => Ok(ParsedMention::Target { target: body, selector: None }),
        None if is_name(body) => Ok(ParsedMention::Name(body)),
        None => Err(GrammarError::Malformed),
    }
}

/// Grammar shape check as a bool — the thin wrapper form. Kept so its
/// existing callers/tests (`mention_grammar_tests.rs`) pass unchanged.
pub fn is_valid_mention(token: &str) -> bool {
    parse_mention(token).is_ok()
}

/// `s` is a valid status keyword (lowercase, matching `StatusKind::label()`).
fn is_status(s: &str) -> bool {
    STATUSES.iter().any(|k| k.label().to_ascii_lowercase() == s)
}

/// `s` is a valid `ally`/`enemy` selector: a ranking selector, the
/// `any-status` catch-all, or a specific status.
fn is_selector(s: &str) -> bool {
    s == ANY_STATUS_SELECTOR || RANKING_SELECTORS.contains(&s) || is_status(s)
}

/// `s` is a valid underscored ability/creature name: non-empty, no
/// whitespace, no `:`.
fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| !c.is_whitespace() && c != ':')
}

/// A grammar-valid token, resolved against a `Vocabulary`. Borrows `token`:
/// the `Ok` path allocates nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// `@self` / `@enemy:most-hp`. Grammar-verified; no vocab lookup.
    Target { target: &'a str, selector: Option<&'a str> },
    /// `@Ember_Fang` — index into the EDITED creature's `abilities()`.
    Ability { index: usize },
    /// `@Ember_Wolf` — index into the roster (`0..ROSTER_SIZE`). Active or
    /// Bench only (spec:101; callers derive role via `squad_role(index)`,
    /// never a stored field).
    Creature { index: usize },
}

/// Why `resolve_mention` could not produce a `Resolved`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Not a mention token at all (`GrammarError::Malformed`). b1-t4's lexer
    /// cannot produce one; only direct callers (spec 10, tests) can.
    Malformed,
    /// A well-shaped token naming something the vocabulary rejects.
    Diagnostic(DiagnosticKind),
}

/// Context-aware resolution — the single entry point spec 10 reuses
/// (spec:88). Parses via `parse_mention`, then for a `Name`, applies an
/// ordered 4-rule ladder against `vocab` (first match wins):
///   1. `vocab.ability_index`     -> `Ok(Ability)`
///   2. `vocab.creature_entry`    -> Active/Bench: `Ok(Creature)`; Reserve:
///      `Err(Diagnostic(CreatureNotFielded))`
///   3. `vocab.roster_has_ability` -> `Err(Diagnostic(NotAnAbility))`
///   4. otherwise                 -> `Err(Diagnostic(CreatureNotOnRoster))`
pub fn resolve_mention<'a>(
    token: &'a str,
    vocab: &Vocabulary,
) -> Result<Resolved<'a>, ResolveError> {
    match parse_mention(token) {
        // These two errors only arise from the `@<target>:<selector>` shape.
        // `UnknownTarget` names the offending TARGET; `BadSelectorForTarget`
        // (below) names the offending SELECTOR — never the whole token.
        // Both payloads are read straight off `GrammarError`, which already
        // carries the exact offending substring: no re-parsing here.
        Err(GrammarError::UnknownTarget { target }) => Err(ResolveError::Diagnostic(
            DiagnosticKind::UnknownTarget { token: target.to_string() },
        )),
        Err(GrammarError::BadSelectorForTarget { selector }) => Err(ResolveError::Diagnostic(
            DiagnosticKind::BadSelectorForTarget { token: selector.to_string() },
        )),
        Err(GrammarError::Malformed) => Err(ResolveError::Malformed),
        Ok(ParsedMention::Target { target, selector }) => Ok(Resolved::Target { target, selector }),
        Ok(ParsedMention::Name(n)) => {
            if let Some(index) = vocab.ability_index(n) {
                Ok(Resolved::Ability { index })
            } else if let Some((index, role)) = vocab.creature_entry(n) {
                match role {
                    SquadRole::Active | SquadRole::Bench => Ok(Resolved::Creature { index }),
                    SquadRole::Reserve => Err(ResolveError::Diagnostic(
                        DiagnosticKind::CreatureNotFielded { token: n.to_string() },
                    )),
                }
            } else if vocab.roster_has_ability(n) {
                Err(ResolveError::Diagnostic(DiagnosticKind::NotAnAbility { token: n.to_string() }))
            } else {
                Err(ResolveError::Diagnostic(DiagnosticKind::CreatureNotOnRoster {
                    token: n.to_string(),
                }))
            }
        }
    }
}

#[cfg(test)]
mod mention_grammar_tests;
#[cfg(test)]
mod resolve_mention_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::Ability;

    fn provider() -> GameMentionProvider {
        let creature =
            Creature::new("Test").with_abilities(vec![Ability::new("Ember Fang", vec![])]);
        let roster = vec![Creature::new("Ember Wolf")];
        GameMentionProvider::new(&creature, &roster)
    }

    fn insert_texts(cands: &[MentionCandidate]) -> Vec<&str> {
        cands.iter().map(|c| c.insert_text.as_str()).collect()
    }

    #[test]
    fn bare_query_lists_targets_abilities_creatures() {
        let p = provider();
        let cands = p.candidates("");
        let texts = insert_texts(&cands);
        assert!(texts.contains(&"@self:"));
        assert!(texts.contains(&"@ally:"));
        assert!(texts.contains(&"@enemy:"));
        assert!(texts.contains(&"@Ember_Fang"));
        assert!(texts.contains(&"@Ember_Wolf"));
    }

    #[test]
    fn self_selectors_are_status_only() {
        let p = provider();
        let cands = p.candidates("self:");
        let texts = insert_texts(&cands);
        assert_eq!(texts.len(), 4);
        assert!(texts.contains(&"@self:burn"));
        assert!(texts.contains(&"@self:frozen"));
        assert!(texts.contains(&"@self:shocked"));
        assert!(texts.contains(&"@self:rooted"));
        assert!(!texts.iter().any(|t| t.contains("most-hp")));
        assert!(!texts.iter().any(|t| t.contains("least-hp")));
        assert!(!texts.iter().any(|t| t.contains("highest-damage")));
        assert!(!texts.iter().any(|t| t.contains("any-status")));
    }

    #[test]
    fn enemy_selectors_include_ranking_and_statuses() {
        let p = provider();
        let cands = p.candidates("enemy:");
        let texts = insert_texts(&cands);
        assert_eq!(texts.len(), 8);
        assert!(texts.contains(&"@enemy:most-hp"));
        assert!(texts.contains(&"@enemy:least-hp"));
        assert!(texts.contains(&"@enemy:highest-damage"));
        assert!(texts.contains(&"@enemy:any-status"));
        assert!(texts.contains(&"@enemy:burn"));
        assert!(texts.contains(&"@enemy:frozen"));
        assert!(texts.contains(&"@enemy:shocked"));
        assert!(texts.contains(&"@enemy:rooted"));
    }

    #[test]
    fn any_status_selector_offered_for_ally_enemy_not_self() {
        let p = provider();
        assert!(insert_texts(&p.candidates("enemy:")).contains(&"@enemy:any-status"));
        assert!(insert_texts(&p.candidates("ally:")).contains(&"@ally:any-status"));
        assert!(
            !insert_texts(&p.candidates("self:")).iter().any(|t| t.contains("any-status")),
            "self takes no any-status (or any ranking) selector"
        );
        // typing the prefix surfaces it
        assert!(insert_texts(&p.candidates("enemy:any")).contains(&"@enemy:any-status"));
        // grammar validity
        assert!(is_valid_mention("@enemy:any-status"));
        assert!(is_valid_mention("@ally:any-status"));
        assert!(!is_valid_mention("@self:any-status"));
    }

    #[test]
    fn ally_behaves_like_enemy() {
        let p = provider();
        let ally_texts = insert_texts(&p.candidates("ally:"))
            .into_iter()
            .map(|t| t.replacen("ally", "X", 1))
            .collect::<Vec<_>>();
        let enemy_texts = insert_texts(&p.candidates("enemy:"))
            .into_iter()
            .map(|t| t.replacen("enemy", "X", 1))
            .collect::<Vec<_>>();
        assert_eq!(ally_texts, enemy_texts);
    }

    #[test]
    fn targets_are_continues_with_trailing_colon() {
        let p = provider();
        let cands = p.candidates("");
        for c in cands.iter().filter(|c| c.category == "target") {
            assert!(c.continues, "{:?} should continue", c);
            assert!(c.insert_text.ends_with(':'), "{:?} should end with ':'", c);
        }
    }

    #[test]
    fn selectors_and_names_are_terminal() {
        let p = provider();
        for c in p.candidates("") {
            if c.category != "target" {
                assert!(!c.continues, "{:?} should be terminal", c);
            }
        }
        for c in p.candidates("enemy:") {
            assert!(!c.continues, "{:?} should be terminal", c);
        }
    }

    #[test]
    fn names_are_underscored_in_insert_text_display_keeps_spaces() {
        let p = provider();
        let cands = p.candidates("");
        let ability = cands.iter().find(|c| c.category == "ability").expect("ability candidate");
        assert_eq!(ability.display, "Ember Fang");
        assert_eq!(ability.insert_text, "@Ember_Fang");

        let creature =
            cands.iter().find(|c| c.category == "creature").expect("creature candidate");
        assert_eq!(creature.display, "Ember Wolf");
        assert_eq!(creature.insert_text, "@Ember_Wolf");
    }

    #[test]
    fn query_prefix_filters_case_insensitive_stage1() {
        let p = provider();
        let cands = p.candidates("al");
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].display, "ally");
    }

    #[test]
    fn query_prefix_filters_case_insensitive_stage2() {
        let p = provider();
        let cands = p.candidates("self:fr");
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].insert_text, "@self:frozen");
    }

    #[test]
    fn invalid_target_before_colon_returns_empty() {
        let p = provider();
        assert!(p.candidates("Ember_Fang:").is_empty());
    }

    #[test]
    fn statuses_derive_from_status_kind_label() {
        let p = provider();
        let cands = p.candidates("self:");
        let mut labels: Vec<String> =
            cands.iter().map(|c| c.display.clone()).collect();
        labels.sort();
        let mut expected: Vec<String> =
            STATUSES.iter().map(|s| s.label().to_ascii_lowercase()).collect();
        expected.sort();
        assert_eq!(labels, expected);
    }

    // --- b1-t1: Vocabulary --------------------------------------------

    /// A roster longer than `ROSTER_SIZE` is truncated: entries beyond the
    /// bound are dropped entirely, not handed to `squad_role` (which would
    /// silently answer `Reserve` for any index, unbounded).
    #[test]
    fn vocabulary_roster_is_bounded_to_roster_size() {
        let creature = Creature::new("Test");
        let roster: Vec<Creature> =
            (0..ROSTER_SIZE + 2).map(|i| Creature::new(format!("C{i}"))).collect();
        let vocab = Vocabulary::new(&creature, &roster);
        assert_eq!(vocab.roster.len(), ROSTER_SIZE);
        for i in ROSTER_SIZE..ROSTER_SIZE + 2 {
            let token = underscore_name(&format!("C{i}"));
            assert_eq!(
                vocab.creature_entry(&token),
                None,
                "index {i} is beyond ROSTER_SIZE and must not resolve to an entry"
            );
        }
    }

    /// `roster_ability_names` is bounded exactly like `roster`: an
    /// off-roster creature's ability must not leak in, so it does not get
    /// mistaken for `NotAnAbility` — it must fall through to
    /// `CreatureNotOnRoster` (the same `ROSTER_SIZE` trap b1-t1 closed for
    /// `roster` itself).
    #[test]
    fn roster_ability_names_is_bounded_to_roster_size() {
        let creature = Creature::new("Test");
        let mut roster: Vec<Creature> =
            (0..ROSTER_SIZE).map(|i| Creature::new(format!("C{i}"))).collect();
        roster.push(
            Creature::new("Offroster").with_abilities(vec![Ability::new("Rare Move", vec![])]),
        );
        let vocab = Vocabulary::new(&creature, &roster);
        assert!(
            !vocab.roster_has_ability("Rare_Move"),
            "an off-roster creature's ability must not be reachable"
        );
        assert_eq!(
            resolve_mention("@Rare_Move", &vocab),
            Err(ResolveError::Diagnostic(DiagnosticKind::CreatureNotOnRoster {
                token: "Rare_Move".to_string()
            }))
        );
    }

    /// Each retained roster entry's role matches `squad_role` at its
    /// position — derived from the constants, not a hardcoded boundary.
    #[test]
    fn vocabulary_roles_derive_from_position() {
        let creature = Creature::new("Test");
        let roster: Vec<Creature> =
            (0..ROSTER_SIZE).map(|i| Creature::new(format!("C{i}"))).collect();
        let vocab = Vocabulary::new(&creature, &roster);
        for i in 0..ROSTER_SIZE {
            assert_eq!(vocab.roster[i].1, squad_role(i), "index {i} role mismatch");
        }
    }

    #[test]
    fn creature_entry_matches_underscored_token() {
        let creature = Creature::new("Test");
        let roster = vec![Creature::new("Ember Wolf")];
        let vocab = Vocabulary::new(&creature, &roster);
        assert_eq!(vocab.creature_entry("Ember_Wolf"), Some((0, SquadRole::Active)));
        assert_eq!(vocab.creature_entry("Ember Wolf"), None, "spaced token must not match");
        assert_eq!(vocab.creature_entry("Nonexistent"), None);
    }

    #[test]
    fn ability_index_matches_own_abilities_only() {
        let creature =
            Creature::new("Test").with_abilities(vec![Ability::new("Ember Fang", vec![])]);
        let roster = vec![Creature::new("Ember Wolf")];
        let vocab = Vocabulary::new(&creature, &roster);
        assert_eq!(vocab.ability_index("Ember_Fang"), Some(0));
        assert_eq!(vocab.ability_index("Frost_Bite"), None);
    }

    // --- b1-t2: parse_mention ------------------------------------------

    #[test]
    fn parse_mention_reports_unknown_target() {
        assert_eq!(
            parse_mention("@foo:burn"),
            Err(GrammarError::UnknownTarget { target: "foo" })
        );
        assert_eq!(parse_mention("@:frozen"), Err(GrammarError::UnknownTarget { target: "" }));
    }

    /// The payload names the offending SELECTOR (`"most-hp"`), never the
    /// (valid) target `"self"` — this is the regression pin for the bug
    /// that already shipped once (player-facing copy calling the valid
    /// target an invalid selector).
    #[test]
    fn parse_mention_reports_bad_selector_for_target() {
        let result = parse_mention("@self:most-hp");
        assert_eq!(result, Err(GrammarError::BadSelectorForTarget { selector: "most-hp" }));
        assert_ne!(result, Err(GrammarError::BadSelectorForTarget { selector: "self" }));
    }

    #[test]
    fn parse_mention_reports_malformed() {
        for token in ["", "@", "self", "self:frozen", "@Ember Wolf"] {
            assert_eq!(
                parse_mention(token),
                Err(GrammarError::Malformed),
                "{:?} should be Malformed",
                token
            );
        }
    }

    #[test]
    fn parse_mention_accepts_and_classifies() {
        assert_eq!(
            parse_mention("@self"),
            Ok(ParsedMention::Target { target: "self", selector: None })
        );
        assert_eq!(
            parse_mention("@enemy:least-hp"),
            Ok(ParsedMention::Target { target: "enemy", selector: Some("least-hp") })
        );
        assert_eq!(parse_mention("@Ember_Fang"), Ok(ParsedMention::Name("Ember_Fang")));
    }

    #[test]
    fn is_valid_mention_is_parse_mention_is_ok() {
        // Full corpus of both existing grammar test files
        // (`mention_grammar_tests.rs`'s `well_formed_examples_accepted` +
        // `malformed_shapes_rejected`, plus this file's ad-hoc
        // `is_valid_mention` calls) — the wrapper must agree with the parser
        // on every one of them.
        let corpus = [
            "@self",
            "@ally",
            "@enemy",
            "@self:frozen",
            "@enemy:least-hp",
            "@enemy:most-hp",
            "@enemy:highest-damage",
            "@ally:shocked",
            "@Douse",
            "@Ember_Wolf",
            "",
            "@",
            "self",
            "self:frozen",
            "@self:",
            "@enemy:",
            "@:frozen",
            "@self:most-hp",
            "@enemy:bogus",
            "@ally:most-hp:extra",
            "@Ember Wolf",
            "@self:Frozen",
            "@enemy:any-status",
            "@ally:any-status",
            "@self:any-status",
        ];
        for token in corpus {
            assert_eq!(
                is_valid_mention(token),
                parse_mention(token).is_ok(),
                "{:?}: is_valid_mention/parse_mention disagree",
                token
            );
        }
    }

    #[test]
    fn dangling_colon_is_bad_selector_not_ok() {
        assert_eq!(parse_mention("@enemy:"), Err(GrammarError::BadSelectorForTarget { selector: "" }));
        assert_eq!(parse_mention("@self:"), Err(GrammarError::BadSelectorForTarget { selector: "" }));
    }
}
