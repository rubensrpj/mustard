//! The cleanup: what Mustard once wrote into files that are not its own. It is
//! an older Mustard's own leftover, so the upsert takes it out with no
//! question and says what left.
//!
//! Three kinds of file carry it:
//!
//! - the instruction files (`CLAUDE.md` and `CLAUDE.local.md`), where an older
//!   scan wrote the `@.claude/scan-map.md` import, the `> Parent: … |
//!   Orchestrator: …` line and the `## Guards` block between
//!   `<!-- mustard:guards -->` and `<!-- /mustard:guards -->`;
//! - the team's `.claude/settings.json`, where an older install wrote the lines
//!   of its seed (its deny rules stay: a protection rule never leaves the
//!   team's file unless someone asks);
//! - `.claude/CLAUDE.md`, the orchestrator an older install planted.
//!
//! [`plan`] only reads, and says what would leave; [`apply`] does it. Nothing
//! is staged or committed: the commit is the person's.
//!
//! Mustard's marks are the block marks and the import line. Only what sits
//! between the block marks, the import line and the breadcrumb line leaves an
//! instruction file; every other byte stays, line endings included. A file is
//! deleted only when nothing is left but the title Mustard wrote (and the
//! `## Guards` heading that held the block). A file with the breadcrumb or the
//! `## Guards` heading of an older scan and no mark at all is not touched: it
//! is listed, and the person decides. Each guard of the guards block that
//! leaves becomes a project-rule lesson, once for the place it holds, before
//! any file changes; the old map block leaves without becoming one.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::io::claude_paths::ClaudePaths;
use crate::io::fs::{self, PRUNE_DIRS};
use crate::io::lessons;
use crate::platform::error::Result;

use super::settings::{parse_json_object, team_settings_path, without_seed_lines, TEAM_SETTINGS};
use super::{CLAUDE_LOCAL_MD, CLAUDE_MD};

/// The import line an older scan wrote at the top of an instruction file.
const SCAN_MAP_IMPORT_LINE: &str = "@.claude/scan-map.md";

/// Opening of the guards block, with or without the `pending` word. It is the
/// only block that holds rules.
const GUARDS_OPEN: &str = "<!-- mustard:guards";

/// Openings of the blocks Mustard wrote, with the closing of each. The old map
/// block held the scan's summary of the folder, not rules.
const BLOCKS: &[(&str, &str)] = &[
    (GUARDS_OPEN, "<!-- /mustard:guards -->"),
    ("<!-- mustard:scan-map", "<!-- /mustard:scan-map -->"),
];

/// The heading an older scan wrote above the guards block.
const GUARDS_HEADING: &str = "## Guards";

/// Marker of the orchestrator an older install planted as `.claude/CLAUDE.md`.
const ORCHESTRATOR_MARKER: &str = "# Orchestrator Rules";

/// The planted orchestrator, relative to the project root.
const PLANTED_ORCHESTRATOR: &str = ".claude/CLAUDE.md";

/// How deep the search for instruction files goes. A project deeper than this
/// is not one an older scan wrote into.
const MAX_DEPTH: usize = 12;

/// The class a guard becomes in the lesson bank.
const PROJECT_RULE: &str = "project_rule";

/// How many words of a guard become the keys of its lesson.
const KEY_WORDS: usize = 6;

/// What happens to one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// The file stays, without the listed lines.
    Edit,
    /// Nothing but Mustard's own is left in it: the file goes.
    Delete,
}

/// One file the cleanup would change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    /// Project-root-relative, with forward slashes.
    pub path: String,
    pub action: Action,
    /// What leaves the file, one entry per line or block, in file order.
    pub removes: Vec<String>,
}

/// One guard that leaves an instruction file and becomes a lesson.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardLesson {
    /// The guard, as it was written.
    pub text: String,
    /// The file it came from.
    pub source: String,
    /// The subproject it holds for; `None` for the project root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subproject: Option<String>,
}

/// What the cleanup would do, read before anything changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub files: Vec<FileChange>,
    /// Instruction files with the traces of an older scan and no mark:
    /// listed for the person to decide, never touched.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmarked: Vec<String>,
    /// The guards that would become lessons, those already in the bank left
    /// out.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lessons: Vec<GuardLesson>,
}

impl CleanupPlan {
    /// Nothing to take out and nothing to list.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.unmarked.is_empty() && self.lessons.is_empty()
    }

    /// Whether taking the list out would change anything.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.files.is_empty() || !self.lessons.is_empty()
    }
}

/// What [`apply`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupDone {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub edited: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<String>,
    /// The numbers the new lessons got in the bank.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lessons: Vec<u64>,
    /// What could not be done, with the reason.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<String>,
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// What the cleanup would do in the project at `root`. Only reads.
#[must_use]
pub fn plan(root: &Path) -> CleanupPlan {
    let mut out = CleanupPlan::default();

    let planted = root.join(PLANTED_ORCHESTRATOR);
    if fs::read_to_string(&planted).is_ok_and(|text| text.contains(ORCHESTRATOR_MARKER)) {
        out.files.push(FileChange {
            path: PLANTED_ORCHESTRATOR.to_string(),
            action: Action::Delete,
            removes: vec![ORCHESTRATOR_MARKER.to_string()],
        });
    }

    let mut guards = Vec::new();
    for rel in instruction_files(root) {
        let Ok(text) = fs::read_to_string(root.join(&rel)) else { continue };
        match strip_marks(&text) {
            Stripped::Untouched => {}
            Stripped::Unmarked => out.unmarked.push(rel),
            Stripped::Changed { removes, delete, guards: found, .. } => {
                let subproject = subproject_of(&rel);
                guards.extend(found.into_iter().map(|text| GuardLesson {
                    text,
                    source: rel.clone(),
                    subproject: subproject.clone(),
                }));
                out.files.push(FileChange {
                    path: rel,
                    action: if delete { Action::Delete } else { Action::Edit },
                    removes,
                });
            }
        }
    }

    if let Some(change) = team_settings_change(root) {
        out.files.push(change);
    }

    out.lessons = new_lessons(root, guards);
    out
}

/// The team's settings file without the seed's lines, when it has any.
fn team_settings_change(root: &Path) -> Option<FileChange> {
    let raw = fs::read_to_string(team_settings_path(root)).ok()?;
    let parsed = serde_json::from_str::<Value>(&raw).ok()?;
    let (left, removes) = without_seed_lines(parsed.as_object()?);
    if removes.is_empty() {
        return None;
    }
    Some(FileChange {
        path: TEAM_SETTINGS.to_string(),
        action: if left.is_empty() { Action::Delete } else { Action::Edit },
        removes,
    })
}

/// The guards not yet in the bank, each once for the place it holds: a guard
/// is known by its text and by where it holds, in the bank as in this pass.
/// The same guard in two subprojects is two lessons; in one subproject, the
/// versioned instruction file wins over its local twin, which usually repeats
/// it.
fn new_lessons(root: &Path, mut guards: Vec<GuardLesson>) -> Vec<GuardLesson> {
    guards.sort_by_key(|g| g.source.ends_with(CLAUDE_LOCAL_MD));
    let known: Vec<(String, Value)> = lesson_bank(root)
        .and_then(|path| lessons::read(&path).ok().flatten())
        .map(|bank| {
            bank.visible()
                .into_iter()
                .filter(|lesson| lesson.event_type == PROJECT_RULE)
                .filter_map(|lesson| {
                    Some((normalized(lesson.str_field("text")?), lesson.fields.get("applies_to")?.clone()))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut seen = known;
    let mut out = Vec::new();
    for guard in guards {
        let key = (normalized(&guard.text), applies_to(guard.subproject.as_deref()));
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(guard);
    }
    out
}

/// Where a guard's lesson holds: its subproject, or the whole project. The
/// lesson is written with it and found in the bank by it.
fn applies_to(subproject: Option<&str>) -> Value {
    match subproject {
        Some(sub) => serde_json::json!({ "subproject": sub }),
        None => serde_json::json!({ "files": [crate::domain::lessons::WHOLE_PROJECT] }),
    }
}

fn lesson_bank(root: &Path) -> Option<PathBuf> {
    ClaudePaths::for_project(root).ok().map(|paths| paths.lessons_path())
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The subproject an instruction file sits in: its folder, or `None` at the
/// project root.
fn subproject_of(rel: &str) -> Option<String> {
    let dir = rel.rsplit_once('/').map(|(dir, _)| dir)?;
    (!dir.is_empty()).then(|| dir.to_string())
}

/// Every `CLAUDE.md` and `CLAUDE.local.md` of the project, relative, sorted.
///
/// Build output, dependencies and every folder whose name starts with a dot
/// are skipped: a worktree under `.claude/` is a whole other checkout.
fn instruction_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if name.starts_with('.') || PRUNE_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(root, &path, depth + 1, out);
        } else if kind.is_file() && (name == CLAUDE_MD || name == CLAUDE_LOCAL_MD)
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

// ---------------------------------------------------------------------------
// One instruction file
// ---------------------------------------------------------------------------

/// What reading one instruction file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Stripped {
    /// Nothing of Mustard's in it.
    Untouched,
    /// Traces of an older scan and no mark (neither the import nor a block),
    /// or a block that never closes: the person decides.
    Unmarked,
    Changed {
        /// The file without Mustard's lines, every other byte as it was.
        text: String,
        removes: Vec<String>,
        /// Nothing but the skeleton is left.
        delete: bool,
        guards: Vec<String>,
    },
}

/// Take Mustard's lines out of an instruction file's `text`.
pub(super) fn strip_marks(text: &str) -> Stripped {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let content = |line: &str| -> String {
        let line = line.strip_suffix('\n').unwrap_or(line);
        line.strip_suffix('\r').unwrap_or(line).to_string()
    };

    let mut keep = String::with_capacity(text.len());
    let mut removes = Vec::new();
    let mut guards = Vec::new();
    let mut breadcrumbs = false;
    // The import line is a mark, as much as a block is.
    let mut marks = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = content(lines[i]);
        let trimmed = line.trim();
        if trimmed == SCAN_MAP_IMPORT_LINE {
            marks += 1;
            removes.push(SCAN_MAP_IMPORT_LINE.to_string());
            i += 1;
            continue;
        }
        if is_breadcrumb(trimmed) {
            breadcrumbs = true;
            removes.push(trimmed.to_string());
            i += 1;
            continue;
        }
        if let Some((open, close)) = BLOCKS.iter().find(|(open, _)| trimmed.starts_with(open)) {
            let Some(end) = (i + 1..lines.len()).find(|j| content(lines[*j]).trim() == *close) else {
                return Stripped::Unmarked;
            };
            if *open == GUARDS_OPEN {
                guards.extend(guards_in(&lines[i + 1..end].iter().map(|l| content(l)).collect::<Vec<_>>()));
            }
            removes.push(format!("{open} … {close}", open = open.trim_end()));
            marks += 1;
            i = end + 1;
            continue;
        }
        keep.push_str(lines[i]);
        i += 1;
    }

    if marks == 0 {
        return if breadcrumbs || has_guards_heading(text) { Stripped::Unmarked } else { Stripped::Untouched };
    }
    let delete = only_skeleton(&keep);
    Stripped::Changed { text: keep, removes, delete, guards }
}

/// The breadcrumb an older scan wrote under the title: `> Parent: … |
/// Orchestrator: …` in a subproject, `> Orchestrator: …` at the root.
fn is_breadcrumb(line: &str) -> bool {
    (line.starts_with("> Parent: [") && line.contains(" | Orchestrator: ["))
        || line.starts_with("> Orchestrator: [")
}

fn has_guards_heading(text: &str) -> bool {
    text.lines().any(|l| l.trim() == GUARDS_HEADING)
}

/// Whether what is left is only what an older scan wrote around the block:
/// one title and the `## Guards` heading.
fn only_skeleton(text: &str) -> bool {
    let mut titles = 0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line == GUARDS_HEADING {
            continue;
        }
        if line.starts_with("# ") && titles == 0 {
            titles += 1;
            continue;
        }
        return false;
    }
    true
}

/// The guards inside a block, without the comments the block carried for the
/// scan. Guards come as `- ` items or as plain lines of prose, and every line
/// starts a guard of its own, except one that is indented right under a guard
/// (no blank line between): that one continues it.
fn guards_in(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Whether the last guard can still take an indented line.
    let mut open = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            open = false;
            continue;
        }
        if trimmed.starts_with("<!--") {
            continue;
        }
        let indented = line.starts_with(char::is_whitespace);
        match (trimmed.strip_prefix("- "), out.last_mut()) {
            (None, Some(last)) if open && indented => {
                last.push(' ');
                last.push_str(trimmed);
            }
            (item, _) => {
                out.push(item.unwrap_or(trimmed).trim().to_string());
                open = true;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Applying the plan
// ---------------------------------------------------------------------------

/// Do what [`plan`] listed.
///
/// The guards become lessons first. When one of them cannot be written, no
/// file is touched, so no guard leaves a file without having become a lesson;
/// the next run finds the ones already written in the bank and retries only
/// the rest. After that every file is handled on its own: a failure is
/// reported and the rest goes on.
///
/// # Errors
///
/// None today; the signature keeps room for a failure that stops everything.
pub fn apply(root: &Path, plan: &CleanupPlan) -> Result<CleanupDone> {
    let mut done = CleanupDone::default();
    if !plan.lessons.is_empty() {
        let Some(bank) = lesson_bank(root) else {
            done.failed.push("lessons: the lesson bank of this project could not be found".to_string());
            return Ok(done);
        };
        for guard in &plan.lessons {
            match lessons::write(&bank, lesson_draft(guard), None) {
                Ok(written) => done.lessons.push(written.id),
                Err(refusal) => done.failed.push(format!("{}: {refusal:?}", guard.source)),
            }
        }
        if !done.failed.is_empty() {
            return Ok(done);
        }
    }
    for change in &plan.files {
        let path = root.join(&change.path);
        let result = match change.action {
            Action::Delete => fs::remove_file(&path).map(|()| done.deleted.push(change.path.clone())),
            Action::Edit => rewrite(root, &change.path).map(|()| done.edited.push(change.path.clone())),
        };
        if let Err(err) = result {
            done.failed.push(format!("{}: {err}", change.path));
        }
    }
    Ok(done)
}

/// Rewrite one file without Mustard's lines, as [`plan`] read it.
fn rewrite(root: &Path, rel: &str) -> Result<()> {
    let path = root.join(rel);
    if rel == TEAM_SETTINGS {
        let raw = fs::read_to_string(&path)?;
        let (left, _) = without_seed_lines(&parse_json_object(&raw));
        let mut body = serde_json::to_string_pretty(&Value::Object(left))?;
        body.push('\n');
        return fs::write_atomic(&path, body.as_bytes());
    }
    let text = fs::read_to_string(&path)?;
    if let Stripped::Changed { text, .. } = strip_marks(&text) {
        fs::write_atomic(&path, text.as_bytes())?;
    }
    Ok(())
}

/// The lesson a guard becomes: a project rule that holds where the guard held
/// (its subproject, or the whole project), born in the file it came from.
fn lesson_draft(guard: &GuardLesson) -> Map<String, Value> {
    let draft = serde_json::json!({
        "class": PROJECT_RULE,
        "author": "binary",
        "text": guard.text,
        "keys": lesson_keys(guard),
        "applies_to": applies_to(guard.subproject.as_deref()),
        "found_in": { "source": guard.source },
    });
    draft.as_object().cloned().unwrap_or_default()
}

/// The keys of a guard's lesson: the subproject's name and the first long
/// words of the guard, lowercased, each once.
fn lesson_keys(guard: &GuardLesson) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    if let Some(name) = guard.subproject.as_deref().and_then(|s| s.rsplit('/').next()) {
        keys.push(name.to_lowercase());
    }
    let words = guard
        .text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 5 && !w.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_lowercase);
    for word in words {
        if keys.len() > KEY_WORDS {
            break;
        }
        if !keys.contains(&word) {
            keys.push(word);
        }
    }
    if keys.is_empty() {
        keys.push("guard".to_string());
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as std_fs;
    use tempfile::tempdir;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std_fs::create_dir_all(path.parent().unwrap()).unwrap();
        std_fs::write(path, body).unwrap();
    }

    const SUBPROJECT_MD: &str = "@.claude/scan-map.md\n\n# Rt\n\n> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/mustard/orchestrator.md](../../.claude/mustard/orchestrator.md)\n\n## Guards\n\n<!-- mustard:guards -->\n<!-- facts: kind=cargo -->\n- Never panic in a hook.\n- Keep `main.rs` thin:\n  routing only.\n<!-- /mustard:guards -->\n";

    /// Um arquivo que só tem o que o Mustard escreveu sai inteiro, e as duas
    /// Guards dele viram lições, com a linha que continua a segunda.
    #[test]
    fn a_file_that_is_only_mustard_goes_and_its_guards_are_read() {
        let Stripped::Changed { delete, guards, removes, .. } = strip_marks(SUBPROJECT_MD) else {
            panic!("the marks were not found");
        };
        assert!(delete, "only the title and the heading are left");
        assert_eq!(guards, ["Never panic in a hook.", "Keep `main.rs` thin: routing only."]);
        assert_eq!(removes.len(), 3, "{removes:?}");
    }

    /// O texto da equipe em volta das marcas fica byte a byte, com os fins de
    /// linha dela.
    #[test]
    fn the_team_text_around_the_marks_stays_byte_for_byte() {
        let body = "# Api\r\n\r\nOur own rule.\r\n\r\n<!-- mustard:guards pending -->\r\n<!-- facts: x -->\r\n<!-- /mustard:guards -->\r\ntail\r\n";
        let Stripped::Changed { text, delete, guards, .. } = strip_marks(body) else {
            panic!("the marks were not found");
        };
        assert!(!delete);
        assert!(guards.is_empty(), "a pending block carries no guard");
        assert_eq!(text, "# Api\r\n\r\nOur own rule.\r\n\r\ntail\r\n");
    }

    /// Um arquivo com os rastros de um scan antigo (a linha de navegação ou o
    /// título das Guards) e sem marca nenhuma só é listado; um bloco que não
    /// fecha também; e um arquivo sem nada do Mustard fica de fora.
    #[test]
    fn unmarked_files_are_only_listed() {
        let crumb_only = "# Cli\n\n> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [x](x)\n\n## Guards\n\n- ours\n";
        assert_eq!(strip_marks(crumb_only), Stripped::Unmarked);
        assert_eq!(strip_marks("# Cli\n\n## Guards\n\n- ours\n"), Stripped::Unmarked);
        assert_eq!(strip_marks("# X\n<!-- mustard:guards -->\n- never closed\n"), Stripped::Unmarked);
        assert_eq!(strip_marks("# Team notes\n\nNothing else.\n"), Stripped::Untouched);
    }

    /// A linha do import é marca do Mustard: um arquivo só com o import, o
    /// título e a linha de navegação sai inteiro; num arquivo com texto da
    /// equipe, saem só o import e a linha de navegação, com os fins de linha
    /// do resto como estavam, e o título das Guards sem bloco fica.
    #[test]
    fn the_import_line_is_a_mark() {
        let only_ours = "@.claude/scan-map.md\n\n# Web\n\n> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/mustard/orchestrator.md](../../.claude/mustard/orchestrator.md)\n";
        let Stripped::Changed { delete, removes, guards, .. } = strip_marks(only_ours) else {
            panic!("the import is a mark");
        };
        assert!(delete, "only the title is left");
        assert!(guards.is_empty());
        assert_eq!(removes.len(), 2, "the import and the breadcrumb: {removes:?}");
        assert_eq!(removes[0], SCAN_MAP_IMPORT_LINE);

        let mixed = "@.claude/scan-map.md\r\n# Mine\r\n> Orchestrator: [x](x)\r\nrest\r\n";
        let Stripped::Changed { text, delete, .. } = strip_marks(mixed) else {
            panic!("the import is a mark");
        };
        assert!(!delete);
        assert_eq!(text, "# Mine\r\nrest\r\n");

        let team_guards = "@.claude/scan-map.md\n# Cli\n\n## Guards\n\n- ours\n";
        let Stripped::Changed { text, delete, guards, .. } = strip_marks(team_guards) else {
            panic!("the import is a mark");
        };
        assert!(!delete, "the team's guard is not the skeleton");
        assert!(guards.is_empty(), "a guard outside a block is never read as Mustard's");
        assert_eq!(text, "# Cli\n\n## Guards\n\n- ours\n");

        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/web/CLAUDE.md", only_ours);
        let listed = plan(root);
        assert_eq!(listed.files.len(), 1, "{listed:?}");
        assert_eq!((listed.files[0].path.as_str(), &listed.files[0].action), ("apps/web/CLAUDE.md", &Action::Delete));
        assert!(listed.unmarked.is_empty(), "a file with the import is not left for the person: {listed:?}");
        let done = apply(root, &listed).unwrap();
        assert_eq!(done.deleted, ["apps/web/CLAUDE.md"]);
    }

    /// Dentro do bloco, cada linha começa uma guard, com ou sem o `- `; só a
    /// linha recuada logo abaixo de uma guard continua essa guard, e uma
    /// linha em branco fecha a anterior.
    #[test]
    fn only_an_indented_line_right_under_a_guard_continues_it() {
        let lines: Vec<String> =
            ["<!-- facts: x -->", "first in prose", "- an item", "  that goes on", "second in prose", "", "  after a blank line", "- last"]
                .map(str::to_string)
                .to_vec();
        assert_eq!(
            guards_in(&lines),
            ["first in prose", "an item that goes on", "second in prose", "after a blank line", "last"],
        );
        assert_eq!(guards_in(&["  indented, with nothing before".to_string()]), ["indented, with nothing before"]);
    }

    /// As linhas de guard dentro do bloco de um arquivo, como o scan as
    /// escreveu: tudo o que não é comentário nem linha em branco.
    fn block_lines(body: &str) -> Vec<String> {
        body.lines()
            .skip_while(|l| !l.trim().starts_with("<!-- mustard:guards"))
            .skip(1)
            .take_while(|l| l.trim() != "<!-- /mustard:guards -->")
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("<!--"))
            .map(str::to_string)
            .collect()
    }

    /// Cópia do `CLAUDE.md` que o scan antigo escreveu na fixture em Go, com as
    /// guards em prosa, sem `- `. O teste guarda o texto: a própria limpeza
    /// apaga o arquivo original quando roda neste repositório.
    const SCAN_FIXTURE_MD: &str = "@.claude/scan-map.md

# Graph_go

> Parent: [../../../../../CLAUDE.md](../../../../../CLAUDE.md) | Orchestrator: [../../../../../.claude/mustard/orchestrator.md](../../../../../.claude/mustard/orchestrator.md)

## Guards

<!-- mustard:guards -->
<!-- facts: kind=go; frameworks=(none) -->
[critical] never import in internal/model/user.go
This directory is a frozen characterization fixture for `apps/scan/tests/graph_resolution.rs`: the non-regression test pins EXACTLY 1 graph edge whose fan-in target is `internal/model/user.go` — any new internal import, file rename, or extra edge breaks that recorded baseline, so update the test's expectations in the same change or don't touch the shape.
The `module example.test/graphdemo` line in `go.mod` and the import path in `internal/server/server.go` are one contract — module-prefixed resolution is the exact behavior under test, so change them only together and verbatim.
`internal/model/user.go` deliberately samples one of each Go definition shape (struct + method, interface, type alias) with zero imports — extend shapes inside it if needed, but keep it import-free so it stays the pure fan-in target.
Never make this fixture buildable or runnable (no `main`, no dependencies, no `go mod tidy`): the scan miner parses it with tree-sitter and never compiles it — minimality is the spec, and any \"fix\" toward a real app adds noise the tests will count.
<!-- /mustard:guards -->
";

    /// As guards que o scan escreveu em prosa, sem `- `, viram lições antes de
    /// o arquivo sair: a cópia do `CLAUDE.md` da fixture do scan em Go, numa
    /// pasta temporária, dá cinco lições, uma por linha, e só então é apagada.
    #[test]
    fn prose_guards_of_the_scan_fixtures_become_lessons_before_the_file_goes() {
        let body = SCAN_FIXTURE_MD;
        let expected = block_lines(body);
        assert_eq!(expected.len(), 5, "the fixture keeps its five guards: {expected:?}");
        assert_eq!(expected[0], "[critical] never import in internal/model/user.go");

        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/graph_go/CLAUDE.md", body);
        let listed = plan(root);
        let texts: Vec<&str> = listed.lessons.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, expected, "one lesson per guard line");
        assert_eq!(listed.files[0].action, Action::Delete);

        let done = apply(root, &listed).unwrap();
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(done.lessons.len(), 5);
        assert!(!root.join("apps/graph_go/CLAUDE.md").exists());
        let bank = lessons::read(&lesson_bank(root).unwrap()).unwrap().unwrap();
        let written: Vec<String> = bank
            .visible()
            .into_iter()
            .filter(|l| l.event_type == PROJECT_RULE)
            .filter_map(|l| l.str_field("text").map(str::to_string))
            .collect();
        assert_eq!(written, expected);
    }

    /// A mesma guard em dois subprojetos vale nos dois: vira uma lição para
    /// cada um, os dois arquivos saem, e a busca de lições de cada subprojeto
    /// acha a sua. O par `CLAUDE.md` e `CLAUDE.local.md` do mesmo subprojeto
    /// continua valendo uma lição só. E, com a lição de um subprojeto já no
    /// banco, a mesma guard noutro subprojeto ainda vira lição.
    #[test]
    fn the_same_guard_in_two_subprojects_is_two_lessons() {
        let body = "# Svc\n\n<!-- mustard:guards -->\n- Never call the database from a handler.\n<!-- /mustard:guards -->\n";
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/a/CLAUDE.md", body);
        write(root, "apps/a/CLAUDE.local.md", body);
        write(root, "apps/b/CLAUDE.md", body);

        let listed = plan(root);
        let places: Vec<Option<&str>> = listed.lessons.iter().map(|l| l.subproject.as_deref()).collect();
        assert_eq!(places, [Some("apps/a"), Some("apps/b")], "{:?}", listed.lessons);
        assert_eq!(listed.lessons[0].source, "apps/a/CLAUDE.md", "the versioned file wins over its local twin");

        let done = apply(root, &listed).unwrap();
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(done.lessons.len(), 2);
        assert_eq!(done.deleted.len(), 3, "{done:?}");
        let bank = lessons::read(&lesson_bank(root).unwrap()).unwrap().unwrap();
        for sub in ["apps/a", "apps/b"] {
            let scope = crate::domain::lessons::Scope { subproject: Some(sub.to_string()), ..Default::default() };
            let found = crate::domain::lessons::in_scope(&bank, &scope);
            assert_eq!(found.len(), 1, "{sub} keeps its rule");
            assert_eq!(found[0].str_field("text"), Some("Never call the database from a handler."));
        }

        let other = tempdir().unwrap();
        let root = other.path();
        write(root, "apps/a/CLAUDE.md", body);
        apply(root, &plan(root)).unwrap();
        write(root, "apps/b/CLAUDE.md", body);
        write(root, "apps/a/CLAUDE.local.md", body);
        let later = plan(root);
        let places: Vec<Option<&str>> = later.lessons.iter().map(|l| l.subproject.as_deref()).collect();
        assert_eq!(places, [Some("apps/b")], "the bank holds the text only for apps/a: {:?}", later.lessons);
    }

    /// O bloco antigo do mapa guardava o resumo da pasta, não regras: ele sai
    /// do arquivo sem virar lição nenhuma. Num arquivo com os dois blocos, só
    /// o das Guards vira lição.
    #[test]
    fn the_old_map_block_leaves_without_becoming_a_lesson() {
        let map_only = "# Dashboard\n\n> Parent: [../CLAUDE.md](../CLAUDE.md) | Orchestrator: [../.claude/CLAUDE.md](../.claude/CLAUDE.md)\n\n<!-- mustard:scan-map -->\nTipo: typescript · 10 arquivos\nPesquise via `mustard-rt run feature` (digest) — não leia o repo direto.\n<!-- /mustard:scan-map -->\n\n## Architecture\n\nHand-written prose that must NOT move.\n";
        let Stripped::Changed { text, guards, removes, delete } = strip_marks(map_only) else {
            panic!("the map block is a mark");
        };
        assert!(guards.is_empty(), "the map block holds no rule: {guards:?}");
        assert!(!delete);
        assert_eq!(removes.len(), 2, "the breadcrumb and the map block: {removes:?}");
        assert_eq!(text, "# Dashboard\n\n\n\n## Architecture\n\nHand-written prose that must NOT move.\n");

        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/dashboard/CLAUDE.md", map_only);
        let both = "# Web\n\n<!-- mustard:scan-map -->\nTipo: npm · 1 arquivos\n<!-- /mustard:scan-map -->\n\n## Guards\n\n<!-- mustard:guards -->\n- Keep pages free of inline styles.\n<!-- /mustard:guards -->\n";
        write(root, "apps/web/CLAUDE.md", both);
        let listed = plan(root);
        let texts: Vec<&str> = listed.lessons.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["Keep pages free of inline styles."], "{:?}", listed.lessons);

        let done = apply(root, &listed).unwrap();
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(done.lessons.len(), 1);
        assert_eq!(done.edited, ["apps/dashboard/CLAUDE.md"]);
        assert_eq!(done.deleted, ["apps/web/CLAUDE.md"]);
        assert!(!std_fs::read_to_string(root.join("apps/dashboard/CLAUDE.md")).unwrap().contains("Tipo:"));
    }

    /// Uma guard que não vira lição segura todos os arquivos: nada é apagado
    /// nem reescrito, e a falha é dita.
    #[test]
    fn a_guard_that_cannot_become_a_lesson_keeps_every_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/rt/CLAUDE.md", SUBPROJECT_MD);
        write(root, "apps/api/CLAUDE.md", "# Api\n\nOurs.\n<!-- mustard:guards -->\n- A guard.\n<!-- /mustard:guards -->\n");
        let listed = plan(root);
        assert_eq!(listed.lessons.len(), 3, "{listed:?}");
        // O banco de lições não pode ser escrito: no lugar dele há uma pasta.
        std_fs::create_dir_all(lesson_bank(root).unwrap()).unwrap();

        let done = apply(root, &listed).unwrap();
        assert!(!done.failed.is_empty(), "the failure is said: {done:?}");
        assert!(done.deleted.is_empty() && done.edited.is_empty(), "{done:?}");
        assert_eq!(std_fs::read_to_string(root.join("apps/rt/CLAUDE.md")).unwrap(), SUBPROJECT_MD);
        assert!(std_fs::read_to_string(root.join("apps/api/CLAUDE.md")).unwrap().contains("- A guard."));
    }

    /// O plano acha o orquestrador plantado, os arquivos marcados em qualquer
    /// subprojeto, o arquivo sem marca e o `settings.json` da equipe, e pula
    /// as pastas de compilação e as que começam com ponto.
    #[test]
    fn the_plan_finds_every_kind_and_skips_build_and_hidden_folders() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, ".claude/CLAUDE.md", "# Orchestrator Rules\n");
        write(root, "apps/rt/CLAUDE.md", SUBPROJECT_MD);
        write(root, "apps/rt/CLAUDE.local.md", SUBPROJECT_MD);
        write(root, "apps/cli/CLAUDE.md", "# Cli\n\n## Guards\n\n- ours\n");
        write(root, "target/x/CLAUDE.md", SUBPROJECT_MD);
        write(root, ".claude/worktrees/w/CLAUDE.md", SUBPROJECT_MD);
        write(root, ".claude/settings.json", "{\n  \"respectGitignore\": true,\n  \"team\": 1\n}\n");

        let plan = plan(root);
        let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [".claude/CLAUDE.md", "apps/rt/CLAUDE.local.md", "apps/rt/CLAUDE.md", ".claude/settings.json"],
        );
        assert_eq!(plan.files[3].action, Action::Edit);
        assert_eq!(plan.files[3].removes, ["respectGitignore"]);
        assert_eq!(plan.unmarked, ["apps/cli/CLAUDE.md"]);
        assert_eq!(plan.lessons.len(), 2, "the local twin repeats the same guards: {:?}", plan.lessons);
        assert!(plan.lessons.iter().all(|l| l.source == "apps/rt/CLAUDE.md"), "{:?}", plan.lessons);
        assert_eq!(plan.lessons[0].subproject.as_deref(), Some("apps/rt"));
    }

    /// As regras de bloqueio ficam no `settings.json` da equipe, mesmo iguais
    /// às do molde, e as outras linhas do molde saem. O arquivo só é apagado
    /// quando não sobra nada além do molde e não há regra de bloqueio nele.
    #[test]
    fn the_deny_rules_stay_in_the_team_settings() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let team = "{\n  \"respectGitignore\": true,\n  \"cleanupPeriodDays\": 30,\n  \"env\": { \"MUSTARD_BOUNDARY_MODE\": \"warn\" },\n  \"permissions\": {\n    \"allow\": [\"Read\", \"Grep\"],\n    \"deny\": [\"Bash(rm -rf:*)\", \"Read(**/*.pem)\"]\n  }\n}\n";
        write(root, ".claude/settings.json", team);

        let listed = plan(root);
        assert_eq!(listed.files.len(), 1, "{listed:?}");
        let change = &listed.files[0];
        assert_eq!(change.action, Action::Edit, "a file with deny rules is not deleted");
        assert_eq!(
            change.removes,
            ["respectGitignore", "cleanupPeriodDays", "env.MUSTARD_BOUNDARY_MODE", "permissions.allow: Read", "permissions.allow: Grep"],
        );
        let done = apply(root, &listed).unwrap();
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(done.edited, [".claude/settings.json"]);
        let left: Value = serde_json::from_str(&std_fs::read_to_string(root.join(".claude/settings.json")).unwrap()).unwrap();
        assert_eq!(left, serde_json::json!({ "permissions": { "deny": ["Bash(rm -rf:*)", "Read(**/*.pem)"] } }));
        assert!(plan(root).is_empty(), "the deny rules are not listed again");

        let other = tempdir().unwrap();
        let root = other.path();
        write(root, ".claude/settings.json", "{\n  \"respectGitignore\": true,\n  \"permissions\": { \"allow\": [\"Read\"] }\n}\n");
        let listed = plan(root);
        assert_eq!(listed.files[0].action, Action::Delete, "only the seed and no deny rule: {listed:?}");
    }

    /// Aplicar tira o que o plano listou, grava as Guards como lições, e uma
    /// segunda passada não acha mais nada: as lições não se repetem.
    #[test]
    fn applying_empties_the_plan_and_the_lessons_are_written_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/rt/CLAUDE.md", SUBPROJECT_MD);
        let first = plan(root);
        let done = apply(root, &first).unwrap();
        assert_eq!(done.deleted, ["apps/rt/CLAUDE.md"]);
        assert_eq!(done.lessons.len(), 2, "{done:?}");
        assert!(done.failed.is_empty(), "{done:?}");
        assert!(plan(root).is_empty());

        write(root, "apps/rt/CLAUDE.local.md", SUBPROJECT_MD);
        let again = plan(root);
        assert!(again.lessons.is_empty(), "the guards are already lessons: {:?}", again.lessons);
        assert_eq!(again.files.len(), 1);

        let bank = lessons::read(&lesson_bank(root).unwrap()).unwrap().unwrap();
        let written = bank.visible();
        assert_eq!(written.len(), 2);
        assert_eq!(written[0].event_type, PROJECT_RULE);
        assert_eq!(written[0].str_field("text"), Some("Never panic in a hook."));
    }
}
