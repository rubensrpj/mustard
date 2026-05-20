//! Read and write of `pipeline-state` files —
//! `.claude/.pipeline-states/{specName}.json`.
//!
//! A pipeline-state file is one JSON object per in-flight pipeline, keyed by
//! spec name. Pipeline commands write it as the pipeline advances; hooks
//! (`model-routing-gate.js`, `close-gate.js`) read it. The on-disk shape is
//! [`PipelineState`].
//!
//! Consumers depend on the [`PipelineRepo`] **trait** so a test can inject a
//! fake; [`FsPipelineRepo`] is the filesystem-backed implementation. Writes go
//! through [`fs::write_atomic`](crate::store::fs::write_atomic) — a pipeline-state
//! is never left half-written, even if the process dies mid-write.
//!
//! Reads are fail-open: an absent state file is reported as
//! [`Error::NotFound`], which a caller can map to "no active pipeline"
//! without treating it as a failure.

use crate::error::{Error, Result};
use crate::store::fs;
use crate::model::pipeline::PipelineState;
use std::path::{Path, PathBuf};

/// Directory name holding pipeline-state files, under `.claude/`.
const PIPELINE_STATES_DIR: &str = ".pipeline-states";

/// A store of pipeline-state, addressable by spec name.
///
/// The trait is the API the B3/B4 dispatcher and hooks program against.
/// Implementations must fail open: a [`read`](PipelineRepo::read) of an
/// unknown spec returns [`Err`] (typically [`Error::NotFound`]) rather than
/// panicking.
pub trait PipelineRepo {
    /// Read the pipeline-state for `spec_name`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotFound`] when no state file exists for the spec, and
    /// [`Error::Parse`] / [`Error::Io`] for a corrupt file or an I/O failure.
    fn read(&self, spec_name: &str) -> Result<PipelineState>;

    /// Write `state` as the pipeline-state for `spec_name`.
    ///
    /// The write is atomic — a concurrent reader sees either the previous
    /// state or the complete new one.
    ///
    /// # Errors
    ///
    /// Returns an [`Error`] if the state could not be serialized or persisted.
    fn write(&self, spec_name: &str, state: &PipelineState) -> Result<()>;
}

/// Filesystem-backed [`PipelineRepo`] rooted at a project directory.
///
/// State files live at
/// `{project_dir}/.claude/.pipeline-states/{spec_name}.json`.
#[derive(Debug, Clone)]
pub struct FsPipelineRepo {
    states_dir: PathBuf,
}

impl FsPipelineRepo {
    /// Create a repo for the standard pipeline-states directory of a project.
    #[must_use]
    pub fn for_project(project_dir: impl AsRef<Path>) -> Self {
        let states_dir = project_dir
            .as_ref()
            .join(".claude")
            .join(PIPELINE_STATES_DIR);
        Self { states_dir }
    }

    /// Create a repo whose state files live directly in `states_dir`.
    #[must_use]
    pub fn new(states_dir: impl Into<PathBuf>) -> Self {
        Self {
            states_dir: states_dir.into(),
        }
    }

    /// The directory holding the pipeline-state files.
    #[must_use]
    pub fn states_dir(&self) -> &Path {
        &self.states_dir
    }

    /// Resolve the on-disk path for a given spec's state file.
    #[must_use]
    pub fn path_for(&self, spec_name: &str) -> PathBuf {
        self.states_dir.join(format!("{spec_name}.json"))
    }
}

impl PipelineRepo for FsPipelineRepo {
    fn read(&self, spec_name: &str) -> Result<PipelineState> {
        let path = self.path_for(spec_name);
        let text = fs::read_to_string(&path)?;
        let state = serde_json::from_str::<PipelineState>(&text)?;
        Ok(state)
    }

    fn write(&self, spec_name: &str, state: &PipelineState) -> Result<()> {
        let path = self.path_for(spec_name);
        // Pretty-printed to match the human-edited JS pipeline-state files.
        let json = serde_json::to_vec_pretty(state)?;
        fs::write_atomic(&path, &json)
    }
}

/// Best-effort read of a pipeline-state — a missing file is `Ok(None)`.
///
/// A convenience wrapper for the common fail-open caller that wants to treat
/// "no pipeline" and "a pipeline" uniformly and only surface real failures.
///
/// # Errors
///
/// Returns [`Error::Parse`] / [`Error::Io`] for a corrupt file or an I/O
/// failure, but maps [`Error::NotFound`] to `Ok(None)`.
pub fn read_optional(repo: &impl PipelineRepo, spec_name: &str) -> Result<Option<PipelineState>> {
    match repo.read(spec_name) {
        Ok(state) => Ok(Some(state)),
        Err(Error::NotFound(_)) => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::pipeline::{Phase, Scope};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn sample_state() -> PipelineState {
        PipelineState {
            spec_name: Some("2026-05-18-b2-mustard-core-crate".to_string()),
            status: Some("implementing".to_string()),
            phase: Some(Phase::Execute),
            scope: Some(Scope::Full),
            current_wave: 2,
            total_waves: 4,
            ..PipelineState::default()
        }
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempdir().unwrap();
        let repo = FsPipelineRepo::new(dir.path());
        let spec = "2026-05-18-b2-mustard-core-crate";
        repo.write(spec, &sample_state()).unwrap();

        let loaded = repo.read(spec).unwrap();
        assert_eq!(loaded.phase, Some(Phase::Execute));
        assert_eq!(loaded.scope, Some(Scope::Full));
        assert_eq!(loaded.current_wave, 2);
        assert_eq!(loaded.total_waves, 4);
        assert_eq!(loaded.spec_name.as_deref(), Some(spec));
    }

    #[test]
    fn write_is_atomic_no_temp_files_left() {
        let dir = tempdir().unwrap();
        let repo = FsPipelineRepo::new(dir.path());
        repo.write("spec-a", &sample_state()).unwrap();
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], std::ffi::OsStr::new("spec-a.json"));
    }

    #[test]
    fn read_unknown_spec_reports_not_found() {
        let dir = tempdir().unwrap();
        let repo = FsPipelineRepo::new(dir.path());
        match repo.read("does-not-exist") {
            Err(Error::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn read_optional_maps_missing_to_none() {
        let dir = tempdir().unwrap();
        let repo = FsPipelineRepo::new(dir.path());
        assert!(read_optional(&repo, "absent").unwrap().is_none());
        repo.write("present", &sample_state()).unwrap();
        assert!(read_optional(&repo, "present").unwrap().is_some());
    }

    #[test]
    fn for_project_resolves_standard_path() {
        let repo = FsPipelineRepo::for_project("/proj");
        let path = repo.path_for("my-spec");
        assert!(path.ends_with("my-spec.json"));
        assert!(
            path.components()
                .any(|c| c.as_os_str() == ".pipeline-states")
        );
    }

    /// A fake [`PipelineRepo`] proves the trait is the injectable seam.
    #[test]
    fn trait_supports_an_in_memory_fake() {
        struct FakeRepo {
            store: Mutex<HashMap<String, PipelineState>>,
        }
        impl PipelineRepo for FakeRepo {
            fn read(&self, spec_name: &str) -> Result<PipelineState> {
                self.store
                    .lock()
                    .ok()
                    .and_then(|m| m.get(spec_name).cloned())
                    .ok_or_else(|| Error::NotFound(spec_name.to_string()))
            }
            fn write(&self, spec_name: &str, state: &PipelineState) -> Result<()> {
                if let Ok(mut m) = self.store.lock() {
                    m.insert(spec_name.to_string(), state.clone());
                }
                Ok(())
            }
        }

        let fake = FakeRepo {
            store: Mutex::new(HashMap::new()),
        };
        fake.write("s", &sample_state()).unwrap();
        assert_eq!(fake.read("s").unwrap().current_wave, 2);
        assert!(read_optional(&fake, "missing").unwrap().is_none());
    }
}
