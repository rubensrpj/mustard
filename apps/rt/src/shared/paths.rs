//! `paths` — the one reader of a DECLARED file path, and the one classifier of
//! the file a hook is about to touch.
//!
//! A declared injectable path is written by hand as often as it is seeded: in
//! `mustard.json#inject`, in the `--inject` flag of a hook registration, and in
//! the seed itself. One file therefore has several honest spellings — a `./`
//! prefix, backslashes on Windows, a trailing separator, mixed case on a
//! case-insensitive filesystem.
//!
//! Comparing the raw strings makes each of those a different file, and the
//! symptom is never an error: a sibling hook silently delivers nothing, or the
//! blocks that belong to the whole invocation are dropped because no sibling
//! recognised itself as the elected one. Both were found in review of the unit
//! that introduced sibling hooks, in two of the three places that needed the
//! comparison — which is why it lives here now instead of being written a
//! fourth time.
//!
//! ## The file a hook is about to touch
//!
//! [`WriteTarget::classify`] is the write gate's one classifier: the tool
//! (read or write), the path relative to the root and the class of the file
//! ([`PathClass`]). [`relative_to_cwd`] is the one "path relative to the root"
//! computation of the write hooks.

use std::path::Path;

use mustard_core::domain::model::contract::HookInput;
use mustard_core::io::claude_paths::{LESSONS_FILE, SPEC_INDEX_FILE};
use mustard_core::io::spec_events::spec_root;

/// `true` when two declared paths name the SAME file.
///
/// Normalisation is deliberately conservative: separators, one leading `./`,
/// trailing separators, and ASCII case. It never resolves symlinks and never
/// touches the filesystem — callers compare paths that may not exist yet
/// (install time), and a filesystem probe would make the answer depend on
/// state the caller cannot see.
#[must_use]
pub fn same_declared_file(a: &str, b: &str) -> bool {
    // Delegated, never re-implemented. `mustard-core` seeds and migrates the
    // same declarations this crate reads, so a second normalisation here would
    // be a second answer to one question — and review already found three
    // copies of it, two of them subtly different at the call site.
    mustard_core::platform::project_seed::same_declared_path(a, b)
}

/// The files of a spec folder only the binary writes. The spec root's
/// `meta.json` counts too: it is what says whether an old draft, with no event
/// file, still locks, and no step says to edit it by hand.
const SPEC_FILES: &[&str] = &["spec.ndjson", "spec.md", "spec.html", "meta.json"];

/// The files of `.claude/spec/` outside a spec folder that only the binary
/// writes: the spec index and the lessons bank.
const BANK_FILES: &[&str] = &[SPEC_INDEX_FILE, LESSONS_FILE];

/// Harness state written before the unit exists: the plan-mode plans, the
/// disposable evidence a diagnosis runs and the harness cache, where the
/// material of `/feature` and `/bugfix` waits for `spec-draft`. None of them is
/// code, and the seeded `.gitignore` ignores all three: the base lock protects
/// the project's code, not the Mustard's own files git does not see.
const HARNESS_PREFIXES: &[&str] = &[".claude/plans/", ".claude/scratch/", ".claude/.cache/"];

/// Artefatos e infraestrutura, nunca código do projeto.
const ARTIFACT_PREFIXES: &[&str] = &[".claude/", "dist/", "node_modules/", ".git/", "target/"];

/// The path of `file_path` relative to `cwd`, with forward slashes. A relative
/// path is read from `cwd`. `None` when the file lies outside `cwd`;
/// `Some("")` for the root itself.
#[must_use]
pub(crate) fn relative_to_cwd(cwd: &str, file_path: &str) -> Option<String> {
    let cwd_norm = cwd.replace('\\', "/");
    let fp_norm = file_path.replace('\\', "/");
    let abs = if is_absolute(&fp_norm) {
        fp_norm
    } else {
        format!("{}/{}", cwd_norm.trim_end_matches('/'), fp_norm)
    };
    // `.` and `..` go away before the comparison: `.claude/../src/x.rs` is
    // project code, not a `.claude/` artefact.
    let (abs, cwd) = (lexical(&abs), lexical(&cwd_norm));
    if abs == cwd {
        return Some(String::new());
    }
    let prefix = if cwd.is_empty() || cwd.ends_with('/') { cwd } else { format!("{cwd}/") };
    abs.strip_prefix(&prefix).map(str::to_string)
}

/// `path` with `.` and `..` resolved in the text only, without looking at the
/// disk. A `..` that would go past the root stops at it.
fn lexical(path: &str) -> String {
    let (root, rest) = if let Some(rest) = path.strip_prefix('/') {
        ("/", rest)
    } else if is_absolute(path) {
        (path.get(..3).unwrap_or_default(), path.get(3..).unwrap_or_default())
    } else {
        ("", path)
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("{root}{}", parts.join("/"))
}

/// `given` joined to the root when it is relative, with forward slashes and
/// with `.` and `..` resolved in the text only.
fn resolved(root: &str, given: &str) -> String {
    let given = given.replace('\\', "/");
    if is_absolute(&given) {
        lexical(&given)
    } else {
        lexical(&format!("{}/{given}", root.replace('\\', "/").trim_end_matches('/')))
    }
}

/// `true` when a path with forward slashes is absolute: `/...` or `C:/...`.
fn is_absolute(p: &str) -> bool {
    p.starts_with('/')
        || (p.len() >= 3
            && p.as_bytes()[0].is_ascii_alphabetic()
            && p.as_bytes()[1] == b':'
            && p.as_bytes()[2] == b'/')
}

/// The sensitive-file pattern `path` matches: credentials, keys and the git
/// configuration. Case-insensitive and over the whole path, so a folder with
/// the name matches too (`config/credentials/prod.yaml`) — which the
/// `permissions.deny` rules cannot say.
#[must_use]
pub(crate) fn sensitive_pattern(path: &str) -> Option<&'static str> {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    if lower.contains("credentials") {
        return Some("credentials");
    }
    for (extension, pattern) in [(".pem", "*.pem"), (".key", "*.key"), (".pfx", "*.pfx"), (".p12", "*.p12")] {
        if lower.ends_with(extension) {
            return Some(pattern);
        }
    }
    if lower.ends_with(".git/config") {
        return Some(".git/config");
    }
    ["id_rsa", "id_ed25519"].into_iter().find(|name| lower.contains(name))
}

/// Como a ferramenta toca o arquivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// `Read`.
    Read,
    /// `Write`, `Edit`, `MultiEdit` ou `NotebookEdit`.
    Write,
}

impl Access {
    /// How `tool` touches the file; `None` for a tool that is not a file
    /// tool.
    #[must_use]
    pub(crate) fn of_tool(tool: &str) -> Option<Self> {
        match tool {
            "Read" => Some(Self::Read),
            "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => Some(Self::Write),
            _ => None,
        }
    }
}

/// What the file is, for the write gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathClass {
    /// Matched a sensitive-file pattern ([`sensitive_pattern`]), inside or
    /// outside the project.
    Secret {
        /// The pattern that matched.
        pattern: &'static str,
    },
    /// A file only the binary writes: a spec's `spec.ndjson`, `spec.md` or
    /// `spec.html`, the spec index or the lessons bank. In a worktree, the main
    /// checkout's ones too.
    SpecFile {
        /// The spec that owns the file, when it lives in its folder.
        spec: Option<String>,
    },
    /// Harness state written before the unit exists: `.claude/plans/`,
    /// `.claude/scratch/` and `.claude/.cache/`.
    Harness,
    /// Artefato ou infraestrutura: o resto de `.claude/`, `dist/`,
    /// `node_modules/`, `.git/` e `target/`.
    Artifact,
    /// Fora da raiz do projeto.
    OutsideRepo,
    /// Código do projeto.
    Production,
}

/// The file a file tool is about to touch, already classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WriteTarget {
    /// Leitura ou escrita.
    pub(crate) access: Access,
    /// The path relative to the root when the file lives in it; otherwise,
    /// the path as it came, with forward slashes.
    pub(crate) path: String,
    /// What the file is.
    pub(crate) class: PathClass,
}

impl WriteTarget {
    /// Classifies the file `input` is about to touch, seen from the root
    /// `root`. `None` when the tool is not a file tool or carries no path.
    #[must_use]
    pub(crate) fn classify(root: &str, input: &HookInput) -> Option<Self> {
        let access = Access::of_tool(input.tool_name.as_deref()?)?;
        let given = input.file_path()?.replace('\\', "/");
        let rel = relative_to_cwd(root, &given).map(|rel| rel.trim_start_matches("./").to_string());
        let class = classify_path(root, &given, rel.as_deref());
        Some(Self { access, path: rel.unwrap_or(given), class })
    }
}

/// The class of `given`, which is `rel` when it lives in the root.
fn classify_path(root: &str, given: &str, rel: Option<&str>) -> PathClass {
    // The two comparisons that look at the whole path get it already joined
    // to the root and without `.`, `..` or a doubled slash: `/p/.git/./config`
    // and `/p/.git//config` are the git configuration, and the main checkout's
    // specs folder is still found when the path carries a `./`.
    let full = resolved(root, given);
    // The whole path contains the file name, so a name pattern matches it
    // too.
    if let Some(pattern) = sensitive_pattern(&full) {
        return PathClass::Secret { pattern };
    }
    let Some(rel) = rel else {
        return main_checkout_rel(root, &full)
            .and_then(|rel| spec_file(&rel))
            .unwrap_or(PathClass::OutsideRepo);
    };
    if let Some(class) = spec_file(rel) {
        return class;
    }
    if HARNESS_PREFIXES.iter().any(|prefix| rel.starts_with(prefix)) {
        return PathClass::Harness;
    }
    if rel.is_empty() || ARTIFACT_PREFIXES.iter().any(|prefix| rel.starts_with(prefix)) {
        return PathClass::Artifact;
    }
    PathClass::Production
}

/// The path of `given` relative to the main checkout, when the root is a
/// worktree and `given` points at the specs folder there: in a worktree, the
/// specs live in the main checkout. Only asks git for an absolute path that
/// goes through `.claude/spec/`.
fn main_checkout_rel(root: &str, given: &str) -> Option<String> {
    if !is_absolute(given) || !given.contains("/.claude/spec/") {
        return None;
    }
    let main = spec_root(Path::new(root));
    relative_to_cwd(&main.to_string_lossy(), given)
}

/// The class of a file of the specs folder only the binary writes.
fn spec_file(rel: &str) -> Option<PathClass> {
    let rest = rel.strip_prefix(".claude/spec/")?;
    match rest.split('/').collect::<Vec<_>>().as_slice() {
        [file] if BANK_FILES.contains(file) => Some(PathClass::SpecFile { spec: None }),
        [spec, file] if !spec.is_empty() && SPEC_FILES.contains(file) => {
            Some(PathClass::SpecFile { spec: Some((*spec).to_string()) })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn equivalent_spellings_name_one_file() {
        let canonical = ".claude/mustard/orchestrator.md";
        for spelling in [
            ".claude/mustard/orchestrator.md",
            "./.claude/mustard/orchestrator.md",
            ".claude\\mustard\\orchestrator.md",
            ".claude/Mustard/Orchestrator.md",
            "  .claude/mustard/orchestrator.md  ",
        ] {
            assert!(same_declared_file(spelling, canonical), "`{spelling}` should match");
        }
    }

    #[test]
    fn different_files_stay_different() {
        assert!(!same_declared_file(
            ".claude/mustard/orchestrator.md",
            ".claude/mustard/dispatch.md",
        ));
        // A prefix is not a match: `dispatch.md` and `dispatch.md.bak` are two
        // files, and treating them as one would elect the wrong sibling.
        assert!(!same_declared_file(
            ".claude/mustard/dispatch.md",
            ".claude/mustard/dispatch.md.bak",
        ));
    }

    fn input(tool: &str, file_path: &str) -> HookInput {
        let field = if tool == "NotebookEdit" { "notebook_path" } else { "file_path" };
        HookInput {
            tool_name: Some(tool.to_string()),
            tool_input: json!({ field: file_path }),
            ..HookInput::default()
        }
    }

    /// The root works for an absolute and a relative path, with Windows
    /// backslashes too; outside the root there is no relative path.
    #[test]
    fn a_path_is_made_relative_to_the_root_once() {
        assert_eq!(relative_to_cwd("/p", "/p/src/a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(relative_to_cwd("/p/", "src/a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(relative_to_cwd("C:\\p", "C:\\p\\src\\a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(relative_to_cwd("/p", "/p").as_deref(), Some(""));
        assert_eq!(relative_to_cwd("/p", "/outra/a.rs"), None);
        assert_eq!(relative_to_cwd("/p", "/pp/a.rs"), None, "a sibling folder is outside");
        // `.` and `..` resolved in the text only: the path that loops through
        // `.claude/` lands where it points, and one that leaves the root stays
        // out.
        assert_eq!(relative_to_cwd("/p", "/p/.claude/../src/x.rs").as_deref(), Some("src/x.rs"));
        assert_eq!(
            relative_to_cwd("/p", ".claude/spec/x/../x/./spec.md").as_deref(),
            Some(".claude/spec/x/spec.md"),
        );
        assert_eq!(relative_to_cwd("/p", "/p/../outra/a.rs"), None);
    }

    /// A `.` or a doubled slash in the middle of the path does not hide the
    /// git configuration.
    #[test]
    fn a_dot_or_a_double_slash_does_not_hide_a_sensitive_file() {
        for path in ["/p/.git/./config", "/p/.git//config", "/p/src/../.git/config", ".git/./config"] {
            for tool in ["Read", "Write"] {
                let target = WriteTarget::classify("/p", &input(tool, path)).expect("a file tool");
                assert_eq!(target.class, PathClass::Secret { pattern: ".git/config" }, "{tool} {path}");
            }
        }
    }

    /// Seen from a worktree, the main checkout's spec event file is still a
    /// file only the binary writes, even with a `./` in the middle of the
    /// path.
    #[test]
    fn from_a_worktree_a_dot_in_the_main_spec_path_still_names_the_spec_file() {
        let git = |dir: &Path, args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed in {}", dir.display());
        };
        let tmp = tempfile::tempdir().expect("tempdir");
        let main = tmp.path().join("principal");
        std::fs::create_dir_all(&main).expect("main");
        git(&main, &["init", "-q"]);
        git(&main, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "root"]);
        let wt = tmp.path().join("trabalho");
        git(&main, &["worktree", "add", "-q", &wt.to_string_lossy(), "-b", "feature/x"]);

        let main_str = main.to_string_lossy().replace('\\', "/");
        let wt_str = wt.to_string_lossy().replace('\\', "/");
        for spelling in [
            format!("{main_str}/.claude/./spec/x/spec.ndjson"),
            format!("{main_str}/.claude//spec/x/spec.ndjson"),
            format!("{main_str}/.claude/spec/x/spec.ndjson"),
        ] {
            let target = WriteTarget::classify(&wt_str, &input("Edit", &spelling)).expect("a file tool");
            assert_eq!(target.class, PathClass::SpecFile { spec: Some("x".to_string()) }, "{spelling}");
        }
    }

    /// Cada classe, pelas cinco ferramentas de arquivo; outra ferramenta não
    /// é classificada.
    #[test]
    fn every_file_tool_gets_one_class_for_its_path() {
        let spec = |name: &str| PathClass::SpecFile { spec: Some(name.to_string()) };
        let cases = [
            ("/p/.aws/credentials", PathClass::Secret { pattern: "credentials" }),
            ("certs/KEY.PEM", PathClass::Secret { pattern: "*.pem" }),
            ("/p/.git/config", PathClass::Secret { pattern: ".git/config" }),
            ("backup/ID_RSA.bak", PathClass::Secret { pattern: "id_rsa" }),
            ("/p/.claude/spec/x/spec.ndjson", spec("x")),
            ("/p/.claude/spec/x/spec.md", spec("x")),
            (".claude/spec/x/spec.html", spec("x")),
            ("/p/.claude/spec/index.ndjson", PathClass::SpecFile { spec: None }),
            ("/p/.claude/spec/lessons.ndjson", PathClass::SpecFile { spec: None }),
            ("/p/.claude/spec/x/meta.json", spec("x")),
            ("/p/.claude/spec/x/wave-1/meta.json", PathClass::Artifact),
            ("/p/.claude/plans/plano.md", PathClass::Harness),
            ("/p/.claude/scratch/probe.sh", PathClass::Harness),
            ("/p/.claude/.cache/spec-material.json", PathClass::Harness),
            ("/p/.claude/../src/x.rs", PathClass::Production),
            ("/p/.claude/spec/x/../x/spec.md", spec("x")),
            ("/p/.claude/settings.json", PathClass::Artifact),
            ("/p/target/debug/x", PathClass::Artifact),
            ("/p/src/scratch_notes.rs", PathClass::Production),
            ("./src/main.rs", PathClass::Production),
            ("/outra/memo.md", PathClass::OutsideRepo),
        ];
        for tool in ["Read", "Write", "Edit", "MultiEdit", "NotebookEdit"] {
            for (path, class) in &cases {
                let target = WriteTarget::classify("/p", &input(tool, path)).expect("a file tool");
                assert_eq!(&target.class, class, "{tool} {path}");
                let access = if tool == "Read" { Access::Read } else { Access::Write };
                assert_eq!(target.access, access, "{tool}");
            }
        }
        let target = WriteTarget::classify("/p", &input("Edit", "/p/src/main.rs")).unwrap();
        assert_eq!(target.path, "src/main.rs", "the path is relative to the root");
        for other in ["Bash", "Task", "Agent", "Glob"] {
            assert_eq!(WriteTarget::classify("/p", &input(other, "/p/src/a.rs")), None, "{other}");
        }
    }
}
