// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// O scan de mentira é um script de shell com permissão de execução do Unix.
#![cfg(unix)]

//! O mapa que outra compilação do scan gravou se refaz com o projeto parado.
//!
//! O mapa guarda em cada bloco a marca da compilação do scan que o gravou. O
//! programa novo, com o scan novo ao lado, achava esse mapa em dia enquanto
//! o commit e os arquivos fossem os mesmos, e a busca respondia com o que o
//! scan velho tirou do código. A prova roda o programa inteiro — a busca do
//! mapa e o início da sessão — sobre um projeto parado, com uma cópia do
//! programa ao lado de um scan de mentira que diz a marca que o teste quer e
//! anota cada passada que lhe pedem.

#[path = "support/executable.rs"]
mod executable;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use mustard_core::domain::normalize::Languages;
use mustard_core::io::project_map as store;
use serde_json::json;

/// A marca com que o mapa do projeto foi gravado.
const WRITTEN_BY: &str = "0.2.4+map-aaaaaaaaaaaaaaaa";

/// O ambiente do teste: a cópia do programa e o scan de mentira lado a lado,
/// e o projeto parado com o mapa de uma passada de `WRITTEN_BY`.
struct Setup {
    _dir: tempfile::TempDir,
    rt: PathBuf,
    project: PathBuf,
    home: PathBuf,
    format: PathBuf,
    calls: PathBuf,
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

        // A cópia do programa: o scan é o que está ao lado de quem roda, e o
        // do teste não pode ser o que a compilação deixou em `target/`. O
        // programa é copiado por outro processo, como o falso é gravado: um
        // arquivo que este processo mantém aberto para escrita o Linux
        // recusa rodar.
        let rt = bin.join("mustard-rt");
        let copied = Command::new("cp").arg(env!("CARGO_BIN_EXE_mustard-rt")).arg(&rt).status().unwrap();
        assert!(copied.success(), "the copy of the program is made");

        // O scan de mentira: `format` diz a marca do arquivo `format`; a
        // passada anota os argumentos e responde o relato de uma linha; a
        // leitura da história, que a passada começa em segundo plano, não
        // anota nada.
        let format = dir.path().join("format");
        let calls = dir.path().join("calls");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = format ]; then cat '{}'; exit 0; fi\nif [ \"$1\" = history-all ]; then exit 0; fi\n\
             echo \"$@\" >> '{}'\n\
             echo '{{\"ok\":true,\"full\":false,\"read\":[],\"files\":1,\"head\":\"\"}}'\n",
            format.display(),
            calls.display()
        );
        executable::write_executable(&bin.join("scan"), &script);

        // O projeto parado: um commit, e o mapa da passada que o leu.
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
        // A pasta `.claude/` fica fora do git, como a instalação a deixa: o
        // que a busca e a sessão gravam nela não muda o conteúdo do projeto.
        std::fs::write(project.join(".git/info/exclude"), ".claude/\n").unwrap();
        std::fs::write(project.join("a.txt"), "x").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "semente"]);
        let now = store::listing(&project).expect("dentro do git");
        let map = json!({
            "state": {"head": now.head, "listing": now.digest(), "base": now.base.name, "base_tip": now.base.tip},
            "modules": [{"path": "src/pedido.rs", "loc": 10, "declarations": [
                {"kind": "function", "name": "gravar_pedido", "line": 1, "end_line": 3}]}]
        });
        store::save_at(&store::model_path(&project), &map, WRITTEN_BY, &Languages::new(["pt-BR", "en-US"])).unwrap();

        Self { _dir: dir, rt, project, home, format, calls }
    }

    /// O scan de mentira passa a dizer a marca `mark`, e o anotado some.
    fn scan_says(&self, mark: &str) {
        std::fs::write(&self.format, mark).unwrap();
        let _ = std::fs::remove_file(&self.calls);
    }

    /// As passadas que o scan de mentira recebeu, uma por linha.
    fn passes(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default()
    }

    /// Roda a cópia do programa no projeto, com uma pasta pessoal falsa.
    fn run(&self, args: &[&str], stdin: &str) {
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
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// A busca do mapa, que confere o mapa antes de responder.
    fn search(&self) {
        self.run(&["run", "map", "search", "--query", "pedido", "--root", self.project.to_str().unwrap()], "");
    }

    /// O início da sessão, que confere o mapa antes de qualquer aviso.
    fn session_start(&self) {
        let payload = json!({
            "hook_event_name": "SessionStart",
            "source": "startup",
            "session_id": "s-outro-scan",
            "cwd": self.project.to_string_lossy(),
        });
        self.run(&["on", "SessionStart"], &payload.to_string());
    }
}

/// Com o commit e os arquivos da passada que gravou o mapa, o scan ao lado
/// que diz a mesma marca dos blocos não é chamado, nem pela busca nem pelo
/// início da sessão; o que diz outra marca é chamado uma vez em cada, com a
/// pasta do projeto e o mapa dela; e o que não responde não põe o mapa atrás.
#[test]
fn the_map_of_another_scan_is_read_again_by_the_search_and_the_session_start_with_the_project_parked() {
    let setup = Setup::new();
    let root = setup.project.to_str().unwrap();

    setup.scan_says(WRITTEN_BY);
    setup.search();
    assert_eq!(setup.passes(), Vec::<String>::new(), "search: the scan of the map is the one beside the program");
    setup.session_start();
    assert_eq!(setup.passes(), Vec::<String>::new(), "start: the scan of the map is the one beside the program");

    setup.scan_says("0.2.4+map-bbbbbbbbbbbbbbbb");
    setup.search();
    let passes = setup.passes();
    assert_eq!(passes.len(), 1, "the search asks for one pass: {passes:?}");
    assert!(
        passes[0].starts_with(&format!("scan {root} --out ")) && passes[0].ends_with(" --json"),
        "the pass is over the project and its map: {passes:?}"
    );

    setup.scan_says("0.2.4+map-bbbbbbbbbbbbbbbb");
    setup.session_start();
    assert_eq!(setup.passes().len(), 1, "the session start asks for one pass: {:?}", setup.passes());

    setup.scan_says("");
    setup.search();
    setup.session_start();
    assert_eq!(setup.passes(), Vec::<String>::new(), "a scan that says no mark does not put the map behind");
}
