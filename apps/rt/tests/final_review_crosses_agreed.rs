// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A revisão final responde por todo o combinado vigente, mesmo pelo item
//! que nenhuma onda carregou: o pedido lista todo mundo, o veredito malformado
//! é recusado nomeando quem faltou, e o item que ela marca sem atender vira
//! tarefa no backlog — a rodada seguinte forma o lote, despacha, e o
//! fechamento só fecha depois de uma revisão final nova com tudo atendido.
//! A aprovação final vale só para o que o revisor recebeu: a onda comitada
//! depois do pedido de revisão faz o fechamento pedir revisão nova, e o
//! ajuste depois da obra fechada e reaberta é revisado sozinho. A falha do
//! fechamento numa linha reabre só a onda que a escreveu, e o conserto dela
//! não toma, no painel, o lugar do commit da onda. Prova de ponta a ponta,
//! pelo binário de verdade, num repositório temporário.

#[path = "support/binary.rs"]
mod binary;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

#[path = "support/mod.rs"]
mod support;
#[path = "support/review.rs"]
mod wave_review;

const SPEC: &str = "revisao";
const SESSION: &str = "s-revisao";
const GOAL: &str = "Somar dois números no programa.";

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// O repositório de teste: `main` e `dev`, parado em `dev`, com o Mustard
/// fora do git e um arquivo de código para as tarefas mexerem.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
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
        let config = json!({"language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}}});
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    println!(\"{}\", 1 + 1);\n}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        git(&root, &["checkout", "-q", "-b", "dev"]);
        support::copies_leave_with_the_test(&root);
        Self { _dir: dir, root, home }
    }

    fn command(&self, args: &[&str], stdin: &str) -> std::process::Output {
        self.command_in(&self.root, args, stdin)
    }

    /// Como [`Self::command`], rodando de dentro de `dir`.
    fn command_in(&self, dir: &Path, args: &[&str], stdin: &str) -> std::process::Output {
        self.command_as(None, dir, args, stdin)
    }

    /// A abertura da spec pela conversa da sessão [`SESSION`], como quem
    /// conduz a roda: o gancho do terminal passa a sessão ao comando, e é a
    /// conversa dela que fica ligada à spec.
    fn open(&self) -> Value {
        let out = self.command_as(Some(SESSION), &self.root, &["run", "open", "--kind", "feature", "--name", SPEC, "--base", "dev"], "");
        let report: Value = serde_json::from_slice(&out.stdout).expect("the opening answers JSON");
        assert_eq!(report["ok"], json!(true), "{report}");
        report
    }

    /// Como [`Self::command_in`], com a sessão `session` no ambiente, como o
    /// gancho do terminal a passa ao comando numa conversa.
    fn command_as(&self, session: Option<&str>, dir: &Path, args: &[&str], stdin: &str) -> std::process::Output {
        let mut child = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
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
            .env_remove("MUSTARD_SESSION_ID")
            .env_remove("CLAUDE_SESSION_ID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .envs(session.map(|session| ("MUSTARD_SESSION_ID", session)))
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

    /// Um comando `run` que precisa responder `ok`.
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
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
    }

    /// O texto que um comando `mustard-rt run read …`, entregue por uma
    /// resposta no lugar do pedido, imprime quando o binário o roda, cru.
    fn read_command(&self, command: &str) -> String {
        let words: Vec<&str> = command.split_whitespace().collect();
        assert_eq!(words.get(..3), Some(&["mustard-rt", "run", "read"][..]), "{command}");
        let out = self.command(&words[1..], "");
        assert!(out.status.success(), "{command}: {}", String::from_utf8_lossy(&out.stdout));
        String::from_utf8(out.stdout).expect("the request is text")
    }

    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string()])
    }

    fn hook(&self, event: &str, payload: &Value) {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// O agente grava, pelo comando que o pedido ensina, o passo de término
    /// de cada tarefa da onda `wave`: sem ele, a entrega que dá a tarefa como
    /// feita é recusada.
    fn finish_tasks(&self, wave: u64) {
        let log = self.log();
        let codes = log.codes();
        for task in log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(wave)) {
            let item = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
            self.write("step", &json!({"wave": wave, "item": item, "text": "A tarefa ficou pronta."}));
        }
    }

    /// O agente lê, de dentro da cópia e pelo comando que o pedido ensina,
    /// cada item que o envio da onda `wave` manda ler (`read_items`): sem
    /// isso a entrega é recusada.
    fn read_request(&self, wave: u64) {
        self.read_send(|sent| sent.wave() == Some(wave) && sent.str_field("role") != Some("review"));
    }

    /// O revisor lê, de dentro da cópia dele e pelo comando que o pedido
    /// ensina, cada item que o envio da revisão manda ler: sem isso o
    /// veredito é recusado.
    fn read_review(&self) {
        self.read_send(|sent| sent.str_field("role") == Some("review"));
    }

    /// A leitura, de dentro da cópia do último envio que `pick` escolhe, de
    /// cada item da lista dele.
    fn read_send(&self, pick: impl Fn(&SpecEvent) -> bool) {
        let log = self.log();
        let sent = log
            .visible()
            .into_iter()
            .rfind(|e| e.event_type == "send" && pick(e))
            .unwrap_or_else(|| panic!("nenhum envio gravado para a leitura"));
        let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
        let root = self.root.display().to_string();
        let listed = sent.fields.get("read_items").and_then(Value::as_array).cloned().unwrap_or_default();
        for item in listed.iter().filter_map(Value::as_str) {
            let block = format!("item-{item}");
            let lesson = item.strip_prefix("lesson-");
            let mut args = vec!["run", "read", lesson.map_or(block.as_str(), |_| "lessons")];
            args.extend(lesson.into_iter().flat_map(|n| ["--term", n]));
            args.extend(["--root", &root, "--spec", SPEC]);
            let out = self.command_in(&copy, &args, "");
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
        }
    }

    fn log(&self) -> SpecLog {
        store::read(&store::spec_file(&self.root, SPEC).expect("spec file")).expect("readable").expect("the spec file")
    }
}

/// A fala do usuário, pelo gancho da entrada; devolve o número dela.
fn user_says(project: &Project, text: &str) -> u64 {
    project.hook(
        "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION,
            "cwd": project.root.to_string_lossy()}),
    );
    project
        .log()
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user") && e.str_field("text") == Some(text))
        .unwrap_or_else(|| panic!("the entry hook did not record the message"))
        .id
}

/// O levantamento inteiro: o objetivo, o `grill` e cada ponto gravado,
/// respondido e fechado com uma decisão do projeto todo (`applies_to`:
/// `{"files": ["**"]}`) — o item combinado que nenhuma onda carrega, porque
/// o dono dele é o projeto inteiro, não uma onda do plano. Devolve a
/// gravação de cada decisão, com o código que a revisão final precisa
/// responder.
fn survey(project: &Project) -> Vec<Value> {
    let said = user_says(project, GOAL);
    project.write("context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));
    let grilled = project.run(&["grill", "--spec", SPEC, "--kinds", "feature", "--full"]);
    let points = grilled["points"].as_array().cloned().expect("the point list");
    assert!(!points.is_empty(), "{grilled}");
    let mut current = Value::Null;
    for point in &points {
        let facts = json!([{"text": "A soma mora no programa.", "source": "src/main.rs:2"}]);
        current = project.write("point", &json!({"replaces": point["id"], "facts": facts}))["point"].clone();
    }
    let mut decisions = Vec::new();
    for point in &points {
        let code = current["code"].as_str().expect("the open point").to_string();
        let answer = project.write(
            "decision",
            &json!({"title": "Combinar o item", "agent": format!("- ponto {code}"), "text": "O usuário respondeu ao ponto.", "keys": ["levantamento"],
                "why": "o usuário respondeu", "origin": said, "applies_to": {"files": ["**"]}}),
        );
        decisions.push(answer.clone());
        let closed = project.write(
            "point",
            &json!({"block": point["block"], "gap": point["gap"], "from": "gap", "status": "closed",
                "closes": code, "result": [answer["id"]], "origin": said}),
        );
        current = closed["point"].clone();
    }
    decisions
}

/// O plano de uma tarefa só: o critério com a prova e a tarefa que o cobre e
/// muda `src/main.rs`, sem onda. A onda nasce da rodada, pelo backlog.
fn plan(project: &Project) {
    let said = user_says(project, "O plano é uma tarefa só, que soma dois números.");
    let criterion = project.write(
        "criterion",
        &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a soma aparece", "proof": "git --version", "form": "ubiquitous",
            "origin": said}),
    );
    project.write(
        "task",
        &json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Somar dois números no programa.", "files": [{"path": "src/main.rs"}],
            "depends_on": [], "covers": [criterion["id"]], "origin": said}),
    );
    project.run(&["plan", "--spec", SPEC]);
}

/// O clique em "Aprovar" na pergunta da aprovação, pelo gancho da testemunha.
fn approve(project: &Project) {
    let question = mustard_core::platform::i18n::translate("approval.question", mustard_core::platform::i18n::Locale::PtBr);
    let yes = mustard_core::platform::i18n::translate("approval.option", mustard_core::platform::i18n::Locale::PtBr);
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
}

/// A rodada dispara a onda `wave`, na mesma chamada: o item do projeto todo
/// não pede escolha de ninguém. Devolve a resposta que traz a onda no
/// despacho.
fn dispatch_wave(project: &Project, wave: u64) -> Value {
    let dispatched = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(
        dispatched["dispatch"].as_array().is_some_and(|d| d.iter().any(|entry| entry["wave"] == json!(wave))),
        "a rodada não despachou a onda {wave}: {dispatched}"
    );
    dispatched
}

/// O conserto da onda `wave` já despachada: muda `changes` na cópia que a
/// rodada abriu para ela e grava a entrega na spec, que a rodada assume e
/// fecha com o commit.
fn deliver_wave(project: &Project, wave: u64, text: &str, changes: &[(&str, &str)]) -> Value {
    record_delivery(project, wave, text, changes);
    project.run(&["round", "--dispatch", "--spec", SPEC])
}

/// A entrega da onda `wave` já despachada, sem a rodada que a assume: muda
/// `changes` na cópia que a rodada abriu para ela, lê o pedido, termina as
/// tarefas e grava a entrega na spec.
fn record_delivery(project: &Project, wave: u64, text: &str, changes: &[(&str, &str)]) {
    let log = project.log();
    let sent = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "send" && e.wave() == Some(wave))
        .unwrap_or_else(|| panic!("nenhum envio gravado para a onda {wave}"));
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    for (path, content) in changes {
        std::fs::write(copy.join(path), content).expect("the change");
    }
    let files: Vec<&str> = changes.iter().map(|(path, _)| *path).collect();
    // A entrega responde por cada item combinado que o pedido da onda levou:
    // as respostas do levantamento valem para o projeto todo, e o pedido as
    // leva todas.
    let agreed: Vec<Value> = mustard_core::domain::wave_prompt::all_agreed(&log)
        .iter()
        .map(|item| json!({"item": item.id, "met": true}))
        .collect();
    let delivered = json!({"wave": wave, "text": text, "files": files, "commit": format!("ajuste da entrega {wave}"),
        "agreed": agreed});
    project.read_request(wave);
    project.finish_tasks(wave);
    project.run(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()]);
}

/// A spec pronta para a revisão final: aberta, levantada, planejada,
/// aprovada, com a onda 1 despachada e entregue. Devolve a gravação de cada
/// decisão do projeto todo — o combinado vigente que nenhuma onda carrega.
fn ready(project: &Project) -> Vec<Value> {
    let opened = project.open();
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    let decisions = survey(project);
    plan(project);
    approve(project);
    dispatch_wave(project, 1);
    deliver_wave(project, 1, "A soma apareceu.", &[("src/main.rs", "fn main() {\n    println!(\"{}\", 3);\n}\n")]);
    decisions
}

/// A entrega cujo comentário acrescentado na cópia cita o código de um item
/// da spec é recusada, nomeando o arquivo e a linha, e nada é gravado; o
/// comentário com um código que a spec não tem passa.
#[test]
fn a_delivery_whose_added_comment_cites_a_spec_item_code_is_refused() {
    let project = Project::new();
    project.open();
    survey(&project);
    plan(&project);
    approve(&project);
    dispatch_wave(&project, 1);

    let log = project.log();
    let task = log.visible().into_iter().find(|e| e.event_type == "task").expect("the task");
    let code = log.codes().get(&task.id).cloned().expect("the task code");
    let unknown = format!("{}-9999", code.rsplit_once('-').expect("a numbered code").0);
    let sent = log.visible().into_iter().rfind(|e| e.event_type == "send" && e.wave() == Some(1)).expect("the send of wave 1");
    let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
    let agreed: Vec<Value> = mustard_core::domain::wave_prompt::all_agreed(&log).iter().map(|item| json!({"item": item.id, "met": true})).collect();
    let deliveries = |project: &Project| project.log().events.iter().filter(|e| e.event_type == "delivered").count();
    let deliver = |comment: &str| -> Value {
        let body = format!("fn main() {{\n    {comment}\n    println!(\"{{}}\", 3);\n}}\n");
        std::fs::write(copy.join("src/main.rs"), body).expect("the change");
        project.read_request(1);
        project.finish_tasks(1);
        let delivered = json!({"wave": 1, "text": "A soma apareceu.", "files": ["src/main.rs"], "commit": "ajuste da soma", "agreed": agreed});
        project.answer(&["write", "delivered", "--spec", SPEC, "--json", &delivered.to_string()])
    };

    let before = deliveries(&project);
    let refused = deliver(&format!("// {code}: soma os dois números"));
    assert_eq!(refused["reason"], json!("delivery-item-code-in-comment"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("src/main.rs:2") && hint.contains(&code), "the refusal names the file, the line and the code: {hint}");
    assert_eq!(deliveries(&project), before, "nothing was written");

    let accepted = deliver(&format!("// {unknown}: soma os dois números"));
    assert_eq!(accepted["ok"], json!(true), "a code the spec does not have passes: {accepted}");
}

/// A revisão final, no pedido do fechamento, responde por todo o combinado
/// vigente pelo código, inclusive o item que nenhuma onda carregou: antes
/// desta obra o pedido só levava o combinado de cada onda; ele nunca via o
/// item de fora — a prova corta a leitura para a de antes (`agreed_for`, por
/// onda) e vê o código sumir do pedido.
#[test]
fn final_review_receives_the_whole_agreed_set() {
    let project = Project::new();
    let decisions = ready(&project);

    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["phase"], json!("running"), "{asked}");
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    assert!(asked["review"].get("prompt").is_none(), "a resposta não leva o pedido inteiro: {asked}");
    let prompt = project.read_command(asked["review"]["read"].as_str().unwrap_or_default());
    for decision in &decisions {
        let code = decision["code"].as_str().expect("the decision code");
        assert!(prompt.contains(code), "o pedido da revisão final não traz o item {code}: {prompt}");
    }
}

/// O veredito final malformado — sem a lista `agreed`, ou com ela faltando
/// um item combinado vigente — é recusado na gravação pelo revisor, nomeando
/// pelo código quem faltou; nada é gravado, e o pedido da revisão final segue
/// aberto, esperando o veredito. Antes desta obra o binário aceitava o
/// veredito final sem checar o combinado: a prova corta essa conferência e vê
/// os dois vereditos malformados gravados como se estivessem completos.
#[test]
fn closing_refuses_a_final_verdict_with_an_agreed_item_left_out() {
    let project = Project::new();
    let decisions = ready(&project);
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    // Cada chamada do binário grava sua própria telemetria (`call`), sucesso
    // ou recusa; "nada foi gravado" é sobre o efeito do veredito — nenhum
    // `verdict` nem `task` novo —, não sobre o total bruto de eventos.
    let recorded_kinds = |project: &Project| -> usize {
        project.log().events.iter().filter(|e| e.event_type == "verdict" || e.event_type == "task").count()
    };
    let before = recorded_kinds(&project);

    // Sem a lista `agreed` nenhuma.
    let missing_list = json!({"final": true, "result": "approved", "text": "Tudo pronto."});
    let refused = project.answer(&["write", "verdict", "--spec", SPEC, "--json", &missing_list.to_string()]);
    assert_eq!(refused["ok"], json!(false), "{refused}");
    assert_eq!(refused["reason"], json!("agreed-items-missing"), "{refused}");
    let hint = refused["hint"].as_str().unwrap_or_default();
    for decision in &decisions {
        let code = decision["code"].as_str().expect("the decision code");
        assert!(hint.contains(code), "a recusa não nomeia o item {code} que faltou: {hint}");
    }
    assert_eq!(recorded_kinds(&project), before, "nada foi gravado com a lista inteira faltando");

    // Com a lista, mas faltando o último item vigente.
    let missing_one: Vec<Value> = decisions[..decisions.len() - 1]
        .iter()
        .map(|decision| json!({"item": decision["code"], "met": true}))
        .collect();
    let partial = json!({"final": true, "result": "approved", "text": "Quase tudo.", "agreed": missing_one});
    let refused_partial = project.answer(&["write", "verdict", "--spec", SPEC, "--json", &partial.to_string()]);
    assert_eq!(refused_partial["ok"], json!(false), "{refused_partial}");
    assert_eq!(refused_partial["reason"], json!("agreed-items-missing"), "{refused_partial}");
    let missing_code = decisions.last().expect("at least one decision")["code"].as_str().expect("the code");
    let hint_partial = refused_partial["hint"].as_str().unwrap_or_default();
    assert!(hint_partial.contains(missing_code), "a recusa não nomeia o item de fora da lista: {hint_partial}");
    assert_eq!(recorded_kinds(&project), before, "nada foi gravado com um item de fora da lista");

    // O pedido da revisão final segue aberto: o fechamento espera o veredito.
    let still_open = project.answer(&["close", "--spec", SPEC]);
    assert_eq!(still_open["reason"], json!("review-verdict-missing"), "{still_open}");
}

/// O item combinado que a revisão final marca `met:false`, com o que falta e
/// os arquivos do conserto, vira o veredito reprovado e uma tarefa nova no
/// backlog cobrindo esse item; a rodada seguinte forma o lote com ela e
/// despacha; o fechamento segue pedindo a revisão final até uma nova que
/// atenda todos, e só então fecha. Antes desta obra a tarefa do backlog nunca
/// virava onda sozinha: a prova corta o laço que liga `dispatch_backlog` à
/// rodada e vê a tarefa parada no backlog, sem onda, rodada após rodada.
#[test]
fn unmet_item_becomes_a_backlog_task_and_the_final_review_runs_again() {
    let project = Project::new();
    let decisions = ready(&project);
    let target = decisions.first().expect("at least one decision").clone();
    let target_id = target["id"].as_u64().expect("the decision id");

    let agreed: Vec<Value> = decisions
        .iter()
        .map(|decision| {
            if decision["id"] == target["id"] {
                json!({"item": decision["code"], "met": false, "text": "Falta ajustar a soma para três parcelas.",
                    "files": ["src/main.rs"]})
            } else {
                json!({"item": decision["code"], "met": true})
            }
        })
        .collect();
    let verdict = json!({"final": true, "result": "approved", "text": "Quase tudo certo.", "agreed": agreed});
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    project.read_review();
    project.write("verdict", &verdict);
    let after_verdict = project.answer(&["close", "--spec", SPEC]);
    // A obra não fecha: o item de fora força o veredito a reprovado e vira
    // tarefa no backlog, e o fechamento recusa enquanto o backlog tiver tarefa —
    // o veredito já ficou gravado, e o passo seguinte é a rodada.
    assert_eq!(after_verdict["reason"], json!("backlog-not-empty"), "{after_verdict}");

    let log = project.log();
    let recorded = log.visible().into_iter().rfind(|e| e.event_type == "verdict").expect("the recorded verdict");
    assert_eq!(recorded.fields.get("final"), Some(&json!(true)), "{recorded:?}");
    assert_eq!(recorded.str_field("result"), Some("rejected"), "o veredito com item de fora vira reprovado: {recorded:?}");
    let task = log
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "task" && e.wave().is_none() && e.ints("covers").contains(&target_id))
        .unwrap_or_else(|| panic!("nenhuma tarefa do backlog cobre o item não atendido"));
    assert_eq!(task.str_field("text"), Some("Falta ajustar a soma para três parcelas."), "{task:?}");
    let code = log.codes().get(&task.id).cloned().expect("a tarefa tem código");
    assert!(after_verdict["hint"].as_str().unwrap_or_default().contains(&code), "a recusa nomeia a tarefa: {after_verdict}");
    let files: Vec<String> =
        task.fields.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(|f| f["path"].as_str().map(str::to_string)).collect();
    assert_eq!(files, vec!["src/main.rs".to_string()], "{task:?}");

    // A rodada seguinte forma o lote com a tarefa do backlog e despacha.
    dispatch_wave(&project, 2);
    deliver_wave(&project, 2, "Ajustei a soma.", &[("src/main.rs", "fn main() {\n    println!(\"{}\", 1 + 1 + 1);\n}\n")]);

    // O fechamento pede a revisão final de novo, mesmo com o conserto
    // entregue: só uma revisão final nova que atenda tudo fecha.
    let asked_again = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked_again["phase"], json!("running"), "{asked_again}");
    assert_eq!(asked_again["review"]["final"], json!(true), "{asked_again}");

    let all_met: Vec<Value> = decisions.iter().map(|decision| json!({"item": decision["code"], "met": true})).collect();
    let approved = json!({"final": true, "result": "approved", "text": "Tudo atendido.", "agreed": all_met});
    project.read_review();
    project.write("verdict", &approved);
    let closed = project.run(&["close", "--spec", SPEC]);
    assert_eq!(closed["phase"], json!("closed"), "{closed}");
}

/// O veredito final aprovado, com todo o combinado atendido.
fn all_met_verdict(decisions: &[Value]) -> Value {
    let all_met: Vec<Value> = decisions.iter().map(|decision| json!({"item": decision["code"], "met": true})).collect();
    json!({"final": true, "result": "approved", "text": "Tudo atendido.", "agreed": all_met})
}

/// O veredito final aprovado que traz achados em `leftovers`, no formato da
/// entrega de onda, não leva a obra ao pull request: o fechamento grava cada
/// achado como uma tarefa no backlog, que cobre o pedido dele, e manda rodar
/// a rodada em vez de abrir o pull request. O fechamento seguinte recusa pelo
/// backlog e não grava os achados de novo.
#[test]
fn an_approved_final_verdict_with_leftovers_becomes_backlog_tasks_instead_of_the_pull_request() {
    let project = Project::new();
    let decisions = ready(&project);
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");

    let mut verdict = all_met_verdict(&decisions);
    verdict["leftovers"] = json!([
        {"title": "A soma não trata estouro", "detail": "A soma de números grandes estoura em src/main.rs.", "files": ["src/main.rs"]},
        {"title": "O README não cita a soma", "detail": "O README não explica a soma.", "files": ["README.md"], "cleanup": true},
    ]);
    project.read_review();
    project.write("verdict", &verdict);
    let after = project.run(&["close", "--spec", SPEC]);

    assert_eq!(after["phase"], json!("running"), "a obra não fecha com achados: {after}");
    let round = format!("mustard-rt run round --dispatch --spec {SPEC}");
    assert_eq!(after["command"], json!(round), "o fechamento manda rodar a rodada: {after}");
    assert!(!after.to_string().contains("pr-open"), "o fechamento não manda abrir o pull request: {after}");

    let log = project.log();
    let backlog = |log: &SpecLog| -> Vec<SpecEvent> {
        log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave().is_none()).cloned().collect()
    };
    let tasks = backlog(&log);
    let titles: Vec<&str> = tasks.iter().filter_map(|task| task.str_field("title")).collect();
    assert_eq!(titles, vec!["A soma não trata estouro", "O README não cita a soma"], "{tasks:?}");
    for task in &tasks {
        let title = task.str_field("title").unwrap_or_default();
        let request = log
            .visible()
            .into_iter()
            .find(|e| e.event_type == "request" && task.ints("covers").contains(&e.id))
            .unwrap_or_else(|| panic!("a tarefa {title} não cobre o pedido do achado: {task:?}"));
        assert_eq!(request.str_field("title"), Some(title), "{request:?}");
    }
    let files = |task: &SpecEvent| -> Vec<String> {
        task.fields.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(|f| f["path"].as_str().map(str::to_string)).collect()
    };
    assert_eq!(files(&tasks[0]), vec!["src/main.rs".to_string()], "{:?}", tasks[0]);
    assert_eq!(files(&tasks[1]), vec!["README.md".to_string()], "{:?}", tasks[1]);
    assert_eq!(tasks[1].fields.get("cleanup"), Some(&json!(true)), "a limpeza segue marcada: {:?}", tasks[1]);

    let again = project.answer(&["close", "--spec", SPEC]);
    assert_eq!(again["reason"], json!("backlog-not-empty"), "{again}");
    assert_eq!(backlog(&project.log()).len(), 2, "o fechamento seguinte não grava os achados de novo");
}

/// Os números dos pedidos de revisão final gravados, do mais velho ao mais
/// novo.
fn review_requests(project: &Project) -> Vec<u64> {
    project.log().visible().into_iter().filter(|e| e.event_type == "send" && e.str_field("role") == Some("review")).map(|e| e.id).collect()
}

/// A onda 2 comitada com a revisão final aberta: o usuário pede mais uma
/// tarefa depois do pedido de revisão, e a rodada a despacha e comita.
/// Devolve o número do commit dela na spec e a resposta da rodada que a
/// comitou.
fn commit_a_wave_past_the_request(project: &Project) -> (u64, Value) {
    let said = user_says(project, "Imprima também a soma de três números.");
    let criterion = project.log().visible().into_iter().find(|e| e.event_type == "criterion").expect("the criterion").id;
    project.write(
        "task",
        &json!({"agent": "- conferir pelo teste", "title": "Somar três números", "text": "Imprimir a soma de três números.",
            "files": [{"path": "src/main.rs"}], "depends_on": [], "covers": [criterion], "origin": said}),
    );
    dispatch_wave(project, 2);
    let round = deliver_wave(project, 2, "A soma de três apareceu.", &[("src/main.rs", "fn main() {\n    println!(\"{}\", 1 + 1 + 1);\n}\n")]);
    (commit_of(project, 2), round)
}

/// O veredito final aprovado que chega depois de uma onda comitada entre o
/// pedido de revisão e ele não fecha a obra: o revisor não recebeu o que a
/// onda mudou, e o fechamento seguinte pede uma revisão nova, gravada depois
/// do commit. Só o veredito que responde a esse pedido novo fecha.
#[test]
fn a_final_approval_answering_a_request_older_than_a_committed_wave_asks_for_a_new_review() {
    let project = Project::new();
    let decisions = ready(&project);
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    project.read_review();
    let (committed, _) = commit_a_wave_past_the_request(&project);

    project.write("verdict", &all_met_verdict(&decisions));
    let again = project.run(&["close", "--spec", SPEC]);
    assert_eq!(again["phase"], json!("running"), "o veredito de antes do commit não fecha a obra: {again}");
    assert_eq!(again["review"]["final"], json!(true), "o fechamento pede revisão nova: {again}");
    let requests = review_requests(&project);
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert!(requests[1] > committed, "o pedido novo vem depois do commit da onda 2: {requests:?}, commit {committed}");

    project.read_review();
    project.write("verdict", &all_met_verdict(&decisions));
    let closed = project.run(&["close", "--spec", SPEC]);
    assert_eq!(closed["phase"], json!("closed"), "o veredito do pedido novo fecha: {closed}");
}

/// O pedido de revisão final aberto, mais velho que uma onda comitada depois
/// dele, não deixa o fechamento esperando um veredito que não aprovaria a
/// obra: o fechamento abre um pedido novo, depois do commit, em vez de
/// recusar dizendo que falta o veredito.
#[test]
fn closing_with_an_open_request_older_than_a_committed_wave_opens_a_new_request() {
    let project = Project::new();
    ready(&project);
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let (committed, _) = commit_a_wave_past_the_request(&project);

    let again = project.answer(&["close", "--spec", SPEC]);
    assert_eq!(again["ok"], json!(true), "o fechamento não espera o veredito do pedido velho: {again}");
    assert_eq!(again["review"]["final"], json!(true), "{again}");
    let requests = review_requests(&project);
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert!(requests[1] > committed, "o pedido novo vem depois do commit da onda 2: {requests:?}, commit {committed}");
}

/// O número do commit gravado para a onda `wave`.
fn commit_of(project: &Project, wave: u64) -> u64 {
    project
        .log()
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "commit" && e.fields.get("waves").and_then(Value::as_array).is_some_and(|waves| waves.contains(&json!(wave))))
        .unwrap_or_else(|| panic!("a onda {wave} não comitou"))
        .id
}

/// A obra fechada com a aprovação final e reaberta para um ajuste pequeno —
/// uma decisão nova e a tarefa dela, numa onda comitada — pede uma revisão
/// final só do que mudou desde a aprovação: o pedido novo cita o commit novo e
/// a decisão nova, e não cita o commit nem as decisões que a aprovação já
/// conferiu. O veredito que responde só pela decisão nova fecha a obra de
/// novo.
#[test]
fn a_small_change_after_a_final_approval_is_reviewed_alone() {
    let project = Project::new();
    let decisions = ready(&project);
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    project.read_review();
    project.write("verdict", &all_met_verdict(&decisions));
    let closed = project.run(&["close", "--spec", SPEC]);
    assert_eq!(closed["phase"], json!("closed"), "{closed}");
    let old_commit = commit_of(&project, 1);

    let reopened = project.run(&["reopen", "--spec", SPEC, "--reason", "Imprimir também a soma de três números."]);
    assert_eq!(reopened["phase"], json!("running"), "{reopened}");
    let said = user_says(&project, "Imprima também a soma de três números.");
    let decision = project.write(
        "decision",
        &json!({"title": "Somar três números", "agent": "- conferir pelo teste", "text": "A soma de três números aparece junto.",
            "keys": ["ajuste"], "why": "o usuário pediu", "origin": said, "applies_to": {"files": ["**"]}}),
    );
    let new_decision = decision["code"].as_str().expect("the new decision code").to_string();
    let criterion = project.log().visible().into_iter().find(|e| e.event_type == "criterion").expect("the criterion").id;
    project.write(
        "task",
        &json!({"agent": "- conferir pelo teste", "title": "Somar três números", "text": "Imprimir a soma de três números.",
            "files": [{"path": "src/main.rs"}], "depends_on": [], "covers": [criterion], "origin": said}),
    );
    dispatch_wave(&project, 2);
    deliver_wave(&project, 2, "A soma de três apareceu.", &[("src/main.rs", "fn main() {\n    println!(\"{}\", 1 + 1 + 1);\n}\n")]);
    let new_commit = commit_of(&project, 2);

    let again = project.run(&["close", "--spec", SPEC]);
    assert_eq!(again["review"]["final"], json!(true), "{again}");
    let prompt = project.read_command(again["review"]["read"].as_str().unwrap_or_default());
    let codes = project.log().codes();
    assert!(prompt.contains(&codes[&new_commit]), "o pedido cita o commit novo: {prompt}");
    assert!(prompt.contains(&new_decision), "o pedido cita a decisão nova: {prompt}");
    assert!(!prompt.contains(&codes[&old_commit]), "o commit que a aprovação conferiu fica fora: {prompt}");
    for old in &decisions {
        let code = old["code"].as_str().expect("the decision code");
        assert!(!prompt.contains(code), "a decisão {code}, que a aprovação conferiu, fica fora: {prompt}");
    }

    project.read_review();
    let only_new = json!({"final": true, "result": "approved", "text": "O ajuste ficou certo.", "agreed": [{"item": new_decision, "met": true}]});
    project.write("verdict", &only_new);
    let closed_again = project.run(&["close", "--spec", SPEC]);
    assert_eq!(closed_again["phase"], json!("closed"), "o veredito do que mudou fecha a obra: {closed_again}");
}

/// O código git do commit gravado para a onda `wave`.
fn sha_of(project: &Project, wave: u64) -> String {
    let log = project.log();
    log.get(commit_of(project, wave)).and_then(|e| e.str_field("sha")).expect("the commit sha").to_string()
}

/// O `src/main.rs` com a soma de um e dois na terceira linha, depois da
/// segunda, que a onda 1 escreveu: a linha que o lint do fechamento acusa.
const SUM_OF_ONE_AND_TWO: &str = "fn main() {\n    println!(\"{}\", 3);\n    println!(\"{}\", 1 + 2);\n}\n";

/// O lint do fechamento caindo, com arquivo e linha, na linha da soma de um e
/// dois em `src/main.rs`.
fn lint_the_sum_of_one_and_two(project: &Project) {
    let config = json!({"language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}},
        "lintCommand": "git grep -n \"1 + 2\" -- src/main.rs && exit 1"});
    std::fs::write(project.root.join("mustard.json"), config.to_string()).expect("config");
}

/// A tarefa `title` do backlog, que muda `file`, pedida na fala `said` e
/// cobrindo o critério do plano.
fn backlog_task(project: &Project, said: u64, title: &str, file: &str) {
    let criterion = project.log().visible().into_iter().find(|e| e.event_type == "criterion").expect("the criterion").id;
    project.write(
        "task",
        &json!({"agent": "- conferir pelo teste", "title": title, "text": format!("{title}."),
            "files": [{"path": file}], "depends_on": [], "covers": [criterion], "origin": said}),
    );
}

/// As ondas que a falha do fechamento reabriu, na ordem.
fn reopened(project: &Project) -> Vec<u64> {
    project.log().visible().into_iter().filter(|e| e.event_type == "validation_failure").filter_map(SpecEvent::wave).collect()
}

/// A obra com as ondas 1 e 2 no mesmo arquivo — a 1 escreve a segunda linha,
/// a 2 acrescenta a terceira — e o lint do fechamento caindo, com arquivo e
/// linha, só na linha que a onda 2 escreveu. Devolve a resposta do
/// fechamento.
fn close_with_lint_on_a_line_of_wave_2(project: &Project) -> Value {
    lint_the_sum_of_one_and_two(project);
    ready(project);
    let said = user_says(project, "Imprima também a soma de um e dois.");
    backlog_task(project, said, "Somar um e dois", "src/main.rs");
    dispatch_wave(project, 2);
    deliver_wave(project, 2, "A soma de um e dois apareceu.", &[("src/main.rs", SUM_OF_ONE_AND_TWO)]);
    project.answer(&["close", "--spec", SPEC])
}

/// O lint do fechamento que cai numa linha de um arquivo das ondas 1 e 2,
/// escrita só pela onda 2, reabre só a onda 2: o `git blame` da linha acha o
/// commit dela entre os commits da obra.
#[test]
fn a_closing_lint_failure_reopens_only_the_wave_that_wrote_the_line() {
    let project = Project::new();
    let refused = close_with_lint_on_a_line_of_wave_2(&project);
    assert_eq!(refused["ok"], json!(false), "{refused}");
    assert_eq!(reopened(&project), [2], "só a onda que escreveu a linha reabre: {refused}");
}

/// Depois do conserto da onda 2, comitado, a tela das ondas mostra para ela o
/// commit dela, e não o do conserto.
#[test]
fn the_repair_commit_does_not_take_the_place_of_the_wave_commit_in_the_panel() {
    let project = Project::new();
    close_with_lint_on_a_line_of_wave_2(&project);
    let own = sha_of(&project, 2);
    dispatch_wave(&project, 2);
    let fixed = "fn main() {\n    println!(\"{}\", 3);\n    println!(\"{}\", 2 + 1);\n}\n";
    deliver_wave(&project, 2, "A soma não repete a linha que o lint acusou.", &[("src/main.rs", fixed)]);
    let repair = sha_of(&project, 2);
    assert_ne!(repair, own, "o conserto ganhou commit próprio");

    let screen = project.answer(&["panel", "--spec", SPEC, "--section", "waves"]);
    let rows = screen["rows"].as_array().cloned().unwrap_or_default();
    let row = rows.iter().find(|row| row["wave"] == json!(2)).unwrap_or_else(|| panic!("a onda 2 fora da tela: {screen}"));
    assert_eq!(row["commit"], json!(own), "a onda mostra o commit dela, não o do conserto: {screen}");
}

/// O texto do próximo passo da spec que o painel mostra.
fn panel_step(project: &Project) -> String {
    let overview = project.answer(&["panel", "--spec", SPEC, "--section", "overview"]);
    overview["spec"]["next"]["text"].as_str().unwrap_or_else(|| panic!("o painel sem o próximo passo: {overview}")).to_string()
}

/// A rodada que comita uma onda depois do pedido de revisão final aberto
/// manda fechar, com o comando do fechamento, e o painel diz o mesmo passo:
/// o veredito do pedido velho já não aprovaria a obra, e é o fechamento que
/// abre o pedido novo. Com o pedido novo aberto, mais novo que a última
/// mudança, a rodada e o painel voltam a mandar esperar o veredito.
#[test]
fn the_round_past_an_open_request_older_than_a_committed_wave_says_to_close() {
    let project = Project::new();
    ready(&project);
    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let (_, round) = commit_a_wave_past_the_request(&project);

    let close = format!("mustard-rt run close --spec {SPEC}");
    assert_eq!(round["command"], json!(close), "a rodada manda fechar: {round}");
    let next = round["next"].as_str().unwrap_or_default();
    assert!(next.ends_with(&translate("round.close", Locale::PtBr).replace("{command}", &close)), "{round}");
    assert!(!next.contains(translate("round.review_open", Locale::PtBr)), "a rodada não espera o veredito do pedido velho: {round}");
    assert_eq!(panel_step(&project), translate("resume.step.close", Locale::PtBr));

    let again = project.run(&["close", "--spec", SPEC]);
    assert_eq!(again["review"]["final"], json!(true), "o fechamento abre o pedido novo: {again}");
    let waiting = project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert!(waiting.get("command").is_none(), "sem o comando de fechar: {waiting}");
    let next = waiting["next"].as_str().unwrap_or_default();
    assert!(next.ends_with(translate("round.review_open", Locale::PtBr)), "a rodada espera o veredito do pedido novo: {waiting}");
    assert_eq!(panel_step(&project), translate("resume.step.review_open", Locale::PtBr));
}

/// A onda da tarefa `title`, depois de a rodada formar o lote dela.
fn wave_of_task(project: &Project, title: &str) -> u64 {
    project
        .log()
        .visible()
        .into_iter()
        .rfind(|e| e.event_type == "task" && e.str_field("title") == Some(title))
        .and_then(SpecEvent::wave)
        .unwrap_or_else(|| panic!("a tarefa {title} sem onda"))
}

/// O lint do fechamento que cai numa linha escrita só por uma onda, num commit
/// em que outra onda entrou sem mudança nenhuma — as duas voltaram na mesma
/// rodada, e o commit da segunda não teria o que levar —, reabre só a que
/// escreveu a linha: a que veio sem mudança divide o commit, mas não escreveu
/// linha nenhuma dele.
#[test]
fn a_closing_lint_failure_spares_a_wave_that_joined_the_commit_without_change() {
    let project = Project::new();
    // O arquivo que a segunda onda só confere já está na base, em `main` e em
    // `dev`, antes da obra.
    let readme = "# Soma\n";
    git(&project.root, &["checkout", "-q", "main"]);
    std::fs::write(project.root.join("README.md"), readme).expect("readme");
    git(&project.root, &["add", "README.md"]);
    git(&project.root, &["commit", "-q", "-m", "readme"]);
    git(&project.root, &["checkout", "-q", "dev"]);
    git(&project.root, &["merge", "-q", "--ff-only", "main"]);
    lint_the_sum_of_one_and_two(&project);
    ready(&project);
    let said = user_says(&project, "Imprima também a soma de um e dois, e confira o README.");
    backlog_task(&project, said, "Somar um e dois", "src/main.rs");
    backlog_task(&project, said, "Conferir o README", "README.md");
    let dispatched = project.run(&["round", "--dispatch", "--spec", SPEC]);
    let sum = wave_of_task(&project, "Somar um e dois");
    let check = wave_of_task(&project, "Conferir o README");
    let out: Vec<u64> = dispatched["dispatch"].as_array().into_iter().flatten().filter_map(|entry| entry["wave"].as_u64()).collect();
    assert_eq!(out, [sum, check], "as duas ondas saem juntas: {dispatched}");

    record_delivery(&project, sum, "A soma de um e dois apareceu.", &[("src/main.rs", SUM_OF_ONE_AND_TWO)]);
    record_delivery(&project, check, "O README já dizia o que precisa.", &[("README.md", readme)]);
    project.run(&["round", "--dispatch", "--spec", SPEC]);
    assert_eq!(sha_of(&project, check), sha_of(&project, sum), "a onda sem mudança entra no commit da que mudou");

    let refused = project.answer(&["close", "--spec", SPEC]);
    assert_eq!(refused["ok"], json!(false), "{refused}");
    assert_eq!(reopened(&project), [sum], "só a onda que escreveu a linha reabre: {refused}");
}
