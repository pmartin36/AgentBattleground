//! Single-owner resolver for sd-cli model filenames and the `loras` subdir
//! against one configured assets-models directory. A present file/dir
//! resolves to an absolute path under the configured dir; every absent
//! case (unset/empty env, missing dir, missing file, missing subdir)
//! surfaces a typed `ModelPathError` naming the missing thing, never a
//! silent filename-only reference or a panic.

use std::path::{Path, PathBuf};

/// Env var pointing at the directory holding sd-cli model files and the
/// `loras` subdir.
pub const ENV_MODELS_DIR: &str = "AGENTBATTLEGROUND_SDCLI_MODELS_DIR";
/// Subdir (under the configured dir) holding LoRA weights.
pub const LORAS_SUBDIR: &str = "loras";

/// A resolution failure: names exactly what is missing, never a silent
/// absence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelPathError {
    /// The env var configuring the models dir is unset or empty.
    DirNotConfigured { env: String },
    /// The configured path is not an existing directory.
    DirMissing { dir: PathBuf },
    /// A required model file is absent under the configured dir.
    MissingFile { name: String, dir: PathBuf },
    /// A required subdir (e.g. `loras`) is absent under the configured dir.
    MissingDir { name: String, dir: PathBuf },
}

impl std::fmt::Display for ModelPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelPathError::DirNotConfigured { env } => {
                write!(f, "{} is not set to a models directory", env)
            }
            ModelPathError::DirMissing { dir } => {
                write!(f, "configured models directory does not exist: {}", dir.display())
            }
            ModelPathError::MissingFile { name, dir } => {
                write!(f, "model file '{}' not found under {}", name, dir.display())
            }
            ModelPathError::MissingDir { name, dir } => {
                write!(f, "'{}' directory not found under {}", name, dir.display())
            }
        }
    }
}

/// A handle over one configured assets-models directory. `resolve` and
/// `resolve_loras_dir` are the only ways to turn a filename into an
/// absolute path; `verify` controls whether presence is actually checked
/// (always `true` outside `#[cfg(test)]`'s `unchecked`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelPaths {
    dir: PathBuf,
    verify: bool,
}

impl ModelPaths {
    /// Production entry: reads `ENV_MODELS_DIR` — the only env read in this
    /// module — and delegates to the pure `from_dir_str`.
    pub fn from_env() -> Result<Self, ModelPathError> {
        Self::from_dir_str(std::env::var(ENV_MODELS_DIR).ok().as_deref())
    }

    /// Pure, hermetic core: takes the configured dir value as an argument
    /// so tests never touch process env. `None`/empty is
    /// `DirNotConfigured`; a non-existent path is `DirMissing`.
    pub fn from_dir_str(dir: Option<&str>) -> Result<Self, ModelPathError> {
        let dir = match dir {
            Some(d) if !d.is_empty() => d,
            _ => {
                return Err(ModelPathError::DirNotConfigured { env: ENV_MODELS_DIR.to_string() })
            }
        };
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err(ModelPathError::DirMissing { dir });
        }
        Ok(ModelPaths { dir, verify: true })
    }

    /// Resolves a required model FILE by name to its absolute path under
    /// the configured dir, or the typed error naming it when absent.
    pub fn resolve(&self, name: &str) -> Result<PathBuf, ModelPathError> {
        let path = self.dir.join(name);
        if self.verify && !path.is_file() {
            return Err(ModelPathError::MissingFile { name: name.to_string(), dir: self.dir.clone() });
        }
        Ok(path)
    }

    /// Resolves the `loras` subdir to its absolute path, or the typed
    /// error naming it when absent.
    pub fn resolve_loras_dir(&self) -> Result<PathBuf, ModelPathError> {
        let path = self.dir.join(LORAS_SUBDIR);
        if self.verify && !path.is_dir() {
            return Err(ModelPathError::MissingDir {
                name: LORAS_SUBDIR.to_string(),
                dir: self.dir.clone(),
            });
        }
        Ok(path)
    }

    /// Non-verifying constructor for tests only: production has no path to
    /// a silent-bad-argv resolver. `resolve`/`resolve_loras_dir` never
    /// error, but still return a path under the given dir.
    #[cfg(test)]
    pub fn unchecked(dir: impl Into<PathBuf>) -> Self {
        ModelPaths { dir: dir.into(), verify: false }
    }

    /// The configured directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique temp dir per test run — hermetic, no reliance on ambient
    /// state or process env.
    fn hermetic_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "game-model-paths-test-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn from_dir_str_none_is_dir_not_configured() {
        assert_eq!(
            ModelPaths::from_dir_str(None),
            Err(ModelPathError::DirNotConfigured { env: ENV_MODELS_DIR.to_string() })
        );
    }

    #[test]
    fn from_dir_str_empty_is_dir_not_configured() {
        assert_eq!(
            ModelPaths::from_dir_str(Some("")),
            Err(ModelPathError::DirNotConfigured { env: ENV_MODELS_DIR.to_string() })
        );
    }

    #[test]
    fn from_dir_str_nonexistent_dir_is_dir_missing() {
        let missing = "/no/such/abg-models-dir-xyz";
        assert_eq!(
            ModelPaths::from_dir_str(Some(missing)),
            Err(ModelPathError::DirMissing { dir: PathBuf::from(missing) })
        );
    }

    #[test]
    fn resolve_present_file_returns_absolute_path() {
        let dir = hermetic_temp_dir("present-file");
        std::fs::write(dir.join("z_image.gguf"), b"stub").unwrap();

        let paths = ModelPaths::from_dir_str(Some(dir.to_str().unwrap())).expect("dir exists");
        let resolved = paths.resolve("z_image.gguf").expect("file is present");

        assert_eq!(resolved, dir.join("z_image.gguf"));
        assert!(resolved.is_absolute());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_missing_file_names_it() {
        let dir = hermetic_temp_dir("missing-file");

        let paths = ModelPaths::from_dir_str(Some(dir.to_str().unwrap())).expect("dir exists");
        let err = paths.resolve("absent.gguf").expect_err("file is absent");

        assert_eq!(
            err,
            ModelPathError::MissingFile { name: "absent.gguf".to_string(), dir: dir.clone() }
        );
        assert!(err.to_string().contains("absent.gguf"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_loras_dir_present_returns_path() {
        let dir = hermetic_temp_dir("loras-present");
        std::fs::create_dir_all(dir.join(LORAS_SUBDIR)).unwrap();

        let paths = ModelPaths::from_dir_str(Some(dir.to_str().unwrap())).expect("dir exists");
        let resolved = paths.resolve_loras_dir().expect("loras dir is present");

        assert_eq!(resolved, dir.join(LORAS_SUBDIR));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_loras_dir_missing_names_it() {
        let dir = hermetic_temp_dir("loras-missing");

        let paths = ModelPaths::from_dir_str(Some(dir.to_str().unwrap())).expect("dir exists");
        let err = paths.resolve_loras_dir().expect_err("loras dir is absent");

        assert_eq!(
            err,
            ModelPathError::MissingDir { name: LORAS_SUBDIR.to_string(), dir: dir.clone() }
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unchecked_skips_verification() {
        let dir = hermetic_temp_dir("unchecked");
        let abs_dir = dir.clone();

        let paths = ModelPaths::unchecked(abs_dir.clone());
        let resolved = paths.resolve("absent.gguf").expect("unchecked never errors");

        assert_eq!(resolved, abs_dir.join("absent.gguf"));
        assert!(resolved.is_absolute());

        std::fs::remove_dir_all(&dir).ok();
    }
}
