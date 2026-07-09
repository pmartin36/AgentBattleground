//! App-wide logging bootstrap (spec 43, Decisions 1-3).
//!
//! `init` resolves the OS-appropriate data directory via `directories` and
//! delegates to `init_at`, the testable primitive that actually opens the
//! log file, wires up the non-blocking writer, and installs the global
//! `tracing_subscriber` (idempotently — repeat installs are ignored so test
//! re-entry and the panic hook's own eventual use of this module never
//! panic on "already set").

use std::io;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::EnvFilter;

/// Handle returned by [`init`]/`init_at`. Callers must hold this for the
/// life of the process — dropping it early stops the background writer
/// thread and further log lines are lost.
pub struct LoggingHandle {
    pub log_path: PathBuf,
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Resolve the OS data dir for `app_name` (via `directories::ProjectDirs`)
/// and initialize logging under `<data_dir>/logs/`.
pub fn init(app_name: &str) -> io::Result<LoggingHandle> {
    let project_dirs = directories::ProjectDirs::from("", "", app_name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no data dir"))?;
    let dir = project_dirs.data_dir().join("logs");
    init_at(&dir)
}

/// Testable primitive: create `dir` if needed, open a new
/// `game-<unix-epoch-seconds>.log` inside it, install the global
/// `tracing_subscriber` (non-blocking writer, ANSI off, `EnvFilter`
/// defaulting to `info`), prune old logs, and return the handle.
#[allow(dead_code)]
fn init_at(dir: &Path) -> io::Result<LoggingHandle> {
    std::fs::create_dir_all(dir)?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let log_path = dir.join(format!("game-{secs}.log"));
    let file = std::fs::File::create(&log_path)?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file);

    let subscriber = build_subscriber(non_blocking, default_filter());
    let _ = tracing::subscriber::set_global_default(subscriber);

    prune_old_logs(dir, 20);

    Ok(LoggingHandle {
        log_path,
        _guard: guard,
    })
}

/// Delete all but the `keep` lexicographically-largest `game-*.log` files
/// in `dir` (filenames embed unix-epoch-seconds, so name-sort == recency).
/// Best-effort: filesystem errors are ignored.
#[allow(dead_code)]
fn prune_old_logs(dir: &Path, keep: usize) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.starts_with("game-") && name.ends_with(".log"))
        .collect();
    names.sort();

    if names.len() > keep {
        for name in &names[..names.len() - keep] {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
}

type Subscriber = tracing_subscriber::fmt::Subscriber<
    tracing_subscriber::fmt::format::DefaultFields,
    tracing_subscriber::fmt::format::Format,
    EnvFilter,
    NonBlocking,
>;

/// Single source of truth for the subscriber's format config (timestamp,
/// level, target, message, fields; ANSI off) given an already-resolved
/// filter. Both `init_at` (global install, filter from `default_filter()`)
/// and tests (thread-local `with_default` with an explicit filter, to avoid
/// depending on `RUST_LOG` process state) build the subscriber through this
/// one function.
#[allow(dead_code)]
fn build_subscriber(writer: NonBlocking, filter: EnvFilter) -> Subscriber {
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .finish()
}

/// Decision 3's filter default: `RUST_LOG` if set and parsable, else `info`.
#[allow(dead_code)]
fn default_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;

    /// Unique tempdir per test, matching the established no-`tempfile`-crate
    /// pattern (crates/game/src/cli.rs, crates/engine/inspector/src/client.rs).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "engine-core-logging-test-{tag}-{}-{}",
                std::process::id(),
                n
            ));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn touch(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"x").unwrap();
    }

    #[test]
    fn prune_old_logs_keeps_only_the_n_most_recent_by_name() {
        let tmp = TempDir::new("prune-keep");
        for i in 0..25u32 {
            touch(tmp.path(), &format!("game-{:010}.log", 1_700_000_000 + i));
        }
        prune_old_logs(tmp.path(), 20);

        let mut remaining: Vec<String> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        remaining.sort();

        assert_eq!(remaining.len(), 20, "expected exactly 20 files to survive pruning");
        // The 20 largest-named (most recent) filenames must be the ones kept.
        let mut expected: Vec<String> = (5..25u32)
            .map(|i| format!("game-{:010}.log", 1_700_000_000 + i))
            .collect();
        expected.sort();
        assert_eq!(remaining, expected);
    }

    #[test]
    fn prune_old_logs_is_a_no_op_when_at_or_under_the_keep_count() {
        let tmp = TempDir::new("prune-noop");
        for i in 0..5u32 {
            touch(tmp.path(), &format!("game-{:010}.log", 1_700_000_000 + i));
        }
        prune_old_logs(tmp.path(), 20);

        let remaining = fs::read_dir(tmp.path()).unwrap().count();
        assert_eq!(remaining, 5, "no files should be deleted when len <= keep");
    }

    #[test]
    fn init_at_creates_exactly_one_log_file_matching_the_handle_path() {
        let tmp = TempDir::new("init-at");
        let handle = init_at(tmp.path()).expect("init_at should succeed in a fresh tempdir");

        let files: Vec<PathBuf> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("game-") && n.ends_with(".log"))
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(files.len(), 1, "init_at must create exactly one game-*.log file");
        assert_eq!(files[0], handle.log_path);
    }

    #[test]
    fn info_reaches_the_log_file_via_a_locally_installed_subscriber() {
        let tmp = TempDir::new("info-reaches-file");
        let log_path = tmp.path().join("game-local.log");
        let file = fs::File::create(&log_path).unwrap();
        let (non_blocking, _guard) = tracing_appender::non_blocking(file);

        let subscriber = build_subscriber(non_blocking, EnvFilter::new("info"));

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("sentinel-info-line");
        });
        drop(_guard);

        let mut contents = String::new();
        fs::File::open(&log_path)
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        assert!(
            contents.contains("sentinel-info-line"),
            "expected info line in log file, got: {contents:?}"
        );
    }

    #[test]
    fn debug_is_filtered_out_under_the_default_info_filter() {
        let tmp = TempDir::new("filter-default");
        let log_path = tmp.path().join("game-local.log");
        let file = fs::File::create(&log_path).unwrap();
        let (non_blocking, _guard) = tracing_appender::non_blocking(file);

        let subscriber = build_subscriber(non_blocking, EnvFilter::new("info"));

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("no-show-debug-line");
            tracing::info!("show-info-line");
        });
        drop(_guard);

        let mut contents = String::new();
        fs::File::open(&log_path)
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        assert!(contents.contains("show-info-line"));
        assert!(!contents.contains("no-show-debug-line"));
    }
}
