//! `mustard-rt run spec-validate` — validate a spec directory against the
//! [`mustard_core::spec::contract`] layout.
//!
//! Reads `meta.json` + `spec.md` (+ optional `wave-plan.md`) at the given
//! path, reconstructs a [`SpecInput`], and runs the contract validator.
//! Emits a JSON report. Exit codes: `0` ok, `2` violations, `1` IO failure.

use crate::shared::context::project_dir;
use crate::run::spec::spec_sections::is_heading;
use mustard_core::fs as mfs;
use mustard_core::meta::read_meta_beside;
use mustard_core::spec::contract::{
    self, AcceptanceCriterion, SectionBody, SpecInput, PLAN_SECTIONS, PRD_SECTIONS,
};
use mustard_core::ClaudePaths;
use mustard_core::{model::view::Phase, Outcome, Scope, Stage};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Emit an error JSON envelope + exit with the given code.
fn emit_error(reason: &str, detail: &str, _json_out: bool, exit_code: i32) -> ! {
    let body = json!({
        "ok": false,
        "error": reason,
        "detail": detail,
    });
    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into()));
    std::process::exit(exit_code);
}

/// CLI entry point.
pub fn run(spec_path: &Path, json_out: bool) {
    let (spec_md_path, spec_dir) = resolve_paths(spec_path);
    let Ok(spec_text) = mfs::read_to_string(&spec_md_path) else {
        emit_error("could not read spec.md", &spec_md_path.display().to_string(), json_out, 1);
    };
    let meta = read_meta_beside(&spec_md_path);

    let slug = spec_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Soft-warning: compare spec.lang (from meta.json) with mustard.json#specLang.
    // Fail-open per `core-fail-open-error`: missing/unreadable mustard.json is silent.
    let spec_lang = meta.as_ref().and_then(|m| m.lang.as_deref());
    let project_root = PathBuf::from(project_dir());
    if let Some(warn) = check_spec_lang_against_mustard_json(spec_lang, &project_root) {
        eprintln!("{warn}");
    }

    let input = build_input_from_files(&slug, &spec_text, meta.as_ref());
    match contract::validate(&input) {
        Ok(()) => {
            let body = json!({ "ok": true, "spec": slug, "violations": [] });
            println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into()));
            std::process::exit(0);
        }
        Err(violations) => {
            let v: Vec<String> = violations.iter().map(ToString::to_string).collect();
            let body = json!({ "ok": false, "spec": slug, "violations": v });
            let _ = json_out; // flag reserved for future structured output
            println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into()));
            std::process::exit(2);
        }
    }
}

/// Read `specLang` from `<project_root>/mustard.json`. Fail-open: missing file,
/// IO error, malformed JSON, or absent key all yield `None`. Mirrors the
/// `read_mustard_tone` reader in `spec_draft.rs`.
fn read_mustard_spec_lang(project_root: &Path) -> Option<String> {
    let path = project_root.join("mustard.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("specLang")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Soft-warning helper. Returns `Some(line)` when both sides resolve and
/// disagree; `None` otherwise (matching pair, missing `mustard.json`, missing
/// `specLang`, or missing `spec.lang`). The returned string is meant to be
/// written to stderr — it never blocks validation.
fn check_spec_lang_against_mustard_json(
    spec_lang: Option<&str>,
    project_root: &Path,
) -> Option<String> {
    let spec = spec_lang?;
    let mustard = read_mustard_spec_lang(project_root)?;
    if spec.eq_ignore_ascii_case(&mustard) {
        None
    } else {
        Some(format!(
            "WARN: spec.lang={spec} differs from mustard.json#specLang={mustard}"
        ))
    }
}

/// Resolve `--spec PATH` to `(spec.md, spec_dir)`. Accepts a directory or a
/// direct `spec.md` path.
fn resolve_paths(spec_path: &Path) -> (PathBuf, PathBuf) {
    if spec_path.is_dir() {
        (spec_path.join("spec.md"), spec_path.to_path_buf())
    } else if spec_path.is_file() {
        let dir = spec_path
            .parent()
            .map_or_else(|| spec_path.to_path_buf(), Path::to_path_buf);
        (spec_path.to_path_buf(), dir)
    } else {
        // Treat as a slug under `.claude/spec/`.
        // ClaudePaths::for_spec failure (empty slug / traversal / separator)
        // degrades to an empty PathBuf — the downstream `read_to_string` will
        // surface "could not read spec.md" with that empty target.
        let project = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let slug = spec_path.to_string_lossy();
        let dir = ClaudePaths::for_project(&project)
            .ok()
            .and_then(|cp| cp.for_spec(slug.as_ref()).ok())
            .map(|sp| sp.dir().to_path_buf())
            .unwrap_or_default();
        (dir.join("spec.md"), dir)
    }
}

/// Reconstruct a [`SpecInput`] from the on-disk files.
fn build_input_from_files(slug: &str, spec_text: &str, meta: Option<&mustard_core::meta::Meta>) -> SpecInput {
    let title = extract_title(spec_text);
    let prd = collect_sections(spec_text, PRD_SECTIONS);
    let plan = collect_sections(spec_text, PLAN_SECTIONS);
    let acs = collect_acceptance_criteria(spec_text);

    let stage = meta
        .and_then(|m| m.stage.as_deref())
        .and_then(Stage::parse)
        .or(Some(Stage::Plan));
    let outcome = meta
        .and_then(|m| m.outcome.as_deref())
        .and_then(Outcome::parse)
        .or(Some(Outcome::Active));
    let phase = meta
        .and_then(|m| m.phase.as_deref())
        .and_then(Phase::parse);
    let scope = meta
        .and_then(|m| m.scope.as_deref())
        .and_then(Scope::parse);
    let lang = meta.and_then(|m| m.lang.clone());
    let total_waves = meta.and_then(|m| m.total_waves);

    SpecInput {
        slug: slug.to_string(),
        title,
        stage,
        outcome,
        phase,
        scope,
        lang,
        total_waves,
        prd_sections: prd,
        plan_sections: plan,
        acceptance_criteria: acs,
    }
}

/// Extract the `# Title` line from a spec body.
fn extract_title(text: &str) -> String {
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("# ") {
            return rest.trim().to_string();
        }
    }
    String::new()
}

/// Walk every canonical section name. For each one found in the spec body,
/// collect the body bytes up to the next `## ` heading.
fn collect_sections(text: &str, names: &[&str]) -> Vec<SectionBody> {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections: Vec<SectionBody> = Vec::new();
    for want in names {
        let want_lower = want.to_ascii_lowercase();
        let Some(start_idx) = lines.iter().position(|l| is_heading_match(l, &want_lower)) else {
            continue;
        };
        let mut end = lines.len();
        for (i, l) in lines.iter().enumerate().skip(start_idx + 1) {
            if l.starts_with("## ") {
                end = i;
                break;
            }
        }
        let body = lines[start_idx + 1..end].join("\n").trim().to_string();
        sections.push(SectionBody {
            name: (*want).to_string(),
            body,
        });
    }
    sections
}

/// Check `## <heading>` line matches `target` case-insensitively, accepting
/// both PT-BR and EN canonical wordings.
fn is_heading_match(line: &str, target_lower: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix("## ") else {
        return false;
    };
    let rest_lower = rest.trim().to_ascii_lowercase();
    if rest_lower.starts_with(target_lower) {
        return true;
    }
    // Match against the same heading equivalence table the existing
    // spec_sections module knows.
    is_heading(line, target_lower)
}

/// Parse Acceptance Criteria entries — lines under the AC section in the
/// canonical `- **AC-X** — statement.\n  Command: \`...\`` shape.
fn collect_acceptance_criteria(text: &str) -> Vec<AcceptanceCriterion> {
    let mut out: Vec<AcceptanceCriterion> = Vec::new();
    let mut in_section = false;
    let mut current: Option<AcceptanceCriterion> = None;
    for raw in text.lines() {
        let line = raw.trim_start();
        if line.starts_with("## ") {
            let header = line.to_ascii_lowercase();
            in_section = header.contains("crit") || header.contains("acceptance");
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(rest) = line.strip_prefix("- **") {
            if let Some(end) = rest.find("**") {
                let id = rest[..end].to_string();
                let after = rest[end + 2..].trim();
                let statement = after.trim_start_matches('—').trim_start_matches('-').trim().to_string();
                if let Some(prev) = current.take() {
                    out.push(prev);
                }
                current = Some(AcceptanceCriterion {
                    id,
                    statement,
                    command: String::new(),
                });
                continue;
            }
        }
        if line.starts_with("Command:") || line.starts_with("Comando:") {
            let after = line.split_once(':').map_or("", |(_, v)| v.trim());
            let cmd = after.trim_matches('`').to_string();
            if let Some(ac) = current.as_mut() {
                ac.command = cmd;
            }
            continue;
        }
        // Inline `Command:` on the same bullet line.
        if let Some(idx) = line.find("Command:") {
            let after = &line[idx + "Command:".len()..];
            let cmd = after.trim().trim_matches('`').to_string();
            if let Some(ac) = current.as_mut() {
                ac.command = cmd;
            }
        }
    }
    if let Some(prev) = current.take() {
        out.push(prev);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture_full_spec() -> String {
        let mut s = String::new();
        s.push_str("# Demo\n\n### Stage: Plan\n### Outcome: Active\n### Flags: \n\n<!-- PRD -->\n");
        for n in PRD_SECTIONS {
            s.push_str(&format!("\n## {n}\n\nbody\n"));
        }
        s.push_str("\n## Critérios de Aceitação (lista)\n\n");
        s.push_str("- **AC-1** — Build green.\n  Command: `rtk cargo build`\n");
        s.push_str("\n<!-- PLAN -->\n");
        for n in PLAN_SECTIONS {
            s.push_str(&format!("\n## {n}\n\nbody\n"));
        }
        s
    }

    #[test]
    fn collects_canonical_sections() {
        let body = fixture_full_spec();
        let prd = collect_sections(&body, PRD_SECTIONS);
        let plan = collect_sections(&body, PLAN_SECTIONS);
        assert_eq!(prd.len(), PRD_SECTIONS.len());
        assert_eq!(plan.len(), PLAN_SECTIONS.len());
    }

    #[test]
    fn collects_ac_with_command() {
        let acs = collect_acceptance_criteria(&fixture_full_spec());
        assert_eq!(acs.len(), 1);
        assert_eq!(acs[0].id, "AC-1");
        assert_eq!(acs[0].command, "rtk cargo build");
    }

    #[test]
    fn spec_lang_match_emits_no_warning() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            br#"{"specLang":"pt-BR"}"#,
        )
        .unwrap();
        let warn = check_spec_lang_against_mustard_json(Some("pt-BR"), dir.path());
        assert!(warn.is_none(), "matching lang should not warn, got {warn:?}");
    }

    #[test]
    fn spec_lang_mismatch_emits_warning_with_both_values() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            br#"{"specLang":"en-US"}"#,
        )
        .unwrap();
        let warn = check_spec_lang_against_mustard_json(Some("pt-BR"), dir.path())
            .expect("mismatch should produce warning");
        assert!(warn.contains("pt-BR"), "warning missing spec value: {warn}");
        assert!(warn.contains("en-US"), "warning missing mustard value: {warn}");
        assert!(warn.starts_with("WARN:"), "warning prefix missing: {warn}");
    }

    #[test]
    fn missing_mustard_json_is_silent() {
        let dir = tempdir().unwrap();
        // No mustard.json written at all.
        let warn = check_spec_lang_against_mustard_json(Some("pt-BR"), dir.path());
        assert!(warn.is_none(), "missing mustard.json must be silent, got {warn:?}");
    }

    #[test]
    fn malformed_mustard_json_is_silent() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), b"not json at all").unwrap();
        let warn = check_spec_lang_against_mustard_json(Some("pt-BR"), dir.path());
        assert!(warn.is_none(), "malformed mustard.json must be silent, got {warn:?}");
    }

    #[test]
    fn missing_spec_lang_is_silent() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            br#"{"specLang":"pt-BR"}"#,
        )
        .unwrap();
        // spec.lang absent → no comparison performed.
        let warn = check_spec_lang_against_mustard_json(None, dir.path());
        assert!(warn.is_none(), "absent spec.lang must be silent, got {warn:?}");
    }

    #[test]
    fn missing_spec_lang_key_in_mustard_json_is_silent() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), br#"{"tone":"didactic"}"#).unwrap();
        let warn = check_spec_lang_against_mustard_json(Some("pt-BR"), dir.path());
        assert!(warn.is_none(), "missing specLang key must be silent, got {warn:?}");
    }

    #[test]
    fn validates_a_fresh_drafted_spec() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("demo");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), fixture_full_spec()).unwrap();
        // Hand-write a valid meta.json (covers the Full scope path).
        std::fs::write(
            spec_dir.join("meta.json"),
            r#"{"stage":"Plan","outcome":"Active","phase":"PLAN","scope":"full","lang":"pt-BR","checkpoint":null,"isWavePlan":true,"totalWaves":1}"#,
        )
        .unwrap();
        // Re-create the `input` to verify it would validate via contract.
        let spec_text = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
        let meta = mustard_core::meta::read_meta_beside(&spec_dir.join("spec.md"));
        let input = build_input_from_files("demo", &spec_text, meta.as_ref());
        assert!(mustard_core::spec::contract::validate(&input).is_ok());
    }
}
