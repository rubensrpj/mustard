// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os comandos antigos que criam ou avançam uma spec recusam na entrada e
//! mandam usar o `open`, com a mensagem nos dois idiomas, sem criar nada: o
//! `spec-draft`, o `tactical-fix-create` e, pela linha de comando, os tipos do
//! `emit-pipeline` que criam ou avançam uma spec. O `approve-spec` recusa do
//! mesmo jeito e manda aprovar pela pergunta. Os tipos que só gravam no log
//! velho passam.
//!
//! Tudo roda pelo binário, com o `CLAUDE_PROJECT_DIR` na pasta temporária e
//! sem as variáveis de sessão, para nada cair no projeto de verdade.

use std::path::Path;
use std::process::{Command, Output};

use mustard_core::platform::i18n::{translate, Locale};
use serde_json::Value;

const PT: &str = r#"{"language":{"text":"pt-BR"},"git":{"flow":{"*":"dev","dev":"main"}}}"#;
const EN: &str = r#"{"language":{"text":"en-US"},"git":{"flow":{"*":"dev","dev":"main"}}}"#;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Um repositório parado em `dev`, com o `mustard.json` dado, fora do git.
fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["checkout", "-q", "-b", "dev"]);
    std::fs::write(root.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").unwrap();
    std::fs::write(root.join("mustard.json"), config).unwrap();
    std::fs::write(root.join("README.md"), "oi\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "init"]);
    dir
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

/// A recusa impressa: exit 1, a razão curta e a mensagem.
fn refusal(out: &Output) -> Value {
    assert_eq!(out.status.code(), Some(1), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).expect("a JSON refusal")
}

/// Nada nasceu: nem branch nova, nem pasta de spec.
fn nothing_created(root: &Path) {
    assert_eq!(git(root, &["rev-parse", "--abbrev-ref", "HEAD"]), "dev");
    assert_eq!(git(root, &["for-each-ref", "--format=%(refname:short)", "refs/heads"]), "dev");
    assert!(!root.join(".claude").join("spec").exists(), "no spec folder");
}

#[test]
fn spec_draft_refuses_and_creates_nothing_in_both_languages() {
    for (config, lang) in [(PT, Locale::PtBr), (EN, Locale::EnUs)] {
        let dir = project(config);
        let root = dir.path();
        let out = run(root, &["spec-draft", "--intent", "Cadastro de clientes", "--slug", "cadastro", "--scope", "light"]);
        let report = refusal(&out);
        assert_eq!(report["reason"], "use-open", "{report}");
        assert_eq!(report["hint"], translate("retired.spec_draft", lang), "{report}");
        nothing_created(root);
    }
}

#[test]
fn tactical_fix_create_refuses_and_creates_nothing_in_both_languages() {
    for (config, lang) in [(PT, Locale::PtBr), (EN, Locale::EnUs)] {
        let dir = project(config);
        let root = dir.path();
        let out = run(root, &["tactical-fix-create", "--parent", "epic", "--description", "Ajuste do cadastro"]);
        let report = refusal(&out);
        assert_eq!(report["reason"], "tactical-fix-retired", "{report}");
        assert_eq!(report["hint"], translate("retired.tactical_fix", lang), "{report}");
        nothing_created(root);
    }
}

/// Pela linha de comando, cada tipo que cria ou avança uma spec é recusado,
/// nos dois idiomas, sem gravar nada e sem `meta.json`; os tipos que só
/// gravam no log velho passam.
#[test]
fn the_stage_command_refuses_only_the_kinds_that_create_or_advance_a_spec() {
    let refused = [
        "pipeline.kind",
        "pipeline.status",
        "pipeline.stage",
        "pipeline.outcome",
        "pipeline.wave.start",
        "pipeline.wave.complete",
        "pipeline.complete",
    ];
    for (config, lang) in [(PT, Locale::PtBr), (EN, Locale::EnUs)] {
        let dir = project(config);
        let root = dir.path();
        for kind in refused {
            let out = run(root, &["emit-pipeline", "--kind", kind, "--spec", "cadastro", "--payload", "{}"]);
            let report = refusal(&out);
            assert_eq!(report["reason"], "pipeline-door-retired", "{kind}: {report}");
            let expected = translate("retired.pipeline_door", lang).replace("{kind}", kind);
            assert_eq!(report["hint"], expected, "{kind}: {report}");
            nothing_created(root);
        }
    }
    let dir = project(PT);
    let root = dir.path();
    for kind in ["pipeline.scope", "pipeline.pause", "pipeline.phase", "hygiene.detected"] {
        let out = run(root, &["emit-pipeline", "--kind", kind, "--spec", "cadastro", "--payload", "{}"]);
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("pipeline-door-retired"),
            "{kind} only writes to the old log and passes",
        );
        assert_eq!(out.status.code(), Some(0), "{kind}: stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    }
}

/// A porta antiga da unidade recusa antes de gravar a nota "virou a spec"
/// na pendência: a lista fica com os mesmos bytes.
#[test]
fn the_old_unit_door_refuses_before_writing_the_became_note() {
    let dir = project(PT);
    let root = dir.path();
    let root_s = root.to_string_lossy().into_owned();
    let added = run(root, &["pending", "--add", "--title", "Cadastro", "--detail", "nasceu na conversa", "--root", &root_s]);
    assert!(added.status.success(), "{}", String::from_utf8_lossy(&added.stdout));
    let ledger = root.join(".claude").join("pending").join("ledger.json");
    let before = std::fs::read(&ledger).expect("the list exists");
    let out = run(
        root,
        &["emit-pipeline", "--kind", "pipeline.kind", "--spec", "cadastro", "--intent", "Cadastro", "--pending", "P-1"],
    );
    assert_eq!(refusal(&out)["reason"], "pipeline-door-retired");
    assert_eq!(std::fs::read(&ledger).unwrap(), before, "the list stays the same");
    nothing_created(root);
}

/// O `approve-spec` recusa na entrada, nos dois idiomas, e manda aprovar pela
/// pergunta. Mesmo numa spec antiga já aprovada, com o `meta.json` ao lado,
/// nada é gravado: o estágio não se move, o arquivo de eventos da spec fica
/// com os mesmos bytes e o log velho nem nasce.
#[test]
fn the_approval_command_refuses_and_writes_nothing_in_both_languages() {
    for (config, lang) in [(PT, Locale::PtBr), (EN, Locale::EnUs)] {
        let dir = project(config);
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("epic");
        std::fs::create_dir_all(&spec_dir).unwrap();
        std::fs::write(spec_dir.join("spec.md"), "# Epic\n\n## Contexto\n\nClientes se cadastram sozinhos.\n").unwrap();
        let meta = r#"{"scope":"light","stage":"Draft","outcome":"Active","phase":"PLAN"}"#;
        std::fs::write(spec_dir.join("meta.json"), meta).unwrap();
        let plan = r#"{"v":1,"id":1,"at":"2026-09-14T10:00:00-03:00","type":"state","author":"binary","phase":"plan"}"#;
        let approved = r#"{"v":1,"id":2,"at":"2026-09-14T10:01:00-03:00","type":"state","author":"user","phase":"approved","witness":{"question":"Aprovar esta spec?","answer":"Aprovar"}}"#;
        let events = format!("{plan}\n{approved}\n");
        std::fs::write(spec_dir.join("spec.ndjson"), &events).unwrap();

        let out = run(root, &["approve-spec", "--spec", "epic", "--resume"]);
        let report = refusal(&out);
        assert_eq!(report["reason"], "approve-by-question", "{report}");
        assert_eq!(report["hint"], translate("retired.approve_spec", lang), "{report}");
        assert_eq!(std::fs::read_to_string(spec_dir.join("meta.json")).unwrap(), meta, "the stage did not move");
        assert_eq!(std::fs::read_to_string(spec_dir.join("spec.ndjson")).unwrap(), events, "the spec file is untouched");
        assert!(!spec_dir.join(".events").exists(), "nothing reached the old log");
    }
}

/// Numa spec aberta pelo `open`, o `approve-spec` recusa e não deixa
/// `meta.json` nenhum: a pasta fica com os três arquivos dela, o critério
/// gravado depois é aceito e a página continua sendo refeita.
#[test]
fn the_approval_command_creates_no_meta_json_on_a_spec_opened_by_open() {
    let dir = project(PT);
    let root = dir.path();
    let opened = run(root, &["open", "--kind", "feature", "--name", "cadastro", "--base", "dev"]);
    assert!(opened.status.success(), "{}", String::from_utf8_lossy(&opened.stdout));
    let spec_dir = root.join(".claude").join("spec").join("cadastro");
    assert!(!spec_dir.join("meta.json").exists(), "the open door creates no sidecar");

    // A testemunha grava o estado aprovado; aqui as duas linhas dela entram
    // direto no arquivo.
    let events = spec_dir.join("spec.ndjson");
    let plan = r#"{"v":1,"id":2,"at":"2026-09-14T10:00:00-03:00","type":"state","author":"binary","phase":"plan"}"#;
    let approved = r#"{"v":1,"id":3,"at":"2026-09-14T10:01:00-03:00","type":"state","author":"user","phase":"approved","witness":{"question":"Aprovar esta spec?","answer":"Aprovar"}}"#;
    let mut log = std::fs::read_to_string(&events).unwrap();
    log.push_str(plan);
    log.push('\n');
    log.push_str(approved);
    log.push('\n');
    std::fs::write(&events, log).unwrap();

    let out = run(root, &["approve-spec", "--spec", "cadastro"]);
    assert_eq!(refusal(&out)["reason"], "approve-by-question");
    assert!(!spec_dir.join("meta.json").exists(), "no door creates a meta.json");

    // A spec segue nova: o critério é aceito e a página é refeita com ele.
    let said = run(root, &["write", "--spec", "cadastro", "message", "--json", r#"{"author":"user","text":"Travar o merge."}"#]);
    let said: Value = serde_json::from_slice(&said.stdout).expect("a JSON report");
    assert_eq!(said["ok"], true, "{said}");
    let criterion = format!(
        r#"{{"when":"o merge roda","then":"a pendência trava","proof":"cargo test","origin":{}}}"#,
        said["id"],
    );
    let written = run(root, &["write", "--spec", "cadastro", "criterion", "--json", &criterion]);
    let written: Value = serde_json::from_slice(&written.stdout).expect("a JSON report");
    assert_eq!(written["ok"], true, "the criterion is accepted: {written}");

    let page = run(root, &["page", "--spec", "cadastro"]);
    let page: Value = serde_json::from_slice(&page.stdout).expect("a JSON report");
    assert_eq!(page["ok"], true, "the page is still rebuilt: {page}");
    let md = std::fs::read_to_string(spec_dir.join("spec.md")).unwrap();
    assert!(md.contains("MSTD-CRIT-0001"), "the criterion reaches the page: {md}");
    assert!(!spec_dir.join("meta.json").exists(), "and still no sidecar");
}
