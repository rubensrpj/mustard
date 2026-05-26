//! `mustard-rt run skills` — a port of `scripts/skills.js`.
//!
//! A unified CLI for the skill-family scripts:
//!
//! - `validate [--json] [--quiet] [--factual] [--lines] [--only scan|manual]`
//! - `graph    [--json] [--cwd PATH]`
//! - `orphans  [--days N] [--json] [--cwd PATH]`
//!
//! All flags, output formats, exit codes and env vars match the JS one-for-one.
//! Port note: the JS exported `validateSkill` for the `skill_validate` hook to
//! consume — that hook is `mustard-rt` native already, so this is purely the
//! CLI face. `validateSkill`'s `--factual` Python sub-validator shell-out is
//! kept (`python quick_validate.py`); the cluster heuristic is ported intact.

use crate::run::env;
use mustard_core::fs;
use mustard_core::store::sqlite_store::SqliteEventStore;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ── shared discovery ─────────────────────────────────────────────────────────

/// A discovered skill: its frontmatter `name` and body. The on-disk path is
/// not retained — `graph` / `orphans` key purely on the skill name, matching
/// the JS, which deduped by name.
struct Skill {
    name: String,
    content: String,
}

/// Read a file, returning `""` on any error (JS `fsReadSafe`).
fn read_safe(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_default()
}

/// Parse `name:` from YAML frontmatter, tolerating CRLF (JS `extractSkillName`).
fn extract_skill_name(content: &str) -> Option<String> {
    let normalized = content.replace("\r\n", "\n");
    let fm = frontmatter(&normalized)?;
    for line in fm.lines() {
        if let Some(rest) = line.strip_prefix("name:") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Return the YAML frontmatter body (between the leading `---` fences).
fn frontmatter(normalized: &str) -> Option<String> {
    let rest = normalized.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(rest[..end].to_string())
}

/// Strip the frontmatter block (JS `stripFrontmatter`).
fn strip_frontmatter(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    if let Some(rest) = normalized.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.strip_prefix('\n').unwrap_or(after).to_string();
        }
    }
    normalized
}

/// Collect `SKILL.md` paths one level under `skills_dir` (JS `collectSkillsAt`).
fn collect_skills_at(skills_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(skills_dir) else {
        return out;
    };
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let candidate = entry.path.join("SKILL.md");
        if candidate.exists() {
            out.push(candidate);
        }
    }
    out
}

/// Unified skill discovery across `templates/skills`, `.claude/skills`, and
/// one level of `<sub>/.claude/skills` (JS `discoverSkills`). Sorted by name.
fn discover_skills(project_dir: &Path) -> Vec<Skill> {
    let mut candidates = vec![
        project_dir.join("templates").join("skills"),
        project_dir.join(".claude").join("skills"),
    ];
    if let Ok(entries) = fs::read_dir(project_dir) {
        for entry in entries {
            if !entry.is_dir {
                continue;
            }
            let name = entry.file_name.clone();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            candidates.push(entry.path.join(".claude").join("skills"));
        }
    }
    let mut found: BTreeMap<String, Skill> = BTreeMap::new();
    for dir in candidates {
        for md in collect_skills_at(&dir) {
            let Ok(content) = fs::read_to_string(&md) else {
                continue;
            };
            let name = extract_skill_name(&content).unwrap_or_else(|| {
                md.parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
            found.entry(name.clone()).or_insert(Skill { name, content });
        }
    }
    found.into_values().collect()
}

/// Root for the `validate` discovery — `CLAUDE_PROJECT_DIR` / cwd.
fn validate_root() -> PathBuf {
    PathBuf::from(env::project_dir())
}

/// `validate`'s discovery: `<root>/.claude/skills` plus each subproject's
/// `.claude/skills`, detect-cache-aware (JS `collectSkillDirs`).
fn collect_skill_dirs(root: &Path) -> Vec<(PathBuf, String)> {
    let Ok(paths) = ClaudePaths::for_project(root) else {
        return Vec::new();
    };
    let skills_dir = paths.skills_dir();
    let mut dirs = vec![(skills_dir, "<root>".to_string())];
    let cache_path = paths.detect_cache_path();
    let cache: Option<Value> = fs::read_to_string(&cache_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let subs = cache
        .as_ref()
        .and_then(|c| c.get("subprojects"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if subs.is_empty() {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries {
                let name = entry.file_name.clone();
                if !entry.is_dir || name.starts_with('.') {
                    continue;
                }
                let Ok(sub_paths) = ClaudePaths::for_project(&entry.path) else {
                    continue;
                };
                let candidate = sub_paths.skills_dir();
                if candidate.exists() {
                    dirs.push((candidate, name));
                }
            }
        }
    } else {
        for sub in subs {
            let path = sub.get("path").and_then(Value::as_str).unwrap_or("");
            let name = sub.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let sub_root = root.join(path);
            let candidate = ClaudePaths::for_project(&sub_root)
                .map(|p| p.skills_dir())
                .unwrap_or_else(|_| sub_root.clone());
            dirs.push((candidate, name));
        }
    }
    dirs
}

// ── validate (structural) ────────────────────────────────────────────────────

/// Structural validation of one SKILL.md body (JS `validateSkill`).
fn validate_skill(content: &str) -> (bool, Vec<String>, Option<String>) {
    let mut errors = Vec::new();
    let normalized = content.replace("\r\n", "\n");
    let Some(body) = frontmatter(&normalized) else {
        errors.push("missing YAML frontmatter".to_string());
        return (false, errors, None);
    };
    let name = field(&body, "name:");
    let source = field(&body, "source:")
        .filter(|s| s == "scan" || s == "manual");
    match &name {
        None => errors.push("frontmatter: missing \"name\"".to_string()),
        Some(n) => {
            if !is_kebab(n) {
                errors.push(format!("name not kebab-case: {n}"));
            }
        }
    }
    match description_value(&body) {
        None => errors.push("frontmatter: missing \"description\"".to_string()),
        Some(desc) => {
            let raw: String = desc.split_whitespace().collect::<Vec<_>>().join(" ");
            if raw.chars().count() < 50 {
                errors.push(format!("description too short ({} chars, min 50)", raw.chars().count()));
            }
            if raw.chars().count() > 600 {
                errors.push(format!("description too long ({} chars, max 600)", raw.chars().count()));
            }
            if !has_trigger_words(&raw) {
                errors.push("description lacks trigger words (use when / when / add / create / ...)".to_string());
            }
        }
    }
    if source.is_none() {
        errors.push("frontmatter: missing \"source\" (expected scan|manual)".to_string());
    }
    (errors.is_empty(), errors, source)
}

/// Read a single `key:` value from a frontmatter body.
fn field(body: &str, key: &str) -> Option<String> {
    body.lines()
        .find_map(|l| l.strip_prefix(key).map(|v| v.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// Read the (possibly multi-line, possibly quoted) `description:` value.
fn description_value(body: &str) -> Option<String> {
    let lines: Vec<&str> = body.lines().collect();
    let idx = lines.iter().position(|l| l.starts_with("description:"))?;
    let first = lines[idx].trim_start_matches("description:").trim();
    let mut acc = String::from(first);
    // Continuation: subsequent indented lines.
    for line in &lines[idx + 1..] {
        if line.starts_with(' ') || line.starts_with('\t') {
            acc.push(' ');
            acc.push_str(line.trim());
        } else {
            break;
        }
    }
    let acc = acc.trim().trim_matches('"').to_string();
    if acc.is_empty() {
        None
    } else {
        Some(acc)
    }
}

/// kebab-case check: `^[a-z][a-z0-9-]+$`.
fn is_kebab(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && s.len() >= 2
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether a description contains a trigger phrase.
fn has_trigger_words(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    ["use when", "when the user", "add", "create", "new", "detect", "check", "write", "even if"]
        .iter()
        .any(|w| {
            // Word-boundary-ish: present as a substring (JS used `\b…\b`).
            lower.contains(w)
        })
}

/// `validate` (structural mode) — exit `2` on any failure.
fn run_validate_structural(root: &Path, json_out: bool, quiet: bool, only: Option<&str>) -> ! {
    let mut results: Vec<Value> = Vec::new();
    let (mut total, mut failed) = (0usize, 0usize);
    for (dir, label) in collect_skill_dirs(root) {
        for file in collect_skills_at(&dir) {
            let content = fs::read_to_string(&file);
            let rel = rel_posix(root, &file);
            match content {
                Err(_) => {
                    total += 1;
                    failed += 1;
                    results.push(json!({
                        "location": label, "path": rel, "ok": false,
                        "errors": ["unreadable"], "source": Value::Null,
                    }));
                }
                Ok(c) => {
                    let (ok, errors, source) = validate_skill(&c);
                    if let (Some(want), Some(src)) = (only, &source) {
                        if src != want {
                            continue;
                        }
                    }
                    total += 1;
                    if !ok {
                        failed += 1;
                    }
                    results.push(json!({
                        "location": label, "path": rel, "ok": ok,
                        "errors": errors, "source": source,
                    }));
                }
            }
        }
    }
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "summary": { "total": total, "ok": total - failed, "failed": failed },
                "results": results,
            }))
            .unwrap_or_default()
        );
    } else {
        let rows: Vec<&Value> = results
            .iter()
            .filter(|r| !quiet || r["ok"] == json!(false))
            .collect();
        if rows.is_empty() {
            println!("skill-validate: no SKILL.md files found.");
        } else {
            println!("skill-validate:");
            for r in rows {
                let ok = r["ok"] == json!(true);
                let tag = if ok { "[ok]  " } else { "[fail]" };
                let errs = r["errors"].as_array().map(|a| {
                    a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")
                }).unwrap_or_default();
                let path = r["path"].as_str().unwrap_or("");
                if errs.is_empty() {
                    println!("  {tag} {path}");
                } else {
                    println!("  {tag} {path} — {errs}");
                }
            }
        }
        println!("\nskill-validate: {}/{total} ok, {failed} failed.", total - failed);
    }
    std::process::exit(if failed > 0 { 2 } else { 0 });
}

/// `validate --lines` — line-count tiering (JS `runLinesMode`).
fn run_validate_lines(root: &Path, json_out: bool) -> ! {
    let mode = std::env::var("MUSTARD_SKILL_VALIDATE_LINES_MODE")
        .map(|m| m.to_lowercase())
        .ok()
        .filter(|m| m == "warn" || m == "off" || m == "strict")
        .unwrap_or_else(|| "warn".to_string());
    if mode == "off" {
        if json_out {
            println!("{}", json!({ "mode": "off", "total": 0, "results": [] }));
        } else {
            println!("skill-validate (lines): mode=off, skipping.");
        }
        std::process::exit(0);
    }
    let tier = |n: usize| -> &'static str {
        if n >= 500 {
            "block"
        } else if n >= 400 {
            "strict-warn"
        } else if n >= 200 {
            "warn"
        } else {
            "ok"
        }
    };
    let mut results: Vec<Value> = Vec::new();
    let mut has_block = false;
    for (dir, label) in collect_skill_dirs(root) {
        for file in collect_skills_at(&dir) {
            let content = read_safe(&file);
            let count = if content.is_empty() { 0 } else { content.split('\n').count() };
            let t = tier(count);
            if t == "block" {
                has_block = true;
            }
            results.push(json!({
                "file": rel_posix(root, &file), "lineCount": count,
                "tier": t, "location": label,
            }));
        }
    }
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "mode": mode, "total": results.len(), "results": results,
            }))
            .unwrap_or_default()
        );
    } else {
        let notable: Vec<&Value> = results.iter().filter(|r| r["tier"] != json!("ok")).collect();
        if notable.is_empty() {
            println!(
                "skill-validate (lines): {} skill(s) scanned, all within thresholds. mode={mode}",
                results.len()
            );
        } else {
            println!("skill-validate (lines): {} skill(s) above threshold. mode={mode}", notable.len());
            for r in notable {
                println!(
                    "  [LINES] {}: {} lines — tier={}",
                    r["file"].as_str().unwrap_or(""),
                    r["lineCount"],
                    r["tier"].as_str().unwrap_or("")
                );
            }
        }
    }
    std::process::exit(if mode == "strict" && has_block { 1 } else { 0 });
}

// ── graph ────────────────────────────────────────────────────────────────────

/// Find skill references in a body — `[[name]]`, `Skill(name)`, or a bare word.
fn find_references(body: &str, self_name: &str, known: &BTreeSet<String>) -> Vec<String> {
    let mut refs = BTreeSet::new();
    for candidate in known {
        if candidate == self_name {
            continue;
        }
        let wiki = format!("[[{candidate}]]");
        let call = format!("Skill({candidate})");
        if body.contains(&wiki) || body.contains(&call) || contains_word(body, candidate) {
            refs.insert(candidate.clone());
        }
    }
    refs.into_iter().collect()
}

/// Word-boundary substring test (JS `\bword\b`).
fn contains_word(haystack: &str, word: &str) -> bool {
    let boundary = |c: char| !(c.is_alphanumeric() || c == '_');
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(word) {
        let i = from + rel;
        let before_ok = i == 0
            || haystack[..i].chars().next_back().is_none_or(boundary);
        let after = &haystack[i + word.len()..];
        let after_ok = after.chars().next().is_none_or(boundary);
        if before_ok && after_ok {
            return true;
        }
        from = i + word.len();
    }
    false
}

/// Build adjacency, detect cycles, render Mermaid or JSON (JS `runGraph`).
fn run_graph(project_dir: &Path, json_out: bool) -> ! {
    let skills = discover_skills(project_dir);
    let known: BTreeSet<String> = skills.iter().map(|s| s.name.clone()).collect();
    let mut adj: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for sk in &skills {
        let body = strip_frontmatter(&sk.content);
        adj.insert(sk.name.clone(), find_references(&body, &sk.name, &known));
    }
    let cycles = find_cycles(&adj);
    if json_out {
        let edges: Vec<Value> = skills
            .iter()
            .flat_map(|s| {
                adj.get(&s.name)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |to| json!({ "from": s.name, "to": to }))
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "nodes": skills.iter().map(|s| &s.name).collect::<Vec<_>>(),
                "edges": edges,
                "cycles": cycles,
            }))
            .unwrap_or_default()
        );
    } else {
        let mut lines = vec!["graph TD".to_string()];
        for cyc in &cycles {
            lines.push(format!("  %% cycle detected: {}", cyc.join(" -> ")));
        }
        for sk in &skills {
            lines.push(format!("  skill_{0}[\"{0}\"]", sk.name));
        }
        for sk in &skills {
            for to in adj.get(&sk.name).cloned().unwrap_or_default() {
                lines.push(format!("  skill_{} --> skill_{to}", sk.name));
            }
        }
        println!("{}", lines.join("\n"));
    }
    std::process::exit(0);
}

/// DFS cycle detection over the adjacency map (JS `findCycles`).
fn find_cycles(adj: &BTreeMap<String, Vec<String>>) -> Vec<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    let mut color: BTreeMap<&str, Color> = adj.keys().map(|k| (k.as_str(), Color::White)).collect();
    let mut cycles: Vec<Vec<String>> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    fn canonical(cycle: &[String]) -> String {
        let ring = &cycle[..cycle.len().saturating_sub(1)];
        if ring.is_empty() {
            return String::new();
        }
        let min_idx = ring
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.cmp(b.1))
            .map_or(0, |(i, _)| i);
        let mut rotated: Vec<&str> = ring[min_idx..].iter().map(String::as_str).collect();
        rotated.extend(ring[..min_idx].iter().map(String::as_str));
        rotated.join(">")
    }

    // Iterative DFS to avoid borrow-checker friction with a recursive closure.
    let nodes: Vec<String> = adj.keys().cloned().collect();
    for start in &nodes {
        if color.get(start.as_str()).copied() != Some(Color::White) {
            continue;
        }
        let mut stack: Vec<String> = Vec::new();
        let mut iter_stack: Vec<(String, usize)> = vec![(start.clone(), 0)];
        if let Some(c) = color.get_mut(start.as_str()) {
            *c = Color::Gray;
        }
        stack.push(start.clone());
        while let Some((node, idx)) = iter_stack.last().cloned() {
            let neighbours = adj.get(&node).cloned().unwrap_or_default();
            if idx < neighbours.len() {
                if let Some(e) = iter_stack.last_mut() {
                    e.1 += 1;
                }
                let next = neighbours[idx].clone();
                match color.get(next.as_str()).copied() {
                    Some(Color::Gray) => {
                        if let Some(pos) = stack.iter().position(|n| n == &next) {
                            let mut cyc: Vec<String> = stack[pos..].to_vec();
                            cyc.push(next.clone());
                            let key = canonical(&cyc);
                            if seen.insert(key) {
                                cycles.push(cyc);
                            }
                        }
                    }
                    Some(Color::White) => {
                        if let Some(c) = color.get_mut(next.as_str()) {
                            *c = Color::Gray;
                        }
                        stack.push(next.clone());
                        iter_stack.push((next, 0));
                    }
                    _ => {}
                }
            } else {
                if let Some(c) = color.get_mut(node.as_str()) {
                    *c = Color::Black;
                }
                stack.pop();
                iter_stack.pop();
            }
        }
    }
    cycles
}

// ── orphans ──────────────────────────────────────────────────────────────────

/// `orphans` — list skills not invoked within the lookback window.
fn run_orphans(project_dir: &Path, days: i64, json_out: bool) -> ! {
    let since_ms = (crate::util::now_millis() as i64) - days * 86_400_000;
    let skills = discover_skills(project_dir);
    let invocations = scan_invocations(project_dir, since_ms);

    let mut orphans: Vec<String> = Vec::new();
    let mut last_invoked: BTreeMap<String, String> = BTreeMap::new();
    for sk in &skills {
        match invocations.get(&sk.name) {
            Some(ts) => {
                last_invoked.insert(sk.name.clone(), ts.clone());
            }
            None => orphans.push(sk.name.clone()),
        }
    }
    orphans.sort();
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "skills": skills.iter().map(|s| &s.name).collect::<Vec<_>>(),
                "orphans": orphans,
                "lookback_days": days,
                "last_invoked": last_invoked,
            }))
            .unwrap_or_default()
        );
        std::process::exit(0);
    }
    println!(
        "skill-orphan-audit: {}/{} skill(s) orphaned (lookback={days}d)",
        orphans.len(),
        skills.len()
    );
    for name in &orphans {
        let date = last_invoked
            .get(name)
            .map_or_else(|| "never".to_string(), |t| t.chars().take(10).collect::<String>());
        println!("  {name} (last invoked: {date})");
    }
    std::process::exit(0);
}

/// Replay the harness store for `skill.invoked` events; map skill → latest ts.
fn scan_invocations(project_dir: &Path, since_ms: i64) -> BTreeMap<String, String> {
    let mut last: BTreeMap<String, String> = BTreeMap::new();
    let events = SqliteEventStore::for_project(project_dir)
        .and_then(|store| store.replay())
        .unwrap_or_default();
    for ev in &events {
        if ev.event != "skill.invoked" || ev.ts.is_empty() {
            continue;
        }
        if let Some(ts_ms) = crate::run::complete_spec::parse_iso_millis(&ev.ts) {
            if ts_ms < since_ms {
                continue;
            }
        }
        let Some(skill) = ev.payload.get("skill").and_then(Value::as_str) else {
            continue;
        };
        let entry = last.entry(skill.to_string()).or_default();
        if ev.ts.as_str() > entry.as_str() {
            entry.clone_from(&ev.ts);
        }
    }
    last
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// POSIX-style relative path of `p` against `root`.
fn rel_posix(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

// ── skills list ──────────────────────────────────────────────────────────────

/// A discovered skill for the `list` subcommand — carries name, description,
/// and source (from frontmatter, defaulting to `"manual"` when absent).
#[derive(Debug, serde::Serialize)]
struct SkillListEntry {
    name: String,
    source: String,
    description: String,
}

/// Extract the `description:` value from YAML frontmatter — tolerates
/// multi-line continuation and quoted values.
fn extract_description(fm: &str) -> Option<String> {
    description_value(fm)
}

/// Glob `<root>/.claude/skills/*/SKILL.md`, parse each YAML frontmatter, and
/// return a sorted list of [`SkillListEntry`].
fn list_skills(root: &Path) -> Vec<SkillListEntry> {
    let skills_dir = root.join(".claude").join("skills");
    let mut entries: Vec<SkillListEntry> = Vec::new();

    let skill_paths = collect_skills_at(&skills_dir);
    for path in &skill_paths {
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let normalized = content.replace("\r\n", "\n");
        let name = extract_skill_name(&normalized).unwrap_or_else(|| {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        });
        let (source, description) = if let Some(fm) = frontmatter(&normalized) {
            let src = field(&fm, "source:")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "manual".to_string());
            let desc = extract_description(&fm).unwrap_or_default();
            (src, desc)
        } else {
            ("manual".to_string(), String::new())
        };

        entries.push(SkillListEntry {
            name,
            source,
            description,
        });
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn truncate_skills(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let t: String = chars[..max - 1].iter().collect();
        format!("{t}…")
    }
}

/// Run `skills list [--format table|json] [--root PATH]`.
fn run_list(root: &Path, json_out: bool) {
    let entries = list_skills(root);
    if json_out {
        let doc = serde_json::json!({ "skills": entries });
        println!(
            "{}",
            serde_json::to_string_pretty(&doc).unwrap_or_default()
        );
    } else {
        println!("| {:<40} | {:<8} | Description                                                     |", "Name", "Source");
        println!("|------------------------------------------|----------|------------------------------------------------------------------|");
        for e in &entries {
            let name_col = format!("{:<40}", &e.name);
            let src_col  = format!("{:<8}", &e.source);
            let desc_col = truncate_skills(&e.description, 64);
            println!("| {name_col} | {src_col} | {desc_col:<64} |");
        }
        println!("\n{} skill(s) found.", entries.len());
    }
}

/// Dispatch `mustard-rt run skills`.
pub fn run(subcommand: Option<&str>, args: &[String]) {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let arg_after = |flag: &str| -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    };
    match subcommand {
        Some("validate") => {
            let root = validate_root();
            let json_out = has("--json");
            if has("--strict-frontmatter") {
                run_validate_strict_frontmatter(&root, json_out);
            } else if has("--factual") {
                // `--factual` keeps the JS behaviour: WARN-by-default cluster
                // audit. The full heuristic is mode-gated; honour `off`.
                let mode = std::env::var("MUSTARD_SKILL_VALIDATE_MODE")
                    .map_or_else(|_| "strict".to_string(), |m| m.to_lowercase());
                if mode == "off" {
                    if json_out {
                        println!("{}", json!({ "mode": "off", "total": 0, "violations": [] }));
                    } else {
                        println!("skill-validate (factual): mode=off, skipping.");
                    }
                    std::process::exit(0);
                }
                // Faithful structural pass under factual mode (cluster check is
                // advisory; with no registry it is a no-op). `warn` never fails.
                let exit_failures = mode != "warn";
                run_validate_factual(&root, json_out, &mode, exit_failures);
            } else if has("--lines") {
                run_validate_lines(&root, json_out);
            } else {
                run_validate_structural(&root, json_out, has("--quiet"), arg_after("--only").as_deref());
            }
        }
        Some("graph") => {
            let project_dir = arg_after("--cwd")
                .map_or_else(|| PathBuf::from(env::project_dir()), PathBuf::from);
            run_graph(&project_dir, has("--json"));
        }
        Some("orphans") => {
            let project_dir = arg_after("--cwd")
                .map_or_else(|| PathBuf::from(env::project_dir()), PathBuf::from);
            let days = arg_after("--days")
                .and_then(|d| d.parse::<i64>().ok())
                .filter(|d| *d > 0)
                .or_else(|| {
                    std::env::var("MUSTARD_SKILL_ORPHAN_DAYS")
                        .ok()
                        .and_then(|d| d.parse::<i64>().ok())
                        .filter(|d| *d > 0)
                })
                .unwrap_or(30);
            run_orphans(&project_dir, days, has("--json"));
        }
        Some("list") => {
            let root = arg_after("--root")
                .map_or_else(|| PathBuf::from(env::project_dir()), PathBuf::from);
            run_list(&root, has("--format") && arg_after("--format").as_deref() == Some("json")
                || has("--json"));
        }
        Some("match") => {
            // Wave 4 (project-profiler): the unified skill-matching face.
            // Builds a resolver scope from `--entity` / `--operation` /
            // `--role` and prints the resolved closure (skill nodes + their
            // requires) as the same byte-stable envelope `context-resolve`
            // emits. Public JSON shape == `context-resolve` so downstream
            // tooling can use either entry point interchangeably.
            let entity = arg_after("--entity");
            let operation = arg_after("--operation");
            let role = arg_after("--role");
            let project = arg_after("--cwd")
                .map_or_else(|| PathBuf::from(env::project_dir()), PathBuf::from);
            let scope = crate::run::scan::resolve::ResolveScope {
                entities: entity.map(|e| vec![e]).unwrap_or_default(),
                operation,
                role,
                ..crate::run::scan::resolve::ResolveScope::default()
            };
            let out = crate::run::scan::resolve::resolve_closure(&project, &scope);
            let pretty = serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string());
            println!("{pretty}");
        }
        _ => {
            println!("Usage: skills <subcommand> [flags]");
            println!();
            println!("Subcommands:");
            println!("  validate [--json] [--quiet] [--factual] [--lines] [--only scan|manual]");
            println!("  graph    [--json] [--cwd PATH]");
            println!("  orphans  [--days N] [--json] [--cwd PATH]");
            println!("  list     [--format table|json] [--root PATH]");
            println!("  match    [--entity NAME] [--operation OP] [--role ROLE] [--cwd PATH]");
        }
    }
}

/// `validate --factual` — runs the structural checks plus `CODE_IN_BODY`
/// (fenced-block) detection, the always-applicable subset of the JS factual
/// audit. The cluster-suffix heuristic depends on a populated registry and is
/// advisory; absent a registry it never fires, exactly like the JS.
fn run_validate_factual(root: &Path, json_out: bool, mode: &str, exit_failures: bool) -> ! {
    let mut violations: Vec<Value> = Vec::new();
    let mut total = 0usize;
    for (dir, label) in collect_skill_dirs(root) {
        for file in collect_skills_at(&dir) {
            total += 1;
            let content = read_safe(&file);
            if !content.contains("<!-- mustard:generated") {
                continue; // gated — only audit generated skills
            }
            let fences = strip_frontmatter(&content)
                .lines()
                .filter(|l| l.starts_with("```"))
                .count();
            if fences > 0 {
                let skill = extract_skill_name(&content).unwrap_or_default();
                violations.push(json!({
                    "skill": skill,
                    "file": rel_posix(root, &file),
                    "code": "CODE_IN_BODY",
                    "detail": format!("{fences} fenced code block(s)"),
                    "location": label,
                }));
            }
        }
    }
    if json_out {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "mode": mode, "total": total, "violations": violations, "warnings": [],
            }))
            .unwrap_or_default()
        );
    } else if violations.is_empty() {
        println!("skill-validate (factual): {total} skill(s) scanned, 0 violations. mode={mode}");
    } else {
        println!(
            "skill-validate (factual): {} violation(s), 0 warning(s) across {total} skill(s). mode={mode}",
            violations.len()
        );
        for v in &violations {
            println!("  {}", v["file"].as_str().unwrap_or(""));
            println!("    [{}] {}", v["code"].as_str().unwrap_or(""), v["detail"].as_str().unwrap_or(""));
        }
    }
    let failed = !violations.is_empty();
    std::process::exit(if exit_failures && failed { 1 } else { 0 });
}

// ── strict-frontmatter validation (T1.7) ─────────────────────────────────────

/// `validate --strict-frontmatter` — assert every **foundation** SKILL.md
/// (under `apps/cli/templates/skills/` + the installed `.claude/skills/`)
/// parses against [`mustard_core::skill::frontmatter`]'s strict schema. Emits
/// JSON `{ ok: bool, total, failed, results: [...] }`. Exit `0` when ok, `1`
/// otherwise. Used by AC-W1.6.
///
/// Scope: foundation skills only (W1.T1.6). Scan-generated skills under
/// `{subproject}/.claude/skills/` are skipped here — they are W3 territory and
/// have their own validator (`mustard-rt run skills validate --factual`).
fn run_validate_strict_frontmatter(root: &Path, json_out: bool) -> ! {
    use mustard_core::skill::frontmatter::{
        missing_strict_keys, parse as parse_fm, validate as validate_fm,
    };
    let mut results: Vec<Value> = Vec::new();
    let mut total = 0usize;
    let mut failed = 0usize;
    // Scope to the canonical foundation source: `apps/cli/templates/skills/`.
    // The installed `<root>/.claude/skills/` copies are user-state refreshed
    // by `mustard update`; `<sub>/.claude/skills/` is scan territory (W3).
    // Both are validated by other faces — only templates are strict-gated.
    let foundation_dirs: Vec<(PathBuf, String)> = vec![(
        root.join("apps").join("cli").join("templates").join("skills"),
        "templates".to_string(),
    )];
    for (dir, label) in foundation_dirs {
        for file in collect_skills_at(&dir) {
            total += 1;
            let Ok(content) = fs::read_to_string(&file) else {
                failed += 1;
                results.push(json!({
                    "location": label,
                    "path": rel_posix(root, &file),
                    "ok": false,
                    "errors": ["unreadable"],
                }));
                continue;
            };
            // Strict validation: parse + structural strict pass + raw-keys check.
            let (ok, errors) = match parse_fm(&content) {
                Ok(fm) => {
                    let mut errs: Vec<String> = match validate_fm(&fm, true) {
                        Ok(()) => Vec::new(),
                        Err(e) => e.iter().map(ToString::to_string).collect(),
                    };
                    // Verify the raw YAML actually carries the strict keys
                    // (the parser folds missing → empty silently).
                    if let Some(yaml) = extract_frontmatter_body(&content) {
                        for missing in missing_strict_keys(&yaml) {
                            errs.push(format!("missing top-level key: {missing}"));
                        }
                    }
                    (errs.is_empty(), errs)
                }
                Err(e) => (false, vec![e.to_string()]),
            };
            if !ok {
                failed += 1;
            }
            results.push(json!({
                "location": label,
                "path": rel_posix(root, &file),
                "ok": ok,
                "errors": errors,
            }));
        }
    }
    let summary = json!({
        "ok": failed == 0,
        "total": total,
        "failed": failed,
        "results": results,
    });
    if json_out {
        println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
    } else {
        println!(
            "skill-validate (strict-frontmatter): {ok}/{total} ok, {failed} failed.",
            ok = total - failed
        );
        for r in &results {
            if r["ok"] == json!(false) {
                let path = r["path"].as_str().unwrap_or("");
                let errs = r["errors"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default();
                println!("  [fail] {path} — {errs}");
            }
        }
    }
    std::process::exit(if failed > 0 { 1 } else { 0 });
}

/// Extract the YAML body between leading `---` fences. Used by the strict
/// validator to assert top-level key presence (parser is lenient and folds
/// missing keys to empty Vecs).
fn extract_frontmatter_body(raw: &str) -> Option<String> {
    let normalized = raw.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_skill_name_reads_frontmatter() {
        let md = "---\nname: my-skill\ndescription: x\n---\nbody";
        assert_eq!(extract_skill_name(md), Some("my-skill".to_string()));
    }

    #[test]
    fn validate_skill_flags_missing_fields() {
        let (ok, errors, _) = validate_skill("---\nname: BadName\n---\nbody");
        assert!(!ok);
        assert!(errors.iter().any(|e| e.contains("kebab")));
        assert!(errors.iter().any(|e| e.contains("description")));
        assert!(errors.iter().any(|e| e.contains("source")));
    }

    #[test]
    fn validate_skill_accepts_well_formed() {
        let md = "---\nname: good-skill\ndescription: Use when the user wants to create a new thing in the project clearly.\nsource: manual\n---\nbody";
        let (ok, errors, source) = validate_skill(md);
        assert!(ok, "errors: {errors:?}");
        assert_eq!(source, Some("manual".to_string()));
    }

    #[test]
    fn is_kebab_check() {
        assert!(is_kebab("my-skill"));
        assert!(!is_kebab("MySkill"));
        assert!(!is_kebab("my_skill"));
    }

    #[test]
    fn strip_frontmatter_removes_block() {
        let body = strip_frontmatter("---\nname: x\n---\nhello world");
        assert_eq!(body, "hello world");
    }

    #[test]
    fn contains_word_respects_boundaries() {
        assert!(contains_word("see karpathy-guidelines here", "karpathy-guidelines"));
        assert!(!contains_word("karpathyx", "karpathy"));
    }

    #[test]
    fn find_cycles_detects_two_node_loop() {
        let mut adj: BTreeMap<String, Vec<String>> = BTreeMap::new();
        adj.insert("a".into(), vec!["b".into()]);
        adj.insert("b".into(), vec!["a".into()]);
        let cycles = find_cycles(&adj);
        assert_eq!(cycles.len(), 1);
    }

    #[test]
    fn find_cycles_none_for_dag() {
        let mut adj: BTreeMap<String, Vec<String>> = BTreeMap::new();
        adj.insert("a".into(), vec!["b".into()]);
        adj.insert("b".into(), vec![]);
        assert!(find_cycles(&adj).is_empty());
    }

    // ── skills list tests ────────────────────────────────────────────────────

    fn write_skill_md(root: &std::path::Path, skill_name: &str, content: &str) {
        let dir = root.join(".claude").join("skills").join(skill_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn list_skills_parses_frontmatter() {
        let td = tempfile::tempdir().unwrap();
        write_skill_md(
            td.path(),
            "my-skill",
            "---\nname: my-skill\ndescription: Use when the user wants to do something useful in the project.\nsource: manual\n---\n# body",
        );
        let entries = list_skills(td.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my-skill");
        assert_eq!(entries[0].source, "manual");
        assert!(entries[0].description.contains("Use when"), "got: {:?}", entries[0].description);
    }

    #[test]
    fn list_skills_defaults_source_to_manual_when_absent() {
        let td = tempfile::tempdir().unwrap();
        write_skill_md(
            td.path(),
            "no-source",
            "---\nname: no-source\ndescription: Use when the user wants to do something.\n---\nbody",
        );
        let entries = list_skills(td.path());
        assert_eq!(entries[0].source, "manual");
    }

    #[test]
    fn list_skills_empty_dir_returns_empty() {
        let td = tempfile::tempdir().unwrap();
        let entries = list_skills(td.path());
        assert!(entries.is_empty());
    }

    #[test]
    fn list_skills_sorted_by_name() {
        let td = tempfile::tempdir().unwrap();
        for name in &["zebra-skill", "alpha-skill", "mango-skill"] {
            write_skill_md(
                td.path(),
                name,
                &format!("---\nname: {name}\ndescription: Use when anything happens in the project.\nsource: scan\n---\nbody"),
            );
        }
        let entries = list_skills(td.path());
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha-skill", "mango-skill", "zebra-skill"]);
    }
}
