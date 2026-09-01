//! Live end-to-end test against a REAL bundled `llm-cli` + a downloaded
//! model, driving the game's own `LocalBackend` (real subprocess) and
//! `parts::parse_parts` — the exact path the hatchery Define uses. `#[ignore]`d
//! because it spawns the multi-GB model, so it never runs in the offline gate.
//!
//! Run it with:
//!   cargo test -p game --lib live_local_model -- --ignored --nocapture
//!
//! It locates the runtime + weights from `ABG_LLM_CLI` / `ABG_LLM_MODEL`, else
//! the `experiments/creature_lab` copies. This is the RED/GREEN that proves the
//! `-sysf` fix: on the pre-fix combined-`-f` invocation the model's echoed
//! system template (`NAME: <a short creature name>`) lands on stdout ahead of
//! the real answer and `parse_parts` reads the `<placeholder>`, so the name
//! assertion FAILS; on the fixed invocation stdout carries only the real
//! completion and it PASSES.
#![cfg(test)]

use std::path::PathBuf;

use crate::scenes::hatchery::parts;
use crate::text_gen::backend::TextBackend;
use crate::text_gen::backend_local::LocalBackend;
use crate::text_gen::job::CancelFlag;
use crate::text_gen::types::ResolvedModelConfig;

/// The real runtime binary + model weights, from env overrides or the
/// in-repo `experiments/creature_lab` copies. `None` (skip) if absent.
fn runtime_and_weights() -> Option<(PathBuf, PathBuf)> {
    let bin = std::env::var("ABG_LLM_CLI")
        .ok()
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("experiments/creature_lab/llama.cpp/build/bin/llama-cli")))
        .filter(|p| p.exists())?;
    let model = std::env::var("ABG_LLM_MODEL")
        .ok()
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("experiments/creature_lab/models/Qwen3-4B-Instruct-2507-Q4_K_M.gguf")))
        .filter(|p| p.exists())?;
    Some((bin, model))
}

#[test]
#[ignore = "requires the bundled llm-cli binary + a downloaded model; run with --ignored"]
fn live_local_model_generates_real_parts() {
    let Some((bin, model)) = runtime_and_weights() else {
        panic!(
            "llm-cli and/or model weights not found; set ABG_LLM_CLI and ABG_LLM_MODEL, \
             or place them under experiments/creature_lab/"
        );
    };

    let config = ResolvedModelConfig::local_registry("qwen3-4b-instruct", bin, model);
    let backend = LocalBackend::new(config);
    let sentence = "a fierce armored beetle that channels lightning through its horns";
    let request = parts::build_parts_prompt(sentence);

    let output = backend
        .generate(&request, &CancelFlag::new())
        .expect("local-model generation must succeed against the real runtime");
    eprintln!("--- raw model stdout ---\n{output}\n--- end raw ---");

    let parsed = parts::parse_parts(&output)
        .expect("the model completion must parse into creature parts");
    eprintln!(
        "PARSED: name={:?} archetype={:?} weighting={:?} description={:?}",
        parsed.name, parsed.archetype, parsed.weighting, parsed.description
    );

    // The bug: the pre-fix combined `-f` prompt echoes the system template's
    // `NAME: <a short creature name>` to stdout ahead of the real answer, so
    // `parse_parts` would read the placeholder. A real name proves the fix.
    assert!(!parsed.name.is_empty(), "parsed name must be non-empty");
    assert!(
        !parsed.name.contains('<') && !parsed.name.contains('>'),
        "parsed name must be a real name, not the echoed `<placeholder>`, got {:?}",
        parsed.name
    );
}
