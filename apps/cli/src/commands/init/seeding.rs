//! Laying the files down: the location guard, the private footprint and the
//! narration of each seed.
//!
//! The seeding itself lives in the core (`mustard_core::platform::project_seed`);
//! this part only guards, locates and narrates.

use std::io::Write;
use std::path::Path;

use anyhow::Result;
use mustard_core::platform::i18n::Locale;
use mustard_core::platform::project_seed::cleanup::{planned_swap_line, CleanupPlan};
use mustard_core::SeedOutcome;


/// Pre-flight location guard: refuse to init a directory that sits INSIDE a
/// git repository without being that repository's root.
///
/// Why: the workspace resolver (`mustard_core::io::workspace`) anchors on git
/// repository roots. A `mustard.json` + `.claude/` planted in a non-root
/// subdirectory would never win the resolution — it would only sit there as a
/// confusing phantom (the historical monorepo defect this guard closes).
///
/// Rules (filesystem probes only — fail-open, no `git` subprocess):
/// - the target IS a git repository root (`.git` as a directory, or as a file
///   for a submodule / linked worktree) → allow;
/// - the target lies inside a git repository but is not its root → refuse,
///   naming the repository root as the right place to init;
/// - no `.git` anywhere up the tree → allow with a note (projects without git
///   are supported through the resolver's loose fallback).
pub(super) fn guard_init_location(project_path: &Path) -> Result<()> {
    use mustard_core::io::workspace::is_git_repo_root;

    if is_git_repo_root(project_path) {
        return Ok(());
    }
    let enclosing_root = project_path
        .ancestors()
        .skip(1)
        .find(|dir| is_git_repo_root(dir));
    let Some(repo_root) = enclosing_root else {
        println!(
            "  note: no git repository found here or above - proceeding (projects without git are supported)"
        );
        return Ok(());
    };
    anyhow::bail!(
        "this folder is inside a git repository, but it is not the repository's root.\n\
         Mustard anchors its workspace at the root of a git repository, so initializing here\n\
         would leave the harness state in the wrong place.\n\
         \n\
           repository root: {}\n\
         \n\
         Either run `mustard init` from that repository root, or - if this subfolder is meant\n\
         to be its own Mustard project - make it its own git repository (or a git submodule)\n\
         first, then re-run `mustard init` here.",
        repo_root.display()
    )
}

/// Private mode, step 0: hide the Mustard footprint from THIS clone's git, and
/// name whatever the host repository already tracks.
///
/// Mirrors the private step of `mustard_core::upsert_project` so the two
/// install faces never drift: the rules are [`mustard_core::footprint_rules`],
/// the residue question is asked with [`mustard_core::footprint_pathspecs`] (the
/// two are NOT the same list — a rule is a pattern, a pathspec is a path), the
/// write goes through the clone-local exclude layer (a path git resolves — never
/// the literal `.git/info/exclude`, which does not exist in a submodule or a
/// linked worktree), and an already-tracked path is REPORTED, never unlinked:
/// `git rm --cached` rewrites the host's index, and that is the operator's
/// decision, not an install-time cosmetic.
///
/// The residue report is SPLIT, and that split is the difference between advice
/// and damage. `git rm --cached` clears a file the install put there; aimed at
/// the client's own `CLAUDE.md` — which a private install never writes, because
/// the Guards go to `CLAUDE.local.md` beside it — the same command untracks
/// THEIR work, and their next commit deletes it. So only a path
/// [`mustard_core::is_written_footprint`] recognises is offered the command; the
/// host's own file is named for what it is and left alone.
///
/// One failure here is NOT narrated away, and it is the reason this function
/// returns a `Result` at all: when git resolved an exclude file in a real
/// repository and the write still did not land, the install refuses. Everything
/// after this point would then be written VISIBLY into a repository the operator
/// believes cannot see it — the one outcome this mode exists to prevent, and the
/// one an operator cannot notice for themselves. A tree with no repository is a
/// different thing entirely (there is nobody for a footprint to be visible to)
/// and still degrades to a printed line.
///
/// # Errors
///
/// [`mustard_core::ExcludeFailure::is_blocking`] — the exclude file could not be
/// read or written inside a repository that exists.
pub(super) fn hide_footprint(project_path: &Path) -> Result<()> {
    let outcome = mustard_core::ensure_excluded(project_path, &mustard_core::footprint_rules());
    match (outcome.unavailable, outcome.appended.len()) {
        (Some(failure), _) if failure.is_blocking() => anyhow::bail!(
            "a private install must not write anything it cannot hide.\n\
             \n\
               {}\n\
             \n\
             Nothing was written. This clone's exclude file is where the footprint is hidden;\n\
             until it can be read and written, every file `mustard init` seeds would be visible\n\
             in this repository's `git status` while the install reported itself private.\n\
             Fix the file's permissions (or its type — it must be a FILE) and re-run.",
            failure.reason(),
        ),
        (Some(failure), _) => println!("  private install: {}", failure.reason()),
        (None, 0) => {
            println!("  private install: this clone's exclude file already carries every rule");
        }
        (None, count) => println!(
            "  private install: hid {count} path(s) from this clone's git (exclude file, never committed)"
        ),
    }

    let tracked =
        mustard_core::tracked_paths(project_path, &mustard_core::footprint_pathspecs());
    let (ours, theirs): (Vec<String>, Vec<String>) = tracked
        .into_iter()
        .partition(|path| mustard_core::is_written_footprint(path));
    if !ours.is_empty() {
        println!(
            "  note: this repository ALREADY tracks {} — a git ignore rule cannot hide a tracked path,",
            ours.join(", ")
        );
        println!("        so those stay visible. Nothing was unlinked; clear them yourself with:");
        println!("          git rm --cached {}", ours.join(" "));
    }
    for path in theirs {
        println!(
            "  note: {path} is the repository's OWN versioned file — a private install never \
             writes it, so it is left exactly as it is and no rule of ours hides it."
        );
    }
    Ok(())
}

/// Print one didactic line per seeded file. The seeding itself lives in the
/// core (`mustard_core::platform::project_seed`) — the CLI only narrates.
///
/// `ours` says whose file it is, and nothing else changes the wording of a
/// `Preserved`. Mustard's own texts (the agents, the session map, the two page
/// templates) are laid down again on every install, so a `Preserved` there
/// means the copy already matched the shipped one — calling it "yours" said
/// the opposite, that an edit had survived, and it read as a promise the
/// seeder never made. The project's own files (the settings, the ignore list)
/// keep the old wording, which is true of them.
pub(super) fn report_seed(name: &str, outcome: SeedOutcome, ours: bool) {
    println!("{}", seed_line(name, outcome, ours));
}

/// The line [`report_seed`] prints, so a test reads the exact words.
fn seed_line(name: &str, outcome: SeedOutcome, ours: bool) -> String {
    match outcome {
        SeedOutcome::Created | SeedOutcome::Updated => format!("  wrote {name}"),
        SeedOutcome::Preserved if ours => format!("  kept {name} (already the shipped text)"),
        SeedOutcome::Preserved => format!("  kept {name} (yours, unchanged)"),
    }
}

/// Print one line per `mustard.json#inject` migration the core performed
/// (`mustard_core::migrate_inject_declarations`).
pub(super) fn report_migration(migrated: &[String]) {
    for entry in migrated {
        println!("  migrated {entry}");
    }
}

/// Say, in one line, how many files carry what an older Mustard left and who
/// takes it out; a file with the traces of an older scan and no mark keeps its
/// short note. Each deny rule of the team's settings that the cleanup will
/// change in place gets its own line, in the project's language `lang`, saying
/// that `/mustard:upsert` swaps it and that the changed line is the person's to
/// commit. Nothing is taken out or swapped here: the plugin's door does it.
///
/// Writes to `out` (the install passes stdout) so a test reads exactly what
/// `mustard init` prints. A failed write is dropped: a notice never aborts the
/// install.
pub(super) fn report_cleanup(out: &mut impl Write, plan: &CleanupPlan, lang: Locale) {
    // Uma linha só para todos os arquivos com sobras: a lista por arquivo,
    // com todos os trechos, repetia o mesmo texto e parecia uma lista de erros.
    let count = plan.files.len();
    if count > 0 {
        let (noun, verb) = if count == 1 { ("file", "carries") } else { ("files", "carry") };
        let _ = writeln!(out, "  {count} {noun} {verb} leftovers of an older Mustard; /mustard:upsert takes them out.");
    }
    for change in &plan.files {
        for swap in &change.swaps {
            let _ = writeln!(out, "  {}", planned_swap_line(&change.path, swap, lang));
        }
    }
    for path in &plan.unmarked {
        let _ = writeln!(out, "  note: {path} carries traces of an older scan without its marks — left for you to decide");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// O instalador só chama um arquivo de "seu" quando ele é do projeto. O
    /// texto do próprio Mustard é gravado de novo a cada instalação, então um
    /// arquivo que sobreviveu intacto apenas já estava igual ao do binário, e a
    /// linha diz isso. Escrever e criar falam igual nos dois casos.
    #[test]
    fn only_the_projects_own_file_is_called_the_users() {
        let ours = |outcome| seed_line(".claude/mustard/pages/spec.html", outcome, true);
        let theirs = |outcome| seed_line(".claude/settings.json", outcome, false);
        assert_eq!(
            ours(SeedOutcome::Preserved),
            "  kept .claude/mustard/pages/spec.html (already the shipped text)"
        );
        assert_eq!(
            theirs(SeedOutcome::Preserved),
            "  kept .claude/settings.json (yours, unchanged)"
        );
        for outcome in [SeedOutcome::Created, SeedOutcome::Updated] {
            assert_eq!(ours(outcome), "  wrote .claude/mustard/pages/spec.html");
            assert_eq!(theirs(outcome), "  wrote .claude/settings.json");
        }
    }

    /// Regression guard (2026-06-03): the legacy per-subproject guards file
    /// `.claude/commands/guards.md` (and its `patterns.md` companion) is
    /// OBSOLETE. No shipped template may point an agent at those non-existent
    /// paths. Walks the REAL bundled `templates/` payloads — the CLI's own
    /// tree AND the core seed tree (`packages/core/templates/`, where the
    /// harness seeds moved) — and fails if the obsolete path is reintroduced.
    #[test]
    fn templates_never_reference_obsolete_guards_file() {
        let templates = crate::manifest_dir::manifest_dir().join("templates");
        assert!(
            templates.is_dir(),
            "templates payload missing at {}",
            templates.display()
        );
        let core_templates = crate::manifest_dir::manifest_dir()
            .join("../../packages/core/templates");
        assert!(
            core_templates.is_dir(),
            "core seed payload missing at {}",
            core_templates.display()
        );

        const FORBIDDEN: [&str; 2] = ["commands/guards.md", "commands/patterns.md"];
        let mut offenders: Vec<String> = Vec::new();

        // Iterative directory walk — no external crate.
        let mut stack = vec![templates.clone(), core_templates];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Ok(bytes) = fs::read(&path) else {
                    continue;
                };
                let text = String::from_utf8_lossy(&bytes);
                for needle in FORBIDDEN {
                    if text.contains(needle) {
                        offenders.push(format!("{} → {needle}", path.display()));
                    }
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "templates must not reference the obsolete standalone guards file:\n{}",
            offenders.join("\n")
        );
    }

    /// Grava `body` em `rel` dentro de `root`, com as pastas no caminho.
    fn write_file(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// Um arquivo de subprojeto que só tem o que um Mustard antigo escreveu:
    /// o import, o título, a linha de navegação e o bloco das Guards.
    const OLD_SUBPROJECT_MD: &str = "@.claude/scan-map.md\n\n# Rt\n\n> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [../../.claude/mustard/orchestrator.md](../../.claude/mustard/orchestrator.md)\n\n## Guards\n\n<!-- mustard:guards -->\n- Never panic in a hook.\n<!-- /mustard:guards -->\n";

    /// O que o `mustard init` imprime sobre as sobras do projeto em `root`: o
    /// plano de verdade, lido do disco, passado ao mesmo aviso que a instalação
    /// chama.
    fn cleanup_notice(root: &Path) -> String {
        let plan = mustard_core::platform::project_seed::cleanup::plan(root);
        let mut out = Vec::new();
        report_cleanup(&mut out, &plan, Locale::EnUs);
        String::from_utf8(out).unwrap()
    }

    /// Quando o init acha sobras do Mustard antigo em vários arquivos, o aviso
    /// é uma linha só, com quantos arquivos têm sobras e que o /mustard:upsert
    /// as tira: três arquivos viram "3 files", sem o nome de nenhum deles e
    /// sem os trechos que saem.
    #[test]
    fn leftovers_in_several_files_become_one_line_with_the_count() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_file(root, "CLAUDE.md", "@.claude/scan-map.md\n# Projeto\n\nNossa regra.\n");
        write_file(root, "apps/rt/CLAUDE.md", OLD_SUBPROJECT_MD);
        write_file(
            root,
            "apps/web/CLAUDE.md",
            "@.claude/scan-map.md\n\n# Web\n\n> Parent: [../../CLAUDE.md](../../CLAUDE.md) | Orchestrator: [x](x)\n",
        );

        let plan = mustard_core::platform::project_seed::cleanup::plan(root);
        assert_eq!(plan.files.len(), 3, "fixture: três arquivos com sobras: {plan:?}");
        assert!(!plan.rules.is_empty(), "fixture: a regra da Guard que sai vai à lista de pendências: {plan:?}");

        assert_eq!(
            cleanup_notice(root),
            "  3 files carry leftovers of an older Mustard; /mustard:upsert takes them out.\n",
        );
    }

    /// O arquivo sem marca continua com a sua nota curta, depois da linha das
    /// sobras; um arquivo só fica no singular; e sem sobra nenhuma o init não
    /// diz nada.
    #[test]
    fn an_unmarked_file_keeps_its_short_note_after_the_count() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write_file(root, "apps/rt/CLAUDE.md", OLD_SUBPROJECT_MD);
        write_file(root, "apps/cli/CLAUDE.md", "# Cli\n\n## Guards\n\n- ours\n");

        assert_eq!(
            cleanup_notice(root),
            "  1 file carries leftovers of an older Mustard; /mustard:upsert takes them out.\n  note: apps/cli/CLAUDE.md carries traces of an older scan without its marks — left for you to decide\n",
        );

        let clean = tempdir().unwrap();
        write_file(clean.path(), "CLAUDE.md", "# Team notes\n\nNothing else.\n");
        assert_eq!(cleanup_notice(clean.path()), "");
    }

    /// A regra de bloqueio do arquivo da equipe que vai mudar no lugar ganha
    /// uma linha só dela, depois da contagem, com a antiga e a nova, no idioma
    /// do projeto. A linha diz que o `/mustard:upsert` faz a troca, nunca que
    /// ela já foi feita: o aviso sai antes de qualquer gravação, e o arquivo
    /// continua como estava.
    #[test]
    fn a_swapped_deny_rule_gets_its_own_line_in_the_project_language() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let team = "{\"permissions\":{\"deny\":[\"Bash(git reset * --hard:*)\"]}}\n";
        write_file(root, ".claude/settings.json", team);

        assert_eq!(
            cleanup_notice(root),
            "  1 file carries leftovers of an older Mustard; /mustard:upsert takes them out.\n  \
             .claude/settings.json: /mustard:upsert swaps the deny rule `Bash(git reset * --hard:*)` for \
             `Bash(git reset * --hard*)`, in the same place. It still blocks the same command, and Claude Code \
             stops warning; the changed line is yours to commit.\n",
        );
        let plan = mustard_core::platform::project_seed::cleanup::plan(root);
        let mut out = Vec::new();
        report_cleanup(&mut out, &plan, Locale::PtBr);
        let notice = String::from_utf8(out).unwrap();
        assert!(
            notice.contains(
                ".claude/settings.json: o /mustard:upsert troca a regra de bloqueio `Bash(git reset * --hard:*)` \
                 por `Bash(git reset * --hard*)`, no mesmo lugar."
            ) && notice.contains("a linha mudada é sua para comitar."),
            "{notice}",
        );
        assert!(!notice.contains("virou"), "nothing was swapped yet: {notice}");
        assert_eq!(std::fs::read_to_string(root.join(".claude/settings.json")).unwrap(), team);
    }
}
