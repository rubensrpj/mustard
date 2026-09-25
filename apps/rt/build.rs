use std::path::Path;
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
    embed_shipped_hooks_manifest();
}

/// The hook manifest the plugin ships, relative to this crate: `plugin/`
/// sits at the repository root.
const SHIPPED_HOOKS_MANIFEST: &str = "../../plugin/hooks/hooks.json";

/// Copy the plugin's hook manifest into `OUT_DIR`, where the doctor embeds it
/// from (`include_str!` of `$OUT_DIR/hooks.json`).
///
/// Embedding it straight from `plugin/` made cargo record the file by its
/// absolute path — the path of the copy of the project that compiled it. The
/// build folder is shared between copies: the next copy saw another path, or a
/// missing file once the old copy was gone, and recompiled the crate. A file
/// under `OUT_DIR` is recorded relative to the build folder, and the watch
/// printed here is relative to this crate, so both name the copy being built.
///
/// Outside cargo — a test compiles and runs this script alone — there is no
/// `OUT_DIR` to fill, and the step is skipped.
fn embed_shipped_hooks_manifest() {
    let (Some(out_dir), Some(crate_root)) = (std::env::var_os("OUT_DIR"), std::env::var_os("CARGO_MANIFEST_DIR")) else {
        return;
    };
    let source = Path::new(&crate_root).join(SHIPPED_HOOKS_MANIFEST);
    let body = std::fs::read(&source).unwrap_or_else(|e| panic!("cannot read {}: {e}", source.display()));
    let target = Path::new(&out_dir).join("hooks.json");
    std::fs::write(&target, body).unwrap_or_else(|e| panic!("cannot write {}: {e}", target.display()));
    println!("cargo:rerun-if-changed={SHIPPED_HOOKS_MANIFEST}");
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
    println!("cargo:rerun-if-env-changed=MUSTARD_GIT_HASH");
    println!("cargo:rerun-if-env-changed=MUSTARD_GIT_DIRTY");
    println!("cargo:rerun-if-env-changed=MUSTARD_GIT_DATE");
    rerun_if_git_head_changed();
}

/// `(short_hash, dirty, commit_date)` from git, or `None` if git is
/// unavailable / this is not a repo. `commit_date` falls back to the build date
/// when the commit date can't be read but the hash can.
///
/// The Linux package (`packaging/linux/build-deb.sh`) copies the source tree
/// into a build area WITHOUT `.git` before compiling, so the `git` calls below
/// would find nothing there. That script reads the commit, the dirty flag and
/// the date from the ORIGINAL repo, before the copy, and hands them over as
/// `MUSTARD_GIT_HASH` / `MUSTARD_GIT_DIRTY` / `MUSTARD_GIT_DATE` — read first,
/// here; without them, this falls back to `git` like before.
fn git_describe() -> Option<(String, bool, String)> {
    if let Some(hash) = env_var("MUSTARD_GIT_HASH") {
        let dirty = env_var("MUSTARD_GIT_DIRTY").is_some();
        let date = env_var("MUSTARD_GIT_DATE").unwrap_or_else(build_date);
        return Some((hash, dirty, date));
    }

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
///
/// The watched paths are printed relative to this crate. Cargo keeps a watched
/// path as printed, and the build folder is shared between copies of the
/// project: an absolute path names the copy that ran this script, so the next
/// copy sees another path — or a missing file, once that copy is gone — and
/// recompiles this crate.
///
/// In a linked worktree (`git worktree add`, the kind of working copy made for
/// each batch of work) the git folder lives outside the checkout, under a name
/// of its own, so no path to it reads the same from two copies: there the
/// watches are skipped. Such a copy is born at one commit and is gone before
/// the next; the next copy brings
/// files newer than the last build, `build.rs` among them, and that recompiles
/// and re-runs this script, which stamps the new commit.
fn rerun_if_git_head_changed() {
    let (Some(git_dir), Some(top)) = (git(&["rev-parse", "--absolute-git-dir"]), git(&["rev-parse", "--show-toplevel"]))
    else {
        return;
    };
    let Some(crate_root) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return;
    };
    let Some(git_dir) = relative_inside(Path::new(&crate_root), Path::new(&top), Path::new(&git_dir)) else {
        return;
    };
    println!("cargo:rerun-if-changed={git_dir}/HEAD");
    // The packed/loose ref HEAD points at (e.g. refs/heads/<branch>) — its
    // change is what actually moves the commit on a normal `git commit`.
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed={git_dir}/{reference}");
    }
}

/// `target` as a path relative to `from`, when both sit inside `top`; `None`
/// when `target` lies outside it, as the git folder of a linked worktree does.
fn relative_inside(from: &Path, top: &Path, target: &Path) -> Option<String> {
    let top = top.canonicalize().ok()?;
    let up = from.canonicalize().ok()?.strip_prefix(&top).ok()?.components().count();
    let target = target.canonicalize().ok()?;
    let down = target.strip_prefix(&top).ok()?;
    Some(format!("{}{}", "../".repeat(up), down.to_string_lossy().replace('\\', "/")))
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
