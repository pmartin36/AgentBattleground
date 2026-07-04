//! Process-lifetime decode cache (layer 1 of the two-layer asset-rasterization
//! cache; the rasterize layer is added in b1-t2 in this same module).
//!
//! [`decoded`] returns the one shared decoded [`DynamicImage`] for a given
//! `'static` byte slice, decoding via `image::load_from_memory` only on the
//! first call for a given `bytes.as_ptr()` and returning a cheap `Arc` clone
//! on every subsequent call for the rest of the process's life. Never
//! evicted (bounded, fixed key space -- bundled assets + creature GIFs).
//!
//! Load-bearing safety property: the cache key is ALWAYS `bytes.as_ptr() as
//! usize` -- the source bytes' own address -- and is NEVER derived from the
//! decoded image's address. A `'static` slice lives in rodata/bss for the
//! whole process and is never freed, so its pointer has no reuse hazard; a
//! decoded image's heap allocation can be freed and its address reused for
//! unrelated data, which is why keying on it would be unsound.

use image::DynamicImage;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::dots::DotBuffer;
use crate::grid::Grid;
use crate::transform::Transform;
use ratatui::layout::Rect;

/// Process-lifetime decode cache: `'static` source bytes' pointer -> the one
/// shared decoded image for those bytes. Never evicted (bounded, fixed key
/// space -- bundled assets + creature GIFs). Key is ALWAYS `bytes.as_ptr()`,
/// NEVER a decoded image's address (the load-bearing safety property).
static DECODE_CACHE: OnceLock<Mutex<HashMap<usize, Arc<DynamicImage>>>> = OnceLock::new();

/// Count of real `image::load_from_memory` decodes performed by `decoded`.
/// Observability for the cache-hit regression tests; sampled as a delta
/// (before/after), never as an absolute, because the cache + counter are
/// process-global and shared across concurrently-running tests.
static DECODE_RECOMPUTES: AtomicU64 = AtomicU64::new(0);

fn store() -> &'static Mutex<HashMap<usize, Arc<DynamicImage>>> {
    DECODE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Return the one shared decoded image for `bytes`, decoding only on the
/// first call for a given `bytes.as_ptr()`. Panics only if `bytes` is not
/// decodable (callers pass first-party bundled `'static` assets --
/// invariant).
pub fn decoded(bytes: &'static [u8]) -> Arc<DynamicImage> {
    let key = bytes.as_ptr() as usize; // <-- source-bytes pointer, the only safe identity
    let mut map = store().lock().expect("decode cache mutex poisoned");
    if let Some(img) = map.get(&key) {
        return Arc::clone(img);
    }
    let img = Arc::new(
        image::load_from_memory(bytes).expect("bundled first-party asset must decode"),
    );
    DECODE_RECOMPUTES.fetch_add(1, Ordering::Relaxed);
    map.insert(key, Arc::clone(&img));
    img
}

/// Total real `image::load_from_memory` decodes performed so far by
/// [`decoded`]. Test-only observability, but compiled unconditionally and
/// `pub` so downstream crates' tests can sample it as a delta (before/after)
/// -- never as an absolute value, since the cache and counter are
/// process-global and shared across concurrently-running tests.
pub fn decode_recompute_count() -> u64 {
    DECODE_RECOMPUTES.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Layer 2: rasterize cache (b1-t2). Keyed on the SAME `bytes.as_ptr()` source
// identity as layer 1, plus frame_index + a Plain(dims)|Transform(fields)
// variant. Never evicted, same process-lifetime scope as layer 1.
// ---------------------------------------------------------------------------

/// Which rasterization primitive a [`RasterKey`] was computed for -- mirrors
/// `anim.rs`'s existing per-instance `PlainKey`/`TransformKey` field sets.
/// `translate` is deliberately excluded from `Transform` (placement-only,
/// never affects the rasterized pixels); floats are stored as `to_bits()` for
/// total `Eq`/`Hash`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum RasterVariant {
    Plain {
        dot_cols: u32,
        dot_rows: u32,
    },
    Transform {
        base_dot_rows: u32,
        rotation_bits: u32,
        scale_x_bits: u32,
        scale_y_bits: u32,
    },
}

/// Rasterize-cache key. `bytes_ptr` is ALWAYS `bytes.as_ptr() as usize` of the
/// `&'static` source bytes -- the same load-bearing safety property as
/// layer 1's `DECODE_CACHE` key (see module doc comment).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct RasterKey {
    bytes_ptr: usize,
    frame_index: usize,
    variant: RasterVariant,
}

/// Process-lifetime rasterize cache: `RasterKey` -> the one shared rasterized
/// `DotBuffer` for that key. Never evicted.
static RASTER_CACHE: OnceLock<Mutex<HashMap<RasterKey, DotBuffer>>> = OnceLock::new();

/// Count of real rasterizations performed by `get_or_compute` (i.e. cache
/// misses). Sampled as a delta, same convention as `DECODE_RECOMPUTES`.
static RASTER_RECOMPUTES: AtomicU64 = AtomicU64::new(0);

fn raster_store() -> &'static Mutex<HashMap<RasterKey, DotBuffer>> {
    RASTER_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Generic get-or-compute core: on a hit, clone the stored `DotBuffer`; on a
/// miss, run `compute`, bump `RASTER_RECOMPUTES`, store, and return it.
fn get_or_compute(key: RasterKey, compute: impl FnOnce() -> DotBuffer) -> DotBuffer {
    let mut map = raster_store().lock().expect("rasterize cache mutex poisoned");
    if let Some(buf) = map.get(&key) {
        return buf.clone();
    }
    let buf = compute();
    RASTER_RECOMPUTES.fetch_add(1, Ordering::Relaxed);
    map.insert(key, buf.clone());
    buf
}

/// Plain (non-transform) rasterize-cache accessor. What `AnimatedSprite`
/// (b2), which owns its own pre-decoded GIF frames, wires into directly --
/// bypasses layer-1 decode entirely, sharing only this layer-2 storage/keying
/// with every other caller.
pub fn plain_cached(
    bytes: &'static [u8],
    frame_index: usize,
    dot_cols: u32,
    dot_rows: u32,
    compute: impl FnOnce() -> DotBuffer,
) -> DotBuffer {
    let key = RasterKey {
        bytes_ptr: bytes.as_ptr() as usize, // <-- source-bytes pointer, the only safe identity
        frame_index,
        variant: RasterVariant::Plain { dot_cols, dot_rows },
    };
    get_or_compute(key, compute)
}

/// Transform rasterize-cache accessor (mirrors `plain_cached` for the
/// rotate/scale path). `transform.translate` never participates in the key
/// (placement-only).
pub fn transform_cached(
    bytes: &'static [u8],
    frame_index: usize,
    transform: &Transform,
    base_dot_rows: u32,
    compute: impl FnOnce() -> DotBuffer,
) -> DotBuffer {
    let key = RasterKey {
        bytes_ptr: bytes.as_ptr() as usize, // <-- source-bytes pointer, the only safe identity
        frame_index,
        variant: RasterVariant::Transform {
            base_dot_rows,
            rotation_bits: transform.rotation.to_bits(),
            scale_x_bits: transform.scale.x.to_bits(),
            scale_y_bits: transform.scale.y.to_bits(),
        },
    };
    get_or_compute(key, compute)
}

/// Cached drop-in for `crate::dots::sprite_to_dots(&decoded(bytes), dot_cols,
/// dot_rows)`. Output is byte-identical to calling the uncached primitive
/// directly on `decoded(bytes)`; a repeat call with identical `(bytes,
/// dot_cols, dot_rows)` is a rasterize-cache hit (no additional recompute).
pub fn sprite_to_dots(bytes: &'static [u8], dot_cols: u32, dot_rows: u32) -> DotBuffer {
    let img = decoded(bytes); // layer 1 (cheap on a hit); lock released before raster lock
    plain_cached(bytes, 0, dot_cols, dot_rows, move || {
        crate::dots::sprite_to_dots(&img, dot_cols, dot_rows)
    })
}

/// Cached drop-in for `crate::convert::convert(&decoded(bytes), area)`.
/// Output is byte-identical to the uncached primitive, including the
/// `Grid::new(0, 0)` zero-area short-circuit. Only the rasterization
/// (`sprite_to_dots`) is cached -- the cheap `fit_dot_dims` + `dots_to_grid`
/// steps are recomputed every call, per the blueprint.
pub fn convert(bytes: &'static [u8], area: Rect) -> Grid {
    let img = decoded(bytes);
    let (cols, rows) = crate::convert::fit_dot_dims(&img, area); // cheap, uncached (per blueprint)
    if cols == 0 || rows == 0 {
        return Grid::new(0, 0); // mirror convert::convert; never touches the raster cache
    }
    let buf = plain_cached(bytes, 0, cols * 2, rows * 4, move || {
        crate::dots::sprite_to_dots(&img, cols * 2, rows * 4)
    });
    crate::dots::dots_to_grid(&buf) // cheap, recomputed per call
}

/// Cached drop-in for `crate::transform::rasterize(&decoded(bytes),
/// transform, base_dot_rows)`. Output is byte-identical to the uncached
/// primitive; a repeat call with an identical transform (including a
/// translate-only change, which never affects the key) is a hit; a
/// rotation/scale/base_dot_rows change recomputes.
pub fn rasterize(bytes: &'static [u8], transform: &Transform, base_dot_rows: u32) -> DotBuffer {
    let img = decoded(bytes);
    transform_cached(bytes, 0, transform, base_dot_rows, move || {
        crate::transform::rasterize(&img, transform, base_dot_rows)
    })
}

/// Total real rasterizations performed so far (cache misses across
/// `plain_cached`/`transform_cached` and the three byte-only entries).
/// Test-only observability; sample as a delta, never an absolute (see
/// `decode_recompute_count`'s doc comment -- same process-global convention).
pub fn rasterize_recompute_count() -> u64 {
    RASTER_RECOMPUTES.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes each test's full before/decode/after sampling window.
    /// `DECODE_RECOMPUTES`/`RASTER_RECOMPUTES` are single process-global
    /// counters shared by every sibling test in this module; libtest runs
    /// `#[test]` fns across multiple OS threads by default, so without this
    /// guard one test's `decoded()`/`sprite_to_dots()`/etc. calls can land
    /// inside another concurrently-running test's `[before, after]` window
    /// and inflate its observed delta. Holding this lock for a test body's
    /// entire measurement window makes the delta reads race-free regardless
    /// of thread scheduling.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire `TEST_LOCK`, tolerating poison. b1-t2's not-yet-implemented
    /// cached entry points deliberately panic (`unimplemented!()`) while
    /// holding this guard, which poisons the `Mutex` for the rest of the
    /// process; a plain `.expect(..)` would then cascade every subsequent
    /// test in this module (including unrelated, already-passing b1-t1
    /// tests) into a false "test lock poisoned" failure. Recovering the
    /// inner guard on poison keeps that isolation intact -- this lock only
    /// ever serializes a counter-sampling window, it protects no invariant
    /// that a partial/aborted critical section could corrupt.
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Encode a small solid-color PNG and leak it to `'static`. Each call
    /// allocates a fresh `Vec`, so distinct calls always yield distinct
    /// pointers -- required so each test's cache entry is isolated from
    /// every other test's, since the decode cache is process-global.
    fn synthetic_png(w: u32, h: u32, r: u8, g: u8, b: u8) -> &'static [u8] {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([r, g, b, 255]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("synthetic test fixture must encode to PNG");
        Box::leak(buf.into_boxed_slice())
    }

    /// Two calls to `decoded` with the SAME `'static` bytes pointer must
    /// perform exactly one real decode (recompute delta == 1) and return
    /// content-identical images.
    #[test]
    fn repeat_call_same_bytes_is_a_cache_hit() {
        let _guard = test_lock();
        let bytes = synthetic_png(4, 4, 10, 20, 30);
        let before = decode_recompute_count();

        let a = decoded(bytes);
        let b = decoded(bytes);

        let delta = decode_recompute_count() - before;
        assert_eq!(
            delta, 1,
            "second call with the same bytes pointer must not recompute"
        );
        assert_eq!(
            a.to_rgba8().into_raw(),
            b.to_rgba8().into_raw(),
            "cached image must be content-identical across hits"
        );
    }

    /// Two calls to `decoded` with two DIFFERENT `'static` bytes pointers
    /// must each decode independently -- no cross-bleed between distinct
    /// source assets.
    #[test]
    fn distinct_bytes_decode_independently() {
        let _guard = test_lock();
        let bytes_a = synthetic_png(4, 4, 200, 0, 0);
        let bytes_b = synthetic_png(4, 4, 0, 200, 0);
        let before = decode_recompute_count();

        let a = decoded(bytes_a);
        let b = decoded(bytes_b);

        let delta = decode_recompute_count() - before;
        assert_eq!(
            delta, 2,
            "two distinct 'static byte pointers must each decode independently"
        );
        assert_ne!(
            a.to_rgba8().into_raw(),
            b.to_rgba8().into_raw(),
            "distinct source images must not collide in the cache"
        );
    }

    /// Mandated pointer-reuse safety regression (spec Decisions v1): decode
    /// the SAME `'static` bytes TWICE independently, OUTSIDE the cache, via
    /// two separate `image::load_from_memory` calls -- guaranteeing the two
    /// resulting `DynamicImage`s live at two different heap addresses. The
    /// cache, looked up twice by the one shared `bytes` reference, must
    /// still serve ONE shared cached entry: the recompute delta across both
    /// cache lookups must stay at 1, proving the key is derived from
    /// `bytes.as_ptr()` and never from either independently-decoded image's
    /// own address.
    #[test]
    fn cache_keys_off_source_bytes_pointer_not_decoded_image_address() {
        let _guard = test_lock();
        let bytes = synthetic_png(4, 4, 5, 5, 5);

        let independent_a = image::load_from_memory(bytes).expect("must decode");
        let independent_b = image::load_from_memory(bytes).expect("must decode");
        assert_ne!(
            independent_a.as_bytes().as_ptr(),
            independent_b.as_bytes().as_ptr(),
            "sanity check: two independent decodes of the same bytes must land \
             at two different heap addresses"
        );

        let before = decode_recompute_count();
        let _first = decoded(bytes);
        let _second = decoded(bytes);
        let delta = decode_recompute_count() - before;

        assert_eq!(
            delta, 1,
            "cache must key off bytes.as_ptr(), never a decoded image's own \
             address -- two lookups of the same source bytes must collapse to \
             one shared entry regardless of decode order"
        );
    }

    // ── b1-t2: rasterize cache (layer 2) ────────────────────────────────

    /// Two `sprite_to_dots` calls with the SAME `'static` bytes pointer AND
    /// the same `(dot_cols, dot_rows)` must perform exactly one real
    /// rasterization and return byte-identical `DotBuffer`s.
    #[test]
    fn sprite_to_dots_repeat_call_same_bytes_dims_is_a_cache_hit() {
        let _guard = test_lock();
        let bytes = synthetic_png(4, 4, 11, 22, 33);
        let before = rasterize_recompute_count();

        let a = sprite_to_dots(bytes, 8, 16);
        let b = sprite_to_dots(bytes, 8, 16);

        let delta = rasterize_recompute_count() - before;
        assert_eq!(
            delta, 1,
            "second call with identical (bytes, dims) must not recompute"
        );
        assert_eq!(a, b, "cached DotBuffer must be identical across hits");
    }

    /// Changing `dot_cols` or `dot_rows` (with the same bytes) must force a
    /// fresh recompute each time -- three distinct dims combos, three
    /// recomputes.
    #[test]
    fn sprite_to_dots_dims_change_forces_recompute() {
        let _guard = test_lock();
        let bytes = synthetic_png(4, 4, 44, 55, 66);
        let before = rasterize_recompute_count();

        let _a = sprite_to_dots(bytes, 8, 16);
        let _b = sprite_to_dots(bytes, 9, 16);
        let _c = sprite_to_dots(bytes, 9, 17);

        let delta = rasterize_recompute_count() - before;
        assert_eq!(
            delta, 3,
            "each distinct (dot_cols, dot_rows) combo must recompute independently"
        );
    }

    /// `plain_cached`'s `frame_index` must participate in the key: frame 0
    /// and frame 1 (same bytes/dims) are distinct entries, but a repeat
    /// lookup of frame 0 after frame 1 was computed must still be a hit.
    #[test]
    fn plain_cached_frame_index_differentiates_entries() {
        let _guard = test_lock();
        let bytes = synthetic_png(4, 4, 77, 88, 99);
        let before = rasterize_recompute_count();

        let a = plain_cached(bytes, 0, 8, 16, || DotBuffer::new(8, 16));
        let _b = plain_cached(bytes, 1, 8, 16, || DotBuffer::new(8, 16));
        let a_again = plain_cached(bytes, 0, 8, 16, || DotBuffer::new(8, 16));

        let delta = rasterize_recompute_count() - before;
        assert_eq!(
            delta, 2,
            "distinct frame_index must be a separate cache entry (2 recomputes: frame 0, frame 1)"
        );
        assert_eq!(
            a, a_again,
            "repeat lookup of frame 0 after frame 1 was computed must still be a cache hit"
        );
    }

    /// `rasterize`'s transform key excludes `translate` (a translate-only
    /// change is a cache hit) but includes rotation, per-axis scale, and
    /// `base_dot_rows` (each is a distinct entry forcing a recompute).
    #[test]
    fn transform_cached_translate_only_is_a_hit_others_recompute() {
        let _guard = test_lock();
        let bytes = synthetic_png(4, 4, 12, 34, 56);
        let before = rasterize_recompute_count();

        let base = crate::transform::Transform::new(
            crate::camera::WorldPos::new(0.0, 0.0),
            0.0,
            crate::transform::Vec2::new(1.0, 1.0),
        );
        let translated = crate::transform::Transform::new(
            crate::camera::WorldPos::new(5.0, -3.0),
            0.0,
            crate::transform::Vec2::new(1.0, 1.0),
        );
        let rotated = crate::transform::Transform::new(
            crate::camera::WorldPos::new(0.0, 0.0),
            45.0,
            crate::transform::Vec2::new(1.0, 1.0),
        );
        let scaled = crate::transform::Transform::new(
            crate::camera::WorldPos::new(0.0, 0.0),
            0.0,
            crate::transform::Vec2::new(2.0, 1.0),
        );

        let _a = rasterize(bytes, &base, 16);
        let _b = rasterize(bytes, &translated, 16); // translate-only -> hit
        let _c = rasterize(bytes, &rotated, 16); // rotation change -> recompute
        let _d = rasterize(bytes, &scaled, 16); // scale change -> recompute
        let _e = rasterize(bytes, &base, 20); // base_dot_rows change -> recompute

        let delta = rasterize_recompute_count() - before;
        assert_eq!(
            delta, 4,
            "base + rotated + scaled + different base_dot_rows are 4 distinct \
             entries; the translate-only variant must be a hit against `base`"
        );
    }

    /// Cached `sprite_to_dots` must return byte-identical output to calling
    /// the uncached `dots::sprite_to_dots` primitive directly on
    /// `decoded(bytes)` for the same inputs.
    #[test]
    fn sprite_to_dots_matches_uncached_primitive_on_decoded_image() {
        let _guard = test_lock();
        let bytes = synthetic_png(6, 4, 1, 2, 3);

        let cached = sprite_to_dots(bytes, 12, 8);
        let img = decoded(bytes);
        let expected = crate::dots::sprite_to_dots(&img, 12, 8);

        assert_eq!(
            cached, expected,
            "cached sprite_to_dots must be byte-identical to the uncached primitive"
        );
    }

    /// Cached `convert` must be byte-identical to the uncached `convert::convert`
    /// primitive, including the zero-area `Grid::new(0, 0)` short-circuit.
    #[test]
    fn convert_matches_uncached_primitive_including_zero_area() {
        let _guard = test_lock();
        let bytes = synthetic_png(8, 4, 9, 8, 7);
        let area = Rect::new(0, 0, 10, 6);

        let cached = convert(bytes, area);
        let img = decoded(bytes);
        let expected = crate::convert::convert(&img, area);
        assert_eq!(
            cached, expected,
            "cached convert must be byte-identical to the uncached primitive"
        );

        let zero_area = Rect::new(0, 0, 0, 6);
        let cached_zero = convert(bytes, zero_area);
        assert_eq!(
            cached_zero,
            Grid::new(0, 0),
            "zero-area must short-circuit to an empty grid, mirroring the uncached primitive"
        );
    }

    /// Cached `rasterize` must be byte-identical to the uncached
    /// `transform::rasterize` primitive for the same transform.
    #[test]
    fn rasterize_matches_uncached_primitive() {
        let _guard = test_lock();
        let bytes = synthetic_png(6, 6, 3, 6, 9);
        let tf = crate::transform::Transform::new(
            crate::camera::WorldPos::new(1.0, 1.0),
            30.0,
            crate::transform::Vec2::new(1.0, -1.0),
        );

        let cached = rasterize(bytes, &tf, 12);
        let img = decoded(bytes);
        let expected = crate::transform::rasterize(&img, &tf, 12);
        assert_eq!(
            cached, expected,
            "cached rasterize must be byte-identical to the uncached primitive"
        );
    }
}
