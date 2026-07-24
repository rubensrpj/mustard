//! `mustard-rt run analyze-validation` — a port of `scripts/analyze-validation.js`.
//!
//! WARN-level spec validator (never blocks the pipeline). Checks layer
//! coverage, file-reference resolvability, task-count sanity, and the
//! extended-light scope ↔ model constraint. Emits one JSON line:
//! `{ "ok": bool, "issues": [{ severity, type, message, file? }] }`.

use crate::commands::spec::spec_sections::is_heading;
use mustard_core::io::fs;
use mustard_core::platform::i18n;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// File extensions expected per declared agent layer.
fn layer_extensions(layer: &str) -> &'static [&'static str] {
    match layer {
        "Backend" => &[".ts", ".cs", ".py", ".go", ".rs"],
        "Frontend" => &[".tsx", ".jsx", ".vue", ".svelte", ".html", ".css"],
        "Database" => &[".sql", ".prisma", "schema.ts"],
        "Mobile" => &[".swift", ".kt", ".dart"],
        _ => &[],
    }
}

/// Emit a fatal `validator-crash` result and exit 1.
fn crash(message: &str) -> ! {
    let out = json!({
        "ok": false,
        "issues": [{ "severity": "ERROR", "type": "validator-crash", "message": message }],
    });
    println!("{out}");
    std::process::exit(1);
}

/// Extract the body lines of the `## Files` section.
fn files_section_lines(lines: &[&str]) -> Vec<String> {
    let mut in_files = false;
    let mut out = Vec::new();
    for line in lines {
        if is_heading(line, "files") {
            in_files = true;
            continue;
        }
        if in_files && line.starts_with("##") {
            in_files = false;
        }
        if in_files {
            out.push((*line).to_string());
        }
    }
    out
}

/// Find every `### {Word} Agent` header and return the agent name + the body
/// up to the next `##`/`###` heading.
fn agent_blocks(content: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = content.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        // `###\s+(\S.*?)\s+Agent`
        let Some(rest) = line.strip_prefix("###") else {
            continue;
        };
        if !rest.starts_with([' ', '\t']) {
            continue;
        }
        let rest = rest.trim_start();
        let Some(agent_pos) = rest.find(" Agent") else {
            continue;
        };
        let name = rest[..agent_pos].trim();
        if name.is_empty() {
            continue;
        }
        let mut body = String::new();
        for next in lines.iter().skip(i + 1) {
            let t = next.trim_start();
            if t.starts_with("## ") || t.starts_with("### ") {
                break;
            }
            body.push_str(next);
            body.push('\n');
        }
        blocks.push((name.to_string(), body));
    }
    blocks
}

/// Extract the first capture of a simple `key:\s*["']?value["']?` pattern,
/// case-insensitive. `value_chars` controls which chars belong to the value.
fn extract_kv<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let lower = content.to_lowercase();
    let key_lower = key.to_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find(&key_lower) {
        let at = search + rel;
        let after = &content[at + key.len()..];
        let after_t = after.trim_start_matches([' ', '\t']);
        if !after_t.starts_with(':') {
            search = at + key.len();
            continue;
        }
        let mut val = after_t[1..].trim_start_matches([' ', '\t']);
        val = val.strip_prefix(['"', '\'']).unwrap_or(val);
        let end = val
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(val.len());
        let token = &val[..end];
        if !token.is_empty() {
            return Some(token);
        }
        search = at + key.len();
    }
    None
}

/// Count `- [ ]` / `- [x]` checkbox markers in a block.
fn count_tasks(block: &str) -> usize {
    let mut count = 0;
    let bytes = block.as_bytes();
    let needle = b"- [";
    let mut i = 0;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 3] == needle {
            let c = bytes[i + 3];
            if (c == b' ' || c == b'x') && bytes[i + 4] == b']' {
                count += 1;
                i += 5;
                continue;
            }
        }
        i += 1;
    }
    count
}

/// Common source/config/doc file extensions — the "is this token a real file?"
/// allowlist for [`backtick_file_refs`]. A backtick token without a path
/// separator must end in one of these to count as a file ref; otherwise dotted
/// prose (`extensions.code`, `err.message`) reads as a path to the char-class
/// check and is wrongly flagged as a missing file.
const KNOWN_FILE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte", "py", "go", "cs",
    "java", "kt", "swift", "dart", "rb", "php", "c", "h", "cpp", "hpp", "scala",
    "ex", "exs", "html", "css", "scss", "sass", "less", "json", "jsonc", "toml",
    "yaml", "yml", "xml", "ini", "env", "lock", "sql", "prisma", "graphql", "proto",
    "md", "mdx", "txt", "sh", "bash", "ps1", "bat",
];

/// `true` when `ext` (no leading dot) is a recognised file extension.
fn is_known_file_ext(ext: &str) -> bool {
    KNOWN_FILE_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

/// Scan a string for `` `path.ext` `` tokens (backtick-wrapped file refs).
fn backtick_file_refs(text: &str) -> Vec<String> {
    let mut refs = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        if let Some(close) = after.find('`') {
            let token = &after[..close];
            // `[\w./-]+\.\w+` — path chars then a dotted extension, AND a real
            // path shape: a separator `/` OR a known file extension. The extra
            // gate rejects dotted-identifier prose (`extensions.code`,
            // `err.message`) that passes the char-class check but is not a file.
            let ext = token.rsplit('.').next().unwrap_or("");
            let ok = !token.is_empty()
                && token.contains('.')
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-' | '_'))
                && !ext.is_empty()
                && ext.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && (token.contains('/') || is_known_file_ext(ext));
            if ok {
                refs.push(token.to_string());
            }
            rest = &after[close + 1..];
        } else {
            break;
        }
    }
    refs
}

/// Whether a `## Files` ref resolves on disk: under the spec dir, the cwd, or
/// any subproject root. The subproject roots quiet false "missing" WARNs for
/// existing-but-extended files declared with a subproject-relative or
/// abbreviated path (e.g. a git-submodule backend).
fn ref_resolves(r: &str, spec_dir: &Path, project_roots: &[PathBuf]) -> bool {
    fs::exists(spec_dir.join(r))
        || fs::exists(Path::new(r))
        || project_roots.iter().any(|root| fs::exists(root.join(r)))
}

/// `true` when a bare (un-backticked) token reads as a file path: it either
/// carries a path separator or ends in a recognised source extension. Requiring
/// a KNOWN extension is what keeps prose out — "3.5", "e.g." and
/// "https://example.com" all fail, while `src/list.rs` and `Cargo.toml` pass.
fn looks_like_file_path(token: &str) -> bool {
    let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '/'
        && c != '\\' && c != '-' && c != '_');
    if token.is_empty() || !token.contains('.') {
        return false;
    }
    let ext = token.rsplit('.').next().unwrap_or("");
    is_known_file_ext(ext)
}

/// The prose-only violations in the PRD `## Context` section, as human-readable
/// fragments. Empty when the section is absent or clean.
///
/// The shipped spec law (`plugin/refs/feature/spec-language.md`, "Contexto
/// rules") makes the PRD layer prose-only: `## Context` briefs a human
/// rediscovering the work next week, so file paths, line numbers, identifiers
/// and bullet lists belong to `## Root cause` / `## Files` / `## Tasks`. The law
/// shipped but nothing enforced it, and the drafter itself violated it — it
/// spliced the scan digest's anchors into Context as a bullet list of paths.
/// Checked here so the violation is caught wherever it comes from.
fn context_prose_violations(content: &str) -> Vec<String> {
    let Some(block) = crate::commands::spec::spec_sections::section_block(content, "context")
    else {
        return Vec::new();
    };
    let mut violations = Vec::new();
    // Skip the heading line itself; a `##` heading is not section body.
    for line in block.lines().skip(1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            violations.push(format!("bullet list (`{}`)", truncate_for_message(trimmed)));
        }
        for token in trimmed.split_whitespace() {
            if looks_like_file_path(token) {
                violations.push(format!("file path (`{}`)", truncate_for_message(token)));
            }
        }
    }
    violations
}

/// Clip a quoted fragment so one long line cannot flood the issue message.
fn truncate_for_message(s: &str) -> String {
    const MAX: usize = 48;
    if s.chars().count() <= MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX).collect();
    format!("{head}…")
}

/// Whether an AC `command` is a TAUTOLOGY — it exits 0 whether or not the
/// feature was actually built, so it verifies nothing. These are the rubber
/// stamps F6 kills: a bare `cargo build`/`cargo check`, a `cargo test` with no
/// test-name filter (it just re-runs the pre-existing suite), `npm test`/
/// `npm run build`, or a source `grep`/`rg` (asserts textual presence, not
/// runtime behaviour).
///
/// Deliberately conservative to avoid false positives: a COMPOUND command
/// (`&&` / `||` / `;` / `|`) is never weak (the author combined steps on
/// purpose), and any positional test-name / assertion target makes it strong.
/// A leading `rtk ` wrapper is transparent. Pure, total, never panics.
fn is_weak_ac_command(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty()
        || cmd.contains("&&")
        || cmd.contains("||")
        || cmd.contains(';')
        || cmd.contains('|')
    {
        return false;
    }
    // `rtk` is a transparent RTK passthrough — the weakness (if any) lives in
    // the wrapped command.
    let cmd = cmd.strip_prefix("rtk ").map_or(cmd, str::trim_start);
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let Some(&first) = tokens.first() else {
        return false;
    };
    match first {
        // A pure source PRESENCE search asserts textual presence, not
        // behaviour — weak. But an ABSENCE search (`--files-without-match`,
        // `grep -L`, `rg -v`) is a genuine post-condition that `qa-run` runs
        // and grades by exit code, so it is exempt.
        "grep" | "egrep" | "fgrep" | "rg" | "ag" | "ack" => !is_absence_search(cmd),
        // A bare build word / whole-project type-check with no target.
        "build" | "tsc" | "make" if tokens.len() == 1 => true,
        "cargo" => match tokens.get(1).copied() {
            Some("build" | "b" | "check" | "c") => true,
            Some("test" | "t" | "nextest") => !cargo_test_has_filter(&tokens),
            _ => false,
        },
        "npm" | "pnpm" | "yarn" | "bun" => match tokens.get(1).copied() {
            Some("test" | "t" | "build") => true,
            Some("run") => matches!(
                tokens.get(2).copied(),
                Some("build" | "test" | "lint" | "typecheck" | "check")
            ),
            _ => false,
        },
        _ => false,
    }
}

/// Whether a search command is an ABSENCE / negation assertion rather than a
/// presence one. `rg --files-without-match PATTERN FILE` / `grep -L` / `rg -v`
/// exit non-zero when the string is STILL present, so they verify a real
/// post-condition (e.g. "the deprecated call is gone") — `qa-run` runs and
/// grades exactly these. (`-L` also means follow-symlinks in `rg`; treating
/// such a command as non-weak only drops a false-positive WARN — the safe
/// direction.) Pure, total.
fn is_absence_search(cmd: &str) -> bool {
    cmd.contains("--files-without-match")
        || cmd.contains("--invert-match")
        || cmd.split_whitespace().any(|t| t == "-L" || t == "-v")
}

/// Whether a `cargo test …` invocation carries a positional test-name filter
/// (which makes it a STRONG assertion). `tokens[0..2]` are `cargo test`; a
/// filter is any positional token after `test` that is neither a flag nor the
/// value consumed by a value-taking flag (`-p`, `--features`, …). A `--`
/// forwards the rest to libtest, where a non-flag is a filter.
fn cargo_test_has_filter(tokens: &[&str]) -> bool {
    const VALUE_FLAGS: &[&str] = &[
        "-p", "--package", "--test", "--bench", "--example", "--bin", "--features",
        "-F", "--manifest-path", "-j", "--jobs", "--target", "--profile",
        "--target-dir", "--color",
    ];
    let mut i = 2;
    while i < tokens.len() {
        let t = tokens[i];
        if t == "--" {
            return tokens[i + 1..].iter().any(|a| !a.starts_with('-'));
        }
        if t.contains('=') {
            i += 1; // self-contained `--flag=value`
            continue;
        }
        if VALUE_FLAGS.contains(&t) {
            i += 2; // skip the flag and its separate value
            continue;
        }
        if t.starts_with('-') {
            i += 1; // boolean flag (--workspace, --all-targets, --release, …)
            continue;
        }
        return true; // a bare positional after `test` ⇒ a test-name filter
    }
    false
}

/// Whether an AC `command` invokes a TEST RUNNER — the subset of the weak-AC
/// vocabulary that runs a suite: `cargo test|t|nextest`, or
/// `npm|pnpm|yarn|bun test|t` / `… run test`. A leading `rtk ` wrapper is
/// transparent; a COMPOUND command (`&&`/`||`/`;`/`|`) is exempt (the author
/// already chained an assertion). Language-agnostic — it keys off the runner
/// verb, never the runner's output. Pure, total, never panics.
///
/// Used by the V6b lint to suggest a declared `Expect:` evidence regex for a
/// test AC that has none: a green suite proves the tests ran, not that THIS
/// feature's behaviour holds.
fn is_test_shaped_command(command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty()
        || cmd.contains("&&")
        || cmd.contains("||")
        || cmd.contains(';')
        || cmd.contains('|')
    {
        return false;
    }
    let cmd = cmd.strip_prefix("rtk ").map_or(cmd, str::trim_start);
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let Some(&first) = tokens.first() else {
        return false;
    };
    match first {
        "cargo" => matches!(tokens.get(1).copied(), Some("test" | "t" | "nextest")),
        "npm" | "pnpm" | "yarn" | "bun" => match tokens.get(1).copied() {
            Some("test" | "t") => true,
            Some("run") => tokens.get(2).copied() == Some("test"),
            _ => false,
        },
        _ => false,
    }
}

/// Run the validation. Returns the issues list.
///
/// `pub` so `plan-materialize` composes the same checks in-process (single
/// validator source — no subprocess, no drift) and the acceptance-criteria
/// tests can assert a verdict without shelling out, while the CLI entry [`run`]
/// keeps the stdout/exit contract.
pub fn validate(abs_path: &Path, content: &str) -> Vec<Value> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut issues: Vec<Value> = Vec::new();

    let file_lines = files_section_lines(&lines);
    let files_text = file_lines.join("\n");

    // Validation 1: layer coverage.
    for layer in ["Backend", "Frontend", "Database", "Mobile"] {
        let header = format!("### {layer} Agent");
        if !content.contains(&header) {
            continue;
        }
        let exts = layer_extensions(layer);
        let has_match = exts.iter().any(|ext| files_text.contains(ext));
        if !has_match {
            issues.push(json!({
                "severity": "WARN",
                "type": "layer-gap",
                "message": format!("Spec declares {layer} Agent but Files has no {layer} extensions"),
            }));
        }
    }

    // Validation 2: file refs resolvable.
    let spec_dir = abs_path.parent().unwrap_or_else(|| Path::new("."));
    // Subproject roots from the scan model: resolve existing-but-extended files
    // declared with a subproject-relative / abbreviated path so they are not
    // reported as false "missing" WARNs. An absent model yields no extra roots,
    // so resolution matches the historical two-path behaviour when no model is
    // present.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let model = cwd.join(".claude").join("grain.model.json");
    let project_roots: Vec<PathBuf> = mustard_core::read_projects(&model)
        .into_iter()
        .map(|p| cwd.join(p.dir))
        .collect();
    for r in backtick_file_refs(&files_text) {
        let line_with_ref = file_lines
            .iter()
            .find(|l| l.contains(&format!("`{r}`")))
            .map_or("", String::as_str);
        // Localized marker recognition: the drafter writes the create marker
        // in the spec's narrative locale (`(novo)`/`(criar)` in pt-BR), so the
        // check goes through the core i18n catalogue — the single origin of
        // the marker synonyms — instead of the historical EN-only literal
        // (which flagged every pt-BR net-new file as `missing-file`).
        let is_create = i18n::line_has_file_marker(line_with_ref, i18n::FileMarker::Create);
        let resolved = ref_resolves(&r, spec_dir, &project_roots);
        if !is_create && !resolved {
            let accepted = i18n::file_marker_synonyms(i18n::FileMarker::Create).join(" / ");
            issues.push(json!({
                "severity": "WARN",
                "type": "missing-file",
                "file": r,
                "message": format!("File referenced but not found and not marked {accepted}"),
            }));
        }
    }

    // Validation 3: task decomposition sane.
    for (agent_name, block) in agent_blocks(content) {
        let tasks = count_tasks(&block);
        if !(2..=10).contains(&tasks) {
            issues.push(json!({
                "severity": "WARN",
                "type": "task-count",
                "message": format!("{agent_name} Agent has {tasks} tasks (expected 2-10)"),
            }));
        }
    }

    // Validation 4: extended-light scope requires the entity to already exist in
    // the repo model (grain.model.json declaration names, read via the scan tool —
    // this crate never parses the model's schema itself).
    if let Some(scope) = extract_kv(content, "scope") {
        if scope.eq_ignore_ascii_case("extended-light") {
            if let Some(entity) = extract_kv(content, "entity") {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let model = cwd.join(".claude").join("grain.model.json");
                let known = mustard_core::read_entity_names(&model);
                if !known.iter().any(|k| k.eq_ignore_ascii_case(entity)) {
                    let message = if known.is_empty() {
                        "Extended Light scope requires the entity in grain.model.json, but no model/declarations were found. Reclassify as Full.".to_string()
                    } else {
                        format!("Extended Light scope requires entity \"{entity}\" in grain.model.json, but not found. Reclassify as Full.")
                    };
                    issues.push(json!({ "severity": "WARN", "type": "scope-mismatch", "message": message }));
                }
            }
        }
    }

    // Validation 5: AC format parseability. The AC section heading resolves
    // (EN `## Acceptance Criteria` / PT `## Critérios de Aceitação`, via the
    // shared i18n-aware extractor) but ZERO items survive the exact parser
    // qa-run executes — qa-run would later degrade to `overall: skip`, so the
    // format problem is surfaced here, at ANALYZE time. An absent section is
    // deliberately NOT flagged: behaviour stays unchanged for specs that carry
    // no ACs at this stage.
    let ac_section = crate::commands::review::qa_run::extract_ac_section(content);
    let ac_items = ac_section
        .as_deref()
        .map(crate::commands::review::qa_run::parse_ac_items)
        .unwrap_or_default();
    if ac_section.is_some() && ac_items.is_empty() {
        issues.push(json!({
            "severity": "WARN",
            "type": "unparseable-ac",
            "message": "Acceptance Criteria section found but no parseable AC items. \
                        Expected format: `**AC-N** — title` followed by a line \
                        `Command: `<runnable command>``.",
        }));
    }

    // Validation 6: AC TAUTOLOGY linter. A criterion "verified" by a bare
    // `cargo build` / `cargo test` (no filter) / `npm test` / source `grep`
    // passes whether or not the feature exists — the rubber stamp F6 kills. Flag
    // each such WEAK AC by id (WARN — analyze-validation never blocks). Two
    // exemptions: the LAST AC is the trailing build-green SAFETY net (kept on
    // purpose), and an unfilled `<…>` skeleton command is not yet a real
    // command. Reuses the exposed `AcItem` `id` + `command`.
    if ac_items.len() > 1 {
        let last = ac_items.len() - 1;
        let weak: Vec<String> = ac_items
            .iter()
            .enumerate()
            .filter(|(i, item)| {
                *i != last && !item.command.contains('<') && is_weak_ac_command(&item.command)
            })
            .map(|(_, item)| item.id.clone())
            .collect();
        if !weak.is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "weak-ac",
                "message": format!(
                    "Acceptance criteria verified by a tautological build/test/search command \
                     that passes whether or not the feature exists: {}. Replace with a command \
                     that asserts the new behaviour.",
                    weak.join(", ")
                ),
            }));
        }

        // Validation 6b: a TEST-RUNNER AC that declares no `Expect:` evidence
        // regex. A green `cargo test` / `npm test` proves the suite ran, not
        // that THIS feature's behaviour holds; a declared `Expect: `<regex>``
        // (matched by qa-run against the command's own output) turns the pass
        // into evidence. WARN-level, language-agnostic (keyed off the runner
        // verb, never its output). Excludes the trailing safety AC, `<…>`
        // skeletons, and ids already flagged weak (a tautology's fix is
        // replacement, not an Expect line). Reuses the exposed `AcItem.expect`.
        let no_expect: Vec<String> = ac_items
            .iter()
            .enumerate()
            .filter(|(i, item)| {
                *i != last
                    && item.expect.is_none()
                    && !item.command.contains('<')
                    && is_test_shaped_command(&item.command)
                    && !weak.contains(&item.id)
            })
            .map(|(_, item)| item.id.clone())
            .collect();
        if !no_expect.is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "test-ac-no-expect",
                "message": format!(
                    "Test-runner acceptance criteria with no declared `Expect:` evidence regex: \
                     {}. A passing suite proves the tests ran, not that this feature's behaviour \
                     holds — add an `Expect: `<regex>`` line so qa-run matches the expected \
                     evidence in the command's output.",
                    no_expect.join(", ")
                ),
            }));
        }
    }

    // Validation 7: cross-artifact coherence (AC × task × file). Mirrors V5 — it
    // only runs once an AC section EXISTS (a spec whose ACs are not authored yet
    // is left alone, behaviour unchanged) and only when the plan carries
    // `### {Role} Agent` task blocks (a virgin draft has none). A present-but-
    // unparseable AC section with agent work, or ACs+tasks that point at no
    // files, is a gap. The wave↔AC COVERAGE itself is enforced deterministically
    // in `wave-scaffold` (the `satisfies`/`acceptance` traceability). Reuses the
    // folded agent-block + file-ref lists.
    let agents_with_tasks: Vec<String> = agent_blocks(content)
        .into_iter()
        .filter(|(_, body)| count_tasks(body) > 0)
        .map(|(name, _)| name)
        .collect();
    if ac_section.is_some() && !agents_with_tasks.is_empty() {
        if ac_items.is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "ac-task-gap",
                "message": format!(
                    "{} agent task block(s) but no acceptance criteria to verify them — \
                     every wave must satisfy an AC.",
                    agents_with_tasks.len()
                ),
            }));
        } else if backtick_file_refs(&files_text).is_empty() {
            issues.push(json!({
                "severity": "WARN",
                "type": "ac-file-gap",
                "message": "Acceptance criteria and agent tasks present but the Files section \
                            lists no files to implement them.",
            }));
        }
    }

    // Validation 8: the PRD layer is PROSE-ONLY. A `## Context` carrying file
    // paths or a bullet list is agent input pasted into a human briefing — the
    // shipped spec law forbids it, and until now nothing checked.
    let context_violations = context_prose_violations(content);
    if !context_violations.is_empty() {
        issues.push(json!({
            "severity": "WARN",
            "type": "context-not-prose",
            "message": format!(
                "The Context section is prose-only — it briefs a human rediscovering the work, \
                 so file paths, line numbers and bullet lists belong to Root cause / Files / \
                 Tasks. Found: {}.",
                context_violations.join(", ")
            ),
        }));
    }

    issues
}

/// Dispatch `mustard-rt run analyze-validation`.
pub fn run(spec: Option<&str>) {
    let Some(spec) = spec else {
        crash("No spec path provided. Use --spec <path>");
    };
    let abs_path = std::fs::canonicalize(spec)
        .unwrap_or_else(|_| PathBuf::from(spec));
    if !fs::exists(&abs_path) {
        crash(&format!("Spec file not found: {}", abs_path.display()));
    }
    let content = match fs::read_to_string(&abs_path) {
        Ok(c) => c,
        Err(e) => crash(&format!("{e}")),
    };
    let issues = validate(&abs_path, &content);
    let out = json!({ "ok": issues.is_empty(), "issues": issues });
    println!("{out}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clean_spec_has_no_issues() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        // A coherent spec now also carries ACs that trace to the agent's work
        // (a behaviour AC + a trailing build-green safety AC).
        let body = "# Spec\n## Files\n- `a.rs` (create)\n### Backend Agent\n- [ ] t1\n- [ ] t2\n\n\
                    ## Acceptance Criteria\n\
                    - **AC-1** — when a.rs runs, then it returns ok.\n  Command: `curl -sf localhost`\n\
                    - **AC-2** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(&path, &content);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// V6: a bare `cargo build` AC (not the trailing safety net) is flagged
    /// WEAK; the LAST AC (the build-green safety criterion) is exempt, and a
    /// behaviour AC with a real assertion command is never flagged.
    #[test]
    fn flags_weak_tautological_ac_command() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — feature works.\n  Command: `cargo build`\n\
                    - **AC-2** — endpoint responds.\n  Command: `curl -sf localhost/health`\n\
                    - **AC-3** — build green.\n  Command: `rtk cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        let weak = issues
            .iter()
            .find(|i| i["type"] == json!("weak-ac"))
            .unwrap_or_else(|| panic!("expected weak-ac WARN: {issues:?}"));
        assert_eq!(weak["severity"], json!("WARN"));
        let msg = weak["message"].as_str().unwrap_or_default();
        assert!(msg.contains("AC-1"), "the planted cargo-build AC is named: {msg}");
        // AC-2 (real assertion) and AC-3 (trailing safety) are NOT flagged.
        assert!(!msg.contains("AC-2"), "a real behaviour AC is strong: {msg}");
        assert!(!msg.contains("AC-3"), "the trailing safety AC is exempt: {msg}");
    }

    /// V6: a `grep -q` "verification" is weak (asserts textual presence, not
    /// behaviour); a `cargo test` WITH a test-name filter is strong.
    #[test]
    fn weak_ac_flags_grep_but_not_filtered_cargo_test() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — doc mentions it.\n  Command: `grep -q Modelo SKILL.md`\n\
                    - **AC-2** — the new unit passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n\
                    - **AC-3** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        let msg = issues
            .iter()
            .find(|i| i["type"] == json!("weak-ac"))
            .and_then(|i| i["message"].as_str())
            .unwrap_or_default();
        assert!(msg.contains("AC-1"), "grep -q AC is weak: {issues:?}");
        assert!(!msg.contains("AC-2"), "filtered cargo test is strong: {issues:?}");
    }

    #[test]
    fn backtick_refs_reject_dotted_prose_keep_real_paths() {
        // Dotted-identifier prose in code spans is NOT a file path.
        assert!(backtick_file_refs("see `extensions.code` and `.message`").is_empty());
        assert!(backtick_file_refs("`error.extensions.code` / `err.message`").is_empty());
        // A path (separator) and a bare known-extension file ARE captured.
        let refs = backtick_file_refs("edit `src/foo.rs` and `Cargo.toml`");
        assert!(refs.contains(&"src/foo.rs".to_string()), "{refs:?}");
        assert!(refs.contains(&"Cargo.toml".to_string()), "{refs:?}");
    }

    #[test]
    fn absence_search_is_not_weak() {
        // A presence search is weak; an absence search (files-without-match /
        // grep -L / rg -v) is a real post-condition that `qa-run` grades.
        assert!(is_weak_ac_command("rg -q Foo src/lib.rs"));
        assert!(!is_weak_ac_command("rg --files-without-match Foo src/lib.rs"));
        assert!(!is_weak_ac_command("grep -L Foo src/lib.rs"));
        assert!(!is_weak_ac_command("rg -v Foo src/lib.rs"));
    }

    /// V6b: a FILTERED `cargo test` (strong, so NOT flagged weak) that declares
    /// no `Expect:` line raises `test-ac-no-expect` — a green suite proves the
    /// tests ran, not that this feature's behaviour holds. Adding an `Expect:`
    /// regex line suppresses the warn (the `expect_regex` evidence contract).
    #[test]
    fn expect_regex_test_ac_without_expect_warns_and_expect_line_clears_it() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        // AC-1: filtered cargo test, no Expect ⇒ warns. AC-2: trailing safety,
        // exempt.
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — the new parser case passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n\
                    - **AC-2** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        let warn = issues
            .iter()
            .find(|i| i["type"] == json!("test-ac-no-expect"))
            .unwrap_or_else(|| panic!("expected test-ac-no-expect WARN: {issues:?}"));
        assert_eq!(warn["severity"], json!("WARN"));
        let msg = warn["message"].as_str().unwrap_or_default();
        assert!(msg.contains("AC-1"), "the un-asserted test AC is named: {msg}");
        assert!(!msg.contains("AC-2"), "the trailing safety AC is exempt: {msg}");

        // Same spec, AC-1 now declares an `Expect:` regex ⇒ no warn.
        let body2 = "# Spec\n\n## Acceptance Criteria\n\
                     - **AC-1** — the new parser case passes.\n  Command: `cargo test -p mustard-rt my_new_case`\n  Expect: `test result: ok`\n\
                     - **AC-2** — build green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body2).unwrap();
        let issues2 = validate(&path, body2);
        assert!(
            !issues2.iter().any(|i| i["type"] == json!("test-ac-no-expect")),
            "a declared Expect line clears the warn: {issues2:?}"
        );
    }

    /// V7: a PRESENT-but-unparseable AC section with agent task blocks →
    /// `ac-task-gap` WARN. (An ABSENT AC section is left alone, like V5 — proven
    /// by `ac_format_validation_absent_section_unchanged`.)
    #[test]
    fn flags_ac_task_gap_when_agent_has_tasks_but_no_parseable_ac() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Files\n- `a.rs` (create)\n### Backend Agent\n- [ ] t1\n- [ ] t2\n\n\
                    ## Acceptance Criteria\nfree prose, no parseable AC line here\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        assert!(
            issues.iter().any(|i| i["type"] == json!("ac-task-gap")),
            "agent tasks with a broken AC section must warn: {issues:?}"
        );
    }

    #[test]
    fn ref_resolves_against_subproject_root() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path().join("spec");
        std::fs::create_dir_all(&spec_dir).unwrap();
        // An existing file under a subproject root, referenced with a path
        // relative to the subproject (not the spec dir or cwd).
        let backend = dir.path().join("backend");
        std::fs::create_dir_all(backend.join("src")).unwrap();
        std::fs::write(backend.join("src").join("Payable.cs"), "// existing").unwrap();
        let roots = vec![backend.clone()];

        // Resolves via the subproject root — no false "missing".
        assert!(ref_resolves("src/Payable.cs", &spec_dir, &roots));
        // A genuinely-absent file still does NOT resolve — the fix must not mask
        // true misses/typos.
        assert!(!ref_resolves("src/Ghost.cs", &spec_dir, &roots));
        // With no subproject roots it falls back to the historical two paths.
        assert!(!ref_resolves("src/Payable.cs", &spec_dir, &[]));
    }

    #[test]
    fn flags_task_count_out_of_range() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(&path, "### Backend Agent\n- [ ] only one\n").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(&path, &content);
        assert!(issues.iter().any(|i| i["type"] == json!("task-count")));
    }

    /// A well-formed AC section (the drafter shape: `- **AC-N** — title` +
    /// indented `Command:` line) must NOT raise `unparseable-ac`.
    #[test]
    fn ac_format_validation_parseable_section_is_clean() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - **AC-1** — workspace builds green.\n  Command: `cargo build`\n\
                    - **AC-2** — tests pass.\n  Command: `cargo test`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("unparseable-ac")),
            "{issues:?}"
        );
    }

    /// An AC section whose items the qa-run parser cannot read (no `Command:`
    /// anywhere) yields a WARN `unparseable-ac` with the format hint — the
    /// exact situation where qa-run later degrades to `overall: skip`.
    #[test]
    fn ac_format_validation_malformed_section_warns() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Acceptance Criteria\n\
                    - AC um: roda os testes sem comando declarado\n\
                    - criterio solto sem id\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        let issue = issues
            .iter()
            .find(|i| i["type"] == json!("unparseable-ac"))
            .unwrap_or_else(|| panic!("expected unparseable-ac WARN: {issues:?}"));
        assert_eq!(issue["severity"], json!("WARN"));
        let msg = issue["message"].as_str().unwrap_or_default();
        assert!(msg.contains("**AC-N**"), "hint must show the exact format: {msg}");
        assert!(msg.contains("Command:"), "hint must mention the Command: line: {msg}");
    }

    /// Roundtrip (TF marcador localizado): the pt-BR drafter marks net-new
    /// files `(novo)`/`(criar)` — both must suppress `missing-file` exactly
    /// like the EN canonical `(create)` (the run that motivated the fix
    /// produced 7 false `missing-file` WARNs from `(novo)` lines).
    #[test]
    fn roundtrip_localized_create_marker_suppresses_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Arquivos\n\
                    - `ghost_a.rs` (novo)\n\
                    - `ghost_b.rs` (criar)\n\
                    - `ghost_c.rs` (create)\n\
                    ### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("missing-file")),
            "localized markers recognised: {issues:?}"
        );
        // The localized set must NOT mask true misses: an unmarked absent
        // file (and an `(editar)`-marked one, which claims to exist) still WARN.
        let body2 = "# Spec\n## Arquivos\n- `ghost.rs`\n- `gone.rs` (editar)\n\
                     ### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body2).unwrap();
        let issues2 = validate(&path, body2);
        let missing: Vec<&Value> = issues2
            .iter()
            .filter(|i| i["type"] == json!("missing-file"))
            .collect();
        assert_eq!(missing.len(), 2, "true misses still flagged: {issues2:?}");
        // The hint names the accepted markers from the shared i18n origin.
        let msg = missing[0]["message"].as_str().unwrap_or_default();
        assert!(msg.contains("(create)") && msg.contains("(novo)"), "hint lists synonyms: {msg}");
    }

    /// Roundtrip (leitor defensivo): a LEGACY spec already on disk with the
    /// duplicated AC heading (placeholder first, real list second) still
    /// validates clean — the defensive `section_block` returns the parseable
    /// section instead of the placeholder that used to trigger
    /// `unparseable-ac`.
    #[test]
    fn roundtrip_legacy_duplicated_ac_heading_still_validates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n\n## Critérios de Aceitação\n\nVer abaixo.\n\n\
                    ## Critérios de Aceitação\n\n\
                    - **AC-1** — workspace builds green.\n  Command: `cargo build`\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("unparseable-ac")),
            "legacy duplicated AC section parses: {issues:?}"
        );
        assert!(issues.is_empty(), "legacy spec validates ok:true: {issues:?}");
    }

    /// No AC section at all → behaviour unchanged (no `unparseable-ac`).
    #[test]
    fn ac_format_validation_absent_section_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        let body = "# Spec\n## Files\n- `a.rs` (create)\n### Backend Agent\n- [ ] t1\n- [ ] t2\n";
        std::fs::write(&path, body).unwrap();
        let issues = validate(&path, body);
        assert!(
            !issues.iter().any(|i| i["type"] == json!("unparseable-ac")),
            "{issues:?}"
        );
        // The clean-spec baseline stays clean overall.
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn flags_layer_gap() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("spec.md");
        std::fs::write(
            &path,
            "## Files\n- `a.txt` (create)\n### Frontend Agent\n- [ ] t1\n- [ ] t2\n",
        )
        .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let issues = validate(&path, &content);
        assert!(issues.iter().any(|i| i["type"] == json!("layer-gap")));
    }
}
