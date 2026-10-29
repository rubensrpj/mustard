//! `base_delete` — a recusa de apagar uma branch de integração.
//!
//! Lê os comandos que [`super::lex::segments`] achou, nunca o texto cru: a
//! mensagem de commit que só cita o comando passa, e o comando dentro de
//! `$(…)`, de `bash -c "…"` ou de `find -exec` conta. As branches de
//! integração são as que o `git.flow` do projeto nomeia, lidas do contexto do
//! gancho; o projeto que não declara fluxo não tem nenhuma, e nenhum nome de
//! branch mora neste arquivo. Os outros perigos fixos (`rm -rf`, `git reset
//! --hard`, `git push --force`) ficam no bloqueio nativo das configurações do
//! Claude Code.

use std::collections::BTreeSet;

use mustard_core::domain::model::contract::{Ctx, Verdict};
use mustard_core::{translate, SupportedLocale};

use super::lex::{truncate, Segment, Word};

/// A branch de integração que um dos comandos apaga, aqui ou no servidor:
/// `git branch` com `-d`, `-D` ou `--delete`, `git push --delete <branch>` e
/// `git push origin :<branch>`. Os nomes se comparam sem caixa.
fn deleted_base(segments: &[Segment], bases: &BTreeSet<String>) -> Option<String> {
    if bases.is_empty() {
        return None;
    }
    let base_named = |name: &str| {
        let name = name.strip_prefix("refs/heads/").unwrap_or(name);
        bases.iter().find(|b| b.eq_ignore_ascii_case(name)).cloned()
    };
    segments.iter().find_map(|seg| {
        if let Some(args) = git_subcommand(seg, "branch") {
            let deletes = options(args).any(|o| o == "--delete" || short_flags(o).is_some_and(|f| f.contains(['d', 'D'])));
            return if deletes { operands(args).find_map(base_named) } else { None };
        }
        let args = git_subcommand(seg, "push")?;
        let deletes = options(args).any(|o| o == "--delete" || short_flags(o).is_some_and(|f| f.contains('d')));
        operands(args).find_map(|p| match p.strip_prefix(':') {
            Some(branch) => base_named(branch),
            None if deletes => base_named(p),
            None => None,
        })
    })
}

/// Os argumentos depois do subcomando `name` do git, passadas as opções do
/// próprio git (`-C <dir>`, `-c <key=value>`, `--git-dir`, …).
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

/// As opções de um comando: as palavras antes de `--` que começam com `-`.
fn options(args: &[Word]) -> impl Iterator<Item = &str> {
    args.iter().map(|w| w.text.as_str()).take_while(|t| *t != "--").filter(|t| t.starts_with('-'))
}

/// Os operandos de um comando: as palavras que não são opção, e toda palavra
/// depois de `--`.
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

/// As letras de um grupo de opções curtas: `-rvf` é `rvf`. `None` para a opção
/// longa, o `-` sozinho ou a palavra que não é opção.
fn short_flags(arg: &str) -> Option<&str> {
    let flags = arg.strip_prefix('-')?;
    (!flags.is_empty() && !flags.starts_with('-')).then_some(flags)
}

/// A recusa do comando que apaga uma branch de integração, na língua do
/// projeto.
pub(super) fn bash_base_delete(segments: &[Segment], cmd: &str, ctx: &Ctx) -> Option<Verdict> {
    let branch = deleted_base(segments, &ctx.config.git.declared_bases())?;
    let lang = ctx.config.language().text_or_default();
    Some(Verdict::Deny { reason: refusal(&branch, cmd, lang) })
}

fn refusal(branch: &str, cmd: &str, lang: SupportedLocale) -> String {
    let reason = translate("command_guard.delete_base", lang).replace("{branch}", branch);
    translate("command_guard.deny", lang).replace("{reason}", &reason).replace("{command}", truncate(cmd, 120))
}

#[cfg(test)]
mod tests {
    use super::super::lex::segments;
    use super::*;

    fn bases(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    fn deleted(cmd: &str, flow: &BTreeSet<String>) -> Option<String> {
        deleted_base(&segments(cmd), flow)
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
            ("git -C repo branch -D develop", "develop"),
            ("git push origin --delete develop", "develop"),
            ("git push -d origin master", "master"),
            ("git push origin :develop", "develop"),
            ("git push origin :refs/heads/master", "master"),
            ("bash -c \"git branch -D develop\"", "develop"),
        ] {
            assert_eq!(deleted(cmd, &flow).as_deref(), Some(branch), "{cmd}");
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
            r#"git commit -m "git branch -D develop""#,
        ] {
            assert_eq!(deleted(cmd, &flow), None, "{cmd}");
        }
    }

    /// Sem fluxo declarado, o projeto não nomeia branch de integração, e
    /// nenhum nome é suposto no lugar.
    #[test]
    fn with_no_declared_flow_no_branch_is_a_base() {
        for cmd in ["git branch -D main", "git branch -D master", "git push origin --delete main"] {
            assert_eq!(deleted(cmd, &BTreeSet::new()), None, "{cmd}");
        }
    }

    /// A recusa sai na língua do projeto e nomeia a branch.
    #[test]
    fn the_refusal_is_written_in_the_project_language() {
        let mut ctx = Ctx::for_test(String::new(), None);
        ctx.config.git.flow.insert("*".to_string(), "develop".to_string());
        for (declared, lang) in [(None, SupportedLocale::PtBr), (Some("en-US"), SupportedLocale::EnUs)] {
            ctx.config.language.text = declared.map(str::to_string);
            let cmd = "git branch -D develop";
            let Some(Verdict::Deny { reason }) = bash_base_delete(&segments(cmd), cmd, &ctx) else {
                panic!("{cmd} must be denied");
            };
            let expected = translate("command_guard.delete_base", lang).replace("{branch}", "develop");
            assert!(reason.contains(&expected), "{lang:?} lacks {expected:?}: {reason}");
        }
    }
}
