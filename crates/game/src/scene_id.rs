use engine_core::SceneKey;

/// Closed enum of every scene in the catalog (spec 14, M1).
/// Wire-encoded as its exact Rust variant name string (e.g. `"BattleViewer"`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SceneId {
    Onboarding,
    MainHub,
    RosterManager,
    Matchmaking,
    BattleViewer,
    PostBattle,
    ReplayBrowser,
    Leaderboard,
    Settings,
    Hatchery,
}

impl SceneId {
    /// Returns a slice of all ten variants in catalog (declaration) order.
    pub fn all() -> &'static [SceneId] {
        use SceneId::*;
        &[
            Onboarding,
            MainHub,
            RosterManager,
            Matchmaking,
            BattleViewer,
            PostBattle,
            ReplayBrowser,
            Leaderboard,
            Settings,
            Hatchery,
        ]
    }

    /// Human-readable label (e.g. `"Battle Viewer"`).
    pub fn display_name(self) -> &'static str {
        match self {
            SceneId::Onboarding => "Onboarding",
            SceneId::MainHub => "Main Hub",
            SceneId::RosterManager => "Roster",
            SceneId::Matchmaking => "Matchmaking",
            SceneId::BattleViewer => "Battle Viewer",
            SceneId::PostBattle => "Post Battle",
            SceneId::ReplayBrowser => "Replay Browser",
            SceneId::Leaderboard => "Leaderboard",
            SceneId::Settings => "Settings",
            SceneId::Hatchery => "Hatchery",
        }
    }

    /// On-wire identity: the exact Rust variant name (e.g. `"BattleViewer"`).
    pub fn wire_name(self) -> &'static str {
        match self {
            SceneId::Onboarding => "Onboarding",
            SceneId::MainHub => "MainHub",
            SceneId::RosterManager => "RosterManager",
            SceneId::Matchmaking => "Matchmaking",
            SceneId::BattleViewer => "BattleViewer",
            SceneId::PostBattle => "PostBattle",
            SceneId::ReplayBrowser => "ReplayBrowser",
            SceneId::Leaderboard => "Leaderboard",
            SceneId::Settings => "Settings",
            SceneId::Hatchery => "Hatchery",
        }
    }

    /// Decode a wire string back to a variant. Returns `None` for unknown strings.
    /// Implemented by scanning `all()` + `wire_name()` — single source of truth.
    pub fn from_wire(s: &str) -> Option<SceneId> {
        SceneId::all().iter().copied().find(|v| v.wire_name() == s)
    }

    /// Decode an opaque wire key back to a variant. `None` for unknown keys.
    /// Delegates to `from_wire` — single source of truth for the mapping.
    pub fn from_key(k: &SceneKey) -> Option<SceneId> {
        SceneId::from_wire(k.as_str())
    }
}

impl From<SceneId> for SceneKey {
    fn from(id: SceneId) -> SceneKey {
        SceneKey::new(id.wire_name())
    }
}

impl serde::Serialize for SceneId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.wire_name())
    }
}

impl<'de> serde::Deserialize<'de> for SceneId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str as serde::Deserialize>::deserialize(deserializer)?;
        SceneId::from_wire(s).ok_or_else(|| {
            serde::de::Error::custom(format!("unknown SceneId wire name: {:?}", s))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- all() ---

    /// Compile-time guard that `SceneId::all()` cannot silently miss a
    /// variant. `display_name()`/`wire_name()` already force a touchpoint
    /// when a variant is added (their own matches are exhaustive over the
    /// enum, so the compiler rejects a missing arm) — `all()` is a bare
    /// literal array with no such enforcement, and could drift out of sync
    /// with the enum with nothing catching it until some other, unrelated
    /// runtime test happens to notice.
    ///
    /// This match is exhaustive with NO wildcard arm, specifically so that
    /// adding a new `SceneId` variant fails to compile right here — do not
    /// add a `_ => {}` arm to silence a compile error; that defeats the
    /// entire point. When you do get a compile error from this match because
    /// you added a variant: add the new arm here, AND add it to `all()`,
    /// AND bump the expected count in `all_has_nine_unique_variants` above.
    fn assert_variant_is_in_all(v: SceneId) {
        match v {
            SceneId::Onboarding
            | SceneId::MainHub
            | SceneId::RosterManager
            | SceneId::Matchmaking
            | SceneId::BattleViewer
            | SceneId::PostBattle
            | SceneId::ReplayBrowser
            | SceneId::Leaderboard
            | SceneId::Settings
            | SceneId::Hatchery => {
                assert!(
                    SceneId::all().contains(&v),
                    "{v:?} is a SceneId variant but missing from SceneId::all()"
                );
            }
        }
    }

    #[test]
    fn all_variant_exhaustiveness_guard() {
        // One call per current variant. If you just added a new variant to
        // fix a compile error in `assert_variant_is_in_all` above, add its
        // call here too.
        assert_variant_is_in_all(SceneId::Onboarding);
        assert_variant_is_in_all(SceneId::MainHub);
        assert_variant_is_in_all(SceneId::RosterManager);
        assert_variant_is_in_all(SceneId::Matchmaking);
        assert_variant_is_in_all(SceneId::BattleViewer);
        assert_variant_is_in_all(SceneId::PostBattle);
        assert_variant_is_in_all(SceneId::ReplayBrowser);
        assert_variant_is_in_all(SceneId::Leaderboard);
        assert_variant_is_in_all(SceneId::Settings);
        assert_variant_is_in_all(SceneId::Hatchery);
    }

    #[test]
    fn all_has_ten_unique_variants() {
        let variants = SceneId::all();
        assert_eq!(variants.len(), 10, "all() must return exactly 10 variants");
        // every variant must appear exactly once
        let mut seen = std::collections::HashSet::new();
        for v in variants {
            assert!(seen.insert(format!("{:?}", v)), "duplicate variant: {:?}", v);
        }
    }

    // --- wire_name / from_wire round-trip ---

    #[test]
    fn wire_name_roundtrips_for_every_variant() {
        for &v in SceneId::all() {
            let wire = v.wire_name();
            let decoded = SceneId::from_wire(wire);
            assert_eq!(decoded, Some(v), "round-trip failed for {:?} (wire: {:?})", v, wire);
        }
    }

    #[test]
    fn wire_name_equals_rust_variant_identifier_battle_viewer() {
        assert_eq!(SceneId::BattleViewer.wire_name(), "BattleViewer");
    }

    #[test]
    fn wire_name_equals_rust_variant_identifier_main_hub() {
        assert_eq!(SceneId::MainHub.wire_name(), "MainHub");
    }

    #[test]
    fn wire_name_equals_rust_variant_identifier_roster_manager() {
        assert_eq!(SceneId::RosterManager.wire_name(), "RosterManager");
    }

    // --- from_wire unknown / empty inputs ---

    #[test]
    fn from_wire_unknown_string_returns_none() {
        assert_eq!(SceneId::from_wire("Nope"), None);
    }

    #[test]
    fn from_wire_empty_string_returns_none() {
        assert_eq!(SceneId::from_wire(""), None);
    }

    #[test]
    fn from_wire_lowercase_variant_returns_none() {
        // wire protocol is case-sensitive — "battleviewer" is not valid
        assert_eq!(SceneId::from_wire("battleviewer"), None);
    }

    // --- display_name ---

    #[test]
    fn display_name_battle_viewer_is_spaced() {
        // spec 14 pins this value explicitly in the Hello example
        assert_eq!(SceneId::BattleViewer.display_name(), "Battle Viewer");
    }

    #[test]
    fn display_name_roster_manager_is_roster() {
        assert_eq!(SceneId::RosterManager.display_name(), "Roster");
    }

    #[test]
    fn from_wire_roster_manager_resolves() {
        assert_eq!(SceneId::from_wire("RosterManager"), Some(SceneId::RosterManager));
    }

    #[test]
    fn wire_name_equals_rust_variant_identifier_hatchery() {
        assert_eq!(SceneId::Hatchery.wire_name(), "Hatchery");
    }

    #[test]
    fn display_name_hatchery_is_hatchery() {
        assert_eq!(SceneId::Hatchery.display_name(), "Hatchery");
    }

    #[test]
    fn from_wire_hatchery_resolves() {
        assert_eq!(SceneId::from_wire("Hatchery"), Some(SceneId::Hatchery));
    }

    // --- serde ---

    #[test]
    fn serde_serializes_as_wire_string() {
        let json = serde_json::to_string(&SceneId::BattleViewer).expect("serialize");
        assert_eq!(json, "\"BattleViewer\"");
    }

    #[test]
    fn serde_round_trip_via_json() {
        let original = SceneId::MainHub;
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: SceneId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, decoded);
    }

    #[test]
    fn serde_rejects_unknown_variant() {
        let result = serde_json::from_str::<SceneId>("\"Nope\"");
        assert!(result.is_err(), "deserializing an unknown wire name must fail");
    }

    // --- SceneKey conversions (b1-t3) ---

    #[test]
    fn from_scene_id_to_scene_key_roundtrips_for_every_variant() {
        for &id in SceneId::all() {
            let key: SceneKey = id.into();
            assert_eq!(
                SceneId::from_key(&key),
                Some(id),
                "round-trip failed for {:?} (key: {:?})",
                id,
                key.as_str()
            );
        }
    }

    #[test]
    fn from_scene_id_to_scene_key_matches_wire_name_battle_viewer() {
        let key: SceneKey = SceneId::BattleViewer.into();
        assert_eq!(key.as_str(), "BattleViewer");
    }

    #[test]
    fn from_key_unknown_string_returns_none() {
        assert_eq!(SceneId::from_key(&SceneKey::new("Nope")), None);
    }

    #[test]
    fn from_key_empty_string_returns_none() {
        assert_eq!(SceneId::from_key(&SceneKey::new("")), None);
    }
}
