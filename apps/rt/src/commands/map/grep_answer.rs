//! A resposta ao texto do `Grep` pelo mapa: os arquivos do mapa com linhas
//! achadas, cada linha na declaração dona dela, no teto da conversa. O
//! arquivo que o mapa não tem, como o de documentação e o de dados, fica de
//! fora, e a última linha diz como ver a busca inteira.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mustard_core::domain::code_search::Request;
use mustard_core::io::code_search::{Answer, occurrences};
use mustard_core::io::map_search::{self, OwnedLines};
use mustard_core::io::project_map::model_path;
use mustard_core::platform::i18n::Locale;
use serde_json::{Value, json};

use crate::commands::knowledge::CONVERSATION_BYTES;
use crate::commands::search::{conversation_output, rest_line};
use crate::shared::say::say;

/// O relato da busca nativa de `request`, feita em `root`, com a resposta em
/// texto que o comando imprime no lugar dele: a do mapa, ou, sem o mapa, as
/// linhas da busca agrupadas como na busca do terminal; as duas no teto da
/// conversa. A busca sem linha achada, ou com erro, fica só com o relato.
pub(super) fn answered(root: &Path, request: &Request, answer: Answer) -> Value {
    let mut report = answer.report.clone();
    if answer.exit_code != 0 {
        return report;
    }
    let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
    let text = match by_the_map(root, request, &answer) {
        Some(owned) => within(root, request, &owned, lang, CONVERSATION_BYTES),
        None => {
            let view = mustard_core::io::code_search::presentation::agent(&answer, request, root).stdout;
            String::from_utf8_lossy(&conversation_output(view, None, CONVERSATION_BYTES, || rest_line(root, request))).into_owned()
        }
    };
    report["question"] = json!("search");
    report["answer"] = json!(text);
    report
}

/// As linhas achadas separadas pelo mapa do projeto, como a porta da busca o
/// acha: o da árvore do git de `root`, ou o da raiz da spec. `None` quando o
/// mapa não se lê.
fn by_the_map(root: &Path, request: &Request, answer: &Answer) -> Option<OwnedLines> {
    let cwd = root.canonicalize().ok()?;
    let anchor = mustard_core::io::spec_events::spec_root(&cwd);
    let git = mustard_core::platform::git::run(&cwd, &["rev-parse", "--show-toplevel"]);
    let tree = if git.ok { PathBuf::from(git.stdout.trim()) } else { anchor.clone() };
    let tree = tree.canonicalize().ok()?;
    let map_root = if model_path(&tree).is_file() { tree.clone() } else { anchor };
    let hits: Vec<(String, u64)> =
        occurrences(&tree, &cwd, request, &answer.report["result"], &answer.stdout).into_iter().map(|(file, line, _)| (file, line)).collect();
    map_search::line_owners(&map_root, &hits).ok()
}

/// O texto da resposta pelo mapa, cortado em `cap` bytes com a nota do corte
/// e a linha de pedir a busca inteira.
fn within(root: &Path, request: &Request, owned: &OwnedLines, lang: Locale, cap: usize) -> String {
    let mut text = String::new();
    if !owned.outside.is_empty() {
        text.push_str(&say("map.answer.outside", lang, &[("{count}", &owned.outside.len().to_string())]));
        text.push('\n');
    }
    if !owned.files.is_empty() {
        text.push_str(&say("map.answer.lines", lang, &[]));
        text.push('\n');
    }
    let joined = |lines: &[u64]| lines.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
    for file in &owned.files {
        let _ = writeln!(text, "@ {}", file.path);
        for owner in &file.owners {
            let _ = writeln!(text, "{} {}-{} ({})", owner.name, owner.line, owner.end_line, joined(&owner.lines));
        }
        if !file.loose.is_empty() {
            let _ = writeln!(text, "({})", joined(&file.loose));
        }
    }
    let cut = text.len() > cap;
    let mut out = conversation_output(text.into_bytes(), None, cap, || rest_line(root, request));
    if !cut && !owned.outside.is_empty() {
        out.extend_from_slice(rest_line(root, request).as_bytes());
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::super::{GrepSearch, MapOpts, Question};
    use super::*;

    /// A busca pelo texto num projeto em TypeScript com o mapa do scan de
    /// verdade responde em texto: o arquivo do mapa, com a linha achada na
    /// função dona dela, e sem a nota em Markdown, que só a contagem de fora
    /// do mapa e a linha da busca inteira lembram. No teto pequeno, a
    /// resposta sai cortada, com a nota do corte.
    #[test]
    fn a_grep_text_answers_with_the_map_files_and_their_owning_declarations_within_the_cap() {
        let scan = mustard_core::Scan::locate();
        assert!(scan.is_compiled_alongside(), "rode `cargo build -p scan` antes de `cargo test -p mustard-rt`");
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(&root)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        std::fs::create_dir_all(root.join("web/src")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("web/src/clinic.ts"),
            "export class Clinic {\n  visit(): string {\n    return \"doctor\";\n  }\n}\n\nexport function payOrder(total: number): number {\n  return total;\n}\n",
        )
        .unwrap();
        std::fs::write(root.join("docs/clinic.md"), "The doctor is in.\n").unwrap();
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        scan.scan(&root, &model_path(&root)).expect("the first pass writes the map");

        let grep = GrepSearch { pattern: "doctor".into(), ..GrepSearch::default() };
        let opts = MapOpts {
            root: root.clone(),
            question: Question::Search,
            grep: Some(grep.clone()),
            file: None,
            query: None,
            intent: None,
            described: None,
            said: None,
            name: None,
            pr: None,
            session: None,
        };
        let report = super::super::source_search(&opts, &grep);
        assert_eq!(report["question"], json!("search"), "{report}");
        let answer = report["answer"].as_str().unwrap();
        assert!(answer.contains("@ web/src/clinic.ts\nvisit 2-4 (3)\n"), "{answer}");
        assert!(!answer.contains("docs/clinic.md") && !answer.contains("The doctor is in"), "{answer}");
        assert!(answer.contains(&say("map.answer.outside", Locale::PtBr, &[("{count}", "1")])), "{answer}");
        assert!(answer.ends_with("> <file>\n"), "{answer}");

        let request = Request::native(&["rg".into(), "-n".into(), "-e".into(), "doctor".into(), "--".into(), ".".into()]).unwrap();
        let owned = map_search::line_owners(&root, &[("web/src/clinic.ts".into(), 3)]).unwrap();
        let cut = within(&root, &request, &owned, Locale::PtBr, 100);
        assert!(cut.contains("# Answer cut to fit the conversation cap of 100 bytes"), "{cut}");
        assert!(!cut.contains("visit 2-4"), "{cut}");
    }
}
