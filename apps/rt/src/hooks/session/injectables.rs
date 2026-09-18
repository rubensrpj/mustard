//! `injectables` — os textos declarados que o início da sessão coloca.
//!
//! `mustard.json#inject` declara `[{on, file, once}]`: arquivos de instrução
//! (em geral `.claude/mustard/*.md`, que a pessoa pode editar) que entram na
//! janela como `additionalContext`. Só as entradas `on: sessionStart` são
//! lidas, e só pelo início da sessão ([`super::session_start_inject`]): a
//! mensagem do usuário não entrega injetável nenhum.
//!
//! ## Uma vez por sessão
//!
//! A entrada com `once: true` sai uma vez por sessão. A marca da entrega é o
//! arquivo `.claude/.session/<session_id>/injected-<nome>`. Uma sessão sem id
//! que sirva (vazio ou `"unknown"`) não guarda marca, e o `once` vira "toda
//! vez": entregar duas vezes é a falha segura; nunca entregar, não. Depois de
//! `/clear` e da compactação, a janela perdeu o texto, e ele volta mesmo com a
//! marca.
//!
//! ## Nunca barra
//!
//! Configuração que não se lê → nenhum texto. Arquivo declarado que falta ou
//! está vazio → a entrada é pulada, com uma linha no stderr. Marca que não se
//! grava → o texto sai assim mesmo.

use mustard_core::io::fs;
use mustard_core::{ClaudePaths, ProjectConfig};
use std::path::{Path, PathBuf};

/// O começo do nome das marcas de entrega.
const MARKER_PREFIX: &str = "injected-";

/// O gatilho das entradas lidas aqui, como a configuração o normaliza.
const SESSION_START: &str = "sessionstart";

/// Os textos declarados para o início da sessão, separados por uma linha em
/// branco, ou `None` quando nada se aplica.
///
/// Para cada entrada `on: sessionStart`: respeita a marca do `once` (menos
/// com `ignore_markers`, a janela renovada), lê o arquivo a partir da raiz do
/// projeto e grava a marca do que foi lido.
pub fn collect(project_dir: &str, session_id: Option<&str>, ignore_markers: bool) -> Option<String> {
    let root = Path::new(project_dir);
    let config = ProjectConfig::load(root);
    let mut blocks: Vec<String> = Vec::new();
    let mut delivered: Vec<String> = Vec::new();

    for entry in config.injectables() {
        if entry.on != SESSION_START {
            continue;
        }
        let marker_name = marker_basename(&entry.file);
        if entry.once
            && !ignore_markers
            && marker_path(project_dir, session_id, &marker_name)
                .is_some_and(|marker| marker.is_file())
        {
            continue; // already delivered this session.
        }
        // Root-relative read. Still FAIL-OPEN — a hook never blocks on this —
        // but no longer SILENT: a declared injectable that cannot be read is
        // a router half that reaches nobody, and the operator saw a working
        // harness. Said on stderr, like every other notice a hook gives.
        let text = match fs::read_to_string(root.join(&entry.file)) {
            Ok(text) => text,
            Err(err) => {
                eprintln!(
                    "mustard: declared injectable `{}` (on {}) could not be read: {err} — \
                     that rule is NOT in force this session; run `/mustard:upsert` to reseed it",
                    entry.file, entry.on,
                );
                continue;
            }
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            eprintln!(
                "mustard: declared injectable `{}` (on {}) is empty — that rule is NOT in force",
                entry.file, entry.on,
            );
            continue;
        }
        blocks.push(trimmed.to_string());
        delivered.push(marker_name);
    }

    if blocks.is_empty() {
        return None;
    }
    // Record the delivery so `once` holds for the rest of the session. The
    // marker body carries the source path — a debugging breadcrumb, not data.
    for name in &delivered {
        write_marker(project_dir, session_id, name);
    }
    Some(blocks.join("\n\n"))
}


/// `injected-<basename>` for a declared file path (either separator accepted).
fn marker_basename(file: &str) -> String {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    format!("{MARKER_PREFIX}{base}")
}

/// `.claude/.session/<session_id>/` for a usable session id — the same base
/// the `active-spec` marker lives in (see `crate::shared::context`). `None`
/// for an empty/`"unknown"` id or a project root the nested `.claude` guard
/// rejects.
fn session_dir(project_dir: &str, session_id: Option<&str>) -> Option<PathBuf> {
    let sid = session_id?.trim();
    if sid.is_empty() || sid == "unknown" {
        return None;
    }
    Some(
        ClaudePaths::for_project(Path::new(project_dir))
            .ok()?
            .claude_dir()
            .join(".session")
            .join(sid),
    )
}

/// Full path of one delivery marker, when the session can hold markers.
fn marker_path(project_dir: &str, session_id: Option<&str>, marker_name: &str) -> Option<PathBuf> {
    Some(session_dir(project_dir, session_id)?.join(marker_name))
}

/// Persist one delivery marker, best-effort (an unwritable marker never blocks
/// the injection — it only means a `once` entry may deliver again).
fn write_marker(project_dir: &str, session_id: Option<&str>, marker_name: &str) {
    let Some(marker) = marker_path(project_dir, session_id, marker_name) else {
        return;
    };
    let Some(parent) = marker.parent() else {
        return;
    };
    let _ = fs::create_dir_all(parent);
    let _ = fs::write_atomic(&marker, marker_name.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Um projeto com a declaração `mustard.json#inject` e o arquivo.
    fn seed_project(dir: &Path, on: &str, file: &str, once: bool, body: &str) {
        let json = format!(
            r#"{{"inject":[{{"on":"{on}","file":"{file}","once":{once}}}]}}"#
        );
        std::fs::write(dir.join("mustard.json"), json).unwrap();
        let target = dir.join(file);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, body).unwrap();
    }

    /// O arquivo declarado sai uma vez por sessão, com a marca gravada; outra
    /// sessão o recebe de novo.
    #[test]
    fn collect_reads_declared_file_and_writes_marker() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_project(dir.path(), "sessionStart", ".claude/mustard/mapa.md", true, "MAPA\n");

        assert_eq!(collect(project, Some("s1"), false).as_deref(), Some("MAPA"));
        assert!(dir.path().join(".claude/.session/s1/injected-mapa.md").is_file(), "delivery marker recorded");
        assert_eq!(collect(project, Some("s1"), false), None, "once entry must not re-deliver in the session");
        assert_eq!(collect(project, Some("s2"), false).as_deref(), Some("MAPA"));
    }

    /// A entrada de outro gatilho não é lida aqui: a mensagem do usuário não
    /// entrega injetável nenhum. O arquivo que falta não sai e não deixa
    /// marca.
    #[test]
    fn collect_skips_missing_file_and_foreign_trigger() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_project(dir.path(), "userPromptSubmit", ".claude/mustard/orchestrator.md", true, "RULES");
        assert_eq!(collect(project, Some("s1"), false), None, "a prompt entry never rides the session start");

        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"inject":[{"on":"sessionStart","file":".claude/mustard/nope.md","once":true}]}"#,
        )
        .unwrap();
        assert_eq!(collect(project, Some("s1"), false), None);
        assert!(!dir.path().join(".claude/.session/s1/injected-nope.md").exists(), "no marker for an undelivered entry");
    }

    /// Sem id de sessão que sirva, a marca não se grava, e a entrada sai toda
    /// vez.
    #[test]
    fn once_without_session_id_degrades_to_every_time() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_project(dir.path(), "sessionStart", "rules.md", true, "X");
        assert!(collect(project, None, false).is_some());
        assert!(collect(project, Some("unknown"), false).is_some());
        assert!(collect(project, None, false).is_some());
    }

    /// A janela renovada recebe o texto de novo, mesmo com a marca.
    #[test]
    fn ignore_markers_redelivers_despite_marker() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        seed_project(dir.path(), "sessionStart", "style.md", true, "STYLE");
        assert!(collect(project, Some("s1"), false).is_some());
        assert_eq!(collect(project, Some("s1"), false), None);
        assert_eq!(collect(project, Some("s1"), true).as_deref(), Some("STYLE"));
    }

    /// O arquivo declarado que falta é pulado, e o que se lê chega assim
    /// mesmo.
    #[test]
    fn an_unreadable_injectable_is_skipped_and_the_rest_still_arrives() {
        let dir = tempdir().unwrap();
        let project = dir.path().to_str().unwrap();
        std::fs::write(
            dir.path().join("mustard.json"),
            r#"{"inject":[
                {"on":"sessionStart","file":".claude/mustard/gone.md","once":true},
                {"on":"sessionStart","file":".claude/mustard/mapa.md","once":true}
            ]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/mustard")).unwrap();
        std::fs::write(dir.path().join(".claude/mustard/mapa.md"), "MAPA").unwrap();

        assert_eq!(
            collect(project, Some("s1"), false).as_deref(),
            Some("MAPA"),
            "the missing entry must not take the readable one down with it",
        );
        assert!(!dir.path().join(".claude/.session/s1/injected-gone.md").exists());
    }
}
