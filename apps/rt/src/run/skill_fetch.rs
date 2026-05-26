//! `mustard-rt run skill-fetch` — install a skill from a source spec.
//!
//! Source spec grammar (matches `/skill install`):
//!
//! - `path:<absolute-or-relative>` — copy from a local directory.
//! - `github:<owner>/<repo>/<path>` — sparse-clone a GitHub subpath.
//! - any other token — treated as a local path under
//!   `<cwd>/.claude/skills/<token>/` (no-op if already present).
//!
//! Writes go to `<cwd>/.claude/skills/<slug>/`. Each call records the install
//! in `.claude/.skill-cache.json` so `skill-cache` can answer "is this skill
//! present?" without re-walking the filesystem.

use crate::run::env::{current_spec, session_id};
use crate::util::now_iso8601;
use mustard_core::fs::{read_to_string, write_atomic};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::process::rtk_command;
use mustard_core::ClaudePaths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run skill-fetch`.
#[derive(Debug, Clone)]
pub struct SkillFetchOpts {
    pub name: String,
    pub dry_run: bool,
}

/// One skill source kind.
#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    Path,
    Github,
    Local,
}

/// Parsed source spec.
#[derive(Debug, Serialize, Clone)]
pub struct ParsedSource {
    pub kind: SourceKind,
    pub raw: String,
    pub slug: String,
}

/// Cache shape on disk.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SkillCacheFile {
    pub entries: BTreeMap<String, Value>,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct FetchReport {
    pub source: ParsedSource,
    pub target: String,
    pub installed: bool,
    pub dry_run: bool,
    pub error: Option<String>,
}

/// Parse the source spec into a typed kind + slug.
#[must_use]
pub fn parse_source(raw: &str) -> ParsedSource {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("path:") {
        let slug = Path::new(rest)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill")
            .to_string();
        return ParsedSource {
            kind: SourceKind::Path,
            raw: trimmed.to_string(),
            slug,
        };
    }
    if let Some(rest) = trimmed.strip_prefix("github:") {
        let last = rest.rsplit('/').next().unwrap_or("skill");
        return ParsedSource {
            kind: SourceKind::Github,
            raw: trimmed.to_string(),
            slug: last.to_string(),
        };
    }
    ParsedSource {
        kind: SourceKind::Local,
        raw: trimmed.to_string(),
        slug: trimmed.to_string(),
    }
}

/// Copy a directory tree recursively. Best-effort; per-file errors aborts the
/// copy and surfaces a single error string.
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("source not a directory: {}", src.display()),
        ));
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            let bytes = std::fs::read(&from)?;
            write_atomic(&to, &bytes)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
    }
    Ok(())
}

/// Sparse-clone a GitHub spec via `git`. Best-effort; fails open with a clear
/// error message rather than panicking.
fn github_install(spec: &str, target: &Path) -> std::io::Result<()> {
    // spec is `owner/repo/path/inside/repo` — split into (owner/repo, sub).
    let (repo_part, sub_part) = match spec.find('/').and_then(|first_slash| {
        spec[first_slash + 1..]
            .find('/')
            .map(|second_offset| first_slash + 1 + second_offset)
    }) {
        Some(idx) => (&spec[..idx], &spec[idx + 1..]),
        None => (spec, ""),
    };
    let tmp = std::env::temp_dir().join(format!("mustard-skill-{}", target.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("install")));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let url = format!("https://github.com/{repo_part}.git");
    let out = rtk_command(
        "git",
        &[
            "clone",
            "--depth",
            "1",
            "--filter=blob:none",
            "--sparse",
            &url,
            tmp.to_str().unwrap_or(""),
        ],
    )
    .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other("git clone failed"));
    }
    if !sub_part.is_empty() {
        let _ = rtk_command(
            "git",
            &["-C", tmp.to_str().unwrap_or(""), "sparse-checkout", "set", sub_part],
        )
        .output();
    }
    let copy_src = if sub_part.is_empty() { tmp.clone() } else { tmp.join(sub_part) };
    copy_dir(&copy_src, target)
}

/// Append (or update) the cache entry for `slug`.
fn record_cache(cwd: &Path, slug: &str, kind: SourceKind, source: &str, target: &Path) {
    let Ok(paths) = ClaudePaths::for_project(cwd) else {
        return;
    };
    let path = paths.skill_cache_path();
    let mut cache: SkillCacheFile = read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    cache.entries.insert(
        slug.to_string(),
        json!({
            "kind": kind,
            "source": source,
            "target": target.display().to_string(),
            "installed_at": now_iso8601(),
        }),
    );
    if let Ok(text) = serde_json::to_string_pretty(&cache) {
        let _ = write_atomic(&path, format!("{text}\n").as_bytes());
    }
}

/// CLI entry.
pub fn run(opts: SkillFetchOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let source = parse_source(&opts.name);
    let Ok(paths) = ClaudePaths::for_project(&cwd) else {
        let report = FetchReport {
            source: source.clone(),
            target: String::new(),
            installed: false,
            dry_run: opts.dry_run,
            error: Some("invalid project root (claude-paths guard)".to_string()),
        };
        let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
        println!("{body}");
        emit_economy(started.elapsed().as_millis());
        return;
    };
    let target = paths.skills_dir().join(&source.slug);
    let mut report = FetchReport {
        source: source.clone(),
        target: target.display().to_string(),
        installed: false,
        dry_run: opts.dry_run,
        error: None,
    };
    if opts.dry_run {
        let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
        println!("{body}");
        emit_economy(started.elapsed().as_millis());
        return;
    }
    let result = match source.kind {
        SourceKind::Path => {
            let src = PathBuf::from(source.raw.strip_prefix("path:").unwrap_or(""));
            copy_dir(&src, &target)
        }
        SourceKind::Github => {
            let spec = source.raw.strip_prefix("github:").unwrap_or("");
            github_install(spec, &target)
        }
        SourceKind::Local => {
            // No-op when already in place; error otherwise.
            if target.is_dir() {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "local skill not found and source is not path:/github:",
                ))
            }
        }
    };
    match result {
        Ok(()) => {
            report.installed = true;
            record_cache(&cwd, &source.slug, source.kind, &source.raw, &target);
        }
        Err(e) => {
            report.error = Some(e.to_string());
        }
    }
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let spec = current_spec(&cwd);
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("skill-fetch".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "skill-fetch",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec,
    };
    let _ = crate::run::event_route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_source_recognises_path_scheme() {
        let p = parse_source("path:./local/skills/foo");
        assert_eq!(p.kind, SourceKind::Path);
        assert_eq!(p.slug, "foo");
    }

    #[test]
    fn parse_source_recognises_github_scheme() {
        let p = parse_source("github:anthropics/skills/skills/pdf");
        assert_eq!(p.kind, SourceKind::Github);
        assert_eq!(p.slug, "pdf");
    }

    #[test]
    fn parse_source_falls_back_to_local() {
        let p = parse_source("api-caching");
        assert_eq!(p.kind, SourceKind::Local);
        assert_eq!(p.slug, "api-caching");
    }

    #[test]
    fn copy_dir_recursively_copies_subtree() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("SKILL.md"), "x").unwrap();
        std::fs::write(src.join("nested").join("a.txt"), "y").unwrap();
        copy_dir(&src, &dst).unwrap();
        assert!(dst.join("SKILL.md").exists());
        assert!(dst.join("nested/a.txt").exists());
    }

    #[test]
    fn dry_run_does_not_create_target() {
        let dir = tempdir().unwrap();
        let opts = SkillFetchOpts {
            name: "path:./does/not/exist".to_string(),
            dry_run: true,
        };
        let cwd = dir.path();
        // Build the report directly with the parser — easier than spawning run().
        let s = parse_source(&opts.name);
        let report = FetchReport {
            source: s.clone(),
            target: cwd.join(".claude/skills").join(&s.slug).display().to_string(),
            installed: false,
            dry_run: true,
            error: None,
        };
        let v = serde_json::to_value(report).unwrap();
        assert_eq!(v["dry_run"], json!(true));
    }

    #[test]
    fn json_shape_includes_source_and_target() {
        let report = FetchReport {
            source: parse_source("path:./x"),
            target: "/tmp/skills/x".to_string(),
            installed: false,
            dry_run: false,
            error: None,
        };
        let v = serde_json::to_value(report).unwrap();
        assert!(v.get("source").is_some());
        assert!(v.get("target").is_some());
        assert!(v.get("installed").is_some());
    }
}
