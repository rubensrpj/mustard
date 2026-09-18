//! The static seeds: Mustard's own texts — the session map under
//! `.claude/mustard/` and the three agents under `.claude/agents/mustard/` —,
//! the `.claude/.gitignore` rule list, and the project-root `mustard.json`,
//! with the migrations that bring an older `inject` list onto the session map.

use std::path::Path;

use crate::domain::command_detect::detect_commands;
use crate::domain::config::{Injectable, ProjectConfig, Runtime};
use crate::io::fs;
use crate::platform::error::Result;
use crate::platform::i18n::Locale;
use crate::platform::seeds::{agent_texts, session_map, AGENT_NAMES, CLAUDE_GITIGNORE, SESSION_MAP_NAME};

use super::SeedOutcome;

/// A pasta do mapa do início da sessão, a partir de `.claude/`.
const SESSION_MAP_DIR: &str = "mustard";

/// A pasta dos agentes do Mustard, a partir de `.claude/`. É uma subpasta
/// própria dentro de `agents/`, que é uma pasta onde o projeto também escreve:
/// os agentes do projeto ficam ao lado, e nenhum arquivo deles é tocado.
const AGENTS_DIR: &str = "agents/mustard";

/// Os textos do Mustard no projeto, a partir de `.claude/`, com o corpo no
/// idioma `text`: o mapa do início da sessão e os três agentes, nessa ordem.
///
/// Os dois idiomas são molde do produto; o projeto recebe só o do
/// `language.text`. O caminho não muda com o idioma, então trocar o idioma e
/// rodar o instalador de novo troca o texto no mesmo arquivo.
#[must_use]
pub fn harness_texts(text: Locale) -> Vec<(String, &'static str)> {
    let mut out = vec![(format!("{SESSION_MAP_DIR}/{SESSION_MAP_NAME}"), session_map(text))];
    out.extend(agent_texts(text).into_iter().map(|(name, body)| (format!("{AGENTS_DIR}/{name}.md"), body)));
    out
}

/// Os caminhos, a partir de `.claude/`, de todo texto que [`harness_texts`]
/// grava — os mesmos em qualquer idioma.
#[must_use]
pub fn harness_text_paths() -> Vec<String> {
    let mut out = vec![format!("{SESSION_MAP_DIR}/{SESSION_MAP_NAME}")];
    out.extend(AGENT_NAMES.iter().map(|name| format!("{AGENTS_DIR}/{name}.md")));
    out
}

/// O caminho declarado do mapa do início da sessão, a partir da raiz do
/// projeto: é por ele que o `mustard.json` o entrega.
#[must_use]
pub fn session_map_declared_path() -> String {
    format!(".claude/{SESSION_MAP_DIR}/{SESSION_MAP_NAME}")
}

/// Grava os textos do Mustard em `.claude/`, no idioma `text`.
///
/// **Always rewritten.** Unlike every other seeder here there is no
/// overwrite/merge decision to take: these files are the harness's own text,
/// not project configuration, so each install and each update lays the
/// compiled-in body down again. A copy that diverged — an operator's edit, a
/// seed from an older release, or the other language — is replaced, and the
/// replacement is reported as [`SeedOutcome::Updated`], never as
/// [`SeedOutcome::Preserved`]. A copy that cannot be READ is rewritten too.
///
/// Returns `(path from .claude/, outcome)` per file, in [`harness_texts`]
/// order.
///
/// # Errors
///
/// An IO error creating a directory or writing a file.
pub fn seed_harness_texts(claude_dir: &Path, text: Locale) -> Result<Vec<(String, SeedOutcome)>> {
    let mut out = Vec::new();
    for (rel, body) in harness_texts(text) {
        let dest = claude_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        out.push((rel, seed_static_file(&dest, body, true)?));
    }
    Ok(out)
}

/// Seed `.claude/.gitignore` (the ephemeral harness state cover).
///
/// The ONE seed that merges by LINE instead of by file. Every other seed is a
/// document whose owner is either the user or Mustard, so "preserve what
/// exists" is the whole answer. This one is a rule list, and preserving it
/// whole meant that a project installed before a rule existed never received
/// it: the entry landed in the template, `mustard init` reported
/// [`SeedOutcome::Preserved`], and the rule reached fresh projects only —
/// exactly the state review found for `scratch/`, where the write gate had
/// already been taught to allow a path nothing ignored.
///
/// Merge therefore APPENDS the pattern lines the file lacks, under a header
/// that says who wrote them. The user's own lines, order and comments are
/// untouched, and a file that already carries every pattern is
/// [`SeedOutcome::Preserved`] — so the operation converges after one run.
/// Overwrite re-lays the seed whole, as before.
///
/// # Errors
///
/// An IO error writing the file.
pub fn seed_gitignore(claude_dir: &Path, overwrite: bool) -> Result<SeedOutcome> {
    let dest = claude_dir.join(".gitignore");
    if overwrite {
        return seed_static_file(&dest, CLAUDE_GITIGNORE, true);
    }
    let Ok(existing) = fs::read_to_string(&dest) else {
        // Absent → the seed IS the file. Unreadable-but-present degrades to
        // preserved inside `seed_static_file` — never stomp what we could not
        // inspect.
        return seed_static_file(&dest, CLAUDE_GITIGNORE, false);
    };
    let missing = missing_ignore_patterns(&existing, CLAUDE_GITIGNORE);
    if missing.is_empty() {
        return Ok(SeedOutcome::Preserved);
    }
    let mut merged = existing;
    if !merged.ends_with('\n') {
        merged.push('\n');
    }
    merged.push_str(GITIGNORE_BACKFILL_HEADER);
    for pattern in missing {
        merged.push_str(pattern);
        merged.push('\n');
    }
    fs::write_atomic(&dest, merged.as_bytes())?;
    Ok(SeedOutcome::Updated)
}

/// The header the line-merge writes above the patterns it appends, so the
/// addition is attributable rather than mysterious.
const GITIGNORE_BACKFILL_HEADER: &str =
    "\n# Added by Mustard: patterns the seed gained after this file was written.\n";

/// The seed's pattern lines that `existing` does not already carry, in seed
/// order and without repeats.
///
/// Compared on the TRIMMED pattern only: comments and blank lines are layout,
/// and re-appending a comment whose pattern is already present would grow the
/// file on every install. Deliberately literal — two patterns that match the
/// same paths by different spellings read as different rules here, which is the
/// conservative direction (a redundant line costs a line; a missing one costs
/// the rule).
fn missing_ignore_patterns<'a>(existing: &str, seed: &'a str) -> Vec<&'a str> {
    let is_pattern = |line: &&str| !line.is_empty() && !line.starts_with('#');
    let present: Vec<&str> = existing.lines().map(str::trim).filter(is_pattern).collect();
    let mut missing: Vec<&str> = Vec::new();
    for pattern in seed.lines().map(str::trim).filter(is_pattern) {
        if !present.contains(&pattern) && !missing.contains(&pattern) {
            missing.push(pattern);
        }
    }
    missing
}

/// Write one static seed to `dest` honouring merge/overwrite, reporting what
/// happened. An existing byte-identical file is [`SeedOutcome::Preserved`]
/// even under overwrite (no gratuitous rewrite).
///
/// **An unreadable-but-present file follows `overwrite`, not a blanket
/// "preserve".** Under MERGE (`overwrite == false`) it is preserved: the caller
/// wanted to keep what is there, and a file we could not inspect is exactly
/// what must not be stomped. Under OVERWRITE it is REWRITTEN, because that flag
/// is the always-rewrite contract [`seed_harness_texts`] states above its own
/// call — these are the harness's own texts, not project configuration, and preserving one is precisely how a corrected rule stops at
/// the project boundary in silence. That silence is the defect the fingerprint
/// catalog was removed to end; a file whose bytes cannot even be read is the
/// LEAST trustworthy copy to leave a window reading.
fn seed_static_file(dest: &Path, body: &str, overwrite: bool) -> Result<SeedOutcome> {
    match fs::read_to_string(dest) {
        Ok(existing) => {
            if !overwrite || existing == body {
                return Ok(SeedOutcome::Preserved);
            }
            fs::write_atomic(dest, body.as_bytes())?;
            Ok(SeedOutcome::Updated)
        }
        Err(crate::platform::error::Error::NotFound(_)) => {
            fs::write_atomic(dest, body.as_bytes())?;
            Ok(SeedOutcome::Created)
        }
        Err(_) if overwrite => {
            fs::write_atomic(dest, body.as_bytes())?;
            Ok(SeedOutcome::Updated)
        }
        Err(_) => Ok(SeedOutcome::Preserved),
    }
}


/// A declaração padrão do `mustard.json#inject`: o mapa do início da sessão,
/// entregue no `sessionStart`.
///
/// O início da sessão roda na abertura, depois de `/clear`, da compactação,
/// da retomada e da bifurcação: toda janela nova recebe o mapa ali, e a
/// mensagem do usuário não entrega texto nenhum. `once` fica desligado porque
/// o próprio evento já só roda quando a janela é nova.
#[must_use]
pub fn default_inject_entries() -> Vec<Injectable> {
    vec![Injectable { on: "sessionStart".to_string(), file: session_map_declared_path(), once: false }]
}

/// Create or minimally update the project-root `mustard.json` through
/// [`ProjectConfig`] (the single owner).
///
/// Absent → created with an empty `git.flow` (the project decides later),
/// agnostically detected commands, the default `inject` declarations,
/// `runtime`, and `version` (when supplied). Present → `version` is re-stamped
/// (only when `Some` and different), an empty `inject` is backfilled with the
/// defaults, an absent `runtime` is filled — everything else is preserved
/// verbatim, and the file is not rewritten when nothing changed.
pub(super) fn upsert_mustard_json(root: &Path, version: Option<&str>) -> Result<SeedOutcome> {
    let existed = ProjectConfig::exists(root);
    let mut config = ProjectConfig::load(root);

    if !existed {
        let commands = detect_commands(root);
        config.build_command = commands.build;
        config.test_command = commands.test;
        config.lint_command = commands.lint;
        config.type_check_command = commands.type_check;
        config.inject = default_inject_entries();
        config.runtime = Some(Runtime::detect());
        config.version = version.map(str::to_string);
        config.write(root)?;
        return Ok(SeedOutcome::Created);
    }

    let mut changed = false;
    if let Some(version) = version
        && config.version.as_deref() != Some(version) {
            config.version = Some(version.to_string());
            changed = true;
        }
    if config.inject.is_empty() {
        config.inject = default_inject_entries();
        changed = true;
    }
    if config.runtime.is_none() {
        config.runtime = Some(Runtime::detect());
        changed = true;
    }
    if !changed {
        return Ok(SeedOutcome::Preserved);
    }
    config.write(root)?;
    Ok(SeedOutcome::Updated)
}

// ---------------------------------------------------------------------------
// The `inject` migrations
// ---------------------------------------------------------------------------

/// Leva o `mustard.json#inject` de um projeto já instalado para o desenho
/// atual e diz o que mudou.
///
/// Mexe só no `mustard.json`, que é do Mustard, e nos arquivos que ele mesmo
/// semeou antes: o estilo de resposta que virou estilo do plugin sai (com o
/// arquivo órfão), o mapa do início da sessão troca o nome antigo pelo novo, e
/// as três partes do roteador antigo dão lugar ao mapa. Idempotente e sem
/// erro: uma configuração que não se lê ou não se grava vira "nada migrado".
pub fn migrate_inject_declarations(root: &Path, claude_dir: &Path) -> Vec<String> {
    let mut migrated = Vec::new();
    if retire_response_style_inject(root, claude_dir) {
        migrated.push("mustard.json (response-style → output-style)".to_string());
    }
    if rename_old_session_map(root, claude_dir) {
        migrated.push(format!("session map ({OLD_SESSION_MAP_NAME} → {SESSION_MAP_NAME})"));
    }
    if retire_router_parts(root, claude_dir) {
        migrated.push("mustard.json (router injectables → session map)".to_string());
    }
    migrated
}

/// Retire the legacy `sessionStart` → `.claude/mustard/response-style.md`
/// injectable: drop the `mustard.json#inject` entry and delete the orphaned
/// instruction file. The response style ships as a plugin output-style, which
/// no per-project injectable can match.
///
/// Returns `true` when something was retired. Idempotent — a project already
/// migrated (or a fresh one) returns `false`. Fail-open: an unreadable or
/// unwritable config degrades to `false`, never an error.
fn retire_response_style_inject(root: &Path, claude_dir: &Path) -> bool {
    const LEGACY_FILE: &str = ".claude/mustard/response-style.md";
    let mut changed = false;

    // Drop the stale inject entry via the config owner (single source of truth).
    if ProjectConfig::exists(root) {
        let mut config = ProjectConfig::load(root);
        let before = config.inject.len();
        config.inject.retain(|e| e.file != LEGACY_FILE);
        if config.inject.len() != before && config.write(root).is_ok() {
            changed = true;
        }
    }

    // Delete the orphaned instruction file under this project's `.claude/`.
    let orphan = claude_dir.join("mustard").join("response-style.md");
    if orphan.is_file() && fs::remove_file(&orphan).is_ok() {
        changed = true;
    }

    changed
}

/// `true` when two declared injectable paths name the SAME file.
///
/// A declaration is written by hand as often as it is seeded, and the same file
/// has several honest spellings: a `./` prefix, backslashes on Windows, a
/// trailing slash, mixed case on a case-insensitive filesystem. Comparing the
/// raw strings makes each of those a different file, and the only consequence a
/// user ever sees is a section that silently reaches nobody.
///
/// Normalisation is deliberately conservative — separators, one leading `./`,
/// trailing separators, and ASCII case. It never resolves symlinks or touches
/// the filesystem: this runs during install, on paths that may not exist yet.
pub fn same_declared_path(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.trim().replace('\\', "/");
        let s = s.strip_prefix("./").unwrap_or(&s).to_string();
        s.trim_end_matches('/').to_ascii_lowercase()
    }
    norm(a) == norm(b)
}

/// O nome que o mapa do início da sessão tinha antes de ganhar nome em inglês,
/// na mesma pasta `.claude/mustard/`.
const OLD_SESSION_MAP_NAME: &str = "mapa-inicio-sessao.md";

/// Troca o nome antigo do mapa do início da sessão pelo novo.
///
/// No `mustard.json`, cada declaração do caminho antigo, em qualquer grafia
/// que [`same_declared_path`] reconhece, passa a apontar para o caminho novo,
/// com o mesmo evento, o mesmo `once` e o mesmo lugar na lista; o resto do
/// arquivo fica como está. Uma declaração antiga cujo evento já entrega o
/// caminho novo sai, para o mapa não chegar duas vezes. O arquivo de nome
/// antigo sai do disco: o mapa é texto do próprio Mustard, e a instalação grava
/// o de nome novo logo depois.
///
/// `true` quando algo mudou. Idempotente e sem erro.
fn rename_old_session_map(root: &Path, claude_dir: &Path) -> bool {
    let old_path = format!(".claude/{SESSION_MAP_DIR}/{OLD_SESSION_MAP_NAME}");
    let map = session_map_declared_path();
    let mut changed = false;
    if ProjectConfig::exists(root) {
        let mut config = ProjectConfig::load(root);
        if config.inject.iter().any(|e| same_declared_path(&e.file, &old_path)) {
            let before = std::mem::take(&mut config.inject);
            for entry in &before {
                if !same_declared_path(&entry.file, &old_path) {
                    config.inject.push(entry.clone());
                    continue;
                }
                let delivers_map =
                    |e: &Injectable| e.on.eq_ignore_ascii_case(&entry.on) && same_declared_path(&e.file, &map);
                if before.iter().any(delivers_map) || config.inject.iter().any(delivers_map) {
                    continue;
                }
                config.inject.push(Injectable { file: map.clone(), ..entry.clone() });
            }
            changed = config.write(root).is_ok();
        }
    }
    let orphan = claude_dir.join(SESSION_MAP_DIR).join(OLD_SESSION_MAP_NAME);
    if orphan.is_file() && fs::remove_file(&orphan).is_ok() {
        changed = true;
    }
    changed
}

/// Os três textos do roteador que instalações antigas semeavam e declaravam,
/// escritos como foram gravados. O mapa do início da sessão tomou o lugar dos
/// três.
const RETIRED_ROUTER_PARTS: [&str; 3] =
    [".claude/mustard/orchestrator.md", ".claude/mustard/dispatch.md", ".claude/mustard/material.md"];

/// Troca as três partes do roteador antigo pelo mapa do início da sessão.
///
/// Num projeto que declara alguma das três, elas saem da lista e o mapa entra
/// no lugar, uma vez só; os três arquivos órfãos saem do disco. Um projeto
/// que não declara nenhuma das três não ganha o mapa por aqui: uma lista
/// vazia é preenchida pelo [`upsert_mustard_json`], e uma lista que a pessoa
/// escreveu sem o roteador fica como está.
///
/// `true` quando algo mudou. Idempotente e sem erro.
fn retire_router_parts(root: &Path, claude_dir: &Path) -> bool {
    let mut changed = false;
    if ProjectConfig::exists(root) {
        let mut config = ProjectConfig::load(root);
        let retired = |file: &str| RETIRED_ROUTER_PARTS.iter().any(|old| same_declared_path(file, old));
        if config.inject.iter().any(|e| retired(&e.file)) {
            config.inject.retain(|e| !retired(&e.file));
            let map = session_map_declared_path();
            config.inject.retain(|e| !same_declared_path(&e.file, &map));
            config.inject.extend(default_inject_entries());
            changed = config.write(root).is_ok();
        }
    }
    for old in RETIRED_ROUTER_PARTS {
        let orphan = claude_dir.join(old.trim_start_matches(".claude/"));
        if orphan.is_file() && fs::remove_file(&orphan).is_ok() {
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::git;
    use crate::platform::project_seed::{upsert_project, InstallMode};
    use std::fs as std_fs;
    use tempfile::tempdir;

    /// A migração reconhece as grafias equivalentes de um caminho declarado:
    /// com `./`, com barra invertida e com maiúscula. Comparar o texto cru
    /// deixaria a parte antiga do roteador na lista, entregue a ninguém.
    #[test]
    fn a_declared_path_is_matched_by_every_equivalent_spelling() {
        for spelling in [
            ".claude/mustard/orchestrator.md",
            "./.claude/mustard/orchestrator.md",
            ".claude\\mustard\\orchestrator.md",
            ".claude/mustard/Orchestrator.md",
            "  .claude/mustard/orchestrator.md  ",
        ] {
            assert!(
                super::same_declared_path(spelling, super::RETIRED_ROUTER_PARTS[0]),
                "`{spelling}` was read as a different file from the declared router part",
            );
        }
        assert!(!super::same_declared_path(".claude/mustard/dispatch.md", super::RETIRED_ROUTER_PARTS[0]));
        assert!(!super::same_declared_path(".claude/mustard/dispatch.md", ".claude/mustard/dispatch.md.bak"));
    }

    // --- .gitignore line-merge ------------------------------------------------

    /// Seeding over an EXISTING ignore file appends the patterns it
    /// lacks instead of preserving the file whole.
    ///
    /// The defect this closes shipped in this very repository: the write gate
    /// was taught to allow `.claude/scratch/`, the `scratch/` line landed in the
    /// template, and every already-initialised project — Mustard included — kept
    /// an ignore file without it, because `Preserved` skipped the whole file. A
    /// gate that allows a path nothing ignores, under a `/git` law that stages
    /// everything, commits throwaway evidence into the unit.
    ///
    /// Both halves, so "append" cannot become "overwrite": the user's own lines
    /// survive, nothing already present is duplicated, and the second run is
    /// `Preserved` — the merge converges.
    #[test]
    fn seeding_over_an_existing_ignore_adds_the_missing_lines() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        // An ignore file from an older install: two of the seed's patterns, plus
        // a line only this project's user knows about.
        std_fs::write(
            claude.join(".gitignore"),
            "# Mustard harness scratch\n.cache/\nworktrees/\n\n# mine\nmy-notes/\n",
        )
        .unwrap();

        let outcome = seed_gitignore(&claude, false).unwrap();

        assert_eq!(outcome, SeedOutcome::Updated, "a file missing patterns is not preserved");
        let merged = std_fs::read_to_string(claude.join(".gitignore")).unwrap();
        assert!(merged.contains("my-notes/"), "the user's own rule survives: {merged}");
        assert!(merged.contains("# mine"), "…and so do their comments: {merged}");
        // EVERY pattern of the seed is now in force — the point of the change.
        for pattern in CLAUDE_GITIGNORE.lines().map(str::trim) {
            if pattern.is_empty() || pattern.starts_with('#') {
                continue;
            }
            assert!(
                merged.lines().any(|l| l.trim() == pattern),
                "seed pattern {pattern:?} missing after the merge: {merged}",
            );
        }
        assert_eq!(
            merged.lines().filter(|l| l.trim() == ".cache/").count(),
            1,
            "a pattern already present is not appended twice: {merged}",
        );

        // Convergence: the same call over the merged file changes nothing.
        let second = seed_gitignore(&claude, false).unwrap();
        assert_eq!(second, SeedOutcome::Preserved, "the merge is idempotent");
        assert_eq!(
            std_fs::read_to_string(claude.join(".gitignore")).unwrap(),
            merged,
            "…byte for byte",
        );
    }

    #[test]
    fn a_fresh_ignore_is_the_seed_verbatim() {
        // The absent case must stay a plain copy: no backfill header, no
        // reordering — a fresh project's file IS the template.
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();

        assert_eq!(seed_gitignore(&claude, false).unwrap(), SeedOutcome::Created);

        assert_eq!(
            std_fs::read_to_string(claude.join(".gitignore")).unwrap(),
            CLAUDE_GITIGNORE,
        );
    }

    /// Uma instalação anterior à lista de pendências recebe `pending/` pelo merge
    /// por linha, sem perder nada do que já estava lá. Sem a linha, o primeiro
    /// `git add -A` levaria o ledger para o branch em uso — e ele existe
    /// justamente para não mudar com o branch.
    #[test]
    fn the_line_merge_backfills_the_pending_ledger() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        std_fs::write(claude.join(".gitignore"), "# mine\nmy-notes/\nworktrees/\n").unwrap();

        assert_eq!(seed_gitignore(&claude, false).unwrap(), SeedOutcome::Updated);
        let merged = std_fs::read_to_string(claude.join(".gitignore")).unwrap();
        assert!(merged.lines().any(|l| l.trim() == "pending/"), "pending/ backfilled: {merged}");
        assert!(merged.starts_with("# mine\nmy-notes/\nworktrees/\n"), "merge-only: {merged}");
    }

    /// The two GATE MARKERS are held back, and the unit's own RECORD is not.
    ///
    /// Both halves are asserted, and the second is the one that makes the test
    /// worth having: a blanket `spec/` cover would pass the first half alone,
    /// and that blanket cover is exactly the state this rule replaces — it hid
    /// 45 of 86 units from git on this repository while the seeded ignore next
    /// to it said a spec's content stays versioned.
    ///
    /// Driven through REAL git rather than by reading the file back. What is
    /// being bought is git's own decision, and `check-ignore` is the question
    /// the operator would ask; a substring search over the file would pass on a
    /// pattern git does not actually apply at that depth.
    #[test]
    fn the_seeded_gitignore_holds_back_the_gate_markers() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert!(
            git::run(root, &["init", "-q"]).ok,
            "the test measures git's decision, so it needs a real repository",
        );

        let claude = root.join(".claude");
        std_fs::create_dir_all(&claude).unwrap();
        seed_gitignore(&claude, false).unwrap();

        let ignored = |rel: &str| git::run(root, &["check-ignore", "-q", rel]).ok;

        // Held back: presence of either file IS the gate's answer, so a commit
        // would let `git clone` hand a fresh checkout an approval nobody gave.
        let marker = ".claude/spec/a-unit/.approved-by-user";
        assert!(ignored(marker), "a gate marker must never be versionable: {marker}");

        // Kept: the unit's record is what a reviewer reads, and it belongs to
        // the repository.
        for kept in ["spec.md", "wave-plan.md", "pr-body.md", "meta.json", "review/findings.md"] {
            let path = format!(".claude/spec/a-unit/{kept}");
            assert!(!ignored(&path), "the unit's own record must stay versioned: {path}");
        }

        // Unchanged by this addition: the machine sidecars stay ignored.
        for sidecar in [".events/2026-01-01.ndjson", ".dispatch/wave-1.prompt.md"] {
            let path = format!(".claude/spec/a-unit/{sidecar}");
            assert!(ignored(&path), "regenerable machine state stays ignored: {path}");
        }
    }

    #[test]
    fn migration_retires_legacy_response_style_injectable() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let claude = root.join(".claude");
        std_fs::create_dir_all(claude.join("mustard")).unwrap();
        // A project installed before the output-style move: the response style
        // rides sessionStart in #inject and its file sits under .claude/mustard/.
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","inject":[{"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true},{"on":"sessionStart","file":".claude/mustard/response-style.md","once":true}]}"#,
        )
        .unwrap();
        std_fs::write(claude.join("mustard/response-style.md"), "# Response Style\n").unwrap();

        let report = upsert_project(root, None, InstallMode::Shared).unwrap();

        // The stale inject entry is gone, and the old router part gave way to
        // the session map.
        let config = ProjectConfig::load(root);
        assert!(
            !config.inject.iter().any(|e| e.file.ends_with("response-style.md")),
            "response-style entry retired: {:?}",
            config.inject,
        );
        assert_eq!(config.inject, default_inject_entries());
        // The orphaned instruction file is deleted.
        assert!(
            !claude.join("mustard/response-style.md").exists(),
            "orphan response-style.md removed"
        );
        // And the migration is reported.
        assert!(
            report.migrated.iter().any(|m| m.contains("response-style")),
            "migration reported: {:?}",
            report.migrated
        );
    }

    /// Um projeto instalado com o roteador antigo — as três partes declaradas
    /// no `userPromptSubmit`, com os arquivos no disco — sai da instalação com
    /// o mapa do início da sessão no lugar delas e sem os arquivos órfãos. As
    /// outras declarações da pessoa ficam, e a segunda rodada não muda nada.
    #[test]
    fn the_old_router_gives_way_to_the_session_map() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let claude = root.join(".claude");
        std_fs::create_dir_all(claude.join("mustard")).unwrap();
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","inject":[
                {"on":"userPromptSubmit","file":".claude/mustard/orchestrator.md","once":true},
                {"on":"sessionStart","file":"docs/my-rules.md","once":false},
                {"on":"userPromptSubmit","file":"./.claude/mustard/dispatch.md","once":true},
                {"on":"userPromptSubmit","file":".claude/mustard/material.md","once":true}
            ]}"#,
        )
        .unwrap();
        for name in ["orchestrator.md", "dispatch.md", "material.md"] {
            std_fs::write(claude.join("mustard").join(name), "# old router\n").unwrap();
        }

        let report = upsert_project(root, None, InstallMode::Shared).unwrap();

        let config = ProjectConfig::load(root);
        let files: Vec<&str> = config.inject.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(files, vec!["docs/my-rules.md", ".claude/mustard/session-map.md"], "{:?}", config.inject);
        assert_eq!(config.inject[1].on, "sessionStart");
        for name in ["orchestrator.md", "dispatch.md", "material.md"] {
            assert!(!claude.join("mustard").join(name).exists(), "{name} stayed on disk");
        }
        assert!(claude.join("mustard/session-map.md").is_file(), "the map is seeded");
        assert!(report.migrated.iter().any(|m| m.contains("session map")), "{:?}", report.migrated);

        let again = upsert_project(root, None, InstallMode::Shared).unwrap();
        assert!(again.migrated.is_empty(), "the migration converges: {:?}", again.migrated);
        assert_eq!(ProjectConfig::load(root).inject, config.inject);
    }

    /// Uma lista que a pessoa escreveu sem o roteador fica como está: a
    /// migração troca o roteador que o projeto declara, e não impõe o mapa a
    /// quem tirou o roteador da lista.
    #[test]
    fn a_list_without_the_router_is_left_alone() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::write(
            root.join("mustard.json"),
            r#"{"version":"1.0.0","inject":[{"on":"sessionStart","file":"docs/my-rules.md","once":false}]}"#,
        )
        .unwrap();

        let report = upsert_project(root, None, InstallMode::Shared).unwrap();

        assert!(report.migrated.is_empty(), "{:?}", report.migrated);
        let config = ProjectConfig::load(root);
        assert_eq!(config.inject.len(), 1);
        assert_eq!(config.inject[0].file, "docs/my-rules.md");
    }

    /// Um `mustard.json` que já declara o mapa de nome novo e ainda traz o de
    /// nome antigo no mesmo evento fica com uma declaração só, para o mapa não
    /// chegar duas vezes. A de nome antigo em outro evento é trocada no lugar,
    /// e a que só se parece com o caminho antigo fica como está.
    #[test]
    fn a_map_already_declared_by_the_new_name_is_not_declared_twice() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std_fs::write(
            root.join("mustard.json"),
            r#"{"inject":[
                {"on":"sessionStart","file":".claude/mustard/session-map.md","once":false},
                {"on":"SessionStart","file":"./.claude/mustard/mapa-inicio-sessao.md","once":true},
                {"on":"userPromptSubmit","file":".claude/mustard/mapa-inicio-sessao.md","once":true},
                {"on":"sessionStart","file":".claude/mustard/mapa-inicio-sessao.md.bak","once":false}
            ]}"#,
        )
        .unwrap();

        let migrated = migrate_inject_declarations(root, &root.join(".claude"));

        let config = ProjectConfig::load(root);
        let entries: Vec<(&str, &str, bool)> =
            config.inject.iter().map(|e| (e.on.as_str(), e.file.as_str(), e.once)).collect();
        assert_eq!(
            entries,
            [
                ("sessionStart", ".claude/mustard/session-map.md", false),
                ("userPromptSubmit", ".claude/mustard/session-map.md", true),
                ("sessionStart", ".claude/mustard/mapa-inicio-sessao.md.bak", false),
            ],
        );
        assert_eq!(migrated, ["session map (mapa-inicio-sessao.md → session-map.md)"]);
        assert!(migrate_inject_declarations(root, &root.join(".claude")).is_empty(), "the rename converges");
    }

    /// Os textos do Mustard são sempre regravados: uma cópia editada volta ao
    /// texto entregue e sai em `Updated`, e a segunda rodada não muda nada.
    #[test]
    fn the_harness_texts_are_always_rewritten() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        let first = seed_harness_texts(&claude, Locale::PtBr).unwrap();
        assert!(first.iter().all(|(_, o)| *o == SeedOutcome::Created), "{first:?}");
        for (rel, _) in harness_texts(Locale::PtBr) {
            std_fs::write(claude.join(&rel), "# the operator wrote this\n").unwrap();
        }

        let report = seed_harness_texts(&claude, Locale::PtBr).unwrap();

        assert_eq!(report.len(), harness_texts(Locale::PtBr).len());
        for ((reported, outcome), (rel, body)) in report.iter().zip(harness_texts(Locale::PtBr)) {
            assert_eq!(*reported, rel, "the report must name the file it wrote");
            assert_eq!(*outcome, SeedOutcome::Updated, "{rel} diverged and was replaced silently");
            assert_eq!(std_fs::read_to_string(claude.join(&rel)).unwrap(), body, "{rel} kept the edit");
        }
        let again = seed_harness_texts(&claude, Locale::PtBr).unwrap();
        for (rel, outcome) in &again {
            assert_eq!(*outcome, SeedOutcome::Preserved, "{rel} rewritten with no change");
        }
    }

    /// Um texto que existe e não se lê é regravado, e não preservado: a
    /// correção chega ao projeto mesmo sobre um arquivo corrompido.
    #[test]
    fn an_unreadable_harness_text_is_rewritten_not_silently_preserved() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        for (rel, _) in harness_texts(Locale::EnUs) {
            let path = claude.join(&rel);
            std_fs::create_dir_all(path.parent().unwrap()).unwrap();
            std_fs::write(path, [0x80_u8, 0xFF, 0xFE]).unwrap();
        }

        let report = seed_harness_texts(&claude, Locale::EnUs).unwrap();

        for ((rel, outcome), (_, body)) in report.iter().zip(harness_texts(Locale::EnUs)) {
            assert_eq!(*outcome, SeedOutcome::Updated, "{rel} was unreadable and reported `{outcome:?}`");
            assert_eq!(std_fs::read_to_string(claude.join(rel)).unwrap(), body);
        }
    }

    /// Trocar o idioma e instalar de novo troca o texto no mesmo arquivo: o
    /// caminho não depende do idioma, e o projeto nunca fica com os dois.
    #[test]
    fn a_language_change_swaps_the_text_in_place() {
        let dir = tempdir().unwrap();
        let claude = dir.path().join(".claude");
        seed_harness_texts(&claude, Locale::PtBr).unwrap();
        let swapped = seed_harness_texts(&claude, Locale::EnUs).unwrap();
        assert!(swapped.iter().all(|(_, o)| *o == SeedOutcome::Updated), "{swapped:?}");
        for (rel, body) in harness_texts(Locale::EnUs) {
            assert_eq!(std_fs::read_to_string(claude.join(&rel)).unwrap(), body, "{rel}");
        }
        let agents: Vec<String> = std_fs::read_dir(claude.join("agents/mustard"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(agents.len(), 3, "{agents:?}");
    }

    /// O mapa é declarado no início da sessão, o único evento que entrega
    /// texto declarado, e cada janela nova o recebe.
    #[test]
    fn the_session_map_rides_the_session_start() {
        let entries = default_inject_entries();
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].on, "sessionStart");
        assert_eq!(entries[0].file, session_map_declared_path());
        assert!(!entries[0].once, "the session start only runs on a new window");
        assert!(harness_text_paths().contains(&"mustard/session-map.md".to_string()));
    }
}
