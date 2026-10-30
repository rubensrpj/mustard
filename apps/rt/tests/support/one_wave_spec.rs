//! Um projeto de teste que leva uma spec de uma onda pelo fluxo de verdade:
//! aberta, levantada, planejada e aprovada pelo clique, com a onda 1
//! despachada, entregue e assumida pela rodada. Tudo pelo binário compilado,
//! com os comandos e os ganchos do uso real: nada do estado da spec é escrito
//! à mão.
//!
//! Um arquivo por pacote, como `binary.rs`: os testes da pasta `tests/` o
//! trazem pelo caminho.

#[path = "binary.rs"]
mod binary;
#[path = "mod.rs"]
mod copies;
#[path = "review.rs"]
mod wave_review;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use mustard_core::domain::spec_events::{SpecEvent, SpecLog};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Value, json};

const SESSION: &str = "s-uma-onda";
const GOAL: &str = "Somar dois números no programa.";

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(root).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// O repositório de teste: `main` e `dev`, parado em `dev`, com o Mustard e
/// os arquivos `ignored` fora do git, um arquivo de código para a tarefa
/// mudar e uma pasta pessoal falsa.
pub struct Project {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    home: PathBuf,
    spec: &'static str,
}

impl Project {
    /// O projeto da spec `spec`, com as chaves de `commands` (o `lintCommand`,
    /// o `testCommand`) somadas ao `mustard.json`.
    pub fn new(spec: &'static str, commands: &Value, ignored: &[&str]) -> Self {
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
        let excluded: String = [".claude/", "mustard.json", "target/"].iter().chain(ignored).fold(String::new(), |text, line| text + line + "\n");
        std::fs::write(root.join(".git/info/exclude"), excluded).expect("exclude");
        let mut config = json!({"language": {"text": "pt-BR"}, "git": {"flow": {"*": "dev", "dev": "main"}}});
        for (key, value) in commands.as_object().into_iter().flatten() {
            config[key] = value.clone();
        }
        std::fs::write(root.join("mustard.json"), config.to_string()).expect("config");
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(root.join("src/main.rs"), "fn main() {\n    println!(\"{}\", 1 + 1);\n}\n").expect("code");
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "init"]);
        git(&root, &["checkout", "-q", "-b", "dev"]);
        copies::copies_leave_with_the_test(&root);
        Self { _dir: dir, root, home, spec }
    }

    /// O binário com `args`, de dentro de `dir`, com a pasta pessoal falsa e
    /// nenhuma sessão nem spec forçada. O `nproc` também lê `OMP_NUM_THREADS`
    /// e `OMP_THREAD_LIMIT`; sem elas, o número que ele diz é o da afinidade.
    fn command_in(&self, dir: &Path, args: &[&str], stdin: &str) -> Output {
        self.command_with(None, dir, args, stdin, &[])
    }

    /// [`Project::command_in`] com as variáveis `env` somadas ao ambiente e,
    /// quando há, na conversa da sessão `session`: o gancho do terminal a passa
    /// ao comando no ambiente.
    fn command_with(&self, session: Option<&str>, dir: &Path, args: &[&str], stdin: &str, env: &[(&str, &Path)]) -> Output {
        let mut child = binary::isolated(env!("CARGO_BIN_EXE_mustard-rt"))
            .args(args)
            .current_dir(dir)
            .envs(env.iter().copied())
            .envs(session.map(|session| ("MUSTARD_SESSION_ID", session)))
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
            .env_remove("OMP_NUM_THREADS")
            .env_remove("OMP_THREAD_LIMIT")
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

    /// Um comando `run`, que precisa responder `ok`. A rodada e o fechamento
    /// levam a conferência de cada volta, como o condutor a faz.
    pub fn run(&self, args: &[&str]) -> Value {
        let mut owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        if matches!(args.first(), Some(&"round" | &"close")) {
            let report = wave_review::review_report(&self.root, self.spec, "", |wave| {
                self.run(&["review-wave", "--wave", &wave.to_string(), "--spec", self.spec])
            });
            if !report.is_empty() {
                owned.extend(["--report".into(), report]);
            }
        }
        let mut all = vec!["run"];
        all.extend(owned.iter().map(String::as_str));
        // A abertura sai da conversa da sessão e a liga à spec: só a fala
        // dessa sessão entra pelo gancho.
        let session = (args.first() == Some(&"open")).then_some(SESSION);
        let out = self.command_with(session, &self.root, &all, "", &[]);
        Self::ok_report(args, &out)
    }

    /// A recusa de um comando `run` que não passa: a resposta em JSON, com
    /// `ok` falso, e o código de saída que o fechamento usa para recusar. Só
    /// os testes do fechamento na cópia limpa, que rodam no Unix, a pedem.
    #[cfg(unix)]
    pub fn refused(&self, args: &[&str]) -> Value {
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let out = self.command_with(None, &self.root, &all, "", &[]);
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let report: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}"));
        assert_eq!(report["ok"], json!(false), "{args:?}: {report}");
        assert_eq!(out.status.code(), Some(1), "{args:?}: {report}");
        report
    }

    /// A abertura da spec pela conversa da sessão [`SESSION`], como quem
    /// conduz a roda: o gancho do terminal passa a sessão ao comando, e é a
    /// conversa dela que fica ligada à spec, a mesma que fala depois pelo
    /// gancho da entrada.
    fn open(&self) -> Value {
        let args = ["run", "open", "--kind", "feature", "--name", self.spec, "--base", "dev"];
        Self::ok_report(&args, &self.command_with(Some(SESSION), &self.root, &args, "", &[]))
    }

    /// A resposta de `args`, que precisa ser JSON com `ok`.
    fn ok_report(args: &[&str], out: &Output) -> Value {
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let report: Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}{}", String::from_utf8_lossy(&out.stderr)));
        assert_eq!(report["ok"], json!(true), "{args:?}: {report}");
        report
    }

    /// Um comando `run` cuja resposta não leva `ok`, como a limpeza, com a
    /// pasta temporária do teste no lugar da da máquina: a limpeza varre a
    /// pasta temporária, e a da máquina nunca entra no alcance do teste. A
    /// resposta, que precisa ser JSON.
    #[cfg(unix)]
    pub fn answer(&self, args: &[&str]) -> Value {
        let temp = self.temp();
        let env = [("TMPDIR", temp.as_path()), ("TMP", temp.as_path()), ("TEMP", temp.as_path())];
        let mut all = vec!["run"];
        all.extend_from_slice(args);
        let out = self.command_with(None, &self.root, &all, "", &env);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{args:?} did not answer JSON ({e}): {text}"))
    }

    /// A abertura de uma sessão nova `session` do Claude Code no projeto, pelo
    /// gancho, com o JSON do `SessionStart` e a pasta temporária do teste.
    /// Devolve o texto que o gancho pôs na sessão.
    #[cfg(unix)]
    pub fn session_start(&self, session: &str) -> String {
        self.session_start_from(session, "startup")
    }

    /// O início da sessão `session` vindo de `source` (`startup`, `clear`,
    /// `compact`, `resume`), como [`Project::session_start`].
    #[cfg(unix)]
    pub fn session_start_from(&self, session: &str, source: &str) -> String {
        let temp = self.temp();
        let env = [("TMPDIR", temp.as_path()), ("TMP", temp.as_path()), ("TEMP", temp.as_path())];
        let payload =
            json!({"hook_event_name": "SessionStart", "session_id": session, "source": source, "cwd": self.root.to_string_lossy()});
        let out = self.command_with(None, &self.root, &["on", "SessionStart"], &payload.to_string(), &env);
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
        serde_json::from_slice::<Value>(&out.stdout)
            .ok()
            .and_then(|said| said.pointer("/hookSpecificOutput/additionalContext").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default()
    }

    /// A pasta temporária do teste, criada na primeira vez.
    #[cfg(unix)]
    fn temp(&self) -> PathBuf {
        let temp = self.home.with_file_name("tmp");
        std::fs::create_dir_all(&temp).expect("the test temp folder");
        temp
    }

    fn write(&self, event_type: &str, fields: &Value) -> Value {
        self.run(&["write", event_type, "--spec", self.spec, "--json", &fields.to_string()])
    }

    /// Um evento do harness entregue ao gancho, como a sessão entrega.
    fn hook(&self, event: &str, payload: &Value) {
        let out = self.command_in(&self.root, &["on", event], &payload.to_string());
        assert_eq!(out.status.code(), Some(0), "a hook always exits 0: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A leitura, de dentro da cópia do último envio que `pick` escolhe, de
    /// cada item da lista dele, pelo comando que o pedido ensina. Devolve a
    /// cópia.
    fn read_send(&self, pick: impl Fn(&SpecEvent) -> bool) -> PathBuf {
        let log = self.log();
        let sent = log.visible().into_iter().rfind(|e| e.event_type == "send" && pick(e)).expect("the send");
        let copy = PathBuf::from(sent.str_field("copy").expect("the copy"));
        let root = self.root.display().to_string();
        let listed = sent.fields.get("read_items").and_then(Value::as_array).cloned().unwrap_or_default();
        for item in listed.iter().filter_map(Value::as_str) {
            let block = format!("item-{item}");
            let lesson = item.strip_prefix("lesson-");
            let mut args = vec!["run", "read", lesson.map_or(block.as_str(), |_| "lessons")];
            args.extend(lesson.into_iter().flat_map(|n| ["--term", n]));
            args.extend(["--root", &root, "--spec", self.spec]);
            let out = self.command_in(&copy, &args, "");
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stdout));
        }
        copy
    }

    pub fn log(&self) -> SpecLog {
        store::read(&store::spec_file(&self.root, self.spec).expect("spec file")).expect("readable").expect("the spec file")
    }

    /// A fala do usuário, pelo gancho da entrada; devolve o número dela.
    fn user_says(&self, text: &str) -> u64 {
        self.hook(
            "UserPromptSubmit",
            &json!({"hook_event_name": "UserPromptSubmit", "prompt": text, "session_id": SESSION, "cwd": self.root.to_string_lossy()}),
        );
        self.log()
            .visible()
            .into_iter()
            .rfind(|e| e.event_type == "message" && e.str_field("author") == Some("user") && e.str_field("text") == Some(text))
            .expect("the entry hook recorded the message")
            .id
    }

    /// A spec aberta, com o levantamento inteiro, o plano de uma tarefa que
    /// muda `src/main.rs` e o clique em "Aprovar", pronta para a primeira
    /// rodada. Devolve a resposta da abertura.
    pub fn approved(&self) -> Value {
        let opened = self.open();
        let said = self.user_says(GOAL);
        self.write("context", &json!({"title": "Combinar o item", "agent": "- conferir pelo teste", "text": GOAL, "origin": said}));
        let grilled = self.run(&["grill", "--spec", self.spec, "--kinds", "feature", "--full"]);
        let points = grilled["points"].as_array().cloned().expect("the point list");
        let mut current = Value::Null;
        for point in &points {
            let facts = json!([{"text": "A soma mora no programa.", "source": "src/main.rs:2"}]);
            current = self.write("point", &json!({"replaces": point["id"], "facts": facts}))["point"].clone();
        }
        // A resposta do usuário vem depois dos pontos que ela fecha.
        let answered = self.user_says("Respondi todos os pontos.");
        for point in &points {
            let code = current["code"].as_str().expect("the open point").to_string();
            let answer = self.write(
                "decision",
                &json!({"title": "Combinar o item", "agent": format!("- ponto {code}"), "text": "O usuário respondeu ao ponto.", "keys": ["levantamento"],
                    "why": "o usuário respondeu", "origin": answered, "applies_to": {"files": ["**"]}}),
            );
            current = self.write(
                "point",
                &json!({"block": point["block"], "gap": point["gap"], "from": "gap", "status": "closed",
                    "closes": code, "result": [answer["id"]], "origin": answered}),
            )["point"]
                .clone();
        }

        let said = self.user_says("O plano é uma tarefa só, que soma dois números.");
        let criterion = self.write(
            "criterion",
            &json!({"title": "Combinar o item", "when": "o programa roda", "then": "a soma aparece", "proof": "git --version",
                "form": "ubiquitous", "origin": said}),
        );
        self.write(
            "task",
            &json!({"agent": "- conferir pelo teste", "title": "Entregar a tarefa", "text": "Somar dois números no programa.",
                "files": [{"path": "src/main.rs"}], "depends_on": [], "covers": [criterion["id"]], "origin": said}),
        );
        self.run(&["plan", "--spec", self.spec]);
        let question = translate("approval.question", Locale::PtBr);
        let yes = translate("approval.option", Locale::PtBr);
        self.hook(
            "PostToolUse",
            &json!({"hook_event_name": "PostToolUse", "tool_name": "AskUserQuestion",
                "tool_input": {"questions": [{"question": question, "options": [{"label": yes}, {"label": "Ajustar"}]}]},
                "tool_response": {"answers": {question: yes}}, "session_id": SESSION, "cwd": self.root.to_string_lossy()}),
        );
        opened
    }

    /// A onda 1, já despachada, muda `src/main.rs` na cópia dela, grava cada
    /// passo e a entrega, e a rodada seguinte a assume e comita. Devolve a
    /// resposta dessa rodada.
    pub fn deliver_the_first_wave(&self) -> Value {
        self.return_the_first_wave();
        self.run(&["round", "--dispatch", "--spec", self.spec])
    }

    /// A onda 1, já despachada, muda `src/main.rs` na cópia dela e grava cada
    /// passo e a entrega, sem a rodada que a assume.
    pub fn return_the_first_wave(&self) {
        let copy = self.read_send(|sent| sent.wave() == Some(1) && sent.str_field("role") != Some("review"));
        std::fs::write(copy.join("src/main.rs"), "fn main() {\n    println!(\"{}\", 3);\n}\n").expect("the change");
        let log = self.log();
        let codes = log.codes();
        for task in log.visible().into_iter().filter(|e| e.event_type == "task" && e.wave() == Some(1)) {
            let item = codes.get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
            self.write("step", &json!({"wave": 1, "item": item, "text": "A tarefa ficou pronta."}));
        }
        let agreed: Vec<Value> =
            mustard_core::domain::wave_prompt::all_agreed(&log).iter().map(|item| json!({"item": item.id, "met": true})).collect();
        self.write("delivered", &json!({"wave": 1, "text": "A soma apareceu.", "files": ["src/main.rs"], "commit": "a soma aparece", "agreed": agreed}));
    }
}
