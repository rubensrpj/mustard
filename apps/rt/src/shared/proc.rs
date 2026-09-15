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
/// never a panic. On timeout the child's whole process group (its process tree
/// on Windows) is killed and reaped before returning — see [`reap`].
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
fn reap(
    child: &mut std::process::Child,
    out_reader: std::thread::JoinHandle<Vec<u8>>,
    err_reader: std::thread::JoinHandle<Vec<u8>>,
) {
    kill_tree(child.id());
    let _ = child.kill();
    let _ = child.wait();
    let grace = Instant::now() + READER_GRACE;
    while !(out_reader.is_finished() && err_reader.is_finished()) && Instant::now() < grace {
        std::thread::sleep(POLL_INTERVAL);
    }
    for reader in [out_reader, err_reader] {
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
    /// `TimedOut` — a class of its own, never a silent success. The call
    /// returns at the deadline, not when the command would have ended: the
    /// sleeping process, a grandchild of the shell, dies with the shell's
    /// process group.
    #[test]
    fn shell_reports_timed_out_when_the_deadline_fires_first() {
        let dir = std::env::temp_dir();
        let started = Instant::now();
        let outcome = run_shell_with_deadline(SLEEPS_SECONDS, &dir, Duration::from_secs(1));
        match outcome {
            ShellOutcome::TimedOut { after } => assert_eq!(after, Duration::from_secs(1)),
            other => panic!("a command past its deadline must report TimedOut, got {other:?}"),
        }
        assert!(started.elapsed() < Duration::from_millis(2_500), "held past the deadline: {:?}", started.elapsed());
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
    /// launched, sleeping longer than the deadline, no longer runs when the
    /// function returns, and it returns on time.
    #[cfg(unix)]
    #[test]
    fn the_deadline_kills_the_grandchild_too() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("neto.pid");
        let command = format!("sleep 7 & echo $! > '{}'; wait", pid_file.display());
        let started = Instant::now();
        let outcome = run_shell_with_deadline(&command, dir.path(), Duration::from_secs(1));
        assert!(matches!(outcome, ShellOutcome::TimedOut { .. }), "{outcome:?}");
        assert!(started.elapsed() < Duration::from_millis(2_500), "held past the deadline: {:?}", started.elapsed());
        let pid: u32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        let gone = (0..20).any(|_| {
            no_longer_runs(pid) || {
                std::thread::sleep(Duration::from_millis(50));
                false
            }
        });
        assert!(gone, "the grandchild {pid} outlived the deadline");
    }

    /// A process that opens its own session escapes the group and holds the
    /// output open; even so the function returns right after the deadline,
    /// without waiting for it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_outside_the_group_does_not_hold_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let outcome = run_shell_with_deadline("setsid sleep 5 & sleep 7", dir.path(), Duration::from_secs(1));
        assert!(matches!(outcome, ShellOutcome::TimedOut { .. }), "{outcome:?}");
        assert!(started.elapsed() < Duration::from_millis(2_500), "held past the deadline: {:?}", started.elapsed());
    }

}
