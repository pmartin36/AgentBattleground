//! `Inspectable` trait, wire-serializable field schema types, and the shared
//! dotted/indexed patch-path parser used by every nested `apply_patch` impl
//! (derive-generated or hand-written).

use serde::{Deserialize, Serialize};

use crate::color::Rgba;

// ── Schema types (wire-serializable; travel embedded in `CatalogEntry`) ────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldTag {
    Bool,
    Int,
    Float,
    String,
    Enum,
    Color,
    Struct,
    List,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub tag: FieldTag,
    #[serde(default, skip_serializing_if = "is_false")]
    pub readonly: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FieldSchema>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
}

// ── Trait ────────────────────────────────────────────────────────────────────

pub trait Inspectable {
    fn schema() -> FieldSchema
    where
        Self: Sized;
    fn snapshot(&self) -> serde_json::Value;
    fn apply_patch(&mut self, path: &str, value: serde_json::Value) -> Result<(), PatchError>;
}

// ── Patch errors (internal control flow; not serde) ─────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PatchError {
    UnknownField(String),
    NotIndexable,
    IndexOutOfBounds(usize),
    TypeMismatch,
    Readonly,
    BadPath(String),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for PatchError {}

// ── Shared path-segment parser ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment<'a> {
    Field(&'a str),
    Index(usize),
}

/// Peel the leading path segment off `path`, returning it plus the remaining
/// tail (`None` if `path` had only one segment). Never panics; a malformed
/// segment (`"pieces[abc]"`, `"pieces["`) is `Err(PatchError::BadPath)`.
pub fn parse_path_segment(path: &str) -> Result<(Segment<'_>, Option<&str>), PatchError> {
    if path.starts_with('[') {
        return parse_index_segment(path);
    }
    let end = path.find(['.', '[']).unwrap_or(path.len());
    let field = &path[..end];
    let rest = &path[end..];
    if rest.starts_with('[') {
        // Eagerly validate the immediately-following bracket so a malformed
        // index (`"pieces[abc]"`, `"pieces["`) is rejected right here, even
        // though it is not consumed until the NEXT call peels it off.
        validate_index_segment(rest)?;
    }
    let tail = split_tail(rest);
    Ok((Segment::Field(field), tail))
}

/// Parse a leading `"[N]"` segment. `path` must start with `'['`.
fn parse_index_segment(path: &str) -> Result<(Segment<'_>, Option<&str>), PatchError> {
    let close = path
        .find(']')
        .ok_or_else(|| PatchError::BadPath(path.to_string()))?;
    let idx: usize = path[1..close]
        .parse()
        .map_err(|_| PatchError::BadPath(path.to_string()))?;
    let rest = &path[close + 1..];
    Ok((Segment::Index(idx), split_tail(rest)))
}

/// Structural check of a leading `"[N]"` segment without consuming it.
fn validate_index_segment(path: &str) -> Result<(), PatchError> {
    parse_index_segment(path).map(|_| ())
}

/// `""` => no tail; a leading `.` is a separator and is stripped; a leading
/// `[` starts the next segment and is retained.
fn split_tail(rest: &str) -> Option<&str> {
    if rest.is_empty() {
        None
    } else if let Some(stripped) = rest.strip_prefix('.') {
        Some(stripped)
    } else {
        Some(rest)
    }
}

// ── Scalar impls ─────────────────────────────────────────────────────────────

/// Bare, un-named/un-labeled schema for a leaf type — the parent (derive,
/// b1-t2) overlays `name`/`label`/`#[inspect(..)]` attrs.
fn leaf_schema(tag: FieldTag) -> FieldSchema {
    FieldSchema {
        name: String::new(),
        label: None,
        tag,
        readonly: false,
        hidden: false,
        range: None,
        children: vec![],
        variants: vec![],
    }
}

macro_rules! impl_scalar {
    ($t:ty, $tag:ident) => {
        impl Inspectable for $t {
            fn schema() -> FieldSchema
            where
                Self: Sized,
            {
                leaf_schema(FieldTag::$tag)
            }
            fn snapshot(&self) -> serde_json::Value {
                serde_json::to_value(self)
                    .expect(concat!(stringify!($t), " is always serializable"))
            }
            fn apply_patch(
                &mut self,
                path: &str,
                value: serde_json::Value,
            ) -> Result<(), PatchError> {
                if !path.is_empty() {
                    return Err(PatchError::NotIndexable);
                }
                *self = serde_json::from_value(value).map_err(|_| PatchError::TypeMismatch)?;
                Ok(())
            }
        }
    };
}

impl_scalar!(bool, Bool);
impl_scalar!(i8, Int);
impl_scalar!(i16, Int);
impl_scalar!(i32, Int);
impl_scalar!(i64, Int);
impl_scalar!(isize, Int);
impl_scalar!(u8, Int);
impl_scalar!(u16, Int);
impl_scalar!(u32, Int);
impl_scalar!(u64, Int);
impl_scalar!(usize, Int);
impl_scalar!(f32, Float);
impl_scalar!(f64, Float);
impl_scalar!(String, String);

impl Inspectable for Rgba {
    fn schema() -> FieldSchema
    where
        Self: Sized,
    {
        leaf_schema(FieldTag::Color)
    }
    fn snapshot(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("Rgba is always serializable")
    }
    fn apply_patch(&mut self, path: &str, value: serde_json::Value) -> Result<(), PatchError> {
        if !path.is_empty() {
            return Err(PatchError::NotIndexable);
        }
        let hex = value.as_str().ok_or(PatchError::TypeMismatch)?;
        *self = Rgba::from_hex(hex).map_err(|_| PatchError::TypeMismatch)?;
        Ok(())
    }
}

impl<T: Inspectable> Inspectable for Vec<T> {
    fn schema() -> FieldSchema
    where
        Self: Sized,
    {
        FieldSchema {
            children: vec![T::schema()],
            ..leaf_schema(FieldTag::List)
        }
    }
    fn snapshot(&self) -> serde_json::Value {
        serde_json::Value::Array(self.iter().map(Inspectable::snapshot).collect())
    }
    fn apply_patch(&mut self, path: &str, value: serde_json::Value) -> Result<(), PatchError> {
        let (segment, tail) = parse_path_segment(path)?;
        let index = match segment {
            Segment::Index(i) => i,
            Segment::Field(_) => return Err(PatchError::NotIndexable),
        };
        let elem = self
            .get_mut(index)
            .ok_or(PatchError::IndexOutOfBounds(index))?;
        elem.apply_patch(tail.unwrap_or(""), value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- scalar tag matrix ---

    #[test]
    fn bool_schema_tag_is_bool() {
        assert_eq!(bool::schema().tag, FieldTag::Bool);
    }

    #[test]
    fn signed_int_schema_tag_is_int() {
        assert_eq!(i32::schema().tag, FieldTag::Int);
        assert_eq!(i64::schema().tag, FieldTag::Int);
    }

    #[test]
    fn unsigned_int_schema_tag_is_int() {
        assert_eq!(u16::schema().tag, FieldTag::Int);
        assert_eq!(u8::schema().tag, FieldTag::Int);
    }

    #[test]
    fn float_schema_tag_is_float() {
        assert_eq!(f32::schema().tag, FieldTag::Float);
        assert_eq!(f64::schema().tag, FieldTag::Float);
    }

    #[test]
    fn string_schema_tag_is_string() {
        assert_eq!(String::schema().tag, FieldTag::String);
    }

    #[test]
    fn rgba_schema_tag_is_color() {
        assert_eq!(Rgba::schema().tag, FieldTag::Color);
    }

    #[test]
    fn vec_rgba_schema_tag_is_list_with_single_color_template() {
        let schema = <Vec<Rgba> as Inspectable>::schema();
        assert_eq!(schema.tag, FieldTag::List);
        assert_eq!(schema.children.len(), 1);
        assert_eq!(schema.children[0].tag, FieldTag::Color);
    }

    #[test]
    fn vec_u16_schema_tag_is_list_with_single_int_template() {
        let schema = <Vec<u16> as Inspectable>::schema();
        assert_eq!(schema.tag, FieldTag::List);
        assert_eq!(schema.children.len(), 1);
        assert_eq!(schema.children[0].tag, FieldTag::Int);
    }

    // --- snapshot / apply_patch round trips ---

    #[test]
    fn bool_snapshot_apply_patch_round_trip() {
        let mut v = true;
        v.apply_patch("", serde_json::json!(false)).expect("apply_patch on bool");
        assert_eq!(v.snapshot(), serde_json::json!(false));
    }

    #[test]
    fn i32_snapshot_apply_patch_round_trip() {
        let mut v: i32 = 1;
        v.apply_patch("", serde_json::json!(42)).expect("apply_patch on i32");
        assert_eq!(v.snapshot(), serde_json::json!(42));
    }

    #[test]
    fn f64_snapshot_apply_patch_round_trip() {
        let mut v: f64 = 1.0;
        v.apply_patch("", serde_json::json!(3.5)).expect("apply_patch on f64");
        assert_eq!(v.snapshot(), serde_json::json!(3.5));
    }

    #[test]
    fn string_snapshot_apply_patch_round_trip() {
        let mut v = String::from("a");
        v.apply_patch("", serde_json::json!("b"))
            .expect("apply_patch on String");
        assert_eq!(v.snapshot(), serde_json::json!("b"));
    }

    #[test]
    fn rgba_apply_patch_preserves_non_trivial_alpha() {
        let mut c = Rgba::new(0x00, 0x00, 0x00, 0x80);
        c.apply_patch("", serde_json::json!("#112233ff"))
            .expect("apply_patch on Rgba with a valid hex string");
        assert_eq!(c, Rgba::new(0x11, 0x22, 0x33, 0xff));
    }

    // --- error paths: never panic ---

    #[test]
    fn scalar_apply_patch_type_mismatch_is_err_not_panic() {
        let mut v: i32 = 1;
        let result = v.apply_patch("", serde_json::json!("not a number"));
        assert_eq!(result, Err(PatchError::TypeMismatch));
    }

    #[test]
    fn scalar_apply_patch_with_non_empty_path_is_not_indexable() {
        let mut v: i32 = 1;
        let result = v.apply_patch("sub", serde_json::json!(1));
        assert_eq!(result, Err(PatchError::NotIndexable));
    }

    // --- Vec<T> indexed apply_patch ---

    #[test]
    fn vec_apply_patch_mutates_only_targeted_index() {
        let mut v: Vec<i32> = vec![10, 20, 30];
        v.apply_patch("[1]", serde_json::json!(99)).expect("apply_patch on Vec index");
        assert_eq!(v, vec![10, 99, 30]);
    }

    #[test]
    fn vec_apply_patch_out_of_bounds_is_err() {
        let mut v: Vec<i32> = vec![10, 20, 30];
        let result = v.apply_patch("[7]", serde_json::json!(1));
        assert_eq!(result, Err(PatchError::IndexOutOfBounds(7)));
    }

    // --- FieldSchema wire round trip ---

    #[test]
    fn field_schema_round_trips_with_nested_children_range_and_variants() {
        let schema = FieldSchema {
            name: "root".to_string(),
            label: Some("Root Field".to_string()),
            tag: FieldTag::Struct,
            readonly: false,
            hidden: false,
            range: None,
            children: vec![
                FieldSchema {
                    name: "amount".to_string(),
                    label: None,
                    tag: FieldTag::Float,
                    readonly: false,
                    hidden: false,
                    range: Some(Range { min: 0.0, max: 10.0 }),
                    children: vec![],
                    variants: vec![],
                },
                FieldSchema {
                    name: "mode".to_string(),
                    label: None,
                    tag: FieldTag::Enum,
                    readonly: true,
                    hidden: false,
                    range: None,
                    children: vec![],
                    variants: vec!["A".to_string(), "B".to_string()],
                },
            ],
            variants: vec![],
        };

        let json = serde_json::to_string(&schema).expect("serialize FieldSchema");
        let decoded: FieldSchema = serde_json::from_str(&json).expect("deserialize FieldSchema");
        assert_eq!(decoded, schema);
    }

    // --- parse_path_segment ---

    #[test]
    fn parse_path_segment_splits_field_chain() {
        let (seg, tail) = parse_path_segment("transform.translate.x").expect("valid path");
        assert_eq!(seg, Segment::Field("transform"));
        assert_eq!(tail, Some("translate.x"));

        let (seg, tail) = parse_path_segment(tail.unwrap()).expect("valid path");
        assert_eq!(seg, Segment::Field("translate"));
        assert_eq!(tail, Some("x"));

        let (seg, tail) = parse_path_segment(tail.unwrap()).expect("valid path");
        assert_eq!(seg, Segment::Field("x"));
        assert_eq!(tail, None);
    }

    #[test]
    fn parse_path_segment_splits_list_index_then_field_tail() {
        let (seg, tail) = parse_path_segment("pieces[3].color").expect("valid path");
        assert_eq!(seg, Segment::Field("pieces"));
        assert_eq!(tail, Some("[3].color"));

        let (seg, tail) = parse_path_segment(tail.unwrap()).expect("valid path");
        assert_eq!(seg, Segment::Index(3));
        assert_eq!(tail, Some("color"));
    }

    #[test]
    fn parse_path_segment_malformed_index_is_bad_path_not_panic() {
        assert!(matches!(
            parse_path_segment("pieces[abc]"),
            Err(PatchError::BadPath(_))
        ));
        assert!(matches!(
            parse_path_segment("pieces["),
            Err(PatchError::BadPath(_))
        ));
    }
}
