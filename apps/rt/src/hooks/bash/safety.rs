//! `safety` — the Rust layer of the destructive-ops law (`BG01`–`BG13`).
//!
//! The law lives in TWO layers, intentionally redundant (see
//! `pipeline-config.md § Destructive-ops Law`):
//!
//! 1. **`settings.json permissions.deny`** — the config-level FIRST line.
//!    Every rule keeps its canonical spelling there (`Bash(git reset
//!    --hard:*)`, `Bash(mkfs*)`, `Bash(rm -rf:*)`, …). Native prefix matching
//!    (`:*` / trailing ` *`) enforces a word boundary — `git push
//!    --force-with-lease` is NOT caught by `Bash(git push --force:*)` — and
//!    this layer survives `/unhook`. But a deny glob is START-ANCHORED: any
//!    wrapper prefix (`rtk git reset --hard`, `sudo shutdown -h now`) slides
//!    the canonical spelling off the anchor and the glob no longer sees it —
//!    and this harness's own golden rule prefixes every Bash command with
//!    `rtk`, making the wrapped spelling the common case, not a corner case.
//! 2. **This table** — the full rule set with the historical substring /
//!    word-pair semantics (`lex::has_word_pair` / `lex::has_word` match
//!    anywhere in the string), wrapper-prefix insensitive by construction; it
//!    also expresses what a glob structurally cannot (flag clusters, flag
//!    reordering, character classes). IDs keep their historical `BGnn` names
//!    so deny reasons stay greppable.

use mustard_core::domain::model::contract::Verdict;

use super::lex::{ends_with_token_seq, has_word, has_word_pair, split_after, truncate};

/// One dangerous-command rule: a substring/structural test plus the user
/// message.
struct DangerRule {
    /// Stable identifier (`BG01`–`BG13`).
    id: &'static str,
    /// `true` when `cmd` (already lowercased) matches this rule.
    test: fn(&str) -> bool,
    /// The user-facing reason fragment.
    msg: &'static str,
}

/// The dangerous-command rules, in historical `bash-safety.js` order.
const DANGER_RULES: &[DangerRule] = &[
    // `-\w*r\w*f` flag CLUSTERS (`rm -rvf`, `-fR`) and flag order cannot be
    // covered by a finite set of word-boundary glob prefixes.
    DangerRule {
        id: "BG01",
        test: is_rm_recursive_force,
        msg: "Recursive force delete blocked",
    },
    // The deny layer start-anchors, so reordered (`git push origin -f`) and
    // clustered (`-uf`) spellings escape it; the token scan here catches them
    // while keeping `--force-with-lease` allowed.
    DangerRule {
        id: "BG02",
        test: is_force_push,
        msg: "Force push blocked (use --force-with-lease for safer overwrite)",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG03",
        test: |c| has_word_pair(c, "git", "reset") && c.contains("--hard"),
        msg: "git reset --hard blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG04",
        test: is_git_clean_force,
        msg: "git clean -f blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG05",
        test: |c| ends_with_token_seq(c, &["git", "checkout", "--", "."]),
        msg: "git checkout -- . blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG06",
        test: |c| ends_with_token_seq(c, &["git", "restore", "."]),
        msg: "git restore . blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG07",
        test: is_branch_delete_main,
        msg: "Deleting main/master branch blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG08",
        test: |c| has_word_pair(c, "chmod", "777"),
        msg: "chmod 777 blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG09",
        test: |c| has_word(c, "mkfs"),
        msg: "mkfs blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG10",
        test: |c| has_word_pair(c, "dd", "if="),
        msg: "dd if= blocked",
    },
    // The drive-letter character class (`[a-z]:`) has no native-pattern
    // equivalent (a glob cannot say "any single letter").
    DangerRule {
        id: "BG11",
        test: is_format_drive,
        msg: "format drive blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG12",
        test: |c| has_word(c, "shutdown"),
        msg: "shutdown blocked",
    },
    // Wrapper-prefix insensitivity is structural (glob is start-anchored).
    DangerRule {
        id: "BG13",
        test: |c| has_word(c, "reboot"),
        msg: "reboot blocked",
    },
];

/// `\brm\s+(-\w*r\w*f|--no-preserve-root|-rf|-fr)\b` — `rm` followed
/// by a flag token that means recursive+force.
fn is_rm_recursive_force(cmd: &str) -> bool {
    for word in split_after(cmd, "rm") {
        if word == "--no-preserve-root" {
            return true;
        }
        if let Some(flag) = word.strip_prefix('-') {
            if flag.starts_with("--") {
                continue;
            }
            // -rf / -fr / -Rf / a flag cluster containing both r and f.
            let has_r = flag.contains('r') || flag.contains('R');
            let has_f = flag.contains('f');
            if has_r && has_f {
                return true;
            }
        }
    }
    false
}

/// `\bgit\s+push\s+(-\w*f\b|--force(?!-with-lease))\b`.
fn is_force_push(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "push") {
        return false;
    }
    for word in cmd.split_whitespace() {
        if word == "--force" {
            return true;
        }
        if word.starts_with("--force-with-lease") {
            // Explicitly the safe form — not a force-push for this rule.
            continue;
        }
        if let Some(flag) = word.strip_prefix('-') {
            if !flag.starts_with('-') && flag.contains('f') {
                return true;
            }
        }
    }
    false
}

/// `\bgit\s+clean\s+-f` — `git clean` with a flag token containing `f`.
fn is_git_clean_force(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "clean") {
        return false;
    }
    cmd.split_whitespace().any(|w| {
        w.strip_prefix('-')
            .is_some_and(|f| !f.starts_with('-') && f.contains('f'))
    })
}

/// `\bgit\s+branch\s+-D\s+(main|master)\b`.
fn is_branch_delete_main(cmd: &str) -> bool {
    if !has_word_pair(cmd, "git", "branch") {
        return false;
    }
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    tokens.windows(2).any(|w| {
        (w[0] == "-d" || w[0] == "-D") && (w[1] == "main" || w[1] == "master")
    })
}

/// `\bformat\s+[A-Z]:` — `format` followed by a drive letter and `:`.
/// The JS regex was case-insensitive on `format` but matched the drive class
/// `[A-Z]` against the *original* command; this port lowercases the command
/// first, so the drive letter is matched lowercased — `format c:` still
/// matches, which is the intended behaviour.
fn is_format_drive(cmd: &str) -> bool {
    for word in split_after(cmd, "format") {
        let bytes = word.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return true;
        }
    }
    false
}

/// The `bash-safety` gate: deny when any rule matches.
pub(super) fn bash_safety(cmd: &str) -> Option<Verdict> {
    let lower = cmd.to_ascii_lowercase();
    for rule in DANGER_RULES {
        if (rule.test)(&lower) {
            return Some(Verdict::Deny {
                reason: format!(
                    "[bash-safety {}] {}.\nCommand: {}",
                    rule.id,
                    rule.msg,
                    truncate(cmd, 120)
                ),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assert `cmd` is denied and the reason carries the rule id.
    fn assert_denied(cmd: &str, id: &str) {
        match bash_safety(cmd) {
            Some(Verdict::Deny { reason }) => {
                assert!(reason.contains(id), "{cmd:?}: reason missing {id}: {reason}");
            }
            other => panic!("{cmd:?}: expected Deny({id}), got {other:?}"),
        }
    }

    // --- BG01: rm recursive+force — clusters and order -----------------------

    #[test]
    fn bg01_blocks_rm_recursive_force_in_every_spelling() {
        assert_denied("rm -rf /", "BG01");
        assert_denied("rm -fr /tmp/work", "BG01");
        assert_denied("rm -Rf src", "BG01");
        // The flag CLUSTER — the spelling the deny globs cannot express.
        assert_denied("rm -rvf build/", "BG01");
        assert_denied("rm --no-preserve-root /", "BG01");
    }

    #[test]
    fn bg01_allows_plain_rm() {
        assert!(bash_safety("rm file.txt").is_none());
        assert!(
            bash_safety("rm -r dir/").is_none(),
            "recursive without force passes the table"
        );
    }

    // --- BG02: force push — reordering, clusters, lease carve-out ------------

    #[test]
    fn bg02_blocks_force_push_variants() {
        // Canonical (also covered config-level by permissions.deny).
        assert_denied("git push --force origin main", "BG02");
        // Reordered flag — the spelling only the token scan catches.
        assert_denied("git push origin main --force", "BG02");
        assert_denied("git push origin -f", "BG02");
        // Short-flag cluster.
        assert_denied("git push -uf origin dev", "BG02");
        assert_denied("git push -f", "BG02");
    }

    /// PROOF item: `--force-with-lease` (and `--force-if-includes`) pass —
    /// the product allows the safe overwrite forms.
    #[test]
    fn bg02_allows_force_with_lease_and_safe_push() {
        assert!(bash_safety("git push --force-with-lease origin dev").is_none());
        assert!(bash_safety("git push --force-with-lease=origin/dev origin dev").is_none());
        assert!(bash_safety("git push --force-if-includes --force-with-lease origin dev").is_none());
        assert!(bash_safety("git push origin dev").is_none());
    }

    // --- BG11: format drive — character class --------------------------------

    #[test]
    fn bg11_blocks_format_drive_letter() {
        assert_denied("format c:", "BG11");
        assert_denied("format D: /q", "BG11");
        assert!(bash_safety("format").is_none());
        assert!(bash_safety("npm run format src/").is_none());
    }

    // --- restored rules: BG03–BG10, BG12, BG13 -------------------------------

    /// Every restored rule denies its canonical spelling (adapted from the
    /// pre-split `safety_regression_all_bg_rules`).
    #[test]
    fn restored_rules_deny_canonical_spellings() {
        for (id, cmd) in [
            ("BG03", "git reset --hard HEAD~1"),
            ("BG04", "git clean -fd"),
            ("BG05", "git checkout -- ."),
            ("BG06", "git restore ."),
            ("BG07", "git branch -D main"),
            ("BG07", "git branch -D master"),
            ("BG08", "chmod 777 /etc/passwd"),
            ("BG09", "mkfs.ext4 /dev/sda1"),
            ("BG10", "dd if=/dev/zero of=/dev/sda"),
            ("BG12", "shutdown -h now"),
            ("BG13", "reboot"),
        ] {
            assert_denied(cmd, id);
        }
    }

    /// THE reason the ten rules are back in Rust: a deny glob is
    /// start-anchored, so a wrapper prefix (`rtk …` — this harness's own
    /// golden rule — or `sudo …`) slides the canonical spelling off the
    /// anchor and the config layer no longer sees it. The substring scan here
    /// must deny the wrapped spellings.
    #[test]
    fn restored_rules_deny_wrapped_spellings() {
        for (id, cmd) in [
            ("BG03", "rtk git reset --hard HEAD~1"),
            ("BG04", "rtk git clean -fd"),
            ("BG05", "rtk git checkout -- ."),
            ("BG06", "rtk git restore ."),
            ("BG07", "rtk git branch -D main"),
            ("BG08", "sudo chmod 777 /etc/passwd"),
            ("BG09", "sudo mkfs.ext4 /dev/sda1"),
            ("BG10", "sudo dd if=/dev/zero of=/dev/sda"),
            ("BG12", "sudo shutdown -h now"),
            ("BG13", "sudo reboot"),
        ] {
            assert_denied(cmd, id);
        }
    }

    /// The carve-outs the substring semantics preserve: near-miss spellings
    /// that are NOT destructive stay allowed.
    #[test]
    fn restored_rules_allow_safe_variants() {
        for safe in [
            "git reset --soft HEAD~1",  // BG03 is --hard only
            "git clean -n",             // BG04 needs an -f flag
            "git checkout -- src/a.rs", // BG05 is the bare `.` wipe only
            "git restore src/a.rs",     // BG06 is the bare `.` wipe only
            "git branch -D feature-x",  // BG07 protects main/master only
            "chmod 755 script.sh",      // BG08 is 777 only
        ] {
            assert!(
                bash_safety(safe).is_none(),
                "{safe:?} must pass the safety table"
            );
        }
    }
}
