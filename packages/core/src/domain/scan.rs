//! `grain` — typed client for the external grain tool.
//!
//! grain is the deterministic codebase miner (it replaces Mustard's old scan
//! engine entirely). Mustard never reads project source to understand a repo;
//! it shells out to the grain binary and consumes its JSON/Markdown:
//!
//! - `grain scan <root> --out <model.json>` — the durable model (run once/repo).
//!
//! What the map holds is read back through the map port
//! (`io/project_map.rs`), never through another run of the tool.
//!
//! The boundary is a TOOL (process + JSON/MD), not a library link: no shared
//! build, no tree-sitter version coupling, grain stays standalone. This module
//! is the single owner of that boundary. Nothing here is language- or
//! framework-specific — grain is itself fully data-driven.
//!
//! Fail-open: spawning or parsing failures return [`Error`]; callers degrade
//! (e.g. an empty subproject list when the tool is missing).

use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::domain::vocabulary::stacks::StackDetection;
use crate::platform::error::{Error, Result};

/// Default tool name — resolved on `PATH`. A project can point at a pinned
/// binary later (e.g. via `mustard.json`); the locator is injected, never
/// hardcoded at a call site.
pub(crate) const DEFAULT_BINARY: &str = "scan";

/// A handle to the grain tool at a known location.
#[derive(Debug, Clone)]
pub struct Scan {
    binary: String,
}

impl Default for Scan {
    fn default() -> Self {
        Self { binary: DEFAULT_BINARY.to_string() }
    }
}

/// One compilation unit from the map (the `projects` table of `.claude/grain.db`) —
/// the subproject list. Replaces the deleted sync-detect discovery: grain mines
/// the same build-manifest set deterministically.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub code_files: usize,
    /// Frameworks/deps recurring across this unit's manifests (mined by `scan`,
    /// frequency-ranked, top-12). Empty when none mined / older model.
    #[serde(default)]
    pub frameworks: Vec<String>,
    /// Build/codegen scripts declared by this unit's manifests (sorted, deduped).
    #[serde(default)]
    pub scripts: Vec<String>,
    /// Stacks inferred for this unit (registry-driven, see
    /// `domain::vocabulary::stacks`). Additive next to [`Self::frameworks`]
    /// (which stays the raw frequency-ranked dep list); empty when the model
    /// predates the field or nothing was inferred.
    #[serde(default)]
    pub detected_stacks: Vec<StackDetection>,
    /// `true` when this subproject's own directory is a NESTED git repository
    /// root (a submodule / linked repo: `.git` present as a directory OR a
    /// pointer file at [`Self::dir`]). The grain miner is git-blind, so this is
    /// stamped by Mustard ([`mark_own_git_roots`]) — never mined. Defaulted
    /// `false` for back-compat with any census that predates the field and for
    /// the superproject root itself (which is not a nested boundary).
    #[serde(default)]
    pub own_git_root: bool,
}

/// Os subprojetos do mapa em `model_path`, lidos só da tabela deles pela
/// porta do mapa ([`crate::io::project_map::projects_at`]), sem abrir outro
/// processo nem ler o resto do mapa. Falha aberta: sem mapa (o scan ainda não
/// rodou) ou com um que não se entende, a lista vem vazia.
#[must_use]
pub fn read_projects(model_path: &std::path::Path) -> Vec<Project> {
    crate::io::project_map::projects_at(model_path).unwrap_or_default()
}

/// Stamp [`Project::own_git_root`] on each census entry by probing whether its
/// directory is a NESTED git repository root (`.git` dir or pointer file),
/// relative to `repo_root`. The grain miner is git-blind (it walks source
/// only), so the git-boundary FACT — "this subproject is its own repo" — is
/// Mustard's to add; the single owner of that probe is
/// [`crate::io::workspace::is_git_repo_root`] (reused, not re-implemented).
///
/// The superproject root itself is never a nested boundary: an empty or `"."`
/// `dir` is skipped (left `false`) so the root project — whose `.git` is the
/// SUPERproject's — is not mistaken for a submodule. Purely a filesystem probe,
/// fail-open: an unreadable / absent dir stays `false`.
pub fn mark_own_git_roots(repo_root: &Path, projects: &mut [Project]) {
    for project in projects.iter_mut() {
        let dir = project.dir.trim();
        if dir.is_empty() || dir == "." {
            continue;
        }
        project.own_git_root = crate::io::workspace::is_git_repo_root(&repo_root.join(dir));
    }
}

impl Scan {
    /// A client for the grain binary at `binary` (a name on `PATH` or a path).
    #[must_use]
    pub fn new(binary: impl Into<String>) -> Self {
        Self { binary: binary.into() }
    }

    /// Locate the bundled grain binary — built as a sibling of the running
    /// executable in the same workspace `target/` dir — falling back to
    /// [`DEFAULT_BINARY`] on `PATH`. Fail-open: any probe error → the fallback.
    #[must_use]
    pub fn locate() -> Self {
        Self::located_from(std::env::current_exe().ok().as_deref())
    }

    /// [`Self::locate`] for the executable at `exe`. A test binary runs from
    /// `deps/`, one folder below the programs of the same build: there the
    /// folder above is searched too, before `PATH`, so a test never runs the
    /// installed scan in place of the one compiled with it. Also how a caller
    /// finds the scan of ANOTHER build than its own: the one beside the
    /// program that build left at `exe` (which need not exist).
    #[must_use]
    pub fn located_from(exe: Option<&Path>) -> Self {
        let name = if cfg!(windows) { "scan.exe" } else { "scan" };
        let dir = exe.and_then(Path::parent);
        let up = dir.filter(|dir| dir.file_name().is_some_and(|n| n == "deps")).and_then(Path::parent);
        let found = dir.into_iter().chain(up).map(|dir| dir.join(name)).find(|cand| cand.is_file());
        Self { binary: found.map_or_else(|| DEFAULT_BINARY.to_string(), |cand| cand.to_string_lossy().into_owned()) }
    }

    /// `true` when this is the scan compiled with the running program, found
    /// by [`Self::locate`], and not the name looked up on `PATH`.
    #[must_use]
    pub fn is_compiled_alongside(&self) -> bool {
        self.binary != DEFAULT_BINARY
    }

    /// Mine `root` into the model file at `out` (`grain scan`). With a model
    /// of the same project already at `out`, the tool reads only what changed
    /// since; the report says which files it read.
    ///
    /// # Errors
    /// [`Error::Io`] if the tool cannot be spawned, [`Error::CheckFailed`] on a
    /// non-zero exit or a report that does not parse.
    pub fn scan(&self, root: &Path, out: &Path) -> Result<ScanReport> {
        parse_scan_report(&self.run(&scan_args(root, out))?)
    }

    /// Read from git the history of each declaration of `file`, in the base
    /// branch the project declares, and keep it in the map at `out` (`grain
    /// history`), following a declaration into the file it came from up to
    /// `moves` times in a row. Only that file's lines of the map change.
    ///
    /// # Errors
    /// [`Error::Io`] if the tool cannot be spawned, [`Error::CheckFailed`] on a
    /// non-zero exit or a report that does not parse.
    pub fn history(&self, root: &Path, out: &Path, file: &str, moves: usize) -> Result<HistoryReport> {
        let stdout = self.run(&history_args(root, out, file, moves))?;
        serde_json::from_str(last_line(&stdout)).map_err(|e| Error::check_failed(format!("scan history report: {e}")))
    }

    /// Como [`Self::scan`], e, com o mapa gravado, começa em segundo plano a
    /// leitura da história de todo arquivo dele ([`Self::read_history_in_background`]).
    /// A leitura que não começa não muda a resposta da passada.
    ///
    /// # Errors
    /// Os de [`Self::scan`].
    pub fn scan_then_read_history(&self, root: &Path, out: &Path) -> Result<ScanReport> {
        let report = self.scan(root, out)?;
        let _ = self.read_history_in_background(root, out);
        Ok(report)
    }

    /// Lê a história de todo arquivo do mapa em `out` que ainda não a tem
    /// (`grain history-all`) e só volta quando a leitura acaba: quem pergunta
    /// ao mapa depois lê a história inteira, não a que já chegou. É o pedido
    /// de [`Self::read_history_in_background`], esperado em vez de solto.
    ///
    /// # Errors
    /// [`Error::Io`] if the tool cannot be spawned, [`Error::CheckFailed`] on a
    /// non-zero exit, ou quando outra leitura do mesmo mapa está em andamento:
    /// o scan sai sem ler nada, e a história fica pela metade.
    pub fn read_history(&self, root: &Path, out: &Path) -> Result<()> {
        let stdout = self.run(&history_all_args(root, out))?;
        if serde_json::from_str::<ReadingReport>(last_line(&stdout)).is_ok_and(|report| report.busy) {
            return Err(Error::check_failed(format!(
                "scan history-all: outra leitura da história de {} está em andamento e esta não leu nada",
                out.display()
            )));
        }
        Ok(())
    }

    /// Começa, em outro processo que segue depois deste, a leitura da história
    /// de todo arquivo do mapa em `out` que ainda não a tem (`grain
    /// history-all`), sem esperar por ela: quem pergunta ao mapa no meio da
    /// leitura lê o que já está gravado. O processo não herda a entrada, a
    /// saída nem o grupo do terminal de quem o chamou. Outra leitura do mesmo
    /// mapa em andamento faz a nova sair sem ler nada.
    ///
    /// # Errors
    /// [`Error::Io`] if the tool cannot be spawned.
    pub fn read_history_in_background(&self, root: &Path, out: &Path) -> Result<()> {
        let mut command = Command::new(&self.binary);
        command.args(history_all_args(root, out)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        detach(&mut command);
        let mut child = command.spawn()?;
        // Só recolhe o processo quando ele acaba, se este ainda estiver de pé.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    /// A marca de formato que este scan grava em cada bloco do mapa
    /// (`scan format`): a versão e o resumo das fontes dele. Com ela se sabe,
    /// sem rodar a passada, se o mapa é de outra compilação do scan e o
    /// mesmo projeto, parado, rende outro mapa. `None` quando o scan não
    /// roda ou não diz a marca: quem pergunta não julga o mapa por ela.
    #[must_use]
    pub fn format(&self) -> Option<String> {
        let stdout = self.run(&["format".to_string()]).ok()?;
        let mark = stdout.trim();
        (!mark.is_empty()).then(|| mark.to_string())
    }

    /// Run grain with `args`, returning stdout. Maps a non-zero exit (with
    /// stderr) to [`Error::CheckFailed`].
    fn run(&self, args: &[String]) -> Result<String> {
        let output = Command::new(&self.binary).args(args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::check_failed(format!("scan {}: {}", args.join(" "), stderr.trim())));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

// --- pure arg builders (unit-testable without the binary present) -----------

fn scan_args(root: &Path, out: &Path) -> Vec<String> {
    vec![
        "scan".to_string(),
        root.to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
        "--json".to_string(),
    ]
}

fn history_args(root: &Path, out: &Path, file: &str, moves: usize) -> Vec<String> {
    vec![
        "history".to_string(),
        root.to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
        "--file".to_string(),
        file.to_string(),
        "--moves".to_string(),
        moves.to_string(),
        "--json".to_string(),
    ]
}

fn history_all_args(root: &Path, out: &Path) -> Vec<String> {
    vec![
        "history-all".to_string(),
        root.to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
        "--json".to_string(),
    ]
}

/// Solta o processo de `command` do grupo e do terminal de quem o inicia: o
/// que ele faz não morre com o fechamento do terminal.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

/// Solta o processo de `command` do console de quem o inicia
/// (`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`).
#[cfg(windows)]
fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0000_0008 | 0x0000_0200);
}

#[cfg(not(any(unix, windows)))]
fn detach(_command: &mut Command) {}

/// What `scan history` reports on its last stdout line: the file, how many
/// commits of the base changed it, and how many of its declarations the base
/// has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct HistoryReport {
    pub file: String,
    pub commits: usize,
    pub declarations: usize,
}

/// What one scan pass reports on its last stdout line: whether it read every
/// file, the files it read, how many code files the map has, the commit it
/// read from, and whether it rewrote the dictionary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ScanReport {
    pub full: bool,
    pub read: Vec<String>,
    pub files: usize,
    pub head: String,
    pub dictionary: bool,
}

/// What `scan history-all --json` reports on its last stdout line that this
/// side reads: whether another reading of the same map was running, so this
/// one read nothing.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ReadingReport {
    busy: bool,
}

/// The last non-empty line of what a `--json` run printed, where the report
/// is; `{}` when it printed nothing.
fn last_line(stdout: &str) -> &str {
    stdout.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("{}")
}

/// The report a `scan --json` run printed: its last non-empty line.
fn parse_scan_report(stdout: &str) -> Result<ScanReport> {
    serde_json::from_str(last_line(stdout)).map_err(|e| Error::check_failed(format!("scan report: {e}")))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn scan_args_shape() {
        let a = scan_args(&PathBuf::from("repo"), &PathBuf::from("m.json"));
        assert_eq!(a, vec!["scan", "repo", "--out", "m.json", "--json"]);
    }

    #[test]
    fn history_args_shape() {
        let a = history_args(&PathBuf::from("repo"), &PathBuf::from("m.db"), "src/a.rs", 3);
        assert_eq!(a, vec!["history", "repo", "--out", "m.db", "--file", "src/a.rs", "--moves", "3", "--json"]);
    }

    #[test]
    fn history_all_args_shape() {
        let a = history_all_args(&PathBuf::from("repo"), &PathBuf::from("m.db"));
        assert_eq!(a, vec!["history-all", "repo", "--out", "m.db", "--json"]);
    }

    /// Um scan de mentira: o programa `name` em `dir` que roda `body`. Ele é
    /// gravado por um shell à parte: os testes rodam em paralelo no mesmo
    /// processo, e o arquivo que este processo mantém aberto para escrita o
    /// Linux recusa rodar ("Text file busy").
    #[cfg(unix)]
    pub(crate) fn script_scan(dir: &Path, name: &str, body: &str) -> Scan {
        let path = dir.join(name);
        let written = Command::new("/bin/sh")
            .args(["-c", "printf '%s' \"$2\" > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(&path)
            .arg(format!("#!/bin/sh\n{body}\n"))
            .status()
            .unwrap();
        assert!(written.success());
        Scan::new(path.to_string_lossy())
    }

    /// Espera até um minuto por `done`, olhando a cada 20 ms; `false` se ele não veio.
    #[cfg(unix)]
    fn wait_until(done: impl Fn() -> bool) -> bool {
        let limit = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !done() {
            if std::time::Instant::now() > limit {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }

    /// Os pedidos que o scan de mentira anotou em `log`, um por linha.
    #[cfg(unix)]
    fn logged(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// A leitura da história não segura quem a pede: o processo dela só acaba
    /// quando o teste o solta, e a chamada já voltou.
    #[cfg(unix)]
    #[test]
    fn the_reading_of_the_history_starts_in_the_background_and_the_call_does_not_wait_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let (log, release) = (dir.path().join("log"), dir.path().join("release"));
        let scan = script_scan(
            dir.path(),
            "reads",
            &format!(
                "echo \"$1\" >> '{log}'\nn=0\nwhile [ ! -e '{release}' ] && [ $n -lt 3000 ]; do sleep 0.02; n=$((n+1)); done\n\
                 if [ -e '{release}' ]; then echo finished >> '{log}'; else echo gave-up >> '{log}'; fi",
                log = log.display(),
                release = release.display()
            ),
        );
        scan.read_history_in_background(dir.path(), &dir.path().join("m.db")).expect("the reading starts");
        assert!(wait_until(|| logged(&log) == ["history-all"]), "the reading was started with its command: {:?}", logged(&log));
        assert!(!release.exists(), "the call came back while the process was still held");
        std::fs::write(&release, "").unwrap();
        assert!(wait_until(|| logged(&log).len() == 2), "{:?}", logged(&log));
        assert_eq!(logged(&log), ["history-all", "finished"]);
    }

    /// Só o mapa gravado ganha a leitura: o scan que falha devolve o erro dele
    /// e não a inicia; o que passa devolve o relato e a inicia.
    #[cfg(unix)]
    #[test]
    fn the_reading_of_the_history_starts_only_after_a_scan_that_passed() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let fails = dir.path().join("fails");
        let scan = script_scan(
            dir.path(),
            "both",
            &format!(
                "echo \"$1\" >> '{log}'\nif [ \"$1\" = scan ]; then [ -e '{fails}' ] && exit 3; echo '{{\"ok\":true,\"full\":true,\"read\":[],\"files\":2}}'; fi",
                log = log.display(),
                fails = fails.display()
            ),
        );
        let out = dir.path().join("m.db");

        std::fs::write(&fails, "").unwrap();
        assert!(scan.scan_then_read_history(dir.path(), &out).is_err(), "the failed scan is the answer");
        // Um sinal de que o scan de mentira responde depressa: a leitura pedida
        // à mão chega ao registro, e nenhuma outra veio antes dela.
        scan.read_history_in_background(dir.path(), &out).unwrap();
        assert!(wait_until(|| logged(&log).contains(&"history-all".to_string())), "{:?}", logged(&log));
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(logged(&log), ["scan", "history-all"], "the failed scan started no reading");

        std::fs::remove_file(&fails).unwrap();
        let report = scan.scan_then_read_history(dir.path(), &out).expect("the scan passed");
        assert_eq!(report.files, 2);
        assert!(wait_until(|| logged(&log).len() == 4), "{:?}", logged(&log));
        assert_eq!(logged(&log)[2..], ["scan", "history-all"], "the scan that passed starts the reading after itself");
    }

    /// A leitura esperada só volta com o processo acabado: o scan de mentira
    /// demora para terminar e anota o fim, e a chamada, ao voltar, já o
    /// encontra no registro, sem esperar por ele.
    #[cfg(unix)]
    #[test]
    fn reading_the_history_returns_only_when_the_process_has_finished() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let scan = script_scan(
            dir.path(),
            "slow",
            &format!("echo \"$1\" >> '{log}'\nsleep 1\necho finished >> '{log}'\necho '{{\"ok\":true,\"busy\":false}}'", log = log.display()),
        );

        scan.read_history(dir.path(), &dir.path().join("m.db")).expect("the reading finishes");

        assert_eq!(logged(&log), ["history-all", "finished"], "the call came back before the process finished");
    }

    /// O scan que falha na leitura devolve o erro, com o que ele disse; o que
    /// não existe também.
    #[cfg(unix)]
    #[test]
    fn reading_the_history_with_a_scan_that_fails_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let scan = script_scan(dir.path(), "fails", "echo 'git quebrou' >&2\nexit 3");
        let error = scan.read_history(dir.path(), &dir.path().join("m.db")).unwrap_err().to_string();
        assert!(error.contains("history-all") && error.contains("git quebrou"), "{error}");
        assert!(Scan::new(dir.path().join("no-such-scan").to_string_lossy()).read_history(dir.path(), &dir.path().join("m.db")).is_err());
    }

    /// Outra leitura do mesmo mapa em andamento faz o scan sair com sucesso sem
    /// ler nada, e a história segue pela metade: isso é erro. A leitura que
    /// leu, ou que não disse nada, passa.
    #[cfg(unix)]
    #[test]
    fn reading_the_history_while_another_reading_holds_the_map_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("m.db");
        let busy = script_scan(dir.path(), "busy", "echo '{\"ok\":true,\"busy\":true,\"files\":0}'");
        let error = busy.read_history(dir.path(), &out).unwrap_err().to_string();
        assert!(error.contains("outra leitura") && error.contains("não leu nada"), "{error}");
        let read = script_scan(dir.path(), "read", "echo '{\"ok\":true,\"busy\":false,\"files\":4}'");
        read.read_history(dir.path(), &out).expect("a reading that read is not an error");
        let silent = script_scan(dir.path(), "silent", "true");
        silent.read_history(dir.path(), &out).expect("a scan that says nothing and exits clean is not an error");
    }

    #[test]
    fn a_scan_that_cannot_be_run_has_no_format() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Scan::new(dir.path().join("no-such-scan").to_string_lossy()).format(), None);
    }

    /// A marca é a linha que o comando `format` do scan imprime, sem a
    /// quebra de linha; um scan que sai com erro ou não diz nada não tem
    /// marca. O programa falso é gravado por um shell à parte: os testes
    /// rodam em paralelo no mesmo processo, e o arquivo que este processo
    /// mantém aberto para escrita o Linux recusa rodar ("Text file busy").
    #[cfg(unix)]
    #[test]
    fn the_format_is_what_the_format_command_of_the_scan_prints() {
        let dir = tempfile::tempdir().unwrap();
        let script = |name: &str, body: &str| script_scan(dir.path(), name, body);
        let says = script("says", r#"[ "$1" = format ] && echo "0.2.4+map-0011223344556677""#);
        assert_eq!(says.format().as_deref(), Some("0.2.4+map-0011223344556677"));
        assert_eq!(script("fails", "exit 3").format(), None);
        assert_eq!(script("silent", "true").format(), None);
    }

    #[test]
    fn the_scan_report_is_the_last_line() {
        let report = parse_scan_report("noise\n{\"ok\":true,\"full\":false,\"read\":[\"src/b.rs\"],\"files\":3}\n\n")
            .expect("report");
        assert!(!report.full);
        assert_eq!(report.read, vec!["src/b.rs".to_string()]);
        assert_eq!(report.files, 3);
        assert!(parse_scan_report("not json").is_err());
    }

    /// O programa de teste roda de `deps/`, uma pasta abaixo dos programas da
    /// mesma compilação: o scan de cima é achado; sem ele, sobra o nome puro,
    /// procurado no `PATH`. Ao lado do programa, vale o de lá.
    #[test]
    fn a_test_binary_in_deps_finds_the_scan_one_folder_up() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "scan.exe" } else { "scan" };
        let deps = dir.path().join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        let exe = deps.join("x");
        std::fs::write(&exe, "").unwrap();

        let without = Scan::located_from(Some(&exe));
        assert_eq!(without.binary, DEFAULT_BINARY);
        assert!(!without.is_compiled_alongside());

        let up = dir.path().join(name);
        std::fs::write(&up, "").unwrap();
        let found = Scan::located_from(Some(&exe));
        assert_eq!(found.binary, up.to_string_lossy());
        assert!(found.is_compiled_alongside());

        let beside = deps.join(name);
        std::fs::write(&beside, "").unwrap();
        assert_eq!(Scan::located_from(Some(&exe)).binary, beside.to_string_lossy());

        // Fora de `deps/`, a pasta de cima não conta.
        let other = dir.path().join("bin");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(Scan::located_from(Some(&other.join("x"))).binary, DEFAULT_BINARY);
        assert_eq!(Scan::located_from(None).binary, DEFAULT_BINARY);
    }

    #[test]
    fn detected_stacks_serde_compat() {
        // An old payload without `detected_stacks` still deserialises, and
        // `frameworks` is untouched by the new field.
        let old = r#"{"name":"api","dir":"apps/api","kind":"node","code_files":3,"frameworks":["express"]}"#;
        let p: Project = serde_json::from_str(old).expect("old payload without detected_stacks");
        assert!(p.detected_stacks.is_empty());
        assert_eq!(p.frameworks, vec!["express"]);

        // A new payload carrying the field round-trips into the contract type.
        let new = r#"{"name":"web","frameworks":["laravel/framework"],"detected_stacks":[{"name":"laravel","confidence":0.9,"signals":["dep:laravel/framework"]}]}"#;
        let p: Project = serde_json::from_str(new).expect("payload with detected_stacks");
        assert_eq!(p.detected_stacks.len(), 1);
        assert_eq!(p.detected_stacks[0].name, "laravel");
        assert_eq!(p.detected_stacks[0].signals, vec!["dep:laravel/framework"]);
        assert_eq!(p.frameworks, vec!["laravel/framework"]);
    }

    #[test]
    fn own_git_root_serde_defaults_false_and_roundtrips() {
        // A census that predates the field (grain never mines it) deserialises
        // with `own_git_root == false` — the git boundary is Mustard's to stamp.
        let old = r#"{"name":"api","dir":"apps/api","kind":"node"}"#;
        let p: Project = serde_json::from_str(old).expect("old payload without own_git_root");
        assert!(!p.own_git_root, "absent field defaults false");

        // A payload carrying the flag round-trips into the contract type.
        let new = r#"{"name":"sub","dir":"backend/Sub","own_git_root":true}"#;
        let p: Project = serde_json::from_str(new).expect("payload with own_git_root");
        assert!(p.own_git_root);
    }

    #[test]
    fn mark_own_git_roots_flags_dir_with_dot_git_file() {
        use tempfile::tempdir;
        let root = tempdir().unwrap();
        // A submodule carries `.git` as a FILE (a `gitdir:` pointer), NOT a dir —
        // the exact sialia shape. It must be flagged its own git root.
        let sub = root.path().join("backend").join("Sialia.Backend");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(".git"), b"gitdir: ../../.git/modules/Sialia.Backend\n").unwrap();
        // A plain subproject (no `.git`) stays false.
        std::fs::create_dir_all(root.path().join("apps").join("api")).unwrap();

        let mut projects = vec![
            Project { name: "backend".into(), dir: "backend/Sialia.Backend".into(), ..Default::default() },
            Project { name: "api".into(), dir: "apps/api".into(), ..Default::default() },
            // The superproject root itself (empty / ".") is never a nested boundary.
            Project { name: "root".into(), dir: ".".into(), ..Default::default() },
        ];
        mark_own_git_roots(root.path(), &mut projects);
        assert!(projects[0].own_git_root, "a `.git` FILE marks a nested git root");
        assert!(!projects[1].own_git_root, "a plain subproject is not a nested git root");
        assert!(!projects[2].own_git_root, "the superproject root `.` is never flagged");
    }

    /// Um mapa em que uma coluna que a lista de projetos não lê guarda o tipo
    /// errado: a leitura do mapa inteiro o recusa, e a lista, que lê só a
    /// tabela dos projetos, vem com cada coluna dela.
    #[test]
    fn the_projects_come_from_their_table_even_when_another_column_is_broken() {
        let dir = tempfile::tempdir().unwrap();
        let model = crate::io::project_map::model_path(dir.path());
        crate::io::project_map::write_text_at(
            &model,
            r#"{"modules": [{"path": "web/artisan", "deps": "um texto no lugar da lista"}],
                "projects": [
                  {"name": "web", "dir": "web", "kind": "composer", "code_files": 4, "frameworks": ["laravel/framework"],
                   "dependencies": ["laravel/framework", "php"], "scripts": ["test"],
                   "detected_stacks": [{"name": "laravel", "confidence": 0.9, "signals": ["path:artisan"]}]},
                  {"name": "core", "dir": "packages/core", "kind": "cargo"}
                ]}"#,
        )
        .unwrap();
        assert!(crate::io::project_map::read_at(&model).is_err(), "the whole map refuses the broken column");
        let projects = read_projects(&model);
        assert_eq!(projects.len(), 2, "{projects:?}");
        let web = &projects[0];
        assert_eq!((web.name.as_str(), web.dir.as_str(), web.kind.as_str(), web.code_files), ("web", "web", "composer", 4));
        assert_eq!(web.frameworks, ["laravel/framework"]);
        assert_eq!(web.scripts, ["test"]);
        assert_eq!(web.detected_stacks.len(), 1);
        assert_eq!(web.detected_stacks[0].signals, ["path:artisan"]);
        assert!(!web.own_git_root);
        assert_eq!((projects[1].name.as_str(), projects[1].code_files), ("core", 0));
        assert!(projects[1].frameworks.is_empty() && projects[1].detected_stacks.is_empty());
    }
}
