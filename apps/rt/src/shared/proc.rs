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
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

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
    let pids = listening_pids(port);
    for &pid in &pids {
        kill_pid(pid);
    }
    pids
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
        // `lsof -ti tcp:<port>` prints one PID per line (TCP, no header).
        let out = Command::new("sh")
            .args(["-c", &format!("lsof -ti tcp:{port}")])
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

/// Parse PIDs from `lsof -ti` output — one PID per line. Pure string parse —
/// unit-testable without spawning `lsof`.
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

/// `true` if a process with `pid` is currently alive on the host.
///
/// Cross-platform without `unsafe`: on Unix, sends signal `0` via `kill -0`
/// (the POSIX existence probe). On Windows, queries `tasklist /FI` for the
/// PID — slower than `OpenProcess` but `windows-sys` is not a dep and the
/// crate forbids `unsafe`. A spawn failure (no `kill`/`tasklist` on PATH)
/// degrades to `false`, which simply forces a re-spawn — safe per the
/// idempotence contract: the second collector will fail to bind the port and
/// exit, leaving the first one running.
#[must_use]
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        // `tasklist /NH /FI "PID eq <pid>"` prints either the matching row or
        // the literal "INFO: No tasks are running…" string when absent. Probe
        // stdout for the PID itself, which appears in the matching row only.
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
                text.split_whitespace().any(|tok| tok == pid_str)
            }
            _ => false,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Unknown platform — pessimistically report not-alive so the caller
        // re-spawns; a duplicate collector will fail to bind and exit cleanly.
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shell command that prints ~90 KB — far past the ~64 KB OS pipe buffer
    /// — and then exits 3. Per-platform because the shell is `cmd.exe` on
    /// Windows and `sh` elsewhere.
    #[cfg(windows)]
    const BIG_OUTPUT_EXIT_3: &str =
        "(for /L %i in (1,1,3000) do @echo AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA) & exit 3";
    #[cfg(not(windows))]
    const BIG_OUTPUT_EXIT_3: &str = "s=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA; i=0; \
         while [ $i -lt 12 ]; do s=\"$s$s\"; i=$((i+1)); done; echo \"$s\"; exit 3";

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
        let outcome = run_shell_with_deadline(BIG_OUTPUT_EXIT_3, &dir, Duration::from_secs(60));
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
