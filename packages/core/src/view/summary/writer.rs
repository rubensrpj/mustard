//! Summary writer — serialises a [`SpecSummaryDoc`] and writes it atomically
//! to `{spec_dir}/.summary.json`.
//!
//! The write is atomic (temp file + rename) so a crash mid-write never leaves
//! a corrupt file. The file is UTF-8 JSON with pretty-print formatting so it
//! diffs cleanly in git.

use std::path::Path;

use crate::platform::error::Result;
use crate::io::fs;

use super::SpecSummaryDoc;

/// File name of the summary artefact inside the spec directory.
pub const SUMMARY_FILENAME: &str = ".summary.json";

/// Serialise `doc` and write it atomically to `{spec_dir}/.summary.json`.
///
/// # Errors
/// Returns [`crate::platform::error::Error::Parse`] if serialisation fails (should not
/// happen with valid structs), or [`crate::platform::error::Error::Io`] on filesystem
/// failures.
pub fn write(spec_dir: &Path, doc: &SpecSummaryDoc) -> Result<()> {
    let json = serde_json::to_string_pretty(doc)?;
    let dest = spec_dir.join(SUMMARY_FILENAME);
    fs::write_atomic(&dest, json.as_bytes())?;
    Ok(())
}

/// Read and deserialise `.summary.json` from `{spec_dir}/.summary.json`.
///
/// Returns `None` when the file does not exist (spec has no summary yet).
///
/// # Errors
/// Returns an error on parse failure or non-NotFound IO errors.
pub fn read(spec_dir: &Path) -> Result<Option<SpecSummaryDoc>> {
    let path = spec_dir.join(SUMMARY_FILENAME);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let doc: SpecSummaryDoc = serde_json::from_str(&content)?;
            Ok(Some(doc))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::summary::{AcResult, SpecSummaryDoc, WaveSummary};
    use tempfile::tempdir;

    fn sample_doc() -> SpecSummaryDoc {
        SpecSummaryDoc {
            version: 1,
            spec: "2026-05-26-no-sqlite-git-source-of-truth".into(),
            title: "No SQLite — Git como fonte de verdade".into(),
            lang: Some("pt-BR".into()),
            stage: Some("Close".into()),
            outcome: Some("Completed".into()),
            waves: vec![WaveSummary {
                n: 1,
                role: "core".into(),
                summary: "Summary schema + writer created".into(),
                status: "completed".into(),
                ac_results: vec![AcResult {
                    id: "AC-W1.1".into(),
                    pass: true,
                    command: Some("node -e \"...\"".into()),
                    note: None,
                }],
                review: Some("approved".into()),
                qa: Some("pass".into()),
                concerns: vec![],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn write_and_read_round_trip() {
        let dir = tempdir().unwrap();
        let doc = sample_doc();
        write(dir.path(), &doc).unwrap();

        let path = dir.path().join(".summary.json");
        assert!(path.exists());

        let read_back = read(dir.path()).unwrap().unwrap();
        assert_eq!(read_back, doc);
    }

    #[test]
    fn read_returns_none_when_file_absent() {
        let dir = tempdir().unwrap();
        let result = read(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn written_json_has_numeric_version() {
        let dir = tempdir().unwrap();
        write(dir.path(), &sample_doc()).unwrap();
        let raw = std::fs::read_to_string(dir.path().join(".summary.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(v["version"].is_number());
        assert_eq!(v["version"].as_u64(), Some(1));
    }
}
