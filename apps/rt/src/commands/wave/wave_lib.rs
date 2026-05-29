//! Shared wave helpers — a port of `scripts/_lib/wave-lib.js`.
//!
//! `detect_role` and `parse_files_section` are used by `wave-dependency`,
//! `exec-rewave-check` and `wave-size-check`.

use crate::commands::spec::spec_sections::is_heading;
use mustard_core::{glob_matches, ProjectConfig, RolePattern};
use std::path::Path;

/// Built-in keyword categories — the pre-F0-e classifier, kept byte-identical so
/// known stacks do not regress. The first matching category wins, in order
/// schema → api → ui → test.
const BUILTIN_ROLES: &[(&str, &[&str])] = &[
    ("schema", &["schema", "migration", "entity", "model", "drizzle", "prisma"]),
    ("api", &["api", "controller", "route", "endpoint", "handler", "service"]),
    ("ui", &["ui", "component", "view", "page", "screen", "widget"]),
    ("test", &["test", "spec", "__tests__"]),
];

/// Generic folder segments that carry no architectural signal — a file whose
/// only differentiating segment is one of these falls through to `"lib"` so the
/// agnostic structural fallback matches the historical `lib` bucket rather than
/// inventing a noisy role. Language-neutral structural names only.
const GENERIC_SEGMENTS: &[&str] = &[
    "lib", "libs", "util", "utils", "helper", "helpers", "common", "shared",
    "core", "internal", "src", "app", "pkg",
];

/// Classify `file_path` into a coarse architectural role.
///
/// Resolution order, first match wins:
/// 1. `patterns` (`mustard.json#rolePatterns`) — user overrides take priority,
///    so a non-English project can name its own layers.
/// 2. The built-in keyword categories ([`BUILTIN_ROLES`]) — known stacks stay
///    byte-identical.
/// 3. **Agnostic structural fallback** (F0-e): instead of dumping everything not
///    matched by an English keyword into `"lib"` (which collapses `layerCount`
///    to 1 for any non-JS project), derive the role from the file's most
///    significant directory segment. A file under `handlers/` becomes role
///    `"handlers"`, under `models/` becomes `"models"`, so structurally-layered
///    projects surface `layerCount >= 2` even when no keyword matches. Files
///    with only a generic segment (`util`, `lib`, `src`, …) or no directory
///    keep the historical `"lib"` bucket.
#[must_use]
pub fn detect_role_with(file_path: &str, patterns: &[RolePattern]) -> String {
    let lower = file_path.to_lowercase();

    // 1. mustard.json overrides (first listed match wins).
    for rp in patterns {
        if glob_matches(&rp.pattern, &lower) {
            return rp.role.clone();
        }
    }

    // 2. Built-in keyword categories (unchanged behaviour for known stacks).
    for (role, needles) in BUILTIN_ROLES {
        if needles.iter().any(|n| lower.contains(n)) {
            return (*role).to_string();
        }
    }

    // 3. Agnostic structural fallback — derive a role from the directory
    //    structure so distinct folders yield distinct roles.
    structural_role(&lower)
}

/// Derive a role from a path's directory segments when no keyword matched.
///
/// Picks the deepest non-generic directory segment (the folder immediately
/// holding the file, walking up past generic wrappers like `src`/`util`). Falls
/// back to `"lib"` when every segment is generic or the path is a bare
/// filename — matching the historical bucket for genuinely unstructured files.
fn structural_role(lower_path: &str) -> String {
    let norm = lower_path.replace('\\', "/");
    let segments: Vec<&str> = norm.split('/').collect();
    // Drop the filename (last segment); consider only directory segments.
    let dir_segments = match segments.split_last() {
        Some((_file, dirs)) => dirs,
        None => return "lib".to_string(),
    };
    // Deepest meaningful (non-generic, non-empty) directory segment.
    for seg in dir_segments.iter().rev() {
        let seg = seg.trim();
        if seg.is_empty() || GENERIC_SEGMENTS.contains(&seg) {
            continue;
        }
        return seg.to_string();
    }
    "lib".to_string()
}

/// Load `mustard.json#rolePatterns` from `project_root`, fail-open to empty.
///
/// Convenience for the wave gates (`wave-size-check`, `exec-rewave-check`,
/// `wave-dependency`) that have a project root in hand and want the override
/// applied to every [`detect_role_with`] call.
#[must_use]
pub fn load_role_patterns(project_root: &Path) -> Vec<RolePattern> {
    ProjectConfig::load(project_root).role_patterns()
}

/// Whether a trimmed line starts a new `## ` section (any heading).
fn is_section_break(trimmed: &str) -> bool {
    if let Some(rest) = trimmed.strip_prefix("##") {
        // JS `/^##\s/` — `##` followed by exactly one whitespace char.
        return rest.starts_with([' ', '\t']);
    }
    false
}

/// Parse the leading `- path` / `- \`path\`` bullet of a line, mirroring the JS
/// `/^-\s+`?([^\s`]+)`?/` pattern. Returns the captured path.
fn parse_bullet(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('-')?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix('`').unwrap_or(rest);
    // `[^\s`]+` — take chars up to whitespace or a backtick.
    let token: &str = rest
        .split(|c: char| c.is_whitespace() || c == '`')
        .next()
        .unwrap_or("");
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Parse the `## Files` section of a spec and return the listed paths.
///
/// Returns `None` when the section is absent; `Some(vec![])` when present but
/// empty.
#[must_use]
pub fn parse_files_section(spec_text: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = spec_text.split('\n').collect();
    let start = lines.iter().position(|l| is_heading(l, "files"))?;

    let mut paths = Vec::new();
    for line in lines.iter().skip(start + 1) {
        let trimmed = line.trim();
        if is_section_break(trimmed) {
            break;
        }
        if let Some(token) = parse_bullet(trimmed) {
            if !token.starts_with('#') {
                paths.push(token.to_string());
            }
        }
    }
    Some(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_role_classifies_paths() {
        // Known-keyword categories stay byte-identical (no regression).
        assert_eq!(detect_role_with("src/schema/user.ts", &[]), "schema");
        assert_eq!(detect_role_with("src/api/users.ts", &[]), "api");
        assert_eq!(detect_role_with("src/components/Btn.tsx", &[]), "ui");
        assert_eq!(detect_role_with("src/foo.test.ts", &[]), "test");
        // Generic-only directory → historical `lib` bucket.
        assert_eq!(detect_role_with("src/util/helpers.ts", &[]), "lib");
        assert_eq!(detect_role_with("main.rs", &[]), "lib");
    }

    #[test]
    fn detect_role_structural_fallback_differentiates_non_keyword_dirs() {
        // A non-JS project whose folders carry no English keyword must NOT
        // collapse every file into `lib` — distinct folders ⇒ distinct roles.
        assert_eq!(detect_role_with("src/repositorios/usuario.rb", &[]), "repositorios");
        assert_eq!(detect_role_with("src/dominio/pedido.rb", &[]), "dominio");
        // The deepest non-generic segment wins, skipping generic wrappers.
        assert_eq!(detect_role_with("src/util/dominio/x.rb", &[]), "dominio");
    }

    #[test]
    fn detect_role_with_respects_role_patterns() {
        let patterns = vec![
            RolePattern { pattern: "repositorios".to_string(), role: "schema".to_string() },
            RolePattern { pattern: "controladores".to_string(), role: "api".to_string() },
        ];
        assert_eq!(detect_role_with("src/repositorios/usuario.rb", &patterns), "schema");
        assert_eq!(detect_role_with("src/controladores/x.rb", &patterns), "api");
        // Unmatched by override → structural fallback still applies.
        assert_eq!(detect_role_with("src/vistas/y.rb", &patterns), "vistas");
    }

    #[test]
    fn parse_files_reads_bullets() {
        let spec = "# Spec\n\n## Files\n- src/a.ts\n- `src/b.ts`\n\n## Tasks\n- [ ] x\n";
        let files = parse_files_section(spec).unwrap();
        assert_eq!(files, vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn parse_files_absent_section_is_none() {
        assert!(parse_files_section("# Spec\n\n## Tasks\n").is_none());
    }

    #[test]
    fn parse_files_empty_section_is_empty_vec() {
        let files = parse_files_section("## Files\n\n## Tasks\n").unwrap();
        assert!(files.is_empty());
    }
}
