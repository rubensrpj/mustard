// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O `pr-review` com `--verdict` não grava mais o veredito: recusa na entrada,
//! nos dois idiomas, manda esperar a rodada e não grava nada. O arquivo de
//! eventos da spec e a lista de pendências ficam com os mesmos bytes, e
//! nenhuma cobrança é armada.
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
