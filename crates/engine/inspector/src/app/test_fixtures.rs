//! Test-only fixtures shared across 2+ of the sibling test modules.

use engine_core::inspect::{FieldSchema, FieldTag};
use engine_core::ipc::{CatalogEntry, Hello, Message};
use engine_core::SceneKey;
use crate::client::tests::stub_schema;

pub(super) fn leaf(name: &str, tag: FieldTag) -> FieldSchema {
    FieldSchema {
        name: name.to_string(),
        label: None,
        tag,
        readonly: false,
        hidden: false,
        range: None,
        children: vec![],
        variants: vec![],
    }
}

pub(super) fn struct_schema(name: &str, children: Vec<FieldSchema>) -> FieldSchema {
    FieldSchema {
        name: name.to_string(),
        label: None,
        tag: FieldTag::Struct,
        readonly: false,
        hidden: false,
        range: None,
        children,
        variants: vec![],
    }
}

pub(super) fn list_schema(name: &str, element: FieldSchema) -> FieldSchema {
    FieldSchema {
        name: name.to_string(),
        label: None,
        tag: FieldTag::List,
        readonly: false,
        hidden: false,
        range: None,
        children: vec![element],
        variants: vec![],
    }
}
/// Like `stub_schema` but with `field_count` synthetic `Bool` children, so
/// distinct schemas are actually distinct in shape (not four identical stubs).
pub(super) fn stub_schema_with_fields(name: &str, field_count: usize) -> FieldSchema {
    let mut s = stub_schema(name);
    s.children = (0..field_count)
        .map(|i| FieldSchema {
            name: format!("f{i}"),
            label: None,
            tag: FieldTag::Bool,
            readonly: false,
            hidden: false,
            range: None,
            children: vec![],
            variants: vec![],
        })
        .collect();
    s
}

pub(super) fn four_scene_hello() -> Message {
    Message::Hello(Hello {
        scenes: vec![
            CatalogEntry {
                id: SceneKey::new("MainHub"),
                name: "Main Hub".to_string(),
                schema: stub_schema_with_fields("MainHub", 1),
            },
            CatalogEntry {
                id: SceneKey::new("BattleViewer"),
                name: "Battle Viewer".to_string(),
                schema: stub_schema_with_fields("BattleViewer", 2),
            },
            CatalogEntry {
                id: SceneKey::new("RosterManager"),
                name: "Roster".to_string(),
                schema: stub_schema_with_fields("RosterManager", 3),
            },
            CatalogEntry {
                id: SceneKey::new("Leaderboard"),
                name: "Leaderboard".to_string(),
                schema: stub_schema_with_fields("Leaderboard", 4),
            },
        ],
        active: SceneKey::new("MainHub"),
    })
}

/// A second, disjoint Hello (different scene set) — used to prove a fresh
/// Hello REBUILDS `schema_cache` rather than merging into it.
pub(super) fn two_scene_hello_distinct() -> Message {
    Message::Hello(Hello {
        scenes: vec![
            CatalogEntry {
                id: SceneKey::new("RosterManager"),
                name: "Roster".to_string(),
                schema: stub_schema_with_fields("RosterManager2", 5),
            },
            CatalogEntry {
                id: SceneKey::new("Leaderboard"),
                name: "Leaderboard".to_string(),
                schema: stub_schema_with_fields("Leaderboard2", 6),
            },
        ],
        active: SceneKey::new("Leaderboard"),
    })
}
