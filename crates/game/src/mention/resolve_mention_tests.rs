//! `resolve_mention`'s tests (b1-t3): the context-aware resolution layer
//! built on top of `parse_mention` (b1-t2) + `Vocabulary` (b1-t1).

use super::*;
use crate::lint_test_fixture::{first_index_with_role, roster, vocab};

#[test]
fn own_ability_resolves_by_index() {
    let vocab = vocab();
    assert_eq!(resolve_mention("@Ember_Fang", &vocab), Ok(Resolved::Ability { index: 0 }));
    assert_eq!(resolve_mention("@Howl", &vocab), Ok(Resolved::Ability { index: 1 }));
}

/// The case that is impossible without `roster_ability_names`: "Frost Bite"
/// belongs to a roster creature (Frost Lizard), not the edited one.
#[test]
fn roster_creatures_ability_is_not_an_ability() {
    let vocab = vocab();
    assert_eq!(
        resolve_mention("@Frost_Bite", &vocab),
        Err(ResolveError::Diagnostic(DiagnosticKind::NotAnAbility {
            token: "Frost_Bite".to_string()
        }))
    );
}

/// Pins `CreatureNotOnRoster` apart from `NotAnAbility`: a name that is
/// neither an ability nor a roster creature at all.
#[test]
fn off_roster_creature_is_creature_not_on_roster() {
    let vocab = vocab();
    let result = resolve_mention("@Volt_Scorpion", &vocab);
    assert_eq!(
        result,
        Err(ResolveError::Diagnostic(DiagnosticKind::CreatureNotOnRoster {
            token: "Volt_Scorpion".to_string()
        }))
    );
    assert_ne!(
        result,
        Err(ResolveError::Diagnostic(DiagnosticKind::NotAnAbility {
            token: "Volt_Scorpion".to_string()
        }))
    );
}

#[test]
fn active_slot_creature_resolves_ok() {
    let vocab = vocab();
    let roster = roster();
    let i = first_index_with_role(SquadRole::Active);
    let token = underscore_name(roster[i].name());
    assert_eq!(resolve_mention(&format!("@{token}"), &vocab), Ok(Resolved::Creature { index: i }));
}

#[test]
fn bench_slot_creature_resolves_ok() {
    let vocab = vocab();
    let roster = roster();
    let i = first_index_with_role(SquadRole::Bench);
    let token = underscore_name(roster[i].name());
    assert_eq!(resolve_mention(&format!("@{token}"), &vocab), Ok(Resolved::Creature { index: i }));
}

#[test]
fn reserve_slot_creature_is_not_fielded() {
    let vocab = vocab();
    let roster = roster();
    let i = first_index_with_role(SquadRole::Reserve);
    let token = underscore_name(roster[i].name());
    assert_eq!(
        resolve_mention(&format!("@{token}"), &vocab),
        Err(ResolveError::Diagnostic(DiagnosticKind::CreatureNotFielded { token }))
    );
}

/// `UnknownTarget.token` names the offending TARGET (`"foo"`); by contrast
/// `BadSelectorForTarget.token` names the offending SELECTOR (`"most-hp"`,
/// not `"self"` — `self` is a valid target, the selector is what's wrong).
#[test]
fn unknown_target_and_bad_selector_are_distinct_diagnostics() {
    let vocab = vocab();
    let unknown = resolve_mention("@foo:burn", &vocab);
    let bad_selector = resolve_mention("@self:most-hp", &vocab);
    assert_eq!(
        unknown,
        Err(ResolveError::Diagnostic(DiagnosticKind::UnknownTarget { token: "foo".to_string() }))
    );
    assert_eq!(
        bad_selector,
        Err(ResolveError::Diagnostic(DiagnosticKind::BadSelectorForTarget {
            token: "most-hp".to_string()
        }))
    );
    assert_ne!(unknown, bad_selector);
}

#[test]
fn well_formed_targets_bypass_vocab_lookup() {
    let vocab = vocab();
    assert_eq!(
        resolve_mention("@self:frozen", &vocab),
        Ok(Resolved::Target { target: "self", selector: Some("frozen") })
    );
    assert_eq!(
        resolve_mention("@enemy", &vocab),
        Ok(Resolved::Target { target: "enemy", selector: None })
    );
}

/// Anti-drift pin: `resolve_mention`'s `DiagnosticKind::token` must equal
/// the EXACT substring `parse_mention` reports for the same grammar
/// failure — nobody downstream may independently re-derive it.
#[test]
fn diagnostic_token_matches_parse_mention_payload() {
    let vocab = vocab();

    let target = match parse_mention("@foo:burn") {
        Err(GrammarError::UnknownTarget { target }) => target,
        other => panic!("expected UnknownTarget, got {:?}", other),
    };
    assert_eq!(
        resolve_mention("@foo:burn", &vocab),
        Err(ResolveError::Diagnostic(DiagnosticKind::UnknownTarget { token: target.to_string() }))
    );

    let selector = match parse_mention("@self:most-hp") {
        Err(GrammarError::BadSelectorForTarget { selector }) => selector,
        other => panic!("expected BadSelectorForTarget, got {:?}", other),
    };
    assert_eq!(
        resolve_mention("@self:most-hp", &vocab),
        Err(ResolveError::Diagnostic(DiagnosticKind::BadSelectorForTarget {
            token: selector.to_string()
        }))
    );
}

#[test]
fn ungrammatical_tokens_are_malformed() {
    let vocab = vocab();
    for token in ["self", "", "@Ember Wolf"] {
        assert_eq!(
            resolve_mention(token, &vocab),
            Err(ResolveError::Malformed),
            "{:?} should be Malformed",
            token
        );
    }
}
