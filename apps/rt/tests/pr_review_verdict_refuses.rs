// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O `pr-review` com `--verdict` não grava mais o veredito: recusa na entrada,
//! nos dois idiomas, manda esperar a rodada e não grava nada. O arquivo de
//! eventos da spec e a lista de pendências ficam com os mesmos bytes, e
//! nenhuma cobrança é armada.
//!
//! E nenhum texto publicado ensina essa gravação: nem a ajuda que o binário
//! imprime, nem as dicas das portas, nem a referência de comandos, nem a prosa
//! do plugin. Um texto que ensina o que o binário recusa gasta a chamada de
//! quem obedece.
//!
//! A recusa encerra o processo, então o teste roda pelo binário, com o
//! `CLAUDE_PROJECT_DIR` na pasta temporária e sem as variáveis de sessão, para
//! nada cair no projeto de verdade.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mustard_core::platform::i18n::{translate, Locale};
use serde_json::Value;

const PT: &str = r#"{"language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"}}}"#;
const EN: &str = r#"{"language":{"text":"en-US"},"git":{"flow":{"*":"dev","dev":"main"}}}"#;

/// A spec aberta em cada projeto destes testes.
const SPEC: &str = "cadastro";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `mustard-rt run <args>` no projeto `root`.
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .args(args)
        .current_dir(root)
        .env("CLAUDE_PROJECT_DIR", root)
        .env_remove("MUSTARD_PROJECT_ROOT")
        .env_remove("MUSTARD_SESSION_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDE_SESSION_ID")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .env_remove("MUSTARD_APPROVAL_MODE")
        .output()
        .expect("run mustard-rt")
}

/// Um repositório com `dev` e `main`, uma pendência na lista e a spec
/// `cadastro` aberta a partir dela: há o que um veredito gravado mudaria.
fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["checkout", "-q", "-b", "main"]);
    std::fs::write(root.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").unwrap();
    std::fs::write(root.join("mustard.json"), config).unwrap();
    std::fs::write(root.join("README.md"), "oi\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["checkout", "-q", "-b", "dev"]);

    let added = run(root, &["pending", "--add", "--title", "Humanize", "--detail", "nasceu na conversa"]);
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stdout));
    let opened = run(root, &["open", "--kind", "feature", "--name", SPEC, "--base", "dev", "--pending", "P-1"]);
    assert!(opened.status.success(), "{}", String::from_utf8_lossy(&opened.stdout));
    assert!(spec_file(root).is_file(), "the spec was opened");
    dir
}

fn spec_file(root: &Path) -> PathBuf {
    root.join(".claude").join("spec").join(SPEC).join("spec.ndjson")
}

fn ledger(root: &Path) -> PathBuf {
    root.join(".claude").join("pending").join("ledger.json")
}

fn charges(root: &Path) -> PathBuf {
    root.join(".claude").join("pending").join("charges.json")
}

/// O `pr-review` com qualquer um dos dois vereditos recusa com exit 1, diz
/// para esperar a rodada no idioma do projeto e não muda nada: nem o arquivo
/// de eventos da spec, nem a lista de pendências, e nenhuma cobrança nasce.
#[test]
fn pr_review_with_a_verdict_refuses_and_records_nothing() {
    for (config, lang) in [(PT, Locale::PtBr), (EN, Locale::EnUs)] {
        let dir = project(config);
        let root = dir.path();
        let events_before = std::fs::read(spec_file(root)).unwrap();
        let ledger_before = std::fs::read(ledger(root)).unwrap();
        let expected = translate("retired.wait_round", lang).replace("{command}", "pr-review --verdict");

        for verdict in ["approved", "rejected"] {
            let args = ["pr-review", "--pr", "1", "--verdict", verdict];
            let out = run(root, &args);
            assert_eq!(out.status.code(), Some(1), "{args:?} stderr:\n{}", String::from_utf8_lossy(&out.stderr));
            let report: Value = serde_json::from_slice(&out.stdout).expect("a JSON refusal");
            assert_eq!(report["ok"], false, "{args:?}: {report}");
            assert_eq!(report["reason"], "wait-for-round", "{args:?}: {report}");
            assert_eq!(report["hint"], expected, "{args:?}: {report}");
            assert_eq!(std::fs::read(spec_file(root)).unwrap(), events_before, "{args:?} wrote to the spec");
            assert_eq!(std::fs::read(ledger(root)).unwrap(), ledger_before, "{args:?} touched the list");
            assert!(!charges(root).exists(), "{args:?} armed a charge");
        }
    }
}

/// A raiz do repositório, a partir desta crate (`apps/rt`), para a varredura
/// não depender da pasta de onde o teste foi chamado.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Os arquivos de extensão `ext` sob `dir`, em qualquer profundidade.
fn collect(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, ext, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

/// Os textos publicados desta porta: a referência de comandos e os READMEs
/// que apontam para ela, a prosa que o plugin entrega ao agente, e o que o
/// binário imprime — a ajuda, as dicas e o próximo passo da porta de revisão e
/// da de merge, mais o catálogo de mensagens.
fn published_texts() -> Vec<(String, String)> {
    let root = repo_root();
    let mut files =
        vec![root.join("MUSTARD-COMMANDS.md"), root.join("README.md"), root.join("README.en.md")];
    for (dir, ext) in [
        ("plugin", "md"),
        ("apps/rt/src/commands/review", "rs"),
        ("packages/core/src/platform/i18n", "rs"),
    ] {
        let dir = root.join(dir);
        // A superfície é afirmada, e não pulada: uma pasta renomeada faria a
        // varredura passar sem ler nada, que é indistinguível de uma limpa.
        assert!(dir.is_dir(), "a superfície publicada `{}` sumiu", dir.display());
        let before = files.len();
        collect(&dir, ext, &mut files);
        assert!(files.len() > before, "a superfície publicada `{}` não rendeu arquivo", dir.display());
    }
    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} não abriu: {e}", path.display()));
            (path.display().to_string(), text)
        })
        .collect()
}

/// A gravação do veredito saiu deste comando, e nenhum texto publicado pode
/// continuar ensinando-a: nem a linha com o veredito, nem a contagem de
/// críticos, que só existia para ser gravada com ele. A contagem também saiu
/// da linha de comando — quem a passa recebe o erro do clap, não uma gravação
/// que não acontece — e a ajuda que o binário imprime diz que o comando
/// recusa.
#[test]
fn no_published_text_teaches_the_recording_the_command_refuses() {
    /// O que um texto publicado não pode trazer: a opção da contagem de
    /// críticos e o veredito com um valor, que juntos são a chamada que grava.
    /// O `--verdict <VERDICT>` da própria ajuda do clap não é um valor: é a
    /// vaga da opção que existe só para recusar.
    const TEACHES: &[&str] =
        &["--critical", "--verdict approved", "--verdict rejected", "--verdict <approved"];

    let mut offenders = Vec::new();
    for (name, text) in published_texts() {
        for (n, line) in text.lines().enumerate() {
            for taught in TEACHES {
                if line.contains(taught) {
                    offenders.push(format!("{name}:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "texto publicado ensinando a gravação que o `pr-review` recusa desde a onda 10 — \
         quem obedecer gasta a chamada numa recusa:\n{}",
        offenders.join("\n")
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    let help = run(root, &["pr-review", "--help"]);
    assert_eq!(help.status.code(), Some(0), "{}", String::from_utf8_lossy(&help.stderr));
    let printed = String::from_utf8_lossy(&help.stdout).to_string();
    for taught in TEACHES {
        assert!(!printed.contains(taught), "a ajuda impressa ainda ensina `{taught}`:\n{printed}");
    }
    assert!(printed.contains("recusa"), "a ajuda impressa não diz que o comando recusa:\n{printed}");

    let gone = run(root, &["pr-review", "--pr", "1", "--critical", "3"]);
    assert_eq!(gone.status.code(), Some(2), "a contagem de críticos ainda é uma opção");
    assert!(
        String::from_utf8_lossy(&gone.stderr).contains("--critical"),
        "o erro nomeia a opção que não existe mais"
    );
}
