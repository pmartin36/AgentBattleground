//! Signed atomic player-data store: HMAC-SHA256 integrity over the bincode
//! body, atomic (temp -> rename) writes, a `.bak` roll of the prior valid
//! main before each overwrite, and the load fallback chain
//! main -> .bak -> caller-supplied seed. Deters hand-edited saves; the key
//! ships in the binary, so this is not cryptographic anti-cheat.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::instructions::base_data_dir;
use crate::player_data::schema::PlayerData;

type HmacSha256 = Hmac<Sha256>;

/// File name for the primary save, under the resolved data dir.
pub const SAVE_FILE_NAME: &str = "player_data.bin";
/// File name for the rolled-over prior-valid save, under the resolved data dir.
pub const BACKUP_FILE_NAME: &str = "player_data.bak";

/// Binary-baked HMAC key. Tamper deterrence only — this ships inside the
/// binary and is never read from disk or the environment.
const STORE_HMAC_KEY: &[u8] = b"agent-battleground/player-data/hmac/v1";

/// Which source satisfied a `PlayerStore::load` call.
pub enum Loaded {
    /// The main save file was present, HMAC-valid, and decoded.
    Main(PlayerData),
    /// The main file was missing/invalid; the `.bak` file was valid.
    Backup(PlayerData),
    /// Neither the main nor `.bak` file was valid/present; this is the
    /// caller-supplied default seed.
    Seeded(PlayerData),
}

impl Loaded {
    pub fn data(&self) -> &PlayerData {
        match self {
            Loaded::Main(d) | Loaded::Backup(d) | Loaded::Seeded(d) => d,
        }
    }

    pub fn into_data(self) -> PlayerData {
        match self {
            Loaded::Main(d) | Loaded::Backup(d) | Loaded::Seeded(d) => d,
        }
    }
}

/// A save/load engine rooted at a resolved data directory.
pub struct PlayerStore {
    dir: PathBuf,
}

impl PlayerStore {
    /// Explicit-base constructor. Does no IO; safe for hermetic tests.
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Runtime resolver: honors `AGENTBATTLEGROUND_DATA_DIR`, else the
    /// same base dir `crate::instructions` uses.
    pub fn resolve() -> Self {
        Self::with_dir(base_data_dir(None))
    }

    pub fn main_path(&self) -> PathBuf {
        self.dir.join(SAVE_FILE_NAME)
    }

    pub fn backup_path(&self) -> PathBuf {
        self.dir.join(BACKUP_FILE_NAME)
    }

    /// Path of the temp file `save` writes before renaming it over
    /// `main_path`. Never present once `save` returns.
    pub fn temp_path(&self) -> PathBuf {
        let mut name = self.main_path().into_os_string();
        name.push(".tmp");
        PathBuf::from(name)
    }

    /// Rolls the prior valid main to `.bak`, then atomically writes `data`
    /// as the new main.
    pub fn save(&self, data: &PlayerData) -> io::Result<()> {
        let body = bincode::serialize(data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // If the current main is present and still HMAC-valid, roll it to
        // `.bak` verbatim (already signed; no need to re-sign) before it is
        // overwritten below.
        if let Ok(main_bytes) = std::fs::read(self.main_path()) {
            if verify_and_extract(&main_bytes).is_some() {
                atomic_write(&self.backup_path(), &main_bytes)?;
            }
        }

        atomic_write(&self.main_path(), &frame(&body))
    }

    /// Walks main -> .bak -> `seed()`, verifying HMAC at each rung and
    /// deserializing only bytes that verify. `seed` is only invoked when
    /// both files are absent/invalid.
    pub fn load(&self, seed: impl FnOnce() -> PlayerData) -> Loaded {
        if let Some(data) = read_verified(&self.main_path()) {
            return Loaded::Main(data);
        }
        if let Some(data) = read_verified(&self.backup_path()) {
            return Loaded::Backup(data);
        }
        Loaded::Seeded(seed())
    }
}

/// HMAC-SHA256 over `body`, keyed by the binary-baked store key.
fn sign(body: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(STORE_HMAC_KEY)
        .expect("HMAC accepts a key of any length");
    mac.update(body);
    mac.finalize().into_bytes().into()
}

/// Frames a body as `[32-byte HMAC-SHA256 tag] ++ body`.
fn frame(body: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(32 + body.len());
    framed.extend_from_slice(&sign(body));
    framed.extend_from_slice(body);
    framed
}

/// Splits `bytes` into `[tag] ++ [body]` and returns `body` only if the tag
/// verifies against it in constant time. Never returns bytes whose tag does
/// not match — this is the single gate every load rung must pass through
/// before the body is deserialized.
fn verify_and_extract(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 32 {
        return None;
    }
    let (tag, body) = bytes.split_at(32);
    let mut mac = HmacSha256::new_from_slice(STORE_HMAC_KEY)
        .expect("HMAC accepts a key of any length");
    mac.update(body);
    mac.verify_slice(tag).ok()?;
    Some(body.to_vec())
}

/// Reads `path`, verifies its HMAC tag, and deserializes the verified body
/// as `PlayerData`. Returns `None` for any failure at any step (missing
/// file, short file, bad tag, bad bincode) — callers treat all of those as
/// "this rung did not pan out", never as a hard error.
fn read_verified(path: &Path) -> Option<PlayerData> {
    let bytes = std::fs::read(path).ok()?;
    let body = verify_and_extract(&bytes)?;
    bincode::deserialize(&body).ok()
}

/// Atomically writes `bytes` to `final_path`: creates the parent dir if
/// needed, writes to a `<final_path>.tmp` sibling, flushes and fsyncs it,
/// then renames it over `final_path`. The rename is what makes this atomic;
/// a reader never observes a partially-written `final_path`.
fn atomic_write(final_path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = final_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = final_path.as_os_str().to_os_string();
    tmp_name.push(".tmp");
    let tmp_path = PathBuf::from(tmp_name);

    let mut file = std::fs::File::create(&tmp_path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    std::fs::rename(&tmp_path, final_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ability::Element;
    use crate::player_data::schema::{Egg, EggState};
    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp dir (pid + monotonic counter), the crate's
    /// no-`tempfile`-crate hermetic-dir pattern.
    fn temp_store_dir(tag: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("game-player-data-store-test-{}-{}-{}", std::process::id(), tag, n))
    }

    /// A `PlayerData` distinguishable from other samples via `mad_lib`.
    fn sample_data(tag: &str) -> PlayerData {
        PlayerData {
            roster: vec![],
            eggs: vec![Egg {
                element: Element::Fire,
                state: EggState::Ready,
                mad_lib: Some(tag.to_string()),
                egg_art: None,
                hatchling: None,
            }],
        }
    }

    fn flip_last_byte(path: &std::path::Path) {
        let mut bytes = std::fs::read(path).expect("read file to corrupt");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        std::fs::write(path, bytes).expect("write corrupted file");
    }

    /// `save` then `load` returns `Loaded::Main` whose data is structurally
    /// equal to what was saved — the core round-trip contract.
    #[test]
    fn save_then_load_returns_main_with_equal_data() {
        let store = PlayerStore::with_dir(temp_store_dir("round-trip"));
        let data = sample_data("v1");

        store.save(&data).expect("save should succeed");
        let loaded = store.load(|| sample_data("seed-should-not-be-used"));

        assert!(matches!(loaded, Loaded::Main(_)), "expected Loaded::Main");
        assert_eq!(loaded.into_data(), data);
    }

    /// After a save leaves the temp file gone and the main file HMAC-valid —
    /// the write is atomic, never half-written.
    #[test]
    fn save_leaves_no_leftover_temp_and_main_is_fully_written() {
        let store = PlayerStore::with_dir(temp_store_dir("atomicity"));
        let data = sample_data("v1");

        store.save(&data).expect("save should succeed");

        assert!(!store.temp_path().exists(), "temp file must not survive a completed save");
        assert!(store.main_path().is_file(), "main file must exist after save");
    }

    /// A second save rolls the prior main to `.bak`. A byte flip inside the
    /// (now-stale) main file makes `load` fall back to `.bak` — the last
    /// legitimate state — never an error and never the seed.
    #[test]
    fn corrupted_main_falls_back_to_backup_after_second_save() {
        let store = PlayerStore::with_dir(temp_store_dir("bak-recovery"));
        let v1 = sample_data("v1");
        let v2 = sample_data("v2");

        store.save(&v1).expect("first save");
        store.save(&v2).expect("second save should roll v1 to .bak");
        flip_last_byte(&store.main_path());

        let loaded = store.load(|| sample_data("seed-should-not-be-used"));

        assert!(matches!(loaded, Loaded::Backup(_)), "expected Loaded::Backup");
        assert_eq!(loaded.into_data(), v1, "backup must hold the last legitimate state, not v2 or the seed");
    }

    /// When both main and `.bak` are corrupted, `load` falls back to the
    /// caller-supplied seed rather than erroring or fabricating data.
    #[test]
    fn both_main_and_backup_corrupt_falls_back_to_seed() {
        let store = PlayerStore::with_dir(temp_store_dir("both-corrupt"));
        let v1 = sample_data("v1");
        let v2 = sample_data("v2");
        let seed = sample_data("seed");

        store.save(&v1).expect("first save");
        store.save(&v2).expect("second save should roll v1 to .bak");
        flip_last_byte(&store.main_path());
        flip_last_byte(&store.backup_path());

        let loaded = store.load(|| seed.clone());

        assert!(matches!(loaded, Loaded::Seeded(_)), "expected Loaded::Seeded");
        assert_eq!(loaded.into_data(), seed);
    }

    /// A file whose HMAC tag does not match its content is never trusted —
    /// even when the body decodes to a perfectly valid `PlayerData`, a
    /// hand-crafted wrong tag must be rejected, not silently accepted.
    #[test]
    fn tampered_hmac_is_rejected_even_when_body_decodes_cleanly() {
        let store = PlayerStore::with_dir(temp_store_dir("hmac-gate"));
        std::fs::create_dir_all(store.main_path().parent().unwrap()).expect("mkdir");

        let smuggled = sample_data("smuggled");
        let body = bincode::serialize(&smuggled).expect("serialize");
        let mut framed = vec![0u8; 32]; // wrong tag: all-zero, does not verify against body
        framed.extend_from_slice(&body);
        std::fs::write(store.main_path(), framed).expect("write hand-crafted main");

        let seed = sample_data("seed");
        let loaded = store.load(|| seed.clone());

        assert!(matches!(loaded, Loaded::Seeded(_)), "wrong-tag content must never be trusted as Main");
        assert_eq!(loaded.into_data(), seed);
    }

    /// On an empty/missing-file dir, `load` returns `Loaded::Seeded` and is
    /// read-only — it creates no files itself.
    #[test]
    fn missing_files_returns_seeded_and_creates_nothing() {
        let store = PlayerStore::with_dir(temp_store_dir("first-run"));
        let seed = sample_data("seed");

        let loaded = store.load(|| seed.clone());

        assert!(matches!(loaded, Loaded::Seeded(_)));
        assert_eq!(loaded.into_data(), seed);
        assert!(!store.main_path().exists(), "load must not write the main file");
        assert!(!store.backup_path().exists(), "load must not write the backup file");
    }

    /// `PlayerStore::resolve()` honors `AGENTBATTLEGROUND_DATA_DIR`: a
    /// save/load round trip through it lands under that directory.
    #[test]
    fn resolve_honors_data_dir_env_override() {
        let dir = temp_store_dir("env-override");
        with_data_dir(&dir, || {
            let store = PlayerStore::resolve();
            let data = sample_data("env-v1");
            store.save(&data).expect("save should succeed");
            let loaded = store.load(|| sample_data("seed-should-not-be-used"));

            assert_eq!(store.main_path(), dir.join(SAVE_FILE_NAME));
            assert!(matches!(loaded, Loaded::Main(_)));
            assert_eq!(loaded.into_data(), data);
        });
    }
}

#[cfg(test)]
mod env_guard {
    use std::path::Path;
    use std::sync::Mutex;

    // Serializes every test in the game crate that touches the
    // process-global AGENTBATTLEGROUND_DATA_DIR env var — `PlayerStore::resolve()`
    // and `registry::construct` are the only two readers of
    // `base_data_dir(None)` in the test binary, and the env var is
    // process-wide, so they must share this one lock or race.
    static ENV_GUARD: Mutex<()> = Mutex::new(());

    /// Unsets `AGENTBATTLEGROUND_DATA_DIR` on drop, including on panic
    /// unwind, so a failing assertion never leaks the override into
    /// whichever test runs next in this process.
    struct EnvVarGuard;
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            std::env::remove_var("AGENTBATTLEGROUND_DATA_DIR");
        }
    }

    /// Runs `f` with `AGENTBATTLEGROUND_DATA_DIR` overridden to `dir`,
    /// holding the crate-wide env-var test lock for the duration and
    /// restoring the environment afterward (even on panic).
    pub(crate) fn with_data_dir<R>(dir: &Path, f: impl FnOnce() -> R) -> R {
        let _lock = ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AGENTBATTLEGROUND_DATA_DIR", dir);
        let _env_guard = EnvVarGuard;
        f()
    }
}

#[cfg(test)]
pub(crate) use env_guard::with_data_dir;
