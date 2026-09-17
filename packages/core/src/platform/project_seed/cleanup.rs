//! The cleanup: what Mustard once wrote into files that are not its own, taken
//! out only after the person says yes.
//!
//! Three kinds of file carry it:
//!
//! - the instruction files (`CLAUDE.md` and `CLAUDE.local.md`), where an older
//!   scan wrote the `@.claude/scan-map.md` import, the `> Parent: … |
//!   Orchestrator: …` line and the `## Guards` block between
//!   `<!-- mustard:guards -->` and `<!-- /mustard:guards -->`;
//! - the team's `.claude/settings.json`, where an older install wrote the lines
//!   of its seed;
//! - `.claude/CLAUDE.md`, the orchestrator an older install planted.
//!
//! [`plan`] only reads, and says what would leave; [`apply`] does it, and is
//! called only after the person confirmed that very list. Nothing is staged or
//! committed: the commit is the person's.
//!
//! Only what sits between the marks, the import line and the breadcrumb line
//! leaves an instruction file; every other byte stays, line endings included.
//! A file is deleted only when nothing is left but the title Mustard wrote
//! (and the `## Guards` heading that held the block). A file with the
//! breadcrumbs of an older scan and no block mark is not touched: it is listed,
//! and the person decides. Each guard that leaves becomes a project-rule lesson,
//! once.

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

/// Openings of the blocks Mustard wrote, with the closing of each. The guards
/// block opened with or without the `pending` word.
const BLOCKS: &[(&str, &str)] = &[
    ("<!-- mustard:guards", "<!-- /mustard:guards -->"),
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

/// What the cleanup would do, before anyone said yes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub files: Vec<FileChange>,
    /// Instruction files with the traces of an older scan and no block mark:
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

    /// Whether there is anything a yes would change.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.files.is_empty() || !self.lessons.is_empty()
    }

    /// The code of this very list: a yes given to one list never applies
    /// another. Changes with any file, any line and any lesson.
    #[must_use]
    pub fn token(&self) -> String {
        let seed = serde_json::to_string(self).unwrap_or_default();
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in seed.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{:08x}", hash & 0xffff_ffff)
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

/// The guards not yet in the bank, each once: the versioned instruction file
/// wins over its local twin, which usually repeats it.
fn new_lessons(root: &Path, mut guards: Vec<GuardLesson>) -> Vec<GuardLesson> {
    guards.sort_by_key(|g| g.source.ends_with(CLAUDE_LOCAL_MD));
    let known: Vec<String> = lesson_bank(root)
        .and_then(|path| lessons::read(&path).ok().flatten())
        .map(|bank| {
            bank.visible()
                .into_iter()
                .filter(|lesson| lesson.event_type == PROJECT_RULE)
                .filter_map(|lesson| lesson.str_field("text").map(normalized))
                .collect()
        })
        .unwrap_or_default();
    let mut seen = known;
    let mut out = Vec::new();
    for guard in guards {
        let key = normalized(&guard.text);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(guard);
    }
    out
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
    /// Traces of an older scan and no block mark, or a block that never
    /// closes: the person decides.
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
    let mut blocks = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = content(lines[i]);
        let trimmed = line.trim();
        if trimmed == SCAN_MAP_IMPORT_LINE {
            breadcrumbs = true;
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
            guards.extend(guards_in(&lines[i + 1..end].iter().map(|l| content(l)).collect::<Vec<_>>()));
            removes.push(format!("{open} … {close}", open = open.trim_end()));
            blocks += 1;
            i = end + 1;
            continue;
        }
        keep.push_str(lines[i]);
        i += 1;
    }

    if blocks == 0 {
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

/// The guards inside a block: each `- ` item, with the lines that continue it,
/// and without the comments the block carried for the scan.
fn guards_in(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            out.push(item.trim().to_string());
        } else if let Some(last) = out.last_mut() {
            last.push(' ');
            last.push_str(trimmed);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Applying the plan
// ---------------------------------------------------------------------------

/// Do what [`plan`] listed. Call only after the person confirmed that list
/// (see [`CleanupPlan::token`]). Every file is handled on its own: a failure is
/// reported and the rest goes on.
///
/// # Errors
///
/// None today; the signature keeps room for a failure that stops everything.
pub fn apply(root: &Path, plan: &CleanupPlan) -> Result<CleanupDone> {
    let mut done = CleanupDone::default();
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
    if let Some(bank) = lesson_bank(root) {
        for guard in &plan.lessons {
            match lessons::write(&bank, lesson_draft(guard), None) {
                Ok(written) => done.lessons.push(written.id),
                Err(refusal) => done.failed.push(format!("{}: {refusal:?}", guard.source)),
            }
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
    let applies = match &guard.subproject {
        Some(sub) => serde_json::json!({ "subproject": sub }),
        None => serde_json::json!({ "files": [crate::domain::lessons::WHOLE_PROJECT] }),
    };
    let draft = serde_json::json!({
        "class": PROJECT_RULE,
        "author": "binary",
        "text": guard.text,
        "keys": lesson_keys(guard),
        "applies_to": applies,
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

    /// Um arquivo com os rastros de um scan antigo e sem a marca do bloco só é
    /// listado; um bloco que não fecha também; e um arquivo sem nada do
    /// Mustard fica de fora.
    #[test]
    fn unmarked_files_are_only_listed() {
        assert_eq!(strip_marks("@.claude/scan-map.md\n# Cli\n\n## Guards\n\n- ours\n"), Stripped::Unmarked);
        assert_eq!(strip_marks("# X\n<!-- mustard:guards -->\n- never closed\n"), Stripped::Unmarked);
        assert_eq!(strip_marks("# Team notes\n\nNothing else.\n"), Stripped::Untouched);
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
        write(root, "apps/cli/CLAUDE.md", "@.claude/scan-map.md\n# Cli\n");
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
        assert_ne!(plan.token(), CleanupPlan::default().token());
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
