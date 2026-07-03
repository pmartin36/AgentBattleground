use crate::inspect::FieldSchema;
use crate::scene_key::SceneKey;

/// Game-supplied scene registry the engine dispatches through (spec 31, Decision 2).
///
/// Phase-A interim shape: `Scene` is an associated type rather than the spec's literal
/// `Box<dyn Scene>`, because the `Scene` trait still lives in `game` this phase and
/// scene-core must not depend on `game`. `game`'s `GameCatalog` binds
/// `type Scene = dyn crate::scene::Scene`; Phase B moves `Scene` into engine-core and
/// collapses this back to the literal `Box<dyn Scene>`.
pub trait SceneCatalog: Send {
    /// The engine-side scene object this catalog constructs. `game` binds this to
    /// its `dyn Scene`; unsized so it can be a trait object.
    type Scene: ?Sized;

    /// Build a fresh boxed scene for `key`. Panics for a cataloged-but-unbuilt key
    /// (mirrors today's `registry::construct` `unimplemented!()`); callers guard with
    /// `is_available` first (spec Risks: panic behavior preserved, not turned into Result).
    fn construct(&self, key: &SceneKey) -> Box<Self::Scene>;

    /// Type-level schema for `key` — the source of every `CatalogEntry.schema`.
    /// Same panic contract as `construct` for an unbuilt key.
    fn schema_for(&self, key: &SceneKey) -> FieldSchema;

    /// Human-readable label for `key`.
    fn display_name(&self, key: &SceneKey) -> &str;

    /// Ordered catalog for `Hello` — replaces today's hardcoded `'1'..'4'` scan.
    fn catalog_keys(&self) -> Vec<SceneKey>;

    /// Cheap availability check (today's `registry::is_implemented`) — separate from
    /// `construct` so checking doesn't build+discard a scene with real `enter()` effects.
    fn is_available(&self, key: &SceneKey) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::FieldTag;

    trait MockScene {
        fn tag(&self) -> &str;
    }

    struct ConcreteMockScene(&'static str);

    impl MockScene for ConcreteMockScene {
        fn tag(&self) -> &str {
            self.0
        }
    }

    struct MockCatalog;

    impl SceneCatalog for MockCatalog {
        type Scene = dyn MockScene;

        fn construct(&self, key: &SceneKey) -> Box<Self::Scene> {
            match key.as_str() {
                "A" => Box::new(ConcreteMockScene("tag-a")),
                "B" => Box::new(ConcreteMockScene("tag-b")),
                other => panic!("MockCatalog::construct: unbuilt key {other}"),
            }
        }

        fn schema_for(&self, key: &SceneKey) -> FieldSchema {
            match key.as_str() {
                "A" => FieldSchema {
                    name: "A".to_string(),
                    label: None,
                    tag: FieldTag::Struct,
                    readonly: false,
                    hidden: false,
                    range: None,
                    children: Vec::new(),
                    variants: Vec::new(),
                },
                other => panic!("MockCatalog::schema_for: unbuilt key {other}"),
            }
        }

        fn display_name(&self, key: &SceneKey) -> &str {
            match key.as_str() {
                "A" => "Mock A",
                "B" => "Mock B",
                other => panic!("MockCatalog::display_name: unbuilt key {other}"),
            }
        }

        fn catalog_keys(&self) -> Vec<SceneKey> {
            vec![SceneKey::new("A"), SceneKey::new("B")]
        }

        fn is_available(&self, key: &SceneKey) -> bool {
            matches!(key.as_str(), "A" | "B")
        }
    }

    #[test]
    fn constructs_as_trait_object() {
        let catalog: Box<dyn SceneCatalog<Scene = dyn MockScene>> = Box::new(MockCatalog);
        assert!(catalog.is_available(&SceneKey::new("A")));
    }

    #[test]
    fn construct_returns_boxed_scene() {
        let catalog: Box<dyn SceneCatalog<Scene = dyn MockScene>> = Box::new(MockCatalog);
        let scene = catalog.construct(&SceneKey::new("A"));
        assert_eq!(scene.tag(), "tag-a");
    }

    #[test]
    fn catalog_keys_and_display_name() {
        let catalog: Box<dyn SceneCatalog<Scene = dyn MockScene>> = Box::new(MockCatalog);
        assert_eq!(
            catalog.catalog_keys(),
            vec![SceneKey::new("A"), SceneKey::new("B")]
        );
        assert_eq!(catalog.display_name(&SceneKey::new("A")), "Mock A");
        assert_eq!(catalog.display_name(&SceneKey::new("B")), "Mock B");
    }

    #[test]
    fn is_available_partition() {
        let catalog: Box<dyn SceneCatalog<Scene = dyn MockScene>> = Box::new(MockCatalog);
        assert!(catalog.is_available(&SceneKey::new("A")));
        assert!(!catalog.is_available(&SceneKey::new("Nope")));
    }

    #[test]
    fn schema_for_returns_fieldschema() {
        let catalog: Box<dyn SceneCatalog<Scene = dyn MockScene>> = Box::new(MockCatalog);
        let schema = catalog.schema_for(&SceneKey::new("A"));
        assert_eq!(schema.name, "A");
        assert_eq!(schema.tag, FieldTag::Struct);
    }
}
