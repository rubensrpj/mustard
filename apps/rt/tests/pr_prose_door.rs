//! Prose ratchet for the pull-request WRITE path.
//!
//! The `/mustard:pr` and `/git` doors used to instruct the model to run
//! `rtk gh pr create/edit/ready` directly — `github` fixed in text no test
//! covered. Those writes now go through the provider port (`mustard-rt run
//! pr-open` / `pr-edit` / `pr-ready`), and this test is what keeps them there:
//! it reads the two door files and fails on ANY line that names a direct
//! `gh pr create` / `gh pr edit` / `gh pr ready` invocation.
//!
//! READS (`gh pr view`, `gh pr list`, `gh pr diff`) are deliberately NOT
//! flagged — they migrate behind the same port in their own unit, and the door
//! prose still names them as fallbacks until then.
//!
//! Deterministic: reads two committed files, no network, no env vars.

use std::fs;
use std::path::{Path, PathBuf};

/// The write invocations the doors must never name directly again. Any
/// spelling that reaches the provider CLI (`gh pr edit`, `rtk gh pr edit`,
/// inside a chain or a code fence) contains one of these substrings.
const FORBIDDEN: &[&str] = &["gh pr create", "gh pr edit", "gh pr ready"];

/// A porta que a catraca guarda. A porta `git` e as regras de submódulo
/// saíram com o fluxo antigo.
const DOOR_FILES: &[&str] = &["plugin/commands/pr.md"];

/// The repo root, resolved from this crate (`apps/rt`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_pr_doors_never_instruct_a_direct_gh_pr_write() {
    let root = repo_root();
    let mut offenders = Vec::new();
    for rel in DOOR_FILES {
        let path = root.join(rel);
        assert!(path.is_file(), "door file missing at {}", path.display());
        let text = fs::read(&path)
            .map_or_else(|_| String::new(), |b| String::from_utf8_lossy(&b).into_owned());
        for (idx, line) in text.lines().enumerate() {
            for needle in FORBIDDEN {
                if line.contains(needle) {
                    offenders.push(format!("{rel}:{}: names `{needle}`", idx + 1));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "door prose instructs a direct provider-CLI PR write - the WRITE path \
         is the port (`mustard-rt run pr-open` / `pr-edit` / `pr-ready`); \
         rewrite the line to name the command, never the CLI:\n{}",
        offenders.join("\n")
    );
}

/// Os ganchos de aviso do pull request que saíram do produto. Enquanto eles
/// não existirem no código, nenhuma prosa pode ensinar que eles avisam.
const GATES_THAT_LEFT: &[&str] = &["pr-qa-gate", "pr_qa_gate", "pr-body-gate", "pr_body_gate"];

/// Todo arquivo de um tipo, sob `dir`, em ordem.
fn files_under(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, extension, out);
        } else if path.extension().is_some_and(|e| e == extension) {
            out.push(path);
        }
    }
}

/// A prosa não ensina o gancho de aviso que saiu do produto, e não manda
/// montar o corpo do pull request à mão.
///
/// As duas metades precisam concordar. A primeira lê o CÓDIGO: enquanto
/// nenhum arquivo do produto implementa o gancho, nenhum texto pode dizer que
/// ele avisa — um texto que ensina um gancho inexistente manda o leitor
/// esperar um aviso que nunca chega. A segunda lê a porta que publica: ela
/// diz que o corpo é montado do arquivo de eventos e, duas linhas depois,
/// mandava compor o corpo numa lista de seções. As duas ordens não podem ser
/// obedecidas juntas, e quem obedecer a segunda escreve um corpo que a
/// primeira reescreve por cima.
#[test]
fn a_prosa_nao_ensina_o_gancho_que_saiu_nem_manda_montar_o_corpo() {
    let root = repo_root();

    // --- 1. O gancho saiu do código, e nenhum texto o ensina ----------------
    let mut sources = Vec::new();
    for crate_dir in ["apps/rt/src", "apps/cli/src", "packages/core/src"] {
        files_under(&root.join(crate_dir), "rs", &mut sources);
    }
    assert!(sources.len() > 100, "a varredura não leu o código: {} arquivos", sources.len());
    let mut in_code = Vec::new();
    for path in &sources {
        let text = fs::read_to_string(path).unwrap_or_default();
        for gate in GATES_THAT_LEFT {
            if text.contains(gate) {
                in_code.push(format!("{}: {gate}", path.strip_prefix(&root).unwrap_or(path).display()));
            }
        }
    }
    assert!(
        in_code.is_empty(),
        "o gancho de aviso do pull request saiu nesta obra, mas o código ainda \
         o cita — o aviso mora no relatório do comando que abre:\n{}",
        in_code.join("\n"),
    );

    let mut prose = Vec::new();
    files_under(&root.join("plugin"), "md", &mut prose);
    assert!(prose.len() > 3, "a varredura não leu a prosa: {} arquivos", prose.len());
    let mut in_prose = Vec::new();
    for path in &prose {
        let text = fs::read_to_string(path).unwrap_or_default();
        for (idx, line) in text.lines().enumerate() {
            for gate in GATES_THAT_LEFT {
                if line.contains(gate) {
                    in_prose.push(format!(
                        "{}:{}: ensina `{gate}`",
                        path.strip_prefix(&root).unwrap_or(path).display(),
                        idx + 1,
                    ));
                }
            }
        }
    }
    assert!(
        in_prose.is_empty(),
        "a prosa ensina um gancho que o produto não tem mais, e o leitor fica \
         esperando um aviso que nunca chega:\n{}",
        in_prose.join("\n"),
    );

    // --- 2. A porta que publica não manda compor o corpo --------------------
    let pr_md = fs::read_to_string(root.join("plugin/commands/pr.md")).unwrap_or_default();
    assert!(
        pr_md.contains("The PR body is not yours to write."),
        "a porta parou de dizer de onde o corpo vem",
    );
    for order in ["Sections, in order", "| Section | What goes in |"] {
        assert!(
            !pr_md.contains(order),
            "a porta manda compor o corpo em seções duas linhas depois de dizer \
             que ele não é de quem escreve: `{order}`",
        );
    }
}
