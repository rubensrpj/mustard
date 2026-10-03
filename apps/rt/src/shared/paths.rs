//! `paths` — o classificador do arquivo que um gancho vai tocar.
//!
//! [`WriteTarget::classify`] é o classificador único do portão de escrita: a
//! ferramenta (leitura ou escrita), o caminho relativo à raiz e a classe do
//! arquivo ([`PathClass`]). [`relative_to_cwd`] é a conta única do "caminho
//! relativo à raiz".

use std::path::Path;

use mustard_core::domain::model::contract::HookInput;
use mustard_core::io::claude_paths::{LESSONS_FILE, SPEC_INDEX_FILE};
use mustard_core::io::spec_events::spec_root;

/// The files of a spec folder only the binary writes. The spec root's
/// `meta.json` counts too: it is what says whether an old draft, with no event
/// file, still locks, and no step says to edit it by hand.
const SPEC_FILES: &[&str] = &["spec.ndjson", "spec.md", "spec.html", "meta.json"];

/// The files of `.claude/spec/` outside a spec folder that only the binary
/// writes: the spec index and the lessons bank.
const BANK_FILES: &[&str] = &[SPEC_INDEX_FILE, LESSONS_FILE];

/// Harness state written before the unit exists: the plan-mode plans and the
/// disposable evidence a diagnosis runs. Neither is code, and the seeded
/// `.gitignore` ignores both.
const HARNESS_PREFIXES: &[&str] = &[".claude/plans/", ".claude/scratch/"];

/// Artefacts and infrastructure, never project code.
const ARTIFACT_PREFIXES: &[&str] = &[".claude/", "dist/", "node_modules/", ".git/", "target/"];

/// `true` quando o caminho `rel`, relativo à raiz do projeto, mora numa pasta
/// de artefato: estado do harness, dependências ou saída de compilação.
pub(crate) fn is_artifact(rel: &str) -> bool {
    ARTIFACT_PREFIXES.iter().any(|prefix| rel.starts_with(prefix))
}

/// The path of `file_path` relative to `cwd`, with forward slashes. A relative
/// path is read from `cwd`. `None` when the file lies outside `cwd`;
/// `Some("")` for the root itself. Both sides go through [`canonical`] first,
/// so a verbatim prefix, backslashes or the case of a Windows drive do not
/// make the same place look like two.
#[must_use]
pub(crate) fn relative_to_cwd(cwd: &str, file_path: &str) -> Option<String> {
    let cwd = canonical(cwd);
    let given = file_path.replace('\\', "/");
    let abs = if is_absolute(&given) {
        given
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), given)
    };
    below(&cwd, &abs)
}

/// The one spelling of a path text, which every comparison of two paths goes
/// through: forward slashes; no verbatim prefix (`\\?\C:\x` is `C:/x`,
/// `\\?\UNC\srv\share\x` is `//srv/share/x`); `.`, `..` and doubled slashes
/// resolved in the text only, without looking at the disk; the drive letter in
/// capitals. A relative path stays relative. Pure: the same text gives the
/// same answer on every system.
#[must_use]
pub(crate) fn canonical(path: &str) -> String {
    let text = path.replace('\\', "/");
    let text = if let Some(rest) = text.strip_prefix("//?/UNC/").or_else(|| text.strip_prefix("//./UNC/")) {
        format!("/{rest}")
    } else if let Some(rest) = text.strip_prefix("//?/").or_else(|| text.strip_prefix("//./")) {
        if has_drive(rest) { rest.to_string() } else { text }
    } else {
        text
    };
    lexical(&text)
}

/// `true` when `path` opens with a drive letter (`C:`): a Windows path, the
/// only kind whose letter case does not tell places apart.
fn has_drive(path: &str) -> bool {
    path.len() >= 2 && path.as_bytes()[0].is_ascii_alphabetic() && path.as_bytes()[1] == b':'
}

/// The key two spellings of one place share: [`canonical`], and lower case
/// when it is a Windows path. Use it as the key of a map or a set of paths.
#[must_use]
pub(crate) fn place_key(path: &str) -> String {
    let canonical = canonical(path);
    if has_drive(&canonical) { canonical.to_lowercase() } else { canonical }
}

/// `true` when `a` and `b` name the same place ([`place_key`]).
#[must_use]
pub(crate) fn same_place(a: &str, b: &str) -> bool {
    place_key(a) == place_key(b)
}

/// What is left of `path` below `folder`, in the spelling of `path` (empty for
/// the folder itself); `None` when `path` is not inside it. A folder that only
/// shares the start of a name (`/pp` for `/p`) is not the folder. The letter
/// case counts only for a path that is not a Windows one.
#[must_use]
pub(crate) fn below(folder: &str, path: &str) -> Option<String> {
    let (folder, path) = (canonical(folder), canonical(path));
    if path == folder {
        return Some(String::new());
    }
    let prefix = if folder.is_empty() || folder.ends_with('/') { folder } else { format!("{folder}/") };
    let head = path.get(..prefix.len())?;
    let same = if has_drive(&prefix) { head.eq_ignore_ascii_case(&prefix) } else { head == prefix };
    same.then(|| path[prefix.len()..].to_string())
}

/// The canonical text of the real place `path` names: the part that exists is
/// resolved by the system (a Windows short name such as `RUNNER~1`, a link, the
/// case on disk), and the part that does not exist yet is kept as written.
/// What the system gives back is spelled by [`canonical`], so the verbatim
/// prefix Windows puts on it is gone.
#[must_use]
pub(crate) fn on_disk(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut head = abs.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(head) {
            let full = tail.iter().rev().fold(real, |full, part| full.join(part));
            return canonical(&full.to_string_lossy());
        }
        match (head.file_name(), head.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                head = parent;
            }
            _ => return canonical(&abs.to_string_lossy()),
        }
    }
}

/// `path` with `.` and `..` resolved in the text only, without looking at the
/// disk. A `..` that would go past the root of an absolute path stops at it;
/// one at the start of a relative path stays.
fn lexical(path: &str) -> String {
    let (root, rest) = if let Some(rest) = path.strip_prefix('/') {
        ("/".to_string(), rest)
    } else if is_absolute(path) {
        let drive = path.get(..1).unwrap_or_default().to_ascii_uppercase();
        (format!("{drive}:/"), path.get(3..).unwrap_or_default())
    } else {
        (String::new(), path)
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => match parts.last() {
                Some(last) if *last != ".." => {
                    parts.pop();
                }
                _ if root.is_empty() => parts.push(".."),
                _ => {}
            },
            other => parts.push(other),
        }
    }
    format!("{root}{}", parts.join("/"))
}

/// `given` joined to the root when it is relative, in the [`canonical`]
/// spelling.
fn resolved(root: &str, given: &str) -> String {
    let given = given.replace('\\', "/");
    if is_absolute(&given) {
        canonical(&given)
    } else {
        canonical(&format!("{}/{given}", root.replace('\\', "/").trim_end_matches('/')))
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

/// How the tool touches the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// `Read`.
    Read,
    /// `Write`, `Edit`, `MultiEdit` or `NotebookEdit`.
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
    /// Harness state written before the unit exists: `.claude/plans/` and
    /// `.claude/scratch/`.
    Harness,
    /// An artefact or infrastructure: the rest of `.claude/`, `dist/`,
    /// `node_modules/`, `.git/` and `target/`.
    Artifact,
    /// Outside the project root.
    OutsideRepo,
    /// Project code.
    Production,
}

/// The file a file tool is about to touch, already classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WriteTarget {
    /// Read or write.
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
        let rel = relative_to_cwd(root, &given)
            .or_else(|| relative_on_disk(root, &given))
            .map(|rel| rel.trim_start_matches("./").to_string());
        let class = classify_path(root, &given, rel.as_deref());
        Some(Self { access, path: rel.unwrap_or(given), class })
    }
}

/// What is left of the absolute `given` below `root` when both are read as the
/// real places they name ([`on_disk`]): the file reached through a short name,
/// a link or another letter case of the folder is still in the project.
fn relative_on_disk(root: &str, given: &str) -> Option<String> {
    if !is_absolute(given) {
        return None;
    }
    below(&on_disk(Path::new(root)), &on_disk(Path::new(given)))
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
    relative_to_cwd(&main.to_string_lossy(), given).or_else(|| below(&on_disk(&main), &on_disk(Path::new(given))))
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
        // No macOS a pasta temporária é um atalho (`/var` aponta para
        // `/private/var`) e no Windows o caminho resolvido traz o prefixo
        // `\\?\`: a pasta vem na forma que o sistema dá, e o classificador a
        // compara com a mesma conta de todo caminho.
        let tmp_root = std::path::PathBuf::from(on_disk(tmp.path()));
        let main = tmp_root.join("principal");
        std::fs::create_dir_all(&main).expect("main");
        git(&main, &["init", "-q"]);
        git(&main, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "root"]);
        let wt = tmp_root.join("trabalho");
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

    /// The same place written the ways Windows writes it has one canonical
    /// text, and the letter case of a drive path does not tell places apart.
    #[test]
    fn a_windows_place_has_one_canonical_text_however_it_is_written() {
        for written in [
            r"C:\Users\runner\proj\src\a.rs",
            r"\\?\C:\Users\runner\proj\src\a.rs",
            "//?/C:/Users/runner/proj/src/a.rs",
            "C:/Users/runner/proj/./src//a.rs",
            "c:/Users/runner/proj/lib/../src/a.rs",
        ] {
            assert_eq!(canonical(written), "C:/Users/runner/proj/src/a.rs", "{written}");
        }
        assert_eq!(canonical(r"\\?\UNC\srv\share\x"), "/srv/share/x");
        assert_eq!(canonical("src/../../a.rs"), "../a.rs", "a relative path keeps the step out of its start");
        assert_eq!(canonical("/p/../../x"), "/x", "an absolute path stops at its root");
        assert!(same_place(r"c:\USERS\Runner\proj", r"\\?\C:\Users\runner\proj\"));
        assert!(!same_place("/Users/x", "/users/x"), "outside Windows the letter case counts");
        assert!(!same_place(r"C:\Users\runner", r"D:\Users\runner"));
    }

    /// What is below a folder is read in the spelling of the path, whatever the
    /// spelling of the folder; a sibling that only shares the start of the name
    /// is outside.
    #[test]
    fn what_is_below_a_folder_does_not_depend_on_how_the_folder_is_written() {
        let file = "C:/Users/runner/proj/Src/a.rs";
        for folder in [r"\\?\C:\Users\runner\proj", "c:/users/RUNNER/proj/", "C:/Users/runner/./proj"] {
            assert_eq!(below(folder, file).as_deref(), Some("Src/a.rs"), "{folder}");
        }
        assert_eq!(below(r"\\?\C:\Users\runner\proj", r"C:\Users\runner\proj").as_deref(), Some(""));
        assert_eq!(below("C:/Users/runner/proj", "C:/Users/runner/project/a.rs"), None);
        assert_eq!(below("C:/Users/runner/proj", "D:/Users/runner/proj/a.rs"), None);
        assert_eq!(below("/p", "/P/a.rs"), None, "outside Windows the letter case counts");
        assert_eq!(relative_to_cwd(r"\\?\C:\p", r"c:\P\src\a.rs").as_deref(), Some("src/a.rs"));
    }

    /// A folder that exists is read as the real place, so a link to it names it
    /// too; a file that does not exist yet keeps the part it has.
    #[cfg(unix)]
    #[test]
    fn a_folder_reached_through_a_link_is_the_same_place_on_disk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let real = std::fs::canonicalize(tmp.path()).expect("resolved").join("real");
        std::fs::create_dir_all(&real).expect("real");
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        assert_eq!(on_disk(&link.join("new/a.rs")), format!("{}/new/a.rs", canonical(&real.to_string_lossy())));
        let target = WriteTarget::classify(&real.to_string_lossy(), &input("Write", &link.join("src/a.rs").to_string_lossy()));
        assert_eq!(target.map(|t| (t.path, t.class)), Some(("src/a.rs".to_string(), PathClass::Production)));
    }

    /// Every class, through the five file tools; another tool is not
    /// classified.
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
