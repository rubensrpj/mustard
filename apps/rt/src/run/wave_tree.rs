//! `mustard-rt run wave-tree` — a port of `scripts/wave-tree.js`.
//!
//! Renders an ASCII or JSON tree of wave status for a given spec-dir.
//!
//! - `<dir>/wave-plan.md` exists → parse the table, render each wave folder.
//! - else `<dir>/spec.md` exists → single-spec line.
//! - else → empty.
//!
//! The `--format json` shape is parsed by `wave-size-check`, so it is preserved
//! exactly: `{ kind, root, waves: [{ label, folder, status, icon }] }`.

use mustard_core::spec;
use serde_json::json;
use std::path::Path;

/// Map a status string to its icon.
fn icon_for(status: &str) -> &'static str {
    match status.to_lowercase().trim() {
        "completed" => "[v]",
        "implementing" => "[>]",
        "closed-followup" => "[~]",
        "blocked" | "rejected" => "[!]",
        "" => "[ ]",
        _ => "[ ]",
    }
}

/// A parsed wave row.
struct Wave {
    label: String,
    folder: String,
    status: String,
    icon: String,
}

/// Read the lifecycle status word from a spec file, defaulting to `"queued"`.
///
/// Delegates to the canonical [`mustard_core::spec`] parser (tolerant of
/// the new `### Stage:`/`### Outcome:`/`### Flags:` header **and** every legacy
/// shape) and projects the resulting [`SpecState`] to the legacy status word
/// the icon map keys off. A missing file / unparseable header → `"queued"`
/// (a not-yet-started wave), exactly as the old inline parser defaulted.
fn read_status(spec_file: &Path) -> String {
    match spec::read_state(spec_file) {
        Some(state) => spec::status_word(&state).to_string(),
        None => "queued".to_string(),
    }
}

/// Whether a token looks like a wave folder name.
fn looks_like_folder(c: &str) -> bool {
    let lower = c.to_lowercase();
    let wave_prefixed = lower.starts_with("wave-")
        && lower[5..].chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false);
    if wave_prefixed {
        return true;
    }
    // `^[a-z0-9][-_a-z0-9]+$`
    let mut chars = lower.chars();
    match chars.next() {
        Some(c0) if c0.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    let rest: Vec<char> = chars.collect();
    !rest.is_empty()
        && rest
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
}

/// First digit-run in a string.
fn first_number(s: &str) -> Option<String> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let end = s[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|e| start + e)
        .unwrap_or(s.len());
    Some(s[start..end].to_string())
}

/// Parse a `wave-plan.md` table into waves and resolve folders on disk.
fn parse_wave_plan(wave_plan_file: &Path, spec_dir: &Path) -> Vec<Wave> {
    let Ok(content) = std::fs::read_to_string(wave_plan_file) else {
        return Vec::new();
    };
    let mut waves: Vec<(String, String)> = Vec::new();
    for line in content.split(['\n']).map(|l| l.trim_end_matches('\r')) {
        // `^\|\s*(W?\d+|Wave\s*\d+)\s*\|(.+)$`
        let Some(rest) = line.strip_prefix('|') else {
            continue;
        };
        let rest = rest.trim_start();
        let lower = rest.to_lowercase();
        let label_end = rest.find('|');
        let Some(label_end) = label_end else {
            continue;
        };
        let label_cell = rest[..label_end].trim();
        // label cell must be `W?\d+` or `Wave\s*\d+`.
        let label_ok = {
            let lc = label_cell.to_lowercase();
            let body = lc.strip_prefix('w').map(str::trim_start).unwrap_or(&lc);
            let body = body.strip_prefix("ave").map(str::trim_start).unwrap_or(body);
            !body.is_empty() && body.chars().all(|c| c.is_ascii_digit())
        };
        if !label_ok {
            continue;
        }
        let _ = lower;
        let label = label_cell.to_string();
        let body = &rest[label_end + 1..];
        let cells: Vec<&str> = body
            .split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        let mut folder: Option<String> = None;
        for c in cells.iter().rev() {
            if looks_like_folder(c) {
                folder = Some((*c).to_string());
                break;
            }
        }
        let folder = folder.unwrap_or_else(|| match first_number(&label) {
            Some(n) => format!("wave-{n}"),
            None => label.to_lowercase().replace(' ', "-"),
        });
        waves.push((label, folder));
    }

    // Resolve actual folders on disk.
    let entries: Vec<String> = std::fs::read_dir(spec_dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();

    waves
        .into_iter()
        .map(|(label, mut folder)| {
            if !entries.iter().any(|e| e == &folder) {
                if let Some(num) = first_number(&folder).or_else(|| first_number(&label)) {
                    let prefix = format!("wave-{num}");
                    if let Some(m) = entries.iter().find(|e| {
                        let el = e.to_lowercase();
                        el == prefix
                            || el.strip_prefix(&prefix).map(|t| {
                                t.starts_with('-') || t.starts_with('_')
                            }).unwrap_or(false)
                    }) {
                        folder = m.clone();
                    }
                }
            }
            let spec_file = spec_dir.join(&folder).join("spec.md");
            let status = read_status(&spec_file);
            let icon = icon_for(&status).to_string();
            Wave {
                label,
                folder,
                status,
                icon,
            }
        })
        .collect()
}

/// Render the ASCII tree.
fn render_ascii(root: &str, waves: &[Wave]) -> String {
    let max_len = waves.iter().map(|w| w.folder.len()).max().unwrap_or(0);
    let mut lines = vec![format!("Roadmap: {root}")];
    for (i, w) in waves.iter().enumerate() {
        let branch = if i == waves.len() - 1 { "└─" } else { "├─" };
        let pad = " ".repeat(max_len.saturating_sub(w.folder.len()) + 2);
        lines.push(format!(
            "{branch} {} {}{}({})",
            w.icon, w.folder, pad, w.status
        ));
    }
    lines.join("\n")
}

/// Dispatch `mustard-rt run wave-tree`.
pub fn run(spec_dir: &str, format: &str) {
    let dir = std::fs::canonicalize(spec_dir)
        .unwrap_or_else(|_| std::path::PathBuf::from(spec_dir));
    if !dir.exists() {
        println!("(no spec at {spec_dir})");
        return;
    }
    let root = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let wave_plan = dir.join("wave-plan.md");
    let single_spec = dir.join("spec.md");

    if wave_plan.exists() {
        let waves = parse_wave_plan(&wave_plan, &dir);
        if format == "json" {
            let waves_json: Vec<_> = waves
                .iter()
                .map(|w| {
                    json!({
                        "label": w.label,
                        "folder": w.folder,
                        "status": w.status,
                        "icon": w.icon,
                    })
                })
                .collect();
            println!(
                "{}",
                json!({ "kind": "wave-plan", "root": root, "waves": waves_json })
            );
        } else {
            println!("{}", render_ascii(&root, &waves));
        }
        return;
    }
    if single_spec.exists() {
        let status = read_status(&single_spec);
        let icon = icon_for(&status);
        if format == "json" {
            println!(
                "{}",
                json!({
                    "kind": "single",
                    "root": root,
                    "spec": { "name": root, "status": status, "icon": icon },
                })
            );
        } else {
            println!("Spec: {root}  {icon} ({status})");
        }
        return;
    }
    if format == "json" {
        println!("{}", json!({ "kind": "empty", "root": root, "waves": [] }));
    } else {
        println!("(no spec at {spec_dir})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn icon_mapping() {
        assert_eq!(icon_for("completed"), "[v]");
        assert_eq!(icon_for("blocked"), "[!]");
        assert_eq!(icon_for("queued"), "[ ]");
    }

    #[test]
    fn read_status_parses_header() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, "# Spec\n### Status: completed\n").unwrap();
        assert_eq!(read_status(&path), "completed");
    }

    #[test]
    fn parse_wave_plan_reads_table_rows() {
        let dir = tempdir().unwrap();
        let plan = dir.path().join("wave-plan.md");
        std::fs::write(&plan, "| Wave 1 | backend | wave-1-backend |\n").unwrap();
        std::fs::create_dir_all(dir.path().join("wave-1-backend")).unwrap();
        std::fs::write(
            dir.path().join("wave-1-backend").join("spec.md"),
            "### Status: completed\n",
        )
        .unwrap();
        let waves = parse_wave_plan(&plan, dir.path());
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].folder, "wave-1-backend");
        assert_eq!(waves[0].status, "completed");
    }
}
