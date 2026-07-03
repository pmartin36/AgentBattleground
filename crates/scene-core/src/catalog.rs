use crate::inspect::FieldSchema;
use crate::scene::Scene;
use crate::scene_key::SceneKey;

/// Game-supplied scene registry the engine dispatches through (spec 31, Decision 2).
///
/// Literal, non-generic shape (b3-t1 collapse): `Scene` now lives in scene-core
/// itself, so this returns `Box<dyn Scene>` directly rather than the Phase-A
/// interim associated-type form (`type Scene: ?Sized`).
pub trait SceneCatalog: Send + Sync {
    /// Build a fresh boxed scene for `key`. Panics for a cataloged-but-unbuilt key
    /// (mirrors today's `registry::construct` `unimplemented!()`); callers guard with
    /// `is_available` first (spec Risks: panic behavior preserved, not turned into Result).
    fn construct(&self, key: &SceneKey) -> Box<dyn Scene>;

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
    // b3-t1: rewritten against the shared `test_support` mock fixture — the
    // old generic-associated-type `MockScene`/`MockCatalog` pair (asserting
    // `.tag()` on a non-`Scene` mock trait) is gone with the collapsed
    // `SceneCatalog` shape; this now builds a real `dyn Scene` and asserts on
    // its real `id()`.
    use crate::test_support::MockCatalog;

    #[test]
    fn constructs_as_trait_object() {
        let catalog: Box<dyn SceneCatalog> = Box::new(MockCatalog);
        assert!(catalog.is_available(&SceneKey::new("A")));
    }

    #[test]
    fn construct_returns_boxed_scene_with_requested_id() {
        let catalog: Box<dyn SceneCatalog> = Box::new(MockCatalog);
        let scene = catalog.construct(&SceneKey::new("A"));
        assert_eq!(scene.id(), SceneKey::new("A"));
    }

    #[test]
    fn catalog_keys_and_display_name() {
        let catalog: Box<dyn SceneCatalog> = Box::new(MockCatalog);
        assert_eq!(
            catalog.catalog_keys(),
            vec![SceneKey::new("A"), SceneKey::new("B"), SceneKey::new("C")]
        );
        assert_eq!(catalog.display_name(&SceneKey::new("A")), "Mock A");
        assert_eq!(catalog.display_name(&SceneKey::new("B")), "Mock B");
    }

    #[test]
    fn is_available_partition() {
        let catalog: Box<dyn SceneCatalog> = Box::new(MockCatalog);
        assert!(catalog.is_available(&SceneKey::new("A")));
        assert!(!catalog.is_available(&SceneKey::new("Nope")));
    }

    #[test]
    fn schema_for_returns_fieldschema() {
        let catalog: Box<dyn SceneCatalog> = Box::new(MockCatalog);
        let schema = catalog.schema_for(&SceneKey::new("A"));
        assert_eq!(schema.tag, FieldTag::Struct);
    }
}
