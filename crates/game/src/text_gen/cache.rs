//! The opt-in `generate_text` result cache, keyed by a normalized derived
//! key (model identity, prompt, params, seed). Off by default; engaged only
//! when a request opts in. Filled by a later task.
