//! As sobras, só com `--residue`: menções a arquivos que não existem mais no
//! `settings.json` e nos `.md` do `.claude/`, e o tamanho do que o `clean`
//! recolheria. Aqui mora também a caminhada pelas pastas, que o desvio dos
//! moldes usa para calcular o hash.

use std::path::{Path, PathBuf};

use mustard_core::io::fs;

use super::{CheckResult, Status};

/// Scan `settings.json`, SKILL.md files, and refs for dead references —
/// `.js` script names that no longer exist, `scripts/` paths with no
/// resolvable target. WARN per hit. Only run when `--residue` is passed.
pub(super) fn check_residue(claude_dir: &Path) -> CheckResult {
    let mut hits: Vec<String> = Vec::new();

    // Check for dead .js script references in settings.json.
    let settings_path = claude_dir.join("settings.json");
    if let Ok(text) = fs::read_to_string(&settings_path) {
        scan_for_dead_js_refs(&text, claude_dir, "settings.json", &mut hits);
    }

    // Scan SKILL.md files for dead .js references.
    scan_md_files_for_dead_refs(claude_dir, &mut hits);

    // Check if CORE_FOLDERS lists scripts/ but no scripts exist.
    let scripts_dir = claude_dir.join("scripts");
    if fs::exists(&scripts_dir) {
        match fs::read_dir(&scripts_dir) {
            Ok(entries) => {
                if entries.is_empty() {
                    hits.push("scripts/ directory is empty (CORE_FOLDER with no content)".to_string());
                }
            }
            Err(e) => {
                hits.push(format!("cannot read scripts/: {e}"));
            }
        }
    }

    if hits.is_empty() {
        CheckResult::ok("residue")
    } else {
        CheckResult::warn("residue", hits)
    }
}

/// Sobras de cópias descartáveis e tamanho da compilação compartilhada, lidos
/// pela varredura do `scratch-gc` — uma leitura só, para o doctor e a porta
/// de limpeza nunca discordarem sobre o que é sobra. Só com `--residue`: a
/// medida percorre cada candidata inteira.
pub(super) fn check_scratch_residue(roots: &crate::commands::maint::scratch_gc::ScratchRoots) -> CheckResult {
    use crate::commands::maint::scratch_gc::{human_bytes, survey, MIN_AGE_HOURS};

    let found = survey(roots);
    let count = found.candidates.len();
    let mut details = vec![format!(
        "{count} scratch leftover(s) older than {MIN_AGE_HOURS}h: {} - list with `mustard-rt run clean`, remove with `--apply`",
        human_bytes(found.candidates_bytes())
    )];
    let over_cap = found.shared_target.as_ref().is_some_and(|s| s.over_cap);
    match found.shared_target.as_ref() {
        Some(shared) => details.push(format!(
            "shared build {}: {} (cap {}){}",
            shared.path,
            human_bytes(shared.size_bytes),
            human_bytes(shared.cap_bytes),
            if shared.over_cap { " - over the cap, `mustard-rt run clean --apply` empties it" } else { "" }
        )),
        None => details.push("shared build: not present".to_string()),
    }
    let status = if count > 0 || over_cap { Status::Warn } else { Status::Ok };
    CheckResult { name: "scratch-residue", status, details }
}

/// Scan text for `.js` filename patterns and check if they exist under
/// `.claude/` or `hooks/`.
fn scan_for_dead_js_refs(text: &str, claude_dir: &Path, source: &str, hits: &mut Vec<String>) {
    for word in text.split_whitespace() {
        // Strip leading quotes or path separators for matching.
        let clean = word.trim_matches(|c: char| c == '"' || c == '\'' || c == ',');
        if clean.ends_with(".js") && !clean.contains("://") {
            // Resolve relative to claude_dir or its parent (project root).
            let project_root = claude_dir.parent().unwrap_or(claude_dir);
            let candidate_claude = claude_dir.join(clean);
            let candidate_root = project_root.join(clean);
            if !candidate_claude.exists() && !candidate_root.exists() {
                hits.push(format!("dead .js reference '{clean}' in {source}"));
            }
        }
    }
}

/// Walk `.claude/` looking for SKILL.md files and scan them for dead refs.
fn scan_md_files_for_dead_refs(claude_dir: &Path, hits: &mut Vec<String>) {
    let walker = collect_files_recursive(claude_dir, 4);
    for path in walker {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".md")
            && let Ok(text) = fs::read_to_string(&path) {
                let source = path.to_string_lossy().into_owned();
                scan_for_dead_js_refs(&text, claude_dir, &source, hits);
            }
    }
}

/// Collect all files under `dir` up to `max_depth` levels deep. Fail-open.
fn collect_files_recursive(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();
    collect_recursive_inner(dir, max_depth, 0, &mut results);
    results
}

pub(super) fn collect_recursive_inner(dir: &Path, max_depth: usize, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        if entry.is_dir {
            collect_recursive_inner(&entry.path, max_depth, depth + 1, out);
        } else {
            out.push(entry.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::commands::doctor::doctor::tests::*;

    #[test]
    fn residue_detects_dead_js_reference() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        // Plant a settings.json that references a .js file that doesn't exist.
        write_file(
            &claude_dir.join("settings.json"),
            r#"{ "command": "node .claude/scripts/dead-hook.js" }"#,
        );
        let result = check_residue(&claude_dir);
        assert_eq!(result.status, Status::Warn);
        let found = result.details.iter().any(|d| d.contains("dead-hook.js"));
        assert!(found, "expected dead-hook.js hit, got: {:?}", result.details);
    }

    #[test]
    fn residue_clean_dir_is_ok() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        write_file(&claude_dir.join("settings.json"), r#"{ "foo": "bar" }"#);
        let result = check_residue(&claude_dir);
        assert_eq!(result.status, Status::Ok);
    }

    /// Com sobra candidata, o `--residue` relata o tamanho dela e o da
    /// compilação compartilhada.
    #[test]
    fn doctor_residue_reports_scratch_leftovers() {
        use crate::commands::maint::scratch_gc::{backdate_tree, human_bytes, AgeClock, ScratchRoots};

        let base = tempdir().unwrap();
        let temp_root = base.path().join("tmp");
        let old = temp_root.join("tmp.old");
        std::fs::create_dir_all(old.join("apps").join("rt")).unwrap();
        write_file(&old.join("Cargo.toml"), "[workspace]\n");
        std::fs::write(old.join("apps").join("rt").join("big.bin"), vec![0u8; 3 * 1024]).unwrap();
        // A árvore inteira envelhecida pelo mtime, o relógio das fixtures.
        backdate_tree(&old, 24);
        let candidate_bytes = std::fs::metadata(old.join("Cargo.toml")).unwrap().len() + 3 * 1024;

        let shared = base.path().join("cache").join("scratch-target");
        std::fs::create_dir_all(shared.join("debug")).unwrap();
        std::fs::write(shared.join("debug").join("lib.rlib"), vec![0u8; 2048]).unwrap();

        let roots = ScratchRoots {
            temp_root,
            shared_target: Some(shared.clone()),
            cap_bytes: 1024 * 1024,
            current_session: "sess-current".to_string(),
            current_dir: None,
            home: None,
            clock: AgeClock::Modified,
            owner_uid: crate::commands::maint::scratch_gc::current_uid(),
            now: std::time::SystemTime::now(),
        };
        let result = check_scratch_residue(&roots);

        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let text = result.details.join("\n");
        assert!(
            text.contains(&format!("1 scratch leftover(s) older than 12h: {}", human_bytes(candidate_bytes))),
            "{text}"
        );
        // O comando de limpeza que a linha manda rodar é o que existe.
        assert!(text.contains("list with `mustard-rt run clean`"), "{text}");
        assert!(
            text.contains(&format!("shared build {}: {}", shared.display(), human_bytes(2048))),
            "{text}"
        );
        assert!(old.exists(), "the doctor only reads");
    }
}
