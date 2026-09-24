// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Depois de um pedido do usuário que muda o plano de uma spec já aprovada,
//! a página recebe uma cópia só, já com as tarefas que o pedido gerou: as
//! gravações do meio do caminho não preparam cópia, e só a última, com
//! `--copy`, prepara. E cada cópia troca os documentos que já estão no banco
//! com a versão que o registro da cópia anterior guardou, sem ler a versão
//! antes. Prova de ponta a ponta, pelo binário de verdade, num repositório
//! temporário.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

const SPEC: &str = "plano";
const SESSION: &str = "s-plano";
const GOAL: &str = "Somar dois números no programa.";
const SPEC_URL: &str = "https://claude.ai/code/artifact/plano";

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
    lang: Locale,
}

impl Project {
    fn new() -> Self {
        Self::in_language(Locale::PtBr)
    }

    /// O repositório de teste com o Mustard falando `lang`.
    fn in_language(lang: Locale) -> Self {
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
        let config = json!({"language": {"text": lang.as_str()}, "git": {"flow": {"*": "dev", "dev": "main"}}});
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        git(&root, &["checkout", "-q", "-b", "dev"]);
        Self { _dir: dir, root, home, lang }
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

    fn run(&self, args: &[&str]) -> Value {
        let report = self.answer(args);
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    }

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

    /// A gravação com `--copy`, a última de um pedido.
    fn write_copying(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", SPEC, "--json", &fields.to_string(), "--copy"])
    }

    fn hook(&self, event: &str, payload: &Value) {
        let out = self.command(&["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    }
}

/// A fala do usuário, pelo gancho da entrada; devolve o número dela.
fn user_says(project: &Project, text: &str) -> u64 {
    project.hook(
        "UserPromptSubmit",
        &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION,
            "cwd": project.root.to_string_lossy()}),
    );
    let events =
        std::fs::read_to_string(project.root.join(".claude/spec").join(SPEC).join("spec.ndjson")).unwrap_or_default();
    events
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|e| e["type"] == json!("message") && e["author"] == json!("user") && e["text"] == json!(text))
        .filter_map(|e| e["id"].as_u64())
        .next_back()
        .unwrap_or_else(|| panic!("the entry hook did not record the message"))
}

/// O levantamento inteiro: o objetivo, o `grill` e cada ponto gravado,
/// respondido e fechado, exatamente como uma sessão de verdade faz.
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
        open["facts"] = json!([{"text": "A soma mora no programa.", "source": "src/main.rs:1"}]);
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
/// sem onda — a onda nasce da rodada, pelo backlog; devolve o número da
/// mensagem que o origina e o do critério.
fn plan(project: &Project) -> (u64, u64) {
    let said = user_says(project, "O plano é uma tarefa só, que soma dois números.");
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a soma aparece", "proof": "git --version", "form": "ubiquitous",
            "origin": said}),
    )["id"]
        .as_u64()
        .expect("the criterion id");
    project.write(
        "task",
        &json!({"title": "Entregar a tarefa", "text": "Somar dois números no programa.", "files": [{"path": "src/main.rs"}],
            "depends_on": [], "covers": [criterion], "origin": said}),
    );
    project.run(&["plan", "--spec", SPEC]);
    (said, criterion)
}

/// O clique em "Aprovar" na pergunta da aprovação, pelo gancho da testemunha.
fn approve(project: &Project) {
    let question = translate("approval.question", project.lang);
    let yes = translate("approval.option", project.lang);
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

/// O carimbo do molde da página `page` que o programa monta em `lang`, como
/// a publicação o grava.
fn stamp_of(page: &str, lang: Locale) -> String {
    use mustard_core::platform::page_templates::{project_page_template, spec_page_template, template_stamp};
    let template = if page == "spec" { spec_page_template(lang) } else { project_page_template(lang) };
    template_stamp(&template).expect("the stamp").to_string()
}

/// O que a conversa faz com a ordem de um marco: publica cada página que
/// ainda não tem endereço e grava o endereço, com o carimbo do molde, e
/// grava a cópia feita de cada página com o `record` que a resposta trouxe.
fn follow(project: &Project, report: &Value) {
    for page in report["publish"].as_array().cloned().unwrap_or_default() {
        let name = page.as_str().unwrap_or_default();
        let url = if name == "spec" { SPEC_URL } else { "https://claude.ai/code/artifact/projeto" };
        project.write(
            "publish",
            &json!({"page": page, "milestone": "round", "ok": true, "template": true,
                "stamp": stamp_of(name, project.lang), "url": url}),
        );
    }
    for page in ["spec", "project"] {
        if !report["copy"][page].is_null() {
            project.write("copy", &report["copy"][page]["record"]);
        }
    }
}

/// Cada número de item que os lotes da cópia da spec, no disco, levam: lê
/// todo arquivo `spec-*.json` da pasta de cópia — cada escrita aponta, em
/// `file_path`, o arquivo com o corpo de verdade — e junta os `id` de
/// `items` de toda escrita da coleção `ranges`.
fn copied_items(project: &Project) -> Vec<u64> {
    let folder = project.root.join(".claude/spec").join(SPEC).join("copy");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&folder) else { return out };
    let mut batches: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("spec-") && n.ends_with(".json")))
        .collect();
    batches.sort();
    for path in batches {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(writes) = serde_json::from_str::<Vec<Value>>(&text) else { continue };
        for write in &writes {
            if write["collection"] != json!(mustard_core::platform::page_templates::RANGES) {
                continue;
            }
            let Some(file) = write["file_path"].as_str() else { continue };
            let Ok(body_text) = std::fs::read_to_string(project.root.join(file)) else { continue };
            let Ok(body) = serde_json::from_str::<Value>(&body_text) else { continue };
            for item in body["items"].as_array().into_iter().flatten() {
                if let Some(id) = item["id"].as_u64() {
                    out.push(id);
                }
            }
        }
    }
    out
}

/// Uma spec aprovada e já publicada recebe um pedido do usuário que muda o
/// plano. O pedido, o critério e a regra que ele gerou são gravados sem
/// `--copy`, e nenhuma dessas respostas traz cópia nem manda copiar: a pasta
/// da cópia fica como a rodada a deixou. A última gravação, a tarefa, vai com
/// `--copy`, e só ela prepara a cópia, uma vez, com os lotes calculados na
/// hora: eles levam o pedido e tudo o que ele gerou, a tarefa inclusive.
#[test]
fn a_ultima_gravacao_do_pedido_prepara_uma_copia_com_as_tarefas() {
    let project = Project::new();
    let opened = project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
    assert_eq!(opened["step"], json!("ask_goal"), "{opened}");
    survey(&project);
    let (said, _criterion) = plan(&project);
    approve(&project);

    // A primeira rodada depois da aprovação é o marco que publica a página e
    // a copia; a conversa segue a ordem, como faria de verdade.
    let first_round = project.run(&["round", "--spec", SPEC]);
    assert!(first_round["publish"].as_array().is_some_and(|p| p.iter().any(|p| p == "spec")), "{first_round}");
    follow(&project, &first_round);
    let before = copied_items(&project);

    // O pedido do usuário e o que ele gerou, sem `--copy`: nada de cópia.
    let asked = user_says(&project, "Incluir também a subtração.");
    let request = project.write(
        "request",
        &json!({"text": "Incluir a subtração.", "keys": ["subtração"], "effect": "new_waves", "origin": asked}),
    );
    let request_next = request["next"].as_str().unwrap_or_default();
    assert!(request_next.contains("--copy"), "the request says which write carries the copy: {request}");
    let criterion = project.write(
        "criterion",
        &json!({"when": "o programa roda", "then": "a subtração aparece", "proof": "git --version",
            "form": "ubiquitous", "origin": asked}),
    );
    let rule = project.write(
        "rule",
        &json!({"text": "A subtração usa o mesmo formato da soma.", "keys": ["subtração"],
            "example": "3 - 1 imprime 2, como 1 + 1 imprime 2.", "applies_to": {"files": ["**"]},
            "origin": asked}),
    );
    for written in [&request, &criterion, &rule] {
        assert!(written.get("copy").is_none(), "a write without --copy prepared a copy: {written}");
        let next = written["next"].as_str().unwrap_or_default();
        assert!(!next.contains("write copy"), "a write without --copy ordered a copy: {written}");
    }
    assert_eq!(copied_items(&project), before, "the batches the round left stay as they were");

    // A última gravação do pedido, com `--copy`: uma cópia só, com tudo.
    let task = project.write_copying(
        "task",
        &json!({"title": "Entregar a subtração", "text": "Subtrair dois números no programa.",
            "files": [{"path": "src/main.rs"}], "depends_on": [], "covers": [criterion["id"]], "origin": asked}),
    );
    assert!(task["copy"].is_object(), "the last write did not prepare the copy: {task}");
    let next = task["next"].as_str().unwrap_or_default();
    assert!(next.contains("write copy") && next.contains(SPEC_URL), "the last write orders the copy: {task}");
    assert!(!next.contains("write publish"), "the page already has its address: {task}");
    let after = copied_items(&project);
    for written in [&request, &criterion, &rule, &task] {
        let id = written["id"].as_u64().expect("the item id");
        assert!(!before.contains(&id), "the new item could not be in the copy before: {before:?}");
        assert!(after.contains(&id), "the copy misses {id}: {after:?}");
    }
    // A faixa tocada vai inteira: o que já estava nela segue junto.
    assert!(after.contains(&said), "the copy lost what was already in the range: {after:?}");
    follow(&project, &task);
}

/// As escritas dos lotes da página `page` (`spec` ou `project`) que a
/// resposta `report` mandou copiar, na ordem, como a ferramenta do banco as
/// recebe.
fn writes_of(project: &Project, report: &Value, page: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for batch in report["copy"][page]["batches"].as_array().cloned().unwrap_or_default() {
        let path = project.root.join(batch.as_str().unwrap_or_default());
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let writes: Vec<Value> = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        out.extend(writes);
    }
    out
}

/// O nome `coleção/doc_id` do documento de uma escrita do lote.
fn doc_name(write: &Value) -> String {
    format!("{}/{}", write["collection"].as_str().unwrap_or_default(), write["doc_id"].as_str().unwrap_or_default())
}

/// A versão que a escrita do documento `doc` leva em `if_version` nos lotes
/// da página `page`; `Null` quando ela vai sem versão.
fn if_version(project: &Project, report: &Value, page: &str, doc: &str) -> Value {
    let writes = writes_of(project, report, page);
    let write = writes.iter().find(|w| doc_name(w) == doc).unwrap_or_else(|| panic!("no write of {doc}: {writes:?}"));
    write.get("if_version").cloned().unwrap_or(Value::Null)
}

/// Grava a cópia da página `page` que a resposta `report` preparou, com o
/// `record` dela: com `version`, como a ordem manda agora, soma em
/// `versions` essa versão para cada documento que o lote escreveu, como se o
/// banco a tivesse devolvido; sem ela, como uma cópia gravada antes de o
/// registro guardar as versões.
fn record_copy(project: &Project, report: &Value, page: &str, version: Option<u64>) {
    let mut record = report["copy"][page]["record"].clone();
    if let Some(version) = version {
        let versions: Map<String, Value> = writes_of(project, report, page)
            .iter()
            .filter(|w| w["op"] == json!("set"))
            .map(|w| (doc_name(w), json!(version)))
            .collect();
        record["versions"] = Value::Object(versions);
    }
    project.write("copy", &record);
}

/// A parte fixa da frase que manda ler a versão de documentos, depois da
/// lista deles.
fn read_order(lang: Locale) -> String {
    translate("page.copy.existing", lang).split("{docs}").nth(1).unwrap_or_default().to_string()
}

/// Uma cópia gravada com `versions` poupa a leitura da seguinte, nas duas
/// páginas e nos dois idiomas. A primeira rodada publica as duas páginas e
/// copia; a da spec é gravada com a versão que o banco devolveu a cada
/// documento, e a do projeto como uma cópia de antes das versões. A página
/// do projeto ficou com o molde de outra versão do Mustard, e por isso a
/// linha da spec vai de novo a cada cópia, no mesmo endereço.
///
/// A cópia seguinte, de uma gravação com `--copy`, troca a faixa e o
/// documento calculado da spec com a versão guardada, e a ordem pede a
/// leitura só da linha do projeto, a única sem versão guardada. Gravadas as
/// duas com versões novas, a cópia depois dela troca os três documentos com a
/// versão mais nova, e a ordem não pede leitura nenhuma. Em cada ordem, a
/// gravação da cópia manda guardar essas versões.
#[test]
fn a_copia_seguinte_leva_a_versao_guardada_sem_pedir_leitura() {
    for lang in [Locale::PtBr, Locale::EnUs] {
        let project = Project::in_language(lang);
        project.run(&["open", "--kind", "feature", "--name", SPEC, "--base", "dev"]);
        survey(&project);
        let (said, _criterion) = plan(&project);
        approve(&project);
        let read = read_order(lang);
        let spec_docs = ["ranges/0", "computed/current"];
        let row = format!("specs/{SPEC}");

        // A primeira rodada publica e copia as duas páginas: nada existe
        // ainda no banco, e nenhuma escrita leva versão.
        let first = project.run(&["round", "--spec", SPEC]);
        assert_eq!(first["publish"], json!(["spec", "project"]), "{lang:?}: {first}");
        for page in ["spec", "project"] {
            let writes = writes_of(&project, &first, page);
            assert!(writes.iter().all(|w| w.get("if_version").is_none()), "{lang:?}: {page}: {writes:?}");
        }
        project.write(
            "publish",
            &json!({"page": "spec", "milestone": "round", "ok": true, "template": true,
                "stamp": stamp_of("spec", lang), "url": SPEC_URL}),
        );
        project.write(
            "publish",
            &json!({"page": "project", "milestone": "round", "ok": true, "template": true,
                "stamp": "0.0.1 0123456789abcdef", "url": "https://claude.ai/code/artifact/projeto"}),
        );
        record_copy(&project, &first, "spec", Some(1));
        record_copy(&project, &first, "project", None);

        // A cópia seguinte: a spec troca com a versão guardada, e só a linha
        // do projeto, sem versão guardada, é nomeada para ler.
        let second = project.write_copying("note", &json!({"text": "Nota nova.", "keys": ["k"], "origin": said}));
        let next = second["next"].as_str().unwrap_or_default();
        for doc in spec_docs {
            assert_eq!(if_version(&project, &second, "spec", doc), json!(1), "{lang:?}: {doc}: {second}");
        }
        assert_eq!(if_version(&project, &second, "project", &row), Value::Null, "{lang:?}: {second}");
        let named = translate("page.copy.existing", lang).replace("{docs}", &format!("`{row}`"));
        assert!(next.contains(&named), "{lang:?}: only the row without a version is read: {next}");
        assert_eq!(next.matches(read.as_str()).count(), 1, "{lang:?}: the spec page reads nothing: {next}");
        for page in ["spec", "project"] {
            let record = translate("page.copy.record", lang)
                .replace("{spec}", SPEC)
                .replace("{record}", &second["copy"][page]["record"].to_string());
            assert!(record.contains("`versions`"), "{lang:?}: {record}");
            assert!(next.contains(&record), "{lang:?}: the order records the versions: {next}");
        }
        record_copy(&project, &second, "spec", Some(2));
        record_copy(&project, &second, "project", Some(2));

        // Gravadas as duas com versões, a cópia depois dela troca tudo com a
        // versão mais nova, sem leitura nenhuma.
        let third = project.write_copying("note", &json!({"text": "Outra nota.", "keys": ["k"], "origin": said}));
        let next = third["next"].as_str().unwrap_or_default();
        for doc in spec_docs {
            assert_eq!(if_version(&project, &third, "spec", doc), json!(2), "{lang:?}: {doc}: {third}");
        }
        assert_eq!(if_version(&project, &third, "project", &row), json!(2), "{lang:?}: {third}");
        assert!(!next.contains(read.as_str()), "{lang:?}: the order reads no version: {next}");
    }
}
