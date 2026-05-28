//! `mustard-rt run wave-size-check` — a port of `scripts/wave-size-check.js`.
//!
//! Advisory audit of per-wave size inside a wave-plan. `exec-rewave-check` only
//! decomposes a flat spec; once a spec is a wave-plan nothing flags an
//! oversized individual wave. This audits each wave and WARNS (never blocks).
//!
//! Output: one JSON line. The `oversizedCount` field is parsed downstream, so
//! the shape is preserved exactly.
//!
//! Port note: the JS version shelled to `wave-tree.js` and `scope-decompose.js`.
//! Both are now in this binary — this port calls the Rust logic directly.

use crate::run::spec::scope_decompose::decide;
use crate::run::wave::wave_lib::{detect_role, parse_files_section};
use mustard_core::fs;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// Resolve the file-count threshold (default 10, floor 3).
fn resolve_limit() -> usize {
    std::env::var("MUSTARD_WAVE_SIZE_LIMIT")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .map_or(10, |n| if n < 3 { 3 } else { n as usize })
}

/// An enumerated wave folder.
struct WaveFolder {
    folder: String,
}

/// Enumerate wave folders for a spec dir, or `None` when it is not a wave-plan.
fn enumerate_waves(spec_dir: &Path) -> Option<Vec<WaveFolder>> {
    if !spec_dir.join("wave-plan.md").exists() {
        return None;
    }
    let mut folders: Vec<String> = fs::read_dir(spec_dir)
        .map(|entries| {
            entries
                .into_iter()
                .filter(|e| e.is_dir)
                .map(|e| e.file_name)
                .filter(|n| {
                    // `^wave-\d+`
                    let lower = n.to_lowercase();
                    lower.starts_with("wave-")
                        && lower[5..].chars().next().is_some_and(|c| c.is_ascii_digit())
                })
                .collect()
        })
        .unwrap_or_default();
    folders.sort_by_key(|f| wave_number_of(f).unwrap_or(0));
    if folders.is_empty() {
        return None;
    }
    Some(folders.into_iter().map(|folder| WaveFolder { folder }).collect())
}

/// Extract a wave number from a folder name.
fn wave_number_of(name: &str) -> Option<u32> {
    let start = name.find(|c: char| c.is_ascii_digit())?;
    let end = name[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(name.len(), |e| start + e);
    name[start..end].parse().ok()
}

/// Try to extract a wave's file list from `wave-plan.md` (for stub waves).
fn files_from_wave_plan(spec_dir: &Path, wave_num: Option<u32>) -> Option<Vec<String>> {
    let wave_num = wave_num?;
    let text = fs::read_to_string(spec_dir.join("wave-plan.md")).ok()?;
    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();

    // 1. `### Wave N` section → `Files (N): a, b, c`.
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if !is_wave_header(t, wave_num) {
            continue;
        }
        for next in lines.iter().skip(i + 1) {
            let l = next.trim();
            if l.starts_with("## ") || l.starts_with("### ") || l.starts_with("#### ") {
                break;
            }
            if let Some(rest) = strip_files_prefix(l) {
                let parts: Vec<String> = rest
                    .split(',')
                    .map(|s| s.trim().trim_matches('`').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !parts.is_empty() {
                    return Some(parts);
                }
            }
        }
    }

    // 2. table row `| W3 | ... |` with a file-list cell.
    for line in &lines {
        let t = line.trim();
        if !is_table_row_for_wave(t, wave_num) {
            continue;
        }
        let cells: Vec<&str> = t.split('|').map(str::trim).filter(|c| !c.is_empty()).collect();
        for c in cells {
            if (c.contains('/') || c.contains('\\')) && c.contains(',') {
                let parts: Vec<String> = c
                    .split(',')
                    .map(|s| s.trim().trim_matches('`').to_string())
                    .filter(|s| s.contains('/') || s.contains('\\'))
                    .collect();
                if !parts.is_empty() {
                    return Some(parts);
                }
            }
        }
    }
    None
}

/// `^#{2,4}\s*Wave\s*N\b`
fn is_wave_header(line: &str, wave_num: u32) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if !(2..=4).contains(&hashes) {
        return false;
    }
    let rest = line[hashes..].trim_start();
    let lower = rest.to_lowercase();
    let Some(after) = lower.strip_prefix("wave") else {
        return false;
    };
    let after = after.trim_start();
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok() == Some(wave_num)
        && after[digits.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

/// `^Files\s*\(\d+\)\s*:\s*(.+)$`
fn strip_files_prefix(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("Files").or_else(|| line.strip_prefix("files"))?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('(')?;
    let close = rest.find(')')?;
    if rest[..close].is_empty() || !rest[..close].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let rest = rest[close + 1..].trim_start();
    let rest = rest.strip_prefix(':')?;
    let body = rest.trim_start();
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

/// `^\|\s*W?N\b`
fn is_table_row_for_wave(line: &str, wave_num: u32) -> bool {
    let Some(rest) = line.strip_prefix('|') else {
        return false;
    };
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(['W', 'w']).unwrap_or(rest);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok() == Some(wave_num)
        && rest[digits.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

/// Audit a single wave.
fn audit_wave(wave: &WaveFolder, spec_dir: &Path, limit: usize) -> Value {
    let folder = &wave.folder;
    let wave_num = wave_number_of(folder);

    // Prefer the wave's own spec.md `## Files` section.
    let mut files: Option<Vec<String>> = None;
    let mut source: Option<&str> = None;
    let wave_spec_path = spec_dir.join(folder).join("spec.md");
    if wave_spec_path.exists() {
        if let Ok(text) = fs::read_to_string(&wave_spec_path) {
            if let Some(parsed) = parse_files_section(&text) {
                if !parsed.is_empty() {
                    files = Some(parsed);
                    source = Some("wave-spec");
                }
            }
        }
    }
    if files.is_none() {
        if let Some(plan_files) = files_from_wave_plan(spec_dir, wave_num) {
            if !plan_files.is_empty() {
                files = Some(plan_files);
                source = Some("wave-plan");
            }
        }
    }

    let Some(files) = files else {
        let status = if wave_spec_path.exists() {
            "unknown"
        } else {
            "stub"
        };
        return json!({ "wave": wave_num, "folder": folder, "status": status });
    };

    let file_count = files.len();
    let roles: BTreeSet<&str> = files.iter().map(|f| detect_role(f)).collect();
    let layer_count = if roles.len() == 1 && roles.contains("lib") {
        1
    } else {
        roles.len()
    };

    let decision = decide(&json!({
        "fileCount": file_count,
        "layerCount": layer_count,
        "newEntityCount": 0,
        "knowledgeMatches": [],
    }));

    let mut reasons: Vec<String> = Vec::new();
    if decision.get("decompose").and_then(Value::as_bool) == Some(true) {
        if let Some(reason) = decision.get("reason").and_then(Value::as_str) {
            reasons.push(reason.to_string());
        }
    }
    if file_count > limit {
        reasons.push(format!("file-count:{file_count}>{limit}"));
    }
    let oversized = !reasons.is_empty();

    json!({
        "wave": wave_num,
        "folder": folder,
        "fileCount": file_count,
        "layerCount": layer_count,
        "oversized": oversized,
        "reason": reasons.join("; "),
        "source": source,
    })
}

/// Dispatch `mustard-rt run wave-size-check`.
pub fn run(spec_dir_arg: Option<&str>) {
    let emit = |v: Value| println!("{v}");
    let Some(spec_dir_arg) = spec_dir_arg else {
        emit(json!({ "action": "skip", "reason": "no-spec-dir-arg" }));
        return;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let spec_dir = if Path::new(spec_dir_arg).is_absolute() {
        std::path::PathBuf::from(spec_dir_arg)
    } else {
        cwd.join(spec_dir_arg)
    };
    if !spec_dir.exists() {
        emit(json!({
            "action": "skip",
            "reason": "error-fallback",
            "error": "spec-dir-not-found",
        }));
        return;
    }

    let Some(waves) = enumerate_waves(&spec_dir) else {
        emit(json!({ "action": "skip", "reason": "not-a-wave-plan" }));
        return;
    };

    let limit = resolve_limit();
    let audited: Vec<Value> = waves.iter().map(|w| audit_wave(w, &spec_dir, limit)).collect();
    let oversized_count = audited
        .iter()
        .filter(|w| w.get("oversized").and_then(Value::as_bool) == Some(true))
        .count();

    emit(json!({
        "action": "audited",
        "specDir": spec_dir.to_string_lossy(),
        "limit": limit,
        "oversizedCount": oversized_count,
        "waves": audited,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_limit_floors_at_three() {
        // Default applies when env unset.
        assert!(resolve_limit() >= 3);
    }

    #[test]
    fn wave_number_extraction() {
        assert_eq!(wave_number_of("wave-3-backend"), Some(3));
        assert_eq!(wave_number_of("wave-12"), Some(12));
    }

    #[test]
    fn audits_wave_plan_with_oversized_wave() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::write(spec_dir.join("wave-plan.md"), "# plan\n").unwrap();
        let wave_dir = spec_dir.join("wave-1-backend");
        std::fs::create_dir_all(&wave_dir).unwrap();
        let mut files = String::from("## Files\n");
        for i in 0..14 {
            files.push_str(&format!("- src/api/h{i}.ts\n"));
        }
        std::fs::write(wave_dir.join("spec.md"), files).unwrap();
        let waves = enumerate_waves(spec_dir).unwrap();
        let audited = audit_wave(&waves[0], spec_dir, 10);
        assert_eq!(audited["oversized"], json!(true));
        assert_eq!(audited["fileCount"], json!(14));
    }

    #[test]
    fn not_a_wave_plan_skips() {
        let dir = tempdir().unwrap();
        assert!(enumerate_waves(dir.path()).is_none());
    }
}
