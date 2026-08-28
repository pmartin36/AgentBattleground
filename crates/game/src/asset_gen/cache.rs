//! Keyed result cache for generated assets: images keyed by their
//! `ImageRequest`, clips keyed by `(image, action)`. A repeat request with
//! an equal key returns the already-baked asset instead of re-invoking the
//! caller-supplied `bake` closure (which, in the real operations, drives
//! the recipe backend and job runner). A failed bake is never cached, so a
//! later retry with the same key bakes again.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;

use super::types::{ClipAsset, ImageAsset, ImageRequest};

/// A generic get-or-bake cache: on a miss, `bake` runs exactly once and its
/// `Ok` result is stored; a hit returns the stored value without running
/// `bake` again. `Err` results are not stored.
struct ResultCache<K, V> {
    map: Mutex<HashMap<K, V>>,
}

impl<K, V> Default for ResultCache<K, V> {
    fn default() -> Self {
        ResultCache {
            map: Mutex::new(HashMap::new()),
        }
    }
}

impl<K, V> ResultCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn get(&self, key: &K) -> Option<V> {
        let map = self.map.lock().expect("ResultCache mutex poisoned");
        map.get(key).cloned()
    }

    fn get_or_bake<F, E>(&self, key: K, bake: F) -> Result<V, E>
    where
        F: FnOnce() -> Result<V, E>,
    {
        {
            let map = self.map.lock().expect("ResultCache mutex poisoned");
            if let Some(value) = map.get(&key) {
                return Ok(value.clone());
            }
        }

        let value = bake()?;

        let mut map = self.map.lock().expect("ResultCache mutex poisoned");
        let value = map.entry(key).or_insert(value).clone();
        Ok(value)
    }
}

/// The per-operation result cache shared across `generate_image` /
/// `generate_animation` calls. Images are keyed by their `ImageRequest`;
/// clips are keyed by `(ImageAsset, action)`.
#[derive(Default)]
pub struct AssetCache {
    images: ResultCache<ImageRequest, ImageAsset>,
    clips: ResultCache<(ImageAsset, String), ClipAsset>,
}

impl AssetCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached `ImageAsset` for `request` if one has already
    /// been baked; `None` before the first successful bake of that key.
    pub fn get_image(&self, request: &ImageRequest) -> Option<ImageAsset> {
        self.images.get(request)
    }

    /// Returns the cached `ClipAsset` for `(image, action)` if one has
    /// already been baked; `None` before the first successful bake of that
    /// key.
    pub fn get_clip(&self, image: &ImageAsset, action: &str) -> Option<ClipAsset> {
        self.clips.get(&(image.clone(), action.to_string()))
    }

    /// Returns the cached `ImageAsset` for `request` if present; otherwise
    /// runs `bake` exactly once, stores its `Ok` result, and returns it. A
    /// second call with an equal `request` never runs `bake` again.
    pub fn image_or_bake<F, E>(&self, request: &ImageRequest, bake: F) -> Result<ImageAsset, E>
    where
        F: FnOnce() -> Result<ImageAsset, E>,
    {
        self.images.get_or_bake(request.clone(), bake)
    }

    /// Returns the cached `ClipAsset` for `(image, action)` if present;
    /// otherwise runs `bake` exactly once, stores its `Ok` result, and
    /// returns it. A second call with an equal `(image, action)` never runs
    /// `bake` again.
    pub fn clip_or_bake<F, E>(&self, image: &ImageAsset, action: &str, bake: F) -> Result<ClipAsset, E>
    where
        F: FnOnce() -> Result<ClipAsset, E>,
    {
        self.clips
            .get_or_bake((image.clone(), action.to_string()), bake)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::asset_gen::types::{Fidelity, KeyColor};

    fn image_request(seed: u64) -> ImageRequest {
        ImageRequest {
            prompt: "a creature".to_string(),
            fidelity: Fidelity::Draft,
            seed,
            background_key: KeyColor { r: 0, g: 255, b: 0 },
            import_path: None,
        }
    }

    fn image_asset(tag: &str) -> ImageAsset {
        ImageAsset {
            path: format!("/tmp/{tag}.png").into(),
        }
    }

    fn clip_asset(tag: &str) -> ClipAsset {
        ClipAsset {
            frames: vec![format!("/tmp/{tag}-0.png").into()],
        }
    }

    /// A second `image_or_bake` call with an equal `ImageRequest` returns
    /// the same asset without invoking `bake` again.
    #[test]
    fn image_cache_hit_bakes_once() {
        let cache = AssetCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let req = image_request(1);

        let bake = |calls: Arc<AtomicUsize>, tag: &'static str| {
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ImageAsset, ()>(image_asset(tag))
            }
        };

        let first = cache.image_or_bake(&req, bake(calls.clone(), "a")).unwrap();
        let second = cache.image_or_bake(&req, bake(calls.clone(), "a")).unwrap();

        assert_eq!(first, second);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Two `ImageRequest`s that differ (by seed) are distinct cache keys:
    /// each misses and bakes its own asset.
    #[test]
    fn image_cache_miss_on_different_key() {
        let cache = AssetCache::new();
        let calls = Arc::new(AtomicUsize::new(0));

        let bake = |calls: Arc<AtomicUsize>, tag: &'static str| {
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ImageAsset, ()>(image_asset(tag))
            }
        };

        let a = cache
            .image_or_bake(&image_request(1), bake(calls.clone(), "a"))
            .unwrap();
        let b = cache
            .image_or_bake(&image_request(2), bake(calls.clone(), "b"))
            .unwrap();

        assert_ne!(a, b);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// A second `clip_or_bake` call with an equal `(image, action)` key
    /// returns the same clip without invoking `bake` again.
    #[test]
    fn clip_cache_hit_bakes_once() {
        let cache = AssetCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let image = image_asset("still");

        let bake = |calls: Arc<AtomicUsize>, tag: &'static str| {
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ClipAsset, ()>(clip_asset(tag))
            }
        };

        let first = cache
            .clip_or_bake(&image, "idle", bake(calls.clone(), "idle"))
            .unwrap();
        let second = cache
            .clip_or_bake(&image, "idle", bake(calls.clone(), "idle"))
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Same image, different `action` string: the key differs, so the
    /// second call misses and bakes again.
    #[test]
    fn clip_cache_miss_on_different_action() {
        let cache = AssetCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let image = image_asset("still");

        let bake = |calls: Arc<AtomicUsize>, tag: &'static str| {
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<ClipAsset, ()>(clip_asset(tag))
            }
        };

        cache
            .clip_or_bake(&image, "idle", bake(calls.clone(), "idle"))
            .unwrap();
        cache
            .clip_or_bake(&image, "attack", bake(calls.clone(), "attack"))
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// A `bake` that returns `Err` is not cached: a subsequent call with
    /// the same key runs `bake` again rather than serving a stale error.
    #[test]
    fn failed_bake_not_cached() {
        let cache = AssetCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let req = image_request(1);

        let calls_fail = calls.clone();
        let first = cache.image_or_bake(&req, || {
            calls_fail.fetch_add(1, Ordering::SeqCst);
            Err::<ImageAsset, ()>(())
        });
        assert!(first.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let calls_ok = calls.clone();
        let second = cache.image_or_bake(&req, || {
            calls_ok.fetch_add(1, Ordering::SeqCst);
            Ok::<ImageAsset, ()>(image_asset("a"))
        });
        assert!(second.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// `get_image` reflects the cache's contents: `None` before any bake
    /// of that key, `Some(asset)` after a successful bake.
    #[test]
    fn get_image_reflects_bake() {
        let cache = AssetCache::new();
        let req = image_request(1);

        assert_eq!(cache.get_image(&req), None);

        let asset = cache
            .image_or_bake(&req, || Ok::<ImageAsset, ()>(image_asset("a")))
            .unwrap();

        assert_eq!(cache.get_image(&req), Some(asset));
    }
}
