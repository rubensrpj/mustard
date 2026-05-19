//! Host runtime metadata recorded into `mustard.json`.
//!
//! **Why this is a shell of `detect-runtime.ts`.** The JS module asserted the
//! process was running under Bun and refused to continue otherwise — Mustard's
//! hooks and scripts were Bun/Node files that needed a runtime to execute. As
//! of epics B3/B4 those hooks and scripts are the compiled `mustard-rt`
//! binary, and (B5) the CLI itself is this native binary. Nothing reads back
//! `runtime.chosen` from `mustard.json` any more — a scan of `packages/rt`
//! confirms the rt hooks spawn `bun`/`node` by *trying each*, never by
//! consulting the persisted choice.
//!
//! So the Bun assertion is dropped. A small `runtime` block is still written
//! to `mustard.json` for backward compatibility — the dashboard (B6) and any
//! legacy `.claude/` reader expect the key to exist — but it now records the
//! *native* CLI host (`os`/`arch`) rather than a JS runtime. This is the
//! "minimal port / vestigial" outcome the Wave 1 brief asked for.

use serde::Serialize;

/// The runtime block stamped into `.claude/mustard.json` by `init`/`update`.
///
/// Serialised under the `runtime` key. `kind` is the literal `"native"` to
/// signal — to anything still reading this field — that the CLI is no longer
/// a Bun script. `os`/`arch` come from `std::env::consts`.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    /// Always `"native"` — the CLI is a compiled binary, not a JS runtime.
    pub kind: &'static str,
    /// Target operating system (`std::env::consts::OS`).
    pub os: &'static str,
    /// Target CPU architecture (`std::env::consts::ARCH`).
    pub arch: &'static str,
}

impl RuntimeInfo {
    /// Capture the current host's runtime metadata.
    pub fn detect() -> Self {
        Self {
            kind: "native",
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }
}

impl Default for RuntimeInfo {
    fn default() -> Self {
        Self::detect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_reports_native_host() {
        let info = RuntimeInfo::detect();
        assert_eq!(info.kind, "native");
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
    }

    #[test]
    fn serializes_with_expected_keys() {
        let json = serde_json::to_value(RuntimeInfo::detect()).unwrap();
        assert_eq!(json.get("kind").and_then(|v| v.as_str()), Some("native"));
        assert!(json.get("os").is_some());
        assert!(json.get("arch").is_some());
    }
}
