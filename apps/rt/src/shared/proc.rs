//! `proc` — signal-free, cross-platform process primitives shared by both
//! the enforcement face (`hooks`) and the script face (`commands`).
//!
//! They live here rather than inside either face so neither has to depend on
//! the other — `shared` is the one module both may depend on, and it never
//! depends back.
//!
//! Every function is best-effort and fail-open: a missing tool on `PATH`, an
//! empty result, or a kill error degrades to an `eprintln!` warning and an empty
//! / `false` value. None of them panic. The crate forbids `unsafe`, so none of
//! these use raw OS signal APIs — they shell out to `netstat`/`lsof`/`taskkill`/
//! `kill`/`tasklist` instead, and to `taskset` for the CPU affinity of a
//! project command.
//!
//! [`run_shell_with_deadline`] additionally depends on [`crate::util::platform`]
//! for the platform shell. That is a sideways edge, not a layering inversion:
//! `util` is a leaf like `shared` and depends on neither face.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Per-user toolchain `bin` directories that every installer of that toolchain
/// creates, by its own documented convention.
///
/// ## Why Mustard resolves these itself
///
/// An acceptance criterion runs through a shell, and a NON-INTERACTIVE shell
/// reads a different (usually smaller) set of startup files than the terminal
/// the operator types in. So `cargo` can be perfectly installed, work in the
/// terminal, and still be invisible to the command the harness spawns. The
/// harness then collects exit 127 — `command not found` — and records the
/// criterion `unproven`, which a reader sees as a failing test and takes to the
/// code (field, 2026-08-28: a whole session was spent this way).
///
/// The fix must not be a line in one shell's profile. That repairs one machine
/// with one shell, and says nothing to bash, fish, a Windows host or a CI
/// container. Looking in the conventional locations is something Mustard can do
/// ITSELF, in-process, before it spawns anything — so it holds everywhere the
/// harness runs.
///
/// Only directories that EXIST are returned, and the caller APPENDS them, so a
/// toolchain the operator deliberately put on `PATH` always wins. Mustard
/// supplements the environment; it never overrides it.
fn toolchain_bin_dirs() -> Vec<PathBuf> {
    let Some(home) = mustard_core::platform::harness::home_dir() else {
        return Vec::new();
    };
    // Each entry is the location that toolchain's own installer documents.
    let mut candidates: Vec<PathBuf> = vec![
        home.join(".cargo").join("bin"),    // rustup
        home.join(".local").join("bin"),    // pip / pipx / uv
        home.join("go").join("bin"),        // go install
        home.join(".bun").join("bin"),      // bun
        home.join(".deno").join("bin"),     // deno
        home.join(".volta").join("bin"),    // volta (node)
        home.join(".dotnet").join("tools"), // dotnet global tools
    ];
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join("pnpm"));
        }
    } else {
        candidates.push(home.join(".local").join("share").join("pnpm"));
    }
    candidates.retain(|p| p.is_dir());
    candidates
}

#[cfg(test)]
mod toolchain_tests {
    use super::*;

    /// Only real directories are offered, so a `PATH` never grows entries that
    /// point at nothing.
    #[test]
    fn only_existing_directories_are_offered() {
        for dir in toolchain_bin_dirs() {
            assert!(dir.is_dir(), "{} was offered but does not exist", dir.display());
        }
    }

    /// Fixed inputs, so the three rules are ASSERTED on every host — not only
    /// on one that happens to be missing a toolchain.
    fn split(v: &std::ffi::OsString) -> Vec<PathBuf> {
        std::env::split_paths(v).collect()
    }

    /// Rule 1: every inherited entry survives, in its original order and
    /// position. A criterion that worked before must still work.
    #[test]
    fn augmentation_preserves_every_inherited_entry() {
        let existing = vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")];
        let out = append_missing(&existing, vec![PathBuf::from("/opt/tool/bin")]).expect("something was missing, so there must be a result");
        let after = split(&out);
        for entry in &existing {
            assert!(after.contains(entry), "dropped {}", entry.display());
        }
    }

    /// Rule 2: appended, never prepended — a toolchain the operator put on
    /// `PATH` deliberately keeps winning over a conventional location.
    #[test]
    fn inherited_entries_keep_their_priority() {
        let existing = vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")];
        let out = append_missing(&existing, vec![PathBuf::from("/opt/tool/bin")]).expect("something was missing, so there must be a result");
        let after = split(&out);
        assert_eq!(after[..existing.len()], existing[..], "inherited must lead");
        assert_eq!(after.last(), Some(&PathBuf::from("/opt/tool/bin")));
    }

    /// Rule 3: nothing to add ⇒ `None`, so the child inherits the environment
    /// untouched. The common case, and it must stay free.
    #[test]
    fn nothing_missing_means_the_environment_is_left_alone() {
        let existing = vec![PathBuf::from("/usr/bin"), PathBuf::from("/opt/tool/bin")];
        assert!(append_missing(&existing, vec![]).is_none());
        assert!(append_missing(&existing, vec![PathBuf::from("/opt/tool/bin")]).is_none(), "a candidate already on PATH is not missing");
    }

    /// A candidate is never appended twice, however many times it is OFFERED.
    ///
    /// The offering list really does repeat here — an earlier version of this
    /// test passed two distinct candidates, so it asserted ordering and called
    /// it de-duplication (found in review).
    #[test]
    fn a_candidate_is_appended_at_most_once() {
        let existing = vec![PathBuf::from("/usr/bin")];
        let out = append_missing(&existing, vec![PathBuf::from("/opt/a"), PathBuf::from("/opt/b"), PathBuf::from("/opt/a"), PathBuf::from("/opt/a")])
            .expect("two distinct ones were missing");
        let after = split(&out);
        assert_eq!(
            after,
            vec![PathBuf::from("/usr/bin"), PathBuf::from("/opt/a"), PathBuf::from("/opt/b"),],
            "a repeated candidate must appear once, in first-offered order"
        );
    }

    #[test]
    fn resolves_finds_a_program_that_is_there() {
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(resolves(program));
    }

    #[test]
    fn resolves_rejects_a_program_that_is_not() {
        assert!(!resolves("mustard-definitely-not-a-real-program-xyz"));
    }
}

/// Can `program` be resolved the way a spawned criterion would resolve it —
/// through `PATH` **or** through a conventional toolchain directory?
///
/// The single answer to "will the harness find this", so the `doctor` never
/// reports a tool missing that `run_shell_with_deadline` would have found. Two
/// resolvers would drift into telling the operator opposite things about the
/// same machine.
///
/// Deliberately NOT a `which`/`where` subprocess: this is called from the
/// doctor and from hook code, and a past Windows incident traced a session hang
/// to child processes inheriting hook stdio pipes. Pure path arithmetic cannot
/// hang.
#[must_use]
pub fn resolves(program: &str) -> bool {
    // On Windows a bare name resolves through PATHEXT; check the spellings a
    // toolchain shim actually ships with rather than guessing one.
    let names: Vec<String> =
        if cfg!(windows) { [".exe", ".cmd", ".bat", ""].iter().map(|ext| format!("{program}{ext}")).collect() } else { vec![program.to_string()] };
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&inherited).chain(toolchain_bin_dirs()).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

/// `PATH` for a spawned criterion: the inherited one, plus any conventional
/// toolchain directory it is missing.
///
/// `None` when there is nothing to add, so the child simply inherits the
/// environment unchanged — the common case, and the one that must stay free.
/// A directory already present is never appended twice.
fn augmented_path() -> Option<std::ffi::OsString> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let existing: Vec<PathBuf> = std::env::split_paths(&current).collect();
    append_missing(&existing, toolchain_bin_dirs())
}

/// The whole decision, as a pure function of its two inputs.
///
/// Split out so the rules below can be ASSERTED rather than observed: driven
/// through [`augmented_path`], every test depends on what this particular
/// machine happens to have installed, and on a host where nothing is missing
/// the test asserts nothing at all while still reporting green (found in
/// review).
///
/// Three rules, and they are the contract:
/// 1. Every inherited entry survives, in its original order and position.
/// 2. What is missing is APPENDED, so an inherited entry always wins.
/// 3. Nothing to add ⇒ `None`, and the child inherits the environment untouched.
fn append_missing(existing: &[PathBuf], candidates: Vec<PathBuf>) -> Option<std::ffi::OsString> {
    // Filtered against BOTH the inherited entries and what has already been
    // taken from this very list. Filtering only against `existing` let the same
    // candidate in twice when it was offered twice — latent today, because
    // `toolchain_bin_dirs` never repeats itself, and caught by the test that
    // finally offered a duplicate (found in review).
    let mut missing: Vec<PathBuf> = Vec::new();
    for dir in candidates {
        if existing.contains(&dir) || missing.contains(&dir) {
            continue;
        }
        missing.push(dir);
    }
    if missing.is_empty() {
        return None;
    }
    let joined: Vec<PathBuf> = existing.iter().cloned().chain(missing).collect();
    std::env::join_paths(joined).ok()
}

/// A memória que cada processo de um executor de testes pede: o jest abriu
/// um processo por núcleo, e cada um usou de 1,1 a 1,5 GB.
#[cfg(target_os = "linux")]
const MEMORY_PER_PROCESS_KIB: u64 = 2 * 1024 * 1024;

/// O comando do projeto no shell da plataforma, preso aos núcleos que a
/// máquina aguenta.
///
/// O executor de testes que abre um processo por núcleo visível — o jest lê a
/// afinidade de CPU — abria, numa máquina de 12 núcleos e 15 GB, 12 processos
/// de 1,1 a 1,5 GB cada, e o matador de falta de memória os derrubava. No
/// Linux, o comando roda por `taskset -c`, nos núcleos que [`affordable_cpus`]
/// escolhe: a afinidade passa a cada processo que ele abre. Sem limite a
/// impor, sem `taskset` na máquina, ou noutro sistema, o comando roda no shell
/// como sempre rodou.
fn project_shell_command(command: &str) -> Command {
    #[cfg(target_os = "linux")]
    if let Some(cpus) = affordable_cpus(command) {
        let shell = crate::util::platform::build_shell_command(command);
        let mut pinned = Command::new("taskset");
        pinned.arg("-c").arg(cpus).arg(shell.get_program()).args(shell.get_args());
        return pinned;
    }
    crate::util::platform::build_shell_command(command)
}

/// Os núcleos em que o comando roda, na forma que o `taskset -c` lê, ou
/// `None` quando não há o que limitar.
///
/// O comando que já traz o próprio `taskset` fica como está: quem o escreveu
/// já escolheu os núcleos. A afinidade e a memória vêm do `/proc`; se algum
/// deles não responde, o comando roda sem limite, como antes.
#[cfg(target_os = "linux")]
fn affordable_cpus(command: &str) -> Option<String> {
    if names_taskset(command) || !resolves("taskset") {
        return None;
    }
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let allowed = cpu_list(status.lines().find_map(|line| line.strip_prefix("Cpus_allowed_list:"))?)?;
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let memory_kib = meminfo.lines().find_map(|line| line.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    let cores = std::thread::available_parallelism().map_or(allowed.len(), std::num::NonZero::get);
    pick_cpus(&allowed, cores, memory_kib)
}

/// A regra inteira, em função só das entradas: o limite é o menor entre os
/// núcleos e a memória total em GB dividida por 2, no mínimo 1, e os núcleos
/// escolhidos são os primeiros que o processo já pode usar — nunca um núcleo
/// fora da afinidade que ele herdou. Com o limite cobrindo todos os
/// permitidos, nada muda, e a resposta é `None`.
#[cfg(target_os = "linux")]
fn pick_cpus(allowed: &[usize], cores: usize, memory_kib: u64) -> Option<String> {
    let by_memory = usize::try_from(memory_kib / MEMORY_PER_PROCESS_KIB).unwrap_or(usize::MAX);
    let limit = cores.min(by_memory).max(1);
    (limit < allowed.len()).then(|| allowed[..limit].iter().map(ToString::to_string).collect::<Vec<_>>().join(","))
}

/// A lista de núcleos do `/proc` (`0-3,8,10-11`), núcleo por núcleo. `None`
/// quando algum pedaço não é número nem faixa.
#[cfg(target_os = "linux")]
fn cpu_list(text: &str) -> Option<Vec<usize>> {
    let mut cpus = Vec::new();
    for part in text.trim().split(',').filter(|part| !part.is_empty()) {
        match part.split_once('-') {
            Some((first, last)) => cpus.extend(first.trim().parse::<usize>().ok()?..=last.trim().parse::<usize>().ok()?),
            None => cpus.push(part.trim().parse().ok()?),
        }
    }
    (!cpus.is_empty()).then_some(cpus)
}

/// O comando chama o `taskset` em algum ponto, também dentro de aspas — o
/// fechamento entrega o comando do projeto entre aspas simples, dentro do
/// roteiro que limpa o ambiente.
#[cfg(target_os = "linux")]
fn names_taskset(command: &str) -> bool {
    command
        .split(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '\'' | '"' | '`' | '{' | '}'))
        .any(|word| word.rsplit('/').next() == Some("taskset"))
}

#[cfg(all(test, target_os = "linux"))]
mod core_tests {
    use super::*;

    /// Números fixos, para a regra valer em qualquer máquina: 12 núcleos e
    /// 15 GB dão 7; a memória folgada deixa os núcleos; a pouca memória ainda
    /// dá 1; e a escolha fica dentro da afinidade herdada.
    #[test]
    fn the_limit_is_the_smaller_of_the_cores_and_half_the_memory() {
        let gib = 1024 * 1024;
        let twelve: Vec<usize> = (0..12).collect();
        assert_eq!(pick_cpus(&twelve, 12, 15 * gib), Some("0,1,2,3,4,5,6".to_string()));
        assert_eq!(pick_cpus(&twelve, 4, 64 * gib), Some("0,1,2,3".to_string()), "a quota below the allowed set still limits");
        assert_eq!(pick_cpus(&twelve, 12, gib), Some("0".to_string()), "at least one core");
        assert_eq!(pick_cpus(&[4, 5, 8, 9], 4, 4 * gib), Some("4,5".to_string()), "only cores the process may already use");
        assert_eq!(pick_cpus(&twelve, 12, 64 * gib), None, "nothing to limit");
        assert_eq!(cpu_list("0-3,8,10-11\n"), Some(vec![0, 1, 2, 3, 8, 10, 11]));
    }

    /// O comando que já traz o `taskset` fica como está, também entre as
    /// aspas do roteiro do fechamento; o que não o traz ganha o limite.
    #[test]
    fn a_command_that_brings_its_own_taskset_stays_as_it_is() {
        assert!(names_taskset("taskset -c 0-3 npx jest"));
        assert!(names_taskset("mustard_cmd='/usr/bin/taskset -c 0 npm test'\nexit 0"));
        assert!(!names_taskset("npx jest --maxWorkers=4"));
        assert!(!names_taskset("mustard_cmd='npm run tasksets'"));
    }
}

/// Poll cadence of [`run_shell_with_deadline`]'s wait loop. `std` has no
/// native wait-with-timeout, so the child is polled with `try_wait`; 50 ms is
/// the historical cadence of both call sites this helper absorbed.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// What became of a shell command run under a deadline.
#[derive(Debug)]
pub enum ShellOutcome {
    /// The child exited on its own. `stdout` / `stderr` are the FULL drained
    /// streams, lossily decoded and NOT trimmed — each caller applies its own
    /// trimming and excerpt policy. Por
    /// [`run_shell_interleaved_with_deadline`], `stdout` traz as duas saídas
    /// na ordem em que o comando as escreveu, e `stderr` vem vazio.
    Exited { status: ExitStatus, stdout: String, stderr: String },
    /// The deadline elapsed first and the child was killed. Its partial output
    /// is dropped: a command that never finished proved nothing.
    TimedOut { after: Duration },
    /// The child never ran, or the wait itself failed. No verdict is possible.
    SpawnFailed { error: String },
}

/// Run `command` through the platform shell in `cwd`, draining stdout AND
/// stderr concurrently, and wait for it until `timeout` elapses.
///
/// **Why the drain threads are not optional.** A verbose command (a
/// `cargo test --workspace`, a chatty AC) can emit far more than the OS pipe
/// buffer (~64 KB). Reading the pipes only after the child exits lets a full
/// buffer block the writer forever: the child never finishes, `try_wait` never
/// returns `Some`, and the caller burns its whole timeout on a process that
/// already did its work — reported as a bogus timeout. Two dedicated reader
/// threads keep the pipes empty so the child always makes progress. This is the
/// one home for that fix; a second copy is how the two call sites drifted apart
/// in the first place.
///
/// Fail-open by construction: every failure mode is a [`ShellOutcome`] variant,
/// never a panic. On timeout the child's whole process group (its process tree
/// on Windows) is killed and reaped before returning — see [`reap`].
#[must_use]
pub fn run_shell_with_deadline(command: &str, cwd: &Path, timeout: Duration) -> ShellOutcome {
    run_under_deadline(command, cwd, timeout, false)
}

/// [`run_shell_with_deadline`] com stdout e stderr escritos num cano só: a
/// saída se lê na ordem em que o comando a escreveu, linha após linha, como o
/// terminal a mostra.
///
/// Dois canos lidos à parte perdem essa ordem: quem os junta escolhe uma saída
/// primeiro, e a última linha da primeira cola na primeira da segunda. Um
/// `npm run lint --workspaces` escreve o anúncio e os erros de cada pacote no
/// stdout e o rodapé `npm error path` no stderr; juntado com o stderr antes, o
/// rodapé vinha antes do bloco do pacote, o primeiro anúncio virava fim de
/// outra linha, e quem lia a falha não ligava o arquivo ao pacote.
#[must_use]
pub fn run_shell_interleaved_with_deadline(command: &str, cwd: &Path, timeout: Duration) -> ShellOutcome {
    run_under_deadline(command, cwd, timeout, true)
}

/// O corpo único dos dois executores acima; `interleaved` escolhe um cano só
/// para as duas saídas em vez de um para cada.
fn run_under_deadline(command: &str, cwd: &Path, timeout: Duration, interleaved: bool) -> ShellOutcome {
    let mut cmd = project_shell_command(command);
    cmd.current_dir(cwd).stdin(Stdio::null());
    let shared = if interleaved {
        match one_pipe_for_both(&mut cmd) {
            Ok(reader) => Some(reader),
            Err(e) => return ShellOutcome::SpawnFailed { error: e.to_string() },
        }
    } else {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        None
    };
    if let Some(path) = augmented_path() {
        cmd.env("PATH", path);
    }
    // The command runs in its own process group, so the deadline kills the
    // whole group: the shell does not always hand its place to the command
    // (Debian's `sh` does not), and the real command ends up its grandchild.
    //
    // Accepted trade-off: outside the terminal's group, a Ctrl-C typed in a
    // terminal kills `mustard-rt`, and the command runs on, orphaned, until it
    // ends. Through the agent, which runs with no terminal, this barely
    // matters. Forwarding the signal would need `unsafe` or a new dependency,
    // and neither goes in.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let spawned = cmd.spawn();
    // O comando guarda as cópias da ponta de escrita do cano até ser
    // descartado, e com ela aberta aqui o leitor nunca vê o fim da saída.
    drop(cmd);
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            return ShellOutcome::SpawnFailed { error: e.to_string() };
        }
    };

    let readers = match shared {
        Some(reader) => vec![drain(Some(reader))],
        None => vec![drain(child.stdout.take()), drain(child.stderr.take())],
    };

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut texts = readers.into_iter().map(|reader| String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned());
                let stdout = texts.next().unwrap_or_default();
                let stderr = texts.next().unwrap_or_default();
                return ShellOutcome::Exited { status, stdout, stderr };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    reap(&mut child, readers);
                    return ShellOutcome::TimedOut { after: timeout };
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            // The wait itself failed (the OS lost the child): no exit status
            // will ever arrive, so this is as un-attemptable as a failed spawn.
            Err(e) => {
                reap(&mut child, readers);
                return ShellOutcome::SpawnFailed { error: e.to_string() };
            }
        }
    }
}

/// Liga o stdout e o stderr do comando à ponta de escrita de um cano novo e
/// devolve a ponta de leitura. O que as duas saídas escrevem chega nele na
/// ordem em que o comando escreveu.
fn one_pipe_for_both(cmd: &mut Command) -> std::io::Result<std::io::PipeReader> {
    let (reader, writer) = std::io::pipe()?;
    cmd.stdout(writer.try_clone()?).stderr(writer);
    Ok(reader)
}

/// Start `command` through the platform shell in `cwd` and leave it running
/// on its own: no wait, no pipe to drain, and a process the caller's exit — a
/// hook's, a terminal's — does not take down with it. The command writes its
/// own output where it wants (a redirect inside `command`); the child's three
/// standard streams are closed.
///
/// `PATH` gets the same conventional toolchain folders as
/// [`run_shell_with_deadline`], so `cargo` is found from a non-interactive
/// session too. Detached like `scan`'s long passes: its own process group on
/// Unix, no console on Windows.
///
/// # Errors
///
/// The shell could not be started.
pub fn spawn_detached_shell(command: &str, cwd: &Path) -> std::io::Result<()> {
    let mut cmd = crate::util::platform::build_shell_command(command);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(path) = augmented_path() {
        cmd.env("PATH", path);
    }
    spawn_detached(cmd, cwd)
}

/// Start `cmd` in `cwd` and leave it running on its own, as
/// [`spawn_detached_shell`] does: no wait, and a process the caller's exit
/// does not take down with it. The standard streams are whatever the caller
/// set on `cmd`.
///
/// # Errors
///
/// The program could not be started.
pub fn spawn_detached(mut cmd: Command, cwd: &Path) -> std::io::Result<()> {
    cmd.current_dir(cwd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`.
        cmd.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    cmd.spawn().map(drop)
}

/// Spawn a thread that drains one child pipe to EOF, returning whatever bytes
/// arrived. Best-effort: an absent pipe or a read error yields what it has.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    })
}

/// Kill + reap a child whose output no longer matters, with everything it
/// started, then join its readers.
///
/// Killing only the shell is not enough: the shell does not always hand its
/// place over to the command (`sh -c "sleep 5"`, or `cmd /C gh …` on Windows),
/// so the real command is a grandchild that would go on running alone — a
/// `cargo` holding the build directory's lock while the next criterion times
/// out behind it. The child runs in its own process group on Unix, and
/// [`kill_tree`] kills the whole group (the whole tree on Windows). With it
/// dead, the pipes close and the readers finish.
///
/// The readers are waited for only [`READER_GRACE`]: a process that left the
/// group (see [`kill_tree`]) can still hold the pipes open, and waiting for it
/// would hold this call past the deadline. A reader still running then is let
/// go; its thread ends on its own when that process closes the pipe.
fn reap(child: &mut std::process::Child, readers: Vec<std::thread::JoinHandle<Vec<u8>>>) {
    kill_tree(child.id());
    let _ = child.kill();
    let _ = child.wait();
    let grace = Instant::now() + READER_GRACE;
    while !readers.iter().all(std::thread::JoinHandle::is_finished) && Instant::now() < grace {
        std::thread::sleep(POLL_INTERVAL);
    }
    for reader in readers {
        if reader.is_finished() {
            let _ = reader.join();
        }
    }
}

/// How long [`reap`] waits for the output readers once the group is dead.
const READER_GRACE: Duration = Duration::from_secs(1);

/// Kill the process group `pid` leads (Unix: `kill -KILL -<pid>` through the
/// shell, since the crate forbids `unsafe`) or the process tree rooted at
/// `pid` (Windows: `taskkill /F /T /PID`). Best-effort, like [`kill_pid`].
///
/// No `--` before the negative group: the `kill` built into `dash`, the `sh`
/// of Debian and Ubuntu, refuses it ("Illegal number"), and the group would
/// live on. With the signal named first, the negative number can only be the
/// group, in `dash`, `bash` and `zsh` alike.
///
/// Accepted gap: a process that opens its own session (`setsid`, a service
/// that detaches itself) leaves the group, and on Windows a process whose
/// parent already exited leaves the tree; neither dies here. [`reap`] does not
/// wait for them past a short grace.
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", &format!("taskkill /F /T /PID {pid}")]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", &format!("kill -KILL -{pid}")]);
        c
    };
    let _ = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shell command that prints ~90 KB — far past the ~64 KB OS pipe buffer
    /// — and then exits 3. Selected at RUN time, not compile time: the Windows
    /// shell is now whichever one [`crate::util::platform::build_shell_command`]
    /// resolves, so a `cfg!(windows)` fixture would drive `cmd.exe` syntax into
    /// a POSIX shell.
    const BIG_OUTPUT_EXIT_3_POSIX: &str = "s=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA; i=0; \
         while [ $i -lt 12 ]; do s=\"$s$s\"; i=$((i+1)); done; echo \"$s\"; exit 3";
    #[cfg(windows)]
    const BIG_OUTPUT_EXIT_3_CMD: &str = "(for /L %i in (1,1,3000) do @echo AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA) & exit 3";

    /// The form matching the shell this process will actually spawn.
    fn big_output_exit_3() -> &'static str {
        #[cfg(windows)]
        if crate::util::platform::posix_shell().is_none() {
            return BIG_OUTPUT_EXIT_3_CMD;
        }
        BIG_OUTPUT_EXIT_3_POSIX
    }

    /// Um comando preso até um arquivo de liberação que o teste nunca escreve,
    /// de modo que um prazo de 1 s sempre dispara primeiro, por mais carregada
    /// que esteja a máquina. Ele só desiste no limite de segurança das esperas
    /// dos testes; fora do Windows, deixa então `mustard-deadline-test.ended`:
    /// o arquivo diz que a chamada esperou o comando em vez de cortá-lo.
    #[cfg(windows)]
    fn sleeps_seconds() -> String {
        crate::held_command::held_until_released(Path::new("mustard-deadline-test.release"))
    }
    #[cfg(not(windows))]
    fn sleeps_seconds() -> String {
        format!("{}; touch mustard-deadline-test.ended", crate::held_command::held_until_released(Path::new("mustard-deadline-test.release")))
    }

    /// O laço do shell preso até `release` existir — ou até o limite de
    /// segurança das esperas dos testes, para nunca ficar para trás —, que
    /// então cria `ended`.
    #[cfg(unix)]
    fn held_until(release: &Path, ended: &Path) -> String {
        format!("{}; touch '{}'", crate::held_command::held_until_released(release), ended.display())
    }

    /// THE regression this helper exists for: a command that overflows the OS
    /// pipe buffer must still finish and be judged by its exit code. Before the
    /// concurrent drain, the child blocked writing into a full pipe, `try_wait`
    /// never saw it exit, and the caller reported a bogus timeout.
    #[test]
    fn shell_drains_beyond_the_pipe_buffer_and_reports_exit_code() {
        let dir = std::env::temp_dir();
        let outcome = run_shell_with_deadline(big_output_exit_3(), &dir, Duration::from_secs(60));
        match outcome {
            ShellOutcome::Exited { status, stdout, .. } => {
                assert_eq!(status.code(), Some(3), "judged by its own exit code");
                assert!(stdout.len() > 64 * 1024, "the whole stream is drained, not just a pipe buffer's worth ({} bytes)", stdout.len());
            }
            other => panic!("a completed command must report Exited, got {other:?}"),
        }
    }

    /// A command that outlives its deadline is killed and reported as
    /// `TimedOut` — a class of its own, never a silent success. The call
    /// returns while the command is still held, not when it would have ended:
    /// the held process dies with the shell's process group.
    #[test]
    fn shell_reports_timed_out_when_the_deadline_fires_first() {
        let dir = tempfile::tempdir().expect("isolated deadline fixture");
        let outcome = run_shell_with_deadline(&sleeps_seconds(), dir.path(), Duration::from_secs(1));
        match outcome {
            ShellOutcome::TimedOut { after } => assert_eq!(after, Duration::from_secs(1)),
            other => panic!("a command past its deadline must report TimedOut, got {other:?}"),
        }
        assert!(!dir.path().join("mustard-deadline-test.ended").exists(), "held past the deadline");
    }

    /// `true` when `pid` no longer runs: absent, or a zombie, which is already
    /// dead and only waits to be collected.
    #[cfg(unix)]
    fn no_longer_runs(pid: u32) -> bool {
        Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().is_ok_and(|out| {
            let stat = String::from_utf8_lossy(&out.stdout);
            let stat = stat.trim();
            stat.is_empty() || stat.starts_with('Z')
        })
    }

    /// At the deadline, the whole group dies: the grandchild the shell
    /// launched, held until the test releases it, stops running without ever
    /// reaching its end, and the function returns while it is still held.
    #[cfg(unix)]
    #[test]
    fn the_deadline_kills_the_grandchild_too() {
        let dir = tempfile::tempdir().unwrap();
        let (pid_file, release, ended) = (dir.path().join("neto.pid"), dir.path().join("release"), dir.path().join("ended"));
        let command = format!("({}) & echo $! > '{}'; wait", held_until(&release, &ended), pid_file.display());
        let outcome = run_shell_with_deadline(&command, dir.path(), Duration::from_secs(1));
        assert!(matches!(outcome, ShellOutcome::TimedOut { .. }), "{outcome:?}");
        assert!(!ended.exists(), "held past the deadline");
        let pid: u32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        crate::wait_for::wait_for(&format!("o fim do neto {pid}"), || no_longer_runs(pid).then_some(()));
        assert!(!ended.exists(), "the grandchild {pid} was killed, and did not run to its end");
        std::fs::write(&release, "").unwrap();
    }

    /// A process that opens its own session escapes the group and holds the
    /// output open; even so the function returns at the deadline, while that
    /// process is still held, without waiting for it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_outside_the_group_does_not_hold_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let (release, outside, inside) = (dir.path().join("release"), dir.path().join("outside-ended"), dir.path().join("inside-ended"));
        let escaping = dir.path().join("outside.sh");
        std::fs::write(&escaping, held_until(&release, &outside)).unwrap();
        let command = format!("setsid sh '{}' & {}", escaping.display(), held_until(&release, &inside));
        let outcome = run_shell_with_deadline(&command, dir.path(), Duration::from_secs(1));
        assert!(matches!(outcome, ShellOutcome::TimedOut { .. }), "{outcome:?}");
        assert!(!outside.exists(), "the call came back while the process outside the group still held the output");
        std::fs::write(&release, "").unwrap();
    }
}
