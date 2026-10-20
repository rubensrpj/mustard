//! Execute a supported configuration reader against masked bytes in memory.
//! The caller's shell owns sequencing, pipes, redirects and permissions.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mustard_core::io::workspace::{is_git_repo_root, linked_worktree_main};

use crate::shared::config_key;

/// Closed argument grammar: no recursive search, additional files, scripts,
/// pattern files, follow mode or executable options. The original file is
/// replaced by stdin; grep's explicit filename label is preserved.
pub(crate) fn supported(reader: &str, args: &[String]) -> bool {
    let (flags, values, positional) = match reader {
        "grep" | "egrep" | "fgrep" => ("EFGivwnxcoqshHaIUzZ", "emABC", true),
        "cat" => ("AbEenstTv", "", false),
        "head" | "tail" => ("q", "nc", false),
        _ => return false,
    };
    let mut plain = false;
    let mut patterns = 0;
    let mut explicit_pattern = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if plain || !arg.starts_with('-') || arg == "-" {
            patterns += 1;
            continue;
        }
        if arg == "--" { plain = true; continue; }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, attached) = long.split_once('=').map_or((long, false), |(name, _)| (name, true));
            let value = match (reader, name) {
                ("grep" | "egrep" | "fgrep", "regexp" | "max-count" | "after-context" | "before-context" | "context") => true,
                ("head" | "tail", "lines" | "bytes") => true,
                ("grep" | "egrep" | "fgrep", "extended-regexp" | "fixed-strings" | "basic-regexp" | "ignore-case" | "invert-match" | "word-regexp" | "line-regexp" | "count" | "only-matching" | "quiet" | "silent" | "no-messages" | "no-filename" | "with-filename" | "line-number" | "text" | "binary" | "null" | "null-data") => false,
                ("cat", "show-all" | "number-nonblank" | "show-ends" | "number" | "squeeze-blank" | "show-tabs" | "show-nonprinting") => false,
                ("head" | "tail", "quiet" | "silent") => false,
                _ => return false,
            };
            if name == "regexp" { explicit_pattern = true; }
            if (value && !attached && iter.next().is_none()) || (!value && attached) { return false; }
            continue;
        }
        let short = &arg[1..];
        if matches!(reader, "head" | "tail") && short.bytes().all(|b| b.is_ascii_digit()) { continue; }
        let mut chars = short.chars().peekable();
        while let Some(c) = chars.next() {
            if values.contains(c) {
                if c == 'e' { explicit_pattern = true; }
                if chars.peek().is_none() && iter.next().is_none() { return false; }
                break;
            }
            if !flags.contains(c) { return false; }
        }
    }
    patterns == usize::from(positional && !explicit_pattern)
}

pub fn run(file: &Path, reader: &str, args_json: &str) {
    let code = execute(file, reader, args_json).unwrap_or_else(|reason| {
        eprintln!("[Mustard] config-read: {reason}");
        2
    });
    std::process::exit(code);
}

/// Where `file` is looked for, in order: the path as given and, when it sits
/// in a linked git worktree such as a wave's separate copy, the same place in
/// the main checkout. The configuration stays out of git, so a copy has none
/// of its own, and the one the session follows is the main checkout's.
fn lookup_places(file: &Path) -> Vec<PathBuf> {
    let given = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let main = given.parent().and_then(|folder| folder.canonicalize().ok()).and_then(|folder| {
        let top = folder.ancestors().find(|above| is_git_repo_root(above))?;
        let main = linked_worktree_main(top)?;
        Some(main.join(folder.strip_prefix(top).ok()?).join(given.file_name()?))
    });
    std::iter::once(given).chain(main).collect()
}

/// The first of `places` that exists, opened. A file there that cannot be
/// opened stops the search; when none exists, the answer names every place.
fn open(places: &[PathBuf]) -> Result<(std::fs::File, &Path), String> {
    for place in places {
        match std::fs::File::open(place) {
            Ok(handle) => return Ok((handle, place)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(format!("configuration could not be opened: {}", place.display())),
        }
    }
    let looked: Vec<String> = places.iter().map(|place| place.display().to_string()).collect();
    Err(format!("configuration not found; looked in {}", looked.join(", ")))
}

fn execute(file: &Path, reader: &str, args_json: &str) -> Result<i32, String> {
    let args: Vec<String> = serde_json::from_str(args_json).map_err(|_| "invalid reader arguments")?;
    if !config_key::is_config_file(&file.to_string_lossy()) || !supported(reader, &args) {
        return Err("unsupported configuration reader; no command executed".into());
    }
    // Read again at execution time, never a cached copy or a secret-bearing
    // temporary file. Preserve original formatting and line numbers.
    const LIMIT: u64 = 1_048_576;
    let places = lookup_places(file);
    let (handle, place) = open(&places)?;
    if place != places[0].as_path() {
        eprintln!("[Mustard] config-read: {} is missing in this copy; read {}, the main checkout's", file.display(), place.display());
    }
    let mut text = String::new();
    handle.take(LIMIT + 1).read_to_string(&mut text).map_err(|_| "configuration could not be read")?;
    if text.len() as u64 > LIMIT { return Err("configuration exceeds safe read size".into()); }
    let text = config_key::masked(&text).unwrap_or(text);
    if reader == "cat" && args.is_empty() {
        if let Err(error) = std::io::stdout().write_all(text.as_bytes()) {
            if error.kind() == std::io::ErrorKind::BrokenPipe { return Ok(141); }
            return Err("output could not be written".into());
        }
        return Ok(0);
    }
    let mut command = Command::new(reader);
    if matches!(reader, "grep" | "egrep" | "fgrep") {
        command.arg(format!("--label={}", file.display()));
    }
    // The input goes without a file name: the `head` of macOS takes `-` for a
    // file that does not exist, and every supported reader reads its input
    // when no file is named.
    let mut child = command.args(&args).stdin(Stdio::piped())
        .spawn().map_err(|_| "configuration reader could not be started")?;
    let written = child.stdin.take().ok_or("reader stdin unavailable")?.write_all(text.as_bytes());
    let status = child.wait().map_err(|_| "configuration reader did not return")?;
    // grep -q / head may finish without consuming all input.
    if written.is_err_and(|error| error.kind() != std::io::ErrorKind::BrokenPipe) {
        return Err("reader input could not be written".into());
    }
    #[cfg(unix)]
    { use std::os::unix::process::ExitStatusExt; Ok(status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(1))) }
    #[cfg(not(unix))]
    { Ok(status.code().unwrap_or(1)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_readers_have_a_closed_read_only_grammar() {
        for (reader, args, ok) in [
            ("grep", vec!["-n", "-E", "flow|protected"], true),
            ("grep", vec!["-H", "-e", "key", "-ejev"], true),
            ("grep", vec!["--", "--roots"], true),
            ("grep", vec!["-f", "secret.txt"], false),
            ("grep", vec!["-r", "key"], false),
            ("grep", vec!["key", "another.json"], false),
            ("grep", vec!["--label=other", "key"], false),
            ("cat", vec!["-n"], true),
            ("cat", vec!["another.json"], false),
            ("head", vec!["-n5"], true),
            ("tail", vec!["-f"], false),
            ("sed", vec!["-n", "1p"], false),
            ("awk", vec!["{system(\"echo hi\")}"], false),
        ] {
            assert_eq!(supported(reader, &args.into_iter().map(String::from).collect::<Vec<_>>()), ok, "{reader}");
        }
    }
}
