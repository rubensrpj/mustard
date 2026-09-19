//! `stuck` — os processos que um agente deixou presos, sem ninguém mais
//! olhando para eles: um laço de espera ([`crate::hooks::bash::waiting`],
//! reaproveitado aqui pelo texto do comando em vez do proposto pelo Bash) ou
//! um comando cujo diretório de trabalho é a cópia de uma onda que
//! [`super::round::commit::close_copies`] já apagou. Uma leitura só os acha
//! e encerra cada um, chamada no início da sessão, em cada rodada e no
//! fechamento; a resposta de cada uma diz quais encerrou.
//!
//! A leitura é a lista de processos do sistema operacional — `/proc`, só no
//! Linux —, restrita aos do mesmo usuário [`current_uid`]. Num sistema sem
//! essa lista (`/proc` ausente, ou fora do Linux) a função não acha nada e
//! não falha: a resposta segue como se não houvesse processo nenhum.

use std::path::Path;

use mustard_core::platform::i18n::{translate, Locale};

#[cfg(target_os = "linux")]
use crate::commands::maint::scratch_gc::current_uid;
use crate::hooks::bash::waiting::is_a_waiting_loop;

/// Um processo preso que foi encerrado.
pub(crate) struct Ended {
    pid: u32,
    /// A chave do motivo, em `stuck.reason.*`.
    reason: &'static str,
}

/// A leitura de um processo do sistema, restrita ao que a função de baixo
/// nível precisa: o comando (para reconhecer o laço) e o diretório de
/// trabalho (para reconhecer a cópia apagada).
pub(crate) struct Snapshot {
    pid: u32,
    argv: Vec<String>,
    cwd: Option<std::path::PathBuf>,
}

/// Acha os processos presos do usuário atual e encerra cada um, com o sinal
/// de término comum (`SIGTERM`). Só toca o que casa uma das duas razões; todo
/// outro processo, inclusive este mesmo, fica como está.
pub(crate) fn end_stuck_processes(root: &Path) -> Vec<Ended> {
    let worktrees = mustard_core::ClaudePaths::compose_unchecked(root).claude_dir().join("worktrees");
    system_processes()
        .into_iter()
        .filter(|proc| proc.pid != std::process::id())
        .filter_map(|proc| reason_of(&proc, root, &worktrees).map(|reason| (proc.pid, reason)))
        .filter(|(pid, _)| terminate(*pid))
        .map(|(pid, reason)| Ended { pid, reason })
        .collect()
}

/// A linha da resposta, no idioma `lang`, com o que foi encerrado — ou
/// `None` sem processo nenhum, para o chamador não acrescentar nada.
pub(crate) fn report_line(ended: &[Ended], lang: Locale) -> Option<String> {
    if ended.is_empty() {
        return None;
    }
    let items: Vec<String> = ended
        .iter()
        .map(|e| format!("{} ({})", e.pid, translate(&format!("stuck.reason.{}", e.reason), lang)))
        .collect();
    Some(translate("stuck.ended", lang).replace("{list}", &items.join(", ")))
}

/// Por que `proc` está preso, ou `None` quando não está: o comando que ele
/// roda é um laço de espera com a pasta de trabalho dentro de `root` — o que
/// cobre as cópias das ondas, e deixa de fora o mesmo laço rodando num outro
/// projeto do usuário —, ou o diretório de trabalho dele é uma cópia de onda
/// (`{worktrees}/mustard-<spec>-<onda>`) que já não existe no disco — o
/// kernel, no Linux, mantém o link de `cwd` apontando para o caminho apagado,
/// às vezes com ` (deleted)` no fim.
fn reason_of(proc: &Snapshot, root: &Path, worktrees: &Path) -> Option<&'static str> {
    if let Some(cwd) = proc.cwd.as_ref()
        && cwd.starts_with(root)
        && let Some(script) = shell_script(&proc.argv)
        && is_a_waiting_loop(script)
    {
        return Some("waiting_loop");
    }
    let cwd = proc.cwd.as_ref()?;
    let shown = cwd.to_string_lossy();
    let clean = Path::new(shown.strip_suffix(" (deleted)").unwrap_or(&shown));
    (clean.starts_with(worktrees) && !clean.exists()).then_some("deleted_copy")
}

/// O trecho de `argv` que um shell recebeu por `-c`: `["sh", "-c", "while …
/// done"]` devolve o roteiro inteiro, sem juntar as outras palavras — só ele
/// é o texto que o terminal de verdade leria. Outra forma de chamar (sem
/// `-c`) não tem roteiro para ler aqui.
fn shell_script(argv: &[String]) -> Option<&str> {
    let program = argv.first()?;
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    if matches!(name, "sh" | "bash" | "zsh" | "dash") && argv.get(1).map(String::as_str) == Some("-c") {
        return argv.get(2).map(String::as_str);
    }
    None
}

/// Manda o sinal de término comum (`SIGTERM`) a `pid`. Melhor esforço: `true`
/// só quando o sinal foi entregue, nunca quando o processo já sumiu ou o
/// sistema não tem o comando `kill`.
#[cfg(unix)]
fn terminate(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(unix))]
fn terminate(_pid: u32) -> bool {
    false
}

/// Os processos do usuário atual, lidos de `/proc`. `None` de [`current_uid`]
/// (sem saber quem roda), a pasta ausente (fora do Linux) ou uma entrada que
/// já sumiu ao ler viram "sem processo": nunca um erro.
#[cfg(target_os = "linux")]
pub(crate) fn system_processes() -> Vec<Snapshot> {
    use std::os::unix::fs::MetadataExt;

    let Some(uid) = current_uid() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let dir = entry.path();
            if std::fs::metadata(&dir).ok()?.uid() != uid {
                return None;
            }
            let raw = std::fs::read(dir.join("cmdline")).ok()?;
            let argv: Vec<String> = raw
                .split(|b| *b == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect();
            if argv.is_empty() {
                return None;
            }
            let cwd = std::fs::read_link(dir.join("cwd")).ok();
            Some(Snapshot { pid, argv, cwd })
        })
        .collect()
}

/// Fora do Linux não há `/proc`: nenhum processo é lido.
#[cfg(not(target_os = "linux"))]
pub(crate) fn system_processes() -> Vec<Snapshot> {
    Vec::new()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    fn spawn(cmd: &mut Command) -> Child {
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("spawn the fixture process")
    }

    /// Espera até 2s o processo sumir; `true` quando ele já não existe.
    fn gone(child: &mut Child) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// Espera até 2s `/proc/<pid>/cmdline` mostrar `needle`: logo após
    /// nascer, o filho ainda carrega a linha de comando do pai (o binário de
    /// teste), e ler antes disso é o que deixava o teste instável.
    fn wait_until_spawned(pid: u32, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline"))
                && String::from_utf8_lossy(&raw).contains(needle)
            {
                return;
            }
            assert!(Instant::now() < deadline, "pid {pid} never showed its own command line");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Os dois casos que ficam presos — o laço de espera com a pasta de
    /// trabalho dentro do projeto e o comando na cópia apagada — são
    /// encontrados, encerrados e citados na resposta; um `sleep` comum e um
    /// laço de espera com a pasta de trabalho fora do projeto, como o de
    /// outro projeto do mesmo usuário, nunca são tocados.
    #[test]
    fn the_stuck_processes_are_ended_and_reported() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let worktrees = mustard_core::ClaudePaths::compose_unchecked(root).claude_dir().join("worktrees");
        std::fs::create_dir_all(&worktrees).expect("worktrees dir");
        let copy = worktrees.join("mustard-x-1");
        std::fs::create_dir_all(&copy).expect("copy dir");
        let outside = tempdir().expect("tempdir for the other project");

        // O laço, dentro do projeto: um `pgrep` que casa a própria linha de
        // comando, então segue rodando até alguém o encerrar.
        let mut looping = spawn(
            Command::new("sh")
                .arg("-c")
                .arg("while pgrep -f mustard-stuck-test-marker >/dev/null 2>&1; do sleep 1; done")
                .current_dir(root),
        );
        // O mesmo laço, mas com a pasta de trabalho de outro projeto: fica
        // de fora, mesmo casando o texto do comando.
        let mut elsewhere_looping = spawn(
            Command::new("sh")
                .arg("-c")
                .arg("while pgrep -f mustard-stuck-test-elsewhere-marker >/dev/null 2>&1; do sleep 1; done")
                .current_dir(outside.path()),
        );
        // O comando cuja cópia some debaixo dele.
        let mut orphaned = spawn(Command::new("sleep").arg("30").current_dir(&copy));
        // Um `sleep` comum, sem laço e sem cópia apagada: fica de fora.
        let mut ordinary = spawn(Command::new("sleep").arg("30"));
        std::fs::remove_dir_all(&copy).expect("remove the wave's copy");

        wait_until_spawned(looping.id(), "mustard-stuck-test-marker");
        wait_until_spawned(elsewhere_looping.id(), "mustard-stuck-test-elsewhere-marker");
        wait_until_spawned(orphaned.id(), "sleep");
        wait_until_spawned(ordinary.id(), "sleep");

        let ended = end_stuck_processes(root);
        assert!(gone(&mut looping), "the waiting loop inside the project must be ended");
        assert!(gone(&mut orphaned), "the command in the deleted copy must be ended");
        assert!(
            elsewhere_looping.try_wait().ok().flatten().is_none(),
            "a waiting loop outside the project is left alone"
        );
        assert!(ordinary.try_wait().ok().flatten().is_none(), "an ordinary sleep is left alone");

        let pids: Vec<u32> = ended.iter().map(|e| e.pid).collect();
        assert!(pids.contains(&looping.id()), "{pids:?}");
        assert!(pids.contains(&orphaned.id()), "{pids:?}");
        assert!(!pids.contains(&elsewhere_looping.id()), "{pids:?}");
        assert!(!pids.contains(&ordinary.id()), "{pids:?}");

        let line = report_line(&ended, Locale::PtBr).expect("a line for two ended processes");
        assert!(line.contains(&looping.id().to_string()) && line.contains(&orphaned.id().to_string()), "{line}");

        let _ = elsewhere_looping.kill();
        let _ = elsewhere_looping.wait();
        let _ = ordinary.kill();
        let _ = ordinary.wait();
    }

    /// Sem processo nenhum encerrado, a linha da resposta não existe: o
    /// chamador não acrescenta nada.
    #[test]
    fn no_ended_process_means_no_report_line() {
        assert!(report_line(&[], Locale::PtBr).is_none());
    }
}
