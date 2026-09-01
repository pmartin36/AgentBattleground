//! GGUF download, checksum verification, and on-disk storage for local
//! text-generation models. Presence and the "not downloaded" signal are
//! pure filesystem checks over the storage layout `weights_path` owns;
//! `ensure_present` fetches through an injected `GgufFetcher` seam, verifies
//! the transferred file's SHA-256 against the registry entry's digest, and
//! atomically stores only a verified file — a checksum mismatch never lands
//! at `weights_path`.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::model_registry::ModelEntry;

/// Number of bytes streamed per read/hash chunk while verifying a
/// downloaded file's digest.
const HASH_CHUNK_SIZE: usize = 64 * 1024;

/// Subdir (under the base data dir) holding downloaded model weights.
pub const MODELS_DIR: &str = "models";
/// How many times a checksum mismatch is re-fetched before giving up.
pub const MAX_FETCH_ATTEMPTS: u32 = 3;

/// A structured install failure. Distinct from `TextError` (a
/// generation-time error); the config layer maps `NotDownloaded` into its
/// own "model not downloaded" error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The model's verified weights are absent.
    NotDownloaded { model_id: String },
    /// The fetch seam failed to transfer the file.
    Fetch(String),
    /// The transferred file's digest did not match the manifest after all
    /// retries. Carries both digests for a debuggable message.
    ChecksumMismatch { model_id: String, expected: String, actual: String },
    /// A filesystem error while storing/checking.
    Io(String),
}

/// Network seam: transfer the bytes at `url` into `dest`. Multi-GB;
/// production streams to disk. Injectable so tests never hit the network.
pub trait GgufFetcher: Send + Sync {
    fn fetch(&self, url: &str, dest: &Path) -> Result<(), InstallError>;
}

/// Production fetcher over `ureq`. Not exercised by hermetic tests (the
/// real transfer is multi-GB and requires network access).
pub struct UreqFetcher;

impl GgufFetcher for UreqFetcher {
    fn fetch(&self, url: &str, dest: &Path) -> Result<(), InstallError> {
        let response =
            ureq::get(url).call().map_err(|e| InstallError::Fetch(e.to_string()))?;
        let mut reader = response.into_body().into_reader();
        let mut file = std::fs::File::create(dest).map_err(|e| InstallError::Io(e.to_string()))?;
        std::io::copy(&mut reader, &mut file).map_err(|e| InstallError::Fetch(e.to_string()))?;
        file.sync_all().map_err(|e| InstallError::Io(e.to_string()))?;
        Ok(())
    }
}

/// The URL basename of `entry.gguf_url` (e.g. `Qwen3-4B-Instruct-2507-Q4_K_M.gguf`),
/// with any `?query` stripped defensively.
pub fn gguf_file_name(entry: &ModelEntry) -> &str {
    let without_query = entry.gguf_url.split('?').next().unwrap_or(entry.gguf_url);
    without_query.rsplit('/').next().unwrap_or(without_query)
}

/// `<base>/models/<model_id>/`.
pub fn model_dir(base: &Path, model_id: &str) -> PathBuf {
    base.join(MODELS_DIR).join(model_id)
}

/// `<base>/models/<model_id>/<file>.gguf` — the single owner of the storage
/// layout. Callers resolving a weights path must call this, never
/// reconstruct it.
pub fn weights_path(base: &Path, entry: &ModelEntry) -> PathBuf {
    model_dir(base, entry.model_id).join(gguf_file_name(entry))
}

/// True iff the verified weights file exists at `weights_path`.
pub fn is_present(base: &Path, entry: &ModelEntry) -> bool {
    weights_path(base, entry).exists()
}

/// The path if present, else `Err(NotDownloaded)` — the pure "not
/// downloaded" signal (no fetch).
pub fn require_present(base: &Path, entry: &ModelEntry) -> Result<PathBuf, InstallError> {
    let path = weights_path(base, entry);
    if path.exists() {
        Ok(path)
    } else {
        Err(InstallError::NotDownloaded { model_id: entry.model_id.to_string() })
    }
}

/// Ensure-present-on-first-need: returns the path if already present; else
/// fetch -> verify SHA-256 -> atomic store, re-fetching on mismatch up to
/// `MAX_FETCH_ATTEMPTS`, returning the final path on success.
pub fn ensure_present(
    base: &Path,
    entry: &ModelEntry,
    fetcher: &dyn GgufFetcher,
) -> Result<PathBuf, InstallError> {
    let final_path = weights_path(base, entry);
    if final_path.exists() {
        return Ok(final_path);
    }

    let dir = model_dir(base, entry.model_id);
    std::fs::create_dir_all(&dir).map_err(|e| InstallError::Io(e.to_string()))?;

    let mut tmp_name = final_path.as_os_str().to_os_string();
    tmp_name.push(".partial");
    let tmp_path = PathBuf::from(tmp_name);

    let mut actual = String::new();
    for _ in 0..MAX_FETCH_ATTEMPTS {
        fetcher.fetch(entry.gguf_url, &tmp_path)?;
        actual = hash_file_hex(&tmp_path)?;
        if actual == entry.sha256 {
            std::fs::rename(&tmp_path, &final_path).map_err(|e| InstallError::Io(e.to_string()))?;
            return Ok(final_path);
        }
        let _ = std::fs::remove_file(&tmp_path);
    }

    Err(InstallError::ChecksumMismatch {
        model_id: entry.model_id.to_string(),
        expected: entry.sha256.to_string(),
        actual,
    })
}

/// Streams `path` through SHA-256 in `HASH_CHUNK_SIZE` chunks and returns
/// the lowercase-hex digest.
fn hash_file_hex(path: &Path) -> Result<String, InstallError> {
    let mut file = std::fs::File::open(path).map_err(|e| InstallError::Io(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; HASH_CHUNK_SIZE];
    loop {
        let n = file.read(&mut buf).map_err(|e| InstallError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// Lowercase-hex encodes a byte slice (no `hex` crate dependency).
fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_gen::model_registry::ModelLicense;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp dir (pid + monotonic counter), the crate's
    /// no-`tempfile`-crate hermetic-dir pattern.
    fn temp_install_dir(tag: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "game-model-install-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ))
    }

    const VERIFIED_BYTES: &[u8] = b"verified-gguf-bytes";
    const CORRUPT_BYTES: &[u8] = b"corrupt-bytes";
    const VERIFIED_SHA256: &str =
        "1ecb56fe32ab3e8a0b5c6896e185eeb84c5bdb53d6d92975290ab31396891e6d";

    /// A synthetic registry entry small enough to hash in a hermetic test;
    /// `sha256` is the digest of `VERIFIED_BYTES`.
    fn test_entry() -> ModelEntry {
        ModelEntry {
            model_id: "test-model",
            display_name: "T",
            param_size: "0B",
            gguf_url: "https://example/test-model.gguf",
            sha256: VERIFIED_SHA256,
            byte_size: 19,
            license: ModelLicense::Mit,
        }
    }

    /// A `GgufFetcher` that writes a scripted sequence of byte payloads on
    /// successive calls (repeating the last entry once exhausted), and
    /// records how many times it was called.
    struct ScriptedFetcher {
        responses: Vec<&'static [u8]>,
        calls: AtomicUsize,
    }

    impl ScriptedFetcher {
        fn new(responses: Vec<&'static [u8]>) -> Self {
            Self { responses, calls: AtomicUsize::new(0) }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl GgufFetcher for ScriptedFetcher {
        fn fetch(&self, _url: &str, dest: &Path) -> Result<(), InstallError> {
            let i = self.calls.fetch_add(1, Ordering::SeqCst);
            let bytes = self
                .responses
                .get(i)
                .or_else(|| self.responses.last())
                .copied()
                .unwrap_or(b"");
            std::fs::write(dest, bytes).map_err(|e| InstallError::Io(e.to_string()))
        }
    }

    /// The weights path is `<base>/models/<model_id>/<url-basename>`.
    #[test]
    fn weights_path_matches_layout() {
        let base = temp_install_dir("layout");
        let entry = test_entry();
        let expected = base.join("models").join("test-model").join("test-model.gguf");
        assert_eq!(weights_path(&base, &entry), expected);
    }

    /// The GGUF file name is the basename of the entry's download URL.
    #[test]
    fn gguf_file_name_is_url_basename() {
        let entry = test_entry();
        assert_eq!(gguf_file_name(&entry), "test-model.gguf");
    }

    /// Before any store, presence is false and `require_present` surfaces
    /// the "not downloaded" signal naming the model id.
    #[test]
    fn absent_reports_not_present_and_not_downloaded() {
        let base = temp_install_dir("absent");
        let entry = test_entry();
        assert!(!is_present(&base, &entry));
        match require_present(&base, &entry) {
            Err(InstallError::NotDownloaded { model_id }) => assert_eq!(model_id, "test-model"),
            other => panic!("expected NotDownloaded, got {other:?}"),
        }
    }

    /// A fetcher that transfers bytes matching the manifest digest results
    /// in a verified file at the expected path and presence flipping true.
    #[test]
    fn verified_download_lands_at_expected_path() {
        let base = temp_install_dir("verified");
        let entry = test_entry();
        let fetcher = ScriptedFetcher::new(vec![VERIFIED_BYTES]);

        let result = ensure_present(&base, &entry, &fetcher).expect("verified fetch succeeds");

        assert_eq!(result, weights_path(&base, &entry));
        assert!(result.exists());
        assert!(is_present(&base, &entry));
    }

    /// A fetcher whose transferred bytes never match the manifest digest is
    /// rejected after exhausting retries, and no file is left at the
    /// expected path — the security property that a corrupt/tampered
    /// download can never masquerade as installed weights.
    #[test]
    fn checksum_mismatch_is_rejected_and_not_stored() {
        let base = temp_install_dir("mismatch");
        let entry = test_entry();
        let fetcher = ScriptedFetcher::new(vec![CORRUPT_BYTES; MAX_FETCH_ATTEMPTS as usize]);

        let result = ensure_present(&base, &entry, &fetcher);

        match result {
            Err(InstallError::ChecksumMismatch { model_id, .. }) => {
                assert_eq!(model_id, "test-model")
            }
            other => panic!("expected ChecksumMismatch, got {other:?}"),
        }
        assert!(!weights_path(&base, &entry).exists());
        assert!(!is_present(&base, &entry));
    }

    /// A fetcher that returns a corrupt transfer once and then the correct
    /// bytes succeeds within the retry budget.
    #[test]
    fn mismatch_then_match_retries_to_success() {
        let base = temp_install_dir("retry");
        let entry = test_entry();
        let fetcher = ScriptedFetcher::new(vec![CORRUPT_BYTES, VERIFIED_BYTES]);

        let result = ensure_present(&base, &entry, &fetcher).expect("succeeds after one retry");

        assert_eq!(result, weights_path(&base, &entry));
        assert!(fetcher.call_count() >= 2);
    }

    /// When a verified file is already present at the expected path,
    /// `ensure_present` returns it without invoking the fetcher at all.
    #[test]
    fn already_present_skips_fetch() {
        let base = temp_install_dir("skip");
        let entry = test_entry();
        let path = weights_path(&base, &entry);
        std::fs::create_dir_all(path.parent().expect("weights path has a parent dir"))
            .expect("create model dir");
        std::fs::write(&path, VERIFIED_BYTES).expect("pre-place verified file");
        let fetcher = ScriptedFetcher::new(vec![]);

        let result = ensure_present(&base, &entry, &fetcher).expect("already-present succeeds");

        assert_eq!(result, path);
        assert_eq!(fetcher.call_count(), 0);
    }
}
