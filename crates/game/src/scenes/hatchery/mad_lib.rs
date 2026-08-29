//! Mad-lib sentence templates: an ordered sequence of literal text
//! interleaved with named blanks, a fixed in-code starter pool, deterministic
//! per-egg template selection keyed by the egg's index in `Hatchery.eggs`
//! (no bincode `Egg` schema field), and a composer that substitutes filled
//! blank values into a template's literals to produce the final sentence.

/// One piece of a template: fixed literal text, or a named editable blank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Literal(&'static str),
    Blank { label: &'static str },
}

/// Ordered mad-lib sentence: literal text interleaved with named blanks.
#[derive(Debug, Clone, Copy)]
pub struct MadLibTemplate {
    segments: &'static [Segment],
}

impl MadLibTemplate {
    pub const fn new(segments: &'static [Segment]) -> Self {
        MadLibTemplate { segments }
    }

    pub fn segments(&self) -> &'static [Segment] {
        self.segments
    }

    /// Number of `Segment::Blank` entries in this template.
    pub fn blank_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| matches!(s, Segment::Blank { .. }))
            .count()
    }

    /// Blank labels, in segment order.
    pub fn blank_labels(&self) -> impl Iterator<Item = &'static str> {
        self.segments.iter().filter_map(|s| match s {
            Segment::Blank { label } => Some(*label),
            Segment::Literal(_) => None,
        })
    }
}

const T0: MadLibTemplate = MadLibTemplate::new(&[
    Segment::Literal("A "),
    Segment::Blank { label: "size" },
    Segment::Literal(" creature with "),
    Segment::Blank { label: "temperament" },
    Segment::Literal(" eyes that fights by "),
    Segment::Blank { label: "signature move" },
    Segment::Literal("."),
]);

const T1: MadLibTemplate = MadLibTemplate::new(&[
    Segment::Literal("A "),
    Segment::Blank { label: "texture" },
    Segment::Literal(" hide covers this "),
    Segment::Blank { label: "size" },
    Segment::Literal(" beast."),
]);

const T2: MadLibTemplate = MadLibTemplate::new(&[
    Segment::Literal("Born in "),
    Segment::Blank { label: "habitat" },
    Segment::Literal(", it moves with a "),
    Segment::Blank { label: "gait" },
    Segment::Literal(" gait and a "),
    Segment::Blank { label: "temperament" },
    Segment::Literal(" nature."),
]);

static POOL: &[MadLibTemplate] = &[T0, T1, T2];

/// Fixed in-code starter pool (non-empty).
pub fn pool() -> &'static [MadLibTemplate] {
    POOL
}

/// Deterministic per-egg selection: the same `egg_index` always maps to the
/// same template. `egg_index` is required so every call site keys by egg.
pub fn select_template(egg_index: usize) -> &'static MadLibTemplate {
    &pool()[egg_index % pool().len()]
}

/// Compose the final sentence: literals verbatim, each blank replaced by its
/// positionally-matching value. Fewer values than blanks leaves those blanks
/// empty; extra values are ignored. Never panics.
pub fn completed_sentence<S: AsRef<str>>(t: &MadLibTemplate, values: &[S]) -> String {
    let mut out = String::new();
    let mut blank_idx = 0;
    for segment in t.segments {
        match segment {
            Segment::Literal(text) => out.push_str(text),
            Segment::Blank { .. } => {
                if let Some(value) = values.get(blank_idx) {
                    out.push_str(value.as_ref());
                }
                blank_idx += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_is_non_empty() {
        assert!(!pool().is_empty());
    }

    /// Two selections with the same index return the same template (checked
    /// via a stable projection: the composed sentence with a fixed value set).
    #[test]
    fn select_template_is_stable_for_same_index() {
        let a = select_template(1);
        let b = select_template(1);
        let values = ["x", "y", "z"];
        assert_eq!(completed_sentence(a, &values), completed_sentence(b, &values));
    }

    /// `select_template(k)` matches `pool()[k % pool().len()]`, including for
    /// an index past the pool length (wrap-around).
    #[test]
    fn select_template_matches_pool_modulo() {
        let len = pool().len();
        let values = ["x", "y", "z"];
        for k in [0usize, 1, 2, len, len + 1, 2 * len + 2] {
            let expected = &pool()[k % len];
            let got = select_template(k);
            assert_eq!(
                completed_sentence(got, &values),
                completed_sentence(expected, &values),
                "select_template({k}) did not match pool()[{k} % {len}]"
            );
        }
    }

    /// A fully-filled template composes literals and substituted values into
    /// the exact expected string, in order.
    #[test]
    fn completed_sentence_composes_full_template() {
        let t = MadLibTemplate::new(&[
            Segment::Literal("A "),
            Segment::Blank { label: "size" },
            Segment::Literal(" creature."),
        ]);
        let values = ["gigantic"];
        assert_eq!(completed_sentence(&t, &values), "A gigantic creature.");
    }

    /// Fewer values than blanks does not panic; the missing blank renders as
    /// empty text rather than dropping surrounding literals.
    #[test]
    fn completed_sentence_missing_values_render_empty() {
        let t = MadLibTemplate::new(&[
            Segment::Literal("A "),
            Segment::Blank { label: "size" },
            Segment::Literal(" creature with "),
            Segment::Blank { label: "temperament" },
            Segment::Literal(" eyes."),
        ]);
        let values: [&str; 0] = [];
        assert_eq!(completed_sentence(&t, &values), "A  creature with  eyes.");
    }

    /// Extra values beyond the blank count are ignored, not appended.
    #[test]
    fn completed_sentence_extra_values_are_ignored() {
        let t = MadLibTemplate::new(&[
            Segment::Literal("A "),
            Segment::Blank { label: "size" },
            Segment::Literal(" creature."),
        ]);
        let values = ["gigantic", "unused", "also-unused"];
        assert_eq!(completed_sentence(&t, &values), "A gigantic creature.");
    }

    /// For every pool template, `blank_count()` equals the number of `Blank`
    /// segments and `blank_labels()` lists them in segment order.
    #[test]
    fn blank_count_and_labels_match_segments() {
        for t in pool() {
            let expected_labels: Vec<&'static str> = t
                .segments()
                .iter()
                .filter_map(|s| match s {
                    Segment::Blank { label } => Some(*label),
                    Segment::Literal(_) => None,
                })
                .collect();
            assert_eq!(t.blank_count(), expected_labels.len());
            let labels: Vec<&'static str> = t.blank_labels().collect();
            assert_eq!(labels, expected_labels);
        }
    }
}
