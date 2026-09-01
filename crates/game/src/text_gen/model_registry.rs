//! Manifest of the local text-generation models a player may select. Pure
//! data + lookups, no IO. `model_install` reads an entry's `gguf_url` /
//! `sha256` / `byte_size` to fetch and verify weights; `model_config`
//! resolves a `model_id` through `lookup`/`default_entry`.

/// SPDX-ish license id for a registry entry. The gate is
/// `permits_free_redistribution`, not the variant set: encumbered licenses
/// are representable so they can be named and REJECTED, never silently
/// allowed by omission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelLicense {
    Apache2_0,
    Mit,
    LlamaCommunity,
    GemmaTerms,
}

impl ModelLicense {
    pub const fn spdx_id(self) -> &'static str {
        match self {
            ModelLicense::Apache2_0 => "Apache-2.0",
            ModelLicense::Mit => "MIT",
            ModelLicense::LlamaCommunity => "LicenseRef-Llama-Community",
            ModelLicense::GemmaTerms => "LicenseRef-Gemma-Terms",
        }
    }

    /// True iff the license permits both redistribution of the weights and
    /// unrestricted use of the model's outputs.
    pub const fn permits_free_redistribution(self) -> bool {
        matches!(self, ModelLicense::Apache2_0 | ModelLicense::Mit)
    }
}

/// One selectable local model: identity, download source, and license.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelEntry {
    pub model_id: &'static str,
    pub display_name: &'static str,
    pub param_size: &'static str,
    pub gguf_url: &'static str,
    pub sha256: &'static str,
    pub byte_size: u64,
    pub license: ModelLicense,
}

pub const DEFAULT_MODEL_ID: &str = "qwen3-4b-instruct";

const REGISTRY: &[ModelEntry] = &[
    ModelEntry {
        model_id: "qwen3-4b-instruct",
        display_name: "Qwen3-4B-Instruct-2507",
        param_size: "4B",
        gguf_url: "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/a06e946bb6b655725eafa393f4a9745d460374c9/Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        sha256: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
        byte_size: 2497281120,
        license: ModelLicense::Apache2_0,
    },
    ModelEntry {
        model_id: "phi-4-mini-instruct",
        display_name: "Phi-4-mini-instruct",
        param_size: "3.8B",
        gguf_url: "https://huggingface.co/unsloth/Phi-4-mini-instruct-GGUF/resolve/78eb92a46fc37e6b524df991ed9aca9bc6aa7b80/Phi-4-mini-instruct-Q4_K_M.gguf",
        sha256: "88c00229914083cd112853aab84ed51b87bdf6b9ce42f532d8c85c7c63b1730a",
        byte_size: 2491874272,
        license: ModelLicense::Mit,
    },
    ModelEntry {
        model_id: "smollm2-1.7b-instruct",
        display_name: "SmolLM2-1.7B-Instruct",
        param_size: "1.7B",
        gguf_url: "https://huggingface.co/HuggingFaceTB/SmolLM2-1.7B-Instruct-GGUF/resolve/2d4a76a30b4af41ecd395c35725ac11688d4cfe4/smollm2-1.7b-instruct-q4_k_m.gguf",
        sha256: "decd2598bc2c8ed08c19adc3c8fdd461ee19ed5708679d1c54ef54a5a30d4f33",
        byte_size: 1055609536,
        license: ModelLicense::Apache2_0,
    },
];

/// The full registry: exactly the three selectable models.
pub fn all() -> &'static [ModelEntry] {
    REGISTRY
}

/// Looks up a registry entry by its stable `model_id`.
pub fn lookup(model_id: &str) -> Option<&'static ModelEntry> {
    REGISTRY.iter().find(|entry| entry.model_id == model_id)
}

/// The entry for `DEFAULT_MODEL_ID`.
pub fn default_entry() -> &'static ModelEntry {
    lookup(DEFAULT_MODEL_ID).expect("DEFAULT_MODEL_ID must resolve to a registry entry")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DEFAULT_MODEL_ID` names the default entry, and that entry resolves
    /// back to the same id.
    #[test]
    fn default_id_is_qwen3_4b_instruct() {
        assert_eq!(DEFAULT_MODEL_ID, "qwen3-4b-instruct");
        assert_eq!(default_entry().model_id, "qwen3-4b-instruct");
    }

    /// All three known ids resolve; an unknown id resolves to `None`.
    #[test]
    fn all_three_ids_resolve_and_unknown_is_none() {
        assert!(lookup("qwen3-4b-instruct").is_some());
        assert!(lookup("phi-4-mini-instruct").is_some());
        assert!(lookup("smollm2-1.7b-instruct").is_some());
        assert!(lookup("nonexistent").is_none());
    }

    /// The registry has exactly three entries with pairwise-unique ids.
    #[test]
    fn registry_has_three_unique_entries() {
        let entries = all();
        assert_eq!(entries.len(), 3);
        for (i, a) in entries.iter().enumerate() {
            for b in &entries[i + 1..] {
                assert_ne!(a.model_id, b.model_id);
            }
        }
    }

    /// Every registry entry's license permits free redistribution. This is
    /// the guard that must fail if an encumbered-license model is added.
    #[test]
    fn every_entry_license_is_redistributable() {
        for entry in all() {
            assert!(
                entry.license.permits_free_redistribution(),
                "{} has a license that does not permit free redistribution",
                entry.model_id
            );
        }
    }

    /// The allowlisted licenses permit redistribution; the excluded
    /// Llama/Gemma licenses do not, proving the gate actually bites.
    #[test]
    fn encumbered_license_is_rejected_by_gate() {
        assert!(ModelLicense::Apache2_0.permits_free_redistribution());
        assert!(ModelLicense::Mit.permits_free_redistribution());
        assert!(!ModelLicense::LlamaCommunity.permits_free_redistribution());
        assert!(!ModelLicense::GemmaTerms.permits_free_redistribution());
    }

    /// Every entry's digest is well-formed lowercase hex and its declared
    /// size is non-zero.
    #[test]
    fn entries_are_wellformed() {
        for entry in all() {
            assert_eq!(entry.sha256.len(), 64, "{} sha256 wrong length", entry.model_id);
            assert!(
                entry.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{} sha256 not lowercase hex",
                entry.model_id
            );
            assert!(entry.byte_size > 0, "{} byte_size must be > 0", entry.model_id);
        }
    }
}
