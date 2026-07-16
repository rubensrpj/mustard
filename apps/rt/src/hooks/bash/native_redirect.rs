//! `native_redirect` — deny / advise native-tool equivalents for shell reads.
//!
//! Port of `bash-native-redirect.js`, with ONE intentional divergence from
//! the 1:1 port: a read of ONE explicit file — `head -20 file.txt`,
//! `grep foo file.txt`, `cat src/main.ts` — is allowed (not nudged) because
//! slicing a single captured file is the CLAUDE.md "capture to a file, slice
//! the file" idiom, not a tree scan a native tool does better. The nudge
//! still fires for tree scans (`grep -r`, globs, directories). See
//! [`redirect_targets_single_file`].

use mustard_core::domain::model::contract::Verdict;

use super::lex::{is_cmd_separator, mask_quoted_operators, split_after, strip_leading_rtk};

/// The redirect map from `bash-native-redirect.js`: command → (native tool,
/// tip). Order is irrelevant — lookup is by first token.
const REDIRECT_MAP: &[(&str, &str, &str)] = &[
    ("grep", "Grep", "Grep(pattern, path, output_mode) — faster, no shell overhead"),
    ("rg", "Grep", "Grep tool is built on ripgrep — same power, structured output"),
    ("egrep", "Grep", "Grep(pattern) supports full regex syntax"),
    ("fgrep", "Grep", "Grep(pattern, -i) for case-insensitive search"),
    ("cat", "Read", "Read(file_path) — structured output with line numbers"),
    ("head", "Read", "Read(file_path, limit: N) — reads first N lines"),
    ("tail", "Read", "Read(file_path, offset: N) — reads from line N"),
    ("less", "Read", "Read(file_path, offset, limit) — paginated reading"),
    ("more", "Read", "Read(file_path) — full file reading"),
    ("ls", "Glob", "Glob(pattern) — e.g. \"src/**/*.ts\" for recursive listing"),
    ("find", "Glob", "Glob(pattern) — e.g. \"**/*.cs\" for pattern matching"),
    ("tree", "Glob", "Glob(pattern) — structured file listing by pattern"),
];

/// Look up the redirect target for a (lowercased) first token.
fn redirect_for(token: &str) -> Option<(&'static str, &'static str)> {
    REDIRECT_MAP
        .iter()
        .find(|(name, _, _)| *name == token)
        .map(|(_, tool, tip)| (*tool, *tip))
}

/// Read-only search commands that `rtk` does **not** filter — it execs the
/// bare binary, which may be absent (e.g. `rg` on Windows → exit 127). These
/// are redirected to the native Grep tool *even when prefixed with `rtk`*,
/// unlike `rtk grep` / `rtk cat` (which `rtk` filters and which exist on this
/// platform — those keep passing through). User decision 2026-05-21.
const RTK_TRANSPARENT_REDIRECT: &[&str] = &["rg", "egrep", "fgrep"];

/// Shell operator that marks a composed command: `[|&;]`, `$(`, backtick, `<<`, `>>`.
fn has_shell_operator(cmd: &str) -> bool {
    cmd.contains('|')
        || cmd.contains('&')
        || cmd.contains(';')
        || cmd.contains("$(")
        || cmd.contains('`')
        || cmd.contains("<<")
        || cmd.contains(">>")
}

/// `(?<![<>])>(?![>])` — a lone `>` output redirect (writing to a file).
/// `>>` and `2>` are explicitly *not* this. Mirrors `OUTPUT_REDIRECT_RE`
/// applied after `stripStderrRedirects`.
fn has_output_redirect(cmd: &str) -> bool {
    let bytes = cmd.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'>' {
            continue;
        }
        let prev = i.checked_sub(1).map(|p| bytes[p]);
        let next = bytes.get(i + 1).copied();
        if prev != Some(b'<') && prev != Some(b'>') && next != Some(b'>') {
            return true;
        }
    }
    false
}

/// Strip trailing `2>/dev/null` / `2>&1` — `stripStderrRedirects`.
fn strip_stderr_redirects(cmd: &str) -> String {
    let trimmed = cmd.trim();
    for suffix in ["2>/dev/null", "2>&1"] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            return stripped.trim_end().to_string();
        }
    }
    trimmed.to_string()
}

/// First executable token, skipping `VAR=value` env-prefix tokens.
/// Mirrors `firstToken` in `bash-native-redirect.js`.
fn first_token(cmd: &str) -> Option<&str> {
    cmd.split_whitespace().find(|tok| !tok.contains('='))
}

/// `\bsed\s+(-\w*i\w*|-i\b)` — a `sed` invocation with an in-place flag.
fn is_sed_in_place(cmd: &str) -> bool {
    for word in split_after(cmd, "sed") {
        if let Some(flag) = word.strip_prefix('-') {
            if !flag.starts_with('-') && flag.contains('i') {
                return true;
            }
        }
    }
    false
}

/// A path operand that names a *directory / tree* by its shape alone: a
/// trailing `/`, or the `.`/`..` current/parent dir. Used by
/// [`redirect_targets_single_file`] to keep the native-tool nudge on a
/// directory scan.
fn is_dir_shaped(tok: &str) -> bool {
    tok == "." || tok == ".." || tok.ends_with('/')
}

/// A path operand that names *one explicit file* by its shape alone: a path
/// segment (`src/main.ts` — a `/` with no trailing slash) or a bare name that
/// carries an extension dot (`file.txt`, `app.log`). A bare word with neither a
/// dot nor a slash (a grep/rg *pattern*, or an extension-less name) is neither
/// file- nor directory-shaped, so on its own it does not make a command
/// single-file — the file operand elsewhere on the line is what does.
fn is_file_shaped(tok: &str) -> bool {
    if tok.ends_with('/') {
        return false;
    }
    tok.contains('/') || tok.contains('.')
}

/// Whether a native-redirectable command targets ONE explicit file, decided by
/// argument FORM only (no filesystem probe — deterministic and cheap).
///
/// True when the command carries **no** tree signal — no recursion flag
/// (`-r`/`-R`/`--recursive`, including a short cluster like `-rn`), no glob
/// metacharacter (`*`, `?`, `[`) in any operand, no directory-shaped operand
/// ([`is_dir_shaped`]) — **and** at least one file-shaped operand
/// ([`is_file_shaped`]). So `grep foo bar.txt`, `head -20 file.txt`,
/// `cat src/main.ts` → true; `grep -r foo src/`, `grep foo *.rs`, `cat dir/`
/// and bare `ls` (no file target at all) → false.
fn redirect_targets_single_file(cmd: &str) -> bool {
    let mut has_file = false;
    let mut seen_cmd = false;
    for tok in cmd.split_whitespace() {
        if !seen_cmd {
            // Skip leading `VAR=value` env-prefix tokens; the first bare token
            // is the command name itself (mirrors [`first_token`]).
            if tok.contains('=') {
                continue;
            }
            seen_cmd = true;
            continue;
        }
        if let Some(flag) = tok.strip_prefix('-') {
            // Recursion turns a read into a tree walk → keep the nudge. Matches
            // `-r`/`-R`/`--recursive`, or a short flag cluster containing r/R
            // (`-rn`). Long `--…` flags other than `--recursive` are neutral.
            let is_recursive = tok == "--recursive"
                || (!flag.starts_with('-')
                    && flag.bytes().any(|b| b == b'r' || b == b'R'));
            if is_recursive {
                return false;
            }
            continue;
        }
        // Non-flag operand.
        if tok.contains('*') || tok.contains('?') || tok.contains('[') {
            return false; // a glob fans out over many paths
        }
        if is_dir_shaped(tok) {
            return false; // a directory is a tree, not one file
        }
        if is_file_shaped(tok) {
            has_file = true;
        }
    }
    has_file
}

/// The `bash-native-redirect` gate. Returns the verdict for a command, or
/// `None` to fall through to the next gate.
///
/// Verdicts (1:1 with `bash-native-redirect.js`, except the single-file
/// `Allow` — see module doc):
/// - output redirect / `rtk` prefix / non-mapped command → `None` (pass).
/// - piped/chained with a redirectable first segment → `Inject` (advisory).
/// - read of ONE explicit file (`grep foo file.txt`, `cat main.rs`) → `Allow`
///   (nudge silenced; [`redirect_targets_single_file`]).
/// - tree scan `grep -r`/glob/dir, or read-only `sed`, or no clear file
///   target → `Deny`.
pub(super) fn bash_native_redirect(raw_cmd: &str) -> Option<Verdict> {
    let cmd = strip_stderr_redirects(raw_cmd);
    if cmd.is_empty() {
        return None;
    }

    // Mask operators inside quotes so a quoted Grep pattern like
    // `"emit-pipeline|emit-phase"` is not mistaken for a shell pipe. Operator
    // and segment-boundary detection runs on the masked view; command names
    // and flags (never quoted) are unchanged, so token lookups still work.
    let masked = mask_quoted_operators(&cmd);

    // Output redirect — writing a file, not reading. Pass through.
    if has_output_redirect(&masked) {
        return None;
    }

    // Piped/chained: cannot deny safely. If the first segment is a
    // redirectable command, advise via `Inject`; otherwise pass.
    if has_shell_operator(&masked) {
        let first_segment = masked
            .split(is_cmd_separator)
            .next()
            .unwrap_or("")
            .trim();
        if let Some(seg_token) = first_token(first_segment) {
            // See through a leading `rtk` only for the ripgrep family (which
            // rtk does not filter); other `rtk`-prefixed first segments pass.
            let effective = if seg_token == "rtk" {
                first_token(strip_leading_rtk(first_segment)).unwrap_or("")
            } else {
                seg_token
            };
            let effective_lc = effective.to_ascii_lowercase();
            let advisable = effective != "rtk"
                && (seg_token != "rtk"
                    || RTK_TRANSPARENT_REDIRECT.contains(&effective_lc.as_str()));
            if advisable {
                if let Some((tool, tip)) = redirect_for(&effective_lc) {
                    return Some(Verdict::Inject {
                        context: format!(
                            "[Native Tool Redirect] The `{effective}` part of this piped \
                             command could use the {tool} tool instead. {tip}. Consider \
                             splitting the pipeline to use native tools where possible."
                        ),
                    });
                }
            }
        }
        return None;
    }

    let token = first_token(&cmd)?;

    // RTK-wrapped: pass through, EXCEPT for the ripgrep family (`rg`/`egrep`/
    // `fgrep`) which rtk execs as a bare binary that may be absent. Redirect
    // those to the native Grep tool; everything else (`rtk grep`, `rtk cat`,
    // `rtk cargo …`) still passes.
    if token == "rtk" {
        let inner = first_token(strip_leading_rtk(&cmd)).unwrap_or("");
        let inner_lc = inner.to_ascii_lowercase();
        if RTK_TRANSPARENT_REDIRECT.contains(&inner_lc.as_str()) {
            if let Some((tool, tip)) = redirect_for(&inner_lc) {
                return Some(Verdict::Deny {
                    reason: format!(
                        "[Native Tool Redirect] Use the {tool} tool instead of `rtk {inner_lc}` \
                         in Bash ({inner_lc} is not installed / not filtered by rtk). {tip}"
                    ),
                });
            }
        }
        return None;
    }

    // `sed`: deny only read-only sed; `sed -i` is a write, pass through.
    if token == "sed" {
        if is_sed_in_place(&cmd) {
            return None;
        }
        return Some(Verdict::Deny {
            reason: "[Native Tool Redirect] Use the Grep tool instead of `sed` in Bash. \
                     Grep(pattern) — for pattern extraction without shell sed overhead"
                .to_string(),
        });
    }

    let token_lc = token.to_ascii_lowercase();
    let (tool, tip) = redirect_for(&token_lc)?;

    // Divergence from the 1:1 JS port (module doc): when the command slices
    // ONE explicit file (`head -20 captured.json`, `grep foo file.txt`), the
    // "use a native tool" nudge is noise — that is the CLAUDE.md "capture to a
    // file, slice the file" idiom. Allow it. The nudge stays for a tree scan
    // (`grep -r`, a glob, a directory) native tools do better, and when no file
    // target is clear (bare `ls`).
    if redirect_targets_single_file(&cmd) {
        return Some(Verdict::Allow);
    }

    Some(Verdict::Deny {
        reason: format!(
            "[Native Tool Redirect] Use the {tool} tool instead of `{token_lc}` in Bash. {tip}"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- bash-native-redirect parity (hooks.test.js "bash-native-redirect.js")

    #[test]
    fn redirect_denies_simple_grep_suggesting_grep() {
        match bash_native_redirect("grep -r pattern src/") {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("Grep")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn redirect_allows_single_file_cat() {
        // T4: `cat` of ONE explicit file is the "slice a captured file" idiom,
        // not a tree scan — the native-tool nudge is silenced (Allow).
        assert_eq!(bash_native_redirect("cat src/main.ts"), Some(Verdict::Allow));
    }

    #[test]
    fn redirect_denies_ls_suggesting_glob() {
        match bash_native_redirect("ls -la src/") {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("Glob")),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn redirect_denies_bare_ls() {
        match bash_native_redirect("ls") {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("Glob"), "reason: {reason}"),
            other => panic!("expected Deny for bare `ls`, got {other:?}"),
        }
    }

    #[test]
    fn redirect_allows_single_file_head_tail() {
        // T4: slicing ONE explicit file with head/tail is silenced.
        for cmd in ["head -20 file.txt", "tail -50 app.log"] {
            assert_eq!(
                bash_native_redirect(cmd),
                Some(Verdict::Allow),
                "expected silent for: {cmd}"
            );
        }
    }

    #[test]
    fn redirect_denies_find_tree_scan() {
        // `find` walks a directory tree → the Glob nudge stands.
        assert!(matches!(
            bash_native_redirect("find . -name '*.ts'"),
            Some(Verdict::Deny { .. })
        ));
    }

    /// T4 single-file vs tree heuristic (AC4): the nudge is silenced for a read
    /// of ONE explicit file (the "capture to a file, slice the file" idiom) and
    /// kept for a tree scan native tools do better.
    #[test]
    fn redirect_single_file_silent_tree_scan_denied() {
        // Single explicit file → silent (Allow). `grep foo bar.txt` — the `foo`
        // pattern is neither file- nor dir-shaped; the trailing file decides.
        for silent in ["grep foo bar.txt", "grep TODO src/main.rs", "head -20 file.txt"] {
            assert_eq!(
                bash_native_redirect(silent),
                Some(Verdict::Allow),
                "expected silent: {silent}"
            );
        }
        // Tree scan → nudge stands (Deny): recursion, glob, or a directory.
        for denied in ["grep -r foo src/", "grep foo *.rs", "cat dir/", "grep foo src/"] {
            assert!(
                matches!(bash_native_redirect(denied), Some(Verdict::Deny { .. })),
                "expected deny: {denied}"
            );
        }
    }

    #[test]
    fn redirect_piped_command_is_advisory_not_blocking() {
        // First segment `grep` is redirectable → Inject (advisory), not Deny.
        let v = bash_native_redirect("grep foo bar.txt | wc -l");
        assert!(matches!(v, Some(Verdict::Inject { .. })), "got {v:?}");
        let v = bash_native_redirect("grep foo bar.txt && echo found");
        assert!(matches!(v, Some(Verdict::Inject { .. })), "got {v:?}");
    }

    #[test]
    fn redirect_warns_on_piped_redirectable_first_segment() {
        match bash_native_redirect("grep foo bar.txt | sort | uniq") {
            Some(Verdict::Inject { context }) => {
                assert!(context.contains("Grep"));
                assert!(context.contains("Native Tool Redirect"));
            }
            other => panic!("expected Inject advisory, got {other:?}"),
        }
    }

    #[test]
    fn redirect_allows_rtk_prefixed() {
        // `rtk grep` keeps passing — rtk filters grep and the binary exists.
        assert!(bash_native_redirect("rtk grep -r pattern src/").is_none());
    }

    /// `rtk rg` must be redirected to the Grep tool: rtk does not filter `rg`,
    /// so it execs the bare binary, which may be absent (Windows → exit 127).
    /// Regression for the 2026-05-21 pipeline failure.
    #[test]
    fn redirect_denies_rtk_rg_suggesting_grep() {
        match bash_native_redirect("rtk rg -n pattern src/") {
            Some(Verdict::Deny { reason }) => {
                assert!(reason.contains("Grep"), "reason: {reason}");
                assert!(reason.contains("rg"), "reason should name rg: {reason}");
            }
            other => panic!("expected Deny for `rtk rg`, got {other:?}"),
        }
    }

    /// The original failing command: `rtk rg` with an alternation pattern whose
    /// `|` is inside quotes (must NOT be read as a shell pipe) plus a `2>&1`
    /// stderr redirect. Must still reach the ripgrep redirect → Deny.
    #[test]
    fn redirect_denies_rtk_rg_with_quoted_pipe_pattern() {
        let v = bash_native_redirect(
            r#"rtk rg -n "emit-pipeline|emit-phase|pipeline\.status" src/ 2>&1"#,
        );
        match v {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("Grep"), "reason: {reason}"),
            other => panic!("expected Deny for quoted-pipe `rtk rg`, got {other:?}"),
        }
    }

    /// A quoted operator must not be treated as a shell pipe: bare `grep` with
    /// an alternation pattern is a plain (non-piped) command → hard Deny, not
    /// the advisory Inject reserved for genuine pipelines.
    #[test]
    fn redirect_quoted_pipe_pattern_is_not_a_real_pipe() {
        let v = bash_native_redirect(r#"grep -n "foo|bar" src/"#);
        match v {
            Some(Verdict::Deny { reason }) => assert!(reason.contains("Grep"), "reason: {reason}"),
            other => panic!("expected hard Deny (quoted | is not a pipe), got {other:?}"),
        }
    }

    /// A genuine pipe is still detected (advisory Inject), even with a quoted
    /// operator earlier in the line.
    #[test]
    fn redirect_real_pipe_still_advisory() {
        let v = bash_native_redirect(r#"grep -n "foo|bar" src/ | sort"#);
        assert!(
            matches!(v, Some(Verdict::Inject { .. })),
            "real pipe must stay advisory, got {v:?}"
        );
    }

    #[test]
    fn redirect_passes_non_mapped_commands() {
        // A non-mapped command is not this gate's business → None (the
        // rtk-rewrite stage downstream decides what to do with it).
        assert!(bash_native_redirect("git status").is_none());
        assert!(bash_native_redirect("npm run build").is_none());
    }

    #[test]
    fn redirect_allows_sed_in_place() {
        assert!(bash_native_redirect("sed -i 's/old/new/g' file.txt").is_none());
    }

    #[test]
    fn redirect_allows_output_redirect() {
        assert!(bash_native_redirect("cat file.txt > output.txt").is_none());
    }

    #[test]
    fn redirect_handles_env_var_prefix() {
        // The `VAR=value` prefix is skipped when finding the command; the tree
        // scan `grep -r … src/` is still denied. (A single-file target such as
        // `grep pattern file.txt` is silenced by the T4 heuristic, so this
        // parity check uses a tree form to keep exercising the prefix skip.)
        assert!(matches!(
            bash_native_redirect("NODE_ENV=test grep -r pattern src/"),
            Some(Verdict::Deny { .. })
        ));
    }

    #[test]
    fn redirect_strips_stderr_redirect_before_analysis() {
        assert!(matches!(
            bash_native_redirect("grep pattern file 2>/dev/null"),
            Some(Verdict::Deny { .. })
        ));
    }

    #[test]
    fn redirect_denies_read_only_sed() {
        assert!(matches!(
            bash_native_redirect("sed -n '1,5p' file.txt"),
            Some(Verdict::Deny { .. })
        ));
    }
}
