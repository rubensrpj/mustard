// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// O scan de mentira é um script de shell com permissão de execução do Unix.
#![cfg(unix)]

//! O mapa que falta se cria na busca e no início da sessão.
//!
//! Dentro do git e sem o arquivo do mapa, a busca do mapa e o início da sessão
//! pedem ao scan uma passada sobre o projeto, e o mapa passa a existir; com o
//! mapa em dia, nenhuma das duas roda o scan. Com o mapa criado, as duas também
//! começam, em segundo plano, a leitura da história de todo arquivo dele. A
//! prova roda o programa inteiro sobre um projeto parado, com uma cópia do
//! programa ao lado de um scan de mentira que anota cada passada e cada
//! leitura da história, e grava o banco do mapa onde a passada manda.

#[path = "support/executable.rs"]
mod executable;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use mustard_core::domain::normalize::Languages;
use mustard_core::io::project_map as store;
use serde_json::{json, Value};

/// O ambiente do teste: a cópia do programa e o scan de mentira lado a lado, e
/// o projeto parado, com um commit e sem mapa.
struct Setup {
    _dir: tempfile::TempDir,
    rt: PathBuf,
    project: PathBuf,
    home: PathBuf,
    calls: PathBuf,
    reads: PathBuf,
    fail: PathBuf,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let project = dir.path().join("project");
        let home = dir.path().join("home");
        for folder in [&bin, &project, &home] {
            std::fs::create_dir_all(folder).unwrap();
        }

        // A cópia do programa, feita por outro processo (ver
        // `support/executable.rs`): o scan é o que está ao lado de quem roda.
        let rt = bin.join("mustard-rt");
        let copied = Command::new("cp").arg(env!("CARGO_BIN_EXE_mustard-rt")).arg(&rt).status().unwrap();
        assert!(copied.success(), "the copy of the program is made");

        // O projeto parado: um commit, com a pasta `.claude/` fora do git,
        // como a instalação a deixa.
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(&project)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        std::fs::write(project.join(".git/info/exclude"), ".claude/\n").unwrap();
        std::fs::write(project.join("a.txt"), "x").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "semente"]);

        // O mapa que a passada do scan de mentira grava: o do commit e dos
        // arquivos de agora, com uma função.
        let now = store::listing(&project).expect("dentro do git");
        let map = json!({
            "state": {"head": now.head, "listing": now.digest(), "base": now.base.name, "base_tip": now.base.tip},
            "modules": [{"path": "src/pedido.rs", "loc": 10, "declarations": [
                {"kind": "function", "name": "gravar_pedido", "line": 1, "end_line": 3}]}]
        });
        let template = dir.path().join("map.db");
        store::save_at(&template, &map, "0.2.4+map-test", &Languages::new(["pt-BR", "en-US"])).unwrap();

        // A passada anota os argumentos e grava o mapa no `--out` (o quarto
        // argumento); com o arquivo `fail` ao lado, ela falha sem gravar. A
        // leitura da história só anota os argumentos, em outro arquivo.
        let calls = dir.path().join("calls");
        let reads = dir.path().join("reads");
        let fail = dir.path().join("fail");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = format ]; then exit 1; fi\n\
             if [ \"$1\" = history-all ]; then echo \"$@\" >> '{reads}'; exit 0; fi\necho \"$@\" >> '{calls}'\n\
             if [ -f '{fail}' ]; then echo 'scan: broken' >&2; exit 1; fi\nmkdir -p \"$(dirname \"$4\")\" && cp '{template}' \"$4\"\n\
             echo '{{\"ok\":true,\"full\":true,\"read\":[],\"files\":1,\"head\":\"\"}}'\n",
            calls = calls.display(),
            reads = reads.display(),
            fail = fail.display(),
            template = template.display(),
        );
        executable::write_executable(&bin.join("scan"), &script);

        Self { _dir: dir, rt, project, home, calls, reads, fail }
    }

    fn model(&self) -> PathBuf {
        store::model_path(&self.project)
    }

    /// As passadas que o scan de mentira recebeu, uma por linha.
    fn passes(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default()
    }

    /// As leituras da história que o scan de mentira recebeu, uma por linha.
    /// O programa não espera por elas: quem pergunta espera o processo em
    /// segundo plano anotar, até um tempo.
    fn reads_after_waiting(&self, at_least: usize) -> Vec<String> {
        let read = || -> Vec<String> {
            std::fs::read_to_string(&self.reads).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default()
        };
        for _ in 0..200 {
            if read().len() >= at_least {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        read()
    }

    /// As leituras da história depois de dar ao processo em segundo plano o
    /// tempo de anotar uma que não devia existir.
    fn reads_after_a_pause(&self) -> Vec<String> {
        std::thread::sleep(std::time::Duration::from_millis(400));
        self.reads_after_waiting(0)
    }

    /// Roda a cópia do programa no projeto, com uma pasta pessoal falsa, e
    /// devolve o que ele imprimiu.
    fn run(&self, args: &[&str], stdin: &str) -> String {
        let mut child = Command::new(&self.rt)
            .args(args)
            .current_dir(&self.project)
            .env("CLAUDE_PROJECT_DIR", &self.project)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_PLUGIN_ROOT")
            .env_remove("MUSTARD_ACTIVE_SPEC")
            .env("MUSTARD_CLAUDE_BIN", self.home.join("no-claude-here"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the program runs");
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(stdin.as_bytes());
        }
        let out = child.wait_with_output().expect("the program finishes");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A busca do mapa, que confere o mapa antes de responder.
    fn search(&self) -> Value {
        let root = self.project.to_str().unwrap();
        let printed = self.run(&["run", "map", "search", "--query", "pedido", "--root", root], "");
        serde_json::from_str(&printed).unwrap_or_else(|e| panic!("the search answers JSON ({e}): {printed}"))
    }

    /// O início da sessão, que confere o mapa antes de qualquer aviso.
    fn session_start(&self) {
        let payload = json!({
            "hook_event_name": "SessionStart",
            "source": "startup",
            "session_id": "s-mapa-ausente",
            "cwd": self.project.to_string_lossy(),
        });
        self.run(&["on", "SessionStart"], &payload.to_string());
    }
}

/// Sem o arquivo do mapa, a busca pede uma passada ao scan, sobre o projeto e
/// para o mapa dele, e responde com o mapa criado, sem recusa. Com o mapa em
/// dia, a busca seguinte não roda o scan. Apagado o mapa, a busca seguinte o
/// cria de novo.
#[test]
fn the_search_creates_a_missing_map_and_again_after_it_is_deleted() {
    let setup = Setup::new();
    let root = setup.project.to_str().unwrap();
    assert!(!setup.model().exists(), "the project starts with no map");

    let answer = setup.search();
    assert_ne!(answer["ok"], json!(false), "no refusal for a map that can be created: {answer}");
    assert_ne!(answer["reason"], json!("map-missing"), "{answer}");
    let passes = setup.passes();
    assert_eq!(passes.len(), 1, "the first search asks for one pass: {passes:?}");
    assert!(
        passes[0].starts_with(&format!("scan {root} --out ")) && passes[0].ends_with(" --json"),
        "the pass is over the project and its map: {passes:?}"
    );
    assert!(passes[0].contains(setup.model().to_str().unwrap()), "{passes:?}");
    assert!(setup.model().is_file(), "the map exists after the search");
    let reads = setup.reads_after_waiting(1);
    assert_eq!(
        reads,
        [format!("history-all {root} --out {} --json", setup.model().display())],
        "the search that made the map starts the reading of the history of every file, in the background"
    );

    setup.search();
    assert_eq!(setup.passes().len(), 1, "the map is up to date: no new pass");
    assert_eq!(setup.reads_after_a_pause().len(), 1, "the map is up to date: no new reading");

    std::fs::remove_file(setup.model()).unwrap();
    let again = setup.search();
    assert_ne!(again["reason"], json!("map-missing"), "{again}");
    assert_eq!(setup.passes().len(), 2, "the deleted map asks for a new pass");
    assert!(setup.model().is_file(), "the map is back after the next search");
    assert_eq!(setup.reads_after_waiting(2).len(), 2, "the map made again starts the reading again");
}

/// O início da sessão cria o mapa que falta, e o mapa em dia não o faz rodar o
/// scan.
#[test]
fn the_session_start_creates_a_missing_map() {
    let setup = Setup::new();

    setup.session_start();
    assert_eq!(setup.passes().len(), 1, "the session start asks for one pass: {:?}", setup.passes());
    assert!(setup.model().is_file(), "the map exists after the session start");
    assert_eq!(setup.reads_after_waiting(1).len(), 1, "the map made at the session start starts the reading of the history");

    setup.session_start();
    assert_eq!(setup.passes().len(), 1, "the map is up to date: no new pass");
    assert_eq!(setup.reads_after_a_pause().len(), 1, "the map is up to date: no new reading");
}

/// O scan que falha deixa o projeto sem mapa: a busca recusa com o mapa
/// ausente, como antes, sem travar, e a busca seguinte tenta de novo.
#[test]
fn a_scan_that_fails_leaves_the_refusal_of_the_missing_map() {
    let setup = Setup::new();
    std::fs::write(&setup.fail, "").unwrap();

    let answer = setup.search();
    assert_eq!(answer["reason"], json!("map-missing"), "{answer}");
    assert!(!setup.model().exists(), "the scan wrote nothing");

    setup.search();
    assert_eq!(setup.passes().len(), 2, "the next search tries again");
    assert!(setup.reads_after_a_pause().is_empty(), "no map was made: no reading of the history starts");
}

/// O comando `run scan` também começa a leitura da história de todo arquivo,
/// em segundo plano, com o mapa gravado, e o comando não espera por ela.
#[test]
fn the_scan_command_starts_the_reading_of_the_history_of_every_file() {
    let setup = Setup::new();
    let root = setup.project.to_str().unwrap();

    let printed = setup.run(&["run", "scan", "--root", root], "");
    let report: Value = serde_json::from_str(&printed).unwrap_or_else(|e| panic!("the scan answers JSON ({e}): {printed}"));
    assert_eq!(report["ok"], json!(true), "{report}");
    assert_eq!(setup.passes().len(), 1, "one pass: {:?}", setup.passes());
    assert_eq!(
        setup.reads_after_waiting(1),
        [format!("history-all {root} --out {} --json", setup.model().display())],
        "the map written by the command starts the reading of the history"
    );
}
