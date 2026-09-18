//! O estado das specs: a saúde de cada pasta de spec contra o arquivo de
//! eventos dela, os links de onda do `wave-plan.md` que apontam para pasta
//! nenhuma e o índice das specs contra os arquivos de eventos.

use std::path::Path;

use mustard_core::io::fs;
use mustard_core::platform::i18n::{translate, Locale};

use super::CheckResult;

/// The health of the project's STATE, read from the spec event files.
///
/// **It used to read the old state folder.** `.claude/.pipeline-states/` held
/// one JSON per spec, and this check walked it for two findings: a state file
/// whose spec no longer existed, and a `closed-followup` older than a day.
/// Both findings were about a folder the harness stopped writing — so on a
/// project that never had it, the check was silent about everything, and on an
/// old one it reported the leftovers of a format nobody reads. The state of a
/// spec lives in its own event file now, and that is what this asks.
///
/// Two findings, both about the file the whole harness reads: a spec folder
/// with no event file at all (nothing states what it is), and an event file
/// that cannot be read. Plus the repository model (`grain.model.json`) the
/// scan produces, which is not state but is the other thing whose absence
/// makes every later answer worse.
pub(super) fn check_state_health(claude_dir: &Path) -> CheckResult {
    let mut warnings: Vec<String> = Vec::new();

    if !claude_dir.join("grain.model.json").exists() {
        warnings.push("grain.model.json missing (run `mustard-rt run scan`)".to_string());
    }

    let root = claude_dir
        .parent()
        .filter(|_| claude_dir.file_name().and_then(|s| s.to_str()) == Some(".claude"))
        .map_or_else(|| claude_dir.to_path_buf(), Path::to_path_buf);
    for spec in collect_active_spec_names(claude_dir) {
        let Ok(path) = mustard_core::io::spec_events::spec_file(&root, &spec) else {
            warnings.push(format!("'{spec}' is not a name a spec can have"));
            continue;
        };
        if !path.exists() {
            warnings.push(format!(
                "'{spec}' has no event file — nothing states what it is or where it stands"
            ));
            continue;
        }
        match mustard_core::io::spec_events::read(&path) {
            Ok(Some(log)) if log.events.is_empty() => {
                warnings.push(format!("'{spec}' has an empty event file"));
            }
            Ok(_) => {}
            Err(refusal) => {
                warnings.push(format!("'{spec}' has an unreadable event file: {}", refusal.reason()));
            }
        }
    }

    if warnings.is_empty() {
        CheckResult::ok("state-health")
    } else {
        CheckResult::warn("state-health", warnings)
    }
}

/// Collect the directory names under `.claude/spec/` (flat layout — no buckets).
fn collect_active_spec_names(claude_dir: &Path) -> Vec<String> {
    // ClaudePaths-exempt: `claude_dir` is already resolved via the seam in
    // `run()`; re-deriving with `for_project` here would be circular.
    let active_dir = claude_dir.join("spec");
    let Ok(entries) = fs::read_dir(&active_dir) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.file_name)
        .collect()
}

/// For each active spec under `.claude/spec/`, parse `wave-plan.md` for
/// `[[wave-N-<role>]]` wikilinks and verify each referenced subdirectory
/// exists. WARN per broken wikilink (an editor typo or partial scaffold);
/// FAIL only on an empty result paired with a non-empty wave-plan body.
/// Fail-open: a missing spec tree, unreadable file, or malformed wikilink is
/// silently ignored — better to skip a check than crash the doctor.
pub(super) fn check_wave_integrity(claude_dir: &Path) -> CheckResult {
    // ClaudePaths-exempt: `claude_dir` is already resolved via the seam in
    // `run()`; re-deriving with `for_project` here would be circular.
    let spec_root = claude_dir.join("spec");
    let Ok(entries) = fs::read_dir(&spec_root) else {
        return CheckResult::skip("wave-integrity", "no .claude/spec/ directory");
    };
    let mut warnings: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let plan_path = entry.path.join("wave-plan.md");
        if !plan_path.is_file() {
            continue;
        }
        scanned += 1;
        let Ok(text) = fs::read_to_string(&plan_path) else {
            continue;
        };
        for link in extract_wave_wikilinks(&text) {
            let dir = entry.path.join(&link);
            if !dir.is_dir() {
                warnings.push(format!(
                    "{spec}: [[{link}]] -> directory missing",
                    spec = entry.file_name,
                ));
            }
        }
    }
    if scanned == 0 {
        return CheckResult::skip("wave-integrity", "no wave-plan.md files found");
    }
    if warnings.is_empty() {
        let mut r = CheckResult::ok("wave-integrity");
        r.details.push(format!("scanned {scanned} wave-plan(s) — no missing dirs"));
        r
    } else {
        CheckResult::warn("wave-integrity", warnings)
    }
}

/// Pull every `[[wave-N-<role>]]` wikilink from raw markdown. Matches the
/// `wave-N-{role}` shape only; ignores generic `[[link]]` references so cross-
/// links to non-wave concept nodes don't trigger spurious warnings.
fn extract_wave_wikilinks(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            // Find the closing `]]` on the same line.
            if let Some(end) = text[i + 2..].find("]]") {
                let link = &text[i + 2..i + 2 + end];
                // Cut piped text (e.g. `[[wave-1-rt|label]]`).
                let core = link.split('|').next().unwrap_or(link).trim();
                if is_wave_link(core) && !out.iter().any(|s| s == core) {
                    out.push(core.to_string());
                }
                i += 2 + end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Loose recogniser for `wave-{N}-{role}` — `N` numeric, role non-empty.
fn is_wave_link(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("wave-") else {
        return false;
    };
    let Some((n, role)) = rest.split_once('-') else {
        return false;
    };
    !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && !role.is_empty()
}

/// O índice das specs do projeto `root` contra os arquivos de eventos. Só lê:
/// sem spec, não há o que conferir; índice que falta, linha que diverge e
/// `search` calculado por outro redutor viram WARN, cada um com a mensagem no
/// idioma `lang`, que manda rodar `mustard-rt run index`. Um erro de leitura
/// também é WARN: a conferência nunca derruba o `doctor`.
pub(super) fn check_spec_index(root: &Path, lang: Locale) -> CheckResult {
    const NAME: &str = "spec-index";
    let divergence = match mustard_core::io::spec_index::divergence(root) {
        Ok(divergence) => divergence,
        Err(refusal) => return CheckResult::warn(NAME, vec![refusal.message(lang)]),
    };
    if divergence.specs == 0 && divergence.stale_search == 0 {
        return CheckResult::skip(NAME, translate("spec_index.no_specs", lang));
    }
    let mut details = Vec::new();
    if divergence.specs > 0 && !divergence.index_exists {
        details.push(translate("spec_index.missing", lang).replace("{count}", &divergence.specs.to_string()));
    }
    if !divergence.diverged.is_empty() {
        details.push(
            translate("spec_index.diverged", lang)
                .replace("{count}", &divergence.diverged.len().to_string())
                .replace("{specs}", &divergence.diverged.join(", ")),
        );
    }
    if divergence.stale_search > 0 {
        details.push(
            translate("spec_index.stale_search", lang).replace("{count}", &divergence.stale_search.to_string()),
        );
    }
    if details.is_empty() {
        CheckResult::ok(NAME)
    } else {
        CheckResult::warn(NAME, details)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;
    use tempfile::tempdir;

    use super::super::Status;
    use super::*;
    use crate::commands::doctor::doctor::tests::*;

    // --- state health tests ---

    /// Uma spec cuja pasta existe e cujo arquivo de eventos não: nada diz o
    /// que ela é nem onde ela está, e o diagnóstico acusa isso pelo nome.
    #[test]
    fn a_spec_sem_arquivo_de_eventos_vira_achado() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(claude_dir.join("spec").join("trava")).unwrap();
        write_file(&claude_dir.join("grain.model.json"), "{}");

        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        assert!(
            result.details.iter().any(|d| d.contains("trava")),
            "a spec é acusada pelo nome: {:?}",
            result.details
        );
    }

    /// Uma spec com arquivo de eventos não é achado nenhum — e a pasta velha
    /// de estado, com o que quer que tenha sobrado dentro, também não: ela
    /// deixou de ser lida.
    #[test]
    fn uma_spec_com_arquivo_de_eventos_esta_sa() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let claude_dir = root.join(".claude");
        std::fs::create_dir_all(claude_dir.join("spec").join("trava")).unwrap();
        write_file(&claude_dir.join("grain.model.json"), "{}");
        let states = claude_dir.join(".pipeline-states");
        std::fs::create_dir_all(&states).unwrap();
        write_file(&states.join("orfa.json"), r#"{ "spec": "nao-existe", "state": "execute" }"#);

        let path = mustard_core::io::spec_events::spec_file(root, "trava").expect("caminho");
        std::fs::create_dir_all(path.parent().expect("pasta")).unwrap();
        let draft = |value: serde_json::Value| value.as_object().cloned().expect("um objeto");
        mustard_core::io::spec_events::write(&path, "state", draft(json!({"phase": "running"})), &[])
            .expect("estado");

        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    #[test]
    fn state_health_missing_model_warns() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Warn);
        let has_model = result.details.iter().any(|d| d.contains("grain.model.json"));
        assert!(has_model, "expected model warning, got: {:?}", result.details);
    }

    #[test]
    fn state_health_clean_install_is_ok() {
        let dir = tempdir().unwrap();
        let claude_dir = dir.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        write_file(&claude_dir.join("grain.model.json"), "{}");
        let result = check_state_health(&claude_dir);
        assert_eq!(result.status, Status::Ok, "{:?}", result.details);
    }

    // --- spec-index tests ---

    fn spec_event(root: &Path, spec: &str, text: &str) {
        let path = root.join(".claude").join("spec").join(spec).join("spec.ndjson");
        let serde_json::Value::Object(draft) = json!({"author": "user", "text": text}) else { unreachable!() };
        mustard_core::io::spec_events::write_at(&path, "message", draft, &[], "2026-09-11T10:00:00-03:00").unwrap();
    }

    fn spec_index_file(root: &Path) -> PathBuf {
        root.join(".claude").join("spec").join("index.ndjson")
    }

    /// Uma linha do índice mexida à mão faz o doctor acusar a spec pelo nome,
    /// com o comando que conserta; depois do `index`, a conferência passa.
    #[test]
    fn the_doctor_flags_a_divergent_index_line() {
        let dir = tempdir().unwrap();
        spec_event(dir.path(), "trava", "um");
        spec_event(dir.path(), "busca", "dois");
        let index = spec_index_file(dir.path());
        let raw = std::fs::read_to_string(&index).unwrap();
        std::fs::write(&index, raw.replace("\"name\":\"trava\"", "\"name\":\"trava\",\"phase\":\"closed\"")).unwrap();

        let result = check_spec_index(dir.path(), Locale::PtBr);
        assert_eq!(result.status, Status::Warn, "{:?}", result.details);
        let detail = result.details.join(" ");
        assert!(detail.contains("difere") && detail.contains("trava"), "{detail}");
        assert!(!detail.contains("busca"), "only the divergent spec is named: {detail}");
        assert!(detail.contains("mustard-rt run index"), "{detail}");

        mustard_core::io::spec_index::rebuild(dir.path()).unwrap();
        assert_eq!(check_spec_index(dir.path(), Locale::PtBr).status, Status::Ok);
    }

    #[test]
    fn the_doctor_flags_a_missing_index_and_is_quiet_when_it_matches() {
        let dir = tempdir().unwrap();
        assert_eq!(check_spec_index(dir.path(), Locale::EnUs).status, Status::Skip, "no spec, nothing to check");
        spec_event(dir.path(), "trava", "um");
        let quiet = check_spec_index(dir.path(), Locale::EnUs);
        assert_eq!(quiet.status, Status::Ok, "{:?}", quiet.details);

        std::fs::remove_file(spec_index_file(dir.path())).unwrap();
        let missing = check_spec_index(dir.path(), Locale::EnUs);
        assert_eq!(missing.status, Status::Warn);
        assert!(missing.details[0].contains("does not exist"), "{:?}", missing.details);
        assert!(missing.details[0].contains("mustard-rt run index"), "{:?}", missing.details);
    }
}
