//! The opt-in `generate_text` result cache, keyed by a normalized derived
//! key (model identity, prompt, params, seed). Off by default; engaged only
//! when a request opts in.

use std::collections::HashMap;
use std::sync::Mutex;

use super::types::{ResolvedModelConfig, TextRequest};

/// Normalized, hashable cache key. `TextRequest` is `PartialEq`-only
/// (`temperature: f32`), so the key derives a hashable form via
/// `f32::to_bits`.
#[derive(Clone, PartialEq, Eq, Hash)]
struct TextCacheKey {
    model_identity: String,
    system: String,
    user: String,
    temperature_bits: u32,
    max_tokens: u32,
    stop: Vec<String>,
    seed: Option<u64>,
    grammar: Option<String>,
}

impl TextCacheKey {
    fn derive(config: &ResolvedModelConfig, request: &TextRequest) -> Self {
        TextCacheKey {
            model_identity: config.model_identity().to_string(),
            system: request.system.clone(),
            user: request.user.clone(),
            temperature_bits: request.temperature.to_bits(),
            max_tokens: request.max_tokens,
            stop: request.stop.clone(),
            seed: request.seed,
            grammar: request.grammar.clone(),
        }
    }
}

/// A get-or-bake cache of generated text. A hit returns the stored string
/// without running `bake` again; an `Err` bake is never stored, so a later
/// retry with the same key bakes again.
pub struct TextCache {
    map: Mutex<HashMap<TextCacheKey, String>>,
}

impl TextCache {
    pub fn new() -> Self {
        TextCache {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the cached string for `(config, request)` if one has already
    /// been baked; `None` before the first successful bake of that key.
    pub fn get(&self, config: &ResolvedModelConfig, request: &TextRequest) -> Option<String> {
        let key = TextCacheKey::derive(config, request);
        let map = self.map.lock().expect("TextCache mutex poisoned");
        map.get(&key).cloned()
    }

    /// Returns the cached string for `(config, request)` if present;
    /// otherwise runs `bake` exactly once and stores its `Ok` result. A
    /// second call with an equal key never runs `bake` again; an `Err`
    /// result is never stored.
    pub fn get_or_bake<F, E>(&self, config: &ResolvedModelConfig, request: &TextRequest, bake: F) -> Result<String, E>
    where
        F: FnOnce() -> Result<String, E>,
    {
        let key = TextCacheKey::derive(config, request);

        {
            let map = self.map.lock().expect("TextCache mutex poisoned");
            if let Some(value) = map.get(&key) {
                return Ok(value.clone());
            }
        }

        let value = bake()?;

        let mut map = self.map.lock().expect("TextCache mutex poisoned");
        let value = map.entry(key).or_insert(value).clone();
        Ok(value)
    }
}

impl Default for TextCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::text_gen::types::Provider;

    fn config() -> ResolvedModelConfig {
        ResolvedModelConfig::new(Provider::Local, "model-a", None, Some("local-bin".to_string()), None)
    }

    fn request(seed: Option<u64>) -> TextRequest {
        TextRequest {
            system: "sys".to_string(),
            user: "user".to_string(),
            temperature: 0.5,
            max_tokens: 64,
            stop: Vec::new(),
            seed,
            grammar: None,
        }
    }

    /// A second `get_or_bake` call with an equal `(config, request)` key
    /// returns the same string without invoking `bake` again.
    #[test]
    fn get_or_bake_hit_bakes_once() {
        let cache = TextCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let cfg = config();
        let req = request(Some(1));

        let bake = |calls: Arc<AtomicUsize>| {
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<String, ()>("baked".to_string())
            }
        };

        let first = cache.get_or_bake(&cfg, &req, bake(calls.clone())).unwrap();
        let second = cache.get_or_bake(&cfg, &req, bake(calls.clone())).unwrap();

        assert_eq!(first, second);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Two requests that differ (by seed) are distinct cache keys: each
    /// misses and bakes its own result.
    #[test]
    fn get_or_bake_miss_on_different_key() {
        let cache = TextCache::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let cfg = config();

        let bake = |calls: Arc<AtomicUsize>, tag: &'static str| {
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<String, ()>(tag.to_string())
            }
        };

        cache.get_or_bake(&cfg, &request(Some(1)), bake(calls.clone(), "a")).unwrap();
        cache.get_or_bake(&cfg, &request(Some(2)), bake(calls.clone(), "b")).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// A `bake` that returns `Err` is not cached: a subsequent call with the
    /// same key runs `bake` again rather than serving a stale error.
    #[test]
    fn get_or_bake_err_not_cached() {
        let cache = TextCache::new();
        let cfg = config();
        let req = request(Some(1));

        let first = cache.get_or_bake(&cfg, &req, || Err::<String, ()>(()));
        assert!(first.is_err());

        let second = cache.get_or_bake(&cfg, &req, || Ok::<String, ()>("recovered".to_string()));
        assert_eq!(second, Ok("recovered".to_string()));
    }

    /// `get` reflects the cache's contents: `None` before any bake of that
    /// key, `Some(value)` after a successful bake.
    #[test]
    fn get_reflects_bake() {
        let cache = TextCache::new();
        let cfg = config();
        let req = request(Some(1));

        assert_eq!(cache.get(&cfg, &req), None);
        cache.get_or_bake(&cfg, &req, || Ok::<String, ()>("baked".to_string())).unwrap();
        assert_eq!(cache.get(&cfg, &req), Some("baked".to_string()));
    }
}
