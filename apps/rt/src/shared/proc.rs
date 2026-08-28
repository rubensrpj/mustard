//! `proc` — signal-free, cross-platform process/port primitives shared by both
//! the enforcement face (`hooks`) and the script face (`commands`).
//!
//! These were originally private helpers in `hooks::session::session_start_inject`
//! (which spawns and reaps the OTEL collector). They are lifted here so a `run`
//! command (`commands::economy::otel::stop`) can reuse the exact same tested kill
//! machinery without a `commands -> hooks` layering inversion — `shared` is the
//! one module both faces may depend on, and it never depends back.
//!
//! Every function is best-effort and fail-open: a missing tool on `PATH`, an
//! empty result, or a kill error degrades to an `eprintln!` warning and an empty
//! / `false` value. None of them panic. The crate forbids `unsafe`, so none of
//! these use raw OS signal APIs — they shell out to `netstat`/`lsof`/`taskkill`/
//! `kill`/`tasklist` instead.
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
    let Some(home) = crate::util::home_dir() else {
        return Vec::new();
    };
    // Each entry is the location that toolchain's own installer documents.
    let mut candidates: Vec<PathBuf> = vec![
        home.join(".cargo").join("bin"),          // rustup
        home.join(".local").join("bin"),          // pip / pipx / uv
        home.join("go").join("bin"),              // go install
        home.join(".bun").join("bin"),            // bun
        home.join(".deno").join("bin"),           // deno
        home.join(".volta").join("bin"),          // volta (node)
        home.join(".dotnet").join("tools"),       // dotnet global tools
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
        let out = append_missing(&existing, vec![PathBuf::from("/opt/tool/bin")])
            .expect("something was missing, so there must be a result");
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
        let out = append_missing(&existing, vec![PathBuf::from("/opt/tool/bin")])
            .expect("something was missing, so there must be a result");
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
        assert!(
            append_missing(&existing, vec![PathBuf::from("/opt/tool/bin")]).is_none(),
            "a candidate already on PATH is not missing"
        );
    }

    /// A candidate is never appended twice, however many times it is OFFERED.
    ///
    /// The offering list really does repeat here — an earlier version of this
    /// test passed two distinct candidates, so it asserted ordering and called
    /// it de-duplication (found in review).
    #[test]
    fn a_candidate_is_appended_at_most_once() {
        let existing = vec![PathBuf::from("/usr/bin")];
        let out = append_missing(
            &existing,
            vec![
                PathBuf::from("/opt/a"),
                PathBuf::from("/opt/b"),
                PathBuf::from("/opt/a"),
                PathBuf::from("/opt/a"),
            ],
        )
        .expect("two distinct ones were missing");
        let after = split(&out);
        assert_eq!(
            after,
            vec![
                PathBuf::from("/usr/bin"),
                PathBuf::from("/opt/a"),
                PathBuf::from("/opt/b"),
            ],
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
    let names: Vec<String> = if cfg!(windows) {
        [".exe", ".cmd", ".bat", ""]
            .iter()
            .map(|ext| format!("{program}{ext}"))
            .collect()
    } else {
        vec![program.to_string()]
    };
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&inherited)
        .chain(toolchain_bin_dirs())
        .any(|dir| names.iter().any(|n| dir.join(n).is_file()))
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

/// Spawn `exe args…` as a detached, long-lived background daemon whose open
/// handles are NOT inherited from this process.
///
/// This matters specifically when the spawner is a harness hook. A hook's
/// stdout is a pipe Claude Code reads until EOF; a plain `Command::spawn` on
/// Windows passes `bInheritHandles = TRUE`, so a long-lived child inherits a
/// duplicate of that stdout pipe handle. The hook process itself can exit, but
/// the pipe's write end stays open inside the daemon, EOF never arrives, and
/// the harness hangs the entire session waiting for the hook's output (observed
/// as a new session that freezes at "Initializing harness…" and must be
/// killed). Routing the spawn through `cmd /C start "" /B` launches the daemon
/// with `bInheritHandles = FALSE`, which breaks the inheritance — the canonical
/// safe-Rust detach, since the crate forbids `unsafe` (so `SetHandleInformation`
/// on the std handles is out). On Unix the `Stdio::null` redirects already
/// replace the inherited fds with `/dev/null`, so a direct spawn carries no such
/// leak.
///
/// Best-effort: returns the spawn error (a missing `cmd`, an exec failure) for
/// the caller to log and fail open — the daemon is telemetry, never load-bearing.
pub fn spawn_detached(exe: &Path, args: &[&str]) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // PowerShell `Start-Process` launches the daemon via `CreateProcess`
        // with `bInheritHandles = FALSE`, so the child inherits NONE of this
        // process's handles — including the harness stdout pipe. (`cmd /C start
        // /B` does NOT achieve this: with `/B` the child stays in the same
        // console and still inherits the pipe, so the session keeps hanging —
        // verified empirically.) `-WindowStyle Hidden` suppresses the new
        // console window the launch would otherwise flash for a console app.
        // The transient `powershell` process inherits the pipe but exits within
        // ~0.5 s of launching the daemon, so EOF arrives promptly.
        //
        // Single quotes are PowerShell's literal string; a literal `'` inside a
        // value is escaped by doubling it.
        let q = |s: &str| s.replace('\'', "''");
        let arg_list = args
            .iter()
            .map(|a| format!("'{}'", q(a)))
            .collect::<Vec<_>>()
            .join(",");
        let script = if arg_list.is_empty() {
            format!("Start-Process -FilePath '{}' -WindowStyle Hidden", q(&exe.display().to_string()))
        } else {
            format!(
                "Start-Process -FilePath '{}' -ArgumentList {arg_list} -WindowStyle Hidden",
                q(&exe.display().to_string())
            )
        };
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        Command::new(exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
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
    /// trimming and excerpt policy.
    Exited {
        status: ExitStatus,
        stdout: String,
        stderr: String,
    },
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
/// never a panic. On timeout the child is killed and reaped before returning.
#[must_use]
pub fn run_shell_with_deadline(command: &str, cwd: &Path, timeout: Duration) -> ShellOutcome {
    let mut cmd = crate::util::platform::build_shell_command(command);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = augmented_path() {
        cmd.env("PATH", path);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ShellOutcome::SpawnFailed { error: e.to_string() },
    };

    let out_reader = drain(child.stdout.take());
    let err_reader = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = out_reader.join().unwrap_or_default();
                let stderr = err_reader.join().unwrap_or_default();
                return ShellOutcome::Exited {
                    status,
                    stdout: String::from_utf8_lossy(&stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&stderr).into_owned(),
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    reap(&mut child, out_reader, err_reader);
                    return ShellOutcome::TimedOut { after: timeout };
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            // The wait itself failed (the OS lost the child): no exit status
            // will ever arrive, so this is as un-attemptable as a failed spawn.
            Err(e) => {
                reap(&mut child, out_reader, err_reader);
                return ShellOutcome::SpawnFailed { error: e.to_string() };
            }
        }
    }
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

/// Kill + reap a child whose output no longer matters, then join its readers.
/// Killing closes the pipes, so the reader threads hit EOF and finish instead
/// of outliving this call.
fn reap(
    child: &mut std::process::Child,
    out_reader: std::thread::JoinHandle<Vec<u8>>,
    err_reader: std::thread::JoinHandle<Vec<u8>>,
) {
    let _ = child.kill();
    let _ = child.wait();
    let _ = out_reader.join();
    let _ = err_reader.join();
}

/// Free the given OTLP port: find whatever process is listening on
/// `127.0.0.1:<port>` and kill it. Best-effort and fail-open at every step.
///
/// Returns the PIDs it attempted to kill (already-dead or unkillable PIDs are
/// still reported — the caller surfaces them for the human line). The
/// idempotence checks live in the callers; this is the raw port-reap.
pub fn free_port(port: u16) -> Vec<u32> {
    let own = session_ancestry();
    let pids: Vec<u32> = listening_pids(port)
        .into_iter()
        .filter(|pid| {
            let protected = own.contains(pid);
            if protected {
                // Killing an ancestor kills the session this code runs INSIDE.
                // Measured in the field (2026-08-19, WSL): the unfiltered lsof
                // below listed the Claude process — an OTLP CLIENT of the very
                // port being freed — and this loop SIGTERMed it, ending the
                // session with exit 143 every few minutes for days.
                eprintln!("proc: refusing to kill pid {pid} — it is this session's own ancestry");
            }
            !protected
        })
        .collect();
    for &pid in &pids {
        kill_pid(pid);
    }
    pids
}

/// The PID of this process and every ancestor above it, read from
/// `/proc/<pid>/status` `PPid:` links. On Windows (no `/proc`) only the own
/// PID is returned — the netstat query there already filters to LISTENING
/// rows, so the ancestry can never appear in the kill list to begin with.
///
/// Fail-open: an unreadable link ends the walk with what was collected —
/// a SHORTER protected set only ever under-protects, never blocks the reap.
fn session_ancestry() -> std::collections::BTreeSet<u32> {
    let mut protected = std::collections::BTreeSet::new();
    let mut pid = std::process::id();
    for _ in 0..32 {
        if !protected.insert(pid) || pid <= 1 {
            break;
        }
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            break;
        };
        let Some(ppid) = status
            .lines()
            .find_map(|l| l.strip_prefix("PPid:"))
            .and_then(|v| v.trim().parse::<u32>().ok())
        else {
            break;
        };
        pid = ppid;
    }
    protected
}

/// PIDs listening on `127.0.0.1:<port>`, parsed from a platform query. Empty
/// on any failure (no tool on PATH, nothing listening, unparseable output).
pub(crate) fn listening_pids(port: u16) -> Vec<u32> {
    #[cfg(windows)]
    {
        // `netstat -ano` rows look like:
        //   TCP    127.0.0.1:4318    0.0.0.0:0    LISTENING    12345
        // The trailing column is the owning PID. Filter to LISTENING rows for
        // our port and parse the last whitespace-separated token.
        let query = format!("netstat -ano | findstr :{port} | findstr LISTENING");
        let out = Command::new("cmd")
            .args(["/C", &query])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) => parse_netstat_pids(&String::from_utf8_lossy(&o.stdout), port),
            Err(e) => {
                eprintln!("proc: netstat for port {port} failed ({e})");
                Vec::new()
            }
        }
    }
    #[cfg(not(windows))]
    {
        // `lsof -ti tcp:<port> -sTCP:LISTEN` prints one PID per line (TCP, no
        // header) — the state filter is LOAD-BEARING: without it lsof lists
        // every process with ANY endpoint on the port, which includes the OTLP
        // CLIENTS shipping telemetry to the collector. The Claude session
        // itself is such a client, and the unfiltered query is what had this
        // reap kill the session it ran inside (the Windows branch above always
        // filtered to LISTENING; only this branch had the hole).
        let out = Command::new("sh")
            .args(["-c", &lsof_listener_query(port)])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) => parse_lsof_pids(&String::from_utf8_lossy(&o.stdout)),
            Err(e) => {
                eprintln!("proc: lsof for port {port} failed ({e})");
                Vec::new()
            }
        }
    }
}

/// Parse owning PIDs from `netstat -ano` output, keeping only LISTENING rows
/// whose local address ends in `:<port>`. The PID is the final whitespace token.
/// Pure string parse — unit-testable without spawning `netstat`.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn parse_netstat_pids(text: &str, port: u16) -> Vec<u32> {
    let suffix = format!(":{port}");
    let mut pids = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Expect: PROTO LOCAL REMOTE STATE PID (at least 5 columns).
        if cols.len() < 5 || !cols.iter().any(|c| c.eq_ignore_ascii_case("LISTENING")) {
            continue;
        }
        // Local address is column 1; match on the :<port> suffix.
        if !cols[1].ends_with(&suffix) {
            continue;
        }
        if let Ok(pid) = cols[cols.len() - 1].parse::<u32>() {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

/// The exact shell query the Unix reap runs. A function so the test can pin
/// the `-sTCP:LISTEN` state filter — the one token whose absence turns the
/// reap into a session-killer (see [`free_port`]).
#[cfg(not(windows))]
fn lsof_listener_query(port: u16) -> String {
    format!("lsof -ti tcp:{port} -sTCP:LISTEN")
}

/// Parse PIDs from `lsof -ti` output — one PID per line. Pure string parse —
/// unit-testable without spawning `lsof`.
///
/// The `allow` belongs to THIS function and it drifted: `lsof_listener_query`
/// was inserted between this doc comment and the function it describes, so both
/// landed on the newcomer — which is itself `#[cfg(not(windows))]` and is
/// stripped on Windows, taking the guard with it. On Windows the only remaining
/// callers of `parse_lsof_pids` are the `#[cfg(not(windows))]` reap and the test
/// module, so it went `dead_code` in the binary target, where the crate-root
/// `#![allow(dead_code)]` does not apply. Invisible while warnings were merely
/// warnings; the third thing `-D warnings` caught. Its twin
/// [`parse_netstat_pids`] never lost its own guard, which is why the asymmetry
/// never showed up on Linux.
#[cfg_attr(not(any(unix, test)), allow(dead_code))]
fn parse_lsof_pids(text: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    for line in text.lines() {
        if let Ok(pid) = line.trim().parse::<u32>() {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

/// Best-effort, signal-free process termination via a subprocess (the crate
/// forbids `unsafe`). `cmd /C taskkill /F /PID` on Windows; `sh -c kill` on
/// POSIX. Fail-open: any error degrades to a warning.
pub fn kill_pid(pid: u32) {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", &format!("taskkill /F /PID {pid}")]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", &format!("kill {pid}")]);
        c
    };
    if let Err(e) = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        eprintln!("proc: kill pid {pid} failed ({e})");
    }
}

/// Whether a process with `pid` is alive — WHEN that question can be answered
/// at all. `Some(true)` alive, `Some(false)` measured absent, `None` when the
/// probe itself could not run (no `kill`/`tasklist` on `PATH`, an unknown
/// platform).
///
/// The third state is not pedantry: collapsing "could not measure" into
/// "absent" is safe in exactly one direction. A caller that respawns a daemon
/// pays a wasted spawn for the mistake; a caller that DELETES what an absent
/// owner left behind pays with the live owner's directory. So the measurement
/// lives here and the judgement lives in each consumer — see
/// [`is_process_alive`] for the respawn reading, and
/// `commands::maint::worktree_gc` for the reading that refuses to remove on an
/// unmeasured answer.
///
/// Cross-platform without `unsafe`: on Unix, sends signal `0` via `kill -0`
/// (the POSIX existence probe). On Windows, queries `tasklist /FI` for the
/// PID — slower than `OpenProcess` but `windows-sys` is not a dep and the
/// crate forbids `unsafe`.
#[must_use]
pub fn process_liveness(pid: u32) -> Option<bool> {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .map(|s| s.success())
    }
    #[cfg(windows)]
    {
        // `tasklist /NH /FI "PID eq <pid>"` prints either the matching row or
        // the literal "INFO: No tasks are running…" string when absent. Probe
        // stdout for the PID itself, which appears in the matching row only.
        // A non-zero `tasklist` exit is the tool failing, not an answer about
        // the process — that is the `None` case, never a `Some(false)`.
        let pid_str = pid.to_string();
        let out = Command::new("tasklist")
            .args(["/NH", "/FI", &format!("PID eq {pid_str}")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                // The PID appears as a whitespace-separated column only when a
                // row matched; the "No tasks" message never contains the
                // numeric PID.
                Some(text.split_whitespace().any(|tok| tok == pid_str))
            }
            _ => None,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Unknown platform — the probe cannot answer at all.
        let _ = pid;
        None
    }
}

/// `true` if a process with `pid` is currently alive on the host.
///
/// The respawn reading of [`process_liveness`]: a probe that could not answer
/// degrades to `false`, which simply forces a re-spawn — safe per the
/// idempotence contract: the second collector will fail to bind the port and
/// exit, leaving the first one running.
#[must_use]
pub fn is_process_alive(pid: u32) -> bool {
    process_liveness(pid).unwrap_or(false)
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
    const BIG_OUTPUT_EXIT_3_CMD: &str =
        "(for /L %i in (1,1,3000) do @echo AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA) & exit 3";

    /// The form matching the shell this process will actually spawn.
    fn big_output_exit_3() -> &'static str {
        #[cfg(windows)]
        if crate::util::platform::posix_shell().is_none() {
            return BIG_OUTPUT_EXIT_3_CMD;
        }
        BIG_OUTPUT_EXIT_3_POSIX
    }

    /// A command that stays alive ~3 s, so a 1 s deadline always fires first.
    #[cfg(windows)]
    const SLEEPS_SECONDS: &str = "ping -n 4 127.0.0.1";
    #[cfg(not(windows))]
    const SLEEPS_SECONDS: &str = "sleep 3";

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
                assert!(
                    stdout.len() > 64 * 1024,
                    "the whole stream is drained, not just a pipe buffer's worth ({} bytes)",
                    stdout.len()
                );
            }
            other => panic!("a completed command must report Exited, got {other:?}"),
        }
    }

    /// A command that outlives its deadline is killed and reported as
    /// `TimedOut` — a class of its own, never a silent success.
    #[test]
    fn shell_reports_timed_out_when_the_deadline_fires_first() {
        let dir = std::env::temp_dir();
        let outcome = run_shell_with_deadline(SLEEPS_SECONDS, &dir, Duration::from_secs(1));
        match outcome {
            ShellOutcome::TimedOut { after } => assert_eq!(after, Duration::from_secs(1)),
            other => panic!("a command past its deadline must report TimedOut, got {other:?}"),
        }
    }

    #[test]
    fn parse_netstat_pid_from_listening_row() {
        // Real `netstat -ano` shape: PROTO LOCAL REMOTE STATE PID.
        let text = "  TCP    127.0.0.1:4318    0.0.0.0:0    LISTENING    12345\r\n";
        assert_eq!(parse_netstat_pids(text, 4318), vec![12345]);
    }

    #[test]
    fn parse_netstat_ignores_other_ports_and_states() {
        let text = "\
  TCP    127.0.0.1:4318    0.0.0.0:0    LISTENING       12345\r\n\
  TCP    127.0.0.1:9999    0.0.0.0:0    LISTENING       67890\r\n\
  TCP    127.0.0.1:4318    127.0.0.1:55000  ESTABLISHED  24680\r\n";
        // Only the LISTENING row on :4318 contributes; ESTABLISHED + :9999 drop.
        assert_eq!(parse_netstat_pids(text, 4318), vec![12345]);
    }

    #[test]
    fn parse_netstat_empty_on_no_match() {
        assert!(parse_netstat_pids("", 4318).is_empty());
        assert!(parse_netstat_pids("garbage line with no pid", 4318).is_empty());
    }

    /// The state filter is the whole fix: an unfiltered `lsof -ti tcp:<port>`
    /// lists the port's CLIENTS too — the Claude session among them — and the
    /// reap then kills the session it runs inside. This pins the token.
    #[cfg(not(windows))]
    #[test]
    fn the_reap_query_asks_only_for_the_listener() {
        assert!(lsof_listener_query(4318).contains("-sTCP:LISTEN"));
    }

    /// The session's own ancestry is never a reap target: the set holds this
    /// process and walks upward to init, so a pid list that (through any
    /// future query bug) names an ancestor is filtered before the kill.
    ///
    /// The full walk needs `/proc`, so only Linux can assert an ancestor was
    /// reached; macOS (no procfs) and Windows degrade to protecting the own
    /// PID alone — fail-open, and on those systems the platform query already
    /// cannot name the session (Windows filters LISTENING; macOS's lsof takes
    /// the same `-sTCP:LISTEN` filter this fix pins).
    #[test]
    fn the_session_ancestry_protects_self_and_parents() {
        let own = session_ancestry();
        assert!(own.contains(&std::process::id()), "self is protected");
        #[cfg(target_os = "linux")]
        assert!(own.len() >= 2, "at least one ancestor walked: {own:?}");
    }

    #[test]
    fn parse_lsof_pids_one_per_line_dedup() {
        let text = "12345\n67890\n12345\n";
        assert_eq!(parse_lsof_pids(text), vec![12345, 67890]);
    }

    #[test]
    fn parse_lsof_empty_on_blank() {
        assert!(parse_lsof_pids("").is_empty());
        assert!(parse_lsof_pids("\n  \n").is_empty());
    }
}
