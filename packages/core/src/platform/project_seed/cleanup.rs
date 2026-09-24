//! The cleanup: what Mustard once wrote into files that are not its own. It is
//! an older Mustard's own leftover, so the upsert takes it out with no
//! question and says what left.
//!
//! Four kinds of file carry it:
//!
//! - the instruction files (`CLAUDE.md` and `CLAUDE.local.md`), where an older
//!   scan wrote the `@.claude/scan-map.md` import, the `> Parent: … |
//!   Orchestrator: …` line and the `## Guards` block between
//!   `<!-- mustard:guards -->` and `<!-- /mustard:guards -->`;
//! - the team's `.claude/settings.json`, where an older install wrote the lines
//!   of its seed (its deny rules stay: a protection rule never leaves the
//!   team's file unless someone asks);
//! - `.claude/CLAUDE.md`, the orchestrator an older install planted;
//! - a spec's own `spec.md` and `spec.html`, left behind by an older binary
//!   that rendered the page to disk beside `spec.ndjson`, which is the ONLY
//!   file of a spec that lives in the project ([`stale_spec_pages`]). A
//!   folder with no `spec.ndjson` is the older format instead, whose
//!   `spec.md` IS the document, and is never touched.
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
//! is listed, and the person decides.
//!
//! The rules of the guards block that leave go to the project's pending list
//! ([`PendingList`]), in one item per cleanup, with the text of each rule and
//! the files it left, before any file changes: when that item cannot be
//! written, no file changes. They go neither to the lesson bank, which stays
//! on one machine and never goes to git, nor back to the files, because
//! nothing of Mustard's goes to git: the person turns each rule into a test in
//! the code, one that fails if the rule is broken, or drops it. A text is
//! listed once, however many files carry it, compared as the lesson write
//! compares texts (`domain::lessons::comparable`). The old map block leaves
//! without becoming a rule.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::domain::config::ProjectConfig;
use crate::domain::lessons as model;
use crate::io::claude_paths::ClaudePaths;
use crate::io::fs::{self, PRUNE_DIRS};
use crate::io::spec_index::DISCARDED_DIR;
use crate::platform::error::Result;
use crate::platform::i18n::{translate, Locale};

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

/// One rule of a guards block that leaves the instruction files and goes to
/// the pending list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeavingRule {
    /// The rule, as it was written.
    pub text: String,
    /// Every file it leaves, project-root-relative: the versioned instruction
    /// file before its local twin.
    pub sources: Vec<String>,
}

/// The project's pending list, where the rules that leave wait for the person
/// to turn each into a test or drop it. The core does not know its format: the
/// caller of the cleanup (`run upsert`) hands its door in.
pub trait PendingList {
    /// Write one open item with `title` and `detail` and answer its number.
    ///
    /// # Errors
    ///
    /// Why nothing was written.
    fn add(&self, title: &str, detail: &str) -> std::result::Result<String, String>;
}

/// No pending list: the write always fails, so no rule leaves its file.
pub struct NoPendingList;

impl PendingList for NoPendingList {
    fn add(&self, _title: &str, _detail: &str) -> std::result::Result<String, String> {
        Err("this install has no pending list, so no rule leaves its file".to_string())
    }
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
    /// The rules of the guards blocks that would leave, each text once: they
    /// go to the pending list.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<LeavingRule>,
}

impl CleanupPlan {
    /// Nothing to take out and nothing to list.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.unmarked.is_empty() && self.rules.is_empty()
    }

    /// Whether taking the list out would change anything.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.files.is_empty() || !self.rules.is_empty()
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
    /// The number of the pending item that holds the rules that left.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<String>,
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
                guards.extend(found.into_iter().map(|text| (text, rel.clone())));
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

    out.files.extend(stale_spec_pages(root));

    out.rules = leaving_rules(guards);
    out
}

/// `spec.md`/`spec.html` left inside a spec's OWN folder, back when the
/// binary rendered the page to disk instead of publishing it — see
/// `SpecPaths::spec_md_path` and `spec_html_path`, whose own doc calls both
/// "projections" of `spec.ndjson`, never a second copy on disk. A folder
/// counts only with its `spec.ndjson` present: that is what tells this
/// leftover apart from the OLDER format, whose `spec.md` IS the document and
/// has no `spec.ndjson` beside it — that one is never touched here.
fn stale_spec_pages(root: &Path) -> Vec<FileChange> {
    let Ok(paths) = ClaudePaths::for_project(root) else { return Vec::new() };
    let spec_dir = paths.spec_dir();
    let Ok(entries) = fs::read_dir(&spec_dir) else { return Vec::new() };
    let mut names: Vec<String> =
        entries.into_iter().filter(|e| e.is_dir && e.file_name != DISCARDED_DIR).map(|e| e.file_name).collect();
    names.sort();

    let mut out = Vec::new();
    for name in names {
        let Ok(spec) = paths.for_spec(&name) else { continue };
        if !spec.spec_ndjson_path().is_file() {
            continue;
        }
        for (path, name_in_dir) in [(spec.spec_md_path(), "spec.md"), (spec.spec_html_path(), "spec.html")] {
            if path.is_file() {
                out.push(FileChange {
                    path: format!(".claude/spec/{name}/{name_in_dir}"),
                    action: Action::Delete,
                    removes: vec!["an older Mustard rendered this to disk beside spec.ndjson".to_string()],
                });
            }
        }
    }
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

/// The rules that leave, each text once, with every file it leaves. The
/// versioned instruction file comes before its local twin, which usually
/// repeats it. Texts are compared as the lesson write compares them (spaces,
/// case and accents aside), so the same rule in two files is listed once.
fn leaving_rules(mut found: Vec<(String, String)>) -> Vec<LeavingRule> {
    found.sort_by_key(|(_, source)| source.ends_with(CLAUDE_LOCAL_MD));
    let mut out: Vec<LeavingRule> = Vec::new();
    for (text, source) in found {
        let key = model::comparable(&text);
        match out.iter_mut().find(|taken| model::comparable(&taken.text) == key) {
            Some(taken) => {
                if !taken.sources.contains(&source) {
                    taken.sources.push(source);
                }
            }
            None => out.push(LeavingRule { text, sources: vec![source] }),
        }
    }
    out
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
/// The rules that leave go first to the pending list (`pending`), in one item
/// with the text of each rule and the files it left. When that item cannot be
/// written, no file is touched, so no rule leaves a file without being
/// written down; the next run lists them again. After that every file is
/// handled on its own: a failure is reported and the rest goes on.
///
/// # Errors
///
/// None today; the signature keeps room for a failure that stops everything.
pub fn apply(root: &Path, plan: &CleanupPlan, pending: &dyn PendingList) -> Result<CleanupDone> {
    let mut done = CleanupDone::default();
    if !plan.rules.is_empty() {
        let lang = ProjectConfig::load(root).language().text_or_default();
        let (title, detail) = pending_item(&plan.rules, lang);
        match pending.add(&title, &detail) {
            Ok(id) => done.pending = Some(id),
            Err(why) => {
                done.failed.push(format!("pending: {why}"));
                return Ok(done);
            }
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

/// The one pending item of a cleanup, in the project's language `lang`: the
/// title says how many rules left, and the detail says what to do with them
/// and lists each rule's text with the files it left.
fn pending_item(rules: &[LeavingRule], lang: Locale) -> (String, String) {
    let title = translate("lessons.rules_left.title", lang).replace("{count}", &rules.len().to_string());
    let listed: Vec<String> = rules
        .iter()
        .enumerate()
        .map(|(at, rule)| {
            // O texto da regra entra por último: uma vaga escrita dentro dele
            // não é preenchida.
            translate("lessons.rules_left.rule", lang)
                .replace("{n}", &(at + 1).to_string())
                .replace("{sources}", &rule.sources.join(", "))
                .replace("{text}", &rule.text)
        })
        .collect();
    let detail = translate("lessons.rules_left.detail", lang).replace("{rules}", &listed.join("; "));
    (title, detail)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::fs as std_fs;
    use tempfile::tempdir;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std_fs::create_dir_all(path.parent().unwrap()).unwrap();
        std_fs::write(path, body).unwrap();
    }

    /// A lista de pendências dos testes: guarda na memória o título e o
    /// detalhe de cada item gravado e responde o número seguinte; quebrada,
    /// recusa sem gravar.
    #[derive(Default)]
    struct Memory {
        items: RefCell<Vec<(String, String)>>,
        broken: bool,
    }

    impl PendingList for Memory {
        fn add(&self, title: &str, detail: &str) -> std::result::Result<String, String> {
            if self.broken {
                return Err("the pending list cannot be written".to_string());
            }
            let mut items = self.items.borrow_mut();
            items.push((title.to_string(), detail.to_string()));
            Ok(format!("P-{}", items.len()))
        }
    }

    /// O banco de lições do projeto em `root`.
    fn bank(root: &Path) -> std::path::PathBuf {
        ClaudePaths::for_project(root).unwrap().lessons_path()
    }

    const SUBPROJECT_MD: &str = "@.claude/scan-map.md\n\n# Rt\n\n> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/mustard/orchestrator.md](../../.claude/mustard/orchestrator.md)\n\n## Guards\n\n<!-- mustard:guards -->\n<!-- facts: kind=cargo -->\n- Never panic in a hook.\n- Keep `main.rs` thin:\n  routing only.\n<!-- /mustard:guards -->\n";

    /// Um arquivo que só tem o que o Mustard escreveu sai inteiro, e as duas
    /// regras do bloco dele são lidas, com a linha que continua a segunda.
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
        let list = Memory::default();
        let done = apply(root, &listed, &list).unwrap();
        assert_eq!(done.deleted, ["apps/web/CLAUDE.md"]);
        assert!(list.items.borrow().is_empty() && done.pending.is_none(), "no rule left, so no pending item: {done:?}");
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

    /// As regras que o scan escreveu em prosa, sem `- `, vão à lista de
    /// pendências antes de o arquivo sair: a cópia do `CLAUDE.md` da fixture
    /// do scan em Go, numa pasta temporária, dá um item só, com as cinco
    /// regras, cada uma com o arquivo de onde saiu. O arquivo é apagado, e o
    /// banco de lições nem nasce.
    #[test]
    fn prose_guards_of_the_scan_fixtures_go_to_the_pending_list_before_the_file_goes() {
        let body = SCAN_FIXTURE_MD;
        let expected = block_lines(body);
        assert_eq!(expected.len(), 5, "the fixture keeps its five guards: {expected:?}");
        assert_eq!(expected[0], "[critical] never import in internal/model/user.go");

        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/graph_go/CLAUDE.md", body);
        let listed = plan(root);
        let texts: Vec<&str> = listed.rules.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, expected, "one rule per guard line");
        assert!(listed.rules.iter().all(|r| r.sources == ["apps/graph_go/CLAUDE.md"]), "{:?}", listed.rules);
        assert_eq!(listed.files[0].action, Action::Delete);

        let list = Memory::default();
        let done = apply(root, &listed, &list).unwrap();
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(done.pending.as_deref(), Some("P-1"));
        let items = list.items.borrow();
        assert_eq!(items.len(), 1, "one item for the whole cleanup: {items:?}");
        let (title, detail) = &items[0];
        assert!(title.contains("(5)"), "{title}");
        for (at, rule) in expected.iter().enumerate() {
            let line = format!("{}) {rule} (saiu de apps/graph_go/CLAUDE.md)", at + 1);
            assert!(detail.contains(&line), "the item keeps `{line}`: {detail}");
        }
        assert!(!root.join("apps/graph_go/CLAUDE.md").exists());
        assert!(!bank(root).exists(), "nothing goes to the lesson bank");
    }

    /// A mesma regra em dois subprojetos, com outros espaços, maiúsculas e
    /// acentos, é listada uma vez só, com os arquivos de onde saiu, o
    /// versionado antes do gêmeo local: os três arquivos saem.
    #[test]
    fn the_same_guard_in_two_subprojects_is_listed_once_with_every_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/a/CLAUDE.md", "# A\n<!-- mustard:guards -->\n- Não chame o banco de dentro do controlador.\n<!-- /mustard:guards -->\n");
        write(root, "apps/a/CLAUDE.local.md", "# A\n<!-- mustard:guards -->\n- Não chame o banco de dentro do controlador.\n<!-- /mustard:guards -->\n");
        write(root, "apps/b/CLAUDE.md", "# B\n<!-- mustard:guards -->\n- NAO chame o banco  de dentro do controlador.\n<!-- /mustard:guards -->\n");

        let listed = plan(root);
        assert_eq!(listed.rules.len(), 1, "{:?}", listed.rules);
        assert_eq!(listed.rules[0].text, "Não chame o banco de dentro do controlador.");
        assert_eq!(listed.rules[0].sources, ["apps/a/CLAUDE.md", "apps/b/CLAUDE.md", "apps/a/CLAUDE.local.md"]);

        let list = Memory::default();
        let done = apply(root, &listed, &list).unwrap();
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(done.deleted.len(), 3, "{done:?}");
        let detail = &list.items.borrow()[0].1;
        assert!(
            detail.contains("1) Não chame o banco de dentro do controlador. (saiu de apps/a/CLAUDE.md, apps/b/CLAUDE.md, apps/a/CLAUDE.local.md)"),
            "{detail}"
        );
        assert!(!detail.contains("2)"), "the rule is listed once: {detail}");
    }

    /// O item fala o idioma do projeto: em inglês, o título, o detalhe e cada
    /// regra saem do catálogo em inglês.
    #[test]
    fn the_pending_item_speaks_the_project_language() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "mustard.json", r#"{"language":{"text":"en-US"}}"#);
        write(root, "apps/rt/CLAUDE.md", SUBPROJECT_MD);
        let list = Memory::default();
        let done = apply(root, &plan(root), &list).unwrap();
        assert!(done.failed.is_empty(), "{done:?}");
        let (title, detail) = list.items.borrow()[0].clone();
        assert!(title.starts_with("Rules the install took out") && title.contains("(2)"), "{title}");
        assert!(detail.contains("1) Never panic in a hook. (taken from apps/rt/CLAUDE.md)"), "{detail}");
        assert!(detail.contains("2) Keep `main.rs` thin: routing only. (taken from apps/rt/CLAUDE.md)"), "{detail}");
    }

    /// O bloco antigo do mapa guardava o resumo da pasta, não regras: ele sai
    /// do arquivo sem virar regra nenhuma. Num arquivo com os dois blocos, só
    /// o das Guards vai à lista.
    #[test]
    fn the_old_map_block_leaves_without_becoming_a_rule() {
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
        let texts: Vec<&str> = listed.rules.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["Keep pages free of inline styles."], "{:?}", listed.rules);

        let list = Memory::default();
        let done = apply(root, &listed, &list).unwrap();
        assert!(done.failed.is_empty(), "{done:?}");
        assert_eq!(list.items.borrow().len(), 1);
        assert_eq!(done.edited, ["apps/dashboard/CLAUDE.md"]);
        assert_eq!(done.deleted, ["apps/web/CLAUDE.md"]);
        assert!(!std_fs::read_to_string(root.join("apps/dashboard/CLAUDE.md")).unwrap().contains("Tipo:"));
    }

    /// Com a lista de pendências impossível de gravar, nenhum arquivo muda:
    /// nada é apagado nem reescrito, a falha é dita e o banco de lições nem
    /// nasce. Sem lista nenhuma, é igual.
    #[test]
    fn a_pending_list_that_cannot_be_written_keeps_every_file() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/rt/CLAUDE.md", SUBPROJECT_MD);
        write(root, "apps/api/CLAUDE.md", "# Api\n\nOurs.\n<!-- mustard:guards -->\n- A guard.\n<!-- /mustard:guards -->\n");
        write(root, ".claude/settings.json", "{\n  \"respectGitignore\": true,\n  \"team\": 1\n}\n");
        let listed = plan(root);
        assert_eq!(listed.rules.len(), 3, "{listed:?}");

        for list in [&Memory { broken: true, ..Memory::default() } as &dyn PendingList, &NoPendingList] {
            let done = apply(root, &listed, list).unwrap();
            assert!(done.failed.iter().any(|f| f.starts_with("pending:")), "the failure is said: {done:?}");
            assert!(done.deleted.is_empty() && done.edited.is_empty() && done.pending.is_none(), "{done:?}");
            assert_eq!(std_fs::read_to_string(root.join("apps/rt/CLAUDE.md")).unwrap(), SUBPROJECT_MD);
            assert!(std_fs::read_to_string(root.join("apps/api/CLAUDE.md")).unwrap().contains("- A guard."));
            assert!(std_fs::read_to_string(root.join(".claude/settings.json")).unwrap().contains("respectGitignore"));
            assert!(!bank(root).exists(), "nothing goes to the lesson bank");
        }
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
        assert_eq!(plan.rules.len(), 2, "the local twin repeats the same guards: {:?}", plan.rules);
        assert!(
            plan.rules.iter().all(|r| r.sources == ["apps/rt/CLAUDE.md", "apps/rt/CLAUDE.local.md"]),
            "{:?}",
            plan.rules
        );
    }

    /// O plano tira `spec.md` e `spec.html` de dentro de uma pasta de spec que
    /// já tem `spec.ndjson` — resto de quando o binário desenhava a página no
    /// disco — e apagar não toca no arquivo de eventos. Uma pasta sem
    /// `spec.ndjson` é o formato antigo, cujo `spec.md` é o próprio
    /// documento, e fica intocada, provando que a regra depende do
    /// `spec.ndjson` estar presente, não só do nome da pasta.
    #[test]
    fn stale_spec_pages_leave_only_beside_an_ndjson() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, ".claude/spec/zerar-as-pendencias/spec.ndjson", "{}\n");
        write(root, ".claude/spec/zerar-as-pendencias/spec.md", "# old render\n");
        write(root, ".claude/spec/zerar-as-pendencias/spec.html", "<html></html>");
        write(root, ".claude/spec/an-old-format-unit/spec.md", "# the document itself\n");

        let listed = plan(root);
        let paths: Vec<&str> = listed.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [".claude/spec/zerar-as-pendencias/spec.md", ".claude/spec/zerar-as-pendencias/spec.html"],
            "{paths:?}",
        );
        assert!(listed.files.iter().all(|f| f.action == Action::Delete), "{listed:?}");

        let done = apply(root, &listed, &NoPendingList).unwrap();
        assert!(done.failed.is_empty(), "no rule leaves, so no list is needed: {done:?}");
        assert!(!root.join(".claude/spec/zerar-as-pendencias/spec.md").exists());
        assert!(!root.join(".claude/spec/zerar-as-pendencias/spec.html").exists());
        assert!(
            root.join(".claude/spec/zerar-as-pendencias/spec.ndjson").exists(),
            "the event file is the one that stays"
        );
        assert!(
            root.join(".claude/spec/an-old-format-unit/spec.md").exists(),
            "the older format's own document, with no spec.ndjson beside it, is untouched"
        );
        assert!(plan(root).is_empty(), "nothing left to clean");
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
        let done = apply(root, &listed, &NoPendingList).unwrap();
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

    /// Aplicar tira o que o plano listou e grava as regras num item só da
    /// lista; uma segunda passada não acha mais nada, e o banco de lições nem
    /// nasce.
    #[test]
    fn applying_empties_the_plan_and_writes_one_item() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/rt/CLAUDE.md", SUBPROJECT_MD);
        let list = Memory::default();
        let done = apply(root, &plan(root), &list).unwrap();
        assert_eq!(done.deleted, ["apps/rt/CLAUDE.md"]);
        assert_eq!(done.pending.as_deref(), Some("P-1"), "{done:?}");
        assert!(done.failed.is_empty(), "{done:?}");
        assert!(plan(root).is_empty());
        assert_eq!(list.items.borrow().len(), 1);
        assert!(!bank(root).exists(), "nothing goes to the lesson bank");
    }
}
