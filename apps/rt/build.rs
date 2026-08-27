use std::process::Command;

fn main() {
    // Windows defaults the main-thread stack to 1 MiB, which is too small for
    // the debug build of the rt dispatcher (large monomorphized frames). Match
    // the POSIX default (8 MiB) so the binary does not stack-overflow on hook
    // dispatch. No-op on other targets.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }

    emit_version_full();
}

/// Emit `MUSTARD_VERSION_FULL` — the per-build version stamp the binary's clap
/// `--version` prints: `<semver> (build <N>, g<hash>[-dirty] <date>)`.
///
/// Sources: semver from `CARGO_PKG_VERSION`; build number from the
/// `MUSTARD_BUILD_NUMBER` env var (literal `dev` when absent, e.g. a plain
/// `cargo build`); short hash + dirty flag + commit date from git. Fail-open:
/// git missing or this not being a repo degrades to the semver alone — the
/// build must never panic.
fn emit_version_full() {
    let semver = env_var("CARGO_PKG_VERSION").unwrap_or_else(|| "0.0.0".to_string());
    let build = env_var("MUSTARD_BUILD_NUMBER").unwrap_or_else(|| "dev".to_string());

    let full = match git_describe() {
        Some((hash, dirty, date)) => {
            let dirty = if dirty { "-dirty" } else { "" };
            format!("{semver} (build {build}, g{hash}{dirty} {date})")
        }
        // Fail-open: no git / not a repo → just the semver, no git block.
        None => semver,
    };

    println!("cargo:rustc-env=MUSTARD_VERSION_FULL={full}");

    // Re-stamp when the build number changes or the checked-out commit moves.
    println!("cargo:rerun-if-env-changed=MUSTARD_BUILD_NUMBER");
    rerun_if_git_head_changed();
}

/// `(short_hash, dirty, commit_date)` from git, or `None` if git is
/// unavailable / this is not a repo. `commit_date` falls back to the build date
/// when the commit date can't be read but the hash can.
fn git_describe() -> Option<(String, bool, String)> {
    let hash = git(&["rev-parse", "--short=12", "HEAD"])?;
    // `--quiet` makes a clean tree exit 0 and a dirty tree exit 1; any other
    // failure (no git) also leaves us treating the tree as not-dirty.
    let dirty = match Command::new("git").args(["diff", "--quiet", "HEAD"]).status() {
        Ok(status) => !status.success(),
        Err(_) => false,
    };
    // Committer date, short ISO (YYYY-MM-DD). Fall back to the build date.
    let date = git(&["log", "-1", "--format=%cs"]).unwrap_or_else(build_date);
    Some((hash, dirty, date))
}

/// Tell cargo to re-run this script when the checked-out commit changes, so the
/// stamped hash/date stay current. Best-effort: a missing `.git` (no repo, or a
/// packaged source tree) just skips the watches.
fn rerun_if_git_head_changed() {
    // `git rev-parse --git-dir` resolves the real `.git` dir even from a
    // worktree or a nested crate. Watch HEAD and the ref it points at.
    let Some(git_dir) = git(&["rev-parse", "--git-dir"]) else {
        return;
    };
    let head = format!("{git_dir}/HEAD");
    println!("cargo:rerun-if-changed={head}");
    // The packed/loose ref HEAD points at (e.g. refs/heads/<branch>) — its
    // change is what actually moves the commit on a normal `git commit`.
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed={git_dir}/{reference}");
    }
}

/// Run a git command, returning trimmed stdout on a clean exit. `None` on any
/// failure (git absent, non-zero exit, non-UTF-8) — the caller degrades.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Today's date as `YYYY-MM-DD` (UTC), the fallback when the commit date can't
/// be read. Computed from the Unix epoch via the civil-from-days algorithm so
/// the build script pulls in no date crate.
fn build_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `civil_from_days`: days-since-1970-01-01 → (year, month,
/// day). Used only for the build-date fallback.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    // No cast: `z` and `era` are already `i64`. The `as` was a leftover from
    // Hinnant's C++ original, where this line changes type to `unsigned`.
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (y + i64::from(m <= 2), m, d)
}

/// `std::env::var` mapping any error (missing / non-UTF-8) — and a present-but-
/// empty value — to `None`, so a blank `MUSTARD_BUILD_NUMBER` (e.g. an env var a
/// caller restored to "") degrades to `dev` rather than stamping an empty token.
/// Mirrors the empty-is-none rule in `git`.
fn env_var(key: &str) -> Option<String> {
    let val = std::env::var(key).ok()?;
    if val.trim().is_empty() { None } else { Some(val) }
}
