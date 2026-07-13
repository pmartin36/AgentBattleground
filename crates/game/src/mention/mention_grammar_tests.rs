//! Grammar (spec 56) shape-check tests for `is_valid_mention` (b2-t2).
//! Not a full resolver — validates the token SHAPE the `10` parser will see.

use super::*;
use crate::ability::Ability;
use crate::creatures::Creature;

fn provider() -> GameMentionProvider {
    let creature = Creature::new("Test").with_abilities(vec![Ability::new("Ember Fang", vec![])]);
    let roster = vec![Creature::new("Ember Wolf")];
    GameMentionProvider::new(&creature, &roster)
}

/// Every terminal (`continues == false`) candidate the provider emits, across
/// stage-1 and every target's stage-2, must be a grammar-valid final token.
#[test]
fn every_terminal_emitted_token_conforms() {
    let p = provider();
    for query in ["", "self:", "ally:", "enemy:"] {
        for c in p.candidates(query) {
            if !c.continues {
                assert!(
                    is_valid_mention(&c.insert_text),
                    "terminal candidate {:?} (query {:?}) should be grammar-valid",
                    c,
                    query
                );
            }
        }
    }
}

/// `continues` target candidates (`@self:`/`@ally:`/`@enemy:`) are
/// intermediate two-stage forms: the dangling-colon form is NOT a final
/// token (rejected), but the bare target with the colon stripped IS valid.
#[test]
fn continues_targets_are_intermediate() {
    let p = provider();
    let cands = p.candidates("");
    let mut saw_continues = false;
    for c in cands.iter().filter(|c| c.continues) {
        saw_continues = true;
        assert!(c.insert_text.ends_with(':'), "{:?} should end with ':'", c);
        assert!(
            !is_valid_mention(&c.insert_text),
            "dangling-colon intermediate {:?} should NOT be grammar-valid",
            c
        );
        let bare = c.insert_text.trim_end_matches(':');
        assert!(is_valid_mention(bare), "bare target {:?} should be grammar-valid", bare);
    }
    assert!(saw_continues, "expected at least one continues candidate");
}

#[test]
fn well_formed_examples_accepted() {
    for token in [
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
    ] {
        assert!(is_valid_mention(token), "{:?} should be grammar-valid", token);
    }
}

#[test]
fn malformed_shapes_rejected() {
    for token in [
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
    ] {
        assert!(!is_valid_mention(token), "{:?} should be grammar-invalid", token);
    }
}
