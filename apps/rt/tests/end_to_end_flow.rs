// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O fluxo inteiro de uma spec de teste, pelo binário de verdade, numa pasta
//! temporária: abrir, levantar, conferir o plano, aprovar pelo clique, duas
//! rodadas, fechar e abrir o pull request.
//!
//! A fala do usuário e o clique chegam pelos ganchos, como chegam numa sessão;
//! o agente da onda e o revisor são as linhas do fim que os textos deles
//! ensinam, e o provedor do pull request é um `gh` falso no começo do `PATH`.
//! No fim, cada passo do fluxo gravou uma chamada só — a aprovação nenhuma — e
//! a pasta da spec tem os três arquivos dela.
//!
//! A mesma spec, num projeto com um submódulo e servidores locais, prova o
//! fluxo de submódulo: a branch de mesmo nome nos dois repositórios, o pull
//! request do submódulo aberto antes e o do principal como rascunho até o do
//! submódulo entrar e o ponteiro ser atualizado — pelo `pr-merge` e pela
//! conferência do início da sessão.
//!
//! O pronto do principal é medido pelo ponteiro que ele tinha na hora, e não
//! pela ordem das chamadas: os casos em que o ponteiro não acontece — commit
//! do submódulo que o merge não levou, envio que o servidor recusa — deixam o
//! principal como rascunho, e duas sessões conferindo ao mesmo tempo movem o
//! ponteiro uma vez só.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

const SPEC: &str = "ponta";
const GOAL: &str = "Trocar a saudação do programa.";
const SESSION: &str = "s-ponta";
/// A branch da spec, a mesma no principal e no submódulo.
const BRANCH: &str = "feature/ponta";
/// O submódulo do projeto de teste e o arquivo dele que a onda muda.
const SUB: &str = "libs/sub";
const SUB_FILE: &str = "libs/sub/lib.txt";

fn git(root: &Path, args: &[&str]) {
    git_out(root, args);
}

/// A saída do git, que precisa responder.
fn git_out(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// O repositório `repo` com o nome e o e-mail de quem comita.
fn identify(repo: &Path) {
    git(repo, &["config", "user.email", "t@example.com"]);
    git(repo, &["config", "user.name", "t"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
}

/// O `gh` falso do projeto com submódulo: anota cada chamada com o
/// repositório de onde veio (`sub` ou `main`), abre o pull request 3 no
/// submódulo e o 7 no principal, lembra quais estão abertos, em rascunho e
/// mergeados, e faz o merge do submódulo de verdade no servidor dele, por um
/// commit de merge. Ao marcar um como pronto, anota o ponteiro do submódulo
/// que o principal tinha naquele instante ([`pointer_when_ready`]): é assim
/// que o teste vê se o ponteiro veio antes do pronto.
const SUBMODULE_GH: &str = r#"#!/bin/sh
case "$(pwd -P)" in
  */libs/sub) repo=sub; number=3 ;;
  *) repo=main; number=7 ;;
esac
echo "$repo $*" >> "$GH_LOG"
mark="$GH_STATE/$repo"
case "$1 $2" in
"pr create")
  touch "$mark.open"
  case " $* " in *" --draft "*) touch "$mark.draft" ;; esac
  echo "https://github.com/exemplo/$repo/pull/$number"
  exit 0 ;;
"pr view")
  [ -f "$mark.open" ] || { echo 'no pull requests found' >&2; exit 1; }
  case "$*" in *statusCheckRollup*) echo '{"statusCheckRollup":[]}'; exit 0 ;; esac
  state=OPEN; [ -f "$mark.merged" ] && state=MERGED
  draft=false; [ -f "$mark.draft" ] && draft=true
  printf '{"number":%s,"title":"t","state":"%s","headRefName":"feature/ponta","baseRefName":"b","isDraft":%s,"url":"https://github.com/exemplo/%s/pull/%s"}
' "$number" "$state" "$draft" "$repo" "$number"
  exit 0 ;;
"pr merge")
  work="$GH_STATE/merge-$repo"
  git clone -q "$(git config --get remote.origin.url)" "$work"     && git -C "$work" -c user.email=t@example.com -c user.name=t merge -q --no-ff origin/feature/ponta -m "Merge pull request #$number"     && git -C "$work" push -q origin HEAD:main     && touch "$mark.merged"
  exit $? ;;
"pr ready")
  git rev-parse "HEAD:libs/sub" > "$GH_STATE/ready-$repo" 2>/dev/null
  rm -f "$mark.draft"; exit 0 ;;
"api --method") exit 0 ;;
esac
exit 1
"#;

/// O projeto de teste: um repositório com `main` e `dev`, parado em `dev`, com
/// as bases declaradas, o provedor do GitHub, o lint do projeto e o Mustard
/// fora do git; uma pasta pessoal falsa; e o `gh` falso, que anota cada
/// chamada e responde que a branch não tem pull request e que o criado é o 7.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    bin: PathBuf,
    /// Os servidores locais do projeto com submódulo.
    remotes: PathBuf,
}

impl Project {
    fn new() -> Self {
        Self::build(false)
    }

    /// O projeto com o submódulo `libs/sub`, que vem de um servidor local com
    /// a base `main`; o principal tem o servidor dele, com `main` e `dev`; e o
    /// `gh` falso responde por repositório ([`SUBMODULE_GH`]).
    fn with_submodule() -> Self {
        Self::build(true)
    }

    fn build(submodule: bool) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("projeto");
        let home = dir.path().join("casa");
        let bin = dir.path().join("bin");
        let remotes = dir.path().join("servidores");
        for folder in [&root, &home, &bin, &remotes] {
            std::fs::create_dir_all(folder).expect("folder");
        }
        git(&root, &["init", "-q"]);
        identify(&root);
        git(&root, &["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\ntarget/\n").expect("exclude");
        let config = json!({
            "language": {"text": "pt-BR"},
            "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
            "lintCommand": "git --version",
        });
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    println!(\"oi\");\n}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        if submodule {
            let sub_server = remotes.join("sub.git");
            let seed = remotes.join("semente");
            git(&remotes, &["init", "-q", "--bare", "-b", "main", "sub.git"]);
            git(&remotes, &["init", "-q", "-b", "main", "semente"]);
            identify(&seed);
            std::fs::write(seed.join("lib.txt"), "a biblioteca\n").expect("the submodule file");
            git(&seed, &["add", "-A"]);
            git(&seed, &["commit", "-q", "-m", "biblioteca"]);
            git(&seed, &["push", "-q", &sub_server.to_string_lossy(), "main"]);
            let sub_url = sub_server.to_string_lossy().to_string();
            git(&root, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &sub_url, SUB]);
            identify(&root.join(SUB));
            git(&root, &["commit", "-q", "-m", "submodulo"]);
            git(&remotes, &["init", "-q", "--bare", "projeto.git"]);
            git(&root, &["remote", "add", "origin", &remotes.join("projeto.git").to_string_lossy()]);
            git(&root, &["push", "-q", "origin", "main"]);
        }
        git(&root, &["checkout", "-q", "-b", "dev"]);
        if submodule {
            git(&root, &["push", "-q", "origin", "dev"]);
        }

        let gh = bin.join("gh");
        let script = if submodule {
            SUBMODULE_GH.to_string()
        } else {
            "#!/bin/sh\necho \"$*\" >> \"$GH_LOG\"\ncase \"$1 $2\" in\n\
             \"pr view\") echo 'no pull requests found' >&2; exit 1 ;;\n\
             \"pr create\") echo 'https://github.com/exemplo/projeto/pull/7'; exit 0 ;;\n\
             esac\nexit 1\n"
                .to_string()
        };
        std::fs::write(&gh, script).expect("the fake gh");
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        Self { _dir: dir, root, home, bin, remotes }
    }

    /// O binário com `args`, no projeto, com a pasta pessoal falsa, o `gh`
    /// falso à frente do `PATH` e nenhuma sessão nem spec forçada.
    fn command(&self, args: &[&str], stdin: &str) -> Output {
        let mut binary = Command::new(env!("CARGO_BIN_EXE_mustard-rt"));
        let mut child = self
            .env(&mut binary)
            .args(args)
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(stdin.as_bytes());
        }
        child.wait_with_output().expect("the binary finishes")
    }

    /// O ambiente de quem roda: a pasta pessoal falsa, o `gh` falso à frente
    /// do `PATH` e nenhuma sessão nem spec forçada.
    fn env<'a>(&self, command: &'a mut Command) -> &'a mut Command {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap_or_default());
        command
            .env("PATH", path)
            .env("GH_LOG", self.gh_log())
            .env("GH_STATE", &self.remotes)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_PROJECT_DIR", &self.root)
            .env("MUSTARD_CLAUDE_BIN", self.home.join("sem-claude"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env_remove("MUSTARD_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
    }

    /// Um comando `run`, que precisa responder `ok`.
    fn run(&self, args: &[&str]) -> Value {
        let report = self.answer(args);
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    }

    /// Um comando `run` que pode recusar: a resposta vem como veio.
    fn answer(&self, args: &[&str]) -> Value {
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let out = self.command(&all, "");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
    }

    /// Uma gravação pelo `run write`.
    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string()])
    }

    /// Um evento do harness entregue ao gancho, como a sessão entrega; devolve
    /// o que o gancho disse.
    fn hook(&self, event: &str, payload: &Value) -> String {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// As chamadas ao `gh` falso, na ordem.
    fn gh_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.gh_log()).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn gh_log(&self) -> PathBuf {
        self.home.join("gh.log")
    }

    fn log(&self) -> SpecLog {
        store::read(&store::spec_file(&self.root, SPEC).expect("spec file")).expect("readable").expect("the spec file")
    }
}

/// A lista `agreed` do veredito final e da entrega da onda, com todo o
/// combinado vigente atendido: estes testes provam o fluxo do fechamento e do
/// pull request, não o do combinado — sem a lista, a revisão final e a
/// entrega da onda, cujo pedido leva as respostas do levantamento, seriam
/// recusadas por faltar item.
fn agreed_all_met(project: &Project) -> Value {
    let log = project.log();
    let codes = log.codes();
    let items: Vec<Value> = mustard_core::domain::wave_prompt::all_agreed(&log)
        .iter()
        .map(|item| json!({"item": codes.get(&item.id).cloned().unwrap_or_default(), "met": true}))
        .collect();
    json!(items)
}

/// A fala do usuário, pelo gancho da entrada; devolve o número dela.
fn user_says(project: &Project, text: &str) -> u64 {
    project.hook(
        "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION,
            "cwd": project.root.to_string_lossy()}),
    );
    let log = project.log();
    let said = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user"))
        .expect("the entry hook recorded the message");
    assert_eq!(said.str_field("text"), Some(text));
    said.id
}

/// O levantamento inteiro: o objetivo, o `grill` e cada ponto gravado,
/// respondido e fechado.
fn survey(project: &Project) {
    let said = user_says(project, GOAL);
    project.write("context", &json!({"text": GOAL, "origin": said}));
    let grilled = project.run(&["grill", "--spec", SPEC, "--kinds", "feature"]);
    let points = grilled["points"].as_array().cloned().expect("the point list");
    assert!(!points.is_empty(), "{grilled}");
    let mut current = Value::Null;
    for point in &points {
        let mut open = point.clone();
        open["status"] = json!("open");
        open["facts"] = json!([{"text": "A saudação mora no programa.", "source": "src/main.rs:2"}]);
        current = project.write("point", &open)["point"].clone();
    }
    for point in &points {
        let code = current["code"].as_str().expect("the open point").to_string();
        let answer = project.write(
            "decision",
            &json!({"text": format!("Resposta ao ponto {code}."), "keys": ["levantamento"],
                "why": "o usuário respondeu", "origin": said, "applies_to": {"files": ["**"]}}),
        );
        let closed = project.write(
            "point",
            &json!({"block": point["block"], "gap": point["gap"], "from": "gap", "status": "closed",
                "closes": code, "result": [answer["id"]], "origin": said}),
        );
        current = closed["point"].clone();
    }
}

/// O plano de uma tarefa só: o critério com a prova e a tarefa que o cobre,
/// sem onda. A onda nasce da rodada, pelo backlog, com autor binário.
fn plan(project: &Project) {
    plan_files(project, &["src/main.rs"]);
}

/// [`plan`] com a tarefa mudando os arquivos `files`.
fn plan_files(project: &Project, files: &[&str]) {
    let said = user_says(project, "O plano é uma tarefa só, que muda a saudação.");
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let files: Vec<Value> = files.iter().map(|path| json!({"path": path})).collect();
    project.write(
        "task",
        &json!({"title": "Entregar a tarefa", "text": "Trocar a saudação no programa.", "files": files, "depends_on": [],
            "covers": [criterion["id"]], "origin": said}),
    );
    let planned = project.run(&["plan", "--spec", SPEC]);
    assert_eq!(State::from_log(&project.log()).phase, Some("plan"), "{planned}");
}

/// A primeira rodada, com a escolha antes do envio: as respostas do
/// levantamento valem para o projeto todo, então a rodada entrega ao
/// orquestrador os candidatos da onda, cada um com o título, sem pedir
/// agente nenhum, e não solta a onda; a linha da escolha, sem mudança, solta
/// a onda. Devolve a resposta da rodada que a soltou.
fn first_round(project: &Project) -> Value {
    let asked = project.run(&["round", "--spec", SPEC]);
    assert_eq!(asked["dispatch"], json!([]), "{asked}");
    let candidates = asked["analysis"][0]["project"].as_array().cloned().unwrap_or_default();
    assert!(!candidates.is_empty(), "{asked}");
    assert!(candidates.iter().all(|c| c["title"].as_str().is_some_and(|t| !t.is_empty())), "{asked}");
    assert!(asked["analysis"][0].get("model").is_none(), "{asked}");
    let answer = json!({"wave": 1, "removed": [], "added": []});
    project.run(&["round", "--spec", SPEC, "--report", &format!("<ANALYSIS>{answer}</ANALYSIS>")])
}

/// O clique em "Aprovar" na pergunta da aprovação, pelo gancho da testemunha.
fn approve(project: &Project) {
    let question = translate("approval.question", Locale::PtBr);
    let yes = translate("approval.option", Locale::PtBr);
    project.hook(
        "PostToolUse",
        &json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [{"question": question, "options": [{"label": yes}, {"label": "Ajustar"}]}]},
            "tool_response": {"answers": {question: yes}},
            "session_id": SESSION,
            "cwd": project.root.to_string_lossy(),
        }),
    );
    assert_eq!(State::from_log(&project.log()).phase, Some("approved"));
}

/// Quantas chamadas cada comando do fluxo gravou na spec.
fn calls(project: &Project) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for call in project.log().visible().into_iter().filter(|e| e.event_type == "call") {
        *out.entry(call.str_field("command").unwrap_or_default().to_string()).or_insert(0) += 1;
    }
    out
}

/// Uma spec de teste roda de ponta a ponta com o binário novo: abrir,
/// levantar, conferir o plano, aprovar pelo clique, uma rodada, o agente de
/// teste dedicado e o pull request funcionam em sequência, e a pasta da spec
/// termina com três arquivos. Cada passo do fluxo é uma chamada só: abrir 1,
/// levantamento 1 (as respostas gravadas não contam), plano 1, aprovar 0,
/// cada rodada 1 e pull request 1; a escolha antes do envio da primeira onda
/// é uma rodada a mais, a que traz a escolha do orquestrador, e o fechamento
/// é duas chamadas — o pedido do agente de teste dedicado e a aprovação dele
/// —, mesmo com uma onda só. A onda tem uma tarefa só e ainda assim ganha
/// cópia separada, como qualquer outra: o orquestrador edita na cópia, e a
/// rodada leva a mudança de volta ao checkout principal.
#[test]
fn a_test_spec_runs_end_to_end_one_call_per_step_and_leaves_three_files() {
    let project = Project::new();

    let opened = project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    survey(&project);
    plan(&project);
    approve(&project);

    // Primeira rodada: a análise antes do envio, e a onda ganha cópia
    // separada, mesmo com uma tarefa só.
    let first = first_round(&project);
    let dispatched = first["dispatch"].as_array().cloned().unwrap_or_default();
    assert_eq!(dispatched.len(), 1, "{first}");
    let next = first["next"].as_str().unwrap_or_default();
    assert!(next.contains(translate("round.next", Locale::PtBr)), "{next}");
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));

    // A onda muda o arquivo na cópia dela e grava a entrega na spec; a
    // rodada assume a volta e não pede revisão nenhuma dela.
    std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"olá\");\n}\n").expect("the change");
    let delivered = json!({"wave": 1, "text": "A saudação virou olá.", "files": ["src/main.rs"],
        "commit": "a saudação vira olá", "agreed": agreed_all_met(&project)});
    project.run(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()]);
    let second = project.run(&["round", "--spec", SPEC]);
    assert!(second.get("reviews").is_none(), "{second}");
    assert!(!copy.exists(), "the copy is removed once the round takes the change back: {second}");
    assert_eq!(std::fs::read_to_string(project.root.join("src/main.rs")).unwrap(), "fn main() {\n    println!(\"olá\");\n}\n");

    // O fechamento roda o lint e o critério e pede o agente de teste
    // dedicado, mesmo numa spec de uma onda.
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["phase"], json!("running"), "{asked}");
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");

    // O revisor grava o veredito aprovado; o fechamento o assume e fecha.
    let verdict = json!({"final": true, "result": "approved", "text": "A saudação mudou.",
        "agreed": agreed_all_met(&project)});
    project.run(&["write", "verdict", "--spec", SPEC, "--json", &verdict.to_string()]);
    let closed = project.run(&["close", "--spec", SPEC]);
    assert_eq!(closed["phase"], json!("closed"), "{closed}");
    assert!(closed.get("review").is_none(), "{closed}");
    let pr_line = format!("mustard-rt run pr-open --base dev --head feature/{SPEC} --spec {SPEC}");
    assert_eq!(closed["command"], json!(pr_line), "{closed}");

    // O pull request abre pela linha que o fechamento devolveu.
    let argv: Vec<&str> = pr_line.split_whitespace().skip(2).collect();
    let pr = project.run(&argv);
    assert_eq!(pr["number"], json!(7), "{pr}");
    let asked_gh = std::fs::read_to_string(project.gh_log()).expect("the fake gh was called");
    assert!(asked_gh.lines().any(|l| l.starts_with("pr create") && l.contains("--head feature/ponta")), "{asked_gh}");
    let state = State::from_log(&project.log());
    assert_eq!(state.phase, Some("pr_open"), "{pr}");

    let expected: BTreeMap<String, usize> =
        [("open", 1), ("grill", 1), ("plan", 1), ("round", 3), ("close", 2), ("pr-open", 1)]
            .into_iter()
            .map(|(command, count)| (command.to_string(), count))
            .collect();
    assert_eq!(calls(&project), expected, "each flow step is one call, the approval none and closing two");

    let folder = project.root.join(".claude/spec").join(SPEC);
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .expect("the spec folder")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["copy", "spec.ndjson"], "the spec folder ends with the events and the copy, and no page");
}

/// O fluxo inteiro, da abertura ao pull request, não grava onda pela linha de
/// comando: o plano leva só o critério e a tarefa, e a onda que sai nasce da
/// rodada, pelo backlog. No fim, toda linha de onda do arquivo da spec — lida
/// crua, com as versões antigas e as removidas — tem autor binário, e há ao
/// menos uma, para a conferência não passar num arquivo sem onda.
#[test]
fn o_fluxo_inteiro_nao_grava_onda_pela_linha_de_comando() {
    let project = Project::new();
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    survey(&project);
    plan(&project);
    approve(&project);

    let first = first_round(&project);
    assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"olá\");\n}\n").expect("the change");
    let delivered = json!({"wave": 1, "text": "A saudação virou olá.", "files": ["src/main.rs"],
        "commit": "a saudação vira olá", "agreed": agreed_all_met(&project)});
    project.run(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()]);
    project.run(&["round", "--spec", SPEC]);
    project.run(&["close", "--spec", SPEC]);
    let verdict = json!({"final": true, "result": "approved", "text": "A saudação mudou.",
        "agreed": agreed_all_met(&project)});
    project.run(&["write", "verdict", "--spec", SPEC, "--json", &verdict.to_string()]);
    let closed = project.run(&["close", "--spec", SPEC]);
    let pr_line = closed["command"].as_str().expect("the pr-open line").to_string();
    let argv: Vec<&str> = pr_line.split_whitespace().skip(2).collect();
    project.run(&argv);
    assert_eq!(State::from_log(&project.log()).phase, Some("pr_open"));

    let path = store::spec_file(&project.root, SPEC).expect("spec file");
    let content = std::fs::read_to_string(&path).expect("the spec file");
    let waves: Vec<Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["type"] == json!("wave"))
        .collect();
    assert!(!waves.is_empty(), "the round wrote the wave of the lot: {content}");
    for wave in &waves {
        assert_eq!(wave["author"], json!("binary"), "a wave that is not the binary's: {wave}");
    }
}

/// Um critério gravado sem declarar a forma dele é recusado, e a recusa lista
/// as cinco formas do padrão pelo nome, em vez de um nome de campo cru.
#[test]
fn o_criterio_sem_forma_declarada_e_recusado() {
    let project = Project::new();
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    survey(&project);
    let said = user_says(&project, "O plano é uma onda só, que muda a saudação.");

    let refused = project.answer(&[
        "write",
        "criterion",
        "--spec",
        SPEC,
        "--json",
        &json!({"when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "origin": said})
            .to_string(),
    ]);
    assert_eq!(refused["ok"], json!(false), "{refused}");
    assert_eq!(refused["reason"], json!("criterion-form-missing"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    for forma in [
        "vale sempre",
        "disparada por um acontecimento",
        "estado durar",
        "recurso existir",
        "acontecimento indesejado",
    ] {
        assert!(hint.contains(forma), "a recusa lista a forma {forma:?} pelo nome: {hint}");
    }

    // Com a forma declarada, a mesma gravação passa.
    let accepted = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    assert_eq!(accepted["ok"], json!(true), "{accepted}");
}

/// A exigência da forma vale só para o critério que nasce agora, nunca para a
/// emenda de um critério antigo: um critério gravado direto no arquivo, sem
/// forma, como as specs de antes da exigência têm, recebe a emenda dele
/// também sem forma, e a gravação passa.
#[test]
fn a_emenda_de_criterio_antigo_nao_exige_forma() {
    let project = Project::new();
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    survey(&project);
    let said = user_says(&project, "O plano é uma onda só, que muda a saudação.");

    // Um critério sem forma, escrito direto no arquivo, como as specs
    // antigas — de antes da exigência — têm.
    let path = store::spec_file(&project.root, SPEC).expect("spec file");
    let old_id = store::read(&path).expect("readable").expect("the spec file").max_id() + 1;
    let old_criterion = json!({"v": 1, "id": old_id, "code": "MSTD-CRIT-0001", "at": "2026-01-01T10:00:00-03:00",
        "type": "criterion", "author": "binary",
        "when": "o programa roda", "then": "a saudação antiga aparece", "proof": "git --version"});
    let mut content = std::fs::read_to_string(&path).expect("read the spec file");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&old_criterion.to_string());
    content.push('\n');
    std::fs::write(&path, content).expect("write the old criterion");

    // A emenda dele, sem forma, passa: a exigência não vale para o critério
    // antigo.
    let amended = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "origin": said, "replaces": old_id}),
    );
    assert_eq!(amended["ok"], json!(true), "{amended}");

    // Um critério novo (sem `replaces`) continua exigindo a forma: a
    // exigência segue protegida para quem nasce agora.
    let new_criterion =
        json!({"when": "outra coisa", "then": "outro efeito", "proof": "git --version", "origin": said});
    let refused_new =
        project.answer(&["write", "criterion", "--spec", SPEC, "--json", &new_criterion.to_string()]);
    assert_eq!(refused_new["ok"], json!(false), "{refused_new}");
    assert_eq!(refused_new["reason"], json!("criterion-form-missing"), "{refused_new}");
}

/// Os três termos internos usam o nome de mercado, nos dois idiomas: o que
/// era "combinado" vira "requisitos acordados", o que era "prova" vira
/// "verificação", e o que era "revisão final" vira "aceitação" — sem sobra do
/// nome antigo no texto impresso, inclusive no pedido de verdade que o
/// binário monta para o agente da onda.
#[test]
fn os_tres_termos_usam_o_nome_de_mercado() {
    let esperado = [
        (Locale::PtBr, "page.block.agreed", "Requisitos acordados"),
        (Locale::EnUs, "page.block.agreed", "Agreed requirements"),
        (Locale::PtBr, "page.field.proof", "Verificação"),
        (Locale::EnUs, "page.field.proof", "Verification"),
        (Locale::PtBr, "page.field.final", "Aceitação"),
        (Locale::EnUs, "page.field.final", "Acceptance"),
        (Locale::PtBr, "prompt.part.agreed", "Requisitos acordados"),
        (Locale::EnUs, "prompt.part.agreed", "Agreed requirements"),
    ];
    for (locale, key, texto) in esperado {
        assert_eq!(translate(key, locale), texto, "{key} ({locale:?}) usa o nome de mercado");
    }

    let project = Project::new();
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    survey(&project);
    plan(&project);
    approve(&project);
    first_round(&project);
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let text = sent.str_field("text").unwrap_or_default();
    assert!(!text.contains("Combinado"), "o pedido enviado não guarda o nome antigo: {text}");
    if text.contains("## ") {
        assert!(
            !text.contains("## Prova") && !text.contains("## Revisão final"),
            "nenhum cabeçalho do pedido guarda um nome antigo: {text}"
        );
    }
}

/// A spec do projeto com submódulo, da abertura ao pull request: a onda muda
/// um arquivo do principal e um do submódulo, a rodada comita os dois e o
/// fechamento devolve a linha do `pr-open`, que roda. Confere o que a rodada
/// deixou nos dois repositórios e devolve a resposta do `pr-open`.
fn open_pull_requests_with_a_submodule(project: &Project) -> Value {
    let opened = project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    survey(project);
    plan_files(project, &["src/main.rs", SUB_FILE]);
    approve(project);

    // A cópia da onda traz o submódulo que a onda toca.
    let first = first_round(project);
    assert_eq!(first["dispatch"].as_array().map(Vec::len), Some(1), "{first}");
    let log = project.log();
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send").expect("the send");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    assert!(copy.join(SUB).join(".git").is_file(), "the copy brings the submodule the wave touches: {first}");

    std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"olá\");\n}\n").expect("the change");
    std::fs::write(copy.join(SUB_FILE), "a biblioteca nova\n").expect("the submodule change");
    let delivered = json!({"wave": 1, "text": "A saudação e a biblioteca mudaram.",
        "files": ["src/main.rs", SUB_FILE], "commit": "a saudação e a biblioteca mudam",
        "agreed": agreed_all_met(project)});
    project.run(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()]);
    let second = project.run(&["round", "--spec", SPEC]);
    assert!(!copy.exists(), "the copy and the submodule copy inside it are removed: {second}");

    // O commit sai dentro do submódulo, na branch de mesmo nome, e o do
    // principal leva o ponteiro novo junto com o arquivo dele.
    let sub = project.root.join(SUB);
    assert_eq!(git_out(&sub, &["rev-parse", "--abbrev-ref", "HEAD"]), BRANCH, "{second}");
    assert_eq!(git_out(&project.root, &["rev-parse", "--abbrev-ref", "HEAD"]), BRANCH);
    assert_eq!(std::fs::read_to_string(sub.join("lib.txt")).unwrap(), "a biblioteca nova\n");
    assert_eq!(git_out(&sub, &["show", "-s", "--format=%s", "HEAD"]), "feat(onda-1): a saudação e a biblioteca mudam");
    assert_eq!(git_out(&sub, &["show", "--name-only", "--format=", "HEAD"]), "lib.txt");
    assert_eq!(
        git_out(&project.root, &["rev-parse", &format!("HEAD:{SUB}")]),
        git_out(&sub, &["rev-parse", "HEAD"]),
        "the main commit carries the new pointer"
    );
    let changed = git_out(&project.root, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(changed.lines().collect::<Vec<_>>(), [SUB, "src/main.rs"], "{second}");
    assert_eq!(second["commit"]["submodules"][0]["path"], json!(SUB), "{second}");

    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let verdict = json!({"final": true, "result": "approved", "text": "Mudaram.", "agreed": agreed_all_met(project)});
    project.run(&["write", "verdict", "--spec", SPEC, "--json", &verdict.to_string()]);
    let closed = project.run(&["close", "--spec", SPEC]);
    let pr_line = closed["command"].as_str().expect("the pr-open line").to_string();
    let argv: Vec<&str> = pr_line.split_whitespace().skip(2).collect();
    project.run(&argv)
}

/// O evento de início de sessão, como a sessão o entrega.
fn session_start(project: &Project) -> Value {
    json!({"hook_event_name": "SessionStart", "source": "startup", "session_id": SESSION,
        "cwd": project.root.to_string_lossy()})
}

/// Outra pessoa mergeia o pull request do submódulo, pelo `gh` de fora do
/// binário.
fn someone_merges_the_submodule(project: &Project) {
    let merged = project
        .env(&mut Command::new(project.bin.join("gh")))
        .args(["pr", "merge", "3", "--merge"])
        .current_dir(project.root.join(SUB))
        .output();
    assert!(merged.expect("the fake gh").status.success(), "someone else merges the submodule pull request");
}

/// A ponta da base do submódulo no servidor dele, que é o ponteiro que o
/// principal passa a gravar depois do merge.
fn submodule_base_tip(project: &Project) -> String {
    git_out(&project.remotes.join("sub.git"), &["rev-parse", "refs/heads/main"])
}

/// O ponteiro do submódulo gravado no commit do principal.
fn pointer(project: &Project) -> String {
    git_out(&project.root, &["rev-parse", &format!("HEAD:{SUB}")])
}

/// O ponteiro que o principal tinha quando o pull request dele foi marcado
/// como pronto, anotado pelo `gh` falso.
fn pointer_when_ready(project: &Project) -> String {
    std::fs::read_to_string(project.remotes.join("ready-main"))
        .expect("the main pull request was marked ready")
        .trim()
        .to_string()
}

/// Quando uma spec mexe no principal e num submódulo, as duas branches têm o
/// mesmo nome, o pull request do submódulo abre primeiro, contra a base dele,
/// e o do principal abre como rascunho. Enquanto o do submódulo não entra, o
/// início da sessão diz qual falta e o principal segue rascunho; quando ele
/// entra pelo `pr-merge`, o principal recebe o ponteiro da base do submódulo,
/// envia a branch e fica pronto.
#[test]
fn a_spec_on_the_main_repository_and_a_submodule_readies_the_main_pull_request_only_after_the_submodule_one() {
    let project = Project::with_submodule();
    let pr = open_pull_requests_with_a_submodule(&project);

    // O do submódulo abre primeiro, da branch de mesmo nome contra a base
    // dele; o do principal, depois, como rascunho.
    let calls = project.gh_calls();
    let created = |repo: &str| {
        calls
            .iter()
            .position(|call| call.starts_with(&format!("{repo} pr create")))
            .unwrap_or_else(|| panic!("no pull request opened in {repo}: {calls:#?}"))
    };
    let (sub_at, main_at) = (created("sub"), created("main"));
    assert!(sub_at < main_at, "the submodule pull request opens first: {calls:#?}");
    assert!(calls[sub_at].contains(&format!("--head {BRANCH} --base main")), "{}", calls[sub_at]);
    assert!(!calls[sub_at].contains("--draft"), "{}", calls[sub_at]);
    assert!(calls[main_at].contains(&format!("--head {BRANCH} --base dev")), "{}", calls[main_at]);
    assert!(calls[main_at].contains("--draft"), "the main pull request opens as a draft: {}", calls[main_at]);
    let sub_server = project.remotes.join("sub.git");
    assert!(
        !git_out(&sub_server, &["for-each-ref", &format!("refs/heads/{BRANCH}")]).is_empty(),
        "the submodule branch, with the same name, is on its server"
    );
    assert_eq!(pr["number"], json!(7), "{pr}");
    assert_eq!(pr["submodules"][0]["path"], json!(SUB), "{pr}");
    assert_eq!(pr["submodules"][0]["url"], json!("https://github.com/exemplo/sub/pull/3"), "{pr}");
    let waiting = translate("pr.submodules.waiting", Locale::PtBr).replace("{pr}", "7").replace("{paths}", SUB);
    assert_eq!(pr["hint"], json!(waiting), "{pr}");

    // Enquanto o do submódulo não entra, o início da sessão diz qual falta, e
    // o principal segue rascunho.
    let said = project.hook("SessionStart", &session_start(&project));
    assert!(said.contains(&waiting), "the session start names the missing pull request: {said}");
    assert!(!project.gh_calls().iter().any(|call| call.starts_with("main pr ready")), "still a draft");

    // O do submódulo entra pelo `pr-merge`: o ponteiro vai para a base do
    // submódulo, a branch do principal é enviada, e o principal fica pronto.
    let sub = project.root.join(SUB);
    let before = pointer(&project);
    let merged = project.run(&["pr-merge", "--pr", "3", "--root", &sub.to_string_lossy()]);
    assert_eq!(merged["action"], json!("merged"), "{merged}");
    assert_eq!(merged["submodules"]["ready"], json!(true), "{merged}");
    let sub_base = submodule_base_tip(&project);
    let pointer = pointer(&project);
    assert_ne!(pointer, before, "the pointer moved: {merged}");
    assert_eq!(pointer, sub_base, "the pointer is the submodule base after the merge: {merged}");
    assert_eq!(pointer_when_ready(&project), sub_base, "the pointer was already in the main when it went ready");
    assert_eq!(
        git_out(&project.remotes.join("projeto.git"), &["rev-parse", &format!("refs/heads/{BRANCH}")]),
        git_out(&project.root, &["rev-parse", "HEAD"]),
        "the main branch was pushed with the pointer"
    );
    let calls = project.gh_calls();
    let merged_at = calls.iter().position(|call| call.starts_with("sub pr merge 3")).expect("the submodule merge");
    let ready_at = calls.iter().position(|call| call.starts_with("main pr ready 7")).expect("the main pull request is ready");
    assert!(merged_at < ready_at, "{calls:#?}");
    assert!(git_out(&sub, &["branch", "--list", BRANCH]).is_empty(), "the submodule branch left this machine");
}

/// O pull request do submódulo que outra pessoa mergeou é achado no início da
/// sessão: o ponteiro vai para a base do submódulo, a branch do principal é
/// enviada, o principal fica pronto e o aviso diz isso.
#[test]
fn a_submodule_pull_request_merged_by_someone_else_readies_the_main_one_at_session_start() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);

    someone_merges_the_submodule(&project);
    assert!(!project.gh_calls().iter().any(|call| call.starts_with("main pr ready")), "still a draft");

    let start = session_start(&project);
    let said = project.hook("SessionStart", &start);
    let ready = translate("pr.submodules.ready", Locale::PtBr).replace("{pr}", "7").replace("{paths}", SUB);
    assert!(said.contains(&ready), "the session start says the main pull request is ready: {said}");
    let sub_base = submodule_base_tip(&project);
    assert_eq!(pointer(&project), sub_base, "the pointer moved");
    assert_eq!(
        git_out(&project.remotes.join("projeto.git"), &["rev-parse", &format!("refs/heads/{BRANCH}")]),
        git_out(&project.root, &["rev-parse", "HEAD"]),
        "the main branch was pushed with the pointer"
    );
    assert!(project.gh_calls().iter().any(|call| call.starts_with("main pr ready 7")), "the main pull request is ready");
    assert_eq!(pointer_when_ready(&project), sub_base, "the pointer was already in the main when it went ready");

    // A conferência seguinte não refaz nada: o principal já está pronto.
    let commits = git_out(&project.root, &["rev-list", "--count", "HEAD"]);
    let again = project.hook("SessionStart", &start);
    assert!(!again.contains(&ready), "{again}");
    assert_eq!(git_out(&project.root, &["rev-list", "--count", "HEAD"]), commits, "no second pointer commit");
}

/// O submódulo que entrou e já não tem a branch na máquina ainda leva o
/// ponteiro ao principal: o pronto vem depois do ponteiro, nunca sem ele. A
/// branch na máquina não diz nada sobre o ponteiro — quem decide é o fato,
/// depois de buscar a base.
#[test]
fn a_landed_submodule_without_its_branch_here_still_moves_the_pointer_before_the_ready() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);
    someone_merges_the_submodule(&project);

    // A branch sai da máquina antes da conferência.
    let sub = project.root.join(SUB);
    git(&sub, &["checkout", "-q", "--detach", BRANCH]);
    git(&sub, &["branch", "-qD", BRANCH]);

    project.hook("SessionStart", &session_start(&project));
    let sub_base = submodule_base_tip(&project);
    assert_eq!(pointer(&project), sub_base, "the pointer moved although the branch had left");
    assert_eq!(pointer_when_ready(&project), sub_base, "the pointer was already in the main when it went ready");
}

/// Um commit do submódulo que o merge não levou trava a conferência: nada é
/// mexido, a branch fica com ele, o principal segue rascunho e a resposta diz
/// o motivo.
#[test]
fn a_submodule_commit_the_merge_did_not_take_keeps_the_branch_and_the_draft() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);

    // O merge leva o que está no servidor do submódulo; este commit fica só
    // aqui, e nunca foi enviado.
    let sub = project.root.join(SUB);
    std::fs::write(sub.join("lib.txt"), "a biblioteca ainda mais nova\n").expect("the change");
    git(&sub, &["commit", "-qam", "o conserto que ficou aqui"]);
    let kept = git_out(&sub, &["rev-parse", "HEAD"]);

    let before = pointer(&project);
    let merged = project.answer(&["pr-merge", "--pr", "3", "--root", &sub.to_string_lossy()]);
    assert_eq!(merged["submodules"]["ready"], json!(false), "{merged}");
    let problem = merged["submodules"]["problem"].as_str().unwrap_or_default().to_string();
    assert!(problem.contains(SUB) && problem.contains(BRANCH), "the refusal names the branch that is ahead: {merged}");
    assert_eq!(pointer(&project), before, "the pointer did not move: {merged}");
    assert_eq!(git_out(&sub, &["rev-parse", BRANCH]), kept, "the local commit still has a branch: {merged}");
    assert!(!project.gh_calls().iter().any(|call| call.starts_with("main pr ready")), "still a draft");
}

/// O `pr-open` que roda de novo com o pull request do submódulo já mergeado
/// não o dá por pronto quando sobra commit aqui: para com o motivo, antes de
/// mexer no pull request do principal.
#[test]
fn a_second_pr_open_refuses_a_merged_submodule_that_still_carries_work() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);
    someone_merges_the_submodule(&project);

    let sub = project.root.join(SUB);
    std::fs::write(sub.join("lib.txt"), "a biblioteca ainda mais nova\n").expect("the change");
    git(&sub, &["commit", "-qam", "o conserto que ficou aqui"]);

    let opened = project.answer(&["pr-open", "--base", "dev", "--head", BRANCH, "--spec", SPEC]);
    assert_eq!(opened["ok"], json!(false), "{opened}");
    let error = opened["error"].as_str().unwrap_or_default().to_string();
    assert!(error.contains(SUB) && error.contains(BRANCH), "the refusal names the branch that is ahead: {opened}");
    assert_eq!(opened["submodules"][0]["action"], json!("open"), "the submodule is not reported as done: {opened}");
}

/// O envio do ponteiro que o servidor recusa deixa o principal como rascunho,
/// com o motivo e a branch do submódulo no lugar; a conferência seguinte, com
/// o servidor aceitando, refaz o envio e só então marca o principal pronto.
#[test]
fn a_submodule_pointer_the_server_refuses_keeps_the_main_pull_request_a_draft() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);

    let refuse = project.remotes.join("projeto.git/hooks/pre-receive");
    std::fs::write(&refuse, "#!/bin/sh\nexit 1\n").expect("the server hook");
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&refuse, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let sub = project.root.join(SUB);
    let merged = project.answer(&["pr-merge", "--pr", "3", "--root", &sub.to_string_lossy()]);
    assert_eq!(merged["submodules"]["ready"], json!(false), "{merged}");
    assert!(merged["submodules"]["problem"].as_str().is_some_and(|r| r.starts_with("push")), "{merged}");
    assert!(!project.gh_calls().iter().any(|call| call.starts_with("main pr ready")), "still a draft");
    assert!(!git_out(&sub, &["branch", "--list", BRANCH]).is_empty(), "the submodule branch is still here");

    // O servidor volta a aceitar: a conferência seguinte refaz o envio.
    std::fs::remove_file(&refuse).expect("the server hook leaves");
    project.hook("SessionStart", &session_start(&project));
    assert_eq!(
        git_out(&project.remotes.join("projeto.git"), &["rev-parse", &format!("refs/heads/{BRANCH}")]),
        git_out(&project.root, &["rev-parse", "HEAD"]),
        "the main branch was pushed with the pointer"
    );
    assert_eq!(pointer_when_ready(&project), submodule_base_tip(&project), "the pointer came before the ready");
}

/// Duas sessões conferindo ao mesmo tempo: a trava do passo do git faz uma
/// esperar a outra, o ponteiro entra uma vez só e nenhuma delas volta com o
/// erro cru do git.
#[test]
fn two_sessions_checking_the_submodule_at_the_same_time_move_the_pointer_once() {
    let project = Project::with_submodule();
    open_pull_requests_with_a_submodule(&project);
    someone_merges_the_submodule(&project);

    // O commit do ponteiro demora, e as duas sessões se encontram dentro dele.
    let hook = project.root.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nsleep 1\n").expect("the commit hook");
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let commits = git_out(&project.root, &["rev-list", "--count", "HEAD"]);
    let start = session_start(&project);
    let (one, two) = std::thread::scope(|scope| {
        let first = scope.spawn(|| project.hook("SessionStart", &start));
        let second = scope.spawn(|| project.hook("SessionStart", &start));
        (first.join().expect("a session"), second.join().expect("the other session"))
    });

    let stuck = translate("pr.submodules.stuck", Locale::PtBr).replace("{pr}", "7");
    let stuck = stuck.split("{reason}").next().unwrap_or_default().to_string();
    for said in [&one, &two] {
        assert!(!said.contains(&stuck), "neither session is stuck on the other's git step: {said}");
    }
    let after: usize = git_out(&project.root, &["rev-list", "--count", "HEAD"]).parse().expect("a number");
    assert_eq!(after, commits.parse::<usize>().expect("a number") + 1, "the pointer commit happened once");
    assert_eq!(pointer(&project), submodule_base_tip(&project), "the pointer is the submodule base");
}
