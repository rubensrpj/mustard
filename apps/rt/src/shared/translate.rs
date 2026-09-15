//! `translate` — client for the OPTIONAL `mustard-translate` sidecar (local
//! Marian MT, its own excluded crate under `apps/translate`).
//!
//! Two callers share it: the `feature` auto-gloss (`text --input`) and the
//! the automatic gloss (`batch`, one spawn for the whole
//! dictionary). The sidecar's contract is one JSON line per input —
//! `{"en":"...","detected":"pt"}` — in input order.
//!
//! Everything here is FAIL-OPEN by construction: the binary is optional, so
//! every method returns `Option` and any resolution/spawn/parse failure is
//! `None` — the caller proceeds without translation, never degraded harder
//! than "no gloss / no equivalences". Resolution order mirrors the sidecar
//! precedent ([`mustard_core::Scan::locate`]) and the dev layout: sibling of
//! the running executable (the install layout — also the workspace
//! `target/{release,debug}` when running a built `mustard-rt` directly), then
//! `PATH`, then the cwd-relative `target/release` / `target/debug` dev dirs.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// One translated line: the English text + the detected source language
/// (`"en"` means the input was already English and passed through).
#[derive(Debug, Clone)]
pub struct Translation {
    pub en: String,
    pub detected: String,
}

/// A handle to a resolved `mustard-translate` binary.
#[derive(Debug, Clone)]
pub struct Translate {
    binary: PathBuf,
}

impl Translate {
    /// Resolve the sidecar: exe-sibling → `PATH` → `target/release` →
    /// `target/debug`. `None` when absent anywhere — the caller's fail-open
    /// branch.
    #[must_use]
    pub fn locate() -> Option<Self> {
        let name = if cfg!(windows) { "mustard-translate.exe" } else { "mustard-translate" };
        if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)) {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(Self { binary: cand });
            }
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                let cand = dir.join(name);
                if cand.is_file() {
                    return Some(Self { binary: cand });
                }
            }
        }
        for rel in ["target/release", "target/debug"] {
            let cand = PathBuf::from(rel).join(name);
            if cand.is_file() {
                return Some(Self { binary: cand });
            }
        }
        None
    }

    /// Translate ONE sentence (`text --input`). `None` on any failure.
    #[must_use]
    pub fn text(&self, input: &str) -> Option<Translation> {
        let out = Command::new(&self.binary)
            .args(["text", "--input", input])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        parse_line(&String::from_utf8_lossy(&out.stdout))
    }

}

/// Parse one sidecar stdout line into a [`Translation`], tolerating any
/// non-JSON prefix (parse from the first `{`). `None` on any malformation.
fn parse_line(s: &str) -> Option<Translation> {
    let start = s.find('{')?;
    let v: serde_json::Value = serde_json::from_str(s[start..].trim()).ok()?;
    Some(Translation {
        en: v.get("en")?.as_str()?.to_string(),
        detected: v.get("detected")?.as_str()?.to_string(),
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line_reads_the_sidecar_contract() {
        let t = parse_line(r#"{"en":"bank statement","detected":"pt"}"#).expect("valid line");
        assert_eq!(t.en, "bank statement");
        assert_eq!(t.detected, "pt");

        // Non-JSON prefix tolerated; missing fields / garbage are None.
        let t = parse_line(r#"warm-up noise {"en":"x","detected":"en"}"#).expect("prefixed line");
        assert_eq!(t.detected, "en");
        assert!(parse_line("no json here").is_none());
        assert!(parse_line(r#"{"en":"only-en"}"#).is_none(), "detected is required");
    }

}
