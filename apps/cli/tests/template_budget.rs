//! `template_budget` — the leanness gate for the `.md` template corpus.
//!
//! The 2026-07-07 audit measured the template layer at 42k words with heavy
//! emphasis inflation; the project's own history shows compliance comes from
//! gates, not prose volume. This test makes "lean" an executable invariant:
//! it FAILS when any template grows past its budget, so the fat cannot creep
//! back one "important note" at a time. Rationale lives in
//! `docs/TEMPLATE-RATIONALE.md`, never in the loaded templates.
//!
//! Mustard 2.0: the command/skill/ref corpus now ships in the `plugin/` tree;
//! `templates/` carries only what init seeds — notably the `templates/mustard/`
//! injectable instruction files the session hooks splice as
//! `additionalContext`. The budget scan therefore walks `plugin/` plus
//! `templates/mustard/`.
//!
//! Budgets (whitespace-separated words):
//! - Dieted files: a strict per-file cap + an emphasis cap (bold pairs
//!   <= 1 per 200 words).
//! - Every other template: the global cap. New templates are born under it.
//! - Injectables additionally get a CHARACTER cap: the harness truncates an
//!   `additionalContext` payload at 10_000 characters, so each injectable must
//!   stay under 9_500 (margin for the composition separator + siblings).

use std::path::{Path, PathBuf};

/// Global word cap for any template not listed in [`STRICT_BUDGETS`].
const GLOBAL_WORD_CAP: usize = 1_500;

/// Per-file strict caps for the dieted templates. Paths are relative to the
/// `plugin/` tree.
const STRICT_BUDGETS: &[(&str, usize)] = &[
    ("commands/feature.md", 1_200),
    ("pipeline-config.md", 1_200),
    ("refs/feature/spec-language.md", 700),
];

/// Bold-pair emphasis cap for dieted files: at most 1 per this many words.
const WORDS_PER_BOLD: usize = 200;

/// Character cap per injectable template (`templates/mustard/*.md`). The real
/// `additionalContext` ceiling is 10_000 characters; 9_500 leaves margin.
const INJECTABLE_CHAR_CAP: usize = 9_500;

/// The `plugin/` tree — home of the command/skill/ref corpus in Mustard 2.0.
fn plugin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin")
}

/// The `templates/` tree — only the files init seeds (settings, `.gitignore`,
/// and the `mustard/` injectables).
fn templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("templates")
}

/// Absolute path of a [`STRICT_BUDGETS`] entry — all live under `plugin/`.
fn strict_path(name: &str) -> PathBuf {
    plugin_dir().join(name)
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Bold pairs = occurrences of `**` divided by two (a `**bold**` span has two).
fn bold_pairs(text: &str) -> usize {
    text.matches("**").count() / 2
}

/// Budget name for a scanned file: its path relative to `plugin/`, or the bare
/// file name when it lives outside the plugin (the seeded `templates/mustard/`
/// injectables).
fn budget_name(path: &Path, plugin_root: &Path) -> String {
    match path.strip_prefix(plugin_root) {
        Ok(rel) => rel.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"),
        Err(_) => path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

#[test]
fn template_budget_word_caps_hold() {
    let plugin = plugin_dir();
    let mut files = Vec::new();
    collect_md(&plugin, &mut files);
    // The injectable templates ship under templates/mustard/ (init seeds them).
    collect_md(&templates_dir().join("mustard"), &mut files);
    assert!(!files.is_empty(), "no templates found under {}", plugin.display());

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let name = budget_name(path, &plugin);
        let words = word_count(&text);
        let cap = STRICT_BUDGETS
            .iter()
            .find(|(f, _)| *f == name)
            .map_or(GLOBAL_WORD_CAP, |(_, cap)| *cap);
        if words > cap {
            violations.push(format!("{name}: {words} words (cap {cap})"));
        }
    }
    assert!(
        violations.is_empty(),
        "templates over their word budget - trim them (law -> checklist, how-to -> table, \
         why -> docs/TEMPLATE-RATIONALE.md):\n{}",
        violations.join("\n"),
    );
}

#[test]
fn template_budget_emphasis_cap_holds_on_dieted_files() {
    let mut violations: Vec<String> = Vec::new();
    for (name, _) in STRICT_BUDGETS {
        let path = strict_path(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            violations.push(format!("{name}: missing dieted template"));
            continue;
        };
        let words = word_count(&text);
        let bolds = bold_pairs(&text);
        let cap = (words / WORDS_PER_BOLD).max(1);
        if bolds > cap {
            violations.push(format!(
                "{name}: {bolds} bold pairs for {words} words (cap {cap} - 1 per {WORDS_PER_BOLD})"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "dieted templates over the emphasis budget - when everything shouts, nothing does:\n{}",
        violations.join("\n"),
    );
}

/// Every injectable template must fit the `additionalContext` payload with
/// margin: the harness caps that payload at 10_000 characters, and an
/// injectable that exceeds it would be truncated mid-sentence at runtime —
/// silently. 9_500 leaves room for the composition separators and any sibling
/// block injected in the same hook response.
#[test]
fn injectable_templates_fit_the_additional_context_cap() {
    let dir = templates_dir().join("mustard");
    let mut files = Vec::new();
    collect_md(&dir, &mut files);
    assert!(
        !files.is_empty(),
        "no injectable templates found under {} — init would seed nothing",
        dir.display()
    );

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            violations.push(format!("{}: unreadable", path.display()));
            continue;
        };
        let chars = text.chars().count();
        if chars > INJECTABLE_CHAR_CAP {
            violations.push(format!(
                "{}: {chars} characters (cap {INJECTABLE_CHAR_CAP} — the harness truncates additionalContext at 10_000)",
                path.display()
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "injectable templates over the additionalContext budget:\n{}",
        violations.join("\n"),
    );
}
