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

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::domain::spec_state::State;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Value};

#[path = "support/mod.rs"]
mod support;

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
        Self { _dir: dir, root, home, jev: None }
    }

    fn command(&self, args: &[&str], stdin: &str) -> Output {
        self.command_in(&self.root, args, stdin)
    }

    /// Como [`Self::command`], rodando de dentro de `dir`.
    fn command_in(&self, dir: &Path, args: &[&str], stdin: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(args)
            .current_dir(dir)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_PROJECT_DIR", &self.root)
            .env("MUSTARD_CLAUDE_BIN", self.home.join("sem-claude"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env_remove("TYPESAFE_API_KEY")
            .env_remove("MUSTARD_JEV_URL")
            .env_remove("MUSTARD_SPEND_DIR")
            .env_remove("MUSTARD_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(self.jev.iter().flat_map(|url| [("TYPESAFE_API_KEY", "fake-key"), ("MUSTARD_JEV_URL", url.as_str())]))
            .spawn()
            .expect("the binary runs");
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(stdin.as_bytes());
        }
        child.wait_with_output().expect("the binary finishes")
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

    /// Um evento do harness entregue ao gancho, como a sessão entrega.
    fn hook(&self, event: &str, payload: &Value) {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn log(&self) -> SpecLog {
        store::read(&store::spec_file(&self.root, SPEC).expect("spec file")).expect("readable").expect("the spec file")
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
    let said = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user"))
        .expect("the entry hook recorded the message");
    said.id
}

/// O levantamento inteiro: o objetivo, o `grill`, que grava os pontos, e
/// cada ponto com os fatos somados, respondido e fechado.
fn survey(project: &Project) -> u64 {
    let said = user_says(project, GOAL);
    project.write("context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));
    let grilled = project.run(&["grill", "--spec", SPEC, "--kinds", "feature"]);
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
    let mut task = json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Tarefa do backlog.", "files": files, "depends_on": depends_on,
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
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
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

    project.run(&["round", "--spec", SPEC]);

    let after = project.log();
    let waves: Vec<_> =
        after.visible().into_iter().filter(|e| e.event_type == "wave" && e.str_field("author") == Some("binary")).collect();
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
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
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

    project.run(&["round", "--spec", SPEC]);

    let after = project.log();
    let wave = after.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(1)).expect("the batch wave");
    assert_eq!(wave.str_field("author"), Some("binary"), "onda de lote é do binário: {:?}", wave.fields);
    assert_eq!(wave.ints("order"), vec![t1, t2], "as duas tarefas do lote, na ordem de despacho");
    assert_eq!(wave.ints("criteria"), vec![crit_id, other_id], "os critérios são a união do que as tarefas cobrem");
    assert_eq!(
        wave.str_field("done_when"),
        Some("git --version && git status --short"),
        "a prova dos dois critérios cobertos"
    );

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
/// do teste: a onda segue em andamento não importa quem lançou a suíte.
fn keep_sent(project: &Project, waves: &[u64]) {
    let (pid, started) = test_process();
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
        project.seed("send", &Value::Object(draft));
    }
}

/// Uma rodada que solta o que estiver pronto, como quem conduz a obra faz: as
/// ondas prontas saem na mesma rodada, sem escolha de quem conduz. Devolve as
/// ondas que pediram a escolha — nenhuma, a rodada não pede mais — e as que
/// saíram; as que saíram ficam em andamento pelo processo do teste
/// ([`keep_sent`]).
fn dispatch_ready(project: &Project) -> (Vec<u64>, Vec<u64>) {
    let first = project.run(&["round", "--spec", SPEC]);
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

/// [`backlog_project`] com a marca de prioridade nas tarefas das posições
/// `marked` de `files`.
fn marked_backlog_project(files: &[&[&str]], marked: &[usize]) -> (Project, u64, u64, Vec<u64>) {
    let project = Project::new();
    project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
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
    let star_wave =
        log.visible().into_iter().find(|e| e.event_type == "wave" && e.wave() == Some(star)).expect("a onda do curinga");
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
/// caem na mesma onda, e com `src/**` em andamento a tarefa de `src/b.rs`
/// espera no backlog, sem onda, enquanto a de `docs/`, que não cruza com ele,
/// sai na sua.
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
    let outside_wave = wave_of(&project, outside).expect("docs vira onda");
    assert_eq!(wave_of(&project, inside), None, "src/b.rs, presa a src/** em andamento, espera no backlog");
    assert_eq!(out, vec![outside_wave], "docs, que src/** não casa, sai");
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
                    let Ok(asked) = serde_json::from_slice::<Value>(&body) else { return };
                    log.lock().unwrap().push(asked.clone());
                    let (status, reply) = answer(&asked);
                    let reply = reply.to_string();
                    let head = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                        reply.len()
                    );
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
    fn judging(
        kind_of: impl Fn(usize) -> (&'static str, f64) + Send + Sync + 'static,
        clash_of: impl Fn(u64, usize) -> f64 + Send + Sync + 'static,
    ) -> Self {
        Self::judging_sized(kind_of, clash_of, |_| 0.0)
    }

    /// O Jev de [`FakeJev::judging`], que também responde a cada pergunta de
    /// tamanho com a nota `size_of(posição)`, de 0 a 3.
    fn judging_sized(
        kind_of: impl Fn(usize) -> (&'static str, f64) + Send + Sync + 'static,
        clash_of: impl Fn(u64, usize) -> f64 + Send + Sync + 'static,
        size_of: impl Fn(usize) -> f64 + Send + Sync + 'static,
    ) -> Self {
        Self::start(move |asked| {
            let keys: Vec<&String> = asked["questions"].as_object().expect("the questions").keys().collect();
            let mut tasks: Vec<u64> =
                keys.iter().filter_map(|key| key.strip_prefix("tipo_t")?.parse().ok()).collect();
            tasks.sort_unstable();
            let position = |task: &str| tasks.iter().position(|id| id.to_string() == task).expect("a task of the request");
            let mut answers = serde_json::Map::new();
            for key in keys {
                if let Some(task) = key.strip_prefix("tipo_t") {
                    let (kind, confidence) = kind_of(position(task));
                    answers.insert(key.clone(), json!({"type": "choice", "choice": kind, "confidence": confidence}));
                } else if let Some(task) = key.strip_prefix("tam_t") {
                    answers.insert(key.clone(), json!({"type": "score", "score": size_of(position(task))}));
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
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "wave" && e.str_field("author") == Some("binary"))
        .map(|wave| wave.ints("order"))
        .collect()
}

/// As chamadas `wave assembly` gravadas na spec.
fn assembly_calls(project: &Project) -> Vec<serde_json::Map<String, Value>> {
    let log = project.log();
    log.visible()
        .into_iter()
        .filter(|e| e.event_type == "call" && e.str_field("command") == Some("wave assembly"))
        .map(|call| call.fields.clone())
        .collect()
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

    project.run(&["round", "--spec", SPEC]);

    assert_eq!(batch_orders(&project), vec![tasks.clone()], "uma onda com as três do mesmo tipo");
    let asked = jev.requests();
    assert_eq!(asked.len(), 1, "o backlog inteiro vai numa chamada só");
    assert_eq!(asked[0]["model"], json!("jev-1.13.0"));
    assert_eq!(asked[0]["questions"].as_object().unwrap().len(), 6, "o tipo e o tamanho por tarefa: {}", asked[0]);
    let calls = assembly_calls(&project);
    assert_eq!(calls.len(), 1, "{calls:?}");
    let call = &calls[0];
    assert_eq!(
        (call["filter"].clone(), call["tokens"].clone(), call["cost_micro_usd"].clone(), call["model"].clone()),
        (json!("jev"), json!(1500), json!(63), json!("jev-1.13.0")),
        "{call:?}"
    );
    assert_eq!((call["requests"].clone(), call["candidates"].clone(), call["returned"].clone()), (json!(1), json!(3), json!(3)));
}

/// Toda onda sai no modelo do projeto, com o Jev ligado ou não, e ainda que o
/// `mustard.json` traga a chave `light_model` de antes: o envio grava o modelo
/// do projeto, a resposta do despacho e o texto do próximo passo não citam
/// modelo nenhum, o pedido da onda diz o do projeto, e o Jev não pergunta
/// mais se a tarefa é mecânica.
#[test]
fn every_wave_goes_out_on_the_project_model_and_the_jev_is_not_asked_about_it() {
    let jev = FakeJev::judging(|position| if position == 0 { ("defect", 0.9) } else { ("feature", 0.9) }, |_, _| 0.0);
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"], &["d.rs"]], &jev);
    let config_path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&config_path).expect("config")).expect("json");
    config["agents"] = json!({"model": "opus", "light_model": "claude-haiku-5"});
    std::fs::write(&config_path, config.to_string()).expect("config");

    let out = project.run(&["round", "--spec", SPEC]);

    assert_eq!(batch_orders(&project), vec![vec![tasks[0]], vec![tasks[1], tasks[2]]], "{out}");
    let log = project.log();
    for wave in [1, 2] {
        let send = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(wave)).expect("the send");
        assert_eq!(send.str_field("model"), Some("opus"), "a onda {wave} vai no modelo do projeto: {out}");
        let prompt = send.str_field("text").expect("the request");
        assert!(prompt.contains("Modelo desta onda: opus."), "o pedido diz o modelo do projeto: {prompt}");
    }
    let dispatch = out["dispatch"].as_array().expect("the dispatch");
    assert_eq!(dispatch.len(), 2, "{out}");
    assert!(dispatch.iter().all(|sent| sent.get("model").is_none()), "o despacho não traz modelo por onda: {out}");
    let next = out["next"].as_str().expect("the next step");
    assert!(!next.contains("modelo") && !next.contains("`model`"), "o próximo passo não cita modelo: {next}");
    for asked in jev.requests() {
        let questions = asked["questions"].as_object().expect("the questions");
        assert!(questions.keys().all(|key| !key.starts_with("mec_")), "o Jev não é perguntado sobre onda mecânica: {asked}");
    }
}

/// Sem chave do Jev a onda também sai no modelo do projeto, e o envio o grava.
#[test]
fn without_a_key_the_wave_goes_out_on_the_project_model() {
    let (project, _, _, _) = backlog_project(&[&["a.rs"]]);

    let out = project.run(&["round", "--spec", SPEC]);

    let log = project.log();
    let send = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the send");
    assert_eq!(send.str_field("model"), Some("sonnet"), "{out}");
    assert!(out["dispatch"][0].get("model").is_none(), "{out}");
}

/// Duas tarefas de tipos diferentes que dividem um arquivo nunca saem juntas,
/// em ondas separadas ou na mesma: sai a do tipo que vem primeiro, e a outra
/// espera no backlog, sem onda, ainda que haja vaga para as duas.
#[test]
fn kinds_with_a_file_in_common_never_run_together() {
    let jev = FakeJev::judging(|at| if at == 0 { ("feature", 0.9) } else { ("defect", 0.9) }, |_, _| 0.0);
    let (project, _, _, tasks) = judged_project(&[&["shared.rs"], &["shared.rs"]], &jev);
    let (feature, defect) = (tasks[0].min(tasks[1]), tasks[0].max(tasks[1]));

    project.run(&["round", "--spec", SPEC]);

    assert_eq!(batch_orders(&project), vec![vec![defect]], "só o defeito sai");
    assert_eq!(wave_of(&project, feature), None, "o recurso espera no backlog, sem onda");
}

/// O defeito sai antes da limpeza, ainda que a limpeza tenha o número mais
/// baixo: com uma vaga só, é o defeito que a ocupa.
#[test]
fn a_defect_leaves_before_a_cleanup_even_with_the_lower_number() {
    let jev = FakeJev::judging(|at| if at == 0 { ("test_cleanup", 0.9) } else { ("defect", 0.9) }, |_, _| 0.0);
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"]], &jev);
    std::fs::write(project.root.join("mustard.json"), json!({
        "language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
        "lintCommand": "git --version", "maxCompilingWaves": 1}).to_string()).expect("config");
    let (cleanup, defect) = (tasks[0].min(tasks[1]), tasks[0].max(tasks[1]));

    project.run(&["round", "--spec", SPEC]);

    assert_eq!(batch_orders(&project), vec![vec![defect]], "o defeito ocupa a única vaga");
    assert_eq!(wave_of(&project, cleanup), None, "a limpeza, de número mais baixo, espera");
}

/// A tarefa de tipo incerto — confiança abaixo de 0,5 — sai sozinha, na onda
/// dela; as de tipo certo, do mesmo tipo, saem juntas.
#[test]
fn a_task_of_an_uncertain_kind_goes_alone() {
    let jev = FakeJev::judging(|at| ("feature", if at == 0 { 0.4 } else { 0.9 }), |_, _| 0.0);
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"], &["d.rs"]], &jev);
    let mut by_number = tasks.clone();
    by_number.sort_unstable();

    project.run(&["round", "--spec", SPEC]);

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
    let (mut project, crit, said, tasks) = backlog_project(&[&["a.rs"]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![wave_of(&project, tasks[0]).expect("a primeira onda")], "a onda de a.rs sai e fica no ar");
    let held = seed_backlog_task(&project, crit, said, &["x1.rs", "x2.rs", "x3.rs", "x4.rs", "x5.rs", "x6.rs"]);

    let strict = FakeJev::judging(|_| ("feature", 0.9), |_, _| 0.8);
    project.jev = Some(strict.url.clone());
    project.run(&["round", "--spec", SPEC]);
    assert_eq!(wave_of(&project, held), None, "o choque de 0,8 segura a tarefa que não divide arquivo");
    assert_eq!(strict.requests()[0]["questions"].as_object().unwrap().len(), 3, "tipo, tamanho e bloqueio da tarefa");

    let loose = FakeJev::judging(|_| ("feature", 0.9), |_, _| 0.1);
    project.jev = Some(loose.url.clone());
    project.run(&["round", "--spec", SPEC]);
    assert!(wave_of(&project, held).is_some(), "com o choque de 0,1 a tarefa sai");
}

/// A montagem fecha a onda no teto de tamanho que o Jev estimou: três tarefas do
/// mesmo tipo e de nível 3 (125 mil tokens cada) saem em três ondas, e três de
/// nota 0,5 (50 mil), em duas, as duas primeiras juntas (100 mil).
#[test]
fn the_wave_closes_at_the_size_budget_the_jev_estimated() {
    let biggest = FakeJev::judging_sized(|_| ("feature", 0.9), |_, _| 0.0, |_| 3.0);
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"], &["d.rs"]], &biggest);
    project.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![vec![tasks[0]], vec![tasks[1]], vec![tasks[2]]], "nível 3: uma por onda");
    assert_eq!(biggest.requests().len(), 1, "o tamanho vem na mesma chamada do tipo");

    let small = FakeJev::judging_sized(|_| ("feature", 0.9), |_, _| 0.0, |_| 0.5);
    let (project, _, _, tasks) = judged_project(&[&["a.rs"], &["c.rs"], &["d.rs"]], &small);
    project.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![vec![tasks[0], tasks[1]], vec![tasks[2]]], "nota 0,5: 100 mil cabem, 150 mil não");
}

/// Sem chave, ou com a chamada recusada, o tamanho não entra: três tarefas do
/// mesmo arquivo saem numa onda só, como sempre. Com o Jev dizendo nível 3 para
/// elas, a montagem as separa, e só a primeira sai, porque as outras dividem o
/// arquivo com ela.
#[test]
fn without_a_key_or_with_a_refused_call_the_size_does_not_split_the_wave() {
    let same: [&[&str]; 3] = [&["a.rs"], &["a.rs"], &["a.rs"]];
    let (project, _, _, tasks) = backlog_project(&same);
    project.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![tasks.clone()], "sem chave");

    let refusing = FakeJev::start(|_| (401, json!({})));
    let (refused, _, _, tasks) = judged_project(&same, &refusing);
    refused.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&refused), vec![tasks.clone()], "chamada recusada");

    let biggest = FakeJev::judging_sized(|_| ("feature", 0.9), |_, _| 0.0, |_| 3.0);
    let (judged, _, _, tasks) = judged_project(&same, &biggest);
    judged.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&judged), vec![vec![tasks[0]]], "com o Jev, o nível 3 separa as três");
}

/// Sem chave a montagem é a de hoje — por arquivo, nenhuma chamada —, e com
/// o Jev recusando a chamada também: duas tarefas que dividem um arquivo saem
/// juntas, a de outro arquivo sai na sua, e a chamada que falhou fica
/// gravada com o motivo.
#[test]
fn without_a_key_or_with_a_refused_call_the_assembly_is_the_one_by_file() {
    let files: [&[&str]; 3] = [&["a.rs"], &["a.rs", "b.rs"], &["c.rs"]];
    let (project, _, _, tasks) = backlog_project(&files);
    project.run(&["round", "--spec", SPEC]);
    let by_file = vec![vec![tasks[0], tasks[1]], vec![tasks[2]]];
    assert_eq!(batch_orders(&project), by_file, "sem chave");
    assert!(assembly_calls(&project).is_empty(), "sem chave nada é perguntado");

    let refusing = FakeJev::start(|_| (401, json!({})));
    let (refused, _, _, tasks) = judged_project(&files, &refusing);
    refused.run(&["round", "--spec", SPEC]);
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
    std::fs::write(project.root.join("mustard.json"), json!({
        "language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
        "lintCommand": "git --version", "maxCompilingWaves": 1}).to_string()).expect("config");
}

/// Três tarefas prontas, cada uma no seu arquivo, e a de número maior com a
/// marca de prioridade: com uma vaga só, é ela que sai, sem o Jev e com o
/// Jev de teste, que a julga limpeza, o último tipo da ordem fixa.
#[test]
fn a_marked_task_with_the_highest_number_leaves_first_with_and_without_the_jev() {
    let files: [&[&str]; 3] = [&["a.rs"], &["b.rs"], &["c.rs"]];
    let (project, _, _, tasks) = marked_backlog_project(&files, &[2]);
    one_slot(&project);
    project.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![vec![tasks[2]]], "sem o Jev, a marcada ocupa a vaga");

    let jev = FakeJev::judging(|at| if at == 2 { ("test_cleanup", 0.9) } else { ("defect", 0.9) }, |_, _| 0.0);
    let (mut judged, _, _, tasks) = marked_backlog_project(&files, &[2]);
    judged.jev = Some(jev.url.clone());
    one_slot(&judged);
    judged.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&judged), vec![vec![tasks[2]]], "com o Jev, a marcada passa à frente dos defeitos");
    assert!(
        jev.requests().iter().all(|asked| !asked.to_string().contains(PRIORITY)),
        "o quadro mandado ao Jev não leva a marca: {:?}",
        jev.requests()
    );
}

/// A tarefa marcada pequena, de um arquivo só, sai ao lado de uma onda em
/// andamento sem arquivo em comum; a não marcada do mesmo tamanho espera
/// juntar trabalho.
#[test]
fn a_small_marked_task_leaves_while_another_wave_runs() {
    let (project, crit, said, tasks) = backlog_project(&[&["a.rs"]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![wave_of(&project, tasks[0]).expect("a primeira onda")], "a onda de a.rs sai e fica no ar");
    let plain = seed_backlog_task(&project, crit, said, &["b.rs"]);
    let marked = seed_task_with(&project, crit, said, &["c.rs"], &json!({"priority": PRIORITY}));

    let (_, out) = dispatch_ready(&project);
    let wave = wave_of(&project, marked).expect("a marcada pequena vira onda");
    assert_eq!(out, vec![wave], "só a marcada sai ao lado da onda em andamento");
    assert_eq!(wave_of(&project, plain), None, "a não marcada pequena espera juntar trabalho");
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

/// A marcada que divide arquivo com uma onda em andamento espera no backlog:
/// a marca não passa por cima dos arquivos de quem está no ar.
#[test]
fn a_marked_task_sharing_a_file_with_the_wave_in_progress_waits() {
    let (project, crit, said, tasks) = backlog_project(&[&["a.rs"]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![wave_of(&project, tasks[0]).expect("a primeira onda")]);
    let marked = seed_task_with(&project, crit, said, &["a.rs"], &json!({"priority": PRIORITY}));

    let (_, out) = dispatch_ready(&project);
    assert!(out.is_empty(), "nada sai por cima de a.rs em andamento: {out:?}");
    assert_eq!(wave_of(&project, marked), None, "a marcada espera a onda de a.rs");
}

/// A não marcada de número menor que divide `a.rs` com a marcada sai depois
/// dela: sem o Jev, as duas vão na mesma onda, com a marcada à frente; com o
/// Jev, de tipos diferentes, a marcada sai, e a outra espera no backlog.
#[test]
fn an_unmarked_task_with_a_lower_number_sharing_a_file_leaves_after_the_marked_one() {
    let files: [&[&str]; 2] = [&["a.rs"], &["a.rs"]];
    let (project, _, _, tasks) = marked_backlog_project(&files, &[1]);
    project.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&project), vec![vec![tasks[1], tasks[0]]], "a marcada à frente na mesma onda");

    let jev = FakeJev::judging(|at| if at == 0 { ("defect", 0.9) } else { ("feature", 0.9) }, |_, _| 0.0);
    let (judged, _, _, tasks) = {
        let (mut project, crit, said, tasks) = marked_backlog_project(&files, &[1]);
        project.jev = Some(jev.url.clone());
        (project, crit, said, tasks)
    };
    judged.run(&["round", "--spec", SPEC]);
    assert_eq!(batch_orders(&judged), vec![vec![tasks[1]]], "a marcada sai");
    assert_eq!(wave_of(&judged, tasks[0]), None, "o defeito de número menor espera atrás dela");
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
                answers.insert(key.clone(), json!({"type": "choice", "choice": "feature", "confidence": 0.9}));
            } else if key.starts_with("tam_") {
                answers.insert(key.clone(), json!({"type": "score", "score": 0.0}));
            } else {
                let title = asked["state"]["items"][key]["title"].as_str().unwrap_or_default();
                answers.insert(key.clone(), json!({"type": "noul", "noul": chance_of(title)}));
            }
        }
        (200, json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 1500, "output_tokens": 0}}))
    })
}

/// As chamadas `wave items` gravadas na spec.
fn item_calls(project: &Project) -> Vec<serde_json::Map<String, Value>> {
    project
        .log()
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "call" && e.str_field("command") == Some("wave items"))
        .map(|call| call.fields.clone())
        .collect()
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
    (
        write_rule("Regra sem relação com a onda", json!({"files": ["**"]})),
        write_rule("Regra de outro arquivo", json!({"files": ["outro.rs"]})),
    )
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

    let out = project.run(&["round", "--spec", SPEC]);

    assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
    assert!(out.get("analysis").is_none(), "{out}");
    let items: Vec<Value> = jev
        .all_requests()
        .into_iter()
        .filter(|asked| asked["questions"].as_object().is_some_and(|q| q.keys().all(|key| key.starts_with('i'))))
        .collect();
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

    let out = project.run(&["round", "--spec", SPEC]);

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
// O teto do mês do Jev, de ponta a ponta
// ---------------------------------------------------------------------------

/// Um Jev de mentira que responde à busca: cada candidato com chance de 0,9,
/// a existência também, e `input_tokens` tokens de entrada cobrados.
fn answering_searches(input_tokens: u64) -> FakeJev {
    FakeJev::start(move |asked| {
        let ids = asked["questions"]["where"]["criteria"].as_object().map(|criteria| criteria.keys().cloned().collect::<Vec<_>>());
        let chances: serde_json::Map<String, Value> = ids.unwrap_or_default().into_iter().map(|id| (id, json!(0.9))).collect();
        let answers = json!({"where": {"type": "choice", "choice": "c000", "probabilities": chances},
            "exists": {"type": "noul", "noul": 0.9}});
        (200, json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": input_tokens, "output_tokens": 0}}))
    })
}

/// Grava o mapa do projeto em dia, com as duas funções de `src/main.rs`: a
/// busca lê o mapa sem passar pelo scan.
fn mapped(project: &Project) {
    let root = &project.root;
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", greeting());\n}\n\nfn greeting() -> &'static str {\n    \"oi\"\n}\n",
    )
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

/// A busca feita sem spec deixa o custo do Jev no arquivo da máquina, e ele
/// conta no teto do mês: a primeira busca custa 10,08 dólares, mais que o
/// teto padrão de 10, e a segunda responde só com o mapa, com o aviso do
/// teto, sem pedido nenhum ao serviço.
#[test]
fn a_search_without_a_spec_spends_from_the_month_and_the_spent_month_answers_from_the_map_alone() {
    let jev = answering_searches(240_000_000);
    let mut project = Project::new();
    project.jev = Some(jev.url.clone());
    mapped(&project);
    let root = project.root.to_string_lossy().to_string();
    let search = || project.run(&["map", "search", "--query", "texto da saudação", "--root", &root]);

    let first = search();
    assert_eq!(first["filter"], json!("jev"), "the month had budget left: {first}");
    assert_eq!(jev.all_requests().len(), 1);

    let second = search();
    assert!(second.get("filter").is_none() && second.get("pieces").is_none(), "{second}");
    assert!(second["files"].as_array().is_some_and(|files| !files.is_empty()), "the map answers: {second}");
    assert_eq!(second["warnings"], json!([translate("map.search.over_budget", Locale::PtBr)]), "{second}");
    assert_eq!(jev.all_requests().len(), 1, "the spent month sent nothing more to the service");
}

/// Com o mês já gasto por uma chamada gravada de 10 dólares, o teto padrão, a
/// rodada não pergunta nada ao Jev, que juntaria as três tarefas e levaria a
/// regra de outro arquivo: a montagem é a por arquivo, o pedido leva os itens
/// do padrão, e a rodada avisa que o teto segurou o Jev.
#[test]
fn once_the_month_is_spent_the_round_assembles_and_picks_the_items_without_the_jev() {
    let jev = judging_items(|_| 0.9);
    let files: [&[&str]; 3] = [&["a.rs"], &["a.rs", "b.rs"], &["c.rs"]];
    let (project, _, said, tasks) = judged_project(&files, &jev);
    let (project_code, other_code) = two_rules(&project, said);
    project.seed(
        "call",
        &json!({"author": "binary", "command": "map search", "ms": 900, "result": "ok", "filter": "jev", "tokens": 238_095_238,
            "cost_micro_usd": 10_000_000}),
    );

    let out = project.run(&["round", "--spec", SPEC]);

    assert!(jev.all_requests().is_empty(), "nothing reached the service: {:?}", jev.all_requests());
    assert_eq!(batch_orders(&project), vec![vec![tasks[0], tasks[1]], vec![tasks[2]]], "the assembly by file");
    let log = project.log();
    let sent = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the send");
    let prompt = sent.str_field("text").expect("the request");
    assert!(prompt.contains(&project_code) && !prompt.contains(&other_code), "the default items: {prompt}");
    assert!(sent.fields.get("analysis").is_none(), "{:?}", sent.fields);
    let reasons: Vec<&Value> = out["warnings"].as_array().into_iter().flatten().map(|warning| &warning["reason"]).collect();
    assert_eq!(reasons, [&json!("jev-over-budget")], "{out}");
}
