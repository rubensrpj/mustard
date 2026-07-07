//! `template_budget` — the leanness gate for the `.md` template corpus.
//!
//! The 2026-07-07 audit measured the template layer at 42k words with heavy
//! emphasis inflation; the project's own history shows compliance comes from
//! gates, not prose volume. This test makes "lean" an executable invariant:
//! it FAILS when any template grows past its budget, so the fat cannot creep
//! back one "important note" at a time. Rationale lives in
//! `docs/TEMPLATE-RATIONALE.md`, never in the loaded templates.
//!
//! Budgets (whitespace-separated words):
//! - Dieted files: a strict per-file cap + an emphasis cap (bold pairs
//!   ≤ 1 per 200 words).
//! - Every other template: the global cap. New templates are born under it.

use std::path::{Path, PathBuf};

/// Global word cap for any template not listed in [`STRICT_BUDGETS`].
const GLOBAL_WORD_CAP: usize = 1_500;

/// Per-file strict caps for the dieted templates (path relative to
/// `templates/`, forward slashes).
const STRICT_BUDGETS: &[(&str, usize)] = &[
    ("CLAUDE.md", 1_000),
    ("commands/mustard/feature/SKILL.md", 1_200),
    ("pipeline-config.md", 1_200),
    ("skills/pipeline-execution/SKILL.md", 1_200),
    ("refs/feature/spec-language.md", 700),
];

/// Bold-pair emphasis cap for dieted files: at most 1 per this many words.
const WORDS_PER_BOLD: usize = 200;

fn templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("templates")
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

fn rel(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn template_budget_word_caps_hold() {
    let root = templates_dir();
    let mut files = Vec::new();
    collect_md(&root, &mut files);
    assert!(!files.is_empty(), "no templates found under {}", root.display());

    let mut violations: Vec<String> = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let name = rel(path, &root);
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
        "templates over their word budget — trim them (law → checklist, how-to → table, \
         why → docs/TEMPLATE-RATIONALE.md):\n{}",
        violations.join("\n"),
    );
}

#[test]
fn template_budget_emphasis_cap_holds_on_dieted_files() {
    let root = templates_dir();
    let mut violations: Vec<String> = Vec::new();
    for (name, _) in STRICT_BUDGETS {
        let path = root.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            violations.push(format!("{name}: missing dieted template"));
            continue;
        };
        let words = word_count(&text);
        let bolds = bold_pairs(&text);
        let cap = (words / WORDS_PER_BOLD).max(1);
        if bolds > cap {
            violations.push(format!(
                "{name}: {bolds} bold pairs for {words} words (cap {cap} — 1 per {WORDS_PER_BOLD})"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "dieted templates over the emphasis budget — when everything shouts, nothing does:\n{}",
        violations.join("\n"),
    );
}
