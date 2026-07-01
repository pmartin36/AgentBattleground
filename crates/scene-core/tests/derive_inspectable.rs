//! Integration tests for `#[derive(Inspectable)]` (`scene-core-derive`).
//!
//! Separate integration-test crate (per research.md's Test surface) so
//! `::scene_core::...` paths emitted by the derive resolve exactly the way a
//! real downstream crate (e.g. `render`, b3-t1) would see them. Deliberately
//! does NOT `use serde_json;` — the derive's generated code must route
//! through `scene_core::__private::serde_json`, never a bare `serde_json::`
//! path (research.md MUST-1); this file only ever references `serde_json::`
//! via its own direct dependency, fully qualified, proving the derive itself
//! never requires the caller to import it.

use scene_core::{FieldTag, Inspectable, Range};
use serde::Serialize;

// --- (a) synthetic struct exercising all 8 non-Json tags at once ------------

#[derive(Inspectable)]
enum Shade {
    Light,
    Dark,
}

#[derive(Inspectable)]
struct Nested {
    x: f32,
    y: f32,
}

#[derive(Inspectable)]
struct Item {
    value: i32,
}

#[derive(Inspectable)]
struct AllTags {
    flag: bool,
    #[inspect(label = "Count", range = 0..10)]
    count: i32,
    ratio: f32,
    name: String,
    mode: Shade,
    tint: scene_core::color::Rgba,
    nested: Nested,
    items: Vec<Item>,
    #[inspect(readonly)]
    locked: i32,
    #[inspect(hidden)]
    #[allow(dead_code)] // derive must never read a hidden field (MUST-2); constructed
                        // only to prove its absence from schema/snapshot below.
    secret: String,
}

#[test]
fn all_tags_struct_schema_reports_each_field_tag_label_range_readonly_and_hides_hidden_field() {
    let schema = AllTags::schema();
    assert_eq!(schema.tag, FieldTag::Struct);

    let by_name = |n: &str| {
        schema
            .children
            .iter()
            .find(|c| c.name == n)
            .unwrap_or_else(|| panic!("missing child schema for field `{n}`"))
    };

    assert_eq!(by_name("flag").tag, FieldTag::Bool);

    let count = by_name("count");
    assert_eq!(count.tag, FieldTag::Int);
    assert_eq!(count.label.as_deref(), Some("Count"));
    assert_eq!(count.range, Some(Range { min: 0.0, max: 10.0 }));

    assert_eq!(by_name("ratio").tag, FieldTag::Float);
    assert_eq!(by_name("name").tag, FieldTag::String);
    assert_eq!(by_name("mode").tag, FieldTag::Enum);
    assert_eq!(by_name("tint").tag, FieldTag::Color);
    assert_eq!(by_name("nested").tag, FieldTag::Struct);
    assert_eq!(by_name("items").tag, FieldTag::List);
    assert!(by_name("locked").readonly);

    assert!(
        schema.children.iter().all(|c| c.name != "secret"),
        "hidden field must be absent from schema.children"
    );

    let instance = AllTags {
        flag: true,
        count: 3,
        ratio: 1.5,
        name: "n".to_string(),
        mode: Shade::Dark,
        tint: scene_core::color::Rgba::new(1, 2, 3, 4),
        nested: Nested { x: 0.0, y: 0.0 },
        items: vec![],
        locked: 9,
        secret: "hush".to_string(),
    };
    let snapshot = instance.snapshot();
    assert!(
        snapshot.get("secret").is_none(),
        "hidden field must be absent from snapshot()"
    );
}

// --- (b) 2-level-nested struct: recursive schema + leaf-only apply_patch ----

#[derive(Inspectable)]
struct Inner {
    c: f32,
}

#[derive(Inspectable)]
struct Middle {
    b: Inner,
    d: i32,
}

#[derive(Inspectable)]
struct Outer {
    a: Middle,
    e: String,
}

#[test]
fn nested_struct_schema_is_recursive_and_apply_patch_mutates_only_the_targeted_leaf() {
    let schema = Outer::schema();
    assert_eq!(schema.tag, FieldTag::Struct);

    let a = schema
        .children
        .iter()
        .find(|c| c.name == "a")
        .expect("outer has child `a`");
    assert_eq!(a.tag, FieldTag::Struct);

    let b = a
        .children
        .iter()
        .find(|c| c.name == "b")
        .expect("middle has child `b`");
    assert_eq!(b.tag, FieldTag::Struct);

    let c = b
        .children
        .iter()
        .find(|c| c.name == "c")
        .expect("inner has child `c`");
    assert_eq!(c.tag, FieldTag::Float);

    let mut outer = Outer {
        a: Middle {
            b: Inner { c: 1.0 },
            d: 2,
        },
        e: "x".to_string(),
    };
    outer
        .apply_patch("a.b.c", serde_json::json!(9.5))
        .expect("apply_patch on a 3-level nested leaf");

    assert_eq!(outer.a.b.c, 9.5);
    assert_eq!(outer.a.d, 2, "sibling field must be untouched");
    assert_eq!(outer.e, "x", "sibling field must be untouched");
}

// --- (c) unit-only enum: Enum tag, variant list, apply-by-name -------------

#[derive(Inspectable, Debug, PartialEq)]
enum Status {
    Idle,
    Active,
}

#[test]
fn unit_only_enum_schema_is_enum_tagged_with_variant_names_and_applies_by_name() {
    let schema = Status::schema();
    assert_eq!(schema.tag, FieldTag::Enum);
    assert_eq!(
        schema.variants,
        vec!["Idle".to_string(), "Active".to_string()]
    );

    let mut s = Status::Idle;
    s.apply_patch("", serde_json::json!("Active"))
        .expect("apply_patch by variant name");
    assert_eq!(s, Status::Active);

    let result = s.apply_patch("", serde_json::json!("NotAVariant"));
    assert!(result.is_err(), "unknown variant name must be Err, not panic");
}

// --- (d) data-carrying enum: Json fallback, always readonly ----------------

#[derive(Inspectable, Serialize)]
enum Payload {
    Empty,
    WithValue(i32),
}

#[test]
fn data_carrying_enum_variant_falls_back_to_readonly_json_tag() {
    let schema = Payload::schema();
    assert_eq!(schema.tag, FieldTag::Json);
    assert!(schema.readonly, "data-carrying enum schema must be readonly");

    let p = Payload::WithValue(7);
    let snapshot = p.snapshot();
    assert!(
        !snapshot.is_null(),
        "Json-fallback snapshot must be the real serialized value, not null"
    );

    let mut p2 = Payload::Empty;
    let result = p2.apply_patch("", serde_json::json!(1));
    assert!(result.is_err(), "Json-fallback apply_patch must always be Err");
}
