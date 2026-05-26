//! `mustard-rt run spec-memory` — manage the per-spec `memory/` directory.
//!
//! Wave 1.T1.9 introduces `memory/{name}.md` files inside an active spec,
//! carrying technical principles, processes, or references that emerge mid-
//! PLAN / EXECUTE. Each file ships with frontmatter (`name`, `kind`,
//! `origin_spec`, `origin_wave`) plus automatic `[[ ]]` wirelinks back to
//! the spec and the wave of origin, and the canonical
//! Origin / Applies-to / Status / Related sections — whose spelling comes
//! from `mustard_core::i18n` (W1.T1.0 — no hardcoded pt-BR strings).

use crate::run::env::project_dir;
use mustard_core::fs as mfs;
use mustard_core::i18n::{project_locale, translate, Locale};
use serde_json::json;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run spec-memory create`.
pub struct SpecMemoryCreateOpts {
    /// Spec slug under `.claude/spec/`.
    pub spec: String,
    /// Memory entry name (kebab-case).
    pub name: String,
    /// One of `principle` / `process` / `reference`.
    pub kind: String,
    /// Wave of origin label (e.g. `wave-1-mixed`). Optional.
    pub origin_wave: Option<String>,
    /// One-line description (becomes the file title + `description:` frontmatter
    /// field). Optional.
    pub description: Option<String>,
}

/// Dispatch the verb (currently only `create`).
pub fn dispatch(subcommand: Option<&str>, opts: SpecMemoryCreateOpts) {
    match subcommand.unwrap_or("create") {
        "create" => create(opts),
        other => {
            let body = json!({
                "ok": false,
                "error": "unknown subcommand",
                "detail": other,
            });
            println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into()));
        }
    }
}

fn create(opts: SpecMemoryCreateOpts) {
    create_with_root(opts, &PathBuf::from(project_dir()));
}

/// Same as [`create`] but with an explicit project root, so tests can inject
/// a temp directory without mutating the process environment (which would be
/// `unsafe` under edition 2024).
fn create_with_root(opts: SpecMemoryCreateOpts, project: &Path) {
    if opts.spec.trim().is_empty() {
        emit_error("missing --spec", "");
        return;
    }
    if opts.name.trim().is_empty() {
        emit_error("missing --name", "");
        return;
    }
    let kind = opts.kind.trim().to_ascii_lowercase();
    let kind_ok = matches!(kind.as_str(), "principle" | "process" | "reference");
    if !kind_ok {
        emit_error("invalid --kind", "expected principle|process|reference");
        return;
    }

    let spec_dir = project.join(".claude").join("spec").join(&opts.spec);
    if !spec_dir.exists() {
        emit_error("spec directory does not exist", &spec_dir.display().to_string());
        return;
    }
    let memory_dir = spec_dir.join("memory");
    if let Err(e) = mfs::create_dir_all(&memory_dir) {
        emit_error("could not create memory directory", &e.to_string());
        return;
    }
    let target = memory_dir.join(format!("{}.md", opts.name));
    if target.exists() {
        emit_error(
            "memory file exists",
            &target.display().to_string(),
        );
        return;
    }
    let lang = project_locale(project);
    let body = render_template(&opts, &kind, lang);
    if let Err(e) = mfs::write_atomic(&target, body.as_bytes()) {
        emit_error("write failed", &e.to_string());
        return;
    }

    // Append a row to the `memory/_index.md` index when present, so the
    // newly-added file shows up alongside the existing entries. Best-effort:
    // failure to append is logged but never fatal.
    if let Err(e) = append_index_row(&memory_dir, &opts, &kind, lang) {
        eprintln!("spec-memory: WARN: could not update _index.md — {e}");
    }

    let report = json!({
        "ok": true,
        "spec": opts.spec,
        "name": opts.name,
        "kind": kind,
        "path": target.display().to_string(),
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
}

fn render_template(opts: &SpecMemoryCreateOpts, kind: &str, lang: Locale) -> String {
    let title = opts
        .description
        .clone()
        .unwrap_or_else(|| opts.name.replace('-', " "));
    let wave_link = opts
        .origin_wave
        .as_deref()
        .map_or_else(|| translate("memory.origin.wave_unknown", lang).to_string(), |w| format!("[[{w}]]"));
    let mut body = String::new();
    body.push_str("---\n");
    let _ = writeln!(body, "name: {}", opts.name);
    let _ = writeln!(body, "kind: {kind}");
    let _ = writeln!(body, "origin_spec: {}", opts.spec);
    if let Some(w) = &opts.origin_wave {
        let _ = writeln!(body, "origin_wave: {w}");
    }
    if let Some(d) = &opts.description {
        let _ = writeln!(body, "description: {d}");
    }
    body.push_str("---\n\n");
    let _ = write!(body, "# {title}\n\n");
    let _ = write!(body, "{}\n\n", translate("placeholder.fill_first_line", lang));

    // Origin section — the catalogue carries both `{spec}` and `{wave}`
    // placeholders inside the `memory.intro.born_during` key; we interpolate
    // them here. PT-BR / EN-US produce different intros from the same key.
    let _ = write!(body, "## {}\n\n", translate("heading.memory.origin", lang));
    let intro = translate("memory.intro.born_during", lang)
        .replace("{spec}", &opts.spec)
        .replace("{wave}", &wave_link);
    let _ = write!(body, "{intro}\n\n");

    let _ = write!(body, "## {}\n\n", translate("heading.memory.applies_to", lang));
    let _ = write!(body, "{}\n\n", translate("placeholder.fill_who_files", lang));
    let _ = write!(body, "## {}\n\n", translate("heading.memory.status", lang));
    let _ = write!(body, "{}\n\n", translate("memory.status.active", lang));
    let _ = write!(body, "## {}\n\n", translate("heading.memory.related", lang));
    let _ = writeln!(body, "{}", translate("placeholder.fill_wirelinks", lang));
    body
}

fn append_index_row(
    memory_dir: &Path,
    opts: &SpecMemoryCreateOpts,
    _kind: &str,
    lang: Locale,
) -> Result<(), String> {
    let index_path = memory_dir.join("_index.md");
    let existing = mfs::read_to_string(&index_path).unwrap_or_default();
    // Update the principle count if present (best-effort; format-tolerant).
    let title = translate("memory.index.title", lang).replace("{title}", &opts.spec);
    let principles_heading = translate("heading.memory.principles", lang);
    let file_col = translate("memory.index.column.file", lang);
    let wave_col = translate("memory.index.column.wave", lang);
    let mut new_body = if existing.is_empty() {
        format!(
            "# {title}\n\n## {principles_heading}\n\n| {file_col} | {wave_col} |\n|---------|------|\n"
        )
    } else {
        existing
    };
    let wave = opts.origin_wave.clone().unwrap_or_else(|| "—".to_string());
    let _ = writeln!(new_body, "| [[{}]] | [[{wave}]] |", opts.name);
    mfs::write_atomic(&index_path, new_body.as_bytes()).map_err(|e| e.to_string())
}

fn emit_error(reason: &str, detail: &str) {
    let body = json!({
        "ok": false,
        "error": reason,
        "detail": detail,
    });
    println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn renders_template_with_frontmatter_and_sections() {
        let opts = SpecMemoryCreateOpts {
            spec: "demo".into(),
            name: "scan-rust-first".into(),
            kind: "principle".into(),
            origin_wave: Some("wave-3-mixed".into()),
            description: Some("Scan structural in Rust".into()),
        };
        // pt-BR locale (project default) — headings use the PT-BR catalogue.
        let body = render_template(&opts, "principle", Locale::PtBr);
        assert!(body.contains("name: scan-rust-first"));
        assert!(body.contains("kind: principle"));
        assert!(body.contains("origin_wave: wave-3-mixed"));
        assert!(body.contains("[[demo]]"));
        assert!(body.contains("[[wave-3-mixed]]"));
        assert!(body.contains("## Origem"));
        assert!(body.contains("## Aplica-se a"));
        assert!(body.contains("## Status"));
        assert!(body.contains("## Relacionado"));
        assert!(body.contains("Nasceu durante"));
    }

    #[test]
    fn renders_template_in_en_us() {
        let opts = SpecMemoryCreateOpts {
            spec: "demo".into(),
            name: "alpha".into(),
            kind: "principle".into(),
            origin_wave: Some("wave-1-mixed".into()),
            description: None,
        };
        let body = render_template(&opts, "principle", Locale::EnUs);
        // EN headings + EN intro line — never the PT catalogue.
        assert!(body.contains("## Origin"));
        assert!(body.contains("## Applies to"));
        assert!(body.contains("## Status"));
        assert!(body.contains("## Related"));
        assert!(body.contains("Born during"));
        assert!(!body.contains("## Origem"));
        assert!(!body.contains("Nasceu durante"));
    }

    #[test]
    fn create_writes_file_under_memory_dir() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join(".claude").join("spec").join("demo");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // Inject the project root directly — `std::env::set_var` is `unsafe`
        // under edition 2024 and the crate forbids `unsafe`.
        let opts = SpecMemoryCreateOpts {
            spec: "demo".into(),
            name: "alpha".into(),
            kind: "principle".into(),
            origin_wave: Some("wave-1-mixed".into()),
            description: None,
        };
        create_with_root(opts, dir.path());
        let target = spec_dir.join("memory").join("alpha.md");
        assert!(target.exists());
    }
}
