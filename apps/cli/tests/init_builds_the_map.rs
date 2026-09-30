// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// O scan de mentira é um script de shell com permissão de execução do Unix.
#![cfg(unix)]

//! `mustard init` deixa o projeto com o mapa criado, também com `--yes` e com o
//! projeto ainda vazio.
//!
//! A prova roda o programa inteiro, como a pessoa o roda: uma cópia do
//! `mustard` ao lado de um scan de mentira, que é onde o programa procura o
//! scan. O de mentira anota cada passada que lhe pedem e, como o de verdade,
//! grava o banco do mapa no lugar que a passada manda; a leitura da história
//! que a instalação começa em segundo plano ele só anota, em outro arquivo.

#[path = "support/executable.rs"]
mod executable;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mustard_core::domain::normalize::Languages;
use mustard_core::io::project_map as store;
use serde_json::json;

/// O ambiente do teste: a cópia do programa e o scan de mentira lado a lado, o
/// projeto vazio com `git init` e uma pasta pessoal falsa.
struct Setup {
    _dir: tempfile::TempDir,
    mustard: PathBuf,
    bin: PathBuf,
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
        let mustard = bin.join("mustard");
        let copied = Command::new("cp").arg(env!("CARGO_BIN_EXE_mustard")).arg(&mustard).status().unwrap();
        assert!(copied.success(), "the copy of the program is made");
        // As duas sondas da instalação só perguntam a versão.
        for name in ["rtk", "rg"] {
            std::os::unix::fs::symlink(&mustard, bin.join(name)).unwrap();
        }

        // O mapa vazio que o scan de mentira grava: o de um projeto sem código.
        let template = dir.path().join("empty-map.db");
        store::save_at(&template, &json!({"modules": []}), "0.2.4+map-test", &Languages::new(["pt-BR", "en-US"])).unwrap();

        // A passada anota os argumentos e grava o mapa no `--out` (o quarto
        // argumento); com o arquivo `fail` na pasta, ela falha sem gravar.
        let calls = dir.path().join("calls");
        let reads = dir.path().join("reads");
        let fail = dir.path().join("fail");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = format ]; then exit 1; fi\n\
             if [ \"$1\" = history-all ]; then echo \"$@\" >> '{reads}'; exit 0; fi\necho \"$@\" >> '{calls}'\n\
             if [ -f '{fail}' ]; then echo 'scan: broken' >&2; exit 1; fi\nmkdir -p \"$(dirname \"$4\")\" && cp '{template}' \"$4\"\n\
             echo '{{\"ok\":true,\"full\":true,\"read\":[],\"files\":0,\"head\":\"\"}}'\n",
            calls = calls.display(),
            reads = reads.display(),
            fail = fail.display(),
            template = template.display(),
        );
        executable::write_executable(&bin.join("scan"), &script);

        let git = Command::new("git").args(["init", "-q"]).current_dir(&project).status().unwrap();
        assert!(git.success(), "git init");

        Self { _dir: dir, mustard, bin, project, home, calls, reads, fail }
    }

    /// `mustard init --yes` no projeto, com uma pasta pessoal falsa.
    fn init(&self) -> Output {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let joined = std::env::join_paths(std::iter::once(self.bin.clone()).chain(std::env::split_paths(&path))).unwrap();
        Command::new(&self.mustard)
            .args(["init", "--yes"])
            .current_dir(&self.project)
            .env("PATH", joined)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .output()
            .expect("the program runs")
    }

    /// As passadas que o scan de mentira recebeu, uma por linha.
    fn passes(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default()
    }

    /// Se nenhuma leitura da história foi anotada, dando um instante ao
    /// processo em segundo plano que a anotaria.
    fn no_reading_started(&self) -> bool {
        std::thread::sleep(std::time::Duration::from_millis(400));
        !self.reads.exists()
    }

    /// As leituras da história que o scan de mentira recebeu, uma por linha:
    /// a instalação não espera por elas, então se espera até um tempo que o
    /// processo em segundo plano anote a primeira.
    fn reads_after_waiting(&self) -> Vec<String> {
        let read = || -> Vec<String> {
            std::fs::read_to_string(&self.reads).map(|text| text.lines().map(str::to_string).collect()).unwrap_or_default()
        };
        for _ in 0..200 {
            if !read().is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        read()
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "init failed ({}):\n{}\n{}",
        out.status,
        stdout(out),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn model_of(project: &Path) -> PathBuf {
    store::model_path(&project.canonicalize().unwrap())
}

/// A instalação num projeto vazio pede ao scan uma passada sobre o projeto,
/// para o mapa dele, e o mapa passa a existir; a instalação diz que o criou.
#[test]
fn init_yes_in_an_empty_project_builds_the_map() {
    let setup = Setup::new();
    assert!(!model_of(&setup.project).exists(), "the project starts with no map");

    let out = setup.init();
    assert_ok(&out);

    let root = setup.project.canonicalize().unwrap();
    assert_eq!(
        setup.passes(),
        vec![format!("scan {} --out {} --json", root.display(), model_of(&setup.project).display())],
        "one pass over the project, into its map"
    );
    assert!(model_of(&setup.project).is_file(), "the map exists after the install");
    assert!(stdout(&out).contains("built the project map (0 code files)"), "{}", stdout(&out));
    assert_eq!(
        setup.reads_after_waiting(),
        vec![format!("history-all {} --out {} --json", root.display(), model_of(&setup.project).display())],
        "the map built by the install starts the reading of the history of every file, in the background"
    );
}

/// Um scan que falha não derruba a instalação: o `init` termina bem, o resto
/// do que instala está no disco, e uma linha de aviso diz que o mapa não saiu.
#[test]
fn init_installs_and_warns_when_the_scan_fails() {
    let setup = Setup::new();
    std::fs::write(&setup.fail, "").unwrap();

    let out = setup.init();
    assert_ok(&out);

    assert_eq!(setup.passes().len(), 1, "the scan was asked");
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(setup.no_reading_started(), "no map was built: no reading of the history starts");
    assert!(!model_of(&setup.project).exists(), "the scan wrote nothing");
    assert!(setup.project.join(".claude").join("settings.local.json").is_file(), "the install is on disk");
    assert!(setup.project.join("mustard.json").is_file(), "the install is on disk");
    let told = stdout(&out);
    let warnings: Vec<&str> = told.lines().filter(|line| line.contains("project map was not built")).collect();
    assert_eq!(warnings.len(), 1, "one warning line: {told}");
    assert!(warnings[0].contains("scan: broken"), "{told}");
}
