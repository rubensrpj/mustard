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
#[path = "support/executable.rs"]
mod executable;
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
    /// O programa compilado da branch, quando o projeto é o código-fonte do
    /// Mustard: é ele que roda cada comando.
    compiled: Option<Compiled>,
}

/// O programa compilado da branch do Mustard que roda a obra do próprio
/// Mustard: uma cópia do programa deste pacote na pasta de compilação do
/// projeto, com um `cargo` de mentira que compila sem compilar.
struct Compiled {
    /// A pasta de compilação (`MUSTARD_BUILD_DIR`) do projeto.
    build: PathBuf,
    /// O programa compilado, dentro dela.
    program: PathBuf,
    /// A pasta do `cargo` de mentira, à frente do `PATH`.
    tools: PathBuf,
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
        Self { _dir: dir, root, home, jev: None, session: None, compiled: None }
    }

    /// O projeto como o código-fonte do Mustard: o pacote `mustard-rt`
    /// declarado em `apps/rt`, um `cargo` de mentira que sempre compila e o
    /// programa compilado da branch — uma cópia do programa deste pacote — na
    /// pasta de compilação do projeto. Cada comando passa a rodar por esse
    /// compilado, como na obra do próprio Mustard.
    fn as_mustard_source() -> Self {
        let mut project = Self::new();
        std::fs::create_dir_all(project.root.join("apps/rt")).expect("apps/rt");
        std::fs::write(project.root.join("apps/rt/Cargo.toml"), "[package]\nname = \"mustard-rt\"\nversion = \"0.1.0\"\n").expect("the manifest");
        git(&project.root, &["add", "-A"]);
        git(&project.root, &["commit", "-q", "-m", "o programa"]);
        let tools = project.home.join("ferramentas");
        std::fs::create_dir_all(&tools).expect("tools");
        let log = project.home.join("cargo.log");
        executable::write_executable(&tools.join("cargo"), &format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n", log.display()));
        let build = project.home.join("build");
        let key = mustard_core::io::wave_prompt::development_build_dir(&project.root);
        let program = build.join(key.file_name().expect("the project key")).join("mustard-dev").join("mustard-rt");
        std::fs::create_dir_all(program.parent().expect("the profile folder")).expect("the build folder");
        // Copiado por outro processo, para que o arquivo nunca fique aberto
        // para escrita aqui enquanto um vizinho o roda.
        let copied = Command::new("cp").arg(env!("CARGO_BIN_EXE_mustard-rt")).arg(&program).status().expect("cp runs");
        assert!(copied.success(), "{} was not copied", program.display());
        project.compiled = Some(Compiled { build, program, tools });
        project
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
            .program(self.compiled.as_ref().map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_mustard-rt")), |compiled| compiled.program.clone()))
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
        if let Some(compiled) = &self.compiled {
            let path = std::env::join_paths(std::iter::once(compiled.tools.clone()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())));
            command.env("MUSTARD_BUILD_DIR", &compiled.build).env("PATH", path.expect("the tools folder joins the PATH"));
        }
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
    backlog_in(Project::new(), files, marked)
}

/// [`marked_backlog_project`] no projeto `project`, que o teste monta antes.
fn backlog_in(project: Project, files: &[&[&str]], marked: &[usize]) -> (Project, u64, u64, Vec<u64>) {
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

/// A remoção que tiraria a última tarefa de uma onda cujo agente ainda
/// trabalha é recusada, nomeando a onda, e não grava nada: o agente não fica
/// sem a tarefa que entrega. Depois da entrega dele a mesma remoção passa, e
/// a resposta diz que a onda saiu do ar. A rodada seguinte, com o pedido
/// ainda aberto, não despacha a onda de novo nem a lista em andamento.
#[test]
fn removing_the_last_task_of_a_wave_whose_agent_works_is_refused_until_the_delivery() {
    let (project, _, _, tasks) = backlog_project(&[&[GREETING]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![1], "a tarefa sai na onda 1");
    let removal = json!({"targets": [tasks[0]], "reason": "Já foi feito fora da onda."});
    let written = project.log().events.len();

    let refused = project.answer(&["write", "remove", "--spec", SPEC, "--json", &removal.to_string()]);
    assert_eq!(refused["ok"], json!(false), "{refused}");
    assert_eq!(refused["reason"], json!("wave-agent-working"), "{refused}");
    let hint = refused["hint"].as_str().expect("the refusal teaches");
    assert!(hint.contains("onda 1") && hint.contains("ainda trabalha"), "a recusa diz a onda e que o agente trabalha: {hint}");
    assert_eq!(project.log().events.len(), written, "a recusa não grava nada");
    assert_eq!(wave_of(&project, tasks[0]), Some(1), "a tarefa segue na onda");

    deliver_in_copy(&project, 1, "hello\n");
    let removed = project.write("remove", &removal);
    assert_eq!(removed["waves_left_without_tasks"], json!([1]), "depois da entrega a remoção passa e a onda sai do ar: {removed}");

    let next = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(!waves_in(&next, "dispatch").contains(&1), "a onda 1 não sai de novo: {next}");
    assert!(!waves_in(&next, "running").contains(&1), "a onda 1 não fica em andamento: {next}");
}

/// A tarefa cuja dependência foi removida (a remoção que o programa de
/// antes aceitava, semeada no arquivo) fica no backlog com o motivo e o nome
/// da dependência, e só ela: a rodada despacha a outra tarefa pronta em vez
/// de parar inteira na gravação da versão da dependente.
#[test]
fn a_task_whose_dependency_is_gone_waits_with_its_reason_and_the_rest_leaves() {
    let (project, crit, said, tasks) = backlog_project(&[&["a.rs"], &["b.rs"]]);
    let dependent = seed_task_with(&project, crit, said, &["c.rs"], &json!({"depends_on": [tasks[0]]}));
    project.seed("remove", &json!({"targets": [tasks[0]], "reason": "Já foi feito fora."}));
    let gone = project.log().codes()[&tasks[0]].clone();

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);
    let dispatched = waves_in(&out, "dispatch");
    assert_eq!(dispatched.len(), 1, "a tarefa pronta sai: {out}");
    assert_eq!(wave_of(&project, tasks[1]), Some(dispatched[0]), "{out}");
    assert_eq!(wave_of(&project, dependent), None, "a dependente fica no backlog");
    let held = out["held"].as_array().unwrap_or_else(|| panic!("a rodada diz o que ficou: {out}"));
    let code = project.log().codes()[&dependent].clone();
    assert_eq!(held.len(), 1, "{out}");
    assert_eq!(held[0]["reason"], json!("broken-dependency"), "{out}");
    assert_eq!(held[0]["tasks"], json!([code]), "{out}");
    assert!(held[0]["detail"].as_str().unwrap_or_default().contains(&gone), "o motivo nomeia a dependência: {out}");
}

/// A rodada recusada pelo teto do pedido deixa a onda de lote montada e sem
/// envio. Antes de a rodada seguinte desfazer essa onda, a remoção que o
/// programa de antes aceitava (semeada no arquivo) tira da spec a tarefa de
/// que outra dessa onda depende. A dependente volta solta ao backlog com a
/// dependência que já trazia, sem recusa, e a rodada segue: as outras tarefas
/// saem em onda, e o motivo que segura a dependente nomeia só a dependência
/// quebrada, não a tarefa aberta de que ela também espera.
#[test]
fn a_task_of_an_unsent_wave_that_lost_a_dependency_returns_to_the_backlog_and_the_round_goes_on() {
    let project = Project::new();
    project.open();
    let said = survey(&project);
    let criterion = |proof: String| {
        let written = project.write(
            "criterion",
            &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": proof,
                "form": "ubiquitous", "origin": said}),
        );
        written["id"].as_u64().expect("the criterion has an id")
    };
    // A prova do critério abre o pedido da onda: a do primeiro passa do teto de tokens.
    let big = criterion(format!("git --version {}", "a".repeat(120_000)));
    let small = criterion("git --version".to_string());
    let gone = backlog_task(&project, big, said, &["a.rs"], &[]);
    let open = backlog_task(&project, small, said, &["a.rs"], &[]);
    let dependent = backlog_task(&project, small, said, &["a.rs"], &[gone, open]);
    project.run(&["plan", "--spec", SPEC]);
    approve(&project);

    let refused = project.answer(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(refused["reason"], json!("wave-token-cap"), "{refused}");
    let log = project.log();
    assert!(log.visible().into_iter().all(|e| e.event_type != "send"), "a onda montada não foi enviada");
    assert_eq!(log.current(dependent).and_then(|task| task.wave()), Some(1), "a dependente está na onda montada");

    project.seed("remove", &json!({"targets": [gone], "reason": "Já foi feito fora."}));
    let codes = project.log().codes();
    let (gone_code, open_code, dependent_code) = (codes[&gone].clone(), codes[&open].clone(), codes[&dependent].clone());
    let other = backlog_task(&project, small, said, &["b.rs"], &[]);

    let out = project.run(&["round", "--dispatch", "--spec", SPEC]);

    let dispatched = waves_in(&out, "dispatch");
    assert_eq!(dispatched.len(), 2, "as duas tarefas prontas saem, cada uma na sua onda: {out}");
    assert!(dispatched.contains(&wave_of(&project, open).expect("a tarefa aberta ganha onda")), "{out}");
    assert!(dispatched.contains(&wave_of(&project, other).expect("a tarefa nova ganha onda")), "{out}");
    assert_eq!(wave_of(&project, dependent), None, "a dependente volta ao backlog: {out}");
    let log = project.log();
    let kept = log.current(dependent).map(|task| task.fields["depends_on"].clone());
    assert_eq!(kept, Some(json!([gone, open])), "a volta guarda as dependências que a versão anterior trazia");
    let held = out["held"].as_array().unwrap_or_else(|| panic!("a rodada diz o que ficou: {out}"));
    assert_eq!(held.len(), 1, "{out}");
    assert_eq!((held[0]["reason"].clone(), held[0]["tasks"].clone()), (json!("broken-dependency"), json!([dependent_code])), "{out}");
    let detail = held[0]["detail"].as_str().unwrap_or_default();
    assert!(detail.contains(&gone_code), "o motivo nomeia a dependência quebrada: {out}");
    assert!(!detail.contains(&open_code), "o motivo não nomeia a tarefa aberta e válida: {out}");
}

/// O bloco de retomada que o aviso antes de compactar traz, pelo gancho
/// `PreCompact` da sessão que conduz a obra, como o Claude Code o chama.
fn precompact_block(project: &Project) -> String {
    let payload = json!({"hook_event_name": "PreCompact", "trigger": "auto", "session_id": SESSION,
        "cwd": project.root.to_string_lossy()});
    let out = project.command(&["on", "PreCompact"], &payload.to_string());
    assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("the hook answers JSON ({e}): {}", String::from_utf8_lossy(&out.stdout)));
    answer["systemMessage"].as_str().unwrap_or_else(|| panic!("the hook brings the resume block: {answer}")).to_string()
}

/// A volta gravada de uma onda que saiu do ar depois dela — a remoção tirou a
/// última tarefa da onda depois da entrega — não espera a rodada, que nunca a
/// assume: a retomada não a lista entre as voltas à espera, e o bloco de
/// retomada do aviso antes de compactar diz que nenhuma volta espera. Antes
/// da remoção, as duas leituras a mostram.
#[test]
fn the_return_of_a_wave_that_left_the_air_does_not_wait_in_the_resume() {
    let (project, _, _, tasks) = backlog_project(&[&[GREETING]]);
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "a tarefa sai na onda 1: {first}");
    deliver_in_copy(&project, 1, "hello\n");
    let awaiting = || {
        let resumed = project.run(&["resume", "--spec", SPEC]);
        waves_in(&resumed["since_last_message"], "awaiting_review")
    };
    let block = translate("conversation_size.block", Locale::PtBr);
    let label = block.split("{returned}").next().and_then(|before| before.rsplit(". ").next()).expect("the label of the returns");
    let waiting = |list: &str| format!("{label}{list}.");
    assert_eq!(awaiting(), vec![1], "a volta da onda no plano espera a rodada");
    let before = precompact_block(&project);
    assert!(before.contains(&waiting("1")), "o bloco lista a volta da onda no plano: {before}");

    let removal = json!({"targets": [tasks[0]], "reason": "Já foi feito fora da onda."});
    let removed = project.write("remove", &removal);
    assert_eq!(removed["waves_left_without_tasks"], json!([1]), "a remoção tira a onda 1 do ar: {removed}");

    assert_eq!(awaiting(), Vec::<u64>::new(), "a volta da onda fora do ar não espera a rodada");
    let after = precompact_block(&project);
    assert!(after.contains(&waiting(translate("resume.none", Locale::PtBr))), "o bloco não lista a volta da onda fora do ar: {after}");
}

/// A onda cujo agente trabalha segue com tarefa quando a remoção tira só uma
/// das duas: a remoção passa, e a onda não sai do ar.
#[test]
fn removing_one_of_two_tasks_of_a_wave_whose_agent_works_goes_through() {
    let (project, _, _, tasks) = backlog_project(&[&["a.rs"], &["a.rs"]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![1], "as duas tarefas saem na onda 1");

    let removed = project.write("remove", &json!({"targets": [tasks[0]], "reason": "Já foi feito fora da onda."}));
    assert!(removed.get("waves_left_without_tasks").is_none(), "a onda segue com a outra tarefa: {removed}");
    assert_eq!(wave_of(&project, tasks[1]), Some(1), "a outra tarefa segue na onda");
}

/// A onda que já saiu do ar sem a volta do agente — a remoção que tirou a
/// última tarefa dela vem de uma versão sem a recusa, semeada no arquivo —
/// não trava a gravação que não tem relação com ela: a nota passa. A remoção
/// da última tarefa da outra onda, cujo agente trabalha, segue recusada, e a
/// recusa nomeia essa onda, e não a que já tinha saído.
#[test]
fn a_write_unrelated_to_a_wave_already_out_of_the_air_goes_through() {
    let (project, _, _, tasks) = backlog_project(&[&["web/app.ts"], &["src/Program.cs"]]);
    let (_, out) = dispatch_ready(&project);
    assert_eq!(out, vec![1, 2], "cada tarefa sai na sua onda");
    project.seed("remove", &json!({"targets": [tasks[0]], "reason": "Já foi feito fora da onda."}));
    let log = project.log();
    let planned = log.planned_waves();
    assert!(!planned.contains(&1) && planned.contains(&2), "a onda 1 saiu do ar e a 2 segue no plano: {planned:?}");
    assert!(
        log.visible().into_iter().all(|e| e.event_type != "delivered" || e.wave() != Some(1)),
        "o agente da onda 1 não voltou"
    );

    let written = log.events.len();
    let noted = project.write("note", &json!({"title": "Observação", "text": "A onda 1 saiu do ar antes da entrega.", "keys": ["onda"]}));
    assert_eq!(noted["type"], json!("note"), "{noted}");
    assert_eq!(project.log().events.len(), written + 1, "a nota foi gravada");

    let removal = json!({"targets": [tasks[1]], "reason": "Já foi feito fora da onda."});
    let refused = project.answer(&["write", "remove", "--spec", SPEC, "--json", &removal.to_string()]);
    assert_eq!(refused["reason"], json!("wave-agent-working"), "{refused}");
    let hint = refused["hint"].as_str().expect("the refusal teaches");
    assert!(hint.contains("onda 2") && !hint.contains("onda 1"), "a recusa nomeia a onda que o agente ainda trabalha: {hint}");
    assert_eq!(wave_of(&project, tasks[1]), Some(2), "a tarefa segue na onda 2");
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
    answer_reserves_with(project, out, certainty, &|text| if text.contains("What kind of work") { "feature" } else { "no" })
}

/// [`answer_reserves`] com a escolha de cada pergunta dada por `choice`, a
/// partir do texto dela.
fn answer_reserves_with(project: &Project, out: &Value, certainty: &str, choice: &dyn Fn(&str) -> &'static str) -> usize {
    let requests = out["reserves"].as_array().cloned().unwrap_or_default();
    for (at, request) in requests.iter().enumerate() {
        let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_default();
        let answers: serde_json::Map<String, Value> = (1..=request["questions"].as_u64().unwrap_or_default())
            .map(|number| {
                let id = format!("q{number}");
                let text = prompt.lines().find_map(|line| line.strip_prefix(&format!("{id}: "))).unwrap_or_default();
                (id, json!({"choice": choice(text), "certainty": certainty}))
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
    slots(project, 1);
}

/// Deixa o projeto com `count` vagas: até `count` ondas ao mesmo tempo.
fn slots(project: &Project, count: usize) {
    std::fs::write(
        project.root.join("mustard.json"),
        json!({
        "language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}, "provider": "github"},
        "ai": {"fallback": true}, "search": {"filter": "jev"},
        "judgement": {"wave-planning": {"filter": "jev"}, "context": {"filter": "jev"}},
        "lintCommand": "git --version", "maxCompilingWaves": count})
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
    FakeJev::start(move |asked| items_answer(asked, &chance_of))
}

/// A resposta de [`judging_items`] ao pedido `asked`: o tipo de cada tarefa é
/// um recurso certo, e cada item ganha a chance que `chance_of` dá ao título.
fn items_answer(asked: &Value, chance_of: &dyn Fn(&str) -> f64) -> (u16, Value) {
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
}

/// `true` quando o pedido `asked` ao Jev é o da escolha dos itens de uma
/// onda: cada pergunta dele é a de um item.
fn asks_items(asked: &Value) -> bool {
    asked["questions"].as_object().is_some_and(|questions| questions.keys().all(|key| key.starts_with('i')))
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
    let items: Vec<Value> = jev.all_requests().into_iter().filter(asks_items).collect();
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

/// O Jev monta a onda do backlog e recusa só a escolha dos itens dela: a
/// rodada grava a onda e a segura, avisando que ela espera a escolha, e manda
/// a pergunta à reserva. O recibo, pela porta do `run knowledge
/// --complete-session`, decide no lugar do Jev, e a rodada seguinte despacha a
/// mesma onda, com o número que ela ganhou, sem perguntar de novo e sem deixar
/// onda gravada sem tarefa.
#[test]
fn a_wave_waiting_for_the_item_choice_leaves_with_its_number_after_the_receipt() {
    let jev = FakeJev::start(|asked| if asks_items(asked) { (400, json!({"error": "refused"})) } else { items_answer(asked, &|_| 0.5) });
    let (mut project, _, _, tasks) = backlog_project(&[&["a.rs"]]);
    project.jev = Some(jev.url.clone());
    let asked_items = || jev.all_requests().iter().filter(|asked| asks_items(asked)).count();

    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert!(waves_in(&first, "dispatch").is_empty(), "the assembled wave waits for the receipt: {first}");
    let waiting = warned_waves(&first, "items-jev-pending");
    assert_eq!(waiting.len(), 1, "{first}");
    assert_eq!(batch_orders(&project), vec![tasks], "the waiting wave stays recorded with its task: {first}");
    assert_eq!(asked_items(), 1, "the item choice reaches the Jev once: {first}");
    assert_eq!(answer_reserves(&project, &first, "sure"), 1, "only the item choice goes to the reserve: {first}");

    let sent = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(waves_in(&sent, "dispatch"), waiting, "the wave that waited leaves with its number: {sent}");
    assert!(sent.get("reserves").is_none(), "{sent}");
    assert_eq!(asked_items(), 1, "the receipt answers in place of the Jev: {sent}");
    let log = project.log();
    let recorded: BTreeSet<u64> = log.events.iter().filter(|e| e.event_type == "wave").filter_map(|e| e.wave()).collect();
    assert_eq!(recorded, log.planned_waves(), "no wave stays recorded without tasks: {sent}");
    let send = log.visible().into_iter().find(|e| e.event_type == "send" && e.wave() == Some(waiting[0])).expect("the send");
    let removed = send.fields["analysis"]["removed"].as_array().map_or(0, Vec::len);
    assert!(removed > 0, "the receipt decides like the Jev: {:?}", send.fields);
}

/// `true` quando o pedido `asked` ao Jev é o da escolha das skills de uma
/// tarefa: o estado traz as skills candidatas.
fn asks_skills(asked: &Value) -> bool {
    asked["state"].get("skills").is_some()
}

/// Um Jev de mentira que recusa só a escolha das skills e responde ao resto
/// como [`judging_items`], com a mesma chance para cada item.
fn refusing_the_skill_pick() -> FakeJev {
    FakeJev::start(|asked| if asks_skills(asked) { (400, json!({"error": "refused"})) } else { items_answer(asked, &|_| 0.5) })
}

/// Cinco skills no `.claude/skills` do projeto, cada uma com `paths` em
/// `src/**`: a tarefa que mexe em `src` tem mais candidatas do que cabem no
/// bloco dela, e a escolha das skills vai ao Jev.
fn tied_skills(project: &Project) {
    for name in ["a1", "a2", "a3", "a4", "a5"] {
        let dir = project.root.join(".claude/skills").join(name);
        std::fs::create_dir_all(&dir).expect("the skill folder");
        let text = format!("---\nname: {name}\ndescription: Does {name}.\npaths:\n  - src/**\n---\n\nAlways sum in whole cents in {name}.\n");
        std::fs::write(dir.join("SKILL.md"), text).expect("the skill");
    }
}

/// A escolha do recibo da escolha das skills: só a quarta e a quinta
/// candidatas fazem o que a tarefa pede.
fn the_fourth_and_fifth_skills(text: &str) -> &'static str {
    if text.contains("skills.s4") || text.contains("skills.s5") { "yes" } else { "no" }
}

/// As ondas que a resposta `out` da rodada avisa esperar pelo motivo
/// `reason`.
fn warned_waves(out: &Value, reason: &str) -> Vec<u64> {
    out["warnings"].as_array().into_iter().flatten().filter(|w| w["reason"] == json!(reason)).filter_map(|w| w["wave"].as_u64()).collect()
}

/// As skills do primeiro bloco do último envio da onda `wave`.
fn sent_skills(project: &Project, wave: u64) -> Value {
    let log = project.log();
    let sent = log.last_by_wave("send")[&wave];
    log.send_field(sent, "tasks").cloned().unwrap_or_default()[0]["skills"].clone()
}

/// A rodada que despachou a onda segurada pela escolha das skills, depois do
/// recibo: a onda que esperou sai com o número dela e as skills do recibo,
/// sem nova pergunta ao Jev nem pedido à reserva, e nenhuma onda fica gravada
/// sem tarefa.
fn the_held_wave_left(project: &Project, sent: &Value, waiting: &[u64], asked_skills: usize) {
    assert_eq!(waves_in(sent, "dispatch"), waiting, "the wave that waited leaves with its number: {sent}");
    assert!(sent.get("reserves").is_none(), "{sent}");
    assert_eq!(asked_skills, 1, "the receipt answers in place of the Jev: {sent}");
    assert_eq!(sent_skills(project, waiting[0]), json!(["a4", "a5"]), "the skills the receipt chose: {sent}");
    let log = project.log();
    let recorded: BTreeSet<u64> = log.events.iter().filter(|e| e.event_type == "wave").filter_map(|e| e.wave()).collect();
    assert_eq!(recorded, log.planned_waves(), "no wave stays recorded without tasks: {sent}");
}

/// O Jev monta a onda do backlog, cuja tarefa tem mais skills candidatas do
/// que cabem no bloco, e recusa só a escolha das skills: a rodada grava a
/// onda e a segura, avisando que ela espera a escolha, e manda a pergunta à
/// reserva. O recibo, pela porta do `run knowledge --complete-session`,
/// escolhe no lugar do Jev, e a rodada seguinte despacha a mesma onda, com o
/// número que ela ganhou e as skills do recibo.
#[test]
fn a_wave_waiting_for_the_skill_pick_leaves_with_its_number_after_the_receipt() {
    let jev = refusing_the_skill_pick();
    let (mut project, _, _, tasks) = backlog_project(&[&["src/a.rs"]]);
    tied_skills(&project);
    project.jev = Some(jev.url.clone());
    let asked_skills = || jev.all_requests().iter().filter(|asked| asks_skills(asked)).count();

    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert!(waves_in(&first, "dispatch").is_empty(), "the assembled wave waits for the receipt: {first}");
    let waiting = warned_waves(&first, "skill-pick-reserve");
    assert_eq!(waiting.len(), 1, "{first}");
    assert_eq!(batch_orders(&project), vec![tasks], "the waiting wave stays recorded with its task: {first}");
    assert_eq!(asked_skills(), 1, "the skill pick reaches the Jev once: {first}");
    assert_eq!(answer_reserves_with(&project, &first, "sure", &the_fourth_and_fifth_skills), 1, "only the skill pick goes to the reserve: {first}");

    let sent = project.run(&["round", "--dispatch", "--spec", SPEC]);

    the_held_wave_left(&project, &sent, &waiting, asked_skills());
}

/// O Jev decide os itens e as skills da onda antes de ela ser montada, e um
/// arquivo ocupa o lugar da pasta da vaga: a onda fica montada e não sai, por
/// falta de cópia, sem esperar decisão nenhuma. A rodada seguinte, com a vaga
/// livre, desfaz a onda, devolve a tarefa ao backlog e a monta de novo com o
/// número seguinte — o da desfeita não volta a nascer —, sem deixar onda vazia
/// para trás.
#[test]
fn an_assembled_wave_with_its_decisions_recorded_and_no_copy_is_undone_and_leaves_with_a_new_number() {
    let jev = FakeJev::start(|asked| items_answer(asked, &|_| 0.5));
    let (mut project, _, _, tasks) = backlog_project(&[&["src/a.rs"]]);
    tied_skills(&project);
    project.jev = Some(jev.url.clone());
    let slot = mustard_core::io::wave_prompt::slot_path(&project.root, SPEC, 0);
    std::fs::create_dir_all(slot.parent().expect("the copies folder")).expect("the copies folder");
    std::fs::write(&slot, b"no caminho da copia").expect("the file in the place of the slot");

    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert!(waves_in(&first, "dispatch").is_empty(), "no copy, no dispatch: {first}");
    let failed = first["warnings"].as_array().into_iter().flatten().find(|w| w["reason"] == json!("copy-not-created"));
    assert_eq!(failed.map(|w| w["wave"].clone()), Some(json!(1)), "{first}");
    assert!(warned_waves(&first, "items-jev-pending").is_empty() && warned_waves(&first, "skill-pick-reserve").is_empty(), "the decisions came first: {first}");
    assert_eq!(batch_orders(&project), vec![tasks], "the wave stays assembled: {first}");
    std::fs::remove_file(&slot).expect("the slot is free again");

    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert_eq!(waves_in(&second, "dispatch"), vec![2], "the undone wave comes back with the next number: {second}");
    assert_eq!(project.log().planned_waves(), BTreeSet::from([2]), "no empty wave stays behind: {second}");
}

/// Com a leitura do classificador pendente, a passada com as travas presas
/// só solta as ondas já gravadas, e a que a escolha das skills segurou é uma
/// delas: o recibo chega, duas tarefas novas esperam a leitura da sessão, e a
/// rodada despacha a onda segurada com o número que ela ganhou e as skills do
/// recibo, sem desfazê-la.
#[test]
fn a_wave_waiting_for_the_skill_pick_leaves_with_its_number_while_the_forming_waits() {
    let jev = refusing_the_skill_pick();
    // A montagem é a por arquivo, sem Jev na ordem: a formação das tarefas
    // novas espera a leitura da sessão.
    let project = Project::new();
    let path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("config")).expect("json");
    config["judgement"].as_object_mut().expect("the judgement").remove("wave-planning");
    std::fs::write(&path, config.to_string()).expect("config");
    let (mut project, crit, said, _) = backlog_in(project, &[&["src/a.rs"]], &[]);
    tied_skills(&project);
    project.jev = Some(jev.url.clone());
    let asked_skills = || jev.all_requests().iter().filter(|asked| asks_skills(asked)).count();
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    let waiting = warned_waves(&first, "skill-pick-reserve");
    assert_eq!(waiting.len(), 1, "{first}");
    assert_eq!(answer_reserves_with(&project, &first, "sure", &the_fourth_and_fifth_skills), 1, "{first}");
    let loose = [backlog_task(&project, crit, said, &["lib/b.rs"], &[]), backlog_task(&project, crit, said, &["lib/c.rs"], &[])];
    project.session = Some(SESSION);

    let sent = project.run(&["round", "--dispatch", "--spec", SPEC]);

    assert!(sent.get("formation").is_some(), "the new tasks wait for the reading of the session: {sent}");
    the_held_wave_left(&project, &sent, &waiting, asked_skills());
    assert_eq!(loose.map(|task| wave_of(&project, task)), [None, None], "the new tasks wait for the forming: {sent}");
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
    // As ondas que esperaram a escolha dos itens saem como foram montadas,
    // pelo arquivo, sem voltar a ser montadas.
    let orders = batch_orders(&project);
    assert_eq!(orders, vec![vec![tasks[0], tasks[1]], vec![tasks[2]]], "by file: {orders:?}");
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
    deliver_file_with(project, wave, file, content, &json!({}))
}

/// [`deliver_file_in_copy`] com os campos de `extra` por cima da entrega,
/// como as sobras que a volta relata.
fn deliver_file_with(project: &Project, wave: u64, file: &str, content: &str, extra: &Value) -> PathBuf {
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
    let mut delivered = json!({"wave": wave, "text": "A saudação saiu.", "files": [file], "commit": "saudação nova",
        "agreed": agreed});
    for (key, value) in extra.as_object().into_iter().flatten() {
        delivered[key] = value.clone();
    }
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

/// Um Jev de mentira que diz sim à pergunta da mesma falha e deixa as outras
/// sem resposta.
fn same_failure_jev() -> FakeJev {
    FakeJev::start(|asked| {
        let answers: serde_json::Map<String, Value> = asked["questions"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, question)| question.to_string().contains("the same problem"))
            .map(|(key, _)| (key.clone(), json!({"type": "noul", "noul": 0.95})))
            .collect();
        (200, json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 10, "output_tokens": 0}}))
    })
}

/// As tarefas de criar uma skill gravadas na spec.
fn skill_creator_tasks(project: &Project) -> Vec<mustard_core::domain::spec_events::SpecEvent> {
    project.log().visible().into_iter().filter(|e| e.event_type == "task" && e.str_field("skill") == Some("skill-creator")).cloned().collect()
}

/// A mesma falha reprovada duas vezes na onda cuja tarefa cobre um critério:
/// a tarefa de criar a skill nasce cobrindo esse critério, e, com a onda
/// reprovada enfim aprovada, a rodada a manda numa onda, sem o aviso da
/// tarefa que não cobre nada.
#[test]
fn the_skill_task_of_a_repeated_failure_covers_the_items_of_the_rejected_wave_and_leaves_in_a_wave() {
    let jev = same_failure_jev();
    let (mut project, criterion, _, _) = backlog_project(&[&[GREETING]]);
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    sent_by(&project, &[1], closed_process::closed_process());
    deliver_in_copy(&project, 1, "hello\n");
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    assert!(dispatch_new_agent(&project).pointer("/hookSpecificOutput/updatedInput/prompt").is_some());
    deliver_in_copy(&project, 1, "hi\n");
    project.jev = Some(jev.url.clone());

    assert_eq!(reject(&project, 1, "A saudação saiu em inglês de novo.")["ok"], json!(true));

    let tasks = skill_creator_tasks(&project);
    assert_eq!(tasks.len(), 1, "one skill task for the repeated failure: {tasks:?}");
    assert_eq!(tasks[0].ints("covers"), vec![criterion], "{:?}", tasks[0].fields);
    project.jev = None;
    assert!(dispatch_new_agent(&project).pointer("/hookSpecificOutput/updatedInput/prompt").is_some());
    deliver_in_copy(&project, 1, "olá\n");
    let committed = project.run(&["round", "--dispatch", "--spec", SPEC, "--report", r#"<USAGE>{"wave":1}</USAGE>"#]);
    assert!(committed.get("commit").is_some(), "{committed}");
    let out = if wave_of(&project, tasks[0].id).is_some() { committed } else { project.run(&["round", "--dispatch", "--spec", SPEC]) };
    let warned = out["warnings"].as_array().into_iter().flatten().any(|warning| warning["reason"] == json!("task-without-covers"));
    assert!(!warned, "{out}");
    assert_eq!(wave_of(&project, tasks[0].id), Some(2), "{out}");
}

/// As refs em que a limpeza das cópias guardou código, no repositório
/// principal.
#[cfg(target_os = "linux")]
fn kept_refs(project: &Project) -> Vec<String> {
    let out = Command::new("git").args(["for-each-ref", "--format=%(refname)", "refs/mustard/kept/"]).current_dir(&project.root).output().expect("git");
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

/// O Claude Code que mandou a onda fechou com o agente no meio do trabalho:
/// a rodada seguinte guarda o que ele deixou na cópia, volta a cópia ao
/// commit e reenvia a onda com o pedido que cita a ref guardada e cada
/// arquivo com mudança, mandando o agente novo conferir e continuar dali.
///
/// Só o Linux lê o processo do Claude Code em `/proc` e o vê fechado; fora
/// dele nenhum processo conta como fechado, a rodada nunca dá uma onda por
/// órfã, e a onda só sai de novo quando quem conduz a pausa — reenvio que
/// mantém a cópia como está, sem guardar nada numa ref. O envio com o
/// processo fechado, que este teste grava, nunca existe lá, e rodá-lo no
/// macOS só dá a onda como em andamento.
#[cfg(target_os = "linux")]
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

/// A rodada sem a linha de revisão de quem conduz, pelo binário: a volta que
/// chegou fica esperando a revisão, fora do commit, e guarda a cópia dela.
fn unreviewed_round(project: &Project) -> Value {
    let out = project.command(&["run", "round", "--dispatch", "--spec", SPEC], "");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    assert_eq!(report["ok"], json!(true), "{report}");
    report
}

/// Duas vagas e três cópias, todas guardadas: as ondas 1 e 2 voltaram e
/// esperam a revisão, e a 3 trabalha. Com uma sessão conduzindo, a rodada
/// pede a formação das duas tarefas que sobram, e o recibo do agente de
/// análise volta. A onda 3 volta também, e a vaga dela abre, mas as cópias
/// seguem cheias. A rodada seguinte, com a mesma fila, reaproveita a
/// formação aceita e a grava nas duas tarefas, sem pedir outra.
#[test]
fn with_full_copies_the_next_round_reuses_the_formation_of_the_same_tasks() {
    let mut project = Project::new();
    let path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("config")).expect("json");
    config["maxCompilingWaves"] = json!(2);
    std::fs::write(&path, config.to_string()).expect("config");
    project.open();
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let crit = criterion["id"].as_u64().expect("the criterion id");
    let files = ["src/a.txt", "src/b.txt", "src/c.txt", "src/d.txt", "src/e.txt"];
    let tasks: Vec<u64> = files.iter().map(|file| backlog_task(&project, crit, said, &[file], &[])).collect();
    project.run(&["plan", "--spec", SPEC]);
    approve(&project);

    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
    keep_sent(&project, &[1, 2]);
    deliver_file_in_copy(&project, 1, files[0], "pronto\n");
    deliver_file_in_copy(&project, 2, files[1], "pronto\n");
    let second = unreviewed_round(&project);
    assert_eq!(waves_in(&second, "dispatch"), vec![3], "{second}");
    keep_sent(&project, &[3]);
    project.session = Some(SESSION);

    let asked = unreviewed_round(&project);
    let formation = asked.get("formation").unwrap_or_else(|| panic!("the round asks the forming of the tasks left: {asked}"));
    let full = asked["warnings"].as_array().into_iter().flatten().find(|w| w["reason"] == json!("copies-full")).cloned();
    assert_eq!(full.map(|w| w["waves"].clone()), Some(json!([1, 2])), "every copy is kept: {asked}");
    let codes = project.log().codes();
    let left: Vec<String> = tasks[3..].iter().map(|id| codes[id].clone()).collect();
    formed_by_the_analysis(&project, formation, json!([{"objective": "Trocar a saudação nos arquivos que sobram", "tasks": left, "merges": []}]));
    deliver_file_in_copy(&project, 3, files[2], "pronto\n");

    let next = unreviewed_round(&project);

    assert!(next.get("formation").is_none(), "the same tasks, the formation already accepted: {next}");
    let full = next["warnings"].as_array().into_iter().flatten().find(|w| w["reason"] == json!("copies-full")).cloned();
    assert_eq!(full.map(|w| w["waves"].clone()), Some(json!([1, 2, 3])), "the copies are still full: {next}");
    let log = project.log();
    for task in &tasks[3..] {
        let group = log.current(*task).and_then(|current| current.fields.get("group").cloned());
        assert_eq!(
            group.as_ref().map(|group| (group["name"].clone(), group["formed"].clone())),
            Some((json!("Trocar a saudação nos arquivos que sobram"), json!(true))),
            "the accepted formation is recorded: {next}"
        );
    }
}

/// O recibo do agente de análise para o pedido de formação `formation`, com
/// as ondas `waves`, entregue pela porta `run knowledge --complete-session`.
fn formed_by_the_analysis(project: &Project, formation: &Value, waves: Value) {
    let receipt = json!({"request_id": formation["request_id"], "model": "inherit", "input_tokens": null, "output_tokens": null,
        "result": {"waves": waves}});
    let path = project.home.join("recibo-formacao.json");
    std::fs::write(&path, receipt.to_string()).expect("the receipt");
    let done = project.command(&["run", "knowledge", "--complete-session", path.to_str().expect("a path")], "");
    assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stdout));
}

/// A rodada que despacha a spec das provas da espera pela onda semelhante.
const ROUND: [&str; 4] = ["round", "--dispatch", "--spec", SPEC];

/// O código da tarefa `id`, como o agente a lê.
fn code_of(project: &Project, id: u64) -> String {
    project.log().codes()[&id].clone()
}

/// A volta de `wave` entregue à rodada, que a comita; devolve a resposta.
fn round_taking_back(project: &Project, wave: u64) -> Value {
    let usage = format!("<USAGE>{}</USAGE>", json!({"wave": wave}));
    project.run(&["round", "--dispatch", "--spec", SPEC, "--report", &usage])
}

/// A cópia da onda 1 e os itens que o pedido dela manda ler.
fn copy_and_items_of_wave_one(project: &Project) -> (PathBuf, Vec<String>) {
    let sent = last_send(project, 1);
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    let listed = sent.fields.get("read_items").and_then(Value::as_array).cloned().unwrap_or_default();
    (copy, listed.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

/// O agente lê, de dentro da cópia, o item `item` do pedido.
fn read_item_in(project: &Project, copy: &Path, item: &str) {
    let root = project.root.display().to_string();
    let block = format!("item-{item}");
    let out = project.command_in(copy, &["run", "read", &block, "--root", &root, "--spec", SPEC], "");
    assert!(out.status.success(), "{item}: {}", String::from_utf8_lossy(&out.stdout));
}

/// O agente da onda 1 grava o passo que termina a tarefa `task`, que libera a
/// seguinte.
fn finish_task_of_wave_one(project: &Project, task: u64) {
    project.write("step", &json!({"wave": 1, "item": code_of(project, task), "text": "A tarefa ficou pronta."}));
}

/// A entrega da onda 1, que responde por todo o combinado, com as tarefas
/// `undone` por fazer.
fn deliver_wave_one(project: &Project, text: &str, files: &[&str], undone: &[String]) {
    let agreed: Vec<Value> = mustard_core::domain::wave_prompt::all_agreed(&project.log()).iter().map(|item| json!({"item": item.id, "met": true})).collect();
    project.write("delivered", &json!({"wave": 1, "text": text, "files": files, "commit": "saudação nova", "agreed": agreed, "undone": undone}));
}

/// Duas vagas e uma sessão conduzindo. A formação põe as duas tarefas da
/// saudação numa onda, que sai. Chegam duas tarefas novas do mesmo sentido: a
/// formação recebe o objetivo da onda em andamento, e a onda delas a segue.
/// Com a onda rodando, nenhuma das duas sai, e a rodada diz por quê. Devolve o
/// projeto e as quatro tarefas: as duas da onda 1 e as duas que a seguem.
fn running_wave_with_followers() -> (Project, [u64; 4]) {
    let mut project = Project::new();
    let path = project.root.join("mustard.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("config")).expect("json");
    config["maxCompilingWaves"] = json!(2);
    std::fs::write(&path, config.to_string()).expect("config");
    project.open();
    let said = survey(&project);
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a saudação nova aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": said}),
    );
    let crit = criterion["id"].as_u64().expect("the criterion id");
    let first = backlog_task(&project, crit, said, &["src/a.txt"], &[]);
    let second = backlog_task(&project, crit, said, &["src/b.txt"], &[]);
    project.run(&["plan", "--spec", SPEC]);
    approve(&project);
    project.session = Some(SESSION);
    let objective = "Trocar a saudação do programa em cada arquivo";
    let code = |id: u64| code_of(&project, id);

    let asked = project.run(&ROUND);
    let formation = asked.get("formation").unwrap_or_else(|| panic!("the round asks the forming: {asked}"));
    formed_by_the_analysis(&project, formation, json!([{"objective": objective, "tasks": [code(first), code(second)], "merges": []}]));
    let out = project.run(&ROUND);
    assert_eq!(waves_in(&out, "dispatch"), vec![1], "{out}");
    keep_sent(&project, &[1]);

    let third = backlog_task(&project, crit, said, &["src/c.txt"], &[]);
    let fourth = backlog_task(&project, crit, said, &["src/d.txt"], &[]);
    let asked = project.run(&ROUND);
    let formation = asked.get("formation").unwrap_or_else(|| panic!("the round asks the forming of the new tasks: {asked}"));
    let prompt = formation["delegation"]["request"]["prompt"].as_str().expect("the prompt");
    let running = format!("Running waves: {}", json!([{"key": "w1", "objective": objective}]));
    assert!(prompt.contains(&running), "the forming gets the objective of the running wave: {prompt}");
    formed_by_the_analysis(
        &project,
        formation,
        json!([{"objective": "Trocar a saudação dos arquivos novos", "tasks": [code(third), code(fourth)], "merges": [], "follows": "w1"}]),
    );

    let waiting = project.run(&ROUND);
    assert!(waves_in(&waiting, "dispatch").is_empty(), "with the wave running, no task of its direction leaves: {waiting}");
    let followers = json!([code(third), code(fourth)]);
    let held = waiting["held"].as_array().into_iter().flatten().find(|group| group["reason"] == json!("following-running-wave")).cloned();
    assert_eq!(held.map(|group| group["tasks"].clone()), Some(followers), "the round says why they wait: {waiting}");
    (project, [first, second, third, fourth])
}

/// A onda em andamento volta com um resumo e deixa uma tarefa por fazer: na
/// rodada que a comita, a tarefa deixada chega ao backlog, e as que seguem a
/// onda esperam por ela; na seguinte, as três saem juntas numa onda, que leva
/// o resumo.
#[test]
fn a_task_in_the_direction_of_a_running_wave_waits_for_its_return_and_leaves_with_its_summary() {
    let (project, [first, second, third, fourth]) = running_wave_with_followers();
    let code = |id: u64| code_of(&project, id);

    // O agente faz só a primeira tarefa e para no limite: a segunda nem
    // chega a ser liberada, e a entrega a deixa por fazer, como resumo.
    let (copy, listed) = copy_and_items_of_wave_one(&project);
    std::fs::write(copy.join("src/a.txt"), "pronto\n").expect("the change");
    for item in listed.iter().filter(|item| **item != code(second)) {
        read_item_in(&project, &copy, item);
    }
    finish_task_of_wave_one(&project, first);
    deliver_wave_one(&project, "Parei no limite depois da primeira tarefa.", &["src/a.txt"], &[code(second)]);
    let back = round_taking_back(&project, 1);
    assert!(back.get("commit").is_some(), "the round commits the wave: {back}");
    assert!(waves_in(&back, "dispatch").is_empty(), "the task the wave left arrives now, and the followers wait for it: {back}");

    let next = project.run(&ROUND);
    assert_eq!(waves_in(&next, "dispatch"), vec![2], "{next}");
    let log = project.log();
    let codes = log.codes();
    let together: BTreeSet<String> = log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(2)).map(|e| codes[&e.id].clone()).collect();
    assert_eq!(together, [code(second), code(third), code(fourth)].into_iter().collect::<BTreeSet<_>>(), "{next}");
    let summary = log.visible().into_iter().find(|e| e.event_type == "delivered" && e.wave() == Some(1)).expect("the summary of wave 1").id;
    let wave = log.visible().into_iter().rfind(|e| e.event_type == "wave" && e.wave() == Some(2)).expect("wave 2");
    assert_eq!(wave.int("summary"), Some(summary), "the next wave carries the summary: {next}");
    let out = project.command(&["run", "read", "request-2", "--spec", SPEC], "");
    let request = String::from_utf8_lossy(&out.stdout);
    assert!(request.contains(translate("wave_prompt.summary.title", Locale::PtBr)), "the request tells of the started work: {request}");
}

/// A onda em andamento volta com tudo feito, sem tarefa por fazer e portanto
/// sem resumo: as tarefas que a seguem esperaram a volta dela e saem juntas
/// numa onda nova, que não leva base nenhuma.
#[test]
fn a_task_in_the_direction_of_a_wave_that_returns_with_everything_done_leaves_in_a_new_wave_without_a_base() {
    let (project, [first, second, third, fourth]) = running_wave_with_followers();
    let code = |id: u64| code_of(&project, id);

    // O agente faz as duas tarefas, uma depois da outra, e a entrega não deixa
    // nenhuma por fazer.
    let (copy, listed) = copy_and_items_of_wave_one(&project);
    std::fs::write(copy.join("src/a.txt"), "pronto\n").expect("the change");
    std::fs::write(copy.join("src/b.txt"), "pronto\n").expect("the change");
    for item in listed.iter().filter(|item| **item != code(second)) {
        read_item_in(&project, &copy, item);
    }
    finish_task_of_wave_one(&project, first);
    read_item_in(&project, &copy, &code(second));
    finish_task_of_wave_one(&project, second);
    deliver_wave_one(&project, "Fiz as duas tarefas.", &["src/a.txt", "src/b.txt"], &[]);
    let back = round_taking_back(&project, 1);
    assert!(back.get("commit").is_some(), "the round commits the wave: {back}");
    assert_eq!(waves_in(&back, "dispatch"), vec![2], "the followers leave in the round that takes the return, with nothing left to wait for: {back}");

    let log = project.log();
    let codes = log.codes();
    let together: BTreeSet<String> = log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(2)).map(|e| codes[&e.id].clone()).collect();
    assert_eq!(together, [code(third), code(fourth)].into_iter().collect::<BTreeSet<_>>(), "the new wave takes only the followers: {back}");
    assert!(log.live_summaries().is_empty(), "a wave that did everything leaves no summary: {:?}", log.live_summaries());
    let wave = log.visible().into_iter().rfind(|e| e.event_type == "wave" && e.wave() == Some(2)).expect("wave 2");
    assert_eq!(wave.int("summary"), None, "the new wave has no base: {:?}", wave.fields);
    let out = project.command(&["run", "read", "request-2", "--spec", SPEC], "");
    let request = String::from_utf8_lossy(&out.stdout);
    assert!(!request.contains(translate("wave_prompt.summary.title", Locale::PtBr)), "the request tells of no started work: {request}");
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
    // O texto do aviso traz o total de cópias e exatamente as ondas da lista
    // `waves`: comparar a frase inteira não depende de um dígito solto, que
    // também apareceria no total de cópias ou em outra contagem.
    let expected = translate("round.copies_full", Locale::PtBr).replace("{copies}", "2").replace("{waves}", "2, 3");
    assert_eq!(warning["hint"], json!(expected), "{held}");
}

/// Três vagas e quatro cópias. As ondas 1 e 2 voltam e esperam a revisão,
/// guardando duas cópias. Do backlog, a tarefa de um arquivo sai numa das
/// duas cópias livres, e a que cruza a árvore inteira segue pronta,
/// esperando a vez de sair sozinha, e não uma cópia: sobra cópia livre, e a
/// rodada não diz que as cópias estão cheias.
#[test]
fn a_round_that_leaves_a_copy_free_does_not_say_the_copies_are_full() {
    let (project, crit, said, _) = backlog_project(&[&["a.rs"], &["b.rs"]]);
    slots(&project, 3);
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
    deliver_file_in_copy(&project, 1, "a.rs", "um\n");
    deliver_file_in_copy(&project, 2, "b.rs", "dois\n");
    let narrow = backlog_task(&project, crit, said, &["c.rs"], &[]);
    let wide = backlog_task(&project, crit, said, &["**"], &[]);

    let out = unreviewed_round(&project);

    assert_eq!(waves_in(&out, "dispatch").len(), 1, "one of the two tasks leaves: {out}");
    let log = project.log();
    let waiting = [narrow, wide].into_iter().filter(|task| log.current(*task).is_some_and(|now| now.wave().is_none())).count();
    assert_eq!(waiting, 1, "the other one stays in the backlog: {out}");
    assert!(copies_full(&out).is_none(), "a copy is still free: {out}");
}

/// Duas vagas e três cópias, com quatro tarefas prontas. As ondas 1 e 2
/// voltam e esperam a revisão, guardando duas cópias, e sobra uma. O Jev
/// recusa só a escolha dos itens: a onda montada do backlog espera o recibo e
/// não sai, então não toma a cópia livre. A rodada diz que a onda espera a
/// escolha, e não que as cópias estão cheias; com o recibo, a onda sai na
/// cópia que sobrou.
#[test]
fn a_wave_held_for_the_item_choice_takes_no_copy_and_the_round_does_not_say_the_copies_are_full() {
    let (mut project, _, _, _) = backlog_project(&[&["a.rs"], &["b.rs"], &["c.rs"], &["d.rs"]]);
    slots(&project, 2);
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1, 2], "{first}");
    deliver_file_in_copy(&project, 1, "a.rs", "um\n");
    deliver_file_in_copy(&project, 2, "b.rs", "dois\n");
    let jev = FakeJev::start(|asked| if asks_items(asked) { (400, json!({"error": "refused"})) } else { items_answer(asked, &|_| 0.5) });
    project.jev = Some(jev.url.clone());

    let held = unreviewed_round(&project);

    assert!(waves_in(&held, "dispatch").is_empty(), "the wave waits for the item choice: {held}");
    assert_eq!(warned_waves(&held, "items-jev-pending").len(), 1, "{held}");
    assert!(copies_full(&held).is_none(), "the held wave takes no copy, and one is still free: {held}");
    assert_eq!(answer_reserves(&project, &held, "sure"), 1, "{held}");

    let sent = unreviewed_round(&project);

    assert_eq!(waves_in(&sent, "dispatch").len(), 1, "with the receipt the wave takes the free copy: {sent}");
}

/// A rodada pelo binário, rodada de dentro de `from` com a raiz do projeto
/// dada, como quem conduz a obra a roda de qualquer pasta, e com a base das
/// cópias em `copies`.
fn round_from(project: &Project, from: &Path, copies: &Path) -> Value {
    let root = project.root.display().to_string();
    let out = project
        .program(env!("CARGO_BIN_EXE_mustard-rt"))
        .args(["run", "round", "--dispatch", "--spec", SPEC, "--root", &root])
        .env("MUSTARD_COPIES_DIR", copies)
        .current_dir(from)
        .output()
        .expect("the round runs");
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("the round answers JSON ({e}): {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

/// O aviso do terminal parado numa cópia, na resposta `out`, quando saiu.
fn terminal_in_a_copy(out: &Value) -> Option<&Value> {
    out["warnings"].as_array().into_iter().flatten().find(|warning| warning["reason"] == json!("terminal-inside-a-copy"))
}

/// A base das cópias atrás de um link — a pasta pessoal num link —: a rodada
/// cria a cópia da onda pelo link, e o sistema dá a pasta do terminal parado
/// nela já resolvida. A rodada chamada de dentro da cópia avisa o terminal e
/// nomeia a pasta dele; chamada da pasta principal, não avisa.
#[test]
fn the_round_warns_the_terminal_standing_in_a_copy_behind_a_link() {
    let (project, _, _, _) = backlog_project(&[&["a.rs"]]);
    let place = project.root.parent().expect("the test folder").to_path_buf();
    let real = place.join("real");
    std::fs::create_dir_all(&real).expect("the real base");
    let link = place.join("link");
    std::os::unix::fs::symlink(&real, &link).expect("the link");

    let first = round_from(&project, &project.root, &link);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    assert_eq!(terminal_in_a_copy(&first), None, "from the main folder there is no warning: {first}");
    keep_sent(&project, &[1]);
    let copy = PathBuf::from(last_send(&project, 1).str_field("copy").expect("the copy"));
    assert!(copy.starts_with(&link), "the round made the copy through the link: {}", copy.display());

    let inside = round_from(&project, &copy, &link);
    let warning = terminal_in_a_copy(&inside).unwrap_or_else(|| panic!("the round warns the terminal in the copy: {inside}"));
    let resolved = mustard_core::io::wave_prompt::shown(&std::fs::canonicalize(&copy).expect("the copy exists"));
    assert!(warning["hint"].as_str().is_some_and(|hint| hint.contains(&resolved)), "the warning names the folder {resolved}: {warning}");
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

/// Uma spec aprovada com uma tarefa que cria [`GREETING`], com a onda 1
/// despachada e em andamento pelo processo do teste. Devolve o projeto e o
/// número da tarefa.
fn wave_in_progress() -> (Project, u64) {
    let (project, _, _, tasks) = backlog_project(&[&[GREETING]]);
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    keep_sent(&project, &[1]);
    (project, tasks[0])
}

/// Grava pelo `run write` a versão nova da tarefa `task`, como o modelo a
/// grava: os campos da versão vigente com o texto revisto, e o código da
/// tarefa (`MSTD-TASK-NNNN`) em `replaces`. Sem `wave`, a versão nova vai
/// sem número de onda; com `wave`, leva esse número. Devolve a resposta do
/// comando, aceita ou recusada.
fn revise_task(project: &Project, task: u64, wave: Option<u64>) -> Value {
    let log = project.log();
    let current = log.current(task).expect("the task").clone();
    let code = log.codes().get(&current.id).cloned().expect("the task code");
    assert!(code.starts_with("MSTD-TASK-"), "{code}");
    let mut fields: serde_json::Map<String, Value> = current
        .fields
        .iter()
        .filter(|(key, _)| !["v", "id", "code", "at", "type", "author", "search", "wave"].contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    fields.insert("text".into(), json!("Tarefa do backlog, com o texto revisto."));
    fields.insert("replaces".into(), json!(code));
    if let Some(wave) = wave {
        fields.insert("wave".into(), json!(wave));
    }
    project.answer(&["write", "task", "--spec", SPEC, "--json", &Value::Object(fields).to_string()])
}

/// A versão nova da tarefa de uma onda em andamento, gravada sem `wave`
/// enquanto o agente trabalha, não tira a tarefa da onda: ela herda a onda
/// da versão vigente, e a entrega do agente segue aceita.
#[test]
fn a_new_task_version_without_the_wave_stays_in_the_wave_in_progress_and_its_delivery_is_accepted() {
    let (project, task) = wave_in_progress();

    let revised = revise_task(&project, task, None);
    assert_eq!(revised["ok"], json!(true), "{revised}");
    assert_eq!(wave_of(&project, task), Some(1), "a versão nova ficou na onda da vigente: {revised}");
    assert_eq!(project.log().current(task).and_then(|t| t.str_field("text").map(str::to_string)).as_deref(), Some("Tarefa do backlog, com o texto revisto."));

    // O plano da onda mudou depois do pedido: a rodada a manda de novo, e
    // nenhuma onda nova leva a tarefa.
    let again = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&again, "dispatch"), vec![1], "{again}");
    keep_sent(&project, &[1]);
    assert_eq!(wave_of(&project, task), Some(1), "{again}");
    assert!(project.log().visible().into_iter().all(|e| e.event_type != "wave" || e.wave() == Some(1)), "nenhuma onda nova: {again}");

    deliver_in_copy(&project, 1, "hello\n");
    assert!(project.log().events.iter().any(|e| e.event_type == "delivered" && e.wave() == Some(1)), "a entrega do agente foi gravada");
}

/// A versão nova da tarefa de uma onda já entregue, gravada só com `files`,
/// herda os outros campos da versão anterior e sai sem onda: a onda não se
/// herda por aí, e a tarefa volta ao backlog.
#[test]
fn a_new_version_of_a_delivered_wave_task_leaves_the_wave() {
    let (project, task) = wave_in_progress();
    deliver_in_copy(&project, 1, "hello\n");
    assert_eq!(wave_of(&project, task), Some(1), "a tarefa entregue segue na onda 1");
    let log = project.log();
    let before = log.current(task).expect("the task").clone();
    let code = log.codes().get(&before.id).cloned().expect("the task code");

    let sent = json!({"replaces": code, "files": [{"path": "src/other.txt"}]});
    let revised = project.answer(&["write", "task", "--spec", SPEC, "--json", &sent.to_string()]);
    assert_eq!(revised["ok"], json!(true), "{revised}");
    assert_eq!(wave_of(&project, task), None, "a versão nova sai sem onda: {revised}");
    let now = project.log().current(task).expect("the task").clone();
    assert_eq!(now.fields.get("files"), Some(&json!([{"path": "src/other.txt"}])), "{:?}", now.fields);
    for field in ["title", "text", "agent", "depends_on", "covers"] {
        assert_eq!(now.fields.get(field), before.fields.get(field), "{field} vem da versão anterior: {:?}", now.fields);
    }
}

/// Mudar a onda de uma tarefa cujo envio está aberto é recusado com o motivo
/// da onda que nasce do backlog, sem gravar nada, e a entrega do agente
/// segue aceita.
#[test]
fn moving_a_task_of_the_wave_in_progress_to_another_wave_is_refused_with_the_reason() {
    let (project, task) = wave_in_progress();
    let before = recorded(&project);

    let moved = revise_task(&project, task, Some(2));
    assert_eq!(moved["ok"], json!(false), "{moved}");
    assert_eq!(moved["reason"], json!("wave-by-backlog"), "{moved}");
    assert_eq!(moved["hint"], json!(translate("spec_events.wave_by_backlog", Locale::PtBr)), "{moved}");
    assert_eq!(recorded(&project), before, "nada foi gravado");
    assert_eq!(wave_of(&project, task), Some(1));

    deliver_in_copy(&project, 1, "hello\n");
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

// ---------------------------------------------------------------------------
// A decisão do classificador das atividades, de ponta a ponta
// ---------------------------------------------------------------------------

/// O título da tarefa aberta que o classificador de mentira diz cobrir a
/// atividade.
const OPEN_TASK: &str = "A tarefa aberta que já cobre o trabalho.";
/// O que falta à decisão que a volta da onda 1 não cumpriu.
const UNMET: &str = "O log não gira por dia.";

/// A chave, entre as tarefas do estado `state` de um pedido da triagem, da
/// tarefa aberta [`OPEN_TASK`].
fn open_task_key(state: &Value) -> Option<String> {
    state["tasks"].as_object().and_then(|tasks| tasks.iter().find(|(_, task)| task["title"] == json!(OPEN_TASK)).map(|(key, _)| key.clone()))
}

/// A resposta de escolha à pergunta `question`: traz a chance de cada opção,
/// a de `label` com certeza.
fn sure_choice(question: &Value, label: &str) -> Value {
    let options = question["criteria"].as_object().expect("the options");
    let others = (0.05 / (options.len() - 1) as f64).max(0.0);
    let chances: serde_json::Map<_, _> = options.keys().map(|option| (option.clone(), json!(if option == label { 0.95 } else { others }))).collect();
    json!({"type": "choice", "choice": label, "confidence": 0.95, "probabilities": chances})
}

/// A resposta do classificador de mentira ao pedido `asked`: cada atividade
/// do pedido é coberta, com certeza, pela tarefa aberta [`OPEN_TASK`]; nas
/// outras perguntas, o tipo é recurso e o resto fica em dúvida.
fn classifier_answer(asked: &Value) -> (u16, Value) {
    let triage = asked["state"]["revision"] == json!("task-triage-v1");
    let open = open_task_key(&asked["state"]);
    let mut answers = serde_json::Map::new();
    for (key, question) in asked["questions"].as_object().expect("the questions") {
        let sure = |label: &str| sure_choice(question, label);
        let answer = match (triage, &open) {
            (true, _) if key.ends_with("_near") => sure("none"),
            (true, Some(task)) => sure(&format!("covered:{task}")),
            (true, None) => sure("new"),
            _ if key.starts_with("tipo_") => json!({"type":"choice","choice":"feature","confidence":0.9,"probabilities":{"feature":0.9,"defect":0.025,"text_fix":0.025,"remove_unused":0.025,"test_cleanup":0.025}}),
            _ => json!({"type": "noul", "noul": 0.5}),
        };
        answers.insert(key.clone(), answer);
    }
    (200, json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 1500, "output_tokens": 0}}))
}

/// Um Jev de mentira do classificador das atividades ([`classifier_answer`]).
fn classifying() -> FakeJev {
    FakeJev::start(classifier_answer)
}

/// Um classificador das atividades que volta ilegível: o pedido da triagem
/// recebe uma resposta sem resposta nenhuma, e as outras perguntas, a de
/// [`classifier_answer`].
fn failing_classifier() -> FakeJev {
    FakeJev::start(|asked| {
        if asked["state"]["revision"] == json!("task-triage-v1") {
            return (200, json!({"model": "jev-1.13.0", "answers": {}, "usage": {"input_tokens": 1500, "output_tokens": 0}}));
        }
        classifier_answer(asked)
    })
}

/// O recibo da reserva `request` da triagem, pela porta do `run knowledge
/// --complete-session`, como o agente de análise o daria: cada atividade
/// coberta, com certeza, pela tarefa aberta [`OPEN_TASK`] do material do
/// pedido, e nenhum assunto próximo.
fn answer_triage_reserve(project: &Project, request: &Value) {
    let prompt = request["delegation"]["request"]["prompt"].as_str().unwrap_or_else(|| panic!("the reserve prompt: {request}"));
    let material: Value = prompt.lines().find_map(|line| line.strip_prefix("Material: ")).and_then(|text| serde_json::from_str(text).ok()).expect("the material");
    let open = open_task_key(&material).unwrap_or_else(|| panic!("the open task among the options: {material}"));
    let answers: serde_json::Map<String, Value> = (1..=request["questions"].as_u64().unwrap_or_default())
        .map(|number| {
            let id = format!("q{number}");
            let text = prompt.lines().find_map(|line| line.strip_prefix(&format!("{id}: "))).unwrap_or_default();
            let choice = if text.contains("closest subject") { "none".to_string() } else { format!("covered:{open}") };
            (id, json!({"choice": choice, "certainty": "sure"}))
        })
        .collect();
    let receipt = json!({"request_id": request["request_id"], "model": "inherit", "input_tokens": null, "output_tokens": null,
        "result": {"answers": answers}});
    let path = project.home.join("recibo-triagem.json");
    std::fs::write(&path, receipt.to_string()).expect("the receipt");
    let done = project.command(&["run", "knowledge", "--complete-session", path.to_str().expect("a path")], "");
    assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stdout));
}

/// Os pedidos do classificador das atividades que o Jev de mentira recebeu.
fn triage_requests(jev: &FakeJev) -> usize {
    jev.all_requests().iter().filter(|asked| asked["state"]["revision"] == json!("task-triage-v1")).count()
}

/// O título da tarefa do backlog sem onda, que o classificador de mentira dos
/// testes de mesmo trabalho acha parecida com a atividade de mesmo título.
const BACKLOG_TASK: &str = "A tarefa do backlog que ainda não saiu em onda.";

/// O classificador de mentira do mesmo trabalho: a atividade nova é coberta,
/// com certeza, pela tarefa aberta de mesmo título, se o pedido a oferece;
/// sem tarefa de mesmo título entre as opções, é trabalho novo.
fn same_title_classifier() -> FakeJev {
    FakeJev::start(|asked| {
        let title = &asked["state"]["items"]["i0"]["title"];
        let same = asked["state"]["tasks"].as_object().and_then(|tasks| tasks.iter().find(|(_, task)| &task["title"] == title)).map(|(key, _)| key.clone());
        let answers: serde_json::Map<String, Value> = asked["questions"]
            .as_object()
            .expect("the questions")
            .iter()
            .map(|(key, question)| {
                let label = match (&same, key.ends_with("_near")) {
                    (_, true) => "none".to_string(),
                    (Some(task), false) => format!("covered:{task}"),
                    (None, false) => "new".to_string(),
                };
                (key.clone(), sure_choice(question, &label))
            })
            .collect();
        (200, json!({"model": "jev-1.13.0", "answers": answers, "usage": {"input_tokens": 1500, "output_tokens": 0}}))
    })
}

/// A tarefa nova de título `title` para a spec dos testes, que cobre o
/// critério `criterion`, na gravação do `run write` com a resposta como veio.
fn write_task_answer(project: &Project, criterion: u64, said: u64, title: &str) -> Value {
    let task = json!({"title": title, "agent": "- conferir pelo teste", "text": "Tarefa nova.", "files": [{"path": "src/outro.rs", "new": true}],
        "depends_on": [], "covers": [criterion], "origin": said});
    project.answer(&["write", "task", "--spec", SPEC, "--json", &task.to_string()])
}

/// A tarefa nova parecida com uma de onda reprovada — o conserto dela — é
/// gravada: a comparação com as tarefas abertas só olha o backlog sem onda,
/// e a de onda é assunto da espera pela onda semelhante. A parecida com a do
/// backlog sem onda segue recusada, nomeando a tarefa do backlog, e nada se
/// grava.
#[test]
fn a_task_like_one_of_a_rejected_wave_is_recorded_and_one_like_the_backlog_is_refused() {
    let (mut project, criterion, said, tasks) = backlog_project(&[&[GREETING]]);
    let wave_title = project.log().current(tasks[0]).and_then(|task| task.str_field("title").map(str::to_string)).expect("the title of the wave's task");
    let sent = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
    sent_by(&project, &[1], closed_process::closed_process());
    deliver_in_copy(&project, 1, "hello\n");
    assert_eq!(reject(&project, 1, REJECTION)["ok"], json!(true));
    let backlog = backlog_task_with(&project, criterion, said, &["src/backlog.rs"], &[], &json!({"title": BACKLOG_TASK}));
    assert_eq!(wave_of(&project, backlog), None, "the backlog task has no wave");
    let jev = same_title_classifier();
    project.jev = Some(jev.url.clone());
    let tasks_written = |project: &Project| project.log().visible().iter().filter(|e| e.event_type == "task").count();
    let before = tasks_written(&project);

    let fix = write_task_answer(&project, criterion, said, &wave_title);

    assert_eq!(fix["ok"], json!(true), "the fix of the rejected wave's task is recorded: {fix}");
    assert_eq!(tasks_written(&project), before + 1, "{fix}");
    let asked: Vec<Value> = jev.all_requests().into_iter().filter(|asked| asked["state"]["revision"] == json!("task-triage-v1")).collect();
    assert_eq!(asked.len(), 1, "the classifier is asked once: {fix}");
    let compared: Vec<&str> = asked[0]["state"]["tasks"].as_object().expect("the open tasks").values().filter_map(|task| task["title"].as_str()).collect();
    assert_eq!(compared, vec![BACKLOG_TASK], "only the backlog task without a wave is compared: {fix}");

    let repeated = write_task_answer(&project, criterion, said, BACKLOG_TASK);

    assert_eq!(repeated["reason"], json!("same-work-open"), "{repeated}");
    assert_eq!(repeated["id"], json!(backlog), "{repeated}");
    assert_eq!(tasks_written(&project), before + 1, "the refused task is not recorded: {repeated}");
}

/// As ondas que a resposta `out` da rodada deixa esperando a decisão do Jev.
fn waiting_for_the_jev(out: &Value) -> Vec<u64> {
    out["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|warning| warning["reason"] == json!("round-jev-decision-pending"))
        .filter_map(|warning| warning["wave"].as_u64())
        .collect()
}

/// A linha de uso da onda 1 que as rodadas desta seção mandam.
const USAGE_OF_WAVE_ONE: &str = r#"<USAGE>{"wave":1}</USAGE>"#;

/// A spec com a onda 1 devolvida sem cumprir a decisão que levou: falta
/// [`UNMET`].
struct WaitingUnmet {
    project: Project,
    jev: FakeJev,
    criterion: u64,
    said: u64,
    /// A tarefa aberta [`OPEN_TASK`], à espera da onda 1.
    open: u64,
    /// A decisão que a onda 1 levou e não cumpriu.
    carried: u64,
    /// A resposta da rodada que deixou a volta esperando.
    asked: Value,
}

/// A resposta da volta da onda 1 por todo o combinado da spec: cada item
/// cumprido, menos a decisão `decision`, à qual falta `missing`.
fn unmet_decision(project: &Project, decision: u64, missing: &str) -> Value {
    let log = project.log();
    let codes = log.codes();
    let agreed: Vec<Value> = mustard_core::domain::wave_prompt::all_agreed(&log)
        .iter()
        .map(|item| {
            if codes.get(&item.id) == codes.get(&decision) {
                json!({"item": item.id, "met": false, "text": missing})
            } else {
                json!({"item": item.id, "met": true})
            }
        })
        .collect();
    json!({"agreed": agreed})
}

/// A spec com a onda 1 devolvida sem cumprir a decisão que levou, a tarefa aberta
/// [`OPEN_TASK`] à espera dela, cobrindo outra decisão, e o classificador de
/// mentira ligado, com a rodada que deixa a volta esperando já feita: ele foi
/// perguntado uma vez, fora das travas.
fn unmet_waiting_for_the_classifier() -> WaitingUnmet {
    unmet_waiting_for(classifying())
}

/// [`unmet_waiting_for_the_classifier`] com o classificador `jev`.
fn unmet_waiting_for(jev: FakeJev) -> WaitingUnmet {
    let (mut project, criterion, said, tasks) = backlog_project(&[&[GREETING]]);
    let carried = project.seed("decision", &json!({"text": "A saudação muda de tom.", "why": "w", "waves": [1], "keys": ["k"], "origin": said}));
    let other = project.seed("decision", &json!({"text": "O log gira por dia.", "why": "w", "keys": ["log"], "origin": said}));
    let open = seed_task_with(&project, criterion, said, &["src/open.rs"], &json!({"title": OPEN_TASK, "depends_on": [tasks[0]], "covers": [other]}));
    let sent = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&sent, "dispatch"), vec![1], "{sent}");
    sent_by(&project, &[1], closed_process::closed_process());
    deliver_file_with(&project, 1, GREETING, "hello\n", &unmet_decision(&project, carried, UNMET));
    project.jev = Some(jev.url.clone());

    let asked = project.run(&["round", "--dispatch", "--spec", SPEC, "--report", USAGE_OF_WAVE_ONE]);

    assert_eq!(waiting_for_the_jev(&asked), vec![1], "the return waits for the decision: {asked}");
    assert_eq!(asked.get("commit"), None, "{asked}");
    assert_eq!(triage_requests(&jev), 1, "the classifier is asked once, outside the locks: {asked}");
    WaitingUnmet { project, jev, criterion, said, open, carried, asked }
}

/// O que falta à decisão, [`UNMET`], foi para a tarefa aberta `open` como
/// nota, e não virou tarefa própria.
fn assert_unmet_noted_on(project: &Project, open: u64) {
    let log = project.log();
    let code = log.codes().get(&open).cloned().expect("the open task code");
    let notes = log.visible().into_iter().filter(|e| e.event_type == "note" && e.str_field("label") == Some(code.as_str())).count();
    assert_eq!(notes, 1, "the unmet item became a note on the task the classifier chose");
    let own = log.visible().into_iter().filter(|e| e.event_type == "task" && e.str_field("text") == Some(UNMET)).count();
    assert_eq!(own, 0, "the rule that makes a task of its own did not apply");
}

/// A decisão que a volta da onda 1 não cumpriu vai ao classificador, que a
/// manda para a tarefa aberta; a rodada deixa a volta esperando, e quem
/// conduz grava, no meio-tempo, uma tarefa que nada tem a ver com ele. A
/// rodada seguinte aplica a decisão já tomada, sem perguntar de novo: a
/// tarefa nova muda as opções do pedido, e não as atividades que ele decide.
#[test]
fn a_classifier_decision_survives_a_task_written_while_the_return_waits() {
    let WaitingUnmet { project, jev, criterion, said, open, .. } = unmet_waiting_for_the_classifier();

    seed_backlog_task(&project, criterion, said, &["src/unrelated.rs"]);
    let applied = project.run(&["round", "--dispatch", "--spec", SPEC, "--report", USAGE_OF_WAVE_ONE]);

    assert!(waiting_for_the_jev(&applied).is_empty(), "the recorded decision answers although the queue changed: {applied}");
    assert!(applied.get("commit").is_some(), "{applied}");
    assert_eq!(triage_requests(&jev), 1, "no new question for the same unmet item: {applied}");
    assert_unmet_noted_on(&project, open);
}

/// O classificador volta ilegível, e a decisão que a volta da onda 1 não
/// cumpriu vai à reserva. Quem conduz grava, no meio-tempo, uma tarefa que
/// nada tem a ver com ele, e só depois manda o recibo da reserva pedida antes
/// dela. A rodada seguinte aplica o recibo, sem perguntar ao classificador de
/// novo nem pedir outra reserva: a tarefa nova muda as opções do pedido, e
/// não as atividades que ele decide.
#[test]
fn a_reserve_receipt_survives_a_task_written_between_the_request_and_the_receipt() {
    let WaitingUnmet { project, jev, criterion, said, open, asked, .. } = unmet_waiting_for(failing_classifier());
    let reserves = asked["reserves"].as_array().cloned().unwrap_or_default();
    assert_eq!(reserves.len(), 1, "the unreadable classifier hands the unmet item to the reserve: {asked}");

    seed_backlog_task(&project, criterion, said, &["src/unrelated.rs"]);
    answer_triage_reserve(&project, &reserves[0]);
    let applied = project.run(&["round", "--dispatch", "--spec", SPEC, "--report", USAGE_OF_WAVE_ONE]);

    assert!(waiting_for_the_jev(&applied).is_empty(), "the receipt answers although the queue changed: {applied}");
    assert!(applied.get("reserves").is_none(), "no new reserve for the same unmet item: {applied}");
    assert_eq!(triage_requests(&jev), 1, "the classifier is not asked again: {applied}");
    assert!(applied.get("commit").is_some(), "{applied}");
    assert_unmet_noted_on(&project, open);
}

/// O classificador volta ilegível duas rodadas seguidas e a reserva do item
/// não cumprido segue sem recibo: a segunda rodada pergunta de novo e devolve
/// o mesmo pedido de reserva a quem conduz, mas a spec guarda uma etapa só
/// dele, e essa etapa não tem o resultado de uma falha.
#[test]
fn a_pending_reserve_asked_again_is_recorded_once_and_not_as_a_failure() {
    let WaitingUnmet { project, jev, asked, .. } = unmet_waiting_for(failing_classifier());
    let first = asked["reserves"].as_array().cloned().unwrap_or_default();
    assert_eq!(first.len(), 1, "the unreadable classifier hands the unmet item to the reserve: {asked}");

    let again = project.run(&["round", "--dispatch", "--spec", SPEC, "--report", USAGE_OF_WAVE_ONE]);

    assert_eq!(waiting_for_the_jev(&again), vec![1], "the return still waits for the receipt: {again}");
    assert_eq!(triage_requests(&jev), 2, "the classifier is asked again while the reserve has no receipt: {again}");
    let handed: Vec<Value> = again["reserves"].as_array().into_iter().flatten().map(|request| request["request_id"].clone()).collect();
    assert_eq!(handed, vec![first[0]["request_id"].clone()], "the same reserve goes back to the conductor: {again}");
    let log = project.log();
    let recorded: Vec<Option<&str>> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "stage_run" && e.str_field("stage").is_some_and(|stage| stage.starts_with("jev-reserve:")))
        .map(|e| e.str_field("result"))
        .collect();
    assert_eq!(recorded, vec![Some("skipped")], "one stage for the pending reserve, and not a failing one");
}

/// A decisão é das atividades: a volta que muda o que falta à decisão
/// enquanto espera não tem decisão nenhuma, e o classificador é perguntado de
/// novo.
#[test]
fn a_classifier_decision_is_asked_again_when_the_unmet_item_changes() {
    let WaitingUnmet { project, jev, carried, .. } = unmet_waiting_for_the_classifier();

    deliver_file_with(&project, 1, GREETING, "hello\n", &unmet_decision(&project, carried, "A cache não expira."));
    let again = project.run(&["round", "--dispatch", "--spec", SPEC, "--report", USAGE_OF_WAVE_ONE]);

    assert_eq!(waiting_for_the_jev(&again), vec![1], "the changed unmet item has no decision yet: {again}");
    assert_eq!(triage_requests(&jev), 2, "the classifier is asked about the changed unmet item: {again}");
}

/// A volta da onda 1 deixa uma sobra de limpeza, que vira tarefa, e quem
/// conduz grava depois uma tarefa comum. A formação põe as duas no mesmo
/// sentido, e a rodada seguinte as despacha juntas na onda 2: a limpeza não
/// espera a tarefa comum terminar.
#[test]
fn a_cleanup_leftover_leaves_with_the_task_of_its_direction() {
    let (mut project, crit, said, _) = backlog_project(&[&[GREETING]]);
    let first = project.run(&ROUND);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    let leftovers = json!({"leftovers": [{"title": "O comentário da saudação ainda diz olá", "detail": "A saudação mudou. Prova: `git --version`",
        "files": ["src/greeting-notes.txt"], "cleanup": true, "rule": "O comentário diz o que a saudação diz", "places": ["src/greeting-notes.txt"]}]});
    deliver_file_with(&project, 1, GREETING, "hello\n", &leftovers);
    let back = round_taking_back(&project, 1);
    assert!(back.get("commit").is_some(), "the round commits the wave 1: {back}");
    let log = project.log();
    let codes = log.codes();
    let tidy = log
        .visible()
        .into_iter()
        .find(|e| e.event_type == "task" && e.fields.get("cleanup") == Some(&json!(true)))
        .and_then(|e| codes.get(&e.id).cloned())
        .unwrap_or_else(|| panic!("the leftover became a cleanup task: {back}"));
    let normal = code_of(&project, backlog_task(&project, crit, said, &["src/greeting-tone.txt"], &[]));
    project.session = Some(SESSION);

    let asked = project.run(&ROUND);
    let formation = asked.get("formation").unwrap_or_else(|| panic!("the round asks the forming of the two tasks: {asked}"));
    formed_by_the_analysis(&project, formation, json!([{"objective": "Acertar a saudação e o que se diz dela", "tasks": [normal, tidy], "merges": []}]));
    let out = project.run(&ROUND);

    assert_eq!(waves_in(&out, "dispatch"), vec![2], "{out}");
    let log = project.log();
    let codes = log.codes();
    let together: BTreeSet<String> = log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(2)).map(|e| codes[&e.id].clone()).collect();
    assert_eq!(together, [normal, tidy].into_iter().collect::<BTreeSet<_>>(), "the cleanup leaves with the task of its direction: {out}");
}

/// Faz o git do projeto ignorar `.env`, como um `.gitignore` faria.
fn ignore_the_env_file(project: &Project) {
    let exclude = project.root.join(".git/info/exclude");
    let kept = std::fs::read_to_string(&exclude).expect("the exclude file");
    std::fs::write(&exclude, format!("{kept}.env\n")).expect("the local file is ignored");
}

/// A sobra que só mexe em arquivo que o git ignora é mudança local: a volta da
/// onda 1 deixa uma nota ao usuário, sem pedido nem tarefa, e a sobra de
/// arquivo versionado da mesma volta segue virando tarefa.
#[test]
fn a_leftover_only_in_a_git_ignored_file_leaves_a_note_and_no_task() {
    let (project, _, _, _) = backlog_project(&[&[GREETING]]);
    ignore_the_env_file(&project);
    let first = project.run(&ROUND);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    let local = "A chave da API falta no ambiente local";
    let versioned = "O README não cita a saudação";
    let leftovers = json!({"leftovers": [
        {"title": local, "detail": "Falta a chave da API no .env desta máquina. Prova: `git --version`", "files": [".env"], "rule": "A chave da API mora no ambiente", "places": [".env"]},
        {"title": versioned, "detail": "O README não explica a saudação. Prova: `git --version`", "files": ["README.md"], "rule": "O README explica a saudação", "places": ["README.md"]}]});
    deliver_file_with(&project, 1, GREETING, "hello\n", &leftovers);

    let back = round_taking_back(&project, 1);

    assert!(back.get("commit").is_some(), "a volta não é recusada pela sobra: {back}");
    let log = project.log();
    let titled = |kind: &str, title: &str| log.visible().into_iter().filter(|e| e.event_type == kind && e.str_field("title") == Some(title)).count();
    assert_eq!(titled("task", local), 0, "a mudança local não vira tarefa: {back}");
    assert_eq!(titled("request", local), 0, "nem pedido: {back}");
    assert_eq!(titled("note", local), 1, "ela fica como nota: {back}");
    let note = log.visible().into_iter().find(|e| e.event_type == "note" && e.str_field("title") == Some(local)).expect("the note");
    let text = note.str_field("text").unwrap_or_default();
    assert!(text.contains("`.env`") && text.contains("mudança local"), "a nota diz o arquivo e que é local: {text}");
    assert_eq!(titled("task", versioned), 1, "a sobra de arquivo versionado segue virando tarefa: {back}");
}

/// A tarefa que só mexe em arquivo que o git ignora é recusada com motivo
/// próprio, sem gravar nada; a que também mexe em arquivo versionado passa.
#[test]
fn a_task_whose_files_are_all_git_ignored_is_refused_with_its_own_reason() {
    let (project, criterion, said, _) = backlog_project(&[&[GREETING]]);
    ignore_the_env_file(&project);
    let tasks = |project: &Project| project.log().visible().iter().filter(|e| e.event_type == "task").count();
    let before = tasks(&project);
    let task = |files: Value| {
        json!({"title": "Pôr a chave da API no ambiente", "agent": "- conferir pelo teste", "text": "Mudança no .env.", "files": files,
            "depends_on": [], "covers": [criterion], "origin": said})
    };

    let refused = project.answer(&["write", "task", "--spec", SPEC, "--json", &task(json!([{"path": ".env", "new": true}])).to_string()]);

    assert_eq!(refused["ok"], json!(false), "{refused}");
    assert_eq!(refused["reason"], json!("files-ignored-by-git"), "{refused}");
    assert!(refused["hint"].as_str().unwrap_or_default().contains("`.env`"), "a recusa diz o arquivo: {refused}");
    assert_eq!(tasks(&project), before, "nada foi gravado: {refused}");

    let mixed = project.answer(&["write", "task", "--spec", SPEC, "--json", &task(json!([{"path": ".env", "new": true}, {"path": "src/outro.rs", "new": true}])).to_string()]);

    assert_eq!(mixed["ok"], json!(true), "com um arquivo versionado a tarefa passa: {mixed}");
    assert_eq!(tasks(&project), before + 1, "{mixed}");
}

/// A obra do próprio Mustard: a rodada que comita código do programa o
/// recompila e passa o despacho ao programa novo, que responde numa segunda
/// passada, e as duas respostas saem juntas. A onda 1 volta com duas sobras de
/// limpeza e entra no commit; a onda 2 volta fora do escopo e espera a
/// revisão. A resposta traz as duas limpezas despachadas pela segunda
/// passada, sem esperar o resto da obra, e cada aviso da onda 2 uma vez.
#[test]
fn a_round_that_commits_the_program_hands_the_dispatch_over_and_answers_once() {
    let (project, crit, said, _) = backlog_in(Project::as_mustard_source(), &[&["apps/rt/a.txt"]], &[]);
    let first = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&first, "dispatch"), vec![1], "{first}");
    keep_sent(&project, &[1]);
    backlog_task(&project, crit, said, &["apps/rt/b.txt"], &[]);
    let second = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(waves_in(&second, "dispatch"), vec![2], "{second}");
    keep_sent(&project, &[2]);
    let leftovers = json!({"leftovers": [
        {"title": "Limpar o primeiro", "detail": "Sobrou um arquivo. Prova: `git --version`", "files": ["apps/rt/c.txt"], "cleanup": true, "rule": "Nenhum arquivo sobra", "places": ["apps/rt/c.txt"]},
        {"title": "Limpar o segundo", "detail": "Sobrou outro. Prova: `git --version`", "files": ["apps/rt/d.txt"], "cleanup": true, "rule": "Nenhum texto velho sobra", "places": ["apps/rt/d.txt"]}]});
    deliver_file_with(&project, 1, "apps/rt/a.txt", "um\n", &leftovers);
    let copy = deliver_file_in_copy(&project, 2, "apps/rt/b.txt", "dois\n");
    std::fs::write(copy.join("apps/rt/fora.txt"), "fora do escopo\n").expect("a file outside the scope");
    // Só a onda 1 é revista: a 2 fica esperando.
    let view = project.run(&["review-wave", "--wave", "1", "--spec", SPEC]);
    assert!(view["fingerprint"].is_string(), "{view}");
    let reviewed = json!({"wave": 1, "note": "O conferente leu o diff e as regras."});
    let report = format!("{USAGE_OF_WAVE_ONE}\n<REVIEWED>{reviewed}</REVIEWED>");

    let out = project.command(&["run", "round", "--dispatch", "--spec", SPEC, "--report", &report], "");
    let out: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));

    assert_eq!(out["ok"], json!(true), "{out}");
    assert!(out.get("commit").is_some(), "the round committed the wave 1: {out}");
    assert!(std::fs::read_to_string(project.home.join("cargo.log")).is_ok_and(|calls| calls.contains("build")), "the commit rebuilt the program: {out}");
    let log = project.log();
    let codes = log.codes();
    let cleanups: Vec<&String> = log
        .visible()
        .into_iter()
        .filter(|e| e.event_type == "task" && e.fields.get("cleanup") == Some(&json!(true)))
        .filter_map(|e| codes.get(&e.id))
        .collect();
    assert_eq!(cleanups.len(), 2, "the two leftovers became cleanup tasks: {out}");
    assert_eq!(waves_in(&out, "dispatch"), vec![3, 4], "the second pass sends the two cleanups: {out}");
    let sent: BTreeSet<&String> =
        log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() >= Some(3)).filter_map(|e| codes.get(&e.id)).collect();
    assert_eq!(sent, cleanups.into_iter().collect::<BTreeSet<_>>(), "the waves sent take the cleanups: {out}");
    assert!(out.get("held").is_none(), "no cleanup is held: {out}");
    let warnings = out["warnings"].as_array().cloned().unwrap_or_default();
    for reason in ["wave-review-required", "files-diverged", "scope-expanded"] {
        let of_wave_two = warnings.iter().filter(|warning| warning["reason"] == json!(reason) && warning["wave"] == json!(2)).count();
        assert_eq!(of_wave_two, 1, "{reason} of the wave waiting for review comes once: {out}");
    }
    let distinct: BTreeSet<String> = warnings.iter().map(Value::to_string).collect();
    assert_eq!(distinct.len(), warnings.len(), "no warning comes twice: {out}");
}
