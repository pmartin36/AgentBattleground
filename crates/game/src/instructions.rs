//! Creature instruction files: path scheme, base-dir resolver, autocreating
//! read/write (spec 47 §4, task b3-t1).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Subdirectory (under the resolved base data dir) where per-creature
/// instruction Markdown files live.
pub const INSTRUCTIONS_DIR: &str = "creature_instructions";

/// Env var that overrides the base data dir at runtime (spec 47 §4).
const DATA_DIR_ENV: &str = "AGENTBATTLEGROUND_DATA_DIR";

/// Resolves the base data dir: `explicit` arg wins; else the
/// `AGENTBATTLEGROUND_DATA_DIR` env var; else (debug build) the workspace
/// root, (release build) the directory containing the running executable.
fn base_data_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    if let Ok(dir) = std::env::var(DATA_DIR_ENV) {
        if !dir.is_empty() {
            return dir.into();
        }
    }
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// `<name with ' '→'_'>.md` (ASCII space only, no other transform).
fn instructions_file_name(name: &str) -> String {
    format!("{}.md", name.replace(' ', "_"))
}

// ── explicit-base forms (test surface, race-free) ──────────────────────────

/// `base/creature_instructions/<name with ' '→'_'>.md`. Pure path
/// construction, no IO.
pub fn instructions_path_in(base: &Path, name: &str) -> PathBuf {
    base.join(INSTRUCTIONS_DIR).join(instructions_file_name(name))
}

/// Reads the instructions file for `name` under `base`, autocreating the
/// parent dir + an empty file when missing and returning `""` in that case.
pub fn read_instructions_in(base: &Path, name: &str) -> io::Result<String> {
    let path = instructions_path_in(base, name);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, "")?;
        return Ok(String::new());
    }
    fs::read_to_string(&path)
}

/// Writes `contents` to the instructions file for `name` under `base`,
/// creating the dir/file as needed and overwriting any existing content.
pub fn write_instructions_in(base: &Path, name: &str, contents: &str) -> io::Result<()> {
    let path = instructions_path_in(base, name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, contents)
}

// ── runtime forms (resolver-driven; honor AGENTBATTLEGROUND_DATA_DIR) ──────

/// `instructions_path_in(&base_data_dir(None), name)`.
pub fn instructions_path(name: &str) -> PathBuf {
    instructions_path_in(&base_data_dir(None), name)
}

/// `read_instructions_in(&base_data_dir(None), name)`.
pub fn read_instructions(name: &str) -> io::Result<String> {
    read_instructions_in(&base_data_dir(None), name)
}

/// `write_instructions_in(&base_data_dir(None), name, contents)`.
pub fn write_instructions(name: &str, contents: &str) -> io::Result<()> {
    write_instructions_in(&base_data_dir(None), name, contents)
}

/// Reads `name`'s instructions via `read_instructions_in(base, name)` when
/// `base` is `Some`, else via the runtime resolver `read_instructions(name)`.
/// Shared by `RosterManager::reload_instructions` and `PromptEditor::new` so
/// the base-dir-overridable fallback is defined in exactly one place.
pub fn read_instructions_maybe(base: Option<&Path>, name: &str) -> io::Result<String> {
    match base {
        Some(b) => read_instructions_in(b, name),
        None => read_instructions(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique per-test temp base dir (pid + monotonic counter), per the
    /// no-`tempfile`-crate pattern in `crates/game/src/cli.rs`. Race-free
    /// under parallel test execution; never touches the real repo tree.
    fn temp_base_dir(tag: &str) -> PathBuf {
        let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "game-instructions-test-{}-{}-{}",
            std::process::id(),
            tag,
            n
        ))
    }

    #[test]
    fn instructions_dir_const_is_creature_instructions() {
        assert_eq!(INSTRUCTIONS_DIR, "creature_instructions");
    }

    #[test]
    fn instructions_path_maps_spaces_to_underscores() {
        let base = temp_base_dir("path-map");
        let path = instructions_path_in(&base, "Ember Wolf");
        assert!(
            path.ends_with("creature_instructions/Ember_Wolf.md"),
            "expected path to end with creature_instructions/Ember_Wolf.md, got {:?}",
            path
        );
    }

    #[test]
    fn read_missing_autocreates_empty_file() {
        let base = temp_base_dir("read-missing");
        let name = "Autocreate Test Creature";

        let contents = read_instructions_in(&base, name).expect("read should succeed");
        assert_eq!(contents, "");

        let expected_path = base.join(INSTRUCTIONS_DIR).join("Autocreate_Test_Creature.md");
        assert!(
            expected_path.is_file(),
            "expected {:?} to have been autocreated",
            expected_path
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn write_then_read_round_trips_contents() {
        let base = temp_base_dir("round-trip");
        let name = "Round Trip Creature";

        write_instructions_in(&base, name, "# notes\nsome flavor text")
            .expect("write should succeed");
        let read_back = read_instructions_in(&base, name).expect("read should succeed");
        assert_eq!(read_back, "# notes\nsome flavor text");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn write_overwrites_existing_contents() {
        let base = temp_base_dir("overwrite");
        let name = "Overwrite Creature";

        write_instructions_in(&base, name, "first version").expect("first write");
        write_instructions_in(&base, name, "second version").expect("second write");
        let read_back = read_instructions_in(&base, name).expect("read should succeed");
        assert_eq!(read_back, "second version");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn gitignore_contains_creature_instructions() {
        let gitignore_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitignore");
        let contents = std::fs::read_to_string(&gitignore_path)
            .unwrap_or_else(|e| panic!("failed to read {:?}: {}", gitignore_path, e));
        assert!(
            contents.lines().any(|l| l.trim() == "creature_instructions/"),
            ".gitignore should contain a `creature_instructions/` line, got:\n{}",
            contents
        );
    }
}
