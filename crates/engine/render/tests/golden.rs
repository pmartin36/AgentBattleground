//! Golden tests for `engine_render::convert` — Tier-1 correctness gate.
//!
//! Cases (a)–(e) use hand-derived constants that are **independent** of the
//! implementation. A wrong luma threshold, wrong DOTS ordering, wrong mean
//! computation, or wrong aspect-fit will fail them.
//!
//! The snapshot test (`golden_snapshot_fixture_grid_regression`) is regression-
//! only — it pins the deterministic text serialisation of the PNG fixture.
//! Generate / update it by running tests with `UPDATE_SNAPSHOTS=1`.

use image::{DynamicImage, Rgba as PixelRgba, RgbaImage};
use engine_render::{convert, Cell};
use ratatui::layout::Rect;
use engine_core::color::Rgba;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Build a uniform RGBA image where every pixel is (r, g, b, a).
fn solid_image(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
    let mut raw = RgbaImage::new(w, h);
    for p in raw.pixels_mut() {
        *p = PixelRgba([r, g, b, a]);
    }
    DynamicImage::from(raw)
}

/// Build a `DynamicImage` from a flat row-major slice of `[r,g,b,a]` values.
/// `pixels[y * w + x]` is the pixel at column `x`, row `y`.
fn image_from_pixels(pixels: &[[u8; 4]], w: u32, h: u32) -> DynamicImage {
    assert_eq!(pixels.len() as u32, w * h, "pixel count must equal w*h");
    let mut raw = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            raw.put_pixel(x, y, PixelRgba(pixels[(y * w + x) as usize]));
        }
    }
    DynamicImage::from(raw)
}

// ── Case (a): exact glyph + fg — the independent oracle ──────────────────────

/// Computes the exact expected glyph and foreground color from a specific 2×4
/// input using the documented algorithm, verified by hand.
///
/// Source: 2×4 (= cols*2 × rows*4 so Lanczos resize is identity).
/// Area: Rect::new(0,0,1,1) → grid 1×1.
///
/// DOTS order: (dx,dy,bit) = (0,0,0),(0,1,1),(0,2,2),(1,0,3),(1,1,4),(1,2,5),(0,3,6),(1,3,7)
/// Pixel RGBA at each dot position (image w=2, h=4):
///   bit 0 (0,0): [10, 20, 30,255]  luma = trunc(0.299*10+0.587*20+0.114*30) = trunc(18.15) = 18
///   bit 1 (0,1): [40, 50, 60,255]  luma = 48
///   bit 2 (0,2): [70, 80, 90,255]  luma = 78
///   bit 3 (1,0): [200,210,220,255] luma = 208
///   bit 4 (1,1): [15, 25, 35,255]  luma = 23
///   bit 5 (1,2): [45, 55, 65,255]  luma = 53
///   bit 6 (0,3): [100,110,120,255] luma = 108
///   bit 7 (1,3): [250,240,230,255] luma = 241
/// avg = 777/8 = 97 (integer division)
/// Lit dots (luma >= 97): bits 3,6,7 → mask = 0x08|0x40|0x80 = 0xC8 → U+28C8 = '⣈'
/// fg = mean RGB of all 8 (all opaque): R=730/8=91, G=790/8=98, B=850/8=106
#[test]
fn golden_case_a_exact_glyph_and_fg() {
    // Row-major layout: pixel[y*2+x]
    let pixels: &[[u8; 4]] = &[
        [10,  20,  30,  255], // (x=0,y=0) — dot bit 0
        [200, 210, 220, 255], // (x=1,y=0) — dot bit 3
        [40,  50,  60,  255], // (x=0,y=1) — dot bit 1
        [15,  25,  35,  255], // (x=1,y=1) — dot bit 4
        [70,  80,  90,  255], // (x=0,y=2) — dot bit 2
        [45,  55,  65,  255], // (x=1,y=2) — dot bit 5
        [100, 110, 120, 255], // (x=0,y=3) — dot bit 6
        [250, 240, 230, 255], // (x=1,y=3) — dot bit 7
    ];
    let img = image_from_pixels(pixels, 2, 4);
    let area = Rect::new(0, 0, 1, 1);
    let grid = convert(&img, area);

    assert_eq!(grid.cols(), 1, "case (a): expected 1 col");
    assert_eq!(grid.rows(), 1, "case (a): expected 1 row");

    assert_eq!(
        grid.get(0, 0),
        Cell::Glyph {
            ch: '\u{28C8}', // ⣈ — bits 3,6,7
            color: Rgba::rgb(91, 98, 106),
        },
        "case (a): glyph+fg must match hand-derived oracle"
    );
}

// ── Case (b): solid opaque → all '⣿' ─────────────────────────────────────────

/// Gate completeness: solid opaque single-color source → every cell '⣿'.
/// Corroborates the existing unit test in convert.rs; kept minimal here.
#[test]
fn golden_case_b_solid_opaque_full_glyph() {
    let (r, g, b) = (200u8, 100u8, 50u8);
    let img = solid_image(16, 16, r, g, b, 255);
    let area = Rect::new(0, 0, 8, 4);
    let grid = convert(&img, area);

    assert!(grid.cols() > 0 && grid.rows() > 0, "case (b): grid must be non-empty");
    for row in 0..grid.rows() {
        for col in 0..grid.cols() {
            assert_eq!(
                grid.get(col, row),
                Cell::Glyph { ch: '⣿', color: Rgba::rgb(r, g, b) },
                "case (b): cell ({col},{row}) must be fully-lit glyph in source color"
            );
        }
    }
}

// ── Case (c): fully transparent → all Cell::Transparent ──────────────────────

/// Gate completeness: every pixel alpha=0 → every cell Transparent.
/// Corroborates the existing unit test in convert.rs; kept minimal here.
#[test]
fn golden_case_c_fully_transparent() {
    let img = solid_image(16, 16, 255, 255, 255, 0);
    let area = Rect::new(0, 0, 8, 4);
    let grid = convert(&img, area);

    assert!(grid.cols() > 0 && grid.rows() > 0, "case (c): grid must be non-empty");
    for row in 0..grid.rows() {
        for col in 0..grid.cols() {
            assert_eq!(
                grid.get(col, row),
                Cell::Transparent,
                "case (c): cell ({col},{row}) must be Transparent for fully transparent source"
            );
        }
    }
}

// ── Case (d1): per-cell transparent split ─────────────────────────────────────

/// Source 4×4: x<2 = opaque green [0,200,0,255], x>=2 = transparent [0,0,0,0].
/// Area Rect::new(0,0,2,1) → grid 2×1 (source = cols*2 × rows*4, resize is identity).
/// Left cell (tx=0): all 8 dots opaque, uniform luma → mask=0xFF → '⣿', fg green.
/// Right cell (tx=1): all 8 dots transparent → Cell::Transparent.
#[test]
fn golden_case_d1_per_cell_transparent_split() {
    let mut raw = RgbaImage::new(4, 4);
    for y in 0..4 {
        for x in 0..4 {
            raw.put_pixel(
                x, y,
                if x < 2 {
                    PixelRgba([0, 200, 0, 255])
                } else {
                    PixelRgba([0, 0, 0, 0])
                },
            );
        }
    }
    let img = DynamicImage::from(raw);
    let area = Rect::new(0, 0, 2, 1);
    let grid = convert(&img, area);

    assert_eq!(grid.cols(), 2, "d1: expected 2 cols");
    assert_eq!(grid.rows(), 1, "d1: expected 1 row");
    assert_eq!(
        grid.get(0, 0),
        Cell::Glyph { ch: '⣿', color: Rgba::rgb(0, 200, 0) },
        "d1: left cell (all opaque green) must be fully-lit glyph"
    );
    assert_eq!(
        grid.get(1, 0),
        Cell::Transparent,
        "d1: right cell (all transparent) must be Cell::Transparent"
    );
}

// ── Case (d2): intra-cell dot-column split ────────────────────────────────────

/// Source 2×4: x=0 column = opaque blue [0,0,200,255], x=1 column = transparent.
/// Area Rect::new(0,0,1,1) → grid 1×1 (source = cols*2 × rows*4, resize is identity).
///
/// DOTS with dx=0: bits 0,1,2,6  (visible, luma = trunc(0.114*200) = 22)
/// DOTS with dx=1: bits 3,4,5,7  (transparent → invisible)
/// avg luma of visible = (22+22+22+22)/4 = 22
/// All visible lumas >= 22 → all lit
/// mask = (1<<0)|(1<<1)|(1<<2)|(1<<6) = 0x01|0x02|0x04|0x40 = 0x47 → U+2847 = '⡇'
/// fg = mean RGB of 4 identical [0,0,200] dots = Rgba::rgb(0,0,200)
#[test]
fn golden_case_d2_intracell_dot_split() {
    // Row-major: pixel[y*2+x]
    let pixels: &[[u8; 4]] = &[
        [0, 0, 200, 255], // (x=0,y=0) — dot bit 0
        [0, 0,   0,   0], // (x=1,y=0) — dot bit 3
        [0, 0, 200, 255], // (x=0,y=1) — dot bit 1
        [0, 0,   0,   0], // (x=1,y=1) — dot bit 4
        [0, 0, 200, 255], // (x=0,y=2) — dot bit 2
        [0, 0,   0,   0], // (x=1,y=2) — dot bit 5
        [0, 0, 200, 255], // (x=0,y=3) — dot bit 6
        [0, 0,   0,   0], // (x=1,y=3) — dot bit 7
    ];
    let img = image_from_pixels(pixels, 2, 4);
    let area = Rect::new(0, 0, 1, 1);
    let grid = convert(&img, area);

    assert_eq!(grid.cols(), 1, "d2: expected 1 col");
    assert_eq!(grid.rows(), 1, "d2: expected 1 row");
    assert_eq!(
        grid.get(0, 0),
        Cell::Glyph {
            ch: '\u{2847}', // ⡇ — bits 0,1,2,6
            color: Rgba::rgb(0, 0, 200),
        },
        "d2: intra-cell dot split must produce mask=0x47 glyph in blue"
    );
}

// ── Case (e): aspect-preserving fit dimensions ────────────────────────────────

/// (e1) 16×8 (aspect=2.0) source into Rect::new(0,0,10,10).
/// Width-first: cols=10, rows=round(10/(2*2.0))=round(2.5)=3.
/// rows(3) <= height(10) → no refit.
#[test]
fn golden_case_e1_aspect_fit_wide_source() {
    let img = solid_image(16, 8, 100, 150, 200, 255);
    let area = Rect::new(0, 0, 10, 10);
    let grid = convert(&img, area);
    assert_eq!(grid.cols(), 10, "e1: 2:1 source, expected cols=10");
    assert_eq!(grid.rows(), 3,  "e1: 2:1 source, expected rows=3 (round(2.5)=3)");
}

/// (e2) 4×16 (aspect=0.25) source into Rect::new(0,0,8,4).
/// Width-first: cols=8, rows=round(8/(2*0.25))=round(16)=16 > area.height(4).
/// Height-first refit: rows=4, cols=round(4*2*0.25)=round(2.0)=2.
#[test]
fn golden_case_e2_aspect_fit_tall_source_height_limited() {
    let img = solid_image(4, 16, 100, 150, 200, 255);
    let area = Rect::new(0, 0, 8, 4);
    let grid = convert(&img, area);
    assert_eq!(grid.cols(), 2, "e2: 1:4 source height-limited, expected cols=2");
    assert_eq!(grid.rows(), 4, "e2: 1:4 source height-limited, expected rows=4");
}

// ── Snapshot: regression-only ─────────────────────────────────────────────────

/// Convert the committed PNG fixture and compare to a pinned text dump.
///
/// Format: `dims C R` header followed by one line per non-transparent cell:
/// `row col U+XXXX rrggbb` (row-major order).
///
/// To generate or update the snapshot, run with `UPDATE_SNAPSHOTS=1`:
///   UPDATE_SNAPSHOTS=1 cargo test -p render --test golden golden_snapshot
#[test]
fn golden_snapshot_fixture_grid_regression() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sprite.png");
    let snapshot_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sprite.grid.txt");

    let img = image::open(fixture_path).expect("sprite.png must exist (run b2-t1 first)");
    let area = Rect::new(0, 0, 16, 16);
    let grid = convert(&img, area);

    // Deterministic text serialisation: both glyph and RGB so color regressions are caught.
    let mut lines = Vec::new();
    lines.push(format!("dims {} {}", grid.cols(), grid.rows()));
    for row in 0..grid.rows() {
        for col in 0..grid.cols() {
            if let Cell::Glyph { ch, color } = grid.get(col, row) {
                lines.push(format!(
                    "{} {} U+{:04X} {:02x}{:02x}{:02x}",
                    row, col,
                    ch as u32,
                    color.r, color.g, color.b
                ));
            }
        }
    }
    let actual = lines.join("\n");

    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        std::fs::write(snapshot_path, &actual).expect("failed to write sprite.grid.txt");
        return; // Snapshot written — test passes.
    }

    let expected = std::fs::read_to_string(snapshot_path)
        .expect(
            "sprite.grid.txt not found — generate it with: \
             UPDATE_SNAPSHOTS=1 cargo test -p render --test golden golden_snapshot"
        );

    assert_eq!(
        actual, expected,
        "convert output for sprite.png changed — if intentional, \
         regenerate with UPDATE_SNAPSHOTS=1"
    );
}
