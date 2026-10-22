// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! O backlog despachado pelo binário de verdade, numa pasta temporária: seis
//! tarefas com dependência e arquivo compartilhado, e as ondas que a rodada
//! grava, como onda de autor `binary`, sempre saem as mesmas — quem
//! compartilha arquivo cai junto, ninguém divide arquivo entre ondas, a
//! tarefa que espera só por tarefas da onda e divide arquivo com ela entra
//! depois delas, e cada onda leva um assunto só, sem teto de tarefas nem de
//! arquivos.
//!
//! Move a prova que antes vivia só como teste de unidade do módulo do grafo
//! (`apps/rt/src/shared/dag.rs`): o critério fala em despacho pelo binário,
//! num repositório temporário, não em chamar a função pura duas vezes.

#![cfg(unix)]

#[path = "support/binary.rs"]
mod binary;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

#[path = "support/closed_process.rs"]
mod closed_process;
#[path = "support/mod.rs"]
mod support;
#[path = "support/review.rs"]
mod wave_review;

const SPEC: &str = "backlog-lotes";
const GOAL: &str = "Trocar a saudação do programa.";
const SESSION: &str = "s-backlog-lotes";
/// O motivo da marca de prioridade das tarefas marcadas nos testes.
const PRIORITY: &str = "O usuário pediu esta antes das outras.";

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Um projeto de teste, com `dev` a partir de `main`, pronto para abrir uma
/// spec pelo binário de verdade.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    /// O endereço do Jev de mentira que o binário acha, com uma chave de
    /// mentira; sem ele o projeto não tem chave nenhuma.
    jev: Option<String>,
    /// A sessão que conduz a obra, entregue a cada comando como o gancho a
    /// entrega; sem ela o comando roda sem sessão nenhuma.
    session: Option<&'static str>,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("projeto");
        let home = dir.path().join("casa");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&home).expect("home");
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "t@example.com"]);
        git(&root, &["config", "user.name", "t"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        git(&root, &["checkout", "-q", "-b", "main"]);
        std::fs::write(root.join(".git/info/exclude"), ".claude/\nmustard.json\ntarget/\n").expect("exclude");
        let config = json!({
            "language": {"text": "pt-BR"},
            "ai": {"fallback": true},
            "search": {"filter": "jev"},
            "judgement": {"wave-planning": {"filter": "jev"}, "context": {"filter": "jev"}},
            "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
            "lintCommand": "git --version",
        });
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    println!(\"oi\");\n}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        git(&root, &["checkout", "-q", "-b", "dev"]);
        support::copies_leave_with_the_test(&root);
        Self { _dir: dir, root, home, jev: None, session: None }
    }

    fn command(&self, args: &[&str], stdin: &str) -> Output {
        self.command_in(&self.root, args, stdin)
    }

    /// Como [`Self::command`], rodando de dentro de `dir`.
    fn command_in(&self, dir: &Path, args: &[&str], stdin: &str) -> Output {
        self.command_from(dir, None, args, stdin)
    }

    /// A abertura da spec pela conversa da sessão [`SESSION`], como quem
    /// conduz a roda: o gancho do terminal passa a sessão ao comando, e é a
    /// conversa dela que fica ligada à spec.
    fn open(&self) -> Value {
        let args = ["run", "open", "--kind", "feature", "--name", SPEC, "--base", "dev"];
        let out = self.command_from(&self.root, Some(SESSION), &args, "");
        let report: Value = serde_json::from_slice(&out.stdout).expect("the opening answers JSON");
        assert_eq!(report["ok"], json!(true), "{report}");
        report
    }

    /// Como [`Self::command_in`], com a sessão `session` no ambiente, como o
    /// gancho do terminal a passa ao comando numa conversa; ela vale no lugar
    /// da sessão do projeto.
    fn command_from(&self, dir: &Path, session: Option<&str>, args: &[&str], stdin: &str) -> Output {
        let mut child = self
            .program(env!("CARGO_BIN_EXE_mustard-rt"))
            .envs(session.map(|session| ("MUSTARD_SESSION_ID", session)))
            .args(args)
            .current_dir(dir)
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

    /// O comando que roda `program` no ambiente do projeto: a pasta pessoal,
    /// o projeto, a chave do Jev de mentira e a sessão que conduz, quando há.
    fn program(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = binary::isolated(program);
        command
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_PROJECT_DIR", &self.root)
            .env("MUSTARD_CLAUDE_BIN", self.home.join("sem-claude"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env_remove("TYPESAFE_API_KEY")
            .env_remove("MUSTARD_JEV_URL")
            .env_remove("MUSTARD_SPEND_DIR")
            .env_remove("MUSTARD_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .envs(self.jev.iter().flat_map(|url| [("TYPESAFE_API_KEY", "fake-key"), ("MUSTARD_JEV_URL", url.as_str())]))
            .envs(self.session.map(|session| ("MUSTARD_SESSION_ID", session)));
        command
    }

    /// Um comando `run`, que precisa responder `ok`.
    fn run(&self, args: &[&str]) -> Value {
        let report = self.answer(args);
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    }

    /// Um comando `run` que pode recusar: a resposta vem como veio.
    fn answer(&self, args: &[&str]) -> Value {
        let mut owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        if matches!(args.first(), Some(&"round" | &"close")) {
            let previous = args.windows(2).find(|pair| pair[0] == "--report").map(|pair| pair[1]).unwrap_or_default();
            let report = wave_review::review_report(&self.root, SPEC, previous, |wave| {
                self.answer(&["review-wave", "--wave", &wave.to_string(), "--spec", SPEC])
            });
            if !report.is_empty() {
                if let Some(at) = owned.iter().position(|arg| arg == "--report") {
                    owned[at + 1] = report;
                } else {
                    owned.extend(["--report".into(), report]);
                }
            }
        }
        let mut all = vec!["run"];
        all.extend(owned.iter().map(String::as_str));
        let out = self.command(&all, "");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
    }

    /// Uma gravação pelo `run write`.
    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string()])
    }

    /// Um evento do harness entregue ao gancho, como a sessão entrega.
    fn hook(&self, event: &str, payload: &Value) {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn log(&self) -> SpecLog {
        store::read(&store::spec_file(&self.root, SPEC).expect("spec file")).expect("readable").expect("the spec file")
    }

    /// As chamadas `command` que o programa observou: as do arquivo de
    /// operações da spec, onde a chamada mora.
    fn calls_of(&self, command: &str) -> Vec<serde_json::Map<String, Value>> {
        let operations = store::operations_file(&self.root, SPEC).and_then(|path| store::read(&path).ok().flatten()).unwrap_or_default();
        operations.visible().into_iter().filter(|e| e.event_type == "call" && e.str_field("command") == Some(command)).map(|call| call.fields.clone()).collect()
    }

    /// Um evento semeado cru no arquivo da spec, sem passar pela linha de
    /// comando: é assim que o teste monta o que uma spec antiga já traz
    /// gravado. Devolve o `id` gravado.
    fn seed(&self, event_type: &str, fields: &Value) -> u64 {
        let path = store::spec_file(&self.root, SPEC).expect("spec file");
        store::write(&path, event_type, fields.as_object().cloned().expect("an object"), &[]).expect(event_type).id
    }
}

/// A fala do usuário, pelo gancho da entrada; devolve o número dela.
fn user_says(project: &Project, text: &str) -> u64 {
    project.hook(
        "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION,
            "cwd": project.root.to_string_lossy()}),
    );
    let log = project.log();
    let said =
        log.visible().into_iter().rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user")).expect("the entry hook recorded the message");
    said.id
}

/// O levantamento inteiro: o objetivo, o `grill`, que grava os pontos, e
/// cada ponto com os fatos somados, respondido e fechado.
fn survey(project: &Project) -> u64 {
    let said = user_says(project, GOAL);
    project.write("context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));
    let grilled = project.run(&["grill", "--spec", SPEC, "--kinds", "feature", "--full"]);
    let points = grilled["points"].as_array().cloned().expect("the point list");
    assert!(!points.is_empty(), "{grilled}");
    let mut current = Value::Null;
    for point in &points {
        let facts = json!([{"text": "A saudação mora no programa.", "source": "src/main.rs:2"}]);
        current = project.write("point", &json!({"replaces": point["id"], "facts": facts}))["point"].clone();
    }
    for point in &points {
        let code = current["code"].as_str().expect("the open point").to_string();
        let answer = project.write(
            "decision",
            &json!({"title": "Combinar o item", "agent": format!("- ponto {code}"), "text": "O usuário respondeu ao ponto.", "keys": ["levantamento"],
                "why": "o usuário respondeu", "origin": said, "applies_to": {"files": ["**"]}}),
        );
        let closed = project.write(
            "point",
            &json!({"block": point["block"], "gap": point["gap"], "from": "gap", "status": "closed",
                "closes": code, "result": [answer["id"]], "origin": said}),
        );
        current = closed["point"].clone();
    }
    said
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

/// Uma tarefa do backlog, gravada sem onda: `files` como caminhos soltos e
/// `depends_on` como a lista de ids das tarefas de que ela depende.
/// Devolve o `id` gravado.
fn backlog_task(project: &Project, criterion: u64, said: u64, files: &[&str], depends_on: &[u64]) -> u64 {
    backlog_task_with(project, criterion, said, files, depends_on, &json!({}))
}

/// [`backlog_task`] com os campos de `extra` por cima, como a marca de
/// prioridade.
fn backlog_task_with(project: &Project, criterion: u64, said: u64, files: &[&str], depends_on: &[u64], extra: &Value) -> u64 {
    // `"new": true` marca um arquivo que a tarefa ainda vai criar: sem isso
    // o plano recusa a pergunta de aprovação, porque o arquivo sintético do
    // teste não existe no repositório.
    let files: Vec<Value> = files.iter().map(|f| json!({"path": f, "new": true})).collect();
    let depends_on: Vec<Value> = depends_on.iter().map(|id| json!(id)).collect();
    let mut task = json!({"agent": "- conferir pelo teste", "title": format!("Entregar a tarefa {}",project.log().visible().iter().filter(|e|e.event_type=="task").count()+1), "text": "Tarefa do backlog.", "files": files, "depends_on": depends_on,
            "covers": [criterion], "origin": said});
    for (key, value) in extra.as_object().into_iter().flatten() {
        task[key] = value.clone();
    }
    let written = project.write("task", &task);
    written["id"].as_u64().expect("the recorded task has an id")
}

/// O backlog inteiro, numa spec de seis tarefas com dependências e arquivos
/// declarados, despachada pelo binário de verdade: o nível topológico, a
/// prontidão com desempate e o agrupamento sempre devolvem as mesmas ondas,
/// uma por assunto — o mesmo grafo do teste que antes vivia só como unidade
/// pura do módulo do grafo (`apps/rt/src/shared/dag.rs`), agora conferido na
/// saída de uma rodada de verdade.
#[test]
fn backlog_always_becomes_the_same_batches() {
    let project = Project::new();
    project.open();
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let crit_id = criterion["id"].as_u64().expect("the criterion has an id");

    // 1 e 5 dividem `b.rs`; 4 é a maior parte, sozinha; 2 não compartilha
    // arquivo com ninguém; 3 espera 1 e divide `b.rs` com ela; 6 espera 4,
    // mas não divide arquivo com a onda dela.
    let t1 = backlog_task(&project, crit_id, said, &["a.rs", "b.rs"], &[]);
    let t2 = backlog_task(&project, crit_id, said, &["c.rs"], &[]);
    let t3 = backlog_task(&project, crit_id, said, &["b.rs", "d.rs"], &[t1]);
    let t4 = backlog_task(&project, crit_id, said, &["e.rs", "f.rs", "g.rs"], &[]);
    let t5 = backlog_task(&project, crit_id, said, &["b.rs"], &[]);
    let t6 = backlog_task(&project, crit_id, said, &["h.rs"], &[t4]);

    project.run(&["plan", "--spec", SPEC]);
    approve(&project);

    let before = project.log();
    assert!(before.visible().into_iter().all(|e| e.event_type != "wave"), "no hand-made wave before the round");

    project.run(&["round", "--dispatch", "--spec", SPEC]);

    let after = project.log();
    let waves: Vec<_> = after.visible().into_iter().filter(|e| e.event_type == "wave" && e.str_field("author") == Some("binary")).collect();
    assert_eq!(waves.len(), 3, "três assuntos, uma onda para cada: {waves:?}");

    // O assunto de `b.rs`: 1 e 5 prontas, e 3, que espera só a 1 e divide
    // `b.rs` com ela. Sem teto de tarefas, as três saem na mesma onda.
    let order = waves[0].ints("order");
    assert_eq!(
        order.iter().copied().collect::<BTreeSet<u64>>(),
        BTreeSet::from([t1, t3, t5]),
        "1 e 5 prontas, e 3, que espera só a 1 e divide `b.rs` com ela: {order:?}"
    );
    let at = |task: u64| order.iter().position(|id| *id == task).expect("a tarefa está na onda");
    assert!(at(t1) < at(t3), "a 3 entra depois da 1, de que depende: {order:?}");
    assert_eq!(waves[1].ints("order"), vec![t2], "a 2 não divide arquivo com ninguém: onda dela");
    assert_eq!(waves[2].ints("order"), vec![t4], "a 4 sai sozinha, com os três arquivos dela");

    // A 6 espera a 4, que está numa onda, mas não divide arquivo com ela.
    assert_eq!(after.current(t6).and_then(|t| t.wave()), None, "a 6 fica no backlog, sem onda");
}

/// Duas tarefas soltas no backlog que dividem um arquivo, cada uma com um
/// critério, saem na mesma onda: o binário, despachado pela rodada de
/// verdade, grava o evento de onda com o autor binário, as duas tarefas na
/// ordem de despacho, os critérios que são a união do que elas cobrem e o
/// pronta-quando tirado da prova dos dois critérios, ligadas por " && ".
#[test]
fn binary_writes_the_wave_event_of_the_batch() {
    let project = Project::new();
    project.open();
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let crit_id = criterion["id"].as_u64().expect("the criterion has an id");
    let other = project.write(
        "criterion",
        &json!({"title": "Combinar a despedida", "when": "o programa termina", "then": "a despedida nova aparece",
            "proof": "git status --short", "form": "ubiquitous", "origin": said}),
    );
    let other_id = other["id"].as_u64().expect("the second criterion has an id");

    let t1 = backlog_task(&project, crit_id, said, &["src/b.rs"], &[]);
    let t2 = backlog_task(&project, other_id, said, &["src/b.rs"], &[]);

    project.run(&["plan", "--spec", SPEC]);
    approve(&project);

    let before = project.log();
    assert!(before.visible().into_iter().all(|e| e.event_type != "wave"), "no hand-made wave before the round");

    project.run(&["round", "--dispatch", "--spec", SPEC]);

    let after = project.log();
    let wave = after.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(1)).expect("the batch wave");
    assert_eq!(wave.str_field("author"), Some("binary"), "onda de lote é do binário: {:?}", wave.fields);
    assert_eq!(wave.ints("order"), vec![t1, t2], "as duas tarefas do lote, na ordem de despacho");
    assert_eq!(wave.ints("criteria"), vec![crit_id, other_id], "os critérios são a união do que as tarefas cobrem");
    assert_eq!(wave.str_field("done_when"), Some("git --version && git status --short"), "a prova dos dois critérios cobertos");

    for task in [t1, t2] {
        let task_now = after.current(task).expect("the task");
        assert_eq!(task_now.wave(), Some(1), "a tarefa ganha a onda que a levou");
    }
}

/// As ondas de uma resposta da rodada numa das listas dela (`dispatch`,
/// `analysis`), pelo número.
fn waves_in(out: &Value, key: &str) -> Vec<u64> {
    out[key].as_array().into_iter().flatten().filter_map(|entry| entry["wave"].as_u64()).collect()
}

/// O número e a hora de início (campo 22 de `/proc/<pid>/stat`) do processo
/// deste teste, que fica vivo enquanto ele roda. Fora do Linux não há
/// `/proc`: a hora vai zerada, e lá o binário não confere o par.
fn test_process() -> (u32, u64) {
    let pid = std::process::id();
    let started = std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|raw| raw.rsplit_once(')').and_then(|(_, after)| after.split_whitespace().nth(19)?.parse().ok()))
        .unwrap_or(0);
    (pid, started)
}

/// Cada rodada é um processo novo do binário. Sem o Claude Code acima dela,
/// como no servidor, o envio grava o par do próprio processo da rodada, que
/// termina assim que ela responde, e a rodada seguinte leria a onda como
/// órfã e a reenviaria. Aqui o envio mais recente de cada onda de `waves`
/// ganha uma versão nova, como a volta da onda grava, com o par do processo
/// do teste e o agente lançado: a onda segue em andamento não importa quem
/// lançou a suíte.
fn keep_sent(project: &Project, waves: &[u64]) {
    sent_by(project, waves, test_process());
}

/// O envio mais recente de cada onda de `waves` ganha uma versão nova, como
/// a volta da onda grava, com o par `(pid, started)` de quem o mandou e o
/// agente dela lançado, como a observação do hospedeiro grava.
fn sent_by(project: &Project, waves: &[u64], (pid, started): (u32, u64)) {
    let log = project.log();
    let last = log.last_by_wave("send");
    for wave in waves {
        let sent = log.get(*last.get(wave).expect("a onda que saiu tem envio")).expect("o envio");
        let mut draft: serde_json::Map<String, Value> = sent
            .fields
            .iter()
            .filter(|(key, _)| !["v", "id", "code", "at", "type", "search"].contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        draft.insert("replaces".into(), json!(sent.id));
        draft.insert("claude_pid".into(), json!(pid));
        draft.insert("claude_started".into(), json!(started));
        draft.insert("agent_launched".into(), json!(true));
        project.seed("send", &Value::Object(draft));
    }
}

/// Uma rodada que solta o que estiver pronto, como quem conduz a obra faz: as
/// ondas prontas saem na mesma rodada, sem escolha de quem conduz. Devolve as
/// ondas que pediram a escolha — nenhuma, a rodada não pede mais — e as que
/// saíram; as que saíram ficam em andamento pelo processo do teste
/// ([`keep_sent`]).
fn dispatch_ready(project: &Project) -> (Vec<u64>, Vec<u64>) {
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    let asked = waves_in(&first, "analysis");
    let out = waves_in(&first, "dispatch");
    keep_sent(project, &out);
    (asked, out)
}

/// Uma spec aprovada com uma tarefa no backlog para cada lista de arquivos de
/// `files`, sem dependência entre elas. Devolve o projeto, o critério, a fala
/// do usuário e as tarefas, na ordem de `files`.
fn backlog_project(files: &[&[&str]]) -> (Project, u64, u64, Vec<u64>) {
    marked_backlog_project(files, &[])
}

#[cfg(target_os = "linux")]
#[test]
fn a_short_lived_round_uses_its_exact_session_owner_and_the_dispatch_hook_accepts_it() {
    let (project, _, _, _) = backlog_project(&[&["src/a.rs"]]);
    let pid = std::process::id();
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let started: u64 = stat.rsplit_once(')').unwrap().1.split_whitespace().nth(19).unwrap().parse().unwrap();
    let mut hash = mustard_core::io::sha256::Sha256::new(); hash.update(SESSION.as_bytes());
    let folder = project.root.join(".claude/mustard/host-observations"); std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("claude-{}.json", hash.hex_digest())), json!({"host_pid":pid,"host_started":started}).to_string()).unwrap();
    let output = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run","round","--dispatch","--spec",SPEC]).current_dir(&project.root)
        .env("MUSTARD_SESSION_ID",SESSION).env("MUSTARD_RT_DELEGATED","1").env("CLAUDE_CONFIG_DIR",&project.home)
        .env_remove("CODEX_THREAD_ID").env_remove("TYPESAFE_API_KEY").env_remove("MUSTARD_JEV_URL").output().unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ok"], true, "{report}");
    let log = project.log(); let send = log.last_by_wave("send")[&1];
    assert_eq!(log.get(send).unwrap().int("claude_pid"), Some(u64::from(pid)));
    let verdict = dispatch_new_agent(&project);
    assert_ne!(verdict["hookSpecificOutput"]["permissionDecision"], "deny", "{verdict}");
}

/// [`backlog_project`] com a marca de prioridade nas tarefas das posições
/// `marked` de `files`.
fn marked_backlog_project(files: &[&[&str]], marked: &[usize]) -> (Project, u64, u64, Vec<u64>) {
    let project = Project::new();
    project.open();
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let crit_id = criterion["id"].as_u64().expect("the criterion has an id");
    let tasks = files
        .iter()
        .enumerate()
        .map(|(at, f)| {
            let extra = if marked.contains(&at) { json!({"priority": PRIORITY}) } else { json!({}) };
            backlog_task_with(&project, crit_id, said, f, &[], &extra)
        })
        .collect();
    project.run(&["plan", "--spec", SPEC]);
    approve(&project);
    (project, crit_id, said, tasks)
}

/// Uma tarefa semeada no backlog depois da aprovação, sem onda, como o
/// conserto a deixa. Devolve o `id` gravado.
fn seed_backlog_task(project: &Project, criterion: u64, said: u64, files: &[&str]) -> u64 {
    seed_task_with(project, criterion, said, files, &json!({}))
}

/// [`seed_backlog_task`] com os campos de `extra` por cima, como a marca de
/// prioridade ou a dependência.
fn seed_task_with(project: &Project, criterion: u64, said: u64, files: &[&str], extra: &Value) -> u64 {
    let files: Vec<Value> = files.iter().map(|f| json!({"path": f, "new": true})).collect();
    let mut task = json!({"title": "Entregar a tarefa", "agent": "- conferir pelo teste", "author": "assistant", "text": "Tarefa do backlog.", "files": files, "depends_on": [],
            "covers": [criterion], "origin": said});
    for (key, value) in extra.as_object().into_iter().flatten() {
        task[key] = value.clone();
    }
    project.seed("task", &task)
}

/// A onda que levou a tarefa `task`, pela versão vigente dela.
fn wave_of(project: &Project, task: u64) -> Option<u64> {
    project.log().current(task).and_then(|t| t.wave())
}

/// A tarefa com o curinga da árvore inteira, ao lado de duas prontas com
/// arquivos próprios: sai um lote só com ela, e as outras esperam. Com a
/// onda dela em andamento, nada mais sai.
#[test]
fn task_with_a_wildcard_goes_out_alone() {
    let (project, _, _, tasks) = backlog_project(&[&["**"], &["a.rs"], &["b.rs"]]);

    let (asked, out) = dispatch_ready(&project);
    let star = wave_of(&project, tasks[0]).expect("a tarefa do curinga vira onda");
    assert!(asked.is_empty(), "a rodada não pede escolha a quem conduz: {asked:?}");
    assert_eq!(out, vec![star], "só a onda do curinga sai");
    let log = project.log();
    let star_wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(star)).expect("a onda do curinga");
    assert_eq!(star_wave.ints("order"), vec![tasks[0]], "o lote do curinga leva só ele");
    for other in &tasks[1..] {
        assert_ne!(wave_of(&project, *other), Some(star), "nenhuma outra tarefa entra no lote do curinga");
    }

    // A onda do curinga está em andamento: a das outras duas espera.
    let (asked, out) = dispatch_ready(&project);
    assert!(asked.is_empty() && out.is_empty(), "com o curinga em andamento nada mais sai: {asked:?} {out:?}");
}

/// O outro lado: com uma onda em andamento, a tarefa do curinga que chega ao
/// backlog não vira onda e não sai ao lado dela: fica no backlog, sem onda,
/// até a vaga ficar sozinha.
#[test]
fn task_with_a_wildcard_goes_out_alone_and_waits_for_the_wave_in_progress() {
    let (project, crit, said, tasks) = backlog_project(&[&["a.rs"]]);
    let (_, out) = dispatch_ready(&project);
    let first = wave_of(&project, tasks[0]).expect("a primeira tarefa vira onda");
    assert_eq!(out, vec![first], "a onda de a.rs sai");

    let star = seed_backlog_task(&project, crit, said, &["**"]);
    let (asked, out) = dispatch_ready(&project);
    assert_eq!(wave_of(&project, star), None, "a tarefa do curinga espera no backlog, sem onda");
    assert!(asked.is_empty() && out.is_empty(), "o curinga não sai ao lado de a.rs em andamento: {asked:?} {out:?}");
}

/// Um padrão cruza com todo arquivo que ele casa: `src/**` e `src/a.rs`
/// caem na mesma onda. Com `src/**` em andamento, a tarefa de `src/b.rs`, que
/// ele casa, sai ao lado dele, como a de `docs/`: o padrão de uma onda em
/// andamento não segura arquivo, só o curinga da árvore inteira sai sozinho.
#[test]
fn task_with_a_wildcard_goes_out_alone_and_the_pattern_joins_the_file_it_matches() {
    let own = ["src/a.rs", "lib/1.rs", "lib/2.rs", "lib/3.rs", "lib/4.rs"];
    let (project, crit, said, tasks) = backlog_project(&[&own, &["src/**"]]);
    let (_, out) = dispatch_ready(&project);
    let joined = wave_of(&project, tasks[0]).expect("a tarefa de src/a.rs vira onda");
    assert_eq!(wave_of(&project, tasks[1]), Some(joined), "src/** junta com src/a.rs, que ele casa");
    assert_eq!(out, vec![joined]);

    let inside = seed_backlog_task(&project, crit, said, &["src/b.rs"]);
    let docs = ["docs/1.md", "docs/2.md", "docs/3.md", "docs/4.md", "docs/5.md", "docs/6.md"];
    let outside = seed_backlog_task(&project, crit, said, &docs);
    let (_, out) = dispatch_ready(&project);
    let inside_wave = wave_of(&project, inside).expect("src/b.rs sai ao lado de src/** em andamento");
    let outside_wave = wave_of(&project, outside).expect("docs vira onda");
    assert_eq!(out, vec![inside_wave, outside_wave], "as duas saem, cada uma na sua onda");
}

/// Uma spec com duas tarefas de `a.rs` cuja onda 1 saiu e foi removida pelo
/// `run write`, sem entrega. Devolve o projeto, as tarefas e a saída da
/// remoção.
fn removed_first_wave() -> (Project, Vec<u64>, Value) {
    let (project, _, _, tasks) = backlog_project(&[&["a.rs"], &["a.rs"]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![1], "as duas tarefas de a.rs saem na onda 1");
    let wave = project.log().visible().into_iter().find(|e| e.event_type == "wave").expect("a onda 1").id;
    let removal = project.write("remove", &json!({"targets": [wave], "reason": "A onda saiu do plano."}));
    (project, tasks, removal)
}

/// A onda que saiu e foi removida sem entrega devolve as tarefas dela ao
/// backlog na mesma gravação: cada uma ganha a versão sem onda, e a leitura
/// do backlog a mostra sem onda.
#[test]
fn removing_a_sent_wave_without_a_delivery_gives_its_tasks_back_to_the_backlog() {
    let (project, tasks, removal) = removed_first_wave();

    let log = project.log();
    let current: BTreeSet<u64> = tasks.iter().map(|task| log.current(*task).expect("a tarefa segue").id).collect();
    let returned: BTreeSet<u64> = removal["returned"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    assert_eq!(returned, current, "a remoção grava junto a versão sem onda de cada tarefa: {removal}");
    for task in &tasks {
        assert_eq!(wave_of(&project, *task), None, "a tarefa da onda removida volta sem onda");
    }
    let backlog = project.run(&["read", "backlog", "--spec", SPEC]);
    let shown: BTreeMap<u64, Value> = backlog["events"]
        .as_array()
        .expect("the backlog lines")
        .iter()
        .map(|line| (line["id"].as_u64().expect("the task number"), line["wave"].clone()))
        .collect();
    assert_eq!(shown, current.iter().map(|id| (*id, Value::Null)).collect(), "o backlog mostra as duas sem onda: {backlog}");
}

/// Depois da remoção, a rodada numera a onda nova adiante da removida, e não
/// com o número dela: a onda nova sai, e o pedido dela lista só as tarefas
/// da ordem dela.
#[test]
fn after_a_wave_is_removed_the_next_one_takes_a_new_number_and_its_request_lists_only_its_tasks() {
    let (project, tasks, _) = removed_first_wave();

    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![2], "a onda nova nasce adiante da removida");
    let log = project.log();
    let codes = log.codes();
    let wave = log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(2)).expect("a onda 2");
    let order: BTreeSet<&String> = wave.ints("order").iter().filter_map(|id| codes.get(id)).collect();
    assert_eq!(order.len(), tasks.len(), "a onda nova leva as duas tarefas devolvidas: {:?}", wave.fields);
    let out = project.command(&["run", "read", "request-2", "--spec", SPEC], "");
    let request = String::from_utf8_lossy(&out.stdout);
    let listed: BTreeSet<&String> = tasks.iter().filter_map(|id| codes.get(id)).filter(|code| request.contains(code.as_str())).collect();
    assert_eq!(listed, order, "o pedido lista só as tarefas da ordem: {request}");
}

// ---------------------------------------------------------------------------
// A montagem da onda pelo Jev, com um Jev de mentira
// ---------------------------------------------------------------------------

/// Um Jev de mentira: um serviço HTTP em 127.0.0.1 que guarda cada pedido e
/// devolve a resposta fixa que `answer` monta a partir do corpo dele.
struct FakeJev {
    url: String,
    received: Arc<Mutex<Vec<Value>>>,
}

impl FakeJev {
    fn start(answer: impl Fn(&Value) -> (u16, Value) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the fake service binds");
        let url = format!("http://{}/v1/systemone", listener.local_addr().expect("address"));
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        let answer = Arc::new(answer);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (log, answer) = (Arc::clone(&log), Arc::clone(&answer));
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().expect("stream"));
                    let mut length = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        let line = line.trim_end().to_ascii_lowercase();
                        if line.is_empty() {
                            break;
                        }
                        if let Some(value) = line.strip_prefix("content-length:") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; length];
                    if reader.read_exact(&mut body).is_err() {
                        return;
                    }
                    let Ok(asked) = serde_json::from_slice::<Value>(&body) else {
                        return;
                    };
                    log.lock().unwrap().push(asked.clone());
                    let (status, reply) = answer(&asked);
                    let reply = reply.to_string();
                    let head = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", reply.len());
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(reply.as_bytes());
                });
            }
        });
        Self { url, received }
    }

    /// Um Jev que responde a cada pergunta de tipo com `kind_of(posição)` — o
    /// tipo e a confiança — e a cada pergunta de bloqueio com
    /// `clash_of(onda, posição)`, e conta 1.500 tokens de entrada. A posição é
    /// a da tarefa entre as do pedido, da de número mais baixo para a mais
    /// alta: o teste não depende dos números que a spec deu.
    fn judging(kind_of: impl Fn(usize) -> (&'static str, f64) + Send + Sync + 'static, clash_of: impl Fn(u64, usize) -> f64 + Send + Sync + 'static) -> Self {
        Self::start(move |asked| {
            let keys: Vec<&String> = asked["questions"].as_object().expect("the questions").keys().collect();
            let position = |task: &str| {
                asked["state"]["backlog"][format!("t{task}")]["title"]
                    .as_str()
                    .and_then(|s| s.split_whitespace().last())
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(1)
                    .saturating_sub(1)
            };
            let mut answers = serde_json::Map::new();
            for key in keys {
                if let Some(task) = key.strip_prefix("tipo_t") {
                    let (kind, confidence) = kind_of(position(task));
                    answers.insert(key.clone(), {
                        let criteria = asked["questions"][key]["criteria"].as_object().unwrap();
                        let chances: serde_json::Map<_, _> = criteria
                            .keys()
                            .map(|k| (k.clone(), json!(if k == kind { confidence } else { (1.0 - confidence) / (criteria.len() - 1) as f64 })))
                            .collect();
                        json!({"type":"choice","choice":kind,"confidence":confidence,"probabilities":chances})
                    });
                } else if let Some((wave, task)) = key.strip_prefix("blk_w").and_then(|rest| rest.split_once("_t")) {
                    let chance = clash_of(wave.parse().expect("a wave"), position(task));
                    answers.insert(key.clone(), json!({"type": "noul", "noul": chance}));
                } else if key.starts_with('i') {
                    answers.insert(key.clone(), json!({"type": "noul", "noul": 0.5}));
                }
            }
            (200, json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 1500, "output_tokens": 0}}))
        })
    }

    /// Os pedidos da montagem da onda: os que perguntam o tipo das tarefas.
    fn requests(&self) -> Vec<Value> {
        self.all_requests().into_iter().filter(|asked| asked["questions"].as_object().is_some_and(|q| q.keys().any(|key| key.starts_with("tipo_")))).collect()
    }

    /// Todos os pedidos que o serviço recebeu, os dos itens do pedido da onda
    /// inclusive.
    fn all_requests(&self) -> Vec<Value> {
        self.received.lock().unwrap().clone()
    }
}

/// As ondas de lote da spec, com a ordem de tarefas de cada uma.
fn batch_orders(project: &Project) -> Vec<Vec<u64>> {
    let log = project.log();
    log.visible().into_iter().filter(|e| e.event_type == "wave" && e.str_field("author") == Some("binary")).map(|wave| wave.ints("order")).collect()
}

/// O recibo, com a certeza `certainty`, de cada pedido de reserva que a
/// resposta `out` da rodada trouxe, pela porta do `run knowledge
/// --complete-session`: o tipo `feature` na pergunta do tipo, e não nas
/// outras. Devolve quantos recibos mandou.
fn answer_reserves(project: &Project, out: &Value, certainty: &str) -> usize {
    let requests = out["reserves"].as_array().cloned().unwrap_or_default();
    for (at, request) in requests.iter().enumerate() {
        let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_default();
        let answers: serde_json::Map<String, Value> = (1..=request["questions"].as_u64().unwrap_or_default())
            .map(|number| {
                let id = format!("q{number}");
                let text = prompt.lines().find_map(|line| line.strip_prefix(&format!("{id}: "))).unwrap_or_default();
                let choice = if text.contains("What kind of work") { "feature" } else { "no" };
                (id, json!({"choice": choice, "certainty": certainty}))
            })
            .collect();
        let receipt = json!({"request_id": request["request_id"], "model": "inherit", "input_tokens": null, "output_tokens": null,
            "result": {"answers": answers}});
        let path = project.root.join(format!("recibo-{at}.json"));
        std::fs::write(&path, receipt.to_string()).expect("the receipt");
        let done = project.command(&["run", "knowledge", "--complete-session", path.to_str().unwrap()], "");
        assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stdout));
    }
    requests.len()
}

/// As chamadas `wave assembly` que o programa observou.
fn assembly_calls(project: &Project) -> Vec<serde_json::Map<String, Value>> {
    project.calls_of("wave assembly")
}

/// As tarefas de `backlog_project` que o Jev de mentira julga, pelo número
/// da tarefa: uma spec aprovada com uma tarefa por lista de arquivos e o
/// Jev ligado.
fn judged_project(files: &[&[&str]], jev: &FakeJev) -> (Project, u64, u64, Vec<u64>) {
    let (mut project, crit, said, tasks) = backlog_project(files);
    project.jev = Some(jev.url.clone());
    (project, crit, said, tasks)
}

/// Três tarefas do mesmo tipo, cada uma num arquivo só dela, saem na mesma
/// onda: o Jev junta o assunto, e o arquivo não. O Jev recebe o backlog
/// inteiro numa chamada só, e a chamada fica gravada com os tokens, o custo e
/// o modelo.
#[test]
fn tasks_of_one_kind_share_a_wave_without_a_file_in_common_and_the_call_is_recorded() {
    let jev = FakeJev::judging(|_| ("feature", 0.9), |_, _| 0.0);
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"], &["d.rs"]], &jev);

    project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(batch_orders(&project), vec![tasks.clone()], "uma onda com as três do mesmo tipo");
    let asked = jev.requests();
    assert_eq!(asked.len(), 3, "um perfil estável por tarefa");
    assert_eq!(asked[0]["model"], json!("jev-1.13.0"));
    assert_eq!(asked[0]["questions"].as_object().unwrap().len(), 1, "o perfil pergunta só o tipo: {}", asked[0]);
    let calls = assembly_calls(&project);
    assert_eq!(calls.len(), 1, "{calls:?}");
    let call = &calls[0];
    assert_eq!(
        (call["filter"].clone(), call["tokens"].clone(), call["cost_micro_usd"].clone(), call["model"].clone()),
        (json!("jev"), json!(4500), json!(189), json!("jev-1.13.0")),
        "{call:?}"
    );
    assert_eq!((call["requests"].clone(), call["candidates"].clone(), call["returned"].clone()), (json!(3), json!(3), json!(3)));
}

/// A onda sem nota de dificuldade sai no modelo do projeto, com o Jev ligado
/// ou não, e ainda que o `mustard.json` traga a chave `light_model` de antes:
/// o envio grava o modelo do projeto e o esforço `xhigh`, o pedido da onda diz
/// os dois, a resposta do despacho traz o modelo de cada onda, o texto do
/// próximo passo manda lançar o agente com ele, e o Jev não pergunta mais se a
/// tarefa é mecânica.
#[test]
fn a_wave_without_a_grade_goes_out_on_the_project_model_and_the_dispatch_names_it() {
    let jev = FakeJev::judging(
        |position| {
            if position == 0 { ("defect", 0.9) } else { ("feature", 0.9) }
        },
        |_, _| 0.0,
    );
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"], &["d.rs"]], &jev);
    let config_path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&config_path).expect("config")).expect("json");
    config["agents"] = json!({"model": "opus", "effort": "low", "light_model": "claude-haiku-5"});
    std::fs::write(&config_path, config.to_string()).expect("config");

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(batch_orders(&project), vec![vec![tasks[0]], vec![tasks[1], tasks[2]]], "{out}");
    let log = project.log();
    for wave in [1, 2] {
        let send = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(wave)).expect("the send");
        assert_eq!((send.str_field("model"), send.str_field("effort")), (Some("opus"), Some("xhigh")), "a onda {wave} vai no modelo do projeto: {out}");
        let prompt = send.str_field("text").expect("the request");
        assert!(prompt.contains("Modelo desta onda: opus. Esforço: xhigh."), "o pedido diz o modelo do projeto: {prompt}");
    }
    let dispatch = out["dispatch"].as_array().expect("the dispatch");
    assert_eq!(dispatch.len(), 2, "{out}");
    assert!(dispatch.iter().all(|sent| sent["model"] == json!("opus")), "o despacho traz o modelo de cada onda: {out}");
    let next = out["next"].as_str().expect("the next step");
    assert!(next.contains("`model`"), "o próximo passo manda lançar com o modelo da entrada: {next}");
    for asked in jev.requests() {
        let questions = asked["questions"].as_object().expect("the questions");
        assert!(questions.keys().all(|key| !key.starts_with("mec_")), "o Jev não é perguntado sobre onda mecânica: {asked}");
    }
}

/// Sem chave do Jev a onda também sai no modelo do projeto, e o envio e o
/// despacho o gravam.
#[test]
fn without_a_key_the_wave_goes_out_on_the_project_model() {
    let (project, _, _, _) = backlog_project(&[&["a.rs"]]);

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    let log = project.log();
    let send = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the send");
    assert_eq!(send.str_field("model"), Some("sonnet"), "{out}");
    assert_eq!(out["dispatch"][0]["model"], json!("sonnet"), "{out}");
}

/// Two waves formed by the classifier, one of two simple tasks and one of a
/// task that spans several modules: the simple one goes out on Sonnet, the
/// other on Opus, both at effort xhigh, and the dispatch names each model.
#[test]
fn each_formed_wave_goes_out_on_the_model_of_its_hardest_task() {
    let project = Project::new();
    project.open();
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let criterion = criterion["id"].as_u64().expect("the criterion has an id");
    let grade = |name: &str, position: u64, level: u64| {
        json!({"group": {"name": name, "position": position, "formed": true, "difficulty": {"level": level, "confidence": 0.9}}})
    };
    let simple = [
        backlog_task_with(&project, criterion, said, &["a.rs"], &[], &grade("Soma", 1, 0)),
        backlog_task_with(&project, criterion, said, &["b.rs"], &[], &grade("Soma", 2, 0)),
    ];
    let hard = backlog_task_with(&project, criterion, said, &["c.rs"], &[], &grade("Fluxo", 1, 3));
    project.run(&["plan", "--spec", SPEC]);
    approve(&project);

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    let log = project.log();
    let wave_of = |task: u64| log.current(task).and_then(|t| t.wave()).unwrap_or_else(|| panic!("the task left in a wave: {out}"));
    assert_eq!(wave_of(simple[0]), wave_of(simple[1]), "{out}");
    let sent = |wave: u64| log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(wave)).cloned().expect("the send");
    let (easy, heavy) = (sent(wave_of(simple[0])), sent(wave_of(hard)));
    assert_eq!((easy.str_field("model"), easy.str_field("effort")), (Some("sonnet"), Some("xhigh")), "{out}");
    assert_eq!((heavy.str_field("model"), heavy.str_field("effort")), (Some("opus"), Some("xhigh")), "{out}");
    assert!(easy.str_field("text").is_some_and(|text| text.contains("Modelo desta onda: sonnet. Esforço: xhigh.")), "{easy:?}");
    let named: BTreeMap<u64, Value> = out["dispatch"].as_array().expect("the dispatch").iter().map(|d| (d["wave"].as_u64().unwrap(), d["model"].clone())).collect();
    assert_eq!(named, BTreeMap::from([(wave_of(simple[0]), json!("sonnet")), (wave_of(hard), json!("opus"))]), "{out}");
}

/// O defeito sai antes da limpeza, ainda que a limpeza tenha o número mais
/// baixo: com uma vaga só, é o defeito que a ocupa.
///
/// Três tarefas, e não duas: a chamada que decide o tipo de cada uma só sai
/// com mais de duas candidatas, e sem tipo não há defeito nem limpeza.
#[test]
fn a_defect_leaves_before_a_cleanup_even_with_the_lower_number() {
    let jev = FakeJev::judging(
        |at| {
            if at == 1 { ("defect", 0.9) } else { ("test_cleanup", 0.9) }
        },
        |_, _| 0.0,
    );
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"], &["e.rs"]], &jev);
    std::fs::write(
        project.root.join("mustard.json"),
        json!({
        "language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
        "ai": {"fallback": true}, "search": {"filter": "jev"},
        "judgement": {"wave-planning": {"filter": "jev"}, "context": {"filter": "jev"}},
        "lintCommand": "git --version", "maxCompilingWaves": 1})
        .to_string(),
    )
    .expect("config");
    let (cleanup, defect, other) = (tasks[0], tasks[1], tasks[2]);

    project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(batch_orders(&project), vec![vec![defect]], "o defeito ocupa a única vaga");
    assert_eq!(wave_of(&project, cleanup), None, "a limpeza, de número mais baixo, espera");
    assert_eq!(wave_of(&project, other), None, "a outra limpeza também espera");
}

/// A tarefa de tipo incerto — confiança abaixo de 0,5 — sai sozinha, na onda
/// dela; as de tipo certo, do mesmo tipo, saem juntas.
#[test]
fn a_task_of_an_uncertain_kind_goes_alone() {
    let jev = FakeJev::judging(|at| ("feature", if at == 0 { 0.4 } else { 0.9 }), |_, _| 0.0);
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"], &["d.rs"]], &jev);
    let mut by_number = tasks.clone();
    by_number.sort_unstable();

    project.run(&["round", "--dispatch", "--spec", SPEC]);

    let orders = batch_orders(&project);
    assert_eq!(orders.len(), 2, "{orders:?}");
    assert!(orders.contains(&vec![by_number[0]]), "a incerta sai sozinha: {orders:?}");
    assert!(orders.contains(&vec![by_number[1], by_number[2]]), "as certas, juntas: {orders:?}");
}

/// Uma tarefa de seis arquivos que nenhuma onda em andamento declara, e que o
/// Jev diz mudar o mesmo que a onda em andamento, espera no backlog; com a
/// chance baixa, sai.
#[test]
fn a_high_clash_with_the_wave_in_progress_holds_a_task_that_shares_no_file_with_it() {
    let (mut project, crit, said, tasks) = backlog_project(&[&["billing/a.rs"]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![wave_of(&project, tasks[0]).expect("a primeira onda")], "a onda de a.rs sai e fica no ar");
    let held = seed_backlog_task(&project, crit, said, &["billing/x1.rs", "billing/x2.rs", "billing/x3.rs", "billing/x4.rs", "billing/x5.rs", "billing/x6.rs"]);

    let strict = FakeJev::judging(|_| ("feature", 0.9), |_, _| 0.8);
    project.jev = Some(strict.url.clone());
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(wave_of(&project, held), None, "o choque de 0,8 segura a tarefa que não divide arquivo");
    assert_eq!(strict.requests()[0]["questions"].as_object().unwrap().len(), 1, "perfil intrínseco; o bloqueio tem evidência própria");

    let loose = FakeJev::judging(|_| ("feature", 0.9), |_, _| 0.1);
    project.jev = Some(loose.url.clone());
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(wave_of(&project, held).is_some(), "com o choque de 0,1 a tarefa sai");
}

/// A onda não tem teto de tamanho: seis tarefas do mesmo tipo, cada uma com
/// sete arquivos só dela, saem juntas numa onda. Quem segura o tamanho é o
/// sinal de conclusão dentro da conversa do agente, não a montagem.
#[test]
fn a_wave_takes_every_task_of_its_kind_without_a_size_ceiling() {
    let jev = FakeJev::judging(|_| ("feature", 0.9), |_, _| 0.0);
    let files: Vec<Vec<String>> = (0..6).map(|task| (0..7).map(|file| format!("t{task}/f{file}.rs")).collect()).collect();
    let paths: Vec<Vec<&str>> = files.iter().map(|own| own.iter().map(String::as_str).collect()).collect();
    let lists: Vec<&[&str]> = paths.iter().map(Vec::as_slice).collect();
    let (project, _, _, tasks) = judged_project(&lists, &jev);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![tasks.clone()], "as seis do mesmo tipo saem numa onda só");
    assert_eq!(jev.requests().len(), 6, "um perfil por tarefa");
}

/// O tamanho nunca separa a onda: três tarefas do mesmo arquivo saem numa
/// onda só sem chave, com a chamada recusada e com o Jev julgando as três do
/// mesmo tipo.
#[test]
fn the_size_never_splits_the_wave_with_or_without_the_jev() {
    let same: [&[&str]; 3] = [&["a.rs"], &["a.rs"], &["a.rs"]];
    let (project, _, _, tasks) = backlog_project(&same);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![tasks.clone()], "sem chave");

    // A chamada recusada vai à reserva, e o recibo incerto deixa a montagem
    // pelo arquivo.
    let refusing = FakeJev::start(|_| (401, json!({})));
    let (refused, _, _, tasks) = judged_project(&same, &refusing);
    let waiting = refused.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(batch_orders(&refused).is_empty(), "the backlog waits for the receipt: {waiting}");
    assert_eq!(answer_reserves(&refused, &waiting, "unsure"), 1, "{waiting}");
    refused.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&refused), vec![tasks.clone()], "chamada recusada");

    let jev = FakeJev::judging(|_| ("feature", 0.9), |_, _| 0.0);
    let (judged, _, _, tasks) = judged_project(&same, &jev);
    judged.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&judged), vec![tasks.clone()], "com o Jev");
}

/// Sem chave a montagem é a de hoje — por arquivo, nenhuma chamada —, e com
/// o Jev recusando a chamada também, depois do recibo incerto da reserva:
/// duas tarefas que dividem um arquivo saem juntas, a de outro arquivo sai na
/// sua, e a chamada que falhou fica gravada com o motivo, uma vez só.
#[test]
fn without_a_key_or_with_a_refused_call_the_assembly_is_the_one_by_file() {
    let files: [&[&str]; 3] = [&["a.rs"], &["a.rs", "b.rs"], &["c.rs"]];
    let (project, _, _, tasks) = backlog_project(&files);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    let by_file = vec![vec![tasks[0], tasks[1]], vec![tasks[2]]];
    assert_eq!(batch_orders(&project), by_file, "sem chave");
    assert!(assembly_calls(&project).is_empty(), "sem chave nada é perguntado");

    let refusing = FakeJev::start(|_| (401, json!({})));
    let (refused, _, _, tasks) = judged_project(&files, &refusing);
    let waiting = refused.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(batch_orders(&refused).is_empty(), "the backlog waits for the receipt: {waiting}");
    assert_eq!(answer_reserves(&refused, &waiting, "unsure"), 1, "{waiting}");
    refused.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&refused), vec![vec![tasks[0], tasks[1]], vec![tasks[2]]], "chamada recusada");
    let calls = assembly_calls(&refused);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0]["filter"], json!("jev:key_refused"), "{calls:?}");
}

// ---------------------------------------------------------------------------
// A marca de prioridade na montagem das ondas
// ---------------------------------------------------------------------------

/// Deixa o projeto com uma vaga só: uma onda por vez.
fn one_slot(project: &Project) {
    std::fs::write(
        project.root.join("mustard.json"),
        json!({
        "language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
        "ai": {"fallback": true}, "search": {"filter": "jev"},
        "judgement": {"wave-planning": {"filter": "jev"}, "context": {"filter": "jev"}},
        "lintCommand": "git --version", "maxCompilingWaves": 1})
        .to_string(),
    )
    .expect("config");
}

/// Três tarefas prontas, cada uma no seu arquivo, e a de número maior com a
/// marca de prioridade: com uma vaga só, é ela que sai, sem o Jev e com o
/// Jev de teste, que a julga limpeza, o último tipo da ordem fixa.
#[test]
fn a_marked_task_with_the_highest_number_leaves_first_with_and_without_the_jev() {
    let files: [&[&str]; 3] = [&["a.rs"], &["b.rs"], &["c.rs"]];
    let (project, _, _, tasks) = marked_backlog_project(&files, &[2]);
    one_slot(&project);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![vec![tasks[2]]], "sem o Jev, a marcada ocupa a vaga");

    let jev = FakeJev::judging(
        |at| {
            if at == 2 { ("test_cleanup", 0.9) } else { ("defect", 0.9) }
        },
        |_, _| 0.0,
    );
    let (mut judged, _, _, tasks) = marked_backlog_project(&files, &[2]);
    judged.jev = Some(jev.url.clone());
    one_slot(&judged);
    judged.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&judged), vec![vec![tasks[2]]], "com o Jev, a marcada passa à frente dos defeitos");
    assert!(jev.requests().iter().all(|asked| !asked.to_string().contains(PRIORITY)), "o quadro mandado ao Jev não leva a marca: {:?}", jev.requests());
}

/// A tarefa marcada pequena, de um arquivo só, sai ao lado de uma onda em
/// andamento sem arquivo em comum; a não marcada independente usa outra
/// vaga, mantendo a prioridade de despacho.
#[test]
fn independent_small_tasks_leave_with_priority_preserved() {
    let (project, crit, said, tasks) = backlog_project(&[&["a.rs"]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![wave_of(&project, tasks[0]).expect("a primeira onda")], "a onda de a.rs sai e fica no ar");
    let plain = seed_backlog_task(&project, crit, said, &["b.rs"]);
    let marked = seed_task_with(&project, crit, said, &["c.rs"], &json!({"priority": PRIORITY}));

    let (_, out) = dispatch_ready(&project);
    let wave = wave_of(&project, marked).expect("a marcada pequena vira onda");
    let plain_wave=wave_of(&project,plain).expect("independent small work uses the other slot");
    assert_eq!(out, vec![wave,plain_wave], "priority is preserved while both free slots get independent work");
}

/// A marcada que espera uma tarefa ainda aberta fica no backlog: a marca não
/// passa por cima do `depends_on`.
#[test]
fn a_marked_task_with_an_open_dependency_waits() {
    let (project, crit, said, tasks) = backlog_project(&[&["a.rs"]]);
    let marked = seed_task_with(&project, crit, said, &["b.rs"], &json!({"priority": PRIORITY, "depends_on": [tasks[0]]}));
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![wave_of(&project, tasks[0]).expect("a dependência vira onda")], "só a dependência sai");
    assert_eq!(wave_of(&project, marked), None, "a marcada espera a dependência");

    let (_, out) = dispatch_ready(&project);
    assert!(out.is_empty(), "com a dependência em andamento, a marcada segue esperando: {out:?}");
    assert_eq!(wave_of(&project, marked), None);
}

/// A não marcada de número menor que divide `a.rs` com a marcada sai depois
/// dela: sem o Jev, as duas vão na mesma onda, com a marcada à frente; com o
/// Jev, de tipos diferentes, a marcada sai na primeira onda e a outra na
/// seguinte, na mesma rodada.
///
/// Com o Jev são três candidatas, e não duas: a chamada que decide o tipo de
/// cada uma só sai com mais de duas, e sem tipo as duas iriam na mesma onda,
/// que é justamente o caso sem Jev deste teste.
#[test]
fn an_unmarked_task_with_a_lower_number_sharing_a_file_leaves_after_the_marked_one() {
    let files: [&[&str]; 2] = [&["a.rs"], &["a.rs"]];
    let (project, _, _, tasks) = marked_backlog_project(&files, &[1]);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![vec![tasks[1], tasks[0]]], "a marcada à frente na mesma onda");

    let jev = FakeJev::judging(
        |at| {
            if at == 0 { ("defect", 0.9) } else { ("feature", 0.9) }
        },
        |_, _| 0.0,
    );
    let judged_files: [&[&str]; 3] = [&["a.rs"], &["a.rs"], &["c.rs"]];
    let (judged, _, _, tasks) = {
        let (mut project, crit, said, tasks) = marked_backlog_project(&judged_files, &[1]);
        project.jev = Some(jev.url.clone());
        (project, crit, said, tasks)
    };
    judged.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(
        batch_orders(&judged),
        vec![vec![tasks[1], tasks[2]], vec![tasks[0]]],
        "a marcada sai na frente, e o defeito de número menor logo atrás"
    );
}

// ---------------------------------------------------------------------------
// Os itens do pedido pelo Jev, de ponta a ponta
// ---------------------------------------------------------------------------

/// Um Jev de mentira que responde ao tipo de cada tarefa (um recurso certo) e
/// à pergunta de cada item pela chance que `chance_of` dá ao título dele.
fn judging_items(chance_of: impl Fn(&str) -> f64 + Send + Sync + 'static) -> FakeJev {
    FakeJev::start(move |asked| {
        let questions = asked["questions"].as_object().expect("the questions");
        let mut answers = serde_json::Map::new();
        for key in questions.keys() {
            if key.starts_with("tipo_") {
                answers.insert(key.clone(), json!({"type":"choice","choice":"feature","confidence":0.9,"probabilities":{"feature":0.9,"defect":0.025,"text_fix":0.025,"remove_unused":0.025,"test_cleanup":0.025}}));
            } else {
                let title = asked["state"]["items"][key]["title"].as_str().unwrap_or_default();
                answers.insert(key.clone(), json!({"type": "noul", "noul": chance_of(title)}));
            }
        }
        (200, json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 1500, "output_tokens": 0}}))
    })
}

/// As chamadas `wave items` que o programa observou.
fn item_calls(project: &Project) -> Vec<serde_json::Map<String, Value>> {
    project.calls_of("wave items")
}

/// A regra do projeto todo com o título `title`, gravada depois da aprovação,
/// e a de outros arquivos; devolve o código de cada uma.
fn two_rules(project: &Project, said: u64) -> (String, String) {
    let write_rule = |title: &str, applies_to: Value| {
        let written = project.write(
            "rule",
            &json!({"title": title, "text": format!("{title} Vale como está escrito."), "agent": "- vale no que a tarefa muda",
                "example": "e", "keys": ["k"], "applies_to": applies_to, "origin": said}),
        );
        assert_eq!(written["ok"], json!(true), "{written}");
        let id = written["id"].as_u64().expect("the rule id");
        project.log().codes().get(&id).cloned().expect("the rule code")
    };
    (write_rule("Regra sem relação com a onda", json!({"files": ["**"]})), write_rule("Regra de outro arquivo", json!({"files": ["outro.rs"]})))
}

/// A rodada pergunta ao Jev, numa chamada por onda e na mesma rodada em que a
/// onda sai, o que o pedido não leva nem tira sozinho: o item do projeto todo
/// sai com a chance de 0,03 e o de outros arquivos entra com a de 0,9. Nada
/// disso pede o orquestrador, e o envio grava o que saiu e o que entrou, cada
/// um com a chance, e a chamada, com os tokens, o custo e o modelo.
#[test]
fn the_jev_judges_the_items_of_the_request_and_the_wave_leaves_in_the_same_round() {
    let jev = judging_items(|title| match title {
        "Regra sem relação com a onda" => 0.03,
        "Regra de outro arquivo" => 0.9,
        _ => 0.5,
    });
    let (mut project, _, said, _) = backlog_project(&[&["a.rs"]]);
    project.jev = Some(jev.url.clone());
    let (out_code, in_code) = two_rules(&project, said);

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
    assert!(out.get("analysis").is_none(), "{out}");
    let items: Vec<Value> =
        jev.all_requests().into_iter().filter(|asked| asked["questions"].as_object().is_some_and(|q| q.keys().all(|key| key.starts_with('i')))).collect();
    assert_eq!(items.len(), 1, "one call about the items of the wave: {items:?}");
    assert!(items[0]["state"]["task"].as_object().is_some_and(|task| !task.is_empty()), "{}", items[0]);
    let log = project.log();
    let codes = log.codes();
    let id_of_code = |code: &str| codes.iter().find(|(_, c)| c.as_str() == code).map(|(id, _)| *id).expect("the item");
    let (out_id, in_id) = (id_of_code(&out_code), id_of_code(&in_code));
    let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the send");
    let analysis = &sent.fields["analysis"];
    assert_eq!(analysis["removed"], json!([{"item": out_id, "why": "Jev p=0.03"}]), "{analysis}");
    assert_eq!(analysis["added"], json!([{"item": in_id, "why": "Jev p=0.90"}]), "{analysis}");
    let prompt = sent.str_field("text").expect("the request");
    assert!(prompt.contains(&in_code) && !prompt.contains(&out_code), "{prompt}");
    let calls = item_calls(&project);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        (calls[0]["filter"].clone(), calls[0]["tokens"].clone(), calls[0]["cost_micro_usd"].clone(), calls[0]["model"].clone()),
        (json!("jev"), json!(1500), json!(63), json!("jev-1.13.0")),
        "{calls:?}"
    );
}

/// Sem chave a onda sai na mesma rodada com o pedido padrão — o item do
/// projeto todo vai, o de outros arquivos fica fora —, nenhuma chamada se faz
/// e o envio não grava escolha.
#[test]
fn without_a_key_the_wave_leaves_in_the_same_round_with_the_default_items() {
    let (project, _, said, _) = backlog_project(&[&["a.rs"]]);
    let (project_code, other_code) = two_rules(&project, said);

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
    assert!(out.get("analysis").is_none(), "{out}");
    let log = project.log();
    let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the send");
    let prompt = sent.str_field("text").expect("the request");
    assert!(prompt.contains(&project_code) && !prompt.contains(&other_code), "{prompt}");
    assert!(sent.fields.get("analysis").is_none(), "{:?}", sent.fields);
    assert!(item_calls(&project).is_empty(), "no key, no call");
}

// ---------------------------------------------------------------------------
// Crédito do serviço, cache e continuidade nativa, de ponta a ponta
// ---------------------------------------------------------------------------

/// Um Jev de mentira que responde à busca: cada candidato com chance de 0,9,
/// a existência também, e `input_tokens` tokens de entrada cobrados.
fn answering_searches(input_tokens: u64) -> FakeJev {
    FakeJev::start(move |asked| (200, search_answer(asked, input_tokens)))
}

fn search_answer(asked: &Value, input_tokens: u64) -> Value {
    let ids = asked["questions"]["where"]["criteria"].as_object().map(|criteria| criteria.keys().cloned().collect::<Vec<_>>());
    let chances: serde_json::Map<String, Value> = ids.unwrap_or_default().into_iter().map(|id| (id, json!(0.9))).collect();
    let answers = json!({"where": {"type": "choice", "choice": "c000", "probabilities": chances},
        "exists": {"type": "noul", "noul": 0.9}});
    json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": input_tokens, "output_tokens": 0}})
}

/// Grava o mapa do projeto em dia, com as duas funções de `src/main.rs`: a
/// busca lê o mapa sem passar pelo scan.
fn mapped(project: &Project) {
    let root = &project.root;
    std::fs::write(root.join("src/main.rs"), "fn main() {\n    println!(\"{}\", greeting());\n}\n\nfn greeting() -> &'static str {\n    \"oi\"\n}\n")
        .expect("code");
    git(root, &["commit", "-q", "-am", "saudação"]);
    let blob = Command::new("git").args(["hash-object", "--", "src/main.rs"]).current_dir(root).output().expect("git");
    let now = mustard_core::io::project_map::listing(root).expect("inside git");
    let map = json!({
        "state": {"head": now.head, "listing": now.digest(), "base": now.base.name, "base_tip": now.base.tip},
        "modules": [{
            "path": "src/main.rs", "language": "rust", "loc": 7,
            "blob": String::from_utf8_lossy(&blob.stdout).trim(),
            "declarations": [
                {"kind": "function", "name": "main", "line": 1, "end_line": 3, "signature": "fn main()",
                 "body_comment": "imprime a saudação do programa"},
                {"kind": "function", "name": "greeting", "line": 5, "end_line": 7, "signature": "fn greeting() -> &'static str",
                 "body_comment": "o texto da saudação"}
            ]
        }]
    });
    mustard_core::io::project_map::write_text(root, &map.to_string()).expect("the map");
}

#[test]
fn historical_spending_does_not_block_search_and_cached_answers_still_avoid_http() {
    let jev = answering_searches(240_000_000);
    let mut project = Project::new();
    project.jev = Some(jev.url.clone());
    mapped(&project);
    let root = project.root.to_string_lossy().to_string();
    let search = |query| project.run(&["map", "search", "--query", query, "--root", &root]);

    let first = search("texto da saudação");
    assert_eq!(first["filter"], json!("jev"), "the provider answers: {first}");
    assert_eq!(jev.all_requests().len(), 1);

    let cached = search("texto da saudação");
    assert_eq!(cached["filter"], "jev", "the cached judgement is reused: {cached}");
    assert_eq!(jev.all_requests().len(), 1, "reuse sent no new physical request");
    let second = search("texto da saudação novamente");
    assert_eq!(second["filter"], "jev", "recorded spending cannot block a new decision: {second}");
    assert!(second["pieces"].as_array().is_some_and(|pieces| !pieces.is_empty()), "{second}");
    assert_eq!(jev.all_requests().len(), 2, "a different query reaches the provider");
}

#[test]
fn legacy_zero_budget_and_old_spending_do_not_block_wave_planning_or_context() {
    let jev = judging_items(|_| 0.9);
    let files: [&[&str]; 3] = [&["a.rs"], &["a.rs", "b.rs"], &["c.rs"]];
    let (project, _, said, tasks) = judged_project(&files, &jev);
    let (project_code, other_code) = two_rules(&project, said);
    let config_path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["jev"] = json!({"monthly_budget_usd": 0});
    std::fs::write(config_path, config.to_string()).unwrap();
    project.seed(
        "call",
        &json!({"author": "binary", "command": "map search", "ms": 900, "result": "ok", "filter": "jev", "tokens": 238_095_238,
            "cost_micro_usd": 10_000_000}),
    );

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert!(!jev.all_requests().is_empty(), "the provider receives new decisions");
    assert_eq!(batch_orders(&project), vec![tasks], "planning uses the provider judgement");
    let log = project.log();
    let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the send");
    let prompt = sent.str_field("text").expect("the request");
    assert!(prompt.contains(&project_code) && prompt.contains(&other_code), "context uses the provider judgement: {prompt}");
    assert!(sent.fields.get("analysis").is_some(), "{:?}", sent.fields);
    let reasons: Vec<&Value> = out["warnings"].as_array().into_iter().flatten().map(|warning| &warning["reason"]).collect();
    assert!(reasons.is_empty(), "no invented spending warning: {out}");
}

#[test]
fn a_credit_refusal_preserves_native_search_and_recharging_restores_judgement() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let available = Arc::new(AtomicBool::new(false));
    let credit = Arc::clone(&available);
    let jev = FakeJev::start(move |asked| {
        if credit.load(Ordering::SeqCst) {
            (200, search_answer(asked, 700))
        } else {
            (402, json!({"error": "credit unavailable"}))
        }
    });
    let mut project = Project::new();
    project.jev = Some(jev.url.clone());
    mapped(&project);
    let root = project.root.to_string_lossy().to_string();
    let search = || project.run(&["map", "search", "--query", "texto da saudação", "--root", &root]);
    let refused = search();
    assert!(refused.get("filter").is_none(), "the failed provider does not replace native evidence: {refused}");
    assert!(refused["files"].as_array().is_some_and(|files| !files.is_empty()), "{refused}");
    assert_eq!(jev.all_requests().len(), 1, "credit refusal is not retried");
    available.store(true, Ordering::SeqCst);
    let restored = search();
    assert_eq!(restored["filter"], "jev", "the failure does not become a permanent block: {restored}");
    assert_eq!(search()["filter"], "jev");
    assert_eq!(jev.all_requests().len(), 2, "the successful decision is cached");
}

#[test]
fn a_credit_refusal_keeps_round_planning_and_context_native_and_records_the_failures() {
    let jev = FakeJev::start(|_| (402, json!({"error": "credit unavailable"})));
    let files: [&[&str]; 3] = [&["a.rs"], &["a.rs", "b.rs"], &["c.rs"]];
    let (project, _, said, tasks) = judged_project(&files, &jev);
    let (project_code, other_code) = two_rules(&project, said);
    // Sem crédito, a montagem e depois a escolha dos itens vão à reserva; o
    // recibo incerto de cada uma deixa o caminho sem Jev: a montagem pelo
    // arquivo e o pedido padrão.
    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(out["ok"], true, "the round continues: {out}");
    assert!(batch_orders(&project).is_empty(), "the assembly waits for the receipt: {out}");
    assert_eq!(answer_reserves(&project, &out, "unsure"), 1, "{out}");
    let items = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(answer_reserves(&project, &items, "unsure") >= 1, "the choice of the items waits too: {items}");
    let sent = project.run(&["round", "--dispatch", "--spec", SPEC]);
    // As ondas que esperaram a escolha dos itens voltaram a ser montadas,
    // pelo arquivo, com as tarefas em versão nova.
    let orders = batch_orders(&project);
    assert_eq!(orders[0], vec![tasks[0], tasks[1]], "{orders:?}");
    assert_eq!(orders[orders.len() - 2..].iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 1], "by file: {orders:?}");
    assert!(sent.get("reserves").is_none(), "{sent}");
    let log = project.log();
    let sent = log.visible().into_iter().find(|e| e.event_type == "send").unwrap();
    let prompt = sent.str_field("text").unwrap();
    assert!(prompt.contains(&project_code) && !prompt.contains(&other_code), "native context is preserved: {prompt}");
    let attempts = std::fs::read_to_string(project.root.join(".claude/judgements/attempts.ndjson")).unwrap();
    let rows: Vec<Value> = attempts.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(rows.len(), jev.all_requests().len());
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row["status"] == 402 && row["attempt"] == 1 && row["error"] == "no_credit"));
    assert!(rows.iter().any(|row| row["purpose"] == "wave-planning"));
    assert!(rows.iter().any(|row| row["purpose"] == "context"));
    // O programa compilado avisa a falha pela trava de produção, sem o Jev
    // de mentira dos testes da biblioteca: com o motivo e o remédio de crédito.
    let warned: Vec<&Value> = out["warnings"].as_array().into_iter().flatten().filter(|warning| warning["reason"] == "jev-failed").collect();
    assert_eq!(warned.len(), 1, "{out}");
    assert_eq!(warned[0]["failure"], "no_credit", "{out}");
    let hint = translate("round.jev_failed", Locale::PtBr)
        .replace("{reason}", "no_credit")
        .replace("{remedy}", translate("round.jev_remedy_credit", Locale::PtBr));
    assert_eq!(warned[0]["hint"], json!(hint), "{out}");
}

/// O arquivo que a tarefa das provas da reprovação cria.
const GREETING: &str = "src/greeting.txt";
/// O motivo com que quem conduz a obra reprova a volta, nas provas.
const REJECTION: &str = "A saudação saiu em inglês, e o combinado pede português.";

/// O envio mais novo da onda `wave`, com a cópia e a lista de leitura dela.
fn last_send(project: &Project, wave: u64) -> mustard_core::domain::spec_events::SpecEvent {
    let log = project.log();
    let id = *log.last_by_wave("send").get(&wave).expect("a onda saiu");
    log.get(id).expect("o envio").clone()
}

/// O agente da onda `wave` faz o trabalho dela: escreve `content` em
/// [`GREETING`] dentro da cópia, lê de lá cada item que o pedido manda ler,
/// grava o passo de término de cada tarefa e grava a entrega, que responde
/// por todo o combinado. Devolve a cópia.
fn deliver_in_copy(project: &Project, wave: u64, content: &str) -> PathBuf {
    deliver_file_in_copy(project, wave, GREETING, content)
}

/// [`deliver_in_copy`] com o arquivo `file` no lugar de [`GREETING`].
fn deliver_file_in_copy(project: &Project, wave: u64, file: &str, content: &str) -> PathBuf {
    let sent = last_send(project, wave);
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    std::fs::write(copy.join(file), content).expect("the change");
    let root = project.root.display().to_string();
    let listed = sent.fields.get("read_items").and_then(Value::as_array).cloned().unwrap_or_default();
    for item in listed.iter().filter_map(Value::as_str) {
        let block = format!("item-{item}");
        let lesson = item.strip_prefix("lesson-");
        let mut args = vec!["run", "read", lesson.map_or(block.as_str(), |_| "lessons")];
        args.extend(lesson.into_iter().flat_map(|n| ["--term", n]));
        args.extend(["--root", &root, "--spec", SPEC]);
        let out = project.command_in(&copy, &args, "");
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
    }
    let log = project.log();
    let codes = log.codes();
    for task in log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(wave)) {
        let item = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
        project.write("step", &json!({"wave": wave, "item": item, "text": "A tarefa ficou pronta."}));
    }
    let agreed: Vec<Value> = mustard_core::domain::wave_prompt::all_agreed(&log).iter().map(|item| json!({"item": item.id, "met": true})).collect();
    let delivered = json!({"wave": wave, "text": "A saudação saiu.", "files": [file], "commit": "saudação nova",
        "agreed": agreed});
    project.write("delivered", &delivered);
    copy
}

/// Uma spec aprovada com uma tarefa que cria [`GREETING`], despachada pela
/// rodada e entregue pelo agente na cópia dela. Com `alive`, o envio fica com
/// o processo do teste, como o Claude Code que mandou a onda ainda aberto;
/// sem ele, com um processo que já terminou. Devolve o projeto e a cópia.
fn delivered_wave(alive: bool) -> (Project, PathBuf) {
    let (project, _, _, _) = backlog_project(&[&[GREETING]]);
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    sent_by(&project, &[1], if alive { test_process() } else { closed_process::closed_process() });
    let copy = deliver_in_copy(&project, 1, "hello\n");
    (project, copy)
}

/// A rodada com a linha que reprova a volta da onda `wave` pelo motivo
/// `reason`, como veio.
fn reject(project: &Project, wave: u64, reason: &str) -> Value {
    let line = format!("<REJECTED>{}</REJECTED>", json!({"wave": wave, "reason": reason}));
    project.answer(&["round", "--dispatch", "--spec", SPEC, "--report", &line])
}

/// O commit em que o repositório principal está.
fn head(project: &Project) -> String {
    let out = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&project.root).output().expect("git");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// O aviso da onda reprovada na resposta `out`.
fn rejected_warning(out: &Value) -> Value {
    out["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|warning| warning["reason"] == json!("round-wave-rejected"))
        .cloned()
        .unwrap_or_else(|| panic!("the warning of the rejected wave: {out}"))
}

/// A frase que manda despachar o agente novo da onda 1, reprovada por
/// [`REJECTION`].
fn new_agent_line() -> String {
    let title = mustard_core::domain::wave_prompt::wave_title(SPEC, 1, Locale::PtBr);
    translate("round.rejected", Locale::PtBr).replace("{wave}", "1").replace("{reason}", REJECTION.trim_end_matches('.')).replace("{title}", &title)
}

/// O gancho de antes da ferramenta com `payload`, pelo binário: a resposta
/// dele, como o harness a lê, ou `null` quando a chamada passa sem resposta.
fn before_tool(project: &Project, payload: &Value) -> Value {
    let out = project.command(&["on", "PreToolUse"], &payload.to_string());
    assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    if out.stdout.trim_ascii().is_empty() {
        return Value::Null;
    }
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

/// O despacho de um agente de onda pelo título do pedido da onda 1, pelo
/// gancho de verdade: a resposta dele, como o harness a lê.
fn dispatch_new_agent(project: &Project) -> Value {
    before_tool(project, &new_agent_payload(project))
}

/// O `Agent` que despacha um agente pelo título do pedido da onda 1, como o
/// Claude Code o entrega ao gancho de antes da ferramenta.
fn new_agent_payload(project: &Project) -> Value {
    let title = mustard_core::domain::wave_prompt::wave_title(SPEC, 1, Locale::PtBr);
    json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Agent",
        "tool_input": {"subagent_type": "mustard-wave", "description": title, "prompt": title},
        "session_id": SESSION,
        "cwd": project.root.to_string_lossy(),
    })
}

/// O despacho do agente novo da onda 1 por quem conduz a obra, como o Claude
/// Code de verdade: o gancho roda sob um processo chamado `claude`, que segue
/// aberto até o teste fechar a entrada dele, e o despacho grava esse processo
/// no envio. Devolve a resposta do gancho e o processo, ainda aberto.
#[cfg(target_os = "linux")]
fn dispatch_from_claude(project: &Project) -> (Value, std::process::Child) {
    let claude = project.home.join("bin").join("claude");
    if !claude.exists() {
        std::fs::create_dir_all(claude.parent().expect("the folder")).expect("the folder");
        std::os::unix::fs::symlink("/bin/sh", &claude).expect("the conductor");
    }
    let (input, output) = (project.home.join("agent.json"), project.home.join("hook.json"));
    std::fs::write(&input, new_agent_payload(project).to_string()).expect("the payload");
    let mut conductor = project
        .program(&claude)
        .args(["-c", "\"$1\" on PreToolUse < \"$2\" > \"$3\"; echo pronto; read _", "claude", env!("CARGO_BIN_EXE_mustard-rt")])
        .arg(&input)
        .arg(&output)
        .current_dir(&project.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the conductor runs");
    let mut ready = String::new();
    BufReader::new(conductor.stdout.as_mut().expect("the conductor's output")).read_line(&mut ready).expect("the hook ends");
    let raw = std::fs::read_to_string(&output).expect("the hook's answer");
    let answer = if raw.trim().is_empty() { Value::Null } else { serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{e}: {raw}")) };
    (answer, conductor)
}

/// Fecha o processo de quem conduz a obra aberto por [`dispatch_from_claude`].
#[cfg(target_os = "linux")]
fn close_conductor(mut conductor: std::process::Child) {
    drop(conductor.stdin.take());
    conductor.wait().expect("the conductor ends");
}

/// O começo da conversa do agente que fez a volta reprovada nas provas: bem
/// antes da volta que ele gravou.
const OLD_AGENT_START: &str = "2026-01-10T21:54:28.200Z";

/// O instante de agora, como o Claude Code o carimba na conversa.
fn now_stamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A conversa principal de quem conduz a obra nas provas do agente antigo:
/// os agentes que ela despachou moram ao lado, em `sessao/subagents/`.
fn conductor_transcript(project: &Project) -> PathBuf {
    project.home.join("sessoes").join("-projeto").join("sessao.jsonl")
}

/// A conversa do agente `agent` da onda 1, despachado na sessão de
/// [`conductor_transcript`]: abre com o título do pedido da onda e começa no
/// instante `started`.
fn wave_agent(project: &Project, agent: &str, started: &str) {
    let dir = conductor_transcript(project).with_extension("").join("subagents");
    std::fs::create_dir_all(&dir).expect("the agents' folder");
    let log = project.log();
    let send = log.get(log.last_by_wave("send")[&1]).unwrap();
    let started = if started == OLD_AGENT_START { log.get(log.dispatch_position(send.id)).unwrap().at() } else { started };
    let opening = log.send_text(send.id).unwrap();
    let line = json!({"type": "user", "timestamp": started, "message": {"role": "user", "content": opening}});
    std::fs::write(dir.join(format!("agent-{agent}.jsonl")), format!("{line}\n")).expect("the agent's conversation");
}

/// A mensagem de quem conduz ao agente `agent`, pelo gancho de verdade: a
/// resposta dele, como o harness a lê.
fn message_to(project: &Project, agent: &str) -> Value {
    before_tool(
        project,
        &json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "SendMessage",
            "tool_input": {"to": agent, "summary": "conserto", "message": "Conserte a saudação, por favor."},
            "session_id": SESSION,
            "transcript_path": conductor_transcript(project).to_string_lossy(),
            "cwd": project.root.to_string_lossy(),
        }),
    )
}

/// O agente `agent` recebeu a ordem de entregar: a marca que o fim de tarefa
/// grava quando a conversa dele passa do limite, e que fecha a trava.
fn ordered_to_deliver(project: &Project, agent: &str) {
    let marks = project.root.join(".claude").join(".session");
    std::fs::create_dir_all(&marks).expect("the marks' folder");
    std::fs::write(marks.join(format!("size-deliver-agent-{agent}")), "200000").expect("the order to deliver");
}

/// O agente `agent` chama a ferramenta `tool` com `input`, pelo gancho de
/// verdade: a resposta dele, como o harness a lê.
fn agent_calls(project: &Project, agent: &str, tool: &str, input: &Value) -> Value {
    before_tool(
        project,
        &json!({
            "hook_event_name": "PreToolUse",
            "tool_name": tool,
            "tool_input": input,
            "session_id": SESSION,
            "agent_id": agent,
            "transcript_path": conductor_transcript(project).to_string_lossy(),
            "cwd": project.root.to_string_lossy(),
        }),
    )
}

/// O motivo da recusa na resposta `out` do gancho; a resposta que não recusa
/// derruba o teste.
fn denial(out: &Value) -> String {
    assert_eq!(out.pointer("/hookSpecificOutput/permissionDecision"), Some(&json!("deny")), "{out}");
    out.pointer("/hookSpecificOutput/permissionDecisionReason").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Com o Claude Code do envio aberto, a mensagem de quem conduz ao agente
/// que fez a volta reprovada é barrada, antes do despacho do agente novo e
/// depois dele: a onda saiu daquele agente, e a cópia nunca tem dois agentes.
/// A mensagem ao agente novo passa, só com o motivo.
#[test]
fn a_message_to_the_agent_of_a_rejected_return_is_refused_and_the_new_agent_gets_it() {
    let (project, _) = delivered_wave(true);
    wave_agent(&project, "antigo", OLD_AGENT_START);
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let title = mustard_core::domain::wave_prompt::wave_title(SPEC, 1, Locale::PtBr);

    let waiting = translate("subagent.rejected_agent", Locale::PtBr).replace("{wave}", "1").replace("{title}", &title);
    assert_eq!(denial(&message_to(&project, "antigo")), waiting, "before the new agent");

    assert!(dispatch_new_agent(&project).pointer("/hookSpecificOutput/updatedInput/prompt").is_some());
    keep_sent(&project, &[1]);
    wave_agent(&project, "novo", &now_stamp());
    let working = translate("subagent.rejected_agent_working", Locale::PtBr).replace("{wave}", "1");
    assert_eq!(denial(&message_to(&project, "antigo")), working, "after the new agent");

    let fix = translate("round.rejected_fix", Locale::PtBr).replace("{wave}", "1").replace("{reason}", REJECTION);
    let to_new = message_to(&project, "novo");
    let message = to_new.pointer("/hookSpecificOutput/updatedInput/message").and_then(Value::as_str).unwrap();
    assert!(message.starts_with(&fix) && message.contains("Conserte a saudação, por favor."), "{to_new}");
    assert!(message.contains("não autorizam alterar escopo"), "{to_new}");
}

/// Com o Claude Code do envio aberto, toda chamada do agente que fez a volta
/// reprovada é recusada depois da reprovação, sem a ordem de entregar e com
/// ela: o comando qualquer, a edição, ler e gravar na spec. Antes da
/// reprovação, as mesmas chamadas passam. O agente novo, com a ordem de
/// entregar, tem a trava aberta para consertar e gravar a volta dele.
#[test]
fn every_call_of_the_agent_of_a_rejected_return_is_refused() {
    let (project, copy) = delivered_wave(true);
    wave_agent(&project, "antigo", OLD_AGENT_START);
    let root = project.root.display();
    let calls = [
        ("Bash", json!({"command": "ls src"})),
        ("Edit", json!({"file_path": copy.join(GREETING), "old_string": "hello", "new_string": "olá"})),
        ("Bash", json!({"command": format!("mustard-rt run read request-1 --root {root} --spec {SPEC}")})),
        ("Bash", json!({"command": format!("mustard-rt run write delivered --root {root} --spec {SPEC} --json '{{}}'")})),
    ];
    let passes = |agent: &str, when: &str| {
        for (tool, input) in &calls {
            let out = agent_calls(&project, agent, tool, input);
            assert_ne!(out.pointer("/hookSpecificOutput/permissionDecision"), Some(&json!("deny")), "{when}: {input} {out}");
        }
    };
    passes("antigo", "before the rejection");

    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let locked = translate("subagent.rejected_agent_locked", Locale::PtBr).replace("{wave}", "1");
    for (tool, input) in &calls {
        assert_eq!(denial(&agent_calls(&project, "antigo", tool, input)), locked, "{input}");
    }
    ordered_to_deliver(&project, "antigo");
    assert_eq!(denial(&agent_calls(&project, "antigo", calls[0].0, &calls[0].1)), locked, "with the order to deliver");

    assert!(dispatch_new_agent(&project).pointer("/hookSpecificOutput/updatedInput/prompt").is_some());
    keep_sent(&project, &[1]);
    wave_agent(&project, "novo", &now_stamp());
    ordered_to_deliver(&project, "novo");
    passes("novo", "the new agent");
}

/// Com o Claude Code do envio aberto, o agente que fez a volta reprovada
/// ainda devolve o relatório a quem conduz, sem a ordem de entregar e com
/// ela: a devolução não grava na spec nem na cópia, e é o fim dele. No mesmo
/// instante, o comando qualquer segue recusado pela reprovação.
#[test]
fn the_agent_of_a_rejected_return_still_hands_back_its_report() {
    let (project, _) = delivered_wave(true);
    wave_agent(&project, "antigo", OLD_AGENT_START);
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let locked = translate("subagent.rejected_agent_locked", Locale::PtBr).replace("{wave}", "1");
    let report = json!({"message": "A onda 1 saiu de mim."});
    for ordered in [false, true] {
        if ordered {
            ordered_to_deliver(&project, "antigo");
        }
        assert_eq!(denial(&agent_calls(&project, "antigo", "Bash", &json!({"command": "ls src"}))), locked, "ordered: {ordered}");
        let out = agent_calls(&project, "antigo", "SubagentHandback", &report);
        assert_ne!(out.pointer("/hookSpecificOutput/permissionDecision"), Some(&json!("deny")), "ordered: {ordered}: {out}");
    }
}

/// A rodada com a linha da reprovação deixa a volta fora do commit e a cópia
/// como está, e manda despachar um agente novo pelo título da onda: com o
/// Claude Code que mandou a onda aberto, e com ele fechado.
#[test]
fn a_rejected_return_stays_out_of_the_commit_and_waits_for_a_new_agent() {
    for alive in [true, false] {
        let (project, copy) = delivered_wave(alive);
        let before = head(&project);

        let out = reject(&project, 1, REJECTION);

        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(out.get("commit"), None, "nada comitado: {out}");
        assert_eq!(head(&project), before, "o repositório principal fica no mesmo commit");
        assert!(!project.root.join(GREETING).exists(), "a volta reprovada não chega ao repositório principal");
        assert!(std::fs::read_to_string(copy.join(GREETING)).expect("a cópia").contains("hello"), "a cópia fica como está");
        let warning = rejected_warning(&out);
        assert_eq!(warning["wave"], json!(1), "{warning}");
        let next = out["next"].as_str().unwrap_or_default();
        assert!(next.contains(&new_agent_line()), "alive={alive}: {next}");
        let sent = last_send(&project, 1);
        let delivered = project.log().unassumed_returns().iter().map(|e| e.id).max().expect("a volta reprovada");
        assert_eq!(sent.fields.get("rejected"), Some(&json!({"delivered": delivered, "reason": REJECTION})), "alive={alive}");
    }
}

/// O despacho do agente novo da onda reprovada traz o motivo de quem
/// reprovou, junto do pedido; com o Claude Code desse agente aberto, outro
/// agente novo é barrado: a cópia nunca tem dois agentes.
#[test]
fn the_new_agent_of_a_rejected_wave_gets_the_reason_and_keeps_the_copy_alone() {
    let (project, _) = delivered_wave(true);
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));

    let first = dispatch_new_agent(&project);
    let prompt = first.pointer("/hookSpecificOutput/updatedInput/prompt").and_then(Value::as_str).unwrap_or_default();
    let fix = translate("round.rejected_fix", Locale::PtBr).replace("{wave}", "1").replace("{reason}", REJECTION);
    assert!(prompt.ends_with(&fix), "o motivo vai ao agente novo: {first}");
    assert!(prompt.contains("run read request-1"), "com o pedido da onda: {prompt}");

    keep_sent(&project, &[1]);
    let second = dispatch_new_agent(&project);
    assert_eq!(second.pointer("/hookSpecificOutput/permissionDecision"), Some(&json!("deny")), "{second}");
}

/// O agente novo que quem conduz despacha para a onda reprovada e que nunca
/// começa não segura a onda: com o Claude Code de quem conduz ainda aberto, a
/// rodada seguinte devolve a onda à espera do agente novo, com o título que o
/// despacha, e o despacho seguinte passa.
#[cfg(target_os = "linux")]
#[test]
fn a_new_agent_that_never_starts_does_not_hold_the_rejected_wave() {
    let (project, _) = delivered_wave(true);
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let (first, conductor) = dispatch_from_claude(&project);
    assert!(first.pointer("/hookSpecificOutput/updatedInput/prompt").is_some(), "{first}");
    assert!(last_send(&project, 1).int("claude_pid").is_some(), "the dispatch records the conductor");

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);
    let again = dispatch_new_agent(&project);
    close_conductor(conductor);

    assert!(out["next"].as_str().unwrap_or_default().contains(&new_agent_line()), "{out}");
    assert!(again.pointer("/hookSpecificOutput/updatedInput/prompt").is_some(), "{again}");
}

/// A chamada do agente que a reprovação tirou da onda, feita depois do
/// despacho do agente novo, não conta como o começo do agente novo: a rodada
/// seguinte devolve a onda à espera dele.
#[cfg(target_os = "linux")]
#[test]
fn a_call_of_the_rejected_agent_does_not_start_the_new_one() {
    let (project, _) = delivered_wave(true);
    wave_agent(&project, "antigo", OLD_AGENT_START);
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let (_, conductor) = dispatch_from_claude(&project);
    agent_calls(&project, "antigo", "Bash", &json!({"command": "git status", "description": "estado da cópia"}));

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);
    close_conductor(conductor);

    assert!(out["next"].as_str().unwrap_or_default().contains(&new_agent_line()), "{out}");
}

/// O agente novo da onda reprovada que começa de fato segura a onda: a
/// rodada seguinte diz que ele trabalha, e outro despacho é barrado.
#[cfg(target_os = "linux")]
#[test]
fn a_new_agent_that_starts_keeps_the_rejected_wave() {
    let (project, _) = delivered_wave(true);
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let (_, conductor) = dispatch_from_claude(&project);
    wave_agent(&project, "novo", &now_stamp());
    agent_calls(&project, "novo", "Bash", &json!({"command": "git status", "description": "estado da cópia"}));

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);
    let again = dispatch_new_agent(&project);
    close_conductor(conductor);

    assert!(!out["next"].as_str().unwrap_or_default().contains(&new_agent_line()), "{out}");
    assert_eq!(again.pointer("/hookSpecificOutput/permissionDecision"), Some(&json!("deny")), "{again}");
}

/// A rodada seguinte, sem a linha no relatório, segue segurando a volta
/// reprovada fora do commit, com o mesmo motivo.
#[test]
fn the_round_after_a_rejection_keeps_holding_the_wave_without_the_line() {
    let (project, _) = delivered_wave(false);
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let before = head(&project);

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(out.get("commit"), None, "{out}");
    assert_eq!(head(&project), before);
    assert!(!project.root.join(GREETING).exists());
    assert!(rejected_warning(&out)["hint"].as_str().unwrap_or_default().contains(REJECTION.trim_end_matches('.')), "{out}");
    assert!(out["next"].as_str().unwrap_or_default().contains(&new_agent_line()), "{out}");
}

/// A volta nova da onda reprovada, gravada pelo agente novo, desfaz a
/// reprovação: a rodada a assume e comita como qualquer outra, e a entrega
/// oficial substitui as duas voltas.
#[test]
fn the_new_return_of_a_rejected_wave_is_taken_and_committed() {
    let (project, _) = delivered_wave(false);
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let rejected = project.log().unassumed_returns().iter().map(|e| e.id).max().expect("a volta reprovada");
    assert!(dispatch_new_agent(&project).pointer("/hookSpecificOutput/updatedInput/prompt").is_some());
    let fixed = "olá\n";
    deliver_in_copy(&project, 1, fixed);
    let renewed = project.log().unassumed_returns().iter().map(|e| e.id).max().expect("a volta nova");

    let out = project.run(&["round", "--dispatch", "--spec", SPEC, "--report", r#"<USAGE>{"wave":1}</USAGE>"#]);

    assert!(out.get("commit").is_some(), "{out}");
    assert_eq!(std::fs::read_to_string(project.root.join(GREETING)).expect("o arquivo entrou"), fixed);
    let log = project.log();
    let official = log.visible().into_iter().rfind(|e| e.event_type == "delivered" && e.wave() == Some(1)).expect("a entrega oficial");
    assert_eq!(official.fields.get("replaces"), Some(&json!([rejected, renewed])), "{:?}", official.fields);
    assert!(out["warnings"].as_array().into_iter().flatten().all(|w| w["reason"] != json!("round-wave-rejected")), "{out}");
}

/// As refs em que a limpeza das cópias guardou código, no repositório
/// principal.
fn kept_refs(project: &Project) -> Vec<String> {
    let out = Command::new("git").args(["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]).current_dir(&project.root).output().expect("git");
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

/// O Claude Code que mandou a onda fechou com o agente no meio do trabalho:
/// a rodada seguinte guarda o que ele deixou na cópia, volta a cópia ao
/// commit e reenvia a onda com o pedido que cita a ref guardada e cada
/// arquivo com mudança, mandando o agente novo conferir e continuar dali.
#[test]
fn an_orphaned_wave_is_resent_citing_the_ref_and_files_its_agent_left() {
    let (project, _, _, _) = backlog_project(&[&[GREETING]]);
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    let copy = PathBuf::from(last_send(&project, 1).str_field("copy").expect("the copy").to_string());
    std::fs::write(copy.join(GREETING), "olá\n").expect("a new file");
    std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"olá\");\n}\n").expect("an edit");
    sent_by(&project, &[1], closed_process::closed_process());

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
    let resent = last_send(&project, 1);
    assert!(resent.int("resends").is_some(), "the wave went out again: {:?}", resent.fields);
    assert!(!copy.join(GREETING).exists(), "the copy is back at the commit");
    let refs = kept_refs(&project);
    assert_eq!(refs.len(), 1, "{refs:?}");
    let text = project.log().send_text(resent.id).expect("the request").to_string();
    assert!(text.contains(translate("round.resume.kept", Locale::PtBr)), "{text}");
    assert!(text.contains(&format!("`{}`", refs[0])), "{text}");
    for file in [GREETING, "src/main.rs"] {
        assert!(text.contains(&format!("`{file}`")), "{file}: {text}");
    }

    // O agente novo também fecha antes de trazer o resto de volta: o reenvio
    // seguinte segue citando a mesma ref.
    sent_by(&project, &[1], closed_process::closed_process());
    let again = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&again, "dispatch"), vec![1], "{again}");
    let third = last_send(&project, 1);
    assert_ne!(third.id, resent.id, "the wave went out once more");
    let text = project.log().send_text(third.id).expect("the request").to_string();
    assert!(text.contains(&format!("`{}`", refs[0])), "{text}");
    assert_eq!(kept_refs(&project), refs, "nothing new to keep, nothing dropped");
}

/// Uma tela do painel da spec, pelo binário: a resposta inteira.
fn panel(project: &Project, screen: &[&str]) -> Value {
    let mut args = vec!["run", "panel", "--spec", SPEC, "--section"];
    args.extend(screen);
    let out = project.command(&args, "");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

/// Três tarefas em fila (A → B → C), com uma sessão conduzindo e o Jev
/// escolhendo os itens do pedido: cada onda montada sai na mesma rodada em
/// que nasce, a primeira já na primeira rodada, sem esperar a leitura do
/// classificador, e a seguinte sai na rodada que comita a anterior, com o
/// número seguinte. Nenhuma onda fica gravada sem sair, a lista das ondas
/// abertas só traz a que saiu, e o pedido, o painel e o `description` que o
/// despacho devolve dão o mesmo número.
#[test]
fn each_formed_wave_leaves_in_the_round_it_is_born_with_one_number() {
    // O Jev escolhe só os itens; a montagem é a por arquivo, que põe cada
    // tarefa da fila numa onda dela.
    let jev = judging_items(|_| 0.5);
    let mut project = Project::new();
    let path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("config")).expect("json");
    config["judgement"].as_object_mut().expect("the judgement").remove("wave-planning");
    std::fs::write(&path, config.to_string()).expect("config");
    project.open();
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let crit = criterion["id"].as_u64().expect("the criterion id");
    let files = ["src/a.txt", "src/b.txt", "src/c.txt"];
    let first = backlog_task(&project, crit, said, &[files[0]], &[]);
    let second = backlog_task(&project, crit, said, &[files[1]], &[first]);
    backlog_task(&project, crit, said, &[files[2]], &[second]);
    project.run(&["plan", "--spec", SPEC]);
    approve(&project);
    project.jev = Some(jev.url.clone());
    project.session = Some(SESSION);
    two_rules(&project, said);

    for n in 1..=3u64 {
        let usage = format!("<USAGE>{}</USAGE>", json!({"wave": n - 1}));
        let args: Vec<&str> = if n == 1 { vec!["round", "--dispatch", "--spec", SPEC] } else { vec!["round", "--dispatch", "--spec", SPEC, "--report", &usage] };
        let out = project.run(&args);
        assert!(n == 1 || out.get("commit").is_some(), "round {n} commits the wave before: {out}");
        assert!(out.get("formation").is_none(), "round {n}: only one task of the queue can leave, and nothing waits for a reading: {out}");
        assert_eq!(waves_in(&out, "dispatch"), vec![n], "round {n}: {out}");
        let title = mustard_core::domain::wave_prompt::wave_title(SPEC, n, Locale::PtBr);
        assert_eq!(out["dispatch"][0]["description"], json!(title), "round {n}: {out}");
        let log = project.log();
        assert!(log.send_text(last_send(&project, n).id).expect("the request").starts_with(&title), "round {n}");
        let sent: BTreeSet<u64> = log.visible().into_iter().filter(|e| e.event_type == "send").filter_map(|e| e.wave()).collect();
        let formed: BTreeSet<u64> = log.visible().into_iter().filter(|e| e.event_type == "wave").filter_map(|e| e.wave()).collect();
        assert_eq!(formed, sent, "round {n}: every recorded wave went out");
        let listed = project.command(&["run", "read", "wave-list", "--spec", SPEC], "");
        let listed = String::from_utf8_lossy(&listed.stdout).to_string();
        assert!(listed.contains(&format!("\"n\":{n}")), "round {n}: wave {n} is open: {listed}");
        for other in (1..=n + 1).filter(|m| *m != n) {
            assert!(!listed.contains(&format!("\"n\":{other}")), "round {n}: wave {other} is not open: {listed}");
        }
        keep_sent(&project, &[n]);
        let screen = panel(&project, &["spec"]);
        let waves = &screen["spec"]["waves"];
        assert_eq!((waves["planned"].as_u64(), waves["running"].as_u64(), waves["committed"].as_u64()), (Some(n), Some(1), Some(n - 1)), "round {n}: {screen}");
        let screen = panel(&project, &["items", "--kind", "task"]);
        let shown: BTreeSet<u64> = screen["rows"].as_array().into_iter().flatten().filter_map(|row| row["wave"].as_u64()).collect();
        assert_eq!(shown, (1..=n).collect::<BTreeSet<u64>>(), "round {n}: {screen}");
        deliver_file_in_copy(&project, n, files[usize::try_from(n - 1).unwrap()], "pronto\n");
    }
}

/// A onda gravada sem tarefa nenhuma, como a que uma rodada antiga deixou,
/// saiu do plano: a lista das ondas abertas não a traz.
#[test]
fn a_wave_without_tasks_is_not_listed_as_open() {
    let (project, crit, _, _) = backlog_project(&[&["a.rs"]]);
    project.seed("wave", &json!({"n": 9, "text": "Onda vazia", "done_when": "Onda vazia", "author": "binary", "order": [], "criteria": [crit]}));

    let listed = project.command(&["run", "read", "wave-list", "--spec", SPEC], "");

    let listed = String::from_utf8_lossy(&listed.stdout).to_string();
    assert!(listed.contains("\"ok\":true"), "{listed}");
    assert!(!listed.contains("\"n\":9"), "{listed}");
}

/// A rodada sem a revisão do condutor: a volta que chegou segue esperando,
/// com a cópia guardada, e a resposta vem inteira.
fn unreviewed_round(project: &Project) -> Value {
    let out = project.command(&["run", "round", "--dispatch", "--spec", SPEC], "");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)))
}

/// O aviso `copies-full` da resposta da rodada, quando saiu.
fn copies_full(out: &Value) -> Option<&Value> {
    out["warnings"].as_array().into_iter().flatten().find(|warning| warning["reason"] == json!("copies-full"))
}

/// Uma vaga só, duas cópias e quatro tarefas prontas, cada uma no seu
/// arquivo. As ondas 1 e 2 voltam e guardam as duas cópias; o aviso de
/// cópias cheias cita as duas. A onda 1 perde a tarefa depois de voltar e
/// sai do ar: a cópia dela vai para a onda 3, e quando as ondas 2 e 3
/// voltam e a quarta tarefa espera, o aviso cita só as duas que guardam
/// cópia, e não a que saiu do ar.
#[test]
fn a_wave_that_lost_its_last_task_after_returning_is_not_named_among_the_copies_kept() {
    let (project, _, _, tasks) = backlog_project(&[&["a.rs"], &["b.rs"], &["c.rs"], &["d.rs"]]);
    one_slot(&project);
    let (_, first) = dispatch_ready(&project);
    assert_eq!(first, vec![1]);
    deliver_file_in_copy(&project, 1, "a.rs", "um\n");
    let second = unreviewed_round(&project);
    assert_eq!(waves_in(&second, "dispatch"), vec![2], "{second}");
    keep_sent(&project, &[2]);
    deliver_file_in_copy(&project, 2, "b.rs", "dois\n");

    let full = unreviewed_round(&project);
    assert!(waves_in(&full, "dispatch").is_empty(), "both copies are kept by returned waves: {full}");
    assert_eq!(copies_full(&full).map(|warning| warning["waves"].clone()), Some(json!([1, 2])), "{full}");

    let removal = project.write("remove", &json!({"targets": [tasks[0]], "reason": "A tarefa saiu da onda."}));
    assert_eq!(removal["ok"], json!(true), "{removal}");
    let third = unreviewed_round(&project);
    assert_eq!(waves_in(&third, "dispatch"), vec![3], "the copy of the wave out of the air goes to the next wave: {third}");
    keep_sent(&project, &[3]);
    deliver_file_in_copy(&project, 3, "c.rs", "tres\n");

    let held = unreviewed_round(&project);
    assert!(waves_in(&held, "dispatch").is_empty(), "the copies of waves 2 and 3 hold the fourth task: {held}");
    let warning = copies_full(&held).unwrap_or_else(|| panic!("the copies are full: {held}"));
    assert_eq!(warning["waves"], json!([2, 3]), "the wave out of the air keeps no copy: {held}");
    let hint = warning["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("2, 3") && !hint.contains('1'), "{hint}");
}

/// Os eventos da spec, fora as chamadas de comando que toda chamada grava:
/// o que a rodada gravou de fato.
fn recorded(project: &Project) -> Vec<u64> {
    project.log().events.iter().filter(|e| !matches!(e.event_type.as_str(), "call" | "stage_run")).map(|e| e.id).collect()
}

/// O projeto passa a falar `lang`.
fn speak(project: &Project, lang: Locale) {
    let path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("config")).expect("json");
    config["language"]["text"] = json!(lang.as_str());
    std::fs::write(&path, config.to_string()).expect("config");
}

/// Reprovar a onda que não tem volta gravada é recusado, nos dois idiomas,
/// sem gravar nada.
#[test]
fn rejecting_a_wave_without_a_return_is_refused_in_both_languages() {
    let (project, _, _, _) = backlog_project(&[&[GREETING]]);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    let before = recorded(&project);
    for lang in [Locale::PtBr, Locale::EnUs] {
        speak(&project, lang);
        let out = reject(&project, 1, REJECTION);
        assert_eq!(out["reason"], json!("round-rejected-without-return"), "{out}");
        assert_eq!(out["hint"], json!(translate("round.rejected_without_return", lang).replace("{wave}", "1")), "{out}");
    }
    assert_eq!(recorded(&project), before, "nada gravado");
}

/// Reprovar a onda cuja volta já entrou num commit é recusado, nos dois
/// idiomas, com o commit, sem gravar nada.
#[test]
fn rejecting_a_committed_wave_is_refused_in_both_languages() {
    let (project, _) = delivered_wave(false);
    let taken = project.run(&["round", "--dispatch", "--spec", SPEC, "--report", r#"<USAGE>{"wave":1}</USAGE>"#]);
    let sha = taken["commit"]["sha"].as_str().unwrap_or_else(|| panic!("the commit: {taken}")).to_string();
    let before = recorded(&project);
    for lang in [Locale::PtBr, Locale::EnUs] {
        speak(&project, lang);
        let out = reject(&project, 1, REJECTION);
        assert_eq!(out["reason"], json!("round-rejected-committed"), "{out}");
        let hint = translate("round.rejected_committed", lang).replace("{wave}", "1").replace("{sha}", &sha[..7]);
        assert_eq!(out["hint"], json!(hint), "{out}");
    }
    assert_eq!(recorded(&project), before, "nada gravado");
}

/// Existing credentials and provider selections never authorize inference alone.
#[test]
fn native_defaults_ignore_legacy_jev_in_search_wave_planning_and_context() {
    let jev = FakeJev::start(|_| (500, json!({})));
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["a.rs"], &["c.rs"]], &jev);
    let path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config.as_object_mut().unwrap().remove("ai");
    std::fs::write(path, config.to_string()).unwrap();
    let out = project.command(&["run", "map", "search", "--query", "texto da saudação"], "");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![vec![tasks[0], tasks[1]], vec![tasks[2]]]);
    assert!(jev.requests().is_empty(), "native defaults must make no HTTP requests");
    assert!(assembly_calls(&project).is_empty());
}
