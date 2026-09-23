// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A revisão final responde por todo o combinado vigente, mesmo pelo item
//! que nenhuma onda carregou: o pedido lista todo mundo, o veredito malformado
//! é recusado nomeando quem faltou, e o item que ela marca sem atender vira
//! tarefa no backlog — a rodada seguinte forma o lote, despacha, e o
//! fechamento só fecha depois de uma revisão final nova com tudo atendido.
//! Prova de ponta a ponta, pelo binário de verdade, num repositório
//! temporário.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::spec_events as store;
use serde_json::{json, Value};

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
        Self { _dir: dir, root, home }
    }

    fn command(&self, args: &[&str], stdin: &str) -> std::process::Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(args)
            .current_dir(&self.root)
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
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let out = self.command(&all, "");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)))
    }

    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string()])
    }

    fn hook(&self, event: &str, payload: &Value) {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
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
    project.write("context", &json!({"text": GOAL, "origin": said}));
    let grilled = project.run(&["grill", "--spec", SPEC, "--kinds", "feature"]);
    let points = grilled["points"].as_array().cloned().expect("the point list");
    assert!(!points.is_empty(), "{grilled}");
    let mut current = Value::Null;
    for point in &points {
        let mut open = point.clone();
        open["status"] = json!("open");
        open["facts"] = json!([{"text": "A soma mora no programa.", "source": "src/main.rs:2"}]);
        current = project.write("point", &open)["point"].clone();
    }
    let mut decisions = Vec::new();
    for point in &points {
        let code = current["code"].as_str().expect("the open point").to_string();
        let answer = project.write(
            "decision",
            &json!({"text": format!("Resposta ao ponto {code}."), "keys": ["levantamento"],
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
        &json!({"when": "o programa roda", "then": "a soma aparece", "proof": "git --version", "form": "ubiquitous",
            "origin": said}),
    );
    project.write(
        "task",
        &json!({"title": "Entregar a tarefa", "text": "Somar dois números no programa.", "files": [{"path": "src/main.rs"}],
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

/// A rodada dispara a onda `wave`: quando ela precisa da escolha antes do
/// envio — o item do projeto todo, aqui sempre, pede o orquestrador —, a
/// linha da escolha, sem mudança, solta a onda na rodada seguinte. Devolve a
/// resposta que traz a onda no despacho.
fn dispatch_wave(project: &Project, wave: u64) -> Value {
    let dispatched_here = |report: &Value| {
        report["dispatch"].as_array().is_some_and(|d| d.iter().any(|entry| entry["wave"] == json!(wave)))
    };
    let asked = project.run(&["round", "--spec", SPEC]);
    if dispatched_here(&asked) {
        return asked;
    }
    assert!(
        asked["analysis"].as_array().is_some_and(|a| a.iter().any(|e| e["wave"] == json!(wave))),
        "a rodada não ofereceu a escolha antes do envio da onda {wave}: {asked}"
    );
    let answer = json!({"wave": wave, "removed": [], "added": []});
    let dispatched = project.run(&["round", "--spec", SPEC, "--report", &format!("<ANALYSIS>{answer}</ANALYSIS>")]);
    assert!(dispatched_here(&dispatched), "a rodada não despachou a onda {wave}: {dispatched}");
    dispatched
}

/// O conserto da onda `wave` já despachada: muda `changes` na cópia que a
/// rodada abriu para ela e devolve a linha `DELIVERED`, que fecha a onda com
/// o commit.
fn deliver_wave(project: &Project, wave: u64, text: &str, changes: &[(&str, &str)]) -> Value {
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
    let delivered = json!({"wave": wave, "text": text, "files": files, "commit": format!("ajuste da onda {wave}")});
    project.run(&["round", "--spec", SPEC, "--report", &format!("<DELIVERED>{delivered}</DELIVERED>")])
}

/// A spec pronta para a revisão final: aberta, levantada, planejada,
/// aprovada, com a onda 1 despachada e entregue. Devolve a gravação de cada
/// decisão do projeto todo — o combinado vigente que nenhuma onda carrega.
fn ready(project: &Project) -> Vec<Value> {
    let opened = project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    let decisions = survey(project);
    plan(project);
    approve(project);
    dispatch_wave(project, 1);
    deliver_wave(project, 1, "A soma apareceu.", &[("src/main.rs", "fn main() {\n    println!(\"{}\", 3);\n}\n")]);
    decisions
}

/// A revisão final, no pedido do fechamento, responde por todo o combinado
/// vigente pelo código, inclusive o item que nenhuma onda carregou: antes
/// desta obra o pedido só levava o combinado de cada onda; ele nunca via o
/// item de fora — a prova corta a leitura para a de antes (`agreed_for`, por
/// onda) e vê o código sumir do pedido.
#[test]
fn a_revisao_final_recebe_o_acordado_inteiro() {
    let project = Project::new();
    let decisions = ready(&project);

    let asked = project.run(&["close", "--spec", SPEC]);
    assert_eq!(asked["phase"], json!("running"), "{asked}");
    assert_eq!(asked["review"]["final"], json!(true), "{asked}");
    let prompt = asked["review"]["prompt"].as_str().unwrap_or_default();
    for decision in &decisions {
        let code = decision["code"].as_str().expect("the decision code");
        assert!(prompt.contains(code), "o pedido da revisão final não traz o item {code}: {prompt}");
    }
}

/// O veredito final malformado — sem a lista `agreed`, ou com ela faltando
/// um item combinado vigente — é recusado na forma, nomeando pelo código
/// quem faltou; nada é gravado, e a obra segue aberta, pedindo a revisão
/// final de novo. Antes desta obra o binário aceitava o veredito final sem
/// checar o combinado: a prova corta essa conferência e vê os dois vereditos
/// malformados gravados como se estivessem completos.
#[test]
fn o_fechamento_recusa_veredito_final_com_item_acordado_de_fora() {
    let project = Project::new();
    let decisions = ready(&project);
    // Cada chamada do binário grava sua própria telemetria (`call`), sucesso
    // ou recusa; "nada foi gravado" é sobre o efeito do veredito — nenhum
    // `verdict` nem `task` novo —, não sobre o total bruto de eventos.
    let recorded_kinds = |project: &Project| -> usize {
        project.log().events.iter().filter(|e| e.event_type == "verdict" || e.event_type == "task").count()
    };
    let before = recorded_kinds(&project);

    // Sem a lista `agreed` nenhuma.
    let missing_list = json!({"final": true, "result": "approved", "text": "Tudo pronto."});
    let refused = project.answer(&["close", "--spec", SPEC, "--report", &format!("<VERDICT>{missing_list}</VERDICT>")]);
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
    let refused_partial = project.answer(&["close", "--spec", SPEC, "--report", &format!("<VERDICT>{partial}</VERDICT>")]);
    assert_eq!(refused_partial["ok"], json!(false), "{refused_partial}");
    assert_eq!(refused_partial["reason"], json!("agreed-items-missing"), "{refused_partial}");
    let missing_code = decisions.last().expect("at least one decision")["code"].as_str().expect("the code");
    let hint_partial = refused_partial["hint"].as_str().unwrap_or_default();
    assert!(hint_partial.contains(missing_code), "a recusa não nomeia o item de fora da lista: {hint_partial}");
    assert_eq!(recorded_kinds(&project), before, "nada foi gravado com um item de fora da lista");

    // A obra segue aberta, pedindo a revisão final de novo.
    let still_open = project.run(&["close", "--spec", SPEC]);
    assert_eq!(still_open["phase"], json!("running"), "{still_open}");
    assert_eq!(still_open["review"]["final"], json!(true), "{still_open}");
}

/// O item combinado que a revisão final marca `met:false`, com o que falta e
/// os arquivos do conserto, vira o veredito reprovado e uma tarefa nova no
/// backlog cobrindo esse item; a rodada seguinte forma o lote com ela e
/// despacha; o fechamento segue pedindo a revisão final até uma nova que
/// atenda todos, e só então fecha. Antes desta obra a tarefa do backlog nunca
/// virava onda sozinha: a prova corta o laço que liga `dispatch_backlog` à
/// rodada e vê a tarefa parada no backlog, sem onda, rodada após rodada.
#[test]
fn item_nao_atendido_vira_tarefa_no_backlog_e_a_revisao_final_roda_de_novo() {
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
    let after_verdict = project.answer(&["close", "--spec", SPEC, "--report", &format!("<VERDICT>{verdict}</VERDICT>")]);
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
    let closed = project.run(&["close", "--spec", SPEC, "--report", &format!("<VERDICT>{approved}</VERDICT>")]);
    assert_eq!(closed["phase"], json!("closed"), "{closed}");
}
