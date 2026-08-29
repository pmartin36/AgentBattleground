//! Mad-lib parts prompt builder and parser: turns a completed mad-lib
//! sentence into a `TextRequest` asking the model for exactly the
//! creature's parts, and parses the model's plain-text completion back into
//! a parts struct (name, description, stat weighting, attack archetype).

use crate::construction::{StartingArchetype, StatWeighting};
use crate::text_gen::TextRequest;

/// The interpretive parts the model yields for one creature. Assembled with
/// the egg's `Element` and a seed into a `ConstructionRequest`.
#[derive(Debug, Clone, PartialEq)]
pub struct Parts {
    pub name: String,
    pub description: String,
    pub weighting: StatWeighting,
    pub archetype: StartingArchetype,
}

/// Archetype used when the model's completion is missing or names an
/// archetype outside the fixed set.
pub const FALLBACK_ARCHETYPE: StartingArchetype = StartingArchetype::Melee;

/// The only parse failure: the completion carried no usable name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsePartsError {
    NoName,
}

impl std::fmt::Display for ParsePartsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParsePartsError::NoName => write!(f, "no usable name in model completion"),
        }
    }
}

impl std::error::Error for ParsePartsError {}

// The single source for the labeled-line field names: written into the
// prompt and read back by `field()`. Keeping both sides on these consts is
// what prevents the offered format and the parsed format from drifting.
const LABEL_NAME: &str = "NAME";
const LABEL_DESCRIPTION: &str = "DESCRIPTION";
const LABEL_STRENGTH: &str = "STRENGTH";
const LABEL_DEXTERITY: &str = "DEXTERITY";
const LABEL_INTELLIGENCE: &str = "INTELLIGENCE";
const LABEL_VITALITY: &str = "VITALITY";
const LABEL_ARCHETYPE: &str = "ARCHETYPE";

/// The single source for archetype spelling. Exhaustive (no wildcard arm)
/// so a new `StartingArchetype` variant fails to compile here until it is
/// given a spelling, keeping the offered set and the parseable set in sync.
fn archetype_label(archetype: StartingArchetype) -> &'static str {
    match archetype {
        StartingArchetype::Ranged => "Ranged",
        StartingArchetype::Melee => "Melee",
        StartingArchetype::Debuff => "Debuff",
        StartingArchetype::Buff => "Buff",
    }
}

/// Matches `s` against every `StartingArchetype::ALL` label, case-insensitive.
fn parse_archetype(s: &str) -> Option<StartingArchetype> {
    StartingArchetype::ALL
        .into_iter()
        .find(|&archetype| archetype_label(archetype).eq_ignore_ascii_case(s.trim()))
}

/// Scans `text` line by line for the first line whose key (everything
/// before the first `:`, trimmed of surrounding whitespace and markdown
/// emphasis characters, compared case-insensitively) matches `label`, and
/// returns that line's trimmed value.
fn field<'a>(text: &'a str, label: &str) -> Option<&'a str> {
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches(|c: char| !c.is_alphanumeric());
        if key.eq_ignore_ascii_case(label) {
            return Some(value.trim());
        }
    }
    None
}

/// Parses the leading whitespace-delimited token of `v` as an `f32`,
/// defaulting to `0.0` if absent or unparseable — so trailing prose after a
/// numeric weight ("8 (high emphasis)") still yields the number.
fn parse_weight(v: Option<&str>) -> f32 {
    v.and_then(|v| v.split_whitespace().next())
        .and_then(|token| token.parse().ok())
        .unwrap_or(0.0)
}

/// Build the parts-request prompt for a completed mad-lib sentence: asks the
/// model for exactly name + description + 4 stat weights + one archetype,
/// never a finished creature.
pub fn build_parts_prompt(completed_sentence: &str) -> TextRequest {
    let archetype_options = StartingArchetype::ALL
        .into_iter()
        .map(archetype_label)
        .collect::<Vec<_>>()
        .join(", ");
    let system = format!(
        "You describe only the parts of a creature, never a finished stat block. \
Return exactly these labeled lines, one per line, and nothing else:\n\
{LABEL_NAME}: <a short creature name>\n\
{LABEL_DESCRIPTION}: <a one-sentence flavor description>\n\
{LABEL_STRENGTH}: <a number>\n\
{LABEL_DEXTERITY}: <a number>\n\
{LABEL_INTELLIGENCE}: <a number>\n\
{LABEL_VITALITY}: <a number>\n\
{LABEL_ARCHETYPE}: <one of {archetype_options}>"
    );
    TextRequest {
        system,
        user: completed_sentence.to_string(),
        temperature: 0.2,
        max_tokens: 200,
        stop: Vec::new(),
        seed: None,
    }
}

/// Parse the model's plain-text completion into `Parts`. Tolerant of
/// markdown/prose-wrapped label lines and case-insensitive keys; the only
/// failure is a missing/blank name.
pub fn parse_parts(text: &str) -> Result<Parts, ParsePartsError> {
    let name = field(text, LABEL_NAME)
        .filter(|s| !s.is_empty())
        .ok_or(ParsePartsError::NoName)?
        .to_string();
    let description = field(text, LABEL_DESCRIPTION).unwrap_or("").to_string();
    let weighting = StatWeighting {
        strength: parse_weight(field(text, LABEL_STRENGTH)),
        dexterity: parse_weight(field(text, LABEL_DEXTERITY)),
        intelligence: parse_weight(field(text, LABEL_INTELLIGENCE)),
        vitality: parse_weight(field(text, LABEL_VITALITY)),
    };
    let archetype = field(text, LABEL_ARCHETYPE)
        .and_then(parse_archetype)
        .unwrap_or(FALLBACK_ARCHETYPE);
    Ok(Parts { name, description, weighting, archetype })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WELL_FORMED: &str = "NAME: Ember\n\
DESCRIPTION: A tiny beast with smoldering eyes.\n\
STRENGTH: 8\n\
DEXTERITY: 4\n\
INTELLIGENCE: 2\n\
VITALITY: 6\n\
ARCHETYPE: Ranged\n";

    /// A fully labeled completion parses into the exact name, description,
    /// per-stat weighting, and archetype it names.
    #[test]
    fn well_formed_completion_parses_all_parts() {
        let parts = parse_parts(WELL_FORMED).expect("well-formed completion parses");
        assert_eq!(
            parts,
            Parts {
                name: "Ember".to_string(),
                description: "A tiny beast with smoldering eyes.".to_string(),
                weighting: StatWeighting {
                    strength: 8.0,
                    dexterity: 4.0,
                    intelligence: 2.0,
                    vitality: 6.0,
                },
                archetype: StartingArchetype::Ranged,
            }
        );
    }

    /// An archetype outside the fixed set falls back to the named constant
    /// rather than erroring or panicking.
    #[test]
    fn archetype_out_of_set_falls_back_to_melee() {
        let text = "NAME: Ember\nARCHETYPE: Wizard\n";
        let parts = parse_parts(text).expect("out-of-set archetype still parses");
        assert_eq!(parts.archetype, FALLBACK_ARCHETYPE);
    }

    /// No ARCHETYPE line at all falls back the same way as an out-of-set one.
    #[test]
    fn archetype_missing_falls_back() {
        let text = "NAME: Ember\n";
        let parts = parse_parts(text).expect("missing archetype still parses");
        assert_eq!(parts.archetype, FALLBACK_ARCHETYPE);
    }

    /// Archetype matching ignores case, for every archetype in the fixed set.
    #[test]
    fn archetype_parse_is_case_insensitive() {
        for (spelling, expected) in [
            ("ranged", StartingArchetype::Ranged),
            ("MELEE", StartingArchetype::Melee),
            ("Debuff", StartingArchetype::Debuff),
            ("bUfF", StartingArchetype::Buff),
        ] {
            let text = format!("NAME: Ember\nARCHETYPE: {spelling}\n");
            let parts = parse_parts(&text).expect("archetype line parses");
            assert_eq!(parts.archetype, expected, "spelling {spelling:?} did not map correctly");
        }
    }

    /// A completion with every field but a name is the sole `Err` case.
    #[test]
    fn missing_name_is_error() {
        let text = "DESCRIPTION: A tiny beast.\nARCHETYPE: Ranged\n";
        assert_eq!(parse_parts(text), Err(ParsePartsError::NoName));
    }

    /// Empty and whitespace-only input both carry no usable name.
    #[test]
    fn empty_input_is_error() {
        assert_eq!(parse_parts(""), Err(ParsePartsError::NoName));
        assert_eq!(parse_parts("   \n  \n"), Err(ParsePartsError::NoName));
    }

    /// Missing stat lines default every weight to 0.0 rather than erroring.
    #[test]
    fn missing_stats_default_to_zero() {
        let text = "NAME: Ember\nARCHETYPE: Ranged\n";
        let parts = parse_parts(text).expect("missing stats still parse");
        assert_eq!(parts.weighting, StatWeighting {
            strength: 0.0,
            dexterity: 0.0,
            intelligence: 0.0,
            vitality: 0.0,
        });
    }

    /// A missing DESCRIPTION line defaults to an empty string.
    #[test]
    fn missing_description_defaults_empty() {
        let text = "NAME: Ember\nARCHETYPE: Ranged\n";
        let parts = parse_parts(text).expect("missing description still parses");
        assert_eq!(parts.description, "");
    }

    /// A stat value followed by trailing prose still yields its leading
    /// number rather than defaulting to 0.0.
    #[test]
    fn weight_tolerates_trailing_text() {
        let text = "NAME: Ember\nSTRENGTH: 8 (high emphasis)\n";
        let parts = parse_parts(text).expect("trailing text after weight still parses");
        assert_eq!(parts.weighting.strength, 8.0);
    }

    /// A label line wrapped in markdown emphasis still resolves to its key.
    #[test]
    fn markdown_wrapped_labels_parse() {
        let text = "**NAME**: Ember\n";
        let parts = parse_parts(text).expect("markdown-wrapped label parses");
        assert_eq!(parts.name, "Ember");
    }

    /// The built prompt's user field carries the completed sentence verbatim.
    #[test]
    fn build_prompt_embeds_sentence() {
        let request = build_parts_prompt("A tiny beast with smoldering eyes.");
        assert_eq!(request.user, "A tiny beast with smoldering eyes.");
    }

    /// The prompt's system instructions name every archetype so the model's
    /// offered options and the parser's accepted spellings cannot drift.
    #[test]
    fn build_prompt_lists_every_archetype_and_label() {
        let request = build_parts_prompt("A tiny beast.");
        for archetype in StartingArchetype::ALL {
            let spelling = match archetype {
                StartingArchetype::Ranged => "Ranged",
                StartingArchetype::Melee => "Melee",
                StartingArchetype::Debuff => "Debuff",
                StartingArchetype::Buff => "Buff",
            };
            assert!(
                request.system.contains(spelling),
                "system prompt missing archetype label {spelling:?}: {}",
                request.system
            );
        }
        for label in ["NAME", "DESCRIPTION", "STRENGTH", "DEXTERITY", "INTELLIGENCE", "VITALITY", "ARCHETYPE"] {
            assert!(
                request.system.contains(label),
                "system prompt missing field label {label:?}: {}",
                request.system
            );
        }
    }
}
