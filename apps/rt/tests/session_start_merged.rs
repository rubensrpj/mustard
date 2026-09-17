// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

//! O início da sessão com a spec atual em "pull request aberto", pelo binário
//! de verdade, num repositório de verdade com um remoto, trinta branches de
//! colegas, e o provedor trocado por um `gh` falso no `PATH` que anota cada
//! pergunta que recebe.
//!
//! - O pull request segue aberto: o provedor é perguntado uma vez só, pelo
//!   pull request da spec, e nada muda.
//! - O pull request entrou pelas mãos de outra pessoa: roda o mesmo caminho do
//!   merge do Mustard — a spec gravada como entregue, a base atualizada, a
//!   branch local apagada, a do servidor só com a opção ligada, e a pergunta
//!   das pendências nascidas na spec —, sem uma pergunta por branch de colega.
//! - O provedor não responde: o aviso diz isso, e nada muda.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;

const SPEC: &str = "entrega";
const BRANCH: &str = "feature/entrega";
const SESSION: &str = "s-entrega";
const COLLEAGUES: usize = 30;

/// O `gh` falso: anota cada chamada, uma por linha, e responde o que o teste
/// mandou — a visão do pull request, uma lista vazia, ou a falha.
const FAKE_GH: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_GH_LOG"
if [ "$FAKE_GH_EXIT" != "0" ]; then
  echo "o provedor está fora do ar" >&2
  exit "$FAKE_GH_EXIT"
fi
case "$1 $2" in
  "pr view") printf '%s' "$FAKE_GH_VIEW" ;;
  "pr list") printf '[]' ;;
  *) printf '{}' ;;
esac
"#;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git on PATH");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Um projeto com o Mustard, o checkout na branch da spec em "pull request
/// aberto", trinta branches de colegas com trabalho próprio (aqui e no
/// remoto) e uma pendência nascida na spec.
struct Scene {
    dir: TempDir,
    work: PathBuf,
}

impl Scene {
    fn new(delete_remote: bool) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let origin = dir.path().join("origin.git");
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(dir.path(), &["init", "-q", "--bare", origin.to_str().unwrap()]);
        git(&work, &["init", "-q", "."]);
        git(&work, &["symbolic-ref", "HEAD", "refs/heads/dev"]);
        std::fs::write(work.join(".git/info/exclude"), ".claude/\nmustard.json\n").unwrap();
        let config = json!({
            "language": {"text": "pt-BR"},
            "git": {"flow": {"*": "dev"}, "provider": "github", "deleteRemoteBranch": delete_remote}
        });
        std::fs::write(work.join("mustard.json"), config.to_string()).unwrap();
        std::fs::write(work.join("README.md"), "loja\n").unwrap();
        git(&work, &["add", "README.md"]);
        git(&work, &["commit", "-q", "-m", "semente"]);
        git(&work, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&work, &["push", "-q", "-u", "origin", "dev"]);

        // As branches dos colegas: cada uma com um commit próprio, aqui e no
        // remoto.
        let tree = git(&work, &["rev-parse", "dev^{tree}"]);
        for n in 1..=COLLEAGUES {
            let commit = git(&work, &["commit-tree", &tree, "-p", "dev", "-m", &format!("colega {n}")]);
            git(&work, &["update-ref", &format!("refs/heads/feature/colega-{n}"), &commit]);
        }
        git(&work, &["push", "-q", "origin", "refs/heads/feature/*:refs/heads/feature/*"]);

        // A branch da spec, com o trabalho dela, no remoto.
        git(&work, &["checkout", "-q", "-b", BRANCH]);
        std::fs::write(work.join("entrega.txt"), "a entrega\n").unwrap();
        git(&work, &["add", "entrega.txt"]);
        git(&work, &["commit", "-q", "-m", "a entrega"]);
        git(&work, &["push", "-q", "-u", "origin", BRANCH]);

        // A pendência combinada durante a spec.
        let added = rt(&work, &["pending", "--add", "--title", "Humanize", "--detail", "combinado na conversa"]);
        assert_eq!(added["id"], json!("P-1"), "{added}");

        // A spec em "pull request aberto", com o número do pull request.
        let at = "2026-09-17T08:00:00-03:00";
        let lines = [
            json!({"v":1,"id":1,"at":at,"type":"state","author":"binary","phase":"survey","branch":BRANCH,"base":"dev"}),
            json!({"v":1,"id":2,"at":at,"type":"message","author":"user","text":"Faça a entrega."}),
            json!({"v":1,"id":3,"at":at,"type":"state","author":"binary","phase":"plan"}),
            json!({"v":1,"id":4,"at":at,"type":"state","author":"user","phase":"approved",
                "witness":{"question":"Aprovar esta spec?","answer":"Aprovar"}}),
            json!({"v":1,"id":5,"at":at,"type":"state","author":"binary","phase":"running"}),
            json!({"v":1,"id":6,"at":at,"type":"deferred","author":"assistant","text":"Humanize fica para depois.",
                "keys":["humanize"],"pending":1,"origin":2}),
            json!({"v":1,"id":7,"at":at,"type":"state","author":"binary","phase":"closed"}),
            json!({"v":1,"id":8,"at":at,"type":"state","author":"binary","phase":"pr_open",
                "pr":{"number":7,"url":"https://exemplo/7"}}),
        ];
        let spec_dir = work.join(".claude/spec").join(SPEC);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(spec_dir.join("spec.ndjson"), body).unwrap();

        // O `gh` falso.
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("gh"), FAKE_GH).unwrap();
        std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o755)).unwrap();

        let scene = Self { dir, work };
        assert_eq!(scene.phase(), "pr_open", "the spec starts with its pull request open");
        scene
    }

    /// Outra pessoa faz o merge da branch da spec na base, no remoto.
    fn merged_by_someone_else(&self) {
        let colega = self.dir.path().join("colega");
        let origin = self.dir.path().join("origin.git");
        git(self.dir.path(), &["clone", "-q", "-b", "dev", origin.to_str().unwrap(), colega.to_str().unwrap()]);
        git(&colega, &["merge", "-q", "--no-ff", "-m", "Merge do PR #7", &format!("origin/{BRANCH}")]);
        git(&colega, &["push", "-q", "origin", "dev"]);
    }

    /// O início da sessão depois de `/clear`, com o `gh` falso respondendo
    /// `view` a `pr view`, ou falhando com `exit`. Devolve o texto que ele pôs
    /// na sessão e as perguntas que o provedor recebeu.
    fn start(&self, view: &Value, exit: i32) -> (String, Vec<String>) {
        let log = self.dir.path().join(format!("gh-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let path = format!("{}:{}", self.dir.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let input = json!({
            "hook_event_name": "SessionStart",
            "session_id": SESSION,
            "source": "clear",
            "cwd": self.work.to_str().unwrap(),
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["on", "SessionStart"])
            .current_dir(&self.work)
            .env("PATH", path)
            .env("FAKE_GH_LOG", &log)
            .env("FAKE_GH_VIEW", view.to_string())
            .env("FAKE_GH_EXIT", exit.to_string())
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("MUSTARD_WORKSPACE_ROOT")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env_remove("MUSTARD_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn mustard-rt");
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().expect("wait mustard-rt");
        assert!(out.status.success(), "mustard-rt: {}", String::from_utf8_lossy(&out.stderr));
        let stdout = String::from_utf8_lossy(&out.stdout);
        let context = serde_json::from_str::<Value>(stdout.trim())
            .ok()
            .and_then(|v| v.pointer("/hookSpecificOutput/additionalContext").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        let asked = std::fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_string).collect();
        (context, asked)
    }

    /// A fase da spec, pelo último `state` do arquivo de eventos.
    fn phase(&self) -> String {
        let body = std::fs::read_to_string(self.work.join(".claude/spec").join(SPEC).join("spec.ndjson")).unwrap();
        body.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|event| event["type"] == json!("state"))
            .last()
            .and_then(|event| event["phase"].as_str().map(str::to_string))
            .unwrap_or_default()
    }

    fn local_branch_exists(&self) -> bool {
        !git(&self.work, &["branch", "--list", BRANCH]).is_empty()
    }

    fn remote_branch_exists(&self) -> bool {
        !git(&self.work, &["ls-remote", "--heads", "origin", BRANCH]).is_empty()
    }
}

fn rt(root: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
        .arg("run")
        .args(args)
        .arg("--root")
        .arg(root)
        .current_dir(root)
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .output()
        .expect("run mustard-rt");
    serde_json::from_slice(&out.stdout).expect("JSON")
}

/// A visão do pull request que o `gh` falso devolve, no estado `state`.
fn view(state: &str) -> Value {
    json!({"number": 7, "title": "Entrega", "state": state, "headRefName": BRANCH,
        "baseRefName": "dev", "isDraft": false, "url": "https://exemplo/7"})
}

/// Com o pull request da spec ainda aberto, o início da sessão pergunta ao
/// provedor uma vez só, por esse pull request, mesmo com trinta branches de
/// colegas no repositório; a spec e a branch ficam como estão.
#[test]
fn an_open_pull_request_costs_one_question_whatever_the_number_of_branches() {
    let scene = Scene::new(false);
    let (context, asked) = scene.start(&view("OPEN"), 0);
    assert_eq!(asked.len(), 1, "one question to the provider: {asked:#?}");
    assert!(asked[0].starts_with("pr view 7 "), "about the spec's pull request: {asked:?}");
    assert!(context.contains(SPEC), "the session start still speaks: {context}");
    assert!(!context.contains("entrou pelas mãos"), "{context}");
    assert_eq!(scene.phase(), "pr_open");
    assert!(scene.local_branch_exists());
    assert_eq!(git(&scene.work, &["branch", "--show-current"]), BRANCH);
}

/// Com o pull request da spec feito por outra pessoa, o início da sessão roda
/// o mesmo caminho do merge do Mustard: a spec gravada como entregue, a base
/// atualizada, a branch local apagada e a do servidor mantida, e a pergunta
/// das pendências nascidas na spec armada e dita no texto. O provedor não é
/// perguntado por nenhuma branch de colega.
#[test]
fn a_pull_request_merged_by_someone_else_is_delivered_through_the_merge_path() {
    let scene = Scene::new(false);
    scene.merged_by_someone_else();
    let (context, asked) = scene.start(&view("MERGED"), 0);

    assert!(asked.len() <= 2, "only the spec's pull request is asked about: {asked:#?}");
    assert!(asked[0].starts_with("pr view 7 "), "{asked:?}");
    assert!(!asked.iter().any(|call| call.contains("colega")), "no colleague's branch is asked about: {asked:#?}");

    assert_eq!(scene.phase(), "delivered", "the spec is recorded as delivered");
    assert!(!scene.local_branch_exists(), "the local branch is gone");
    assert_eq!(git(&scene.work, &["branch", "--show-current"]), "dev", "the checkout is back on the base");
    assert_eq!(
        git(&scene.work, &["rev-parse", "dev"]),
        git(&scene.work, &["rev-parse", "origin/dev"]),
        "the base holds the merge"
    );
    assert!(scene.work.join("entrega.txt").is_file(), "the merged work is in the tree");
    assert!(scene.remote_branch_exists(), "the server branch stays without the option");

    for expected in ["#7", "entrou pelas mãos de outra pessoa", "gravada como entregue", "saiu desta máquina", "Humanize"]
    {
        assert!(context.contains(expected), "{expected}: {context}");
    }
    assert!(!context.contains("segue(m) viva(s)"), "the tidied branch is not listed again as alive: {context}");
    let charges: Value = serde_json::from_str(
        &std::fs::read_to_string(scene.work.join(".claude/pending/charges.json")).expect("the charge is armed"),
    )
    .unwrap();
    assert_eq!(charges["armed"][0]["spec"], json!(SPEC), "{charges}");
    assert_eq!(charges["armed"][0]["session"], json!(SESSION), "{charges}");

    // A próxima sessão não refaz nada: a spec já não está em "pull request
    // aberto", e o provedor nem é perguntado.
    let (_, again) = scene.start(&view("MERGED"), 0);
    assert!(again.is_empty(), "{again:?}");
}

/// Com `git.deleteRemoteBranch` ligada, a branch do servidor sai junto.
#[test]
fn the_server_branch_goes_only_with_the_option_on() {
    let scene = Scene::new(true);
    scene.merged_by_someone_else();
    let (context, _) = scene.start(&view("MERGED"), 0);
    assert_eq!(scene.phase(), "delivered", "{context}");
    assert!(!scene.local_branch_exists());
    assert!(!scene.remote_branch_exists(), "the server branch goes with the option on");
}

/// Quando o provedor não responde, o aviso diz isso e nada muda.
#[test]
fn a_silent_provider_warns_and_changes_nothing() {
    let scene = Scene::new(false);
    scene.merged_by_someone_else();
    let (context, asked) = scene.start(&view("MERGED"), 1);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    assert!(context.contains("o provedor não respondeu"), "{context}");
    assert!(context.contains(SPEC), "{context}");
    assert_eq!(scene.phase(), "pr_open", "nothing was recorded");
    assert!(scene.local_branch_exists(), "the branch stays");
    assert_eq!(git(&scene.work, &["branch", "--show-current"]), BRANCH);
    assert!(!scene.work.join(".claude/pending/charges.json").exists(), "no charge is armed");
}
