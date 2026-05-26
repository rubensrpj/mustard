//! `mustard-rt run skill-cache --check <slug>` — answer "is this skill installed?".
//!
//! Reads `.claude/.skill-cache.json` (written by `skill-fetch`) and
//! cross-checks the recorded `target` path actually exists on disk. The output
//! is a small JSON shape consumed by `/skill install` to short-circuit a
//! re-install when the user already has the same skill.

use crate::run::env::session_id;
use crate::util::now_iso8601;
use crate::run::skill_fetch::SkillCacheFile;
use mustard_core::fs::read_to_string;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::ClaudePaths;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Options for `mustard-rt run skill-cache`.
#[derive(Debug, Clone)]
pub struct SkillCacheOpts {
    pub check: String,
}

/// JSON report.
#[derive(Debug, Serialize)]
pub struct CacheCheckReport {
    pub slug: String,
    pub cached: bool,
    pub installed_on_disk: bool,
    pub target: Option<String>,
    pub entry: Option<Value>,
}

fn cache_path(cwd: &Path) -> PathBuf {
    ClaudePaths::for_project(cwd)
        .map(|p| p.skill_cache_path())
        .unwrap_or_default()
}

/// Pure inspector — returns the (cached entry, on-disk presence).
#[must_use]
pub fn inspect(cwd: &Path, slug: &str) -> CacheCheckReport {
    let cache: SkillCacheFile = read_to_string(cache_path(cwd))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let entry = cache.entries.get(slug).cloned();
    let target = entry.as_ref().and_then(|e| {
        e.get("target")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    let installed_on_disk = match target.as_deref() {
        Some(p) => Path::new(p).is_dir(),
        None => ClaudePaths::for_project(cwd)
            .map(|p| p.skills_dir().join(slug).is_dir())
            .unwrap_or(false),
    };
    CacheCheckReport {
        slug: slug.to_string(),
        cached: entry.is_some(),
        installed_on_disk,
        target,
        entry,
    }
}

/// CLI entry.
pub fn run(opts: SkillCacheOpts) {
    let started = std::time::Instant::now();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let report = inspect(&cwd, &opts.check);
    let body = serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string());
    println!("{body}");
    emit_economy(started.elapsed().as_millis());
}

fn emit_economy(duration_ms: u128) {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| ".".to_string());
    let duration_capped = i64::try_from(duration_ms).unwrap_or(i64::MAX);
    let ev = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        session_id: session_id(),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Orchestrator,
            id: Some("skill-cache".to_string()),
            actor_type: None,
        },
        event: "pipeline.economy.operation.invoked".to_string(),
        payload: json!({
            "operation": "skill-cache",
            "duration_ms": duration_capped,
            "tokens_used": 0,
            "was_rust_only": true,
        }),
        spec: None,
    };
    let _ = crate::run::event_route::emit(&cwd, &ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_cache(cwd: &Path, body: &str) {
        let p = cwd.join(".claude").join(".skill-cache.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
    }

    #[test]
    fn missing_cache_returns_negative_inspection() {
        let dir = tempdir().unwrap();
        let r = inspect(dir.path(), "foo");
        assert!(!r.cached);
        assert!(!r.installed_on_disk);
        assert!(r.target.is_none());
    }

    #[test]
    fn cached_slug_with_missing_target_reports_cached_but_not_installed() {
        let dir = tempdir().unwrap();
        write_cache(
            dir.path(),
            r#"{"entries":{"foo":{"target":"/does/not/exist","kind":"local"}}}"#,
        );
        let r = inspect(dir.path(), "foo");
        assert!(r.cached);
        assert!(!r.installed_on_disk);
        assert_eq!(r.target.as_deref(), Some("/does/not/exist"));
    }

    #[test]
    fn skills_folder_alone_marks_installed_on_disk() {
        let dir = tempdir().unwrap();
        let p = dir.path().join(".claude/skills/foo");
        std::fs::create_dir_all(&p).unwrap();
        let r = inspect(dir.path(), "foo");
        assert!(r.installed_on_disk);
    }

    #[test]
    fn json_shape_has_required_fields() {
        let r = inspect(&tempdir().unwrap().keep(), "ghost");
        let v = serde_json::to_value(r).unwrap();
        for f in ["slug", "cached", "installed_on_disk", "target", "entry"] {
            assert!(v.get(f).is_some(), "missing field {f}");
        }
    }
}
