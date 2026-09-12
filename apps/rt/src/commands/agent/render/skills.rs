//! `{skills_list}` — the target subproject's skill shelf: one line per
//! `<subproject>/.claude/skills/*/SKILL.md` (`- name — description`), names and
//! trigger descriptions only (never bodies) so the `## SKILLS` section stays
//! PREFIX-STABLE (cache-safe).
//!
//! `{mold_pointer}` — the shelf's other half: which of those molds actually
//! govern the files THIS wave will touch. The shelf is a catalogue and is
//! identical for every wave of a spec; the pointer is a prescription for one
//! wave. They are two placeholders and two sections on purpose — folding the
//! pointer into the shelf would make the shelf vary per wave and cost the
//! prefix that sibling waves share, for a line that reads just as well one
//! section further down.

use std::fmt::Write as _;
use std::path::Path;

use crate::util::glob::glob_match;

/// Build `{skills_list}` — the target subproject's skill shelf: one line per
/// `<subproject>/.claude/skills/*/SKILL.md` (`- name — description`), sorted
/// by folder name for byte-stable output, preceded by the load instruction.
/// Names and trigger descriptions only — never bodies — so the `## SKILLS`
/// section stays PREFIX-STABLE (cache-safe) exactly as the agent-prompt ref
/// documents. Empty (the section collapses) when the subproject has no
/// readable skills. Fail-open: an unparseable SKILL.md contributes its folder
/// name without a description rather than dropping the shelf.
pub(crate) fn build_skills_list(project: &Path, subproject: &str) -> String {
    let skills_dir = project.join(subproject).join(".claude").join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return String::new();
    };
    let mut rows: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let skill_md = entry.path().join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&skill_md) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let description = mustard_core::domain::skill::frontmatter::parse(&text)
            .map(|fm| fm.description)
            .unwrap_or_default();
        rows.push((name, description));
    }
    if rows.is_empty() {
        return String::new();
    }
    rows.sort();
    let mut out = String::from(
        "This subproject has skills — its module molds and conventions. BEFORE creating \
         or refactoring a module of a kind listed below, load the matching skill (Skill \
         tool, or Read its SKILL.md) and follow it; deviations are review findings:\n",
    );
    for (name, description) in rows {
        if description.is_empty() {
            let _ = writeln!(out, "- {name}");
        } else {
            let _ = writeln!(out, "- {name} — {description}");
        }
    }
    out.trim_end().to_string()
}

/// Build `{mold_pointer}` — the molds whose `paths:` cover the files this wave
/// declares, one line each, sorted by mold name for byte-stable output.
///
/// Both halves of this answer were already written down, in two places, and
/// nothing crossed them: the wave declares its files under `## Arquivos`, and
/// every mold declares under `paths:` the folders it governs. Handing the agent
/// the whole shelf and leaving the crossing to it is a weak instruction — a
/// menu asks for a choice where a name asks for obedience, and the choice falls
/// due at the worst moment, before the agent knows the terrain.
///
/// Empty (the section collapses) when nothing matches, when the spec names no
/// files, or when the subproject carries no mold — a dispatch with nothing to
/// prescribe prescribes nothing rather than gesturing at the catalogue again.
/// Fail-open throughout: an unreadable spec or an unparseable mold drops out of
/// the crossing instead of sinking the block.
pub(crate) fn build_mold_pointer(project: &Path, subproject: &str, spec_path: &Path) -> String {
    let Ok(spec_text) = std::fs::read_to_string(spec_path) else {
        return String::new();
    };
    let files = arquivos_paths(&spec_text);
    let covers = wave_molds(project, subproject, &files);
    if covers.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "These molds govern the files this wave touches — `## SKILLS` above is the \
         catalogue, this is the PRESCRIPTION. Load each one named here (Skill tool, or \
         Read its SKILL.md) BEFORE writing the first line of the module it governs; \
         deviating from a mold named here is a review finding:\n",
    );
    for cover in covers {
        let files: Vec<String> = cover.files.iter().map(|f| format!("`{f}`")).collect();
        let _ = writeln!(out, "- {} — covers {}", cover.name, files.join(", "));
    }
    out.trim_end().to_string()
}

/// Um molde e TODOS os arquivos da onda que ele cobre.
///
/// A forma estruturada do cruzamento: o prompt da onda a transforma em prosa
/// (`build_mold_pointer`) e o resumo da spec em HTML, cada um no seu idioma, a
/// partir da mesma lista — dois leitores do cruzamento com duas contas próprias
/// divergiriam sobre qual molde governa qual arquivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MoldCover {
    /// O nome da pasta do molde (`rt-entry-pattern`).
    pub(crate) name: String,
    /// Os arquivos da onda que caem sob o `paths:` do molde, ordenados.
    pub(crate) files: Vec<String>,
}

/// Os moldes de `<subproject>/.claude/skills/` cujo `paths:` cobre algum dos
/// `files`, cada um com a lista COMPLETA dos arquivos que cobre, ordenados
/// pelo nome do molde para a saída ser estável byte a byte.
///
/// Antes só o primeiro arquivo de cada molde era guardado, e uma onda com três
/// comandos sob o mesmo molde lia como se só um deles o seguisse.
///
/// Vazio quando nada casa, quando `files` está vazio ou quando o subprojeto não
/// tem moldes. Fail-open: um SKILL.md ilegível ou sem frontmatter sai do
/// cruzamento em vez de derrubar a lista.
pub(crate) fn wave_molds(project: &Path, subproject: &str, files: &[String]) -> Vec<MoldCover> {
    if files.is_empty() {
        return Vec::new();
    }
    let skills_dir = project.join(subproject).join(".claude").join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return Vec::new();
    };
    let mut covers: Vec<MoldCover> = Vec::new();
    for entry in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("SKILL.md")) else {
            continue;
        };
        let Ok(fm) = mustard_core::domain::skill::frontmatter::parse(&text) else {
            continue;
        };
        // A mold with no `paths:` governs no folder and can cover nothing.
        let mut hits: Vec<String> = files
            .iter()
            .filter(|f| fm.paths.iter().any(|g| glob_match(f, g)))
            .cloned()
            .collect();
        if hits.is_empty() {
            continue;
        }
        hits.sort();
        hits.dedup();
        covers.push(MoldCover { name: entry.file_name().to_string_lossy().into_owned(), files: hits });
    }
    covers.sort_by(|a, b| a.name.cmp(&b.name));
    covers
}

/// Every repo-relative path the spec's `## Arquivos` / `## Files` section names.
///
/// The section is a bullet list in some specs and a table in others, and in both
/// the path is spelled inside backticks. So the read is on the BACKTICKED SPANS
/// rather than on the row shape: a table row contributes its file cell and not
/// its prose cell, and a bullet contributes its path either way. A bullet that
/// carries no backticks still gives up its first slash-bearing token, because
/// older specs wrote the path plain.
///
/// `pub(crate)` para o resumo da spec ler os arquivos de cada onda por este
/// mesmo leitor — o cruzamento com os moldes só concorda com o prompt enquanto
/// os dois partem da mesma lista.
pub(crate) fn arquivos_paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("## ") {
            // Entering the section, or the next heading ending it.
            in_section = trimmed == "## Arquivos" || trimmed == "## Files";
            continue;
        }
        if !in_section {
            continue;
        }
        let backticked: Vec<&str> = line.split('`').skip(1).step_by(2).collect();
        if backticked.iter().any(|s| s.contains('/')) {
            out.extend(backticked.iter().filter(|s| s.contains('/')).map(|s| s.trim().to_string()));
            continue;
        }
        let lead = line.trim_start();
        if (lead.starts_with("- ") || lead.starts_with("* "))
            && let Some(tok) = lead[2..].split_whitespace().find(|t| t.contains('/')) {
                out.push(tok.trim_matches(|c: char| !c.is_ascii_graphic()).to_string());
            }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_list_renders_shelf_sorted_and_fails_open() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let skills = root.join("apps/api/.claude/skills");
        // Two parseable skills + one broken (no frontmatter) — the broken one
        // still contributes its name (fail-open), never sinks the shelf.
        for (folder, desc) in [
            ("api-service-pattern", "Use when adding or refactoring a service."),
            ("api-log-pattern", "Use when adding or refactoring an audit log entity."),
        ] {
            let d = skills.join(folder);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("SKILL.md"),
                format!("---\nname: {folder}\ndescription: \"{desc}\"\nsource: scan\n---\n\nbody\n"),
            )
            .unwrap();
        }
        let broken = skills.join("api-odd-pattern");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("SKILL.md"), "# no frontmatter\n").unwrap();

        let out = build_skills_list(root, "apps/api");
        assert!(out.contains("- api-log-pattern — Use when adding or refactoring an audit log entity."), "{out}");
        assert!(out.contains("- api-service-pattern — Use when adding"), "{out}");
        assert!(out.contains("- api-odd-pattern"), "broken skill keeps its name: {out}");
        // Sorted by name: log < odd < service.
        let (log, odd, svc) = (
            out.find("api-log-pattern").unwrap(),
            out.find("api-odd-pattern").unwrap(),
            out.find("api-service-pattern").unwrap(),
        );
        assert!(log < odd && odd < svc, "shelf must be sorted: {out}");
        // No skills dir → empty (the ## SKILLS section collapses).
        assert!(build_skills_list(root, "apps/none").is_empty());
    }

    /// Two molds, and only one governs a folder this wave touches. The mold the
    /// files fall under is NAMED; the sibling that governs another folder is
    /// not, which is the whole point — a list of two is already a choice handed
    /// back, and sixteen is what the shelf hands back today.
    #[test]
    fn the_wave_names_the_mold_its_files_fall_under() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_mold(root, "rt-gate-pattern", "apps/rt/src/hooks/**");
        seed_mold(root, "rt-cmd-pattern", "apps/rt/src/commands/**");

        // A table-shaped `## Arquivos`, the shape this repository's specs use.
        let spec = root.join("spec.md");
        std::fs::write(
            &spec,
            "## Contexto\n\nprosa com `apps/rt/src/commands/nada.rs` que NÃO conta.\n\n\
             ## Arquivos\n\n| arquivo | papel |\n|---|---|\n\
             | `apps/rt/src/hooks/write/post_edit.rs` | o portão |\n\n\
             ## Critérios de Aceitação\n\n- AC-1 — algo\n",
        )
        .unwrap();

        let out = build_mold_pointer(root, "apps/rt", &spec);
        assert!(
            out.contains("rt-gate-pattern — covers `apps/rt/src/hooks/write/post_edit.rs`"),
            "the mold the wave's files fall under is named: {out}"
        );
        assert!(
            !out.contains("rt-cmd-pattern"),
            "a mold governing another folder is NOT named — prose outside `## Arquivos` \
             must not reach the crossing: {out}"
        );
    }

    /// The block earns its own section by collapsing when it has nothing to say,
    /// and by leaving the shelf byte-identical when it does. The shelf is shared
    /// by every wave of a spec; a pointer folded into it would make it vary per
    /// wave and cost that shared prefix.
    #[test]
    fn the_pointer_collapses_and_leaves_the_shelf_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_mold(root, "rt-gate-pattern", "apps/rt/src/hooks/**");
        let shelf_before = build_skills_list(root, "apps/rt");
        assert!(!shelf_before.is_empty(), "the shelf itself still renders");

        // Files that fall under no mold → nothing to prescribe.
        let spec = root.join("spec.md");
        std::fs::write(&spec, "## Arquivos\n\n- `docs/leia-me.md`\n").unwrap();
        assert!(
            build_mold_pointer(root, "apps/rt", &spec).is_empty(),
            "no match prescribes nothing"
        );

        // No `## Arquivos` at all (a spec-less or file-less dispatch) → same.
        let bare = root.join("bare.md");
        std::fs::write(&bare, "## Contexto\n\nsem seção de arquivos\n").unwrap();
        assert!(build_mold_pointer(root, "apps/rt", &bare).is_empty(), "no files, no block");

        // An unreadable spec path fails open rather than panicking.
        assert!(build_mold_pointer(root, "apps/rt", &root.join("nao-existe.md")).is_empty());

        // And through all of it the shelf never moved a byte.
        assert_eq!(shelf_before, build_skills_list(root, "apps/rt"), "the shelf is untouched");
    }

    /// AC-6 — um molde que cobre vários arquivos da onda os nomeia TODOS, na
    /// lista estruturada e na linha do prompt, e não só o primeiro que casou.
    #[test]
    fn wave_molds_list_every_covered_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        seed_mold(root, "rt-entry-pattern", "apps/rt/src/commands/**");
        seed_mold(root, "rt-gate-pattern", "apps/rt/src/hooks/**");

        let spec = root.join("spec.md");
        std::fs::write(
            &spec,
            "## Arquivos\n\n\
             - `apps/rt/src/commands/spec/spec_doc.rs`\n\
             - `apps/rt/src/commands/spec/cli.rs`\n\
             - `apps/rt/src/commands/spec/mod.rs`\n\
             - `docs/fora-de-molde.md`\n",
        )
        .unwrap();
        let files = arquivos_paths(&std::fs::read_to_string(&spec).unwrap());

        let covers = wave_molds(root, "apps/rt", &files);
        assert_eq!(
            covers,
            vec![MoldCover {
                name: "rt-entry-pattern".to_string(),
                files: vec![
                    "apps/rt/src/commands/spec/cli.rs".to_string(),
                    "apps/rt/src/commands/spec/mod.rs".to_string(),
                    "apps/rt/src/commands/spec/spec_doc.rs".to_string(),
                ],
            }],
            "every covered file is listed, and a mold that covers none is absent"
        );

        let out = build_mold_pointer(root, "apps/rt", &spec);
        assert!(
            out.contains(
                "- rt-entry-pattern — covers `apps/rt/src/commands/spec/cli.rs`, \
                 `apps/rt/src/commands/spec/mod.rs`, `apps/rt/src/commands/spec/spec_doc.rs`"
            ),
            "the prompt line names all three files: {out}"
        );
        assert!(!out.contains("rt-gate-pattern"), "{out}");
    }

    /// A mold under `<subproject>/.claude/skills/<name>/SKILL.md` governing
    /// `glob`, with the frontmatter shape `scan-patterns-apply` writes.
    fn seed_mold(root: &Path, name: &str, glob: &str) {
        let d = root.join("apps/rt/.claude/skills").join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: \"Use when adding or refactoring an X.\"\n\
                 paths:\n  - {glob}\nsource: scan\n---\n\n## Purpose\nbody\n"
            ),
        )
        .unwrap();
    }
}
