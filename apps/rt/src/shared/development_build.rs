//! `development_build` — o programa compilado da branch do Mustard, o que a
//! sessão do trabalho no próprio Mustard roda no lugar do instalado.
//!
//! O programa nasce numa pasta fora do repositório e fora das cópias
//! ([`mustard_core::io::wave_prompt::development_build_dir`]), onde nem o
//! fechamento da spec nem a limpeza das cópias o apagam. Este módulo diz se ele
//! está em dia com o commit da branch ([`gap`]) e o compila: em primeiro plano,
//! pelo comando de [`build_command`], quando quem chama espera o resultado (a
//! rodada), e em segundo plano, com uma trava de arquivo, quando não espera
//! (o início da sessão, [`start_in_background`]).
//!
//! Nada aqui instala nada: o programa fica na pasta de compilação e a
//! passagem da chamada a ele é de `mustard_core::development_rt`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mustard_core::io::wave_prompt::development_build_dir;

/// Os pacotes que a compilação gera: o `mustard-rt`, o `scan` e o `mustard`.
const PACKAGES: [&str; 3] = ["mustard-rt", "scan", "mustard-cli"];

/// Quanto tempo a trava de uma compilação em segundo plano vale: uma compilação
/// completa do zero cabe folgada em uma hora, e a trava mais velha que isso é
/// de uma compilação que morreu sem soltá-la.
const BUILD_VALIDITY: Duration = Duration::from_secs(60 * 60);

/// Quanto o `--version` do programa compilado pode levar antes de valer como
/// "não abre".
const VERSION_DEADLINE: Duration = Duration::from_secs(5);

/// A palavra entre aspas simples para o shell, com a aspa que ela mesma tenha
/// escapada.
fn single_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// O comando que compila os três programas da branch na pasta `target`, a
/// rodar na pasta do checkout principal.
#[must_use]
pub(crate) fn build_command(target: &Path) -> String {
    let packages: Vec<String> = PACKAGES.iter().map(|package| format!("-p {package}")).collect();
    format!("cargo build --release --locked {} --target-dir {}", packages.join(" "), single_quoted(&target.display().to_string()))
}

/// O programa compilado da pasta `target`, esteja ele em disco ou não.
fn program_in(target: &Path) -> PathBuf {
    target.join("release").join(if cfg!(windows) { "mustard-rt.exe" } else { "mustard-rt" })
}

/// O que separa o programa compilado do commit da branch.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Gap {
    /// O commit em que o programa compilado foi feito, ou o texto da versão
    /// dele quando ele não carimba commit; `None` quando não há programa
    /// compilado que abra.
    pub compiled: Option<String>,
    /// O commit atual do checkout principal, em 12 dígitos.
    pub head: String,
}

/// O que falta ao programa compilado do checkout principal `main` para estar
/// no commit atual dele, ou `None` quando ele já está — ou quando o commit
/// atual não se lê, e então nada se prova.
#[must_use]
pub(crate) fn gap(main: &Path) -> Option<Gap> {
    gap_in(main, &development_build_dir(main))
}

/// [`gap`] com a pasta do programa compilado recebida, que é como um teste o
/// põe onde quer.
fn gap_in(main: &Path, target: &Path) -> Option<Gap> {
    let head = head_of(main)?;
    let short = head.chars().take(12).collect::<String>();
    let program = program_in(target);
    let Some(version) = program.is_file().then(|| version_of(&program)).flatten() else {
        return Some(Gap { compiled: None, head: short });
    };
    match stamped_commit(&version) {
        Some(commit) if head.starts_with(commit) => None,
        Some(commit) => Some(Gap { compiled: Some(commit.to_string()), head: short }),
        None => Some(Gap { compiled: Some(version), head: short }),
    }
}

/// O commit atual do checkout `main`, inteiro.
fn head_of(main: &Path) -> Option<String> {
    let head = mustard_core::platform::git::run(main, &["rev-parse", "HEAD"]).out()?;
    (head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit())).then_some(head)
}

/// O commit que o carimbo de versão cita — `<versão> (build N, g<commit>[-dirty]
/// <data>)` —, sem o `-dirty`; `None` no carimbo de uma compilação sem git.
fn stamped_commit(version: &str) -> Option<&str> {
    let word = version.split_once(", g")?.1.split_whitespace().next()?.trim_end_matches(')');
    let commit = word.strip_suffix("-dirty").unwrap_or(word);
    (commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit())).then_some(commit)
}

/// O texto que `<programa> --version` imprime, ou `None` quando o programa não
/// abre, não termina no prazo ou não imprime nada. Roda com a guarda de um
/// salto ligada: sem ela, a passagem da chamada a um plugin mais novo
/// responderia no lugar do programa, e a versão lida seria a dele.
fn version_of(program: &Path) -> Option<String> {
    let mut child = std::process::Command::new(program)
        .arg("--version")
        .env("MUSTARD_RT_DELEGATED", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if started.elapsed() >= VERSION_DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let out = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// O que aconteceu com o pedido de compilar em segundo plano.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Launch {
    /// A compilação foi solta agora.
    Started,
    /// Outra compilação já está em andamento: a trava dela vale.
    Running,
    /// A compilação não pôde ser iniciada.
    Failed,
}

/// Solta a compilação do checkout principal `main` em segundo plano, sem
/// esperar, e diz se soltou. Com uma trava ainda válida de outra compilação,
/// não solta uma segunda: duas juntas disputariam a mesma pasta.
#[must_use]
pub(crate) fn start_in_background(main: &Path) -> Launch {
    start_in_background_with(main, &development_build_dir(main), &|command, cwd| {
        crate::shared::proc::spawn_detached_shell(command, cwd)
    })
}

/// [`start_in_background`] com a pasta do programa e o disparo recebidos, que é
/// como um teste prova a trava sem compilar nada.
fn start_in_background_with(
    main: &Path,
    target: &Path,
    spawn: &dyn Fn(&str, &Path) -> std::io::Result<()>,
) -> Launch {
    let beside = |suffix: &str| {
        let mut name = target.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
        name.push(suffix);
        target.with_file_name(name)
    };
    let (marker, log) = (beside(".building"), beside(".log"));
    match claim(&marker, BUILD_VALIDITY) {
        Ok(true) => {}
        Ok(false) => return Launch::Running,
        Err(_) => return Launch::Failed,
    }
    // O shell compila e, de qualquer jeito que ela termine, solta a trava; a
    // saída fica no arquivo ao lado, para quem quiser ver por que falhou.
    let command = format!(
        "{} > {} 2>&1; rm -f {}",
        build_command(target),
        single_quoted(&log.display().to_string()),
        single_quoted(&marker.display().to_string())
    );
    if spawn(&command, main).is_err() {
        let _ = std::fs::remove_file(&marker);
        return Launch::Failed;
    }
    Launch::Started
}

/// Toma a trava `marker`: um arquivo que só nasce se não existe. `Ok(false)`
/// quando outra compilação a tem e ela ainda vale; a mais velha que `validity`
/// é de uma compilação que morreu e é refeita.
fn claim(marker: &Path, validity: Duration) -> std::io::Result<bool> {
    if let Some(folder) = marker.parent() {
        std::fs::create_dir_all(folder)?;
    }
    for _ in 0..2 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(marker) {
            Ok(_) => return Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let age = std::fs::metadata(marker).and_then(|meta| meta.modified()).ok().and_then(|at| at.elapsed().ok());
                if age.is_some_and(|age| age < validity) {
                    return Ok(false);
                }
                let _ = std::fs::remove_file(marker);
            }
            Err(err) => return Err(err),
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// O comando é o combinado, com a pasta como uma palavra só mesmo com
    /// espaço e aspa no caminho.
    #[test]
    fn the_build_command_builds_the_three_programs_into_the_folder() {
        assert_eq!(
            build_command(Path::new("/cache/build/mustard-1")),
            "cargo build --release --locked -p mustard-rt -p scan -p mustard-cli --target-dir '/cache/build/mustard-1'"
        );
        assert!(build_command(Path::new("/o dono's/build")).ends_with("--target-dir '/o dono'\\''s/build'"));
    }

    /// O commit sai do carimbo com ou sem `-dirty`, e o carimbo de uma
    /// compilação sem git não cita commit.
    #[test]
    fn the_commit_comes_out_of_the_version_stamp() {
        let stamp = |commit: &str| format!("mustard-rt 0.2.4 (build dev, g{commit} 2026-10-02)");
        assert_eq!(stamped_commit(&stamp("39e91178887f")), Some("39e91178887f"));
        assert_eq!(stamped_commit(&stamp("39e91178887f-dirty")), Some("39e91178887f"));
        assert_eq!(stamped_commit("mustard-rt 0.2.4"), None);
        assert_eq!(stamped_commit(&stamp("abc")), None, "poucos dígitos não são um commit");
    }

    /// A trava só deixa uma compilação soltar de cada vez: a segunda chamada
    /// não dispara nada, a trava velha demais é refeita, e a que falha ao
    /// disparar não deixa trava para trás.
    #[test]
    fn only_one_background_build_runs_at_a_time() {
        let place = tempfile::tempdir().unwrap();
        let target = place.path().join("build").join("mustard-1");
        let main = place.path().join("projeto");
        let calls: RefCell<Vec<(String, PathBuf)>> = RefCell::new(Vec::new());
        let fake = |command: &str, cwd: &Path| -> std::io::Result<()> {
            calls.borrow_mut().push((command.to_string(), cwd.to_path_buf()));
            Ok(())
        };

        assert_eq!(start_in_background_with(&main, &target, &fake), Launch::Started);
        {
            let calls = calls.borrow();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].1, main, "compila no checkout principal");
            assert!(calls[0].0.starts_with(&build_command(&target)), "{}", calls[0].0);
            assert!(calls[0].0.contains("rm -f "), "a trava sai quando a compilação termina: {}", calls[0].0);
        }
        let marker = place.path().join("build").join("mustard-1.building");
        assert!(marker.is_file());

        assert_eq!(start_in_background_with(&main, &target, &fake), Launch::Running, "a trava vale");
        assert_eq!(calls.borrow().len(), 1, "a segunda compilação não é solta");

        let long_ago = std::time::SystemTime::now() - BUILD_VALIDITY - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&marker).unwrap().set_modified(long_ago).unwrap();
        assert_eq!(start_in_background_with(&main, &target, &fake), Launch::Started, "a trava de uma compilação morta é refeita");
        assert_eq!(calls.borrow().len(), 2);

        std::fs::remove_file(&marker).unwrap();
        let broken = |_: &str, _: &Path| -> std::io::Result<()> { Err(std::io::Error::other("sem shell")) };
        assert_eq!(start_in_background_with(&main, &target, &broken), Launch::Failed);
        assert!(!marker.exists(), "o disparo que falhou não deixa a trava");
    }

    #[cfg(unix)]
    mod against_git {
        use super::*;

        /// Um repositório com um commit, e o commit dele inteiro.
        fn repo_with_a_commit(at: &Path) -> String {
            std::fs::create_dir_all(at).unwrap();
            let git = |args: &[&str]| {
                let out = std::process::Command::new("git").args(args).current_dir(at).output().unwrap();
                assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            };
            git(&["init", "-q"]);
            std::fs::write(at.join("a"), "a").unwrap();
            git(&["add", "-A"]);
            git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "um"]);
            git(&["rev-parse", "HEAD"])
        }

        /// O programa compilado de mentira, que imprime `says` ao `--version`.
        fn compiled(target: &Path, says: &str) {
            let program = program_in(target);
            std::fs::create_dir_all(program.parent().unwrap()).unwrap();
            crate::executable::write_executable(&program, &format!("#!/bin/sh\nprintf '%s\\n' '{says}'\n"));
        }

        /// O programa compilado falta, está em outro commit, abre sem carimbo
        /// ou não abre: em todos o que falta é dito; só o do commit atual, com
        /// ou sem `-dirty`, está em dia.
        #[test]
        fn the_compiled_program_is_behind_unless_it_carries_the_head_commit() {
            let place = tempfile::tempdir().unwrap();
            let main = place.path().join("projeto");
            let head = repo_with_a_commit(&main);
            let short = head[..12].to_string();
            let target = place.path().join("build");

            assert_eq!(gap_in(&main, &target), Some(Gap { compiled: None, head: short.clone() }), "sem programa");

            compiled(&target, "mustard-rt 0.2.4 (build dev, g0123456789ab 2026-10-02)");
            assert_eq!(gap_in(&main, &target), Some(Gap { compiled: Some("0123456789ab".into()), head: short.clone() }), "outro commit");

            compiled(&target, &format!("mustard-rt 0.2.4 (build dev, g{short} 2026-10-02)"));
            assert_eq!(gap_in(&main, &target), None, "no commit atual");
            compiled(&target, &format!("mustard-rt 0.2.4 (build dev, g{short}-dirty 2026-10-02)"));
            assert_eq!(gap_in(&main, &target), None, "no commit atual, com código por comitar");

            compiled(&target, "mustard-rt 0.2.4");
            assert_eq!(
                gap_in(&main, &target),
                Some(Gap { compiled: Some("mustard-rt 0.2.4".into()), head: short.clone() }),
                "sem carimbo de commit nada se prova"
            );

            crate::executable::write_executable(&program_in(&target), "#!/bin/sh\nexit 3\n");
            assert_eq!(gap_in(&main, &target), Some(Gap { compiled: None, head: short }), "o que não abre vale como faltando");

            let outside = place.path().join("fora");
            std::fs::create_dir_all(&outside).unwrap();
            assert_eq!(gap_in(&outside, &target), None, "sem o commit atual nada se prova");
        }
    }
}
