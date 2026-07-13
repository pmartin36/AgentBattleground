//! Decode-once StaticSoundData cache.
//!
//! [`sound_data`] returns the one shared decoded [`StaticSoundData`] for a
//! given `'static` byte slice, decoding via `StaticSoundData::from_cursor`
//! only on the first call for a given `bytes.as_ptr()` and returning a cheap
//! clone (kira's `Arc<[Frame]>`-backed sample buffer is shared, not copied)
//! on every subsequent call for the rest of the process's life. Never
//! evicted (bounded, fixed key space -- bundled game sounds).
//!
//! Load-bearing safety property: the cache key is ALWAYS `bytes.as_ptr() as
//! usize` -- the source bytes' own address -- and is NEVER derived from the
//! decoded `StaticSoundData`'s address. A `'static` slice lives in
//! rodata/bss for the whole process and is never freed, so its pointer has
//! no reuse hazard; a decoded sound's heap allocation can be freed and its
//! address reused for unrelated data, which is why keying on it would be
//! unsound. (Mirrors `engine_render::asset_cache`'s `DECODE_CACHE`.)

use kira::sound::static_sound::StaticSoundData;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Process-lifetime decode cache: `'static` source bytes' pointer -> the one
/// shared decoded sound for those bytes. Never evicted (bounded, fixed key
/// space -- bundled game sounds). Key is ALWAYS `bytes.as_ptr()`, NEVER a
/// decoded sound's address (the load-bearing safety property, see module doc
/// comment).
static SOUND_CACHE: OnceLock<Mutex<HashMap<usize, StaticSoundData>>> = OnceLock::new();

/// Count of real `StaticSoundData::from_cursor` decodes performed by
/// [`sound_data`]. Test-only observability; sample as a delta (before/after),
/// never as an absolute -- the cache and counter are process-global and
/// shared across concurrently-running tests.
static SOUND_DECODES: AtomicU64 = AtomicU64::new(0);

fn store() -> &'static Mutex<HashMap<usize, StaticSoundData>> {
    SOUND_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Return the one shared decoded sound for `bytes`, decoding only on the
/// first call for a given `bytes.as_ptr()`. Panics only if `bytes` is not
/// decodable (callers pass first-party bundled `'static` sounds --
/// invariant).
///
/// Consumed by `backend.rs`'s `play_sfx`/`play_music`.
pub(crate) fn sound_data(bytes: &'static [u8]) -> StaticSoundData {
    let key = bytes.as_ptr() as usize; // <-- source-bytes pointer, the only safe identity
    let mut map = store().lock().expect("sound cache mutex poisoned");
    if let Some(data) = map.get(&key) {
        return data.clone();
    }
    let data = StaticSoundData::from_cursor(Cursor::new(bytes))
        .expect("bundled first-party sound must decode");
    SOUND_DECODES.fetch_add(1, Ordering::Relaxed);
    map.insert(key, data.clone());
    data
}

/// Total real `StaticSoundData::from_cursor` decodes performed so far by
/// [`sound_data`]. Test-only observability, sampled as a delta (before/after)
/// -- never as an absolute value, since the cache and counter are
/// process-global and shared across concurrently-running tests.
#[cfg(test)]
pub(crate) fn decode_recompute_count() -> u64 {
    SOUND_DECODES.load(Ordering::Relaxed)
}

/// Test-only, crate-visible lock serializing every `#[cfg(test)]` code path
/// in THIS crate's lib-test binary (`cargo test -p engine-audio --lib`) that
/// either samples `decode_recompute_count()` as a delta, or performs a
/// decode that would inflate such a delta. b3-t3's `play_sfx`/`play_music`
/// tests MUST reuse THIS lock, not declare their own, or the delta they
/// observe can be polluted by an unrelated test's cache miss (mirrors
/// `engine_render::asset_cache::cache_test_lock`).
#[cfg(test)]
pub(crate) fn cache_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // Tolerates poison: this lock only ever serializes a counter-sampling
    // window -- it protects no invariant a partial/aborted critical section
    // could corrupt.
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Serializes each test's full before/decode/after sampling window
    /// against every other cache-touching `#[cfg(test)]` code path in this
    /// lib-test binary (see [`cache_test_lock`]'s doc comment).
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        cache_test_lock()
    }

    /// Encode a minimal 16-bit mono PCM WAV (44-byte header + a handful of
    /// samples derived from `amplitude`) and leak it to `'static`. Each call
    /// allocates a fresh `Vec`, so distinct calls always yield distinct
    /// pointers -- required so each test's cache entry is isolated from
    /// every other test's, since the decode cache is process-global.
    fn synthetic_wav(amplitude: i16) -> &'static [u8] {
        let samples: [i16; 4] = [amplitude, -amplitude, amplitude / 2, 0];
        let sample_rate: u32 = 8000;
        let num_channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let byte_rate = sample_rate * num_channels as u32 * (bits_per_sample as u32 / 8);
        let block_align = num_channels * (bits_per_sample / 8);
        let data_size = (samples.len() * 2) as u32;

        let mut buf = Vec::with_capacity(44 + data_size as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_size).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size (PCM)
        buf.extend_from_slice(&1u16.to_le_bytes()); // AudioFormat = PCM
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        for s in samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        Box::leak(buf.into_boxed_slice())
    }

    /// Two calls to `sound_data` with the SAME `'static` bytes pointer must
    /// perform exactly one real decode (recompute delta == 1); the second
    /// call must return a clone sharing the same sample buffer, not a fresh
    /// decode.
    #[test]
    fn repeat_call_same_bytes_is_one_decode() {
        let _guard = test_lock();
        let bytes = synthetic_wav(1000);
        let before = decode_recompute_count();

        let a = sound_data(bytes);
        let c = sound_data(bytes);

        let delta = decode_recompute_count() - before;
        assert_eq!(
            delta, 1,
            "second call with the same bytes pointer must not recompute"
        );
        assert!(
            Arc::ptr_eq(&a.frames, &c.frames),
            "second call must return a clone sharing the same sample buffer"
        );
    }

    /// Two calls with two DIFFERENT `'static` bytes pointers must each
    /// decode independently -- no cross-bleed between distinct sources.
    #[test]
    fn distinct_bytes_decode_independently() {
        let _guard = test_lock();
        let bytes_a = synthetic_wav(1000);
        let bytes_b = synthetic_wav(-2000);
        let before = decode_recompute_count();

        let a = sound_data(bytes_a);
        let b = sound_data(bytes_b);

        let delta = decode_recompute_count() - before;
        assert_eq!(
            delta, 2,
            "two distinct 'static byte pointers must each decode independently"
        );
        assert!(
            !Arc::ptr_eq(&a.frames, &b.frames),
            "distinct source sounds must not share a cached sample buffer"
        );
    }

    /// Mandated pointer-reuse safety regression: decode the SAME `'static`
    /// bytes TWICE independently, OUTSIDE the cache, guaranteeing the two
    /// resulting `StaticSoundData`s hold two different `frames` allocations.
    /// The cache, looked up twice by the one shared `bytes` reference, must
    /// still serve ONE shared cached entry -- the key is `bytes.as_ptr()`,
    /// never a decoded object's own address.
    #[test]
    fn cache_keys_off_source_bytes_pointer_not_decoded_data_address() {
        let _guard = test_lock();
        let bytes = synthetic_wav(500);

        let independent_a =
            StaticSoundData::from_cursor(Cursor::new(bytes)).expect("must decode");
        let independent_b =
            StaticSoundData::from_cursor(Cursor::new(bytes)).expect("must decode");
        assert!(
            !Arc::ptr_eq(&independent_a.frames, &independent_b.frames),
            "sanity check: two independent decodes of the same bytes must not \
             share a buffer"
        );

        let before = decode_recompute_count();
        let _first = sound_data(bytes);
        let _second = sound_data(bytes);
        let delta = decode_recompute_count() - before;

        assert_eq!(
            delta, 1,
            "cache must key off bytes.as_ptr(), never a decoded object's own \
             address -- two lookups of the same source bytes must collapse to \
             one shared entry regardless of decode order"
        );
    }
}
