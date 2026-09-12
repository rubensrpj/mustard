//! `safety` — the command guard: a command that destroys work with no way
//! back is refused.
//!
//! It reads the commands [`super::lex::segments`] found, never the raw text,
//! and looks only at the program and its options. A commit message, a title
//! or any other quoted text that merely names a dangerous command passes; what
//! the terminal would really run is judged, including the command inside
//! `$(…)` and the line handed to `bash -c "…"`.
//!
//! Seven dangers, all of them work lost for good: deleting a folder by force,
//! four ways of discarding changes (`git reset --hard`, `git clean -f`,
//! `git checkout -- .`, `git restore .`), deleting an integration branch and
//! force-pushing. The same danger spelled another way is the same danger
//! (`git -C dir reset --hard`, `rm -r -f`, `git push origin +main`, deleting
//! the branch on the server). Commands that harm the machine rather than the
//! work (`chmod 777`, `mkfs`, `dd`, `shutdown`, `reboot`) are not judged here:
//! the permission list of the Claude Code settings refuses them.
//!
//! The integration branches are the ones the project's `git.flow` names, read
//! from the hook context. A project that declares no flow has none, and no
//! branch name is written in this file.

use std::collections::BTreeSet;

use mustard_core::domain::model::contract::{Ctx, Verdict};
use mustard_core::{translate, SupportedLocale};

use super::lex::{truncate, Segment, Word};

/// What a command would destroy.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Danger {
    RecursiveForceDelete,
    ForcePush,
    ResetHard,
    CleanForce,
    CheckoutAll,
    RestoreAll,
    /// Deleting an integration branch, locally or on the server.
    DeleteBase(String),
}

impl Danger {
    /// The catalogue key of the reason.
    fn key(&self) -> &'static str {
        match self {
            Self::RecursiveForceDelete => "command_guard.rm_recursive_force",
            Self::ForcePush => "command_guard.force_push",
            Self::ResetHard => "command_guard.reset_hard",
            Self::CleanForce => "command_guard.clean_force",
            Self::CheckoutAll => "command_guard.checkout_all",
            Self::RestoreAll => "command_guard.restore_all",
            Self::DeleteBase(_) => "command_guard.delete_base",
        }
    }

    /// The branch an integration-branch deletion names.
    fn detail(&self) -> Option<&str> {
        match self {
            Self::DeleteBase(branch) => Some(branch),
            _ => None,
        }
    }

    fn reason(&self, lang: SupportedLocale) -> String {
        let text = translate(self.key(), lang);
        match self.detail() {
            Some(branch) => text.replace("{branch}", branch),
            None => text.to_string(),
        }
    }
}

/// One danger check over one command, given the integration branches.
type Rule = fn(&Segment, &BTreeSet<String>) -> Option<Danger>;

/// The checks, in order; the first one to answer wins. A new danger is one
/// more function here.
const RULES: &[Rule] = &[
    recursive_force_delete,
    force_push,
    reset_hard,
    clean_force,
    checkout_all,
    restore_all,
    delete_base,
];

/// `rm` with recursion and force, in any spelling: `-rf`, `-fr`, `-Rf`,
/// `-rvf`, `-r -f`, `--recursive --force`, or `--no-preserve-root`.
fn recursive_force_delete(seg: &Segment, _: &BTreeSet<String>) -> Option<Danger> {
    if seg.name() != "rm" {
        return None;
    }
    let (mut recursive, mut force) = (false, false);
    for option in options(&seg.args) {
        match option {
            "--no-preserve-root" => return Some(Danger::RecursiveForceDelete),
            "--recursive" => recursive = true,
            "--force" => force = true,
            _ => {
                if let Some(flags) = short_flags(option) {
                    recursive |= flags.contains(['r', 'R']);
                    force |= flags.contains('f');
                }
            }
        }
    }
    (recursive && force).then_some(Danger::RecursiveForceDelete)
}

/// `git push` with `--force`, a short group holding `f` (`-f`, `-uf`) or a
/// branch forced with `+` (`+main`). `--force-with-lease` and
/// `--force-if-includes` pass.
fn force_push(seg: &Segment, _: &BTreeSet<String>) -> Option<Danger> {
    let args = git_subcommand(seg, "push")?;
    let forced_option = options(args).any(|o| o == "--force" || short_flags(o).is_some_and(|f| f.contains('f')));
    let forced_branch = operands(args).any(|p| p.len() > 1 && p.starts_with('+'));
    (forced_option || forced_branch).then_some(Danger::ForcePush)
}

/// `git reset --hard`.
fn reset_hard(seg: &Segment, _: &BTreeSet<String>) -> Option<Danger> {
    let args = git_subcommand(seg, "reset")?;
    options(args).any(|o| o == "--hard").then_some(Danger::ResetHard)
}

/// `git clean` with `--force` or a short group holding `f`; `git clean -n`
/// only lists, and passes.
fn clean_force(seg: &Segment, _: &BTreeSet<String>) -> Option<Danger> {
    let args = git_subcommand(seg, "clean")?;
    options(args)
        .any(|o| o == "--force" || short_flags(o).is_some_and(|f| f.contains('f')))
        .then_some(Danger::CleanForce)
}

/// `git checkout` over the whole tree: the path `.`, after `--` or alone.
fn checkout_all(seg: &Segment, _: &BTreeSet<String>) -> Option<Danger> {
    let args = git_subcommand(seg, "checkout")?;
    operands(args).any(|p| p == ".").then_some(Danger::CheckoutAll)
}

/// `git restore` of the path `.` that touches the files: `--staged` alone
/// only takes the changes out of the index, and passes; with `--worktree` it
/// discards them.
fn restore_all(seg: &Segment, _: &BTreeSet<String>) -> Option<Danger> {
    let args = git_subcommand(seg, "restore")?;
    if !operands(args).any(|p| p == ".") {
        return None;
    }
    let (mut staged, mut worktree) = (false, false);
    for option in options(args) {
        match option {
            "--staged" => staged = true,
            "--worktree" => worktree = true,
            _ => {
                if let Some(flags) = short_flags(option) {
                    staged |= flags.contains('S');
                    worktree |= flags.contains('W');
                }
            }
        }
    }
    (!staged || worktree).then_some(Danger::RestoreAll)
}

/// Deleting a branch the project's `git.flow` names: `git branch` with `-d`,
/// `-D` or `--delete`, or on the server, `git push --delete <branch>` and
/// `git push origin :<branch>`. Names are compared ignoring case.
fn delete_base(seg: &Segment, bases: &BTreeSet<String>) -> Option<Danger> {
    if bases.is_empty() {
        return None;
    }
    let base_named = |name: &str| {
        let name = name.strip_prefix("refs/heads/").unwrap_or(name);
        bases.iter().find(|b| b.eq_ignore_ascii_case(name)).cloned()
    };
    if let Some(args) = git_subcommand(seg, "branch") {
        let deletes = options(args).any(|o| o == "--delete" || short_flags(o).is_some_and(|f| f.contains(['d', 'D'])));
        if !deletes {
            return None;
        }
        return operands(args).find_map(base_named).map(Danger::DeleteBase);
    }
    let args = git_subcommand(seg, "push")?;
    let deletes = options(args).any(|o| o == "--delete" || short_flags(o).is_some_and(|f| f.contains('d')));
    operands(args)
        .find_map(|p| match p.strip_prefix(':') {
            Some(branch) => base_named(branch),
            None if deletes => base_named(p),
            None => None,
        })
        .map(Danger::DeleteBase)
}

/// The arguments after the git subcommand `name`, past git's own options
/// (`-C <dir>`, `-c <key=value>`, `--git-dir`, `--work-tree`, `--no-pager`,
/// …), so `git -C dir reset --hard` is a reset.
fn git_subcommand<'a>(seg: &'a Segment, name: &str) -> Option<&'a [Word]> {
    if seg.name() != "git" {
        return None;
    }
    let mut i = 0;
    while let Some(arg) = seg.args.get(i) {
        match arg.text.as_str() {
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--config-env" | "--super-prefix" => i += 2,
            option if option.starts_with('-') => i += 1,
            sub => return if sub == name { seg.args.get(i + 1..) } else { None },
        }
    }
    None
}

/// The options of a command: the words before `--` that start with `-`.
fn options(args: &[Word]) -> impl Iterator<Item = &str> {
    args.iter().map(|w| w.text.as_str()).take_while(|t| *t != "--").filter(|t| t.starts_with('-'))
}

/// The operands of a command: the words that are not options, and every
/// word after `--`.
fn operands(args: &[Word]) -> impl Iterator<Item = &str> {
    let mut after_dashes = false;
    args.iter().map(|w| w.text.as_str()).filter(move |t| {
        if after_dashes {
            return true;
        }
        if *t == "--" {
            after_dashes = true;
            return false;
        }
        !t.starts_with('-')
    })
}

/// The letters of a short option group: `-rvf` is `rvf`. `None` for a long
/// option, a lone `-` or a word that is not an option.
fn short_flags(arg: &str) -> Option<&str> {
    let flags = arg.strip_prefix('-')?;
    (!flags.is_empty() && !flags.starts_with('-')).then_some(flags)
}

/// The first danger among the commands. Pure: the tests drive it with an
/// explicit set of integration branches.
fn find_danger(segments: &[Segment], bases: &BTreeSet<String>) -> Option<Danger> {
    segments.iter().find_map(|seg| RULES.iter().find_map(|rule| rule(seg, bases)))
}

/// The command guard: deny when one of the commands would destroy work. The
/// integration branches come from the project's `git.flow`, and the refusal
/// is written in the project's language.
pub(super) fn bash_safety(segments: &[Segment], cmd: &str, ctx: &Ctx) -> Option<Verdict> {
    let danger = find_danger(segments, &ctx.config.git.declared_bases())?;
    let lang = ctx.config.language().text_or_default();
    Some(Verdict::Deny { reason: refusal(&danger, cmd, lang) })
}

fn refusal(danger: &Danger, cmd: &str, lang: SupportedLocale) -> String {
    translate("command_guard.deny", lang)
        .replace("{reason}", &danger.reason(lang))
        .replace("{command}", truncate(cmd, 120))
}

#[cfg(test)]
mod tests {
    use super::super::lex::segments;
    use super::*;

    fn bases(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    fn danger_with(cmd: &str, flow: &BTreeSet<String>) -> Option<Danger> {
        find_danger(&segments(cmd), flow)
    }

    fn danger(cmd: &str) -> Option<Danger> {
        danger_with(cmd, &BTreeSet::new())
    }

    fn assert_blocked(cmds: &[&str], expected: &Danger) {
        for cmd in cmds {
            assert_eq!(danger(cmd).as_ref(), Some(expected), "{cmd:?} must be blocked");
        }
    }

    fn assert_passes(cmds: &[&str]) {
        for cmd in cmds {
            assert_eq!(danger(cmd), None, "{cmd:?} must pass");
        }
    }

    /// The three cases of the guard: text that only names a dangerous
    /// command passes, and the real command is blocked.
    #[test]
    fn quoted_text_passes_and_a_real_delete_is_blocked() {
        assert_passes(&[
            r#"git commit -m "limpa: rm -rf build antigo""#,
            r#"mustard-rt run pending --add --title "rodar rm -rf target antes do build""#,
            r#"echo "git push --force""#,
            r#"gh pr create --body "git reset --hard""#,
            "git commit -m \"$(cat <<'EOF'\ntira o rm -rf da pasta\nEOF\n)\"",
        ]);
        assert_blocked(&["rm -rf pasta"], &Danger::RecursiveForceDelete);
    }

    /// What the terminal runs counts: the command inside `$(…)` and the line
    /// handed to a shell with `-c`.
    #[test]
    fn a_command_the_terminal_runs_inside_another_is_judged() {
        assert_blocked(
            &[
                r#"bash -c "rm -rf pasta""#,
                "sh -c 'rm -rf pasta'",
                "echo $(rm -rf pasta)",
                r#"git commit -m "$(rm -rf pasta)""#,
                r#"sudo bash -lc "cd x && rm -rf pasta""#,
            ],
            &Danger::RecursiveForceDelete,
        );
        assert_blocked(&[r#"bash -c "git reset --hard""#], &Danger::ResetHard);
        assert_passes(&[r#"git commit -m "tira o rm -rf""#, "echo '$(rm -rf pasta)'"]);
    }

    #[test]
    fn recursive_force_delete_is_blocked_in_every_spelling() {
        assert_blocked(
            &[
                "rm -rf /",
                "rm -fr /tmp/work",
                "rm -Rf src",
                "rm -rvf build/",
                "rm --no-preserve-root /",
                "rtk rm -rf x",
                "sudo rm -rf x",
                "rm -r -f x",
                "rm --recursive --force x",
                "rm x -rf",
                "/bin/rm -rf x",
                "cd a && rm -rf b",
                "find . -name x | xargs rm -rf",
            ],
            &Danger::RecursiveForceDelete,
        );
    }

    #[test]
    fn a_delete_without_force_or_recursion_passes() {
        assert_passes(&["rm file.txt", "rm -r dir/", "rm -f file.txt", "rm -- -rf"]);
    }

    #[test]
    fn force_push_is_blocked_and_lease_passes() {
        assert_blocked(
            &[
                "git push --force origin main",
                "git push origin main --force",
                "git push origin -f",
                "git push -uf origin dev",
                "git push -f",
                "rtk git push --force",
                "git push origin +main",
            ],
            &Danger::ForcePush,
        );
        assert_passes(&[
            "git push --force-with-lease origin dev",
            "git push --force-with-lease=origin/dev origin dev",
            "git push --force-if-includes --force-with-lease origin dev",
            "git push origin dev",
            "git push -u origin feature/x",
        ]);
    }

    #[test]
    fn discarding_changes_is_blocked() {
        assert_blocked(&["git reset --hard HEAD~1", "rtk git reset --hard", "git -C pasta reset --hard"], &Danger::ResetHard);
        assert_blocked(&["git clean -fd", "git clean --force", "git -c core.x=y clean -xdf"], &Danger::CleanForce);
        assert_blocked(
            &["git checkout -- .", "git checkout -- . && echo ok", "git checkout .", "rtk git checkout -- ."],
            &Danger::CheckoutAll,
        );
        assert_blocked(
            &["git restore .", "git restore -- .", "git restore --staged --worktree .", "git restore -SW ."],
            &Danger::RestoreAll,
        );
    }

    #[test]
    fn near_miss_git_commands_pass() {
        assert_passes(&[
            "git reset --soft HEAD~1",
            "git reset HEAD file",
            "git clean -n",
            "git checkout -- src/a.rs",
            "git checkout -b feature/x",
            "git restore src/a.rs",
            "git restore --staged .",
            "git log --oneline -- .",
        ]);
    }

    #[test]
    fn deleting_a_flow_base_is_blocked() {
        let flow = bases(&["develop", "master"]);
        for (cmd, branch) in [
            ("git branch -D develop", "develop"),
            ("git branch -d master", "master"),
            ("git branch --delete develop", "develop"),
            ("rtk git branch -D develop", "develop"),
            ("git branch -D Develop", "develop"),
            ("git push origin --delete develop", "develop"),
            ("git push -d origin master", "master"),
            ("git push origin :develop", "develop"),
            ("git push origin :refs/heads/master", "master"),
        ] {
            assert_eq!(danger_with(cmd, &flow), Some(Danger::DeleteBase(branch.to_string())), "{cmd}");
        }
    }

    #[test]
    fn deleting_a_branch_the_flow_does_not_name_passes() {
        let flow = bases(&["develop", "master"]);
        for cmd in [
            "git branch -D develop_rubens",
            "git branch -D feature-x",
            "git branch -D main",
            "git push origin --delete feature/x",
            "git push origin develop",
            "git branch develop",
        ] {
            assert_eq!(danger_with(cmd, &flow), None, "{cmd}");
        }
    }

    /// Without a declared flow the project names no integration branch, and
    /// no branch name is assumed in its place.
    #[test]
    fn with_no_declared_flow_no_branch_is_a_base() {
        assert_passes(&["git branch -D main", "git branch -D master", "git push origin --delete main"]);
    }

    #[test]
    fn machine_commands_are_left_to_the_permission_list() {
        assert_passes(&[
            "chmod 777 /etc/passwd",
            "mkfs.ext4 /dev/sda1",
            "dd if=/dev/zero of=/dev/sda",
            "format c:",
            "shutdown -h now",
            "sudo reboot",
        ]);
    }

    /// The refusal comes from the catalogue, in the language `language.text`
    /// declares, with the branch filled in for an integration branch.
    #[test]
    fn the_refusal_is_written_in_the_project_language() {
        let cmd = "rm -rf pasta";
        let mut ctx = Ctx::for_test(String::new(), None);
        let Some(Verdict::Deny { reason }) = bash_safety(&segments(cmd), cmd, &ctx) else {
            panic!("{cmd} must be denied");
        };
        assert_eq!(
            reason,
            "Comando barrado: apagar pasta à força (`rm` com `-r` e `-f`). Isso apaga trabalho sem \
             volta.\nComando: rm -rf pasta\nSe for isso mesmo, peça ao usuário para rodar o comando no \
             terminal dele."
        );

        ctx.config.language.text = Some("en-US".to_string());
        let Some(Verdict::Deny { reason }) = bash_safety(&segments(cmd), cmd, &ctx) else {
            panic!("{cmd} must be denied");
        };
        assert_eq!(
            reason,
            "Command blocked: deleting a folder by force (`rm` with `-r` and `-f`). This destroys \
             work with no way back.\nCommand: rm -rf pasta\nIf this is really what you want, ask the \
             user to run the command in their own terminal."
        );

        ctx.config.git.flow.insert("*".to_string(), "develop".to_string());
        let branch = "git branch -D develop";
        let Some(Verdict::Deny { reason }) = bash_safety(&segments(branch), branch, &ctx) else {
            panic!("{branch} must be denied");
        };
        assert!(reason.contains("deleting the integration branch `develop`"), "{reason}");
    }
}
