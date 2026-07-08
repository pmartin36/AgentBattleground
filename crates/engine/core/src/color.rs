/// Canonical RGBA color type for scene-core.
/// Wire-encoded as the 9-character string `"#rrggbbaa"` (lowercase hex).

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Returned when a hex string cannot be parsed as `Rgba`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRgbaError;

impl std::fmt::Display for ParseRgbaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid rgba hex color: expected \"#rrggbbaa\"")
    }
}

impl std::error::Error for ParseRgbaError {}

/// Parse two lowercase hex characters into a u8. Returns None on failure.
fn parse_hex_byte(hi: u8, lo: u8) -> Option<u8> {
    let h = hex_digit(hi)?;
    let l = hex_digit(lo)?;
    Some((h << 4) | l)
}

/// Parse a single ASCII hex digit (case-insensitive) into 0..=15.
fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }

    /// Opaque shorthand: alpha is set to `0xFF`.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 0xFF }
    }

    /// Returns `"#rrggbbaa"` with lowercase hex digits.
    pub fn to_hex(self) -> String {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            self.r, self.g, self.b, self.a
        )
    }

    /// Parses `"#rrggbbaa"` (case-insensitive hex digits).
    /// Returns `Err(ParseRgbaError)` for any other input.
    pub fn from_hex(s: &str) -> Result<Self, ParseRgbaError> {
        let bytes = s.as_bytes();
        // Must be exactly 9 bytes: '#' + 8 hex digits
        if bytes.len() != 9 || bytes[0] != b'#' {
            return Err(ParseRgbaError);
        }
        let r = parse_hex_byte(bytes[1], bytes[2]).ok_or(ParseRgbaError)?;
        let g = parse_hex_byte(bytes[3], bytes[4]).ok_or(ParseRgbaError)?;
        let b = parse_hex_byte(bytes[5], bytes[6]).ok_or(ParseRgbaError)?;
        let a = parse_hex_byte(bytes[7], bytes[8]).ok_or(ParseRgbaError)?;
        Ok(Rgba { r, g, b, a })
    }
}

impl Rgba {
    /// Per-channel (r,g,b,a) linear interpolation from `self` (t=0) toward
    /// `other` (t=1). `t` is clamped to `[0.0, 1.0]` before use. Each channel
    /// is rounded to the nearest `u8` (b2-t1: drives the roster name's
    /// fade-toward-background colour cross-fade).
    ///
    pub fn lerp(self, other: Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        let ch = |a: u8, b: u8| -> u8 {
            (a as f32 + (b as f32 - a as f32) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Rgba {
            r: ch(self.r, other.r),
            g: ch(self.g, other.g),
            b: ch(self.b, other.b),
            a: ch(self.a, other.a),
        }
    }

    /// Alpha-composite `self` (source) over `dest` (destination):
    /// `out = lerp(dest, self, self.a / 255.0)` per channel (r,g,b,a).
    /// `self.a == 0xFF` returns `self` byte-identical; `self.a == 0x00`
    /// returns `dest` unchanged. Rounding/clamping is inherited from `lerp`.
    pub fn over(self, dest: Rgba) -> Rgba {
        dest.lerp(self, self.a as f32 / 255.0)
    }
}

impl std::str::FromStr for Rgba {
    type Err = ParseRgbaError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Rgba::from_hex(s)
    }
}

impl std::fmt::Display for Rgba {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl serde::Serialize for Rgba {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> serde::Deserialize<'de> for Rgba {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str as serde::Deserialize>::deserialize(deserializer)?;
        Rgba::from_hex(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- construction ---

    #[test]
    fn new_sets_all_fields() {
        let c = Rgba::new(0xc8, 0x1e, 0x1e, 0xff);
        assert_eq!((c.r, c.g, c.b, c.a), (0xc8, 0x1e, 0x1e, 0xff));
    }

    #[test]
    fn rgb_constructor_sets_opaque_alpha() {
        let c = Rgba::rgb(0x10, 0x20, 0x30);
        assert_eq!(c.a, 0xFF);
        assert_eq!((c.r, c.g, c.b), (0x10, 0x20, 0x30));
    }

    // --- from_hex / to_hex round-trip ---

    #[test]
    fn from_hex_parses_non_trivial_value_with_alpha() {
        let rgba = Rgba::from_hex("#c81e1eff").expect("#c81e1eff should parse");
        assert_eq!(rgba, Rgba { r: 0xc8, g: 0x1e, b: 0x1e, a: 0xff });
    }

    #[test]
    fn to_hex_produces_lowercase_9char_string() {
        let rgba = Rgba::new(0xc8, 0x1e, 0x1e, 0xff);
        assert_eq!(rgba.to_hex(), "#c81e1eff");
    }

    #[test]
    fn from_hex_to_hex_round_trip() {
        let hex = "#c81e1eff";
        let rgba = Rgba::from_hex(hex).expect("valid hex");
        assert_eq!(rgba.to_hex(), hex);
    }

    #[test]
    fn alpha_preserved_distinctly() {
        let rgba = Rgba::from_hex("#00000080").expect("valid hex");
        assert_eq!((rgba.r, rgba.g, rgba.b, rgba.a), (0x00, 0x00, 0x00, 0x80));
    }

    // --- case-insensitive parse, lowercase output ---

    #[test]
    fn case_insensitive_parse_upper() {
        let lower = Rgba::from_hex("#c81e1eff").expect("lowercase valid");
        let upper = Rgba::from_hex("#C81E1EFF").expect("uppercase should also parse");
        assert_eq!(lower, upper);
    }

    #[test]
    fn to_hex_always_lowercase_even_after_upper_parse() {
        let rgba = Rgba::from_hex("#C81E1EFF").expect("uppercase valid");
        assert_eq!(rgba.to_hex(), "#c81e1eff");
    }

    // --- FromStr / Display ---

    #[test]
    fn from_str_parses_same_as_from_hex() {
        let via_parse: Rgba = "#c81e1eff".parse().expect("FromStr");
        let via_method = Rgba::from_hex("#c81e1eff").expect("from_hex");
        assert_eq!(via_parse, via_method);
    }

    #[test]
    fn display_emits_lowercase_hex_string() {
        let rgba = Rgba::new(0xc8, 0x1e, 0x1e, 0xff);
        assert_eq!(rgba.to_string(), "#c81e1eff");
    }

    // --- serde: wire form must be a JSON string, not an object ---

    #[test]
    fn serde_serializes_as_quoted_hex_string() {
        let rgba = Rgba::new(0xc8, 0x1e, 0x1e, 0xff);
        let json = serde_json::to_string(&rgba).expect("serialize");
        // Must be `"#c81e1eff"`, NOT `{"r":200,"g":30,"b":30,"a":255}`
        assert_eq!(json, "\"#c81e1eff\"");
    }

    #[test]
    fn serde_round_trip_via_json() {
        let original = Rgba::new(0xc8, 0x1e, 0x1e, 0xff);
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: Rgba = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, decoded);
    }

    #[test]
    fn serde_deserialize_from_hex_string() {
        let decoded: Rgba =
            serde_json::from_str("\"#c81e1eff\"").expect("deserialize from literal");
        assert_eq!(decoded, Rgba::new(0xc8, 0x1e, 0x1e, 0xff));
    }

    // --- malformed inputs: all must return Err, never panic ---

    #[test]
    fn reject_missing_hash_prefix() {
        assert!(Rgba::from_hex("c81e1eff").is_err(), "no # prefix");
    }

    #[test]
    fn reject_6_digit_hex_without_alpha() {
        assert!(Rgba::from_hex("#c81e1e").is_err(), "6 digits rejected");
    }

    #[test]
    fn reject_7_digit_hex() {
        assert!(Rgba::from_hex("#c81e1ef").is_err(), "7 digits rejected");
    }

    #[test]
    fn reject_9_digit_hex() {
        assert!(Rgba::from_hex("#c81e1efff").is_err(), "9 digits rejected");
    }

    #[test]
    fn reject_non_hex_chars() {
        assert!(Rgba::from_hex("#gggggggg").is_err(), "non-hex chars rejected");
    }

    #[test]
    fn reject_empty_string() {
        assert!(Rgba::from_hex("").is_err(), "empty string rejected");
    }

    #[test]
    fn reject_serde_malformed_string() {
        let result = serde_json::from_str::<Rgba>("\"not-a-color\"");
        assert!(result.is_err(), "serde must reject malformed hex");
    }

    // --- lerp (b2-t1: roster name fade-toward-background) ---

    #[test]
    fn lerp_at_t0_returns_self() {
        let a = Rgba::new(0xc8, 0x1e, 0x1e, 0xff);
        let b = Rgba::new(0x00, 0x00, 0x00, 0xff);
        assert_eq!(a.lerp(b, 0.0), a, "t=0 must return the first colour unchanged");
    }

    #[test]
    fn lerp_at_t1_returns_other() {
        let a = Rgba::new(0xc8, 0x1e, 0x1e, 0xff);
        let b = Rgba::new(0x00, 0x00, 0x00, 0xff);
        assert_eq!(a.lerp(b, 1.0), b, "t=1 must return the second colour unchanged");
    }

    #[test]
    fn lerp_at_midpoint_averages_channels() {
        let a = Rgba::new(0xff, 0xff, 0xff, 0xff);
        let b = Rgba::new(0x00, 0x00, 0x00, 0xff);
        let mid = a.lerp(b, 0.5);
        assert_eq!(
            (mid.r, mid.g, mid.b),
            (0x80, 0x80, 0x80),
            "midpoint of white->black must be the per-channel rounded average"
        );
    }

    #[test]
    fn lerp_clamps_t_outside_unit_range() {
        let a = Rgba::new(0xc8, 0x1e, 0x1e, 0xff);
        let b = Rgba::new(0x00, 0x00, 0x00, 0xff);
        assert_eq!(a.lerp(b, -1.0), a, "t below 0 must clamp to t=0 (return self)");
        assert_eq!(a.lerp(b, 2.0), b, "t above 1 must clamp to t=1 (return other)");
    }

    // --- over (alpha blend, b1-t1: shared compositing primitive) ---

    #[test]
    fn over_opaque_source_returns_source_byte_identical() {
        let src = Rgba::new(0x12, 0x34, 0x56, 0xFF);
        let dest = Rgba::new(0x99, 0x88, 0x77, 0x66);
        assert_eq!(src.over(dest), src, "fully opaque source must pass through unchanged");
    }

    #[test]
    fn over_transparent_source_returns_dest_unchanged() {
        let src = Rgba::new(0x12, 0x34, 0x56, 0x00);
        let dest = Rgba::new(0x99, 0x88, 0x77, 0x66);
        assert_eq!(src.over(dest), dest, "fully transparent source must leave dest unchanged");
    }

    #[test]
    fn over_mid_alpha_is_exact_per_channel_lerp() {
        let src = Rgba::new(0xFF, 0xFF, 0xFF, 0x80);
        let dest = Rgba::new(0x00, 0x00, 0x00, 0xFF);
        let out = src.over(dest);
        assert_eq!((out.r, out.g, out.b), (0x80, 0x80, 0x80), "rgb channels must lerp toward src by src.a/255");
        assert_eq!(out.a, 0xBF, "alpha channel is itself lerped, not forced to src.a or dest.a");
    }

    #[test]
    fn over_mid_alpha_asymmetric_channels() {
        let src = Rgba::new(0xC0, 0x40, 0x00, 0x40);
        let dest = Rgba::new(0x00, 0x40, 0xC0, 0xFF);
        let out = src.over(dest);
        // t = 0x40 / 255 = 64/255
        // r: 0 + (192-0)*64/255 = 48.19... -> 48 (0x30)
        // g: 64 + (64-64)*64/255 = 64 -> 64 (0x40, unchanged since src.g == dest.g)
        // b: 192 + (0-192)*64/255 = 192 - 48.19 = 143.81 -> 144 (0x90)
        assert_eq!((out.r, out.g, out.b), (0x30, 0x40, 0x90), "each channel blends independently, not a single averaged value");
    }
}
