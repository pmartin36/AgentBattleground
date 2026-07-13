//! Game `@`-mention vocabulary: implements `engine_render::MentionProvider`
//! (b1-t1) over this game's data — targets, per-target selectors/statuses,
//! the edited creature's own abilities, and the roster's creature names
//! (spec 56, b2-t1).

use engine_render::{MentionCandidate, MentionProvider};

use crate::ability::StatusKind;
use crate::creatures::Creature;
use crate::instructions::underscore_name;

/// The four status kinds, in display order. `ability::StatusKind` has no
/// `all()`/iterator, so this enumerates the variants explicitly (research
/// b2-t1) rather than hardcoding their string labels anywhere.
const STATUSES: [StatusKind; 4] =
    [StatusKind::Burn, StatusKind::Frozen, StatusKind::Shocked, StatusKind::Rooted];

/// Ranking selectors valid for `ally`/`enemy` targets only (not `self`).
const RANKING_SELECTORS: [&str; 3] = ["most-hp", "least-hp", "highest-damage"];

/// Target keywords, in emit order.
const TARGETS: [&str; 3] = ["self", "ally", "enemy"];

/// Case-insensitive prefix match; an empty `query` matches everything.
fn matches(key: &str, query: &str) -> bool {
    query.is_empty() || key.to_ascii_lowercase().starts_with(&query.to_ascii_lowercase())
}

/// `@`-mention vocabulary for one creature being authored: its own
/// abilities plus the full roster's creature names, snapshotted as owned
/// `String`s (no borrows) so the provider is `'static` and boxable.
pub struct GameMentionProvider {
    ability_names: Vec<String>,
    creature_names: Vec<String>,
}

impl GameMentionProvider {
    /// Snapshot the current creature's ability names + the roster's
    /// creature names.
    pub fn new(creature: &Creature, roster: &[Creature]) -> Self {
        let ability_names =
            creature.abilities().iter().map(|a| a.description().to_string()).collect();
        let creature_names = roster.iter().map(|c| c.name().to_string()).collect();
        Self { ability_names, creature_names }
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

        for name in &self.ability_names {
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

        for name in &self.creature_names {
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

/// Grammar (spec 56) shape check — the token shape `10` will parse. Validates
/// a COMPLETE, final mention token; intermediate two-stage forms (e.g.
/// `@enemy:`, emitted by a `continues` target candidate) are NOT final and
/// are rejected.
pub fn is_valid_mention(token: &str) -> bool {
    let Some(body) = token.strip_prefix('@') else { return false };
    match body.split_once(':') {
        Some((target, selector)) => match target {
            "self" => is_status(selector),
            "ally" | "enemy" => is_selector(selector),
            _ => false,
        },
        None => TARGETS.contains(&body) || is_name(body),
    }
}

/// `s` is a valid status keyword (lowercase, matching `StatusKind::label()`).
fn is_status(s: &str) -> bool {
    STATUSES.iter().any(|k| k.label().to_ascii_lowercase() == s)
}

/// `s` is a valid `ally`/`enemy` selector: a ranking selector or a status.
fn is_selector(s: &str) -> bool {
    RANKING_SELECTORS.contains(&s) || is_status(s)
}

/// `s` is a valid underscored ability/creature name: non-empty, no
/// whitespace, no `:`.
fn is_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| !c.is_whitespace() && c != ':')
}

#[cfg(test)]
mod mention_grammar_tests;

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
    }

    #[test]
    fn enemy_selectors_include_ranking_and_statuses() {
        let p = provider();
        let cands = p.candidates("enemy:");
        let texts = insert_texts(&cands);
        assert_eq!(texts.len(), 7);
        assert!(texts.contains(&"@enemy:most-hp"));
        assert!(texts.contains(&"@enemy:least-hp"));
        assert!(texts.contains(&"@enemy:highest-damage"));
        assert!(texts.contains(&"@enemy:burn"));
        assert!(texts.contains(&"@enemy:frozen"));
        assert!(texts.contains(&"@enemy:shocked"));
        assert!(texts.contains(&"@enemy:rooted"));
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
}
