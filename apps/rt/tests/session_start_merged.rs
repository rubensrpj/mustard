// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

//! O pull request de uma spec, do `pr-open` ao início da sessão, pelo binário
//! de verdade, num repositório de verdade com um remoto, trinta branches de
//! colegas, e o provedor trocado por um `gh` falso no `PATH` que anota cada
//! pergunta que recebe.
//!
//! A spec parte fechada, e é o `pr-open` que a leva a "pull request aberto",
//! com o número e o endereço: nenhum teste escreve essa fase à mão.
//!
//! - O pull request segue aberto: o provedor é perguntado uma vez só, pelo
//!   pull request da spec, e nada muda.
//! - O pull request entrou pelas mãos de outra pessoa, por merge normal ou por
//!   squash: roda o mesmo caminho do merge do fluxo — a spec gravada como
//!   entregue, a base atualizada, a branch local apagada, a do servidor só com
//!   a opção ligada, e a pergunta das pendências nascidas na spec —, sem uma
//!   pergunta por branch de colega. No squash o git não prova o merge, e é a
//!   lista do provedor que decide se a branch sai.
//! - O provedor não responde: o aviso diz isso, e nada muda.
//!
//! E o que o início da sessão já contou não se repete na sessão seguinte, a
//! limpeza alcança a cópia e a branch da spec que o usuário apagou, e uma
//! cópia do projeto recém-feita na pasta temporária não entra no aviso de
//! disco.

#[path = "support/executable.rs"]
mod executable;
#[path = "support/mod.rs"]
mod support;

#[path = "support/binary.rs"]
mod binary;

#[path = "support/one_wave_spec.rs"]
mod one_wave_spec;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;

const SPEC: &str = "entrega";
const BRANCH: &str = "feature/entrega";
const SESSION: &str = "s-entrega";
const COLLEAGUES: usize = 30;

/// O `gh` falso: anota cada chamada, uma por linha, e responde o que o teste
/// mandou — o endereço do pull request criado, a visão do pull request pelo
/// número, a lista dos pull requests da branch, ou a falha. Antes da abertura,
/// a branch não tem pull request nenhum.
const FAKE_GH: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_GH_LOG"
if [ "$FAKE_GH_EXIT" != "0" ]; then
  echo "o provedor está fora do ar" >&2
  exit "$FAKE_GH_EXIT"
fi
case "$1 $2" in
  "pr create") printf 'https://exemplo/pull/7\n' ;;
  "pr view")
    if [ "$3" = "7" ]; then
      printf '%s' "$FAKE_GH_VIEW"
    else
      echo "no pull requests found for branch \"$3\"" >&2
      exit 1
    fi ;;
  "pr list") printf '%s' "${FAKE_GH_LIST:-[]}" ;;
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

/// Um projeto com o fluxo instalado, o checkout na branch da spec em "pull request
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
            "git": {"flow": {"*": "dev"}, "provider": "github", "deleteRemoteBranch": delete_remote,
                // O texto dos pull requests da história do mapa é outra
                // pergunta ao provedor, que este teste não conta.
                "pullRequestText": false}
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

        // A spec fechada, com a base e a branch no estado e o objetivo de onde
        // o título do pull request sai.
        let at = "2026-09-17T08:00:00-03:00";
        let lines = [
            json!({"v":1,"id":1,"at":at,"type":"state","author":"binary","phase":"survey","branch":BRANCH,"base":"dev"}),
            json!({"v":1,"id":2,"at":at,"type":"message","author":"user","text":"Faça a entrega."}),
            json!({"v":1,"id":3,"at":at,"type":"context","author":"assistant","text":"Faça a entrega.","origin":2}),
            json!({"v":1,"id":4,"at":at,"type":"state","author":"binary","phase":"plan"}),
            json!({"v":1,"id":5,"at":at,"type":"state","author":"user","phase":"approved",
                "witness":{"question":"Aprovar esta spec?","answer":"Aprovar"}}),
            json!({"v":1,"id":6,"at":at,"type":"state","author":"binary","phase":"running"}),
            json!({"v":1,"id":7,"at":at,"type":"deferred","author":"assistant","text":"Humanize fica para depois.",
                "keys":["humanize"],"pending":1,"origin":2}),
            json!({"v":1,"id":8,"at":at,"type":"state","author":"binary","phase":"closed"}),
        ];
        let spec_dir = work.join(".claude/spec").join(SPEC);
        std::fs::create_dir_all(&spec_dir).unwrap();
        let body: String = lines.iter().map(|line| line.to_string() + "\n").collect();
        std::fs::write(spec_dir.join("spec.ndjson"), body).unwrap();

        // O `gh` falso.
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        executable::write_executable(&bin.join("gh"), FAKE_GH);

        let scene = Self { dir, work };
        assert_eq!(scene.phase(), "closed", "the spec starts closed");

        // O `pr-open` de verdade leva a spec a "pull request aberto".
        let (opened, asked) = scene.pr_open();
        assert_eq!(opened["ok"], json!(true), "{opened}");
        assert_eq!(opened["action"], json!("open"), "{opened}");
        assert_eq!(opened["number"], json!(7), "{opened}");
        assert!(asked.iter().any(|call| call.starts_with("pr create ")), "{asked:?}");
        assert_eq!(scene.phase(), "pr_open", "the pr-open records the open pull request");
        let state = scene.last_state();
        assert_eq!(state["author"], json!("binary"), "{state}");
        assert_eq!(state["pr"], json!({"number": 7, "url": "https://exemplo/pull/7"}), "{state}");
        assert!(
            !scene.work.join(".claude/pending/charges.json").exists(),
            "opening the pull request arms no charge"
        );
        scene.install_the_map();
        scene
    }

    /// O mapa que a instalação deixa no projeto: sem ele, o primeiro início de
    /// sessão o criaria antes de qualquer aviso, e o que essa criação fizesse
    /// entraria na conta deste teste. O que o provedor falso receber aqui não
    /// conta.
    fn install_the_map(&self) {
        let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "map", "search", "--query", "entrega", "--root"])
            .arg(&self.work)
            .current_dir(&self.work)
            .env("PATH", self.path())
            .env("FAKE_GH_LOG", self.dir.path().join("gh-map.log"))
            .env("FAKE_GH_EXIT", "1")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .output()
            .expect("run mustard-rt");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(
            mustard_core::io::project_map::model_path(&self.work).is_file(),
            "the map is on disk, as the install leaves it"
        );
    }

    /// Roda o `pr-open` da spec pelo binário, com o `gh` falso. Devolve a
    /// resposta e as perguntas que o provedor recebeu.
    fn pr_open(&self) -> (Value, Vec<String>) {
        let log = self.dir.path().join(format!("gh-open-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "pr-open", "--base", "dev", "--head", BRANCH, "--spec", SPEC, "--root"])
            .arg(&self.work)
            .current_dir(&self.work)
            .env("PATH", self.path())
            .env("FAKE_GH_LOG", &log)
            .env("FAKE_GH_EXIT", "0")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env_remove("MUSTARD_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .output()
            .expect("run mustard-rt");
        let report = serde_json::from_slice(&out.stdout).expect("JSON");
        let asked = std::fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_string).collect();
        (report, asked)
    }

    /// O `PATH` com o `gh` falso na frente.
    fn path(&self) -> String {
        format!("{}:{}", self.dir.path().join("bin").display(), std::env::var("PATH").unwrap_or_default())
    }

    /// Outra pessoa faz o merge da branch da spec na base, no remoto: um
    /// merge normal, ou um squash, que o git não reconhece como merge da
    /// branch.
    fn merged_by_someone_else(&self, squash: bool) {
        let colleague = self.dir.path().join("colega");
        let origin = self.dir.path().join("origin.git");
        git(self.dir.path(), &["clone", "-q", "-b", "dev", origin.to_str().unwrap(), colleague.to_str().unwrap()]);
        if squash {
            git(&colleague, &["merge", "-q", "--squash", &format!("origin/{BRANCH}")]);
            git(&colleague, &["commit", "-q", "-m", "A entrega (#7)"]);
        } else {
            git(&colleague, &["merge", "-q", "--no-ff", "-m", "Merge do PR #7", &format!("origin/{BRANCH}")]);
        }
        git(&colleague, &["push", "-q", "origin", "dev"]);
    }

    /// A lista que o provedor dá da branch da spec depois do merge: o pull
    /// request mergeado, com a cabeça que ele levou.
    fn merged_list(&self) -> Value {
        json!([{"state": "MERGED", "headRefOid": git(&self.work, &["rev-parse", BRANCH])}])
    }

    /// O início da sessão na retomada da conversa, com o `gh` falso
    /// respondendo `view` a `gh pr view` e uma lista vazia a `gh pr list`, ou
    /// falhando com `exit`. Devolve o texto que ele pôs na sessão e as
    /// perguntas que o provedor recebeu.
    fn start(&self, view: &Value, exit: i32) -> (String, Vec<String>) {
        self.start_listing(view, &json!([]), exit)
    }

    /// [`Scene::start`], com a lista que o `gh` falso dá a `gh pr list`.
    fn start_listing(&self, view: &Value, list: &Value, exit: i32) -> (String, Vec<String>) {
        self.start_from("resume", view, list, exit)
    }

    /// [`Scene::start_listing`], com a origem `source` do início da sessão.
    fn start_from(&self, source: &str, view: &Value, list: &Value, exit: i32) -> (String, Vec<String>) {
        let log = self.dir.path().join(format!("gh-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let input = json!({
            "hook_event_name": "SessionStart",
            "session_id": SESSION,
            "source": source,
            "cwd": self.work.to_str().unwrap(),
        });
        let mut child = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["on", "SessionStart"])
            .current_dir(&self.work)
            .env("PATH", self.path())
            .env("FAKE_GH_LOG", &log)
            .env("FAKE_GH_VIEW", view.to_string())
            .env("FAKE_GH_LIST", list.to_string())
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

    /// O último `state` do arquivo de eventos.
    fn last_state(&self) -> Value {
        let body = std::fs::read_to_string(self.work.join(".claude/spec").join(SPEC).join("spec.ndjson")).unwrap();
        body.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .rfind(|event| event["type"] == json!("state"))
            .unwrap_or_default()
    }

    /// A fase da spec, pelo último `state` do arquivo de eventos.
    fn phase(&self) -> String {
        self.last_state()["phase"].as_str().map(str::to_string).unwrap_or_default()
    }

    fn local_branch_exists(&self) -> bool {
        !git(&self.work, &["branch", "--list", BRANCH]).is_empty()
    }

    fn remote_branch_exists(&self) -> bool {
        !git(&self.work, &["ls-remote", "--heads", "origin", BRANCH]).is_empty()
    }
}

fn rt(root: &Path, args: &[&str]) -> Value {
    let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
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
/// o mesmo caminho do merge do fluxo: a spec gravada como entregue, a base
/// atualizada, a branch local apagada e a do servidor mantida, e a pergunta
/// das pendências nascidas na spec armada e dita no texto. O provedor não é
/// perguntado por nenhuma branch de colega.
#[test]
fn a_pull_request_merged_by_someone_else_is_delivered_through_the_merge_path() {
    let scene = Scene::new(false);
    // Uma ref guardada cujo código a obra levou à base, e outra cujo código
    // a base não tem: só a primeira sai com a entrega.
    let head = git(&scene.work, &["rev-parse", BRANCH]);
    let landed_commit = git(&scene.work, &["commit-tree", &format!("{head}^{{tree}}"), "-p", &format!("{head}^"), "-m", "guardado"]);
    let landed = format!("refs/mustard/kept/{SPEC}/1-2-landed");
    git(&scene.work, &["update-ref", &landed, &landed_commit]);
    let tree = git(&scene.work, &["rev-parse", "dev^{tree}"]);
    let lost_commit = git(&scene.work, &["commit-tree", &tree, "-p", &head, "-m", "guardado"]);
    let lost = format!("refs/mustard/kept/{SPEC}/2-3-lost");
    git(&scene.work, &["update-ref", &lost, &lost_commit]);
    scene.merged_by_someone_else(false);
    let (context, asked) = scene.start(&view("MERGED"), 0);
    delivered_through_the_merge_path(&scene, &context, &asked);
    assert_eq!(git(&scene.work, &["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]), lost);
}

/// Com o pull request feito por squash, o git não reconhece a branch como
/// mergeada, e a lista do provedor decide: com o pull request mergeado na
/// lista, a branch sai pelo mesmo caminho do merge normal.
#[test]
fn a_squashed_pull_request_is_delivered_when_the_provider_lists_it_merged() {
    let scene = Scene::new(false);
    scene.merged_by_someone_else(true);
    assert!(
        git(&scene.work, &["branch", "--merged", "origin/dev", "--list", BRANCH]).is_empty(),
        "git alone does not prove a squash"
    );
    let (context, asked) = scene.start_listing(&view("MERGED"), &scene.merged_list(), 0);
    delivered_through_the_merge_path(&scene, &context, &asked);
}

/// Com o squash e a lista do provedor sem o pull request mergeado, nada prova
/// o merge da branch: a spec é gravada como entregue, porque o pull request
/// entrou, mas a branch local fica, e o texto diz por quê.
#[test]
fn a_squash_the_provider_does_not_list_keeps_the_branch() {
    let scene = Scene::new(false);
    scene.merged_by_someone_else(true);
    let (context, _) = scene.start_listing(&view("MERGED"), &json!([]), 0);
    assert_eq!(scene.phase(), "delivered", "{context}");
    assert!(scene.local_branch_exists(), "without the provider's word the branch stays: {context}");
    assert!(context.contains("ficou nesta máquina"), "{context}");
}

/// O que o caminho do merge deixa, pelo merge normal ou pelo squash.
fn delivered_through_the_merge_path(scene: &Scene, context: &str, asked: &[String]) {

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
    scene.merged_by_someone_else(false);
    let (context, _) = scene.start(&view("MERGED"), 0);
    assert_eq!(scene.phase(), "delivered", "{context}");
    assert!(!scene.local_branch_exists());
    assert!(!scene.remote_branch_exists(), "the server branch goes with the option on");
}

/// Depois de `/clear` e da compactação, o início da sessão só retoma: o
/// provedor não é perguntado, nem com o pull request já mergeado, e nada muda.
/// A retomada da conversa pergunta e entrega.
#[test]
fn the_provider_is_not_asked_after_clear_or_compact() {
    let scene = Scene::new(false);
    scene.merged_by_someone_else(false);
    for source in ["clear", "compact"] {
        let (context, asked) = scene.start_from(source, &view("MERGED"), &json!([]), 0);
        assert!(asked.is_empty(), "{source}: no question to the provider: {asked:#?}");
        assert_eq!(scene.phase(), "pr_open", "{source}: nothing was recorded: {context}");
    }
    let (_, asked) = scene.start(&view("MERGED"), 0);
    assert!(asked[0].starts_with("pr view 7 "), "resume asks about the spec's pull request: {asked:?}");
    assert_eq!(scene.phase(), "delivered");
}

/// Quando o provedor não responde, o aviso diz isso e nada muda.
#[test]
fn a_silent_provider_warns_and_changes_nothing() {
    let scene = Scene::new(false);
    scene.merged_by_someone_else(false);
    let (context, asked) = scene.start(&view("MERGED"), 1);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    assert!(context.contains("o provedor não respondeu"), "{context}");
    assert!(context.contains(SPEC), "{context}");
    assert_eq!(scene.phase(), "pr_open", "nothing was recorded");
    assert!(scene.local_branch_exists(), "the branch stays");
    assert_eq!(git(&scene.work, &["branch", "--show-current"]), BRANCH);
    assert!(!scene.work.join(".claude/pending/charges.json").exists(), "no charge is armed");
}

// ---------------------------------------------------------------------------
// A spec juntada na base fora do fechamento
// ---------------------------------------------------------------------------

/// A pasta das cópias que o teste e o binário dividem: a dos testes, nunca a
/// de quem programa.
fn test_copies_base() -> String {
    std::env::var("MUSTARD_COPIES_DIR").expect("the tests point the copies at their own folder")
}

/// Um projeto com o fluxo instalado e o remoto, sem pull request nenhum: a obra das
/// specs entra na base pelas mãos de um colega, sem passar pelo fechamento.
struct Outside {
    dir: TempDir,
    work: PathBuf,
}

impl Outside {
    fn new() -> Self {
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
            "git": {"flow": {"*": "dev"}, "provider": "github", "pullRequestText": false}
        });
        std::fs::write(work.join("mustard.json"), config.to_string()).unwrap();
        std::fs::write(work.join("README.md"), "loja\n").unwrap();
        git(&work, &["add", "README.md"]);
        git(&work, &["commit", "-q", "-m", "semente"]);
        git(&work, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&work, &["push", "-q", "-u", "origin", "dev"]);
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        executable::write_executable(&bin.join("gh"), FAKE_GH);
        let scene = Self { dir, work };
        support::copies_leave_with_the_test(&scene.work);
        scene
    }

    fn spec_file(&self, spec: &str) -> PathBuf {
        self.work.join(".claude/spec").join(spec).join("spec.ndjson")
    }

    /// A spec `spec` aprovada e em execução, na branch `branch` cortada da
    /// base agora; o checkout fica nela.
    fn running_spec(&self, spec: &str, branch: &str) {
        git(&self.work, &["checkout", "-q", "-b", branch, "dev"]);
        let at = "2026-10-17T08:00:00-03:00";
        let lines = [
            json!({"v":1,"id":1,"at":at,"type":"state","author":"binary","phase":"survey","branch":branch,"base":"dev"}),
            json!({"v":1,"id":2,"at":at,"type":"message","author":"user","text":"Faça a entrega."}),
            json!({"v":1,"id":3,"at":at,"type":"context","author":"assistant","text":"Faça a entrega.","origin":2}),
            json!({"v":1,"id":4,"at":at,"type":"state","author":"binary","phase":"plan"}),
            json!({"v":1,"id":5,"at":at,"type":"state","author":"user","phase":"approved",
                "witness":{"question":"Aprovar esta spec?","answer":"Aprovar"}}),
            json!({"v":1,"id":6,"at":at,"type":"state","author":"binary","phase":"running"}),
        ];
        let file = self.spec_file(spec);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let body: String = lines.iter().map(|line| line.to_string() + "\n").collect();
        std::fs::write(file, body).unwrap();
    }

    /// Um commit da rodada na branch do checkout, enviado ao remoto e gravado
    /// na spec `spec`, como a rodada grava. Devolve o commit.
    fn round_commit(&self, spec: &str, file: &str, text: &str) -> String {
        std::fs::write(self.work.join(file), text).unwrap();
        git(&self.work, &["add", file]);
        git(&self.work, &["commit", "-q", "-m", &format!("fix: {file}")]);
        let sha = git(&self.work, &["rev-parse", "HEAD"]);
        let branch = git(&self.work, &["branch", "--show-current"]);
        git(&self.work, &["push", "-q", "-u", "origin", &branch]);
        self.append(spec, json!({"type":"commit","author":"binary",
            "sha":sha,"title":format!("fix: {file}"),"waves":[1],"files":[file],"repo":"work"}));
        sha
    }

    /// Acrescenta o evento `event` ao fim do arquivo de eventos da spec
    /// `spec`, com o número seguinte.
    fn append(&self, spec: &str, mut event: Value) {
        let path = self.spec_file(spec);
        let body = std::fs::read_to_string(&path).unwrap();
        event["v"] = json!(1);
        event["id"] = json!(body.lines().count() + 1);
        event["at"] = json!("2026-10-17T09:00:00-03:00");
        std::fs::write(&path, body + &event.to_string() + "\n").unwrap();
    }

    /// O índice das specs refeito, como cada gravação o deixa, e o mapa que a
    /// instalação deixa no projeto.
    fn installed(&self) {
        let index = rt(&self.work, &["index"]);
        assert_eq!(index["ok"], json!(true), "{index}");
        let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "map", "search", "--query", "entrega", "--root"])
            .arg(&self.work)
            .current_dir(&self.work)
            .env("PATH", self.path())
            .env("FAKE_GH_LOG", self.dir.path().join("gh-map.log"))
            .env("FAKE_GH_EXIT", "1")
            .env("MUSTARD_COPIES_DIR", test_copies_base())
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .output()
            .expect("run mustard-rt");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// Uma vaga da obra `spec` na pasta das cópias dos testes, no commit
    /// `at`, com um rascunho que nenhum commit tem.
    fn slot_with_a_draft(&self, spec: &str, at: &str) -> PathBuf {
        let slot = mustard_core::io::wave_prompt::slot_path(&self.work, spec, 0);
        assert!(slot.starts_with(test_copies_base()), "the slot lives in the tests' copies folder: {}", slot.display());
        std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
        git(&self.work, &["worktree", "add", "-q", "--detach", slot.to_str().unwrap(), at]);
        std::fs::write(slot.join("rascunho.txt"), "rascunho da onda\n").unwrap();
        slot
    }

    /// Um colega junta a branch `branch` na base, no remoto, por merge com
    /// commit de merge ou por avanço rápido; este checkout busca o remoto.
    fn merged_by_a_colleague(&self, branch: &str, fast_forward: bool) {
        let colleague = self.dir.path().join(format!("colega-{}", self.dir.path().read_dir().unwrap().count()));
        let origin = self.dir.path().join("origin.git");
        git(self.dir.path(), &["clone", "-q", "-b", "dev", origin.to_str().unwrap(), colleague.to_str().unwrap()]);
        let theirs = format!("origin/{branch}");
        if fast_forward {
            git(&colleague, &["merge", "-q", "--ff-only", &theirs]);
        } else {
            git(&colleague, &["merge", "-q", "--no-ff", "-m", &format!("Merge da {branch}"), &theirs]);
        }
        git(&colleague, &["push", "-q", "origin", "dev"]);
        git(&self.work, &["fetch", "-q", "origin"]);
    }

    /// Um colega junta a branch `branch` na base como o GitHub faz no squash
    /// (`squash`) ou no rebase: commits novos na base, com o conteúdo da
    /// branch, e nenhum commit dela entre os antepassados da base. A base anda
    /// antes, com um commit do colega, para o rebase nunca refazer os mesmos
    /// commits. Este checkout busca o remoto.
    fn rewritten_by_a_colleague(&self, branch: &str, squash: bool) {
        let colleague = self.dir.path().join(format!("colega-{}", self.dir.path().read_dir().unwrap().count()));
        let origin = self.dir.path().join("origin.git");
        git(self.dir.path(), &["clone", "-q", "-b", "dev", origin.to_str().unwrap(), colleague.to_str().unwrap()]);
        std::fs::write(colleague.join("colega.txt"), "do colega\n").unwrap();
        git(&colleague, &["add", "colega.txt"]);
        git(&colleague, &["commit", "-q", "-m", "do colega"]);
        let theirs = format!("origin/{branch}");
        if squash {
            git(&colleague, &["merge", "-q", "--squash", &theirs]);
            git(&colleague, &["commit", "-q", "-m", &format!("Squash da {branch}")]);
        } else {
            let fork = git(&colleague, &["merge-base", "dev", &theirs]);
            git(&colleague, &["cherry-pick", &format!("{fork}..{theirs}")]);
        }
        git(&colleague, &["push", "-q", "origin", "dev"]);
        git(&self.work, &["fetch", "-q", "origin"]);
    }

    /// `mustard-rt run deliver-merged` da spec `spec`, pedido à mão: a
    /// resposta dele.
    fn deliver_by_hand(&self, spec: &str) -> Value {
        let out = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["run", "deliver-merged", "--spec", spec, "--root"])
            .arg(&self.work)
            .current_dir(&self.work)
            .env("MUSTARD_COPIES_DIR", test_copies_base())
            .output()
            .expect("run mustard-rt");
        serde_json::from_slice(&out.stdout).expect("JSON")
    }

    fn path(&self) -> String {
        format!("{}:{}", self.dir.path().join("bin").display(), std::env::var("PATH").unwrap_or_default())
    }

    /// O início da sessão com a origem `source` (`startup` na abertura,
    /// `clear` depois de `/clear`). Devolve o texto que ele pôs na sessão e as
    /// perguntas que o provedor recebeu.
    fn start(&self, source: &str) -> (String, Vec<String>) {
        let log = self.dir.path().join(format!("gh-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let input = json!({
            "hook_event_name": "SessionStart",
            "session_id": SESSION,
            "source": source,
            "cwd": self.work.to_str().unwrap(),
        });
        let mut child = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(["on", "SessionStart"])
            .current_dir(&self.work)
            .env("PATH", self.path())
            .env("FAKE_GH_LOG", &log)
            .env("FAKE_GH_VIEW", "{}")
            .env("FAKE_GH_LIST", "[]")
            .env("FAKE_GH_EXIT", "0")
            .env("MUSTARD_COPIES_DIR", test_copies_base())
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

    /// A fase da spec `spec`, pelo último `state` do arquivo de eventos.
    fn phase_of(&self, spec: &str) -> String {
        let body = std::fs::read_to_string(self.spec_file(spec)).unwrap();
        body.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .rfind(|event| event["type"] == json!("state"))
            .and_then(|event| event["phase"].as_str().map(str::to_string))
            .unwrap_or_default()
    }

    /// Espera o processo em segundo plano da spec `spec` terminar: ele
    /// acrescenta ao relatório uma linha só, no fim.
    fn background_report(&self, spec: &str) -> Value {
        let report = mustard_core::io::wave_prompt::copies_dir(&self.work).join(format!("{spec}.deliver-merged.log"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let text = std::fs::read_to_string(&report).unwrap_or_default();
            if let Some(done) = text.lines().rev().find_map(|line| serde_json::from_str::<Value>(line).ok()) {
                return done;
            }
            assert!(std::time::Instant::now() < deadline, "the background delivery never finished: {text}");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// A spec em execução cuja branch um colega juntou na base com commit de
/// merge: a abertura da sessão vê só pelo git, sem perguntar ao provedor, e a
/// cópia sai em segundo plano, guardando antes o que nenhum commit tem; só
/// depois a spec é gravada como entregue. O processo em segundo plano não
/// troca o ramo do checkout nem apaga a branch. A spec entregue não volta a
/// ser entregue, e a recusa diz a fase pelo nome.
#[test]
fn a_running_spec_merged_outside_frees_its_copies_then_is_delivered() {
    let scene = Outside::new();
    scene.running_spec(SPEC, BRANCH);
    let sha = scene.round_commit(SPEC, "entrega.txt", "a entrega\n");
    scene.installed();
    let slot = scene.slot_with_a_draft(SPEC, &sha);
    // A ref guardada antes, cujo código a obra levou à base: sai com a entrega.
    let landed_commit = git(&scene.work, &["commit-tree", &format!("{sha}^{{tree}}"), "-p", &format!("{sha}^"), "-m", "guardado"]);
    let landed = format!("refs/mustard/kept/{SPEC}/1-2-landed");
    git(&scene.work, &["update-ref", &landed, &landed_commit]);
    scene.merged_by_a_colleague(BRANCH, false);

    let (context, asked) = scene.start("startup");
    for expected in [SPEC, BRANCH, "entrou na base fora do fechamento", "sem a revisão final", "A cópia da obra sai em segundo plano"] {
        assert!(context.contains(expected), "{expected}: {context}");
    }
    let report = scene.background_report(SPEC);
    assert_eq!(report["ok"], json!(true), "{report}");
    assert_eq!(scene.phase_of(SPEC), "delivered", "{report}");
    let summary: Vec<String> = mustard_core::io::project_summary::read(&scene.work).into_iter().map(|entry| entry.spec).collect();
    assert_eq!(summary, vec![SPEC.to_string()], "the delivery outside also leaves the summary line: {report}");
    assert!(!slot.exists(), "the slot left the disk: {report}");
    let leaving = mustard_core::io::wave_prompt::copies_dir(&scene.work).join(".leaving");
    let still: Vec<_> = std::fs::read_dir(&leaving).map(|entries| entries.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    assert!(still.is_empty(), "the copy was deleted after the lock, not left aside: {still:?}");

    let kept = git(&scene.work, &["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]);
    assert!(!kept.contains(&landed), "the kept ref whose code the base has left with the delivery: {kept}");
    assert_eq!(report["kept_removed"], json!([landed]), "{report}");
    assert!(kept.starts_with(&format!("refs/mustard/kept/{SPEC}/")), "the draft was kept before the slot left: {kept}");
    assert_eq!(git(&scene.work, &["show", &format!("{kept}:rascunho.txt")]), "rascunho da onda");
    assert!(asked.is_empty(), "git alone proves the merge: {asked:#?}");
    assert_eq!(git(&scene.work, &["branch", "--show-current"]), BRANCH, "the checkout stays where it was: {report}");
    assert!(!git(&scene.work, &["branch", "--list", BRANCH]).is_empty(), "the branch stays: {report}");

    let (again, _) = scene.start("startup");
    assert!(!again.contains("fora do fechamento"), "a delivered spec is not told again: {again}");
    let refused = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "deliver-merged", "--spec", SPEC, "--root"])
        .arg(&scene.work)
        .current_dir(&scene.work)
        .env("MUSTARD_COPIES_DIR", test_copies_base())
        .output()
        .expect("run mustard-rt");
    let refused: Value = serde_json::from_slice(&refused.stdout).expect("JSON");
    assert_eq!(refused["reason"], json!("spec-not-open"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("entregue") && !hint.contains("delivered"), "the phase is said by its name: {hint}");
}

/// A spec de antes desta versão fica fora da conferência do
/// início da sessão: juntada na base por um colega, ela não é entregue nem
/// citada.
#[test]
fn a_legacy_spec_merged_outside_is_left_alone() {
    let scene = Outside::new();
    scene.running_spec(SPEC, BRANCH);
    scene.round_commit(SPEC, "entrega.txt", "a entrega\n");
    scene.installed();
    mustard_core::io::legacy_specs::mark_once(&scene.work, "0.2.24").expect("the list");
    scene.merged_by_a_colleague(BRANCH, false);

    let (context, _) = scene.start("startup");
    assert!(!context.contains("fora do fechamento"), "{context}");
    assert_eq!(scene.phase_of(SPEC), "running", "{context}");
}

/// Com uma onda despachada e sem volta, a obra não está pronta, mesmo com a
/// branch já juntada na base: nada é entregue e a cópia da onda fica.
#[test]
fn a_spec_merged_outside_with_a_wave_in_progress_keeps_its_copies() {
    let scene = Outside::new();
    scene.running_spec(SPEC, BRANCH);
    let sha = scene.round_commit(SPEC, "entrega.txt", "a entrega\n");
    scene.installed();
    let slot = scene.slot_with_a_draft(SPEC, &sha);
    scene.append(SPEC, json!({"type":"wave","author":"assistant","n":2,"title":"o resto"}));
    scene.append(SPEC, json!({"type":"task","author":"assistant","wave":2,"title":"o resto","files":["resto.txt"]}));
    scene.append(SPEC, json!({"type":"send","author":"binary","wave":2,"copy":slot.to_str().unwrap()}));
    scene.merged_by_a_colleague(BRANCH, false);

    let (context, asked) = scene.start("startup");
    assert_eq!(scene.phase_of(SPEC), "running", "{context}");
    assert!(slot.join("rascunho.txt").is_file(), "the wave's copy stays: {context}");
    assert!(!context.contains("fora do fechamento"), "{context}");
    assert!(asked.is_empty(), "{asked:#?}");
}

/// A spec atual em execução, juntada na base, não é entregue depois de
/// `/clear` nem da compactação: a conferência das fases em execução e
/// fechada é só da abertura da sessão e da retomada da conversa.
#[test]
fn a_running_spec_merged_outside_is_not_delivered_on_compaction() {
    let scene = Outside::new();
    scene.running_spec(SPEC, BRANCH);
    let sha = scene.round_commit(SPEC, "entrega.txt", "a entrega\n");
    scene.installed();
    let slot = scene.slot_with_a_draft(SPEC, &sha);
    scene.merged_by_a_colleague(BRANCH, false);

    for source in ["compact", "clear"] {
        let (context, _) = scene.start(source);
        assert_eq!(scene.phase_of(SPEC), "running", "{source}: {context}");
        assert!(!context.contains("fora do fechamento"), "{source}: {context}");
        assert!(slot.join("rascunho.txt").is_file(), "{source}: the copy stays");
    }
}

/// A retomada da conversa varre como a abertura: a spec em execução juntada
/// na base por um colega é entregue pelo git, sem perguntar ao provedor.
#[test]
fn a_running_spec_merged_outside_is_delivered_on_resume() {
    let scene = Outside::new();
    scene.running_spec(SPEC, BRANCH);
    scene.round_commit(SPEC, "entrega.txt", "a entrega\n");
    scene.installed();
    scene.merged_by_a_colleague(BRANCH, false);

    let (context, asked) = scene.start("resume");
    assert!(context.contains("entrou na base fora do fechamento"), "{context}");
    assert_eq!(scene.phase_of(SPEC), "delivered", "{context}");
    assert!(asked.is_empty(), "git alone proves the merge: {asked:#?}");
}

/// A spec fechada já passou pelo fechamento e pela revisão final: o aviso da
/// obra juntada fora da porta não diz que ela ficou sem revisão.
#[test]
fn a_closed_spec_merged_outside_is_not_told_it_skipped_the_final_review() {
    let scene = Outside::new();
    scene.running_spec(SPEC, BRANCH);
    scene.round_commit(SPEC, "entrega.txt", "a entrega\n");
    scene.append(SPEC, json!({"type":"state","author":"binary","phase":"closed"}));
    git(&scene.work, &["checkout", "-q", "dev"]);
    scene.installed();
    scene.merged_by_a_colleague(BRANCH, false);

    let (context, _) = scene.start("startup");
    assert_eq!(scene.phase_of(SPEC), "delivered", "{context}");
    assert!(context.contains("já fechada"), "{context}");
    assert!(!context.contains("sem a revisão final"), "{context}");
}

/// A varredura das outras specs, nunca depois de `/clear` e só pelo git: o avanço
/// rápido no meio da obra, a branch que andou depois do merge e a branch
/// recém-cortada não entregam nada; depois do merge de verdade e com a branch
/// fora daqui e do servidor, a spec é gravada como entregue pelo último
/// commit que ela gravou.
#[test]
fn a_spec_merged_outside_is_delivered_from_another_branch_only_through_a_real_merge() {
    let scene = Outside::new();
    scene.running_spec(SPEC, BRANCH);
    scene.round_commit(SPEC, "entrega.txt", "parte um\n");
    git(&scene.work, &["checkout", "-q", "dev"]);
    scene.running_spec("recem", "feature/recem");
    git(&scene.work, &["checkout", "-q", "dev"]);
    scene.installed();

    // A obra entra na base por avanço rápido, no meio do trabalho.
    scene.merged_by_a_colleague(BRANCH, true);
    let (context, _) = scene.start("startup");
    assert_eq!(scene.phase_of(SPEC), "running", "a fast-forward is not a delivery: {context}");

    // O merge de verdade, e um commit à mão na branch que ele não leva.
    git(&scene.work, &["checkout", "-q", BRANCH]);
    git(&scene.work, &["merge", "-q", "--ff-only", "origin/dev"]);
    scene.round_commit(SPEC, "entrega.txt", "parte dois\n");
    scene.merged_by_a_colleague(BRANCH, false);
    std::fs::write(scene.work.join("depois.txt"), "depois do merge\n").unwrap();
    git(&scene.work, &["add", "depois.txt"]);
    git(&scene.work, &["commit", "-q", "-m", "depois do merge"]);
    git(&scene.work, &["checkout", "-q", "dev"]);
    let (context, _) = scene.start("startup");
    assert_eq!(scene.phase_of(SPEC), "running", "a branch that moved past the merge is not delivered: {context}");

    // A branch sai daqui e do servidor; depois de `/clear` nada é varrido.
    git(&scene.work, &["branch", "-q", "-D", BRANCH]);
    git(&scene.work, &["push", "-q", "origin", "--delete", BRANCH]);
    git(&scene.work, &["fetch", "-q", "--prune", "origin"]);
    let (cleared, _) = scene.start("clear");
    assert_eq!(scene.phase_of(SPEC), "running", "the sweep never runs after /clear: {cleared}");

    let (context, asked) = scene.start("startup");
    assert_eq!(scene.phase_of(SPEC), "delivered", "{context}");
    for expected in [SPEC, "entrou na base fora do fechamento", "sem a revisão final", "já tinha saído"] {
        assert!(context.contains(expected), "{expected}: {context}");
    }
    assert!(asked.is_empty(), "nothing is asked of the provider: {asked:#?}");
    assert_eq!(scene.phase_of("recem"), "running", "a branch cut a moment ago delivered nothing");
}

/// A obra juntada na base com tarefa que ainda não saiu — no backlog, sem
/// onda, ou numa onda do plano que nunca foi enviada — não é entregue: a
/// spec segue em execução, e a entrega pedida à mão recusa dizendo quantas
/// tarefas faltam.
#[test]
fn a_spec_merged_outside_with_tasks_left_is_not_delivered() {
    let scene = Outside::new();
    scene.running_spec(SPEC, BRANCH);
    scene.round_commit(SPEC, "entrega.txt", "a entrega\n");
    scene.append(SPEC, json!({"type":"task","author":"assistant","title":"o resto","files":["resto.txt"]}));
    scene.append(SPEC, json!({"type":"wave","author":"assistant","n":2,"title":"a outra"}));
    scene.append(SPEC, json!({"type":"task","author":"assistant","wave":2,"title":"a outra","files":["outra.txt"]}));
    scene.installed();
    scene.merged_by_a_colleague(BRANCH, false);

    let (context, _) = scene.start("startup");
    assert_eq!(scene.phase_of(SPEC), "running", "{context}");
    assert!(!context.contains("fora do fechamento"), "{context}");
    assert!(
        context.contains(SPEC) && context.contains("2 tarefa(s)") && context.contains("não foi gravada como entregue"),
        "the session start warns that the work went in with tasks left: {context}"
    );

    let refused = scene.deliver_by_hand(SPEC);
    assert_eq!(refused["reason"], json!("backlog-left"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains(SPEC) && hint.contains('2'), "the refusal names the spec and the tasks left: {hint}");
    assert_eq!(scene.phase_of(SPEC), "running", "{refused}");
}

/// O squash e o rebase do GitHub põem na base commits novos, e o commit que
/// a rodada gravou nunca vira antepassado dela: a obra entra pelo conteúdo.
/// A abertura da sessão vê, só pelo git, que o que a branch mudou já está na
/// base e grava a entrega; a branch que andou depois do squash segue em obra.
#[test]
fn a_spec_squashed_or_rebased_outside_is_delivered_by_its_content() {
    for squash in [true, false] {
        let scene = Outside::new();
        scene.running_spec(SPEC, BRANCH);
        scene.round_commit(SPEC, "entrega.txt", "parte um\n");
        scene.round_commit(SPEC, "outra.txt", "parte dois\n");
        scene.installed();
        scene.rewritten_by_a_colleague(BRANCH, squash);

        if squash {
            std::fs::write(scene.work.join("depois.txt"), "depois do squash\n").unwrap();
            git(&scene.work, &["add", "depois.txt"]);
            git(&scene.work, &["commit", "-q", "-m", "depois do squash"]);
            let (context, _) = scene.start("startup");
            assert_eq!(scene.phase_of(SPEC), "running", "a branch that moved past the squash is not delivered: {context}");
            git(&scene.work, &["reset", "-q", "--hard", "HEAD~1"]);
        }

        let (context, asked) = scene.start("startup");
        assert_eq!(scene.phase_of(SPEC), "delivered", "squash={squash}: {context}");
        assert!(context.contains("entrou na base fora do fechamento"), "squash={squash}: {context}");
        assert!(asked.is_empty(), "git alone proves it: {asked:#?}");
    }
}

/// A spec da obra que o usuário apaga do disco no meio do caminho.
const DELETED: &str = "apagada";

/// A branch que o projeto protege em `git.protected`, com registro de obra e
/// já na base.
const PROTECTED: &str = "feature/protegida";

/// A branch de `branch` existe no repositório do projeto `root`.
fn has_branch(root: &Path, branch: &str) -> bool {
    !git(root, &["branch", "--list", branch]).is_empty()
}

/// Corta `branch` de `dev` com um commit que versiona o registro de obra de
/// `spec`, e volta para `dev`; com `merge`, a branch entra em `dev` por merge.
fn recorded_branch(root: &Path, branch: &str, spec: &str, merge: bool) {
    git(root, &["checkout", "-q", "-b", branch, "dev"]);
    let dir = root.join(".claude/spec").join(spec);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("nota.md"), format!("{spec}\n")).unwrap();
    git(root, &["add", "-f", &format!(".claude/spec/{spec}")]);
    git(root, &["commit", "-q", "-m", &format!("o registro de {spec}")]);
    git(root, &["checkout", "-q", "dev"]);
    if merge {
        git(root, &["merge", "-q", "--no-ff", "-m", &format!("{branch} entra"), branch]);
    }
}

/// Os nomes das branches de uma parte da resposta da limpeza.
fn branches_of(answer: &Value, part: &str) -> Vec<String> {
    answer["copies"][part]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|record| record["branch"].as_str().or_else(|| record.as_str()).map(str::to_string))
        .collect()
}

/// Uma obra levada pelo fluxo de verdade até a onda 1 sair, com a cópia dela;
/// depois a spec vai versionada na branch, que entra em `dev` por merge, e o
/// usuário apaga a spec do disco. Ao lado: uma branch protegida, com registro
/// e já na base; uma com registro e commit que a base não tem; uma
/// `release/x` sem registro, contida na base. Na pasta das cópias, um
/// programa deixou uma pasta `.claude`.
///
/// No início da sessão, em `dev`, o aviso das pendências e o das branches
/// mergeadas saem na primeira sessão e não na segunda, com a mesma contagem;
/// a lista de pendências que não se lê não faz a contagem ser esquecida; a
/// contagem nova traz o aviso de volta. A limpeza lista a cópia da spec
/// apagada e nunca a `.claude`, e com `--apply` tira a cópia e a branch da
/// spec apagada sem nenhum erro, deixando a `.claude`, a branch protegida, a
/// `release/x`, a branch atual e a com commit fora da base, cada uma com o
/// motivo. Com uma branch mergeada a menos, o aviso delas volta.
#[test]
fn a_start_notice_comes_once_per_count_and_the_clean_reaches_the_deleted_spec() {
    use mustard_core::io::wave_prompt::shown;
    use mustard_core::platform::i18n::{Locale, translate};

    let config = json!({"git": {"flow": {"*": "dev", "dev": "main"}, "protected": [PROTECTED]}});
    let project = one_wave_spec::Project::new(DELETED, &config, &[]);
    project.approved();
    project.run(&["round", "--dispatch", "--spec", DELETED]);
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the wave went out");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy of the wave"));
    let spec_copies = copy.parent().expect("the folder of the spec copies").to_path_buf();
    let copies = spec_copies.parent().expect("the copies folder").to_path_buf();
    let root = project.root.clone();
    let branch = format!("feature/{DELETED}");
    assert!(copy.join(".git").is_file(), "the round made the copy of the wave");
    assert!(has_branch(&root, &branch), "the opening cut the branch of the spec");

    // O que o usuário e outros programas fazem por fora do fluxo.
    git(&root, &["checkout", "-q", &branch]);
    git(&root, &["add", "-f", &format!(".claude/spec/{DELETED}")]);
    git(&root, &["commit", "-q", "-m", "a spec na branch"]);
    git(&root, &["checkout", "-q", "dev"]);
    git(&root, &["merge", "-q", "--no-ff", "-m", "a obra entra", &branch]);
    recorded_branch(&root, PROTECTED, "protegida", true);
    recorded_branch(&root, "feature/sem-base", "sem-base", false);
    git(&root, &["branch", "release/x", "dev"]);
    for spec in [DELETED, "protegida"] {
        std::fs::remove_dir_all(root.join(".claude/spec").join(spec)).unwrap();
    }
    let foreign = copies.join(".claude/mustard/host-observations");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(foreign.join("claude-x.json"), "{}").unwrap();

    let one = translate("pending.count.one", Locale::PtBr).to_string();
    let merged = |count: &str, branches: &str| translate("session.merged", Locale::PtBr).replace("{count}", count).replace("{branches}", branches);
    let both = merged("2", &format!("{branch}, {PROTECTED}"));
    let added = project.run(&["pending", "--add", "--title", "Revisar o texto", "--detail", "combinado na conversa"]);
    assert_eq!(added["id"], json!("P-1"), "{added}");
    let first = project.session_start("s-primeira");
    assert!(first.contains(&one) && first.contains(&both), "the first session gets both counts: {first}");
    let second = project.session_start("s-segunda");
    assert!(!second.contains(&one) && !second.contains(PROTECTED), "the next session, with the same counts, gets neither: {second}");
    let ledger = root.join(".claude/pending/ledger.json");
    let saved = std::fs::read(&ledger).expect("the pending list");
    std::fs::write(&ledger, "{ quebrada").unwrap();
    let unread = project.session_start("s-ilegivel");
    assert!(!unread.contains(&one), "an unreadable list says nothing: {unread}");
    std::fs::write(&ledger, saved).unwrap();
    let back = project.session_start("s-de-volta");
    assert!(!back.contains(&one), "an unreadable list does not forget the count it gave: {back}");
    project.run(&["pending", "--add", "--title", "Trocar o nome", "--detail", "combinado na conversa"]);
    let two = translate("pending.count.many", Locale::PtBr).replace("{count}", "2");
    let third = project.session_start("s-terceira");
    assert!(third.contains(&two) && !third.contains(PROTECTED), "only the new count comes back: {third}");

    git(&root, &["checkout", "-q", "-b", "feature/atual", "dev"]);
    std::fs::create_dir_all(root.join(".claude/spec/atual")).unwrap();
    let listed = project.answer(&["clean"]);
    let records: Vec<&Value> = ["candidates", "kept"]
        .iter()
        .flat_map(|part| listed["copies"][part].as_array().into_iter().flatten())
        .collect();
    assert!(
        records.iter().any(|record| record["path"] == json!(shown(&spec_copies)) && record["reason"] == json!("spec missing")),
        "the copy of the deleted spec leaves: {listed}"
    );
    assert!(records.iter().all(|record| !record["path"].as_str().unwrap_or_default().contains(".claude")), "{listed}");
    assert_eq!(branches_of(&listed, "branches"), vec![branch.clone()], "{listed}");
    let reasons: Vec<(&str, &str)> = listed["copies"]["branches_kept"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|record| (record["branch"].as_str().unwrap_or_default(), record["reason"].as_str().unwrap_or_default()))
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("feature/atual", "spec missing; current branch"),
            (PROTECTED, "spec missing; integration base"),
            ("feature/sem-base", "spec missing; commits outside the base"),
            ("release/x", "spec missing; no unit record"),
        ],
        "{listed}"
    );
    assert!(spec_copies.is_dir() && has_branch(&root, &branch), "without --apply nothing leaves");

    let applied = project.answer(&["clean", "--apply"]);
    assert_eq!(applied["copies"]["errors"], json!([]), "{applied}");
    assert!(!spec_copies.exists(), "the copy of the deleted spec left: {applied}");
    assert!(foreign.join("claude-x.json").is_file(), "the `.claude` folder is not a copy and stays: {applied}");
    assert_eq!(branches_of(&applied, "branches_removed"), vec![branch.clone()], "{applied}");
    assert!(!has_branch(&root, &branch), "the branch of the deleted spec left: {applied}");
    for stays in ["feature/atual", PROTECTED, "feature/sem-base", "release/x", "dev", "main"] {
        assert!(has_branch(&root, stays), "{stays} stays: {applied}");
    }

    git(&root, &["checkout", "-q", "dev"]);
    let after = project.session_start("s-depois");
    assert!(after.contains(&merged("1", PROTECTED)), "one merged branch less brings the notice back: {after}");
}

/// A obra aberta cuja branch um colega juntou na base.
const OPEN_MERGED: &str = "aberta-juntada";

/// A obra aberta cuja branch acabou de ser cortada da base.
const OPEN_JUST_CUT: &str = "aberta-recem";

/// Com duas obras ainda abertas, a limpeza tira, com `--apply`, a branch local
/// da que um colega juntou na base e deixa a da que acabou de ser cortada: a
/// base também contém a branch recém-cortada, mas não há trabalho nela.
#[test]
fn the_clean_removes_the_merged_branch_of_an_open_spec_and_keeps_the_one_just_cut() {
    let project = one_wave_spec::Project::new(OPEN_MERGED, &json!({}), &[]);
    let root = project.root.clone();
    let merged = format!("feature/{OPEN_MERGED}");
    let just_cut = format!("feature/{OPEN_JUST_CUT}");

    // A primeira obra, aberta pelo comando de verdade; o trabalho dela entra
    // na base por um merge de outra pessoa.
    project.run(&["open", "--kind", "feature", "--name", OPEN_MERGED, "--base", "dev"]);
    git(&root, &["checkout", "-q", &merged]);
    std::fs::write(root.join("juntada.txt"), "o trabalho da obra\n").unwrap();
    git(&root, &["add", "juntada.txt"]);
    git(&root, &["commit", "-q", "-m", "o trabalho da obra"]);
    git(&root, &["checkout", "-q", "dev"]);
    git(&root, &["merge", "-q", "--no-ff", "-m", "a obra entra", &merged]);
    // A segunda, aberta depois: cortada da base que já tem a primeira.
    project.run(&["open", "--kind", "feature", "--name", OPEN_JUST_CUT, "--base", "dev"]);
    git(&root, &["checkout", "-q", "dev"]);
    assert!(has_branch(&root, &merged) && has_branch(&root, &just_cut), "both branches exist before the clean");

    let listed = project.answer(&["clean"]);
    assert_eq!(branches_of(&listed, "branches"), vec![merged.clone()], "{listed}");
    assert!(has_branch(&root, &merged), "without --apply nothing leaves");

    let applied = project.answer(&["clean", "--apply"]);
    assert_eq!(applied["copies"]["errors"], json!([]), "{applied}");
    assert_eq!(branches_of(&applied, "branches_removed"), vec![merged.clone()], "{applied}");
    assert!(!has_branch(&root, &merged), "the merged branch of the open spec left: {applied}");
    assert!(has_branch(&root, &just_cut), "the branch just cut stays: {applied}");
    assert!(root.join(".claude/spec").join(OPEN_MERGED).join("spec.ndjson").is_file(), "the clean never touches the open spec");
}

/// O binário com `args`, de dentro de `root`, com a pasta pessoal `home`, a
/// pasta temporária `temp` e o limite do aviso de disco em `limit` bytes
/// (`None` deixa a variável fora do ambiente), sem sessão nem spec forçada.
/// Devolve o que ele imprimiu.
fn with_a_disk_limit(root: &Path, home: &Path, temp: &Path, limit: Option<&str>, args: &[&str], stdin: &str) -> String {
    let mut command = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"));
    command
        .args(args)
        .current_dir(root)
        .env("HOME", home)
        .env("TMPDIR", temp)
        .env("TMP", temp)
        .env("TEMP", temp)
        .env("MUSTARD_COPIES_DIR", test_copies_base())
        .env_remove("MUSTARD_WORKSPACE_ROOT")
        .env_remove("MUSTARD_ACTIVE_SPEC")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match limit {
        Some(bytes) => command.env("MUSTARD_SCRATCH_WARN_BYTES", bytes),
        None => command.env_remove("MUSTARD_SCRATCH_WARN_BYTES"),
    };
    let mut child = command.spawn().expect("spawn mustard-rt");
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().expect("wait mustard-rt");
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// O limite do aviso de disco vem do ambiente do binário: a limpeza mostra,
/// no que imprime, o limite com que o aviso do início da sessão compara o
/// total das sobras. Um número de bytes na variável, alto ou baixo, é o
/// limite; sem a variável, ou com um valor que não é número, vale o padrão de
/// 5 GiB.
#[test]
fn the_clean_shows_the_disk_limit_the_environment_sets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("projeto");
    let home = dir.path().join("casa");
    let temp = dir.path().join("tmp");
    for folder in [&root, &home, &temp] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let limit_with = |value: Option<&str>| -> Value {
        let listed: Value = serde_json::from_str(&with_a_disk_limit(&root, &home, &temp, value, &["run", "clean"], "")).expect("the clean answers JSON");
        listed["warn_bytes"].clone()
    };

    let default = json!(5u64 * 1024 * 1024 * 1024);
    assert_eq!(limit_with(Some("1024")), json!(1024), "a low limit is the number in the variable");
    assert_eq!(limit_with(Some("0")), json!(0), "zero turns the notice on for any leftover");
    assert_eq!(limit_with(Some("107374182400")), json!(107_374_182_400u64), "a high limit is the number in the variable");
    assert_eq!(limit_with(None), default, "without the variable the default stays");
    assert_eq!(limit_with(Some("muito")), default, "a value that is not a number leaves the default");
}

/// Uma cópia do projeto recém-feita na pasta temporária, com as datas de dois
/// dias atrás que o `cp -a` traz da origem, não entra no aviso de disco do
/// início da sessão, nem com o limite do aviso em zero: o binário lê a pasta
/// temporária e o limite do ambiente, e a varredura data a cópia pela mudança
/// do inode, que nenhuma cópia preserva. A limpeza, no mesmo ambiente, mostra
/// que a varredura viu a cópia e a guardou pela idade, e não por não achá-la.
#[test]
fn a_copy_just_made_in_the_temporary_folder_brings_no_disk_notice_even_with_a_zero_limit() {
    use mustard_core::platform::i18n::{Locale, translate};

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("projeto");
    let home = dir.path().join("casa");
    let temp = dir.path().join("tmp");
    for folder in [&root, &home, &temp] {
        std::fs::create_dir_all(folder).unwrap();
    }
    git(&root, &["init", "-q", "-b", "dev"]);
    std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\n").unwrap();
    std::fs::write(root.join("mustard.json"), json!({"language": {"text": "pt-BR"}}).to_string()).unwrap();
    std::fs::write(root.join("README.md"), "loja\n").unwrap();
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-q", "-m", "semente"]);
    support::copies_leave_with_the_test(&root);

    // A cópia, feita agora, com as datas da origem em cada arquivo e pasta.
    let copy = temp.join("tmp.copia");
    std::fs::create_dir_all(copy.join("apps/rt")).unwrap();
    std::fs::write(copy.join("Cargo.toml"), "[workspace]\n").unwrap();
    std::fs::write(copy.join("apps/rt/big.bin"), vec![0u8; 4096]).unwrap();
    let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(48 * 3600);
    for path in [copy.join("apps/rt/big.bin"), copy.join("Cargo.toml"), copy.join("apps/rt"), copy.join("apps"), copy.clone()] {
        std::fs::File::open(&path).unwrap().set_modified(two_days_ago).unwrap();
    }

    let payload = json!({"hook_event_name": "SessionStart", "session_id": "s-disco", "source": "startup", "cwd": root.to_string_lossy()});
    let said = with_a_disk_limit(&root, &home, &temp, Some("0"), &["on", "SessionStart"], &payload.to_string());
    let context = serde_json::from_str::<Value>(said.trim())
        .ok()
        .and_then(|v| v.pointer("/hookSpecificOutput/additionalContext").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    let notice = translate("scratch.residue.notice", Locale::PtBr);
    let opening = notice.split("{total}").next().unwrap_or_default();
    assert!(!opening.is_empty(), "{notice}");
    assert!(!context.contains(opening), "a copy just made is not residue: {context}");

    let listed: Value = serde_json::from_str(&with_a_disk_limit(&root, &home, &temp, Some("0"), &["run", "clean"], "")).expect("the clean answers JSON");
    assert_eq!(listed["candidates"], json!([]), "{listed}");
    let copy_path = copy.display().to_string();
    assert!(
        listed["kept"].as_array().into_iter().flatten().any(|kept| kept["path"] == json!(copy_path) && kept["reason"] == json!("younger than 12h")),
        "the scan saw the copy and kept it for its age: {listed}"
    );
    assert!(copy.join("apps/rt/big.bin").is_file(), "the notice and the list only read");
}
