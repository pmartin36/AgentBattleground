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
}

impl SceneId {
    /// Returns a slice of all nine variants in catalog (declaration) order.
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
        }
    }

    /// Decode a wire string back to a variant. Returns `None` for unknown strings.
    /// Implemented by scanning `all()` + `wire_name()` — single source of truth.
    pub fn from_wire(s: &str) -> Option<SceneId> {
        SceneId::all().iter().copied().find(|v| v.wire_name() == s)
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

    #[test]
    fn all_has_nine_unique_variants() {
        let variants = SceneId::all();
        assert_eq!(variants.len(), 9, "all() must return exactly 9 variants");
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
}
