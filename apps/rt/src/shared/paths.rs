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
//! ## O arquivo que um gancho vai tocar
//!
//! [`WriteTarget::classify`] é o classificador único do portão de escrita: a
//! ferramenta (leitura ou escrita), o caminho relativo à raiz e a classe do
//! arquivo ([`PathClass`]). [`relative_to_cwd`] é a única conta de "caminho
//! relativo à raiz" dos ganchos de escrita.

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

/// Os arquivos da pasta de uma spec que só o binário grava. O `meta.json` da
/// raiz da spec entra também: é ele que diz se um rascunho antigo, sem
/// arquivo de eventos, ainda trava, e nenhum passo manda editá-lo à mão.
const SPEC_FILES: &[&str] = &["spec.ndjson", "spec.md", "spec.html", "meta.json"];

/// Os arquivos de `.claude/spec/` fora da pasta de uma spec que só o binário
/// grava: o índice das specs e o banco de lições.
const BANK_FILES: &[&str] = &[SPEC_INDEX_FILE, LESSONS_FILE];

/// Estado do harness escrito antes de a unidade existir: os planos do modo de
/// plano, a evidência descartável que um diagnóstico roda e o cache do
/// harness, onde o material do `/feature` e do `/bugfix` espera o
/// `spec-draft`. Nenhum deles é código, e o `.gitignore` semeado ignora os
/// três: a trava das bases protege o código do projeto, e não os arquivos do
/// próprio Mustard que o git não vê.
const HARNESS_PREFIXES: &[&str] = &[".claude/plans/", ".claude/scratch/", ".claude/.cache/"];

/// Artefatos e infraestrutura, nunca código do projeto.
const ARTIFACT_PREFIXES: &[&str] = &[".claude/", "dist/", "node_modules/", ".git/", "target/"];

/// O caminho de `file_path` relativo a `cwd`, com barras normais. Um caminho
/// relativo é lido a partir de `cwd`. `None` quando o arquivo fica fora de
/// `cwd`; `Some("")` para a própria raiz.
#[must_use]
pub(crate) fn relative_to_cwd(cwd: &str, file_path: &str) -> Option<String> {
    let cwd_norm = cwd.replace('\\', "/");
    let fp_norm = file_path.replace('\\', "/");
    let abs = if is_absolute(&fp_norm) {
        fp_norm
    } else {
        format!("{}/{}", cwd_norm.trim_end_matches('/'), fp_norm)
    };
    // `.` e `..` saem antes da comparação: `.claude/../src/x.rs` é código do
    // projeto, e não um artefato de `.claude/`.
    let (abs, cwd) = (lexical(&abs), lexical(&cwd_norm));
    if abs == cwd {
        return Some(String::new());
    }
    let prefix = if cwd.is_empty() || cwd.ends_with('/') { cwd } else { format!("{cwd}/") };
    abs.strip_prefix(&prefix).map(str::to_string)
}

/// `path` com `.` e `..` resolvidos só no texto, sem olhar o disco. Um `..`
/// que passaria da raiz para nela.
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

/// `given` junto da raiz quando é relativo, com barras normais e com `.` e
/// `..` resolvidos só no texto.
fn resolved(root: &str, given: &str) -> String {
    let given = given.replace('\\', "/");
    if is_absolute(&given) {
        lexical(&given)
    } else {
        lexical(&format!("{}/{given}", root.replace('\\', "/").trim_end_matches('/')))
    }
}

/// `true` quando um caminho com barras normais é absoluto: `/...` ou `C:/...`.
fn is_absolute(p: &str) -> bool {
    p.starts_with('/')
        || (p.len() >= 3
            && p.as_bytes()[0].is_ascii_alphabetic()
            && p.as_bytes()[1] == b':'
            && p.as_bytes()[2] == b'/')
}

/// O padrão de arquivo sensível que `path` casa: credenciais, chaves e a
/// configuração do git. Sem distinguir maiúsculas e sobre o caminho inteiro,
/// então uma pasta com o nome também casa (`config/credentials/prod.yaml`) —
/// o que as regras de `permissions.deny` não conseguem dizer.
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
    /// Como `tool` toca o arquivo; `None` para uma ferramenta que não é de
    /// arquivo.
    #[must_use]
    pub(crate) fn of_tool(tool: &str) -> Option<Self> {
        match tool {
            "Read" => Some(Self::Read),
            "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => Some(Self::Write),
            _ => None,
        }
    }
}

/// O que o arquivo é, para o portão de escrita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathClass {
    /// Casou um padrão de arquivo sensível ([`sensitive_pattern`]), dentro ou
    /// fora do projeto.
    Secret {
        /// O padrão que casou.
        pattern: &'static str,
    },
    /// Um arquivo que só o binário grava: o `spec.ndjson`, o `spec.md` ou o
    /// `spec.html` de uma spec, o índice das specs ou o banco de lições. Num
    /// worktree, também os do checkout principal.
    SpecFile {
        /// A spec dona do arquivo, quando ele mora na pasta dela.
        spec: Option<String>,
    },
    /// Estado do harness escrito antes de a unidade existir:
    /// `.claude/plans/`, `.claude/scratch/` e `.claude/.cache/`.
    Harness,
    /// Artefato ou infraestrutura: o resto de `.claude/`, `dist/`,
    /// `node_modules/`, `.git/` e `target/`.
    Artifact,
    /// Fora da raiz do projeto.
    OutsideRepo,
    /// Código do projeto.
    Production,
}

/// O arquivo que uma ferramenta de arquivo vai tocar, já classificado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WriteTarget {
    /// Leitura ou escrita.
    pub(crate) access: Access,
    /// O caminho relativo à raiz quando o arquivo mora nela; senão, o caminho
    /// como veio, com barras normais.
    pub(crate) path: String,
    /// O que o arquivo é.
    pub(crate) class: PathClass,
}

impl WriteTarget {
    /// Classifica o arquivo que `input` vai tocar, visto da raiz `root`.
    /// `None` quando a ferramenta não é de arquivo ou não traz caminho.
    #[must_use]
    pub(crate) fn classify(root: &str, input: &HookInput) -> Option<Self> {
        let access = Access::of_tool(input.tool_name.as_deref()?)?;
        let given = input.file_path()?.replace('\\', "/");
        let rel = relative_to_cwd(root, &given).map(|rel| rel.trim_start_matches("./").to_string());
        let class = classify_path(root, &given, rel.as_deref());
        Some(Self { access, path: rel.unwrap_or(given), class })
    }
}

/// A classe de `given`, que fica em `rel` quando mora na raiz.
fn classify_path(root: &str, given: &str, rel: Option<&str>) -> PathClass {
    // As duas comparações que olham o caminho inteiro recebem-no já junto da
    // raiz e sem `.`, `..` nem barra dobrada: `/p/.git/./config` e
    // `/p/.git//config` são a configuração do git, e a pasta das specs do
    // checkout principal continua achada quando o caminho traz um `./`.
    let full = resolved(root, given);
    // O caminho inteiro contém o nome do arquivo, então um padrão de nome casa
    // nele também.
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

/// O caminho de `given` relativo ao checkout principal, quando a raiz é um
/// worktree e `given` aponta a pasta das specs de lá: num worktree, as specs
/// moram no checkout principal. Só olha o git para um caminho absoluto que
/// passa por `.claude/spec/`.
fn main_checkout_rel(root: &str, given: &str) -> Option<String> {
    if !is_absolute(given) || !given.contains("/.claude/spec/") {
        return None;
    }
    let main = spec_root(Path::new(root));
    relative_to_cwd(&main.to_string_lossy(), given)
}

/// A classe de um arquivo da pasta das specs que só o binário grava.
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

    /// A raiz vale para o caminho absoluto e para o relativo, com barras de
    /// Windows também; fora da raiz não há caminho relativo.
    #[test]
    fn a_path_is_made_relative_to_the_root_once() {
        assert_eq!(relative_to_cwd("/p", "/p/src/a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(relative_to_cwd("/p/", "src/a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(relative_to_cwd("C:\\p", "C:\\p\\src\\a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(relative_to_cwd("/p", "/p").as_deref(), Some(""));
        assert_eq!(relative_to_cwd("/p", "/outra/a.rs"), None);
        assert_eq!(relative_to_cwd("/p", "/pp/a.rs"), None, "a sibling folder is outside");
        // `.` e `..` resolvidos só no texto: o caminho que dá a volta por
        // `.claude/` chega onde aponta, e um que sai da raiz fica fora.
        assert_eq!(relative_to_cwd("/p", "/p/.claude/../src/x.rs").as_deref(), Some("src/x.rs"));
        assert_eq!(
            relative_to_cwd("/p", ".claude/spec/x/../x/./spec.md").as_deref(),
            Some(".claude/spec/x/spec.md"),
        );
        assert_eq!(relative_to_cwd("/p", "/p/../outra/a.rs"), None);
    }

    /// Um `.` ou uma barra dobrada no meio do caminho não escondem a
    /// configuração do git.
    #[test]
    fn a_dot_or_a_double_slash_does_not_hide_a_sensitive_file() {
        for path in ["/p/.git/./config", "/p/.git//config", "/p/src/../.git/config", ".git/./config"] {
            for tool in ["Read", "Write"] {
                let target = WriteTarget::classify("/p", &input(tool, path)).expect("a file tool");
                assert_eq!(target.class, PathClass::Secret { pattern: ".git/config" }, "{tool} {path}");
            }
        }
    }

    /// Visto de um worktree, o arquivo de eventos da spec do checkout
    /// principal continua sendo um arquivo que só o binário grava, mesmo com
    /// um `./` no meio do caminho.
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
