//! `post_edit` — the consolidated PostToolUse(Write|Edit) module.
//!
//! ## Scope (b3 Wave 4, Write/Edit family)
//!
//! This module consolidates four JavaScript hooks, all `PostToolUse(Write|Edit)`.
//! Two are pure side effects (`Observer`), two reach a verdict (`Check`):
//!
//! - `auto-format.js` — an **`Observer`**: runs Prettier / `dotnet format` on
//!   the just-written file. Fire-and-forget — no verdict.
//! - `checklist-auto-mark.js` — an **`Observer`**: silently marks Checklist
//!   items in the active spec when the edited file matches an item. No verdict.
//! - `guard-verify.js` — a **`Check`**: verifies a production file edit against
//!   critical architectural rules; a critical violation `block`s, a boundary
//!   mismatch is an advisory.
//! - `pipeline-phase.js` — an **`Observer`**: emits a `pipeline.phase` event
//!   when a pipeline-state file's phase changes. No verdict.
//!
//! `PostEdit` therefore implements **both** [`Check`] (guard-verify) and
//! [`Observer`] (auto-format + checklist-auto-mark + pipeline-phase) — the same
//! dual shape `budget` and `bash_guard` use.
//!
//! Consolidation **regroups, it does not re-decide** — every verdict is a 1:1
//! port of the JS decision logic. Parity tests mirror `__tests__/hooks.test.js`
//! ("guard-verify.js"), `__tests__/checklist-mark.test.js`, and the
//! `pipeline-phase` block of `__tests__/harness-dual-emission.test.js`.
//!
//! ## Verdict note (guard-verify)
//!
//! `guard-verify.js` is a PostToolUse hook that writes the `decision:
//! "block"/"approve"` protocol. The `mustard-core` contract has one blocking
//! [`Verdict::Deny`] and the dispatcher encodes it as `permissionDecision`.
//! The **verdict** (block on a critical violation) is preserved exactly; only
//! the wire encoding normalises. A boundary mismatch — advisory in the JS — is
//! an [`Verdict::Inject`].

use mustard_core::error::Error;
use mustard_core::io::event_store::{EventSink, JsonlEventStore};
use mustard_core::model::contract::{Check, Ctx, HookInput, Observer, Trigger, Verdict};
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The consolidated PostToolUse(Write|Edit) module.
pub struct PostEdit;

// ===========================================================================
// Shared helpers
// ===========================================================================

/// The `file_path` of a Write/Edit invocation.
fn file_path_of(input: &HookInput) -> Option<String> {
    let ti = &input.tool_input;
    ti.get("file_path")
        .or_else(|| ti.get("path"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The cwd for an invocation — the harness `cwd`, else `.`.
fn project_dir(input: &HookInput, ctx: &Ctx) -> String {
    if !ctx.project_dir.is_empty() {
        return ctx.project_dir.clone();
    }
    match input.cwd.as_deref() {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => ".".to_string(),
    }
}

/// `true` if this is a `Write` or `Edit` tool invocation.
fn is_write_or_edit(input: &HookInput) -> bool {
    matches!(input.tool_name.as_deref(), Some("Write") | Some("Edit"))
}

/// An RFC-3339 / ISO-8601 UTC timestamp string (`YYYY-MM-DDThh:mm:ss.sssZ`).
fn now_iso8601() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

/// The basename (last `/`- or `\`-separated segment) of a path.
fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

// ===========================================================================
// guard-verify — Check on PostToolUse(Write|Edit)
// ===========================================================================

/// Path-segment patterns whose match means the file is skipped entirely.
/// Mirrors `SKIP_PATTERNS` in `guard-verify.js`.
fn is_skipped_path(rel: &str) -> bool {
    let p = rel.replace('\\', "/");
    p.contains("node_modules")
        || p.contains(".next/")
        || p.contains("/bin/")
        || p.contains("/obj/")
        || p.contains("/dist/")
        || p.contains("/_backup/")
        || p.contains(".claude/")
        || p.contains(".git/")
        || p.contains("migrations/")
}

/// The new content of a Write/Edit — `new_string` (Edit) or `content` (Write).
fn new_content_of(input: &HookInput) -> String {
    let ti = &input.tool_input;
    ti.get("new_string")
        .or_else(|| ti.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// The module name from a `.NET` module path: `Modules/v{N}/{Module}`.
fn module_of(rel: &str) -> Option<String> {
    let p = rel.replace('\\', "/");
    let idx = p.find("Modules/")?;
    let after = &p[idx + "Modules/".len()..];
    // Expect `v<digits>/<Module>`.
    let mut segs = after.split('/');
    let v = segs.next()?;
    if !v.starts_with('v') || !v[1..].chars().all(|c| c.is_ascii_digit()) || v.len() < 2 {
        return None;
    }
    let module = segs.next()?;
    // The module token: `\w+` — alphanumeric/underscore.
    let module: String = module
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if module.is_empty() { None } else { Some(module) }
}

/// Check the new content against the critical architectural rules. Returns the
/// list of violation messages. Port of `checkCriticalRules`.
fn check_critical_rules(content: &str, rel: &str) -> Vec<String> {
    let mut violations: Vec<String> = Vec::new();
    let p = rel.replace('\\', "/");
    let in_services = scope_services(&p);
    let in_repository = p.to_ascii_lowercase().contains("repositor");

    // Rule: `\bDbContext\b` in a Service that is not a Repository.
    if in_services && !in_repository && contains_word_ci(content, "dbcontext") {
        violations.push(format!(
            "L7: DbContext proibido em Services — use Repository (in {rel})"
        ));
    }
    // Rule: a `\w+Repository` referenced from a Service, cross-module.
    if in_services && content_has_repository_ref(content) {
        if let Some(module) = module_of(rel) {
            let module_base = module.to_ascii_lowercase();
            let module_base = module_base.strip_suffix('s').unwrap_or(&module_base);
            if has_cross_module_repository(content, module_base) {
                violations.push(format!(
                    "L8: cross-module SEMPRE via Service, NUNCA Repository (in {rel})"
                ));
            }
        }
    }
    // Rule: `new \w+(Service|Repository)(` in a `.cs` file.
    if p.ends_with(".cs") && content_has_new_service_or_repository(content) {
        violations.push(format!(
            "DIP: inject interface, NEVER concrete class (in {rel})"
        ));
    }
    // Rule: `\b(uint|int)\s+\w*[Ii]d\b` in a `.cs` file.
    if p.ends_with(".cs") && content_has_int_id(content) {
        violations.push(format!(
            "IDs must be Guid (UUIDv7), never int/uint (in {rel})"
        ));
    }
    // Rule: `directClient` in an `app/api/` route.
    if (p.contains("app/api/") || p.contains("app\\api\\")) && content.contains("directClient") {
        violations.push(format!(
            "API routes NUNCA usam directClient — use backend-client.ts (in {rel})"
        ));
    }
    violations
}

/// `true` if a path is scoped under a `Services/` or `Service/` directory.
fn scope_services(p: &str) -> bool {
    p.contains("Services/") || p.contains("Service/")
}

/// `true` if `content` contains `\b\w+Repository\b`.
fn content_has_repository_ref(content: &str) -> bool {
    word_followed_by(content, "Repository")
}

/// `true` if `content` references a cross-module `I?<Module>Repository`.
/// `module_base` is the lowercased, de-pluralised owning module.
fn has_cross_module_repository(content: &str, module_base: &str) -> bool {
    // The JS regex is `\bI?([A-Z]\w+)Repository\b`.
    let bytes = content.as_bytes();
    let mut i = 0;
    while let Some(rel) = content[i..].find("Repository") {
        let end_rel = i + rel;
        let suffix_end = end_rel + "Repository".len();
        // Right boundary: `\b` after `Repository`.
        let right_ok = bytes
            .get(suffix_end)
            .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'));
        if right_ok {
            // Walk backwards over `[A-Z]\w+` then an optional leading `I`.
            let mut start = end_rel;
            while start > 0 {
                let b = bytes[start - 1];
                if b.is_ascii_alphanumeric() || b == b'_' {
                    start -= 1;
                } else {
                    break;
                }
            }
            // `start..end_rel` is the type name preceding `Repository`.
            let type_name = &content[start..end_rel];
            // Must start with an uppercase letter (`[A-Z]\w+`), at least 2 chars.
            if type_name.len() >= 2 && type_name.starts_with(|c: char| c.is_ascii_uppercase()) {
                // Strip a leading `I` (interface convention) for the name test.
                let repo_name = type_name
                    .strip_prefix('I')
                    .filter(|r| r.starts_with(|c: char| c.is_ascii_uppercase()))
                    .unwrap_or(type_name);
                let repo_lower = repo_name.to_ascii_lowercase();
                // Same-module if the repo name shares the module base.
                let same_module =
                    repo_lower.contains(module_base) || module_base.contains(repo_lower.as_str());
                if !same_module {
                    return true;
                }
            }
        }
        i = suffix_end;
    }
    false
}

/// `true` if `content` matches `new \w+(Service|Repository)\(`.
fn content_has_new_service_or_repository(content: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = content[from..].find("new ") {
        let start = from + rel;
        let after = &content[start + 4..];
        let after = after.trim_start();
        // `\w+` then `(Service|Repository)` then `(`.
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            for suffix in ["Service", "Repository"] {
                if name.ends_with(suffix) {
                    let rest = &after[name.len()..];
                    if rest.starts_with('(') {
                        return true;
                    }
                }
            }
        }
        from = start + 4;
    }
    false
}

/// `true` if `content` matches `\b(uint|int)\s+\w*[Ii]d\b`.
fn content_has_int_id(content: &str) -> bool {
    for keyword in ["int", "uint"] {
        let mut from = 0;
        while let Some(rel) = content[from..].find(keyword) {
            let start = from + rel;
            let end = start + keyword.len();
            let bytes = content.as_bytes();
            let left_ok = start == 0 || !is_word_byte(bytes[start - 1]);
            let rest = &content[end..];
            let trimmed = rest.trim_start();
            let had_ws = trimmed.len() < rest.len();
            if left_ok && had_ws {
                // `\w*[Ii]d\b` — an identifier ending in `Id`/`id`.
                let ident: String = trimmed
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if ident.len() >= 2 {
                    let tail = &ident[ident.len() - 2..];
                    if (tail == "Id" || tail == "id")
                        && trimmed
                            .as_bytes()
                            .get(ident.len())
                            .is_none_or(|&b| !is_word_byte(b))
                    {
                        return true;
                    }
                }
            }
            from = end;
        }
    }
    false
}

/// `true` if `content` contains `word` followed by an identifier char (a
/// `\bword\w` shape, used for `\b\w+Repository`-style probes).
fn word_followed_by(content: &str, word: &str) -> bool {
    let mut from = 0;
    let bytes = content.as_bytes();
    while let Some(rel) = content[from..].find(word) {
        let start = from + rel;
        let left_ok = start == 0 || is_word_byte(bytes[start - 1]);
        // `\b\w+Repository\b`: at least one word char must precede `Repository`.
        let end = start + word.len();
        let right_ok = bytes
            .get(end)
            .is_none_or(|&b| !(b.is_ascii_alphanumeric() || b == b'_'));
        if left_ok && right_ok {
            return true;
        }
        from = end;
    }
    false
}

/// `true` if `content` contains `word` (case-insensitive) with word
/// boundaries — the `\bword\b` shape.
fn contains_word_ci(content: &str, word_lower: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(word_lower) {
        let start = from + rel;
        let end = start + word_lower.len();
        let left_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let right_ok = bytes.get(end).is_none_or(|&b| !is_word_byte(b));
        if left_ok && right_ok {
            return true;
        }
        from = end;
    }
    false
}

/// `true` for an ASCII word byte (alphanumeric or `_`).
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan `.cs` import statements for cross-module Repository / DbContext
/// imports. Port of `analyzeImports`.
fn analyze_imports(rel: &str, content: &str) -> Vec<String> {
    let p = rel.replace('\\', "/");
    if !p.ends_with(".cs") {
        return Vec::new();
    }
    let Some(current_module) = module_of(rel) else {
        return Vec::new();
    };
    let is_service = scope_services(&p);
    let is_repository = {
        let lower = p.to_ascii_lowercase();
        lower.contains("repository/") || lower.contains("repositories/")
    };
    let mut violations: Vec<String> = Vec::new();
    // `using\s+[\w.]+\.Modules\.v\d+\.(\w+)\.([\w.]*)`.
    for line in content.split('\n') {
        let Some((import_module, import_path)) = parse_module_using(line) else {
            continue;
        };
        if is_service
            && import_module != current_module
            && import_path.to_ascii_lowercase().contains("repositor")
        {
            violations.push(format!(
                "L8: importing {import_module}.{import_path} from {current_module} \
                 Service — use Service instead"
            ));
        }
        if !is_repository && import_path.to_ascii_lowercase().contains("dbcontext") {
            violations.push(format!(
                "L7: DbContext import in non-Repository file ({rel})"
            ));
        }
    }
    violations
}

/// Parse a `using X.Modules.v<N>.<Module>.<Path>` line into `(Module, Path)`.
fn parse_module_using(line: &str) -> Option<(String, String)> {
    let t = line.trim_start();
    let rest = t.strip_prefix("using")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let idx = rest.find(".Modules.v")?;
    let after = &rest[idx + ".Modules.v".len()..];
    // Skip the version digits then a `.`.
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let after = after[digits.len()..].strip_prefix('.')?;
    // `(\w+)` — the module.
    let module: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if module.is_empty() {
        return None;
    }
    let after = &after[module.len()..];
    let after = after.strip_prefix('.').unwrap_or(after);
    // `([\w.]*)` — the import path, up to a `;` / whitespace.
    let import_path: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
        .collect();
    Some((module, import_path))
}

/// Scan active specs for a `## Boundaries` section the edited file violates.
/// Advisory only — returns the warning message or `None`. Port of
/// `checkBoundaries`.
fn check_boundaries(file_path: &str, cwd: &str) -> Option<String> {
    let spec_root = Path::new(cwd).join(".claude").join("spec").join("active");
    let entries = std::fs::read_dir(&spec_root).ok()?;
    let normalized_edit = file_path.replace('\\', "/");

    for entry in entries.filter_map(std::result::Result::ok) {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().into_owned();
        let spec_file = entry.path().join("spec.md");
        let Ok(content) = std::fs::read_to_string(&spec_file) else {
            continue;
        };
        let Some(lines) = boundary_block_lines(&content) else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }
        // Does the edited file match any declared boundary?
        let mut matched = false;
        for pattern in &lines {
            let pattern = pattern.replace('\\', "/");
            if pattern.is_empty() {
                continue;
            }
            if pattern.ends_with('/') {
                if normalized_edit.contains(&pattern) || normalized_edit.starts_with(&pattern) {
                    matched = true;
                    break;
                }
                continue;
            }
            if pattern.contains('*') || pattern.contains('?') {
                if glob_loose_match(&normalized_edit, &pattern) {
                    matched = true;
                    break;
                }
                continue;
            }
            if normalized_edit.ends_with(&pattern) || normalized_edit == pattern {
                matched = true;
                break;
            }
        }
        if matched {
            return None;
        }
        // No match in this spec — the first spec with a boundary section wins.
        let rel_edited = file_path.replace('\\', "/");
        return Some(format!(
            "\"{rel_edited}\" is outside the boundaries declared in spec \
             \"{dir_name}\". Declared: {}. Verify this edit is intentional.",
            lines.join(", ")
        ));
    }
    None
}

/// Extract the cleaned bullet lines of a spec's `## Boundaries` block.
fn boundary_block_lines(content: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        let lower = line.trim().to_ascii_lowercase();
        if h2_named(&lower, "boundaries") || h2_named(&lower, "limites") {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
    let mut out: Vec<String> = Vec::new();
    for line in &lines[start..] {
        if line.starts_with("## ") || line.trim() == "---" {
            break;
        }
        // `.replace(/^[-*]\s+`?/, '').replace(/`.*/, '').trim()`.
        let mut cleaned = line.trim_start();
        if let Some(rest) = cleaned.strip_prefix('-').or_else(|| cleaned.strip_prefix('*')) {
            cleaned = rest.trim_start().trim_start_matches('`');
        } else {
            continue;
        }
        let cleaned = match cleaned.find('`') {
            Some(idx) => &cleaned[..idx],
            None => cleaned,
        };
        let cleaned = cleaned.trim();
        if !cleaned.is_empty() {
            out.push(cleaned.to_string());
        }
    }
    Some(out)
}

/// `true` if a lowercased line is an H2 heading whose name is exactly `name`.
fn h2_named(lower: &str, name: &str) -> bool {
    let Some(rest) = lower.strip_prefix("## ") else {
        return false;
    };
    let rest = rest.trim_start();
    if !rest.starts_with(name) {
        return false;
    }
    rest.as_bytes()
        .get(name.len())
        .is_none_or(|&b| !b.is_ascii_alphanumeric() && b != b'_')
}

/// A permissive glob match for boundary patterns (`**`→`.+`, `*`→one segment,
/// `?`→one char), tested as an unanchored "contains" like the JS
/// `new RegExp(regexStr).test(...)`.
fn glob_loose_match(text: &str, pattern: &str) -> bool {
    // Build segments; an unanchored search means trying every start position.
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();
    for start in 0..=tb.len() {
        if glob_loose_at(&tb[start..], pb) {
            return true;
        }
    }
    false
}

/// Anchored permissive glob walk. `**`/`*` consume ≥1 char (the JS uses `(.+)`
/// / `([^/]+)`), `?` consumes exactly one.
fn glob_loose_at(text: &[u8], pat: &[u8]) -> bool {
    if pat.is_empty() {
        return true; // unanchored tail — a partial match suffices
    }
    if pat.starts_with(b"**") {
        let rest = &pat[2..];
        // `(.+)` — one or more of anything.
        let mut i = 1;
        while i <= text.len() {
            if glob_loose_at(&text[i..], rest) {
                return true;
            }
            i += 1;
        }
        return false;
    }
    if pat[0] == b'*' {
        let rest = &pat[1..];
        // `([^/]+)` — one or more non-`/`.
        let mut i = 1;
        while i <= text.len() {
            if text[i - 1] == b'/' {
                return false;
            }
            if glob_loose_at(&text[i..], rest) {
                return true;
            }
            i += 1;
        }
        return false;
    }
    if pat[0] == b'?' {
        // `([^/])` — exactly one non-`/`.
        if text.is_empty() || text[0] == b'/' {
            return false;
        }
        return glob_loose_at(&text[1..], &pat[1..]);
    }
    if !text.is_empty() && text[0] == pat[0] {
        return glob_loose_at(&text[1..], &pat[1..]);
    }
    false
}

/// The `guard-verify` verdict for a `PostToolUse(Write|Edit)` invocation.
fn guard_verify(input: &HookInput, cwd: &str) -> Verdict {
    if !is_write_or_edit(input) {
        return Verdict::Allow;
    }
    let Some(file_path) = file_path_of(input) else {
        return Verdict::Allow;
    };
    // `path.relative(ROOT, filePath)` — relative to cwd, forward-slash.
    let rel = relative_to_cwd(cwd, &file_path);
    if is_skipped_path(&rel) {
        return Verdict::Allow;
    }
    let content = new_content_of(input);
    if content.is_empty() {
        return Verdict::Allow;
    }
    let mut violations = check_critical_rules(&content, &rel);
    violations.extend(analyze_imports(&rel, &content));
    if !violations.is_empty() {
        let msgs = violations
            .iter()
            .map(|v| format!("CRITICAL: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        return Verdict::Deny {
            reason: format!(
                "Guard Enforcement BLOCKED:\n{msgs}\n\nFix these violations before \
                 proceeding."
            ),
        };
    }
    // Advisory: a boundary mismatch.
    if let Some(warning) = check_boundaries(&file_path, cwd) {
        return Verdict::Inject {
            context: format!("[BOUNDARY WARNING] {warning}"),
        };
    }
    Verdict::Allow
}

/// `file_path` relative to `cwd`, forward-slash normalised. When `file_path`
/// is not under `cwd` it is returned normalised as-is (the JS `path.relative`
/// would produce a `../`-prefixed path; `is_skipped_path` handles neither
/// specially, and the rule scopes still apply on the raw path).
fn relative_to_cwd(cwd: &str, file_path: &str) -> String {
    let cwd_norm = cwd.replace('\\', "/");
    let fp_norm = file_path.replace('\\', "/");
    let prefix = format!("{}/", cwd_norm.trim_end_matches('/'));
    fp_norm
        .strip_prefix(&prefix)
        .map_or(fp_norm.clone(), str::to_string)
}

// ===========================================================================
// auto-format — Observer on PostToolUse(Write|Edit)
// ===========================================================================

/// Extensions Prettier handles. Mirrors `PRETTIER_EXTS`.
const PRETTIER_EXTS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".json", ".css", ".md", ".html", ".scss",
];

/// The lowercase extension of a path (including the dot), or `""`.
fn extension(path: &str) -> String {
    let base = basename(path);
    match base.rfind('.') {
        Some(idx) if idx > 0 => base[idx..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// `auto-format`: run the appropriate formatter on the just-written file.
///
/// Pure side effect — fail-open throughout, no verdict. Mirrors
/// `auto-format.js`: Prettier for the JS/TS/CSS/MD family (only when a
/// Prettier config or `node_modules/.bin/prettier` is present), `dotnet
/// format` for `.cs`.
fn run_auto_format(input: &HookInput, cwd: &str) {
    let Some(file_path) = file_path_of(input) else {
        return;
    };
    if file_path.is_empty() {
        return;
    }
    // The file must exist on disk (the JS `fs.existsSync` guard).
    if !Path::new(&file_path).exists() {
        return;
    }
    let ext = extension(&file_path);
    if PRETTIER_EXTS.contains(&ext.as_str()) {
        run_prettier(&file_path, cwd);
    } else if ext == ".cs" {
        run_dotnet_format(&file_path);
    }
}

/// Run Prettier on `file_path` when a Prettier setup is detected under `cwd`
/// (or its parent — monorepo). Best-effort.
fn run_prettier(file_path: &str, cwd: &str) {
    let has_prettier = ["node_modules/.bin/prettier", ".prettierrc", ".prettierrc.js", ".prettierrc.json", "prettier.config.js"]
        .iter()
        .any(|rel| Path::new(cwd).join(rel).exists());
    let parent_has = Path::new(cwd)
        .parent()
        .is_some_and(|p| p.join("node_modules/.bin/prettier").exists());
    if !has_prettier && !parent_has {
        return;
    }
    // `npx prettier --write "<file>"`.
    let _ = Command::new("npx")
        .args(["prettier", "--write", file_path])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Run `dotnet format` on `file_path`, scoping to the nearest `.sln`/`.csproj`.
/// Best-effort.
fn run_dotnet_format(file_path: &str) {
    // Walk up to 5 directories for a `.sln` or `.csproj`.
    let mut search_dir = Path::new(file_path).parent().map(Path::to_path_buf);
    let mut project_file: Option<std::path::PathBuf> = None;
    for _ in 0..5 {
        let Some(dir) = search_dir.clone() else {
            break;
        };
        if let Ok(entries) = std::fs::read_dir(&dir) {
            let mut sln = None;
            let mut csproj = None;
            for entry in entries.filter_map(std::result::Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".sln") {
                    sln = Some(entry.path());
                } else if name.ends_with(".csproj") {
                    csproj = Some(entry.path());
                }
            }
            if let Some(p) = sln.or(csproj) {
                project_file = Some(p);
                break;
            }
        }
        let parent = search_dir.as_ref().and_then(|d| d.parent()).map(Path::to_path_buf);
        if parent == search_dir {
            break;
        }
        search_dir = parent;
    }
    let Some(project) = project_file else {
        return;
    };
    let Some(project_dir) = project.parent() else {
        return;
    };
    let _ = Command::new("dotnet")
        .args(["format"])
        .arg(&project)
        .args(["--include", file_path, "--no-restore"])
        .current_dir(project_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// ===========================================================================
// checklist-auto-mark — Observer on PostToolUse(Write|Edit)
// ===========================================================================

/// `checklist-auto-mark`: silently mark Checklist items in the active spec
/// that match the just-edited file.
///
/// Pure side effect — fail-open throughout, no verdict. Port of
/// `checklist-auto-mark.js`.
fn run_checklist_auto_mark(input: &HookInput, cwd: &str) {
    if !is_write_or_edit(input) {
        return;
    }
    let Some(file_path) = file_path_of(input) else {
        return;
    };
    if file_path.is_empty() {
        return;
    }
    let Some((spec_path, _spec_name)) = find_active_spec(cwd) else {
        return;
    };
    // Don't auto-mark when the edited file IS the spec itself (avoid loops).
    if same_path(&file_path, &spec_path) {
        return;
    }
    let Ok(raw) = std::fs::read_to_string(&spec_path) else {
        return;
    };
    let mut lines: Vec<String> = raw.split('\n').map(str::to_string).collect();
    let Some((start, end)) = find_checklist_section(&lines) else {
        return;
    };

    let edited_base = basename(&file_path).to_string();
    let norm_edited = file_path.replace('\\', "/").to_ascii_lowercase();
    let mut dirty = false;

    for line in lines.iter_mut().take(end).skip(start) {
        let Some((prefix, gap, text)) = parse_unchecked_item(line) else {
            continue;
        };
        let mut matched = false;
        // Strategy 1: arrow target — `… → <path>`.
        if let Some(target) = arrow_target(&text) {
            let target = target.replace('\\', "/").to_ascii_lowercase();
            if norm_edited.ends_with(&target)
                || norm_edited.contains(&format!("/{target}"))
                || norm_edited == target
            {
                matched = true;
            } else if basename(&target) == edited_base.to_ascii_lowercase() {
                matched = true;
            }
        }
        // Strategy 2: basename anywhere in the item text.
        if !matched
            && !edited_base.is_empty()
            && text
                .to_ascii_lowercase()
                .contains(&edited_base.to_ascii_lowercase())
        {
            matched = true;
        }
        if matched {
            *line = format!("{prefix}[x]{gap}{text}");
            dirty = true;
        }
    }

    if dirty {
        let _ = std::fs::write(&spec_path, lines.join("\n"));
    }
}

/// Parse a `- [ ] <text>` unchecked-item line into `(prefix, gap, text)`.
/// Mirrors the JS regex `^(\s*-\s+)\[ \](\s+)(.*)$`.
fn parse_unchecked_item(line: &str) -> Option<(String, String, String)> {
    // Leading whitespace + `-` + whitespace.
    let ws_end = line.len() - line.trim_start().len();
    let leading = &line[..ws_end];
    let rest = &line[ws_end..];
    let rest = rest.strip_prefix('-')?;
    let dash_gap_end = rest.len() - rest.trim_start().len();
    if dash_gap_end == 0 {
        return None; // `-` must be followed by whitespace
    }
    let prefix = format!("{leading}-{}", &rest[..dash_gap_end]);
    let rest = &rest[dash_gap_end..];
    let rest = rest.strip_prefix("[ ]")?;
    // The gap after `[ ]` — one or more whitespace.
    let gap_end = rest.len() - rest.trim_start().len();
    if gap_end == 0 {
        return None;
    }
    let gap = rest[..gap_end].to_string();
    let text = rest[gap_end..].to_string();
    Some((prefix, gap, text))
}

/// Extract an arrow-target path from a Checklist item — `… → <path>` or
/// `… > <path>`. Mirrors `/[→>]\s*([^\s].*?)\s*$/`.
fn arrow_target(text: &str) -> Option<String> {
    let idx = text.rfind(['→', '>'])?;
    // The arrow char width: `→` is 3 bytes, `>` is 1.
    let arrow_len = text[idx..].chars().next().map_or(1, char::len_utf8);
    let after = text[idx + arrow_len..].trim();
    if after.is_empty() {
        None
    } else {
        Some(after.to_string())
    }
}

/// Locate the `## Checklist` section: returns `(start, end)` line indices.
fn find_checklist_section(lines: &[String]) -> Option<(usize, usize)> {
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if is_checklist_heading(line) {
            start = Some(i + 1);
            break;
        }
    }
    let start = start?;
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start) {
        if line.starts_with("## ") || line == "##" {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// `true` if `line` is the `## Checklist` heading (`^##\s+Checklist\b`).
fn is_checklist_heading(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("##") else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = rest.trim_start();
    if !rest.starts_with("Checklist") {
        return false;
    }
    rest.as_bytes()
        .get("Checklist".len())
        .is_none_or(|&b| !is_word_byte(b))
}

/// Find the active spec for `cwd`. Strategy: the newest pipeline-state's
/// `spec`/`specName`, else the newest `.claude/spec/active/*/spec.md`.
/// Port of `findActiveSpec`. Returns `(spec_path, spec_name)`.
fn find_active_spec(cwd: &str) -> Option<(String, String)> {
    let claude = Path::new(cwd).join(".claude");
    if !claude.exists() {
        return None;
    }
    // Strategy 1: newest pipeline-state.
    let states = claude.join(".pipeline-states");
    if let Ok(entries) = std::fs::read_dir(&states) {
        let mut best: Option<(SystemTime, std::path::PathBuf)> = None;
        for entry in entries.filter_map(std::result::Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".json") || name.ends_with(".metrics.json") {
                continue;
            }
            let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
                best = Some((mtime, entry.path()));
            }
        }
        if let Some((_, path)) = best {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(obj) = serde_json::from_str::<Value>(&text) {
                    let name = obj
                        .get("spec")
                        .or_else(|| obj.get("specName"))
                        .and_then(|v| v.as_str());
                    if let Some(name) = name {
                        let candidate = claude
                            .join("spec")
                            .join("active")
                            .join(name)
                            .join("spec.md");
                        if candidate.exists() {
                            return Some((
                                candidate.to_string_lossy().into_owned(),
                                name.to_string(),
                            ));
                        }
                    }
                }
            }
        }
    }
    // Strategy 2: newest active spec dir.
    let active = claude.join("spec").join("active");
    let entries = std::fs::read_dir(&active).ok()?;
    let mut best: Option<(SystemTime, String, String)> = None;
    for entry in entries.filter_map(std::result::Result::ok) {
        let dir_name = entry.file_name().to_string_lossy().into_owned();
        let candidate = entry.path().join("spec.md");
        if !candidate.exists() {
            continue;
        }
        let Ok(mtime) = candidate.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(t, _, _)| mtime > *t) {
            best = Some((mtime, candidate.to_string_lossy().into_owned(), dir_name));
        }
    }
    best.map(|(_, path, name)| (path, name))
}

/// `true` if two paths resolve to the same file (canonicalised; falls back to
/// a normalised string compare when canonicalisation fails).
fn same_path(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a.replace('\\', "/") == b.replace('\\', "/"),
    }
}

// ===========================================================================
// pipeline-phase — Observer on PostToolUse(Write|Edit)
// ===========================================================================

/// `true` if `file_path` is a pipeline-state file
/// (`.claude/.pipeline-states/{name}.json`, excluding `*.metrics.json`).
fn is_pipeline_state_file(file_path: &str) -> bool {
    let p = file_path.replace('\\', "/");
    if !p.ends_with(".json") || p.ends_with(".metrics.json") {
        return false;
    }
    let Some(idx) = p.find("/.claude/.pipeline-states/") else {
        return false;
    };
    let rest = &p[idx + "/.claude/.pipeline-states/".len()..];
    // Exactly one path segment: `[^/]+\.json`.
    !rest.contains('/') && rest.len() > ".json".len()
}

/// `pipeline-phase`: emit a `pipeline.phase` event when a pipeline-state
/// file's phase changes.
///
/// Pure side effect — fail-open throughout, no verdict. Port of
/// `pipeline-phase.js`. The phase-cache lives at
/// `.claude/.harness/.phase-cache.json`.
fn run_pipeline_phase(input: &HookInput, cwd: &str) {
    if !is_write_or_edit(input) {
        return;
    }
    let Some(file_path) = file_path_of(input) else {
        return;
    };
    if !is_pipeline_state_file(&file_path) {
        return;
    }
    // Read the just-written pipeline-state.
    let Ok(text) = std::fs::read_to_string(&file_path) else {
        return;
    };
    let Ok(state) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let current_phase = state
        .get("phaseName")
        .or_else(|| state.get("phase"))
        .and_then(|v| v.as_str());
    let Some(current_phase) = current_phase else {
        return;
    };
    // Spec name from the filename.
    let spec = basename(&file_path)
        .strip_suffix(".json")
        .unwrap_or(basename(&file_path))
        .to_string();

    // Read the phase cache.
    let harness_dir = Path::new(cwd).join(".claude").join(".harness");
    let cache_file = harness_dir.join(".phase-cache.json");
    let mut cache: serde_json::Map<String, Value> = std::fs::read_to_string(&cache_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let previous_phase = cache.get(&spec).and_then(|v| v.as_str()).map(str::to_string);

    // Only emit when the phase actually changed.
    if previous_phase.as_deref() == Some(current_phase) {
        return;
    }
    // Update the cache.
    cache.insert(spec.clone(), Value::String(current_phase.to_string()));
    let _ = std::fs::create_dir_all(&harness_dir);
    let _ = std::fs::write(
        &cache_file,
        serde_json::to_string_pretty(&Value::Object(cache)).unwrap_or_default(),
    );

    // Emit the `pipeline.phase` event.
    let event = HarnessEvent {
        v: SCHEMA_VERSION,
        ts: now_iso8601(),
        // CONCERN: `Ctx` carries no session id / wave — emit "unknown" / 0
        // (the JS `getCurrentSessionId` / `getCurrentWave` fallbacks).
        session_id: input.session_id.clone().unwrap_or_else(|| "unknown".to_string()),
        wave: 0,
        actor: Actor {
            kind: ActorKind::Hook,
            id: Some("pipeline-phase".to_string()),
            actor_type: None,
        },
        event: "pipeline.phase".to_string(),
        payload: json!({
            "from": previous_phase,
            "to": current_phase,
        }),
        spec: Some(spec),
    };
    let _ = JsonlEventStore::for_project(cwd).append(&event);
}

// ===========================================================================
// Contract impls
// ===========================================================================

impl Check for PostEdit {
    /// `guard-verify`: gate a `PostToolUse(Write|Edit)` against the critical
    /// architectural rules. A critical violation `Deny`s; a boundary mismatch
    /// is an `Inject` advisory; everything else `Allow`s.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return Ok(Verdict::Allow);
        }
        if !is_write_or_edit(input) {
            return Ok(Verdict::Allow);
        }
        let cwd = project_dir(input, ctx);
        Ok(guard_verify(input, &cwd))
    }
}

impl Observer for PostEdit {
    /// Run the three fire-and-forget side effects of a `PostToolUse(Write|Edit)`:
    /// `auto-format`, `checklist-auto-mark`, `pipeline-phase`.
    ///
    /// Pure side effects — never affect a verdict. Fail-open throughout.
    fn observe(&self, input: &HookInput, ctx: &Ctx) {
        if ctx.trigger != Some(Trigger::PostToolUse) {
            return;
        }
        if !is_write_or_edit(input) {
            return;
        }
        let cwd = project_dir(input, ctx);
        run_auto_format(input, &cwd);
        run_checklist_auto_mark(input, &cwd);
        run_pipeline_phase(input, &cwd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn edit_input(file_path: &str, new_string: &str) -> HookInput {
        HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": file_path, "new_string": new_string }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        }
    }

    fn ctx(dir: &str) -> Ctx {
        Ctx {
            project_dir: dir.to_string(),
            trigger: Some(Trigger::PostToolUse),
        }
    }

    // --- guard-verify parity (hooks.test.js "guard-verify.js") -------------

    #[test]
    fn guard_blocks_dbcontext_in_services() {
        let input = edit_input(
            "/proj/src/Modules/v1/Users/Services/UserService.cs",
            "var ctx = new DbContext();",
        );
        let verdict = guard_verify(&input, "/proj");
        assert!(verdict.is_blocking(), "DbContext in Services must block");
    }

    #[test]
    fn guard_allows_dbcontext_in_repositories() {
        let input = edit_input(
            "/proj/src/Modules/v1/Users/Repositories/UserRepository.cs",
            "var ctx = new DbContext();",
        );
        // `new UserRepository(` would trip the DIP rule — use a plain field.
        let input = HookInput {
            tool_input: json!({
                "file_path": "/proj/src/Modules/v1/Users/Repositories/UserRepository.cs",
                "new_string": "private readonly DbContext _ctx;",
            }),
            ..input
        };
        assert_eq!(guard_verify(&input, "/proj"), Verdict::Allow);
    }

    #[test]
    fn guard_blocks_cross_module_repository_in_service() {
        let input = edit_input(
            "/proj/src/Modules/v1/Users/Services/UserService.cs",
            "private readonly ContractRepository _repo;",
        );
        assert!(guard_verify(&input, "/proj").is_blocking());
    }

    #[test]
    fn guard_allows_same_module_repository() {
        let input = edit_input(
            "/proj/src/Modules/v1/Users/Services/UserService.cs",
            "private readonly UserRepository _repo;",
        );
        assert_eq!(guard_verify(&input, "/proj"), Verdict::Allow);
    }

    #[test]
    fn guard_skips_claude_files() {
        let input = edit_input(
            "/proj/.claude/hooks/some-hook.js",
            "DbContext something bad int UserId",
        );
        assert_eq!(guard_verify(&input, "/proj"), Verdict::Allow);
    }

    #[test]
    fn guard_blocks_int_id_in_cs() {
        let input = edit_input(
            "/proj/src/Models/User.cs",
            "public int UserId { get; set; }",
        );
        assert!(guard_verify(&input, "/proj").is_blocking());
    }

    #[test]
    fn guard_skip_patterns_recognised() {
        assert!(is_skipped_path("src/.claude/x.js"));
        assert!(is_skipped_path("a/node_modules/b.ts"));
        assert!(is_skipped_path("pkg/dist/out.js"));
        assert!(!is_skipped_path("src/Models/User.cs"));
    }

    #[test]
    fn guard_via_check_only_post_tool_use() {
        let input = edit_input("/proj/src/Models/User.cs", "public int UserId { get; set; }");
        // PreToolUse trigger → the Check self-allows.
        let pre_ctx = Ctx {
            project_dir: "/proj".to_string(),
            trigger: Some(Trigger::PreToolUse),
        };
        assert_eq!(
            PostEdit.evaluate(&input, &pre_ctx).expect("no error"),
            Verdict::Allow
        );
        // PostToolUse → blocks.
        assert!(
            PostEdit
                .evaluate(&input, &ctx("/proj"))
                .expect("no error")
                .is_blocking()
        );
    }

    // --- checklist-auto-mark parity (checklist-mark.test.js) ---------------

    /// Write a spec + pipeline-state under `dir`, returning the spec.md path.
    fn setup_spec(dir: &Path, spec_name: &str, body: &str) -> std::path::PathBuf {
        let spec_dir = dir
            .join(".claude")
            .join("spec")
            .join("active")
            .join(spec_name);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let spec_file = spec_dir.join("spec.md");
        std::fs::write(&spec_file, body).unwrap();
        let states = dir.join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        std::fs::write(
            states.join(format!("{spec_name}.json")),
            json!({ "spec": spec_name, "phase": "EXECUTE" }).to_string(),
        )
        .unwrap();
        spec_file
    }

    #[test]
    fn checklist_marks_basename_pista() {
        let dir = tempdir().unwrap();
        let spec_file = setup_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [ ] Update UserService.cs to add validation\n\
             - [ ] Write docs\n",
        );
        let edited = dir
            .path()
            .join("src")
            .join("Services")
            .join("UserService.cs");
        let input = edit_input(&edited.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let updated = std::fs::read_to_string(&spec_file).unwrap();
        assert!(updated.contains("- [x] Update UserService.cs"));
        assert!(updated.contains("- [ ] Write docs"));
    }

    #[test]
    fn checklist_marks_arrow_target() {
        let dir = tempdir().unwrap();
        let spec_file = setup_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [ ] Add validation → src/Services/UserService.cs\n",
        );
        let edited = dir
            .path()
            .join("src")
            .join("Services")
            .join("UserService.cs");
        let input = edit_input(&edited.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let updated = std::fs::read_to_string(&spec_file).unwrap();
        assert!(updated.contains("- [x] Add validation"));
    }

    #[test]
    fn checklist_does_not_mark_when_no_pista() {
        let dir = tempdir().unwrap();
        let spec_file = setup_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [ ] Refactor OtherFile.ts\n",
        );
        let edited = dir.path().join("src").join("Unrelated.cs");
        let input = edit_input(&edited.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let updated = std::fs::read_to_string(&spec_file).unwrap();
        assert!(updated.contains("- [ ] Refactor OtherFile.ts"));
    }

    #[test]
    fn checklist_does_not_loop_on_spec_itself() {
        let dir = tempdir().unwrap();
        let spec_file = setup_spec(
            dir.path(),
            "demo",
            "# Spec\n\n## Checklist\n\n- [ ] Edit spec.md notes\n",
        );
        // Editing the spec itself must not auto-mark.
        let input = edit_input(&spec_file.to_string_lossy(), "whatever");
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let updated = std::fs::read_to_string(&spec_file).unwrap();
        assert!(updated.contains("- [ ] Edit spec.md notes"));
    }

    #[test]
    fn checklist_observe_infallible_without_spec() {
        let dir = tempdir().unwrap();
        let input = edit_input(
            &dir.path().join("src").join("Any.cs").to_string_lossy(),
            "x",
        );
        // No spec at all — observe must not panic.
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
    }

    // --- pipeline-phase parity (harness-dual-emission.test.js) -------------

    #[test]
    fn pipeline_phase_emits_on_phase_change() {
        let dir = tempdir().unwrap();
        let states = dir.path().join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        let state_file = states.join("add-login.json");
        std::fs::write(
            &state_file,
            json!({ "spec": "add-login", "phaseName": "ANALYZE" }).to_string(),
        )
        .unwrap();
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({ "file_path": state_file.to_string_lossy() }),
            hook_event_name: Some("PostToolUse".to_string()),
            session_id: Some("s-test".to_string()),
            ..HookInput::default()
        };
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let events = JsonlEventStore::for_project(dir.path().to_str().unwrap())
            .replay()
            .unwrap();
        let phase = events.iter().find(|e| e.event == "pipeline.phase");
        assert!(phase.is_some(), "pipeline.phase event must be emitted");
        let phase = phase.unwrap();
        assert_eq!(phase.payload["to"], json!("ANALYZE"));
        assert_eq!(phase.payload["from"], Value::Null);
    }

    #[test]
    fn pipeline_phase_does_not_emit_when_phase_unchanged() {
        let dir = tempdir().unwrap();
        let states = dir.path().join(".claude").join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        let state_file = states.join("stable.json");
        std::fs::write(
            &state_file,
            json!({ "spec": "stable", "phaseName": "EXECUTE" }).to_string(),
        )
        .unwrap();
        let input = HookInput {
            tool_name: Some("Edit".to_string()),
            tool_input: json!({ "file_path": state_file.to_string_lossy() }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let project = dir.path().to_str().unwrap();
        PostEdit.observe(&input, &ctx(project));
        PostEdit.observe(&input, &ctx(project));
        let events = JsonlEventStore::for_project(project).replay().unwrap();
        let phase_count = events.iter().filter(|e| e.event == "pipeline.phase").count();
        assert_eq!(phase_count, 1, "unchanged phase must not re-emit");
    }

    #[test]
    fn pipeline_phase_ignores_non_state_files() {
        let dir = tempdir().unwrap();
        let input = edit_input(
            &dir.path().join("src").join("app.js").to_string_lossy(),
            "x",
        );
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
        let events = JsonlEventStore::for_project(dir.path().to_str().unwrap())
            .replay()
            .unwrap();
        assert!(events.iter().all(|e| e.event != "pipeline.phase"));
    }

    #[test]
    fn pipeline_state_file_recognition() {
        assert!(is_pipeline_state_file(
            "/p/.claude/.pipeline-states/add-login.json"
        ));
        assert!(!is_pipeline_state_file(
            "/p/.claude/.pipeline-states/add-login.metrics.json"
        ));
        assert!(!is_pipeline_state_file("/p/src/app.json"));
    }

    // --- auto-format -------------------------------------------------------

    #[test]
    fn auto_format_skips_missing_file() {
        // The file does not exist — run_auto_format must be a silent no-op.
        let dir = tempdir().unwrap();
        let input = edit_input(
            &dir.path().join("nonexistent.ts").to_string_lossy(),
            "const x=1;",
        );
        // Must not panic.
        run_auto_format(&input, dir.path().to_str().unwrap());
    }

    #[test]
    fn extension_extraction() {
        assert_eq!(extension("/a/b/file.TS"), ".ts");
        assert_eq!(extension("/a/b/SKILL.md"), ".md");
        assert_eq!(extension("/a/b/noext"), "");
        assert_eq!(extension("/a/.hidden"), "");
    }

    #[test]
    fn observe_is_infallible() {
        // observe must never panic regardless of payload shape.
        let dir = tempdir().unwrap();
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            tool_input: json!({}),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        PostEdit.observe(&input, &ctx(dir.path().to_str().unwrap()));
    }
}
