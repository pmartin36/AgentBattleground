/// Opaque, engine-side scene identity on the wire (spec 31, Decision 1).
/// String-backed; serializes as a bare JSON string, byte-identical to `SceneId`'s
/// wire encoding so the M1/M2 envelope shape is unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SceneKey(String);

impl SceneKey {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl serde::Serialize for SceneKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for SceneKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(SceneKey::new(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_as_str_round_trip() {
        let key = SceneKey::new("MainHub");
        assert_eq!(key.as_str(), "MainHub");
    }

    #[test]
    fn serializes_as_bare_json_string() {
        let json = serde_json::to_string(&SceneKey::new("BattleViewer")).expect("serialize");
        assert_eq!(json, "\"BattleViewer\"");
    }

    #[test]
    fn deserializes_from_bare_json_string() {
        let decoded: SceneKey = serde_json::from_str("\"BattleViewer\"").expect("deserialize");
        assert_eq!(decoded, SceneKey::new("BattleViewer"));
    }

    #[test]
    fn serde_round_trip() {
        let original = SceneKey::new("Leaderboard");
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: SceneKey = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, decoded);
    }

    #[test]
    fn equal_strings_are_eq_and_hash_equal() {
        let a = SceneKey::new("RosterManager");
        let b = SceneKey::new("RosterManager");
        assert_eq!(a, b);

        let mut set = std::collections::HashSet::new();
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 1, "equal keys must hash to the same slot");
    }

    #[test]
    fn different_strings_not_equal() {
        assert_ne!(SceneKey::new("A"), SceneKey::new("B"));
    }
}
