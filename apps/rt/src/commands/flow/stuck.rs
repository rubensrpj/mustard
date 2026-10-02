//! `stuck` — os processos que um agente deixou presos, sem ninguém mais
//! olhando para eles: um laço de espera ([`crate::hooks::bash::waiting`],
//! reaproveitado aqui pelo texto do comando em vez do proposto pelo Bash), um
//! comando cujo diretório de trabalho é uma cópia de onda já apagada, ou um
//! comando parado numa vaga sem onda em andamento — a vaga fica no disco de
//! uma onda para a seguinte, e o que a onda anterior deixou rodando nela não
//! tem mais dono. Uma leitura só os acha e encerra cada um, chamada no início
//! da sessão, em cada rodada e no fechamento; a resposta de cada uma diz
//! quais encerrou.
//!
//! A leitura só roda com a trava do passo do git presa: é com ela que a
//! rodada e o fechamento preparam uma vaga e gravam o envio de quem vai
//! trabalhar nela. Presa a trava, a vaga que alguém preparou já tem o envio
//! gravado e conta como vaga com trabalho; sem ela, o git que outra rodada
//! roda dentro de uma vaga ainda sem envio pareceria um comando esquecido e
//! seria encerrado no meio da cópia.
//!
//! A leitura é a lista de processos do sistema operacional — `/proc`, só no
//! Linux —, restrita aos do mesmo usuário [`current_uid`]. Num sistema sem
//! essa lista (`/proc` ausente, ou fora do Linux) a função não acha nada e
//! não falha: a resposta segue como se não houvesse processo nenhum.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::spec_events as store;
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
/// nível precisa: o comando (para reconhecer o laço; vazio enquanto o sistema
/// ainda não o montou) e o diretório de trabalho (para reconhecer a cópia
/// apagada e a vaga sem onda).
pub(crate) struct Snapshot {
    pid: u32,
    argv: Vec<String>,
    cwd: Option<std::path::PathBuf>,
}

/// Acha os processos presos do usuário atual e encerra cada um, com o sinal
/// de término comum (`SIGTERM`). Só toca o que casa uma das razões; todo
/// outro processo, inclusive este mesmo, fica como está. As cópias das ondas
/// moram na pasta das cópias do projeto, fora dele
/// ([`mustard_core::io::wave_prompt::copies_dir`]); ela é lida já resolvida
/// quando existe, porque a pasta de trabalho que o sistema mostra de cada
/// processo também vem resolvida. Roda com a trava do passo do git presa
/// (`_held`, de [`crate::commands::git_settle::git_step_lock`]): nenhuma
/// rodada está no meio de preparar uma vaga sem o envio gravado.
pub(crate) fn end_stuck_processes(root: &Path, _held: &LockedFile) -> Vec<Ended> {
    let copies = mustard_core::io::wave_prompt::copies_dir(root);
    let copies = std::fs::canonicalize(&copies).unwrap_or(copies);
    let busy = busy_slots(root, &copies);
    // A pasta de trabalho que o sistema mostra vem resolvida; o projeto,
    // alcançado por um link, precisa ser comparado pelo caminho real.
    let real_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    system_processes()
        .into_iter()
        .filter(|proc| proc.pid != std::process::id())
        .filter_map(|proc| reason_of(&proc, &real_root, &copies, &busy).map(|reason| (proc.pid, reason)))
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
/// roda é um laço de espera com a pasta de trabalho dentro de `root` ou das
/// cópias dele (`copies`) — o que deixa de fora o mesmo laço rodando num
/// outro projeto do usuário —; o diretório de trabalho dele fica na pasta
/// das cópias do projeto e já não existe no disco — o kernel, no Linux,
/// mantém o link de `cwd` apontando para o caminho apagado, às vezes com
/// ` (deleted)` no fim —; ou o diretório de trabalho fica numa vaga viva que
/// não está em `busy`, as vagas com trabalho de cada spec.
fn reason_of(
    proc: &Snapshot,
    root: &Path,
    copies: &Path,
    busy: &BTreeMap<String, BTreeSet<String>>,
) -> Option<&'static str> {
    if let Some(cwd) = proc.cwd.as_ref()
        && (cwd.starts_with(root) || cwd.starts_with(copies))
        && let Some(script) = shell_script(&proc.argv)
        && is_a_waiting_loop(script)
    {
        return Some("waiting_loop");
    }
    let cwd = proc.cwd.as_ref()?;
    let shown = cwd.to_string_lossy();
    let clean = Path::new(shown.strip_suffix(" (deleted)").unwrap_or(&shown));
    if clean.starts_with(copies) && !clean.exists() {
        return Some("deleted_copy");
    }
    let (spec, slot) = crate::hooks::observe::wave_alive_observer::slot_of_copy(copies, clean)?;
    let held = busy.get(&spec).is_some_and(|slots| slots.contains(&slot));
    (!held && is_slot_folder(copies, &spec, &slot)).then_some("idle_copy")
}

/// `<copies>/<spec>/<slot>` é uma vaga viva: a pasta da spec não é ela mesma
/// uma cópia do git — o endereço antigo, uma cópia por onda direto na pasta
/// das cópias, nunca conta como vaga — e a da vaga é, com o `.git` em
/// arquivo, como o git grava a cópia ligada ao projeto.
fn is_slot_folder(copies: &Path, spec: &str, slot: &str) -> bool {
    let spec_dir = copies.join(spec);
    !spec_dir.join(".git").exists() && spec_dir.join(slot).join(".git").is_file()
}

/// As vagas com trabalho de cada spec que tem pasta sob `copies`, pelo nome:
/// as presas da spec, pela mesma conta do despacho
/// ([`crate::commands::flow::round::held_slots`]) — o envio aberto de onda e
/// a revisão final aberta. A spec sem registro no projeto não tem vaga com
/// trabalho.
fn busy_slots(root: &Path, copies: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let Ok(entries) = std::fs::read_dir(copies) else { return BTreeMap::new() };
    let name = |copy: &str| Path::new(copy).file_name().map(|name| name.to_string_lossy().into_owned());
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .map(|spec| {
            let log = store::spec_file(root, &spec).ok().and_then(|path| store::read(&path).ok().flatten());
            let held = log
                .map(|log| crate::commands::flow::round::held_slots(root, &spec, &log))
                .unwrap_or_default()
                .iter()
                .filter_map(|copy| name(copy))
                .collect();
            (spec, held)
        })
        .collect()
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
    let Some(uid) = current_uid() else { return Vec::new() };
    processes_in(Path::new("/proc"), uid)
}

/// Os processos de `uid` listados em `proc_dir`, com a pasta de trabalho de
/// cada um. A linha de comando pode vir vazia: o processo que acabou de
/// nascer, ou que ainda está trocando de programa, não a tem montada por
/// alguns instantes, e a pasta de trabalho dele já vale — um processo preso
/// numa cópia apagada o está qualquer que seja o comando. Só o laço de espera
/// precisa do comando, e ele não casa sem.
#[cfg(target_os = "linux")]
fn processes_in(proc_dir: &Path, uid: u32) -> Vec<Snapshot> {
    use std::os::unix::fs::MetadataExt;

    let Ok(entries) = std::fs::read_dir(proc_dir) else { return Vec::new() };
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

/// O pai (`ppid`) e a hora de início (`starttime`, em tiques de relógio desde
/// o boot, campo 22 de `/proc/<pid>/stat`) de `pid`. A hora de início é o que
/// faz um número de processo reaproveitado pelo kernel não enganar: dois
/// processos diferentes têm o mesmo `pid` só em momentos diferentes, nunca a
/// mesma hora de início. `None` quando `pid` já sumiu ou o arquivo não bate
/// com o formato esperado.
#[cfg(target_os = "linux")]
fn stat_of(pid: u32) -> Option<(u32, u64)> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // O nome do comando, entre parênteses, pode ter espaço ou parêntese
    // dentro; os campos de verdade só começam depois do último `)`, com o
    // estado (`state`) primeiro, então o pai.
    let after = raw.rsplit_once(')')?.1;
    let fields: Vec<&str> = after.split_whitespace().collect();
    let ppid: u32 = fields.get(1)?.parse().ok()?;
    let starttime: u64 = fields.get(19)?.parse().ok()?;
    Some((ppid, starttime))
}

/// O processo que responde por este envio, com o número e a hora de início —
/// o par que [`process_alive`] confere depois. Sobe pelos pais em `/proc` até
/// achar um de nome `claude` e devolve esse, com um limite de 64 subidas para
/// nunca entrar em laço. Sem nenhum pai `claude` — uma rodada tocada por um
/// script, por um teste ou pela linha de comando —, devolve o processo atual,
/// que é quem manda a onda ali. O envio nunca sai sem o par: quem lê depois
/// compara um processo com o relógio, em vez de ler ausência de dado como
/// prova de morte.
#[cfg(target_os = "linux")]
pub(crate) fn sender_process() -> (u32, u64) {
    let mut pid = std::process::id();
    for _ in 0..64 {
        let Some((ppid, starttime)) = stat_of(pid) else { break };
        if std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|comm| comm.trim() == "claude") {
            return (pid, starttime);
        }
        if ppid == 0 || ppid == pid {
            break;
        }
        pid = ppid;
    }
    this_process()
}

/// Fora do Linux não há `/proc`: o envio leva o processo atual, e lá
/// [`process_alive`] não confere nenhum dos dois números.
#[cfg(not(target_os = "linux"))]
pub(crate) fn sender_process() -> (u32, u64) {
    this_process()
}

/// O número e a hora de início do processo atual — o par de quem está
/// rodando agora. Serve ao envio sem Claude Code por cima e aos testes, que
/// assim não dependem de quem lançou a suíte.
#[cfg(target_os = "linux")]
pub(crate) fn this_process() -> (u32, u64) {
    let pid = std::process::id();
    (pid, stat_of(pid).map_or(0, |(_, starttime)| starttime))
}

/// Fora do Linux, sem `/proc`, a hora de início não é legível: vai zerada, e
/// [`process_alive`] não olha para ela.
#[cfg(not(target_os = "linux"))]
pub(crate) fn this_process() -> (u32, u64) {
    (std::process::id(), 0)
}

/// `true` quando o processo `pid`, nascido em `started` (a mesma hora de
/// início que `/proc` contava então), ainda está vivo: um `pid` que o kernel
/// deu a outro processo depois mostra outra hora de início, e conta como
/// morto.
#[cfg(target_os = "linux")]
pub(crate) fn process_alive(pid: u32, started: u64) -> bool {
    stat_of(pid).is_some_and(|(_, starttime)| starttime == started)
}

/// Fora do Linux, sem `/proc`, nada é dado como morto: quem decide é a pausa
/// que o orquestrador manda, não uma leitura que este sistema não tem.
#[cfg(not(target_os = "linux"))]
pub(crate) fn process_alive(_pid: u32, _started: u64) -> bool {
    true
}

/// Espera até 2s `/proc/<pid>/cmdline` mostrar `needle`. O processo que um
/// teste acabou de criar volta do `spawn` antes de o kernel terminar de
/// montar a linha de comando dele: por alguns instantes ela vem vazia — ou
/// ainda é a do pai —, e a leitura dos processos do sistema ([`system_processes`])
/// ignora o processo sem linha de comando. É nessa janela, que o relógio de
/// uma máquina ocupada alarga, que um teste que procura o processo logo em
/// seguida não o acha. Todo teste que cria um processo para a busca de
/// processo preso espera por aqui antes de procurar.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn wait_until_spawned(pid: u32, needle: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline"))
            && String::from_utf8_lossy(&raw).contains(needle)
        {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "pid {pid} never showed its own command line");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
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

    /// Os dois casos que ficam presos — o laço de espera com a pasta de
    /// trabalho dentro do projeto e o comando na cópia apagada — são
    /// encontrados, encerrados e citados na resposta; um `sleep` comum e um
    /// laço de espera com a pasta de trabalho fora do projeto, como o de
    /// outro projeto do mesmo usuário, nunca são tocados.
    #[test]
    fn the_stuck_processes_are_ended_and_reported() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        crate::commands::flow::round::copies_leave_with_the_test(root);
        let copy = mustard_core::io::wave_prompt::slot_path(root, "x", 0);
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

        let held = crate::commands::git_settle::git_step_lock(root).expect("git step lock");
        let ended = end_stuck_processes(root, &held);
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

    /// O projeto alcançado por um link (a pasta do usuário que é um link, o
    /// temporário do Mac) é o mesmo projeto: o laço de espera que roda dentro
    /// dele aparece com a pasta de trabalho já resolvida pelo sistema, e é
    /// encerrado do mesmo jeito; o laço de outra pasta continua de fora.
    #[test]
    fn a_waiting_loop_in_a_project_reached_through_a_link_is_stuck() {
        let dir = tempdir().expect("tempdir");
        let real = std::fs::canonicalize(dir.path()).expect("the real project folder");
        let linked_in = tempdir().expect("tempdir for the link");
        let linked = linked_in.path().join("project-link");
        std::os::unix::fs::symlink(&real, &linked).expect("link to the project");
        crate::commands::flow::round::copies_leave_with_the_test(&linked);
        let outside = tempdir().expect("tempdir for the other project");

        let mut inside = spawn(
            Command::new("sh")
                .arg("-c")
                .arg("while pgrep -f mustard-stuck-linked-marker >/dev/null 2>&1; do sleep 1; done")
                .current_dir(&real),
        );
        let mut elsewhere = spawn(
            Command::new("sh")
                .arg("-c")
                .arg("while pgrep -f mustard-stuck-linked-elsewhere-marker >/dev/null 2>&1; do sleep 1; done")
                .current_dir(outside.path()),
        );
        wait_until_spawned(inside.id(), "mustard-stuck-linked-marker");
        wait_until_spawned(elsewhere.id(), "mustard-stuck-linked-elsewhere-marker");

        let held = crate::commands::git_settle::git_step_lock(&linked).expect("git step lock");
        let ended = end_stuck_processes(&linked, &held);
        assert!(gone(&mut inside), "the waiting loop inside the linked project must be ended");
        assert!(elsewhere.try_wait().ok().flatten().is_none(), "a waiting loop outside the project is left alone");
        let reasons: Vec<(u32, &str)> = ended.iter().map(|e| (e.pid, e.reason)).collect();
        assert!(reasons.contains(&(inside.id(), "waiting_loop")), "{reasons:?}");
        assert!(!reasons.iter().any(|(pid, _)| *pid == elsewhere.id()), "{reasons:?}");

        let _ = elsewhere.kill();
        let _ = elsewhere.wait();
    }

    /// A cópia da onda mora fora da pasta do projeto, na pasta das cópias
    /// dele. O processo que roda nela depois de ela ser apagada aparece como
    /// preso, pela cópia apagada, e é encerrado; o laço de espera numa cópia
    /// viva dali também, mesmo com a pasta de trabalho fora do projeto. O
    /// processo numa pasta apagada de mesmo nome sob as cópias de outro
    /// projeto fica de fora.
    #[test]
    fn a_process_in_a_deleted_outside_copy_is_stuck() {
        use mustard_core::io::wave_prompt::slot_path;

        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        let other = tempdir().expect("tempdir for the other project");
        for project in [root, other.path()] {
            crate::commands::flow::round::copies_leave_with_the_test(project);
        }
        let copy = slot_path(root, "x", 0);
        let live = slot_path(root, "x", 1);
        let foreign = slot_path(other.path(), "x", 0);
        let project = std::fs::canonicalize(root).expect("the project folder");
        assert!(!copy.starts_with(root) && !copy.starts_with(&project), "the copy lives outside the project: {copy:?}");
        for folder in [&copy, &live, &foreign] {
            std::fs::create_dir_all(folder).expect("copy dir");
        }

        let mut orphaned = spawn(Command::new("sleep").arg("30").current_dir(&copy));
        let mut looping = spawn(
            Command::new("sh")
                .arg("-c")
                .arg("while pgrep -f mustard-stuck-outside-copy-marker >/dev/null 2>&1; do sleep 1; done")
                .current_dir(&live),
        );
        let mut foreign_orphan = spawn(Command::new("sleep").arg("31").current_dir(&foreign));
        wait_until_spawned(orphaned.id(), "sleep");
        wait_until_spawned(looping.id(), "mustard-stuck-outside-copy-marker");
        wait_until_spawned(foreign_orphan.id(), "sleep");
        std::fs::remove_dir_all(&copy).expect("remove the wave's copy");
        std::fs::remove_dir_all(&foreign).expect("remove the other project's copy");

        let held = crate::commands::git_settle::git_step_lock(root).expect("git step lock");
        let ended = end_stuck_processes(root, &held);
        assert!(gone(&mut orphaned), "the command in the deleted outside copy must be ended");
        assert!(gone(&mut looping), "the waiting loop in a live outside copy must be ended");
        assert!(foreign_orphan.try_wait().ok().flatten().is_none(), "another project's copy is left alone");
        let reasons: Vec<(u32, &str)> = ended.iter().map(|e| (e.pid, e.reason)).collect();
        assert!(reasons.contains(&(orphaned.id(), "deleted_copy")), "{reasons:?}");
        assert!(reasons.contains(&(looping.id(), "waiting_loop")), "{reasons:?}");
        assert!(!reasons.iter().any(|(pid, _)| *pid == foreign_orphan.id()), "{reasons:?}");

        let _ = foreign_orphan.kill();
        let _ = foreign_orphan.wait();
    }

    /// A spec `x` com o plano de duas ondas: a onda 1 saiu na vaga a e foi
    /// entregue; depois dela, as linhas `after`. Linhas escritas direto no
    /// arquivo de eventos, como a rodada as deixa, numeradas na ordem.
    fn spec_with_wave_one_delivered(root: &Path, after: &[serde_json::Value]) {
        use mustard_core::io::wave_prompt::{shown, slot_path};
        let folder = root.join(".claude").join("spec").join("x");
        std::fs::create_dir_all(&folder).expect("spec folder");
        let at = "2026-09-25T10:00:00-03:00";
        let lines = [
            serde_json::json!({"type": "message", "author": "user", "text": "o objetivo"}),
            serde_json::json!({"type": "wave", "author": "assistant", "n": 1,
                "text": "Onda 1.", "done_when": "A suíte passa.", "origin": 1}),
            serde_json::json!({"type": "wave", "author": "assistant", "n": 2,
                "text": "Onda 2.", "done_when": "A suíte passa.", "origin": 1}),
            serde_json::json!({"type": "send", "author": "binary", "wave": 1,
                "role": "wave", "text": "pedido", "copy": shown(&slot_path(root, "x", 0))}),
            serde_json::json!({"type": "delivered", "author": "assistant", "wave": 1, "text": "entregue"}),
        ];
        let text: String = lines
            .iter()
            .chain(after)
            .zip(1_u64..)
            .map(|(line, id)| {
                let mut line = line.clone();
                line["v"] = serde_json::json!(1);
                line["id"] = serde_json::json!(id);
                line["at"] = serde_json::json!(at);
                line.to_string() + "\n"
            })
            .collect();
        std::fs::write(folder.join("spec.ndjson"), text).expect("spec file");
    }

    /// A spec `x` com o pedido aberto da onda 2, gravado na vaga b; a onda 1
    /// já foi entregue.
    fn spec_with_wave_two_running(root: &Path) {
        use mustard_core::io::wave_prompt::{shown, slot_path};
        spec_with_wave_one_delivered(
            root,
            &[serde_json::json!({"type": "send", "author": "binary", "wave": 2, "role": "wave", "text": "pedido",
                "copy": shown(&slot_path(root, "x", 1))})],
        );
    }

    /// As vagas `slots` da pasta das cópias, cada uma com uma pasta de código
    /// e o `.git` em arquivo, como a cópia ligada ao projeto.
    fn live_slots(slots: &[&Path]) {
        for copy in slots {
            std::fs::create_dir_all(copy.join("src")).expect("copy dir");
            std::fs::write(copy.join(".git"), "gitdir: /nowhere\n").expect("the copy's git link");
        }
    }

    /// A vaga fica no disco de uma onda para a seguinte. O processo parado
    /// numa vaga sem onda em andamento — a da onda já entregue — é
    /// encerrado, pela vaga sem onda; o da vaga com o pedido aberto de outra
    /// onda fica. A cópia no endereço antigo, uma por onda direto na pasta
    /// das cópias, não é vaga e também fica.
    #[test]
    fn a_process_in_a_slot_with_no_wave_running_is_stuck() {
        use mustard_core::io::wave_prompt::{copies_dir, slot_path};

        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        crate::commands::flow::round::copies_leave_with_the_test(root);
        spec_with_wave_two_running(root);
        let free = slot_path(root, "x", 0);
        let held = slot_path(root, "x", 1);
        let old = copies_dir(root).join("x-3");
        live_slots(&[&free, &held, &old]);

        let mut idle = spawn(Command::new("sleep").arg("32").current_dir(free.join("src")));
        let mut working = spawn(Command::new("sleep").arg("33").current_dir(held.join("src")));
        let mut legacy = spawn(Command::new("sleep").arg("34").current_dir(old.join("src")));
        wait_until_spawned(idle.id(), "sleep");
        wait_until_spawned(working.id(), "sleep");
        wait_until_spawned(legacy.id(), "sleep");

        let held = crate::commands::git_settle::git_step_lock(root).expect("git step lock");
        let ended = end_stuck_processes(root, &held);
        assert!(gone(&mut idle), "the command left in the slot with no wave running must be ended");
        assert!(working.try_wait().ok().flatten().is_none(), "the slot of the running wave is left alone");
        assert!(legacy.try_wait().ok().flatten().is_none(), "a copy at the old address is not a slot");
        let reasons: Vec<(u32, &str)> = ended.iter().map(|e| (e.pid, e.reason)).collect();
        assert!(reasons.contains(&(idle.id(), "idle_copy")), "{reasons:?}");
        assert!(!reasons.iter().any(|(pid, _)| *pid == working.id() || *pid == legacy.id()), "{reasons:?}");
        let line = report_line(&ended, Locale::PtBr).expect("a line for the ended process");
        assert!(line.contains("sem onda"), "{line}");

        for child in [&mut working, &mut legacy] {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// A vaga da revisão final aberta tem trabalho, como a de uma onda em
    /// andamento: o processo do revisor, parado na vaga da última onda, fica;
    /// o da vaga sem onda nem revisão é encerrado.
    #[test]
    fn a_process_in_the_slot_of_the_open_final_review_is_left_alone() {
        use mustard_core::io::wave_prompt::slot_path;

        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        crate::commands::flow::round::copies_leave_with_the_test(root);
        spec_with_wave_one_delivered(
            root,
            &[serde_json::json!({"type": "send", "author": "binary", "role": "review", "text": "revise"})],
        );
        let review = slot_path(root, "x", 0);
        let free = slot_path(root, "x", 1);
        live_slots(&[&review, &free]);

        let mut reviewing = spawn(Command::new("sleep").arg("35").current_dir(review.join("src")));
        let mut idle = spawn(Command::new("sleep").arg("36").current_dir(free.join("src")));
        wait_until_spawned(reviewing.id(), "sleep");
        wait_until_spawned(idle.id(), "sleep");

        let held = crate::commands::git_settle::git_step_lock(root).expect("git step lock");
        let ended = end_stuck_processes(root, &held);
        assert!(gone(&mut idle), "the command left in the slot with no work must be ended");
        assert!(reviewing.try_wait().ok().flatten().is_none(), "the slot of the open final review is left alone");
        let reasons: Vec<(u32, &str)> = ended.iter().map(|e| (e.pid, e.reason)).collect();
        assert!(reasons.contains(&(idle.id(), "idle_copy")), "{reasons:?}");
        assert!(!reasons.iter().any(|(pid, _)| *pid == reviewing.id()), "{reasons:?}");

        let _ = reviewing.kill();
        let _ = reviewing.wait();
    }

    /// O processo que acabou de nascer ainda não tem a linha de comando
    /// montada — ela vem vazia por alguns instantes —, e nem por isso some da
    /// lista: com a pasta de trabalho numa cópia já apagada ele está preso, e
    /// a busca o acha. A pasta `/proc` aqui é de mentira, com o mesmo formato
    /// da de verdade: uma pasta por processo, `cmdline` e o link `cwd`; o link
    /// aponta para o caminho apagado com o ` (deleted)` que o kernel põe.
    #[test]
    fn a_process_with_no_command_line_yet_is_listed_and_stuck_in_a_deleted_copy() {
        use mustard_core::io::wave_prompt::{copies_dir, slot_path};
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        crate::commands::flow::round::copies_leave_with_the_test(root);
        let copies = copies_dir(root);
        let gone = slot_path(root, "x", 0);
        let proc_dir = tempdir().expect("tempdir for the fake process list");
        let born = proc_dir.path().join("4242");
        std::fs::create_dir_all(&born).expect("process folder");
        std::fs::write(born.join("cmdline"), b"").expect("empty command line");
        symlink(format!("{} (deleted)", gone.display()), born.join("cwd")).expect("cwd link");

        let uid = current_uid().expect("who runs");
        let listed = processes_in(proc_dir.path(), uid);
        let found: Vec<(u32, usize)> = listed.iter().map(|p| (p.pid, p.argv.len())).collect();
        assert_eq!(found, vec![(4242, 0)], "the process with no command line is still listed: {found:?}");
        assert_eq!(listed[0].cwd.as_deref(), Some(Path::new(&format!("{} (deleted)", gone.display()))));
        assert_eq!(reason_of(&listed[0], root, &copies, &BTreeMap::new()), Some("deleted_copy"));
    }

    /// Sem processo nenhum encerrado, a linha da resposta não existe: o
    /// chamador não acrescenta nada.
    #[test]
    fn no_ended_process_means_no_report_line() {
        assert!(report_line(&[], Locale::PtBr).is_none());
    }

    /// Um processo vivo, com a própria hora de início, está vivo; o mesmo
    /// número depois que o processo sai — a hora de início já não bate com
    /// nenhuma, porque `/proc/<pid>` sumiu — conta como morto, mesmo com a
    /// hora antiga.
    #[test]
    fn a_live_process_is_alive_and_a_gone_one_with_its_old_start_time_is_not() {
        let (ppid, started) = stat_of(std::process::id()).expect("read our own /proc/self/stat");
        assert!(process_alive(std::process::id(), started), "the running test process is alive");
        assert_ne!(ppid, 0, "the test process has a parent");

        let mut child = spawn(&mut Command::new("true"));
        let pid = child.id();
        let (_, child_started) = stat_of(pid).expect("read the child's stat before it exits");
        assert!(gone(&mut child), "the fixture child must exit");
        let _ = child.wait();
        assert!(!process_alive(pid, child_started), "an exited process is never alive again");
    }

    /// Subindo dos pais do processo atual, o achado é o mesmo dono do
    /// processo atual: a hora de início de um processo que ainda está vivo
    /// não muda entre duas leituras.
    #[test]
    fn stat_of_reports_a_stable_start_time_for_the_running_process() {
        let pid = std::process::id();
        let (_, first) = stat_of(pid).expect("first read");
        let (_, second) = stat_of(pid).expect("second read");
        assert_eq!(first, second, "the same live process keeps the same start time");
    }

    /// Sem achar nenhum pai de nome `claude`, ou achando um, nunca entra em
    /// laço e sempre devolve um processo vivo: com pai `claude`, ele; sem
    /// nenhum, o processo atual. Vale rodando por baixo do Claude Code e
    /// rodando solto, e é o que faz a suíte não mudar de resultado conforme
    /// quem a lançou.
    #[test]
    fn the_sender_process_is_always_a_living_one() {
        let (pid, started) = sender_process();
        assert!(process_alive(pid, started), "the sender process must still be running");
        let (own_pid, own_started) = this_process();
        assert_eq!(own_pid, std::process::id(), "this_process reports the running process");
        assert!(process_alive(own_pid, own_started), "the running process is alive");
    }
}
