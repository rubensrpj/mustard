//! Historical page readers and active secret warnings. Current pages are
//! local Mods projections or explicit native static publications. No workflow
//! milestone prepares a remote page or synchronizes an external database.

pub(crate) mod secrets;

use std::path::Path;

use mustard_core::ClaudePaths;
use mustard_core::platform::i18n::{Locale, translate};

use serde_json::{Value, json};

/// A pasta é de uma spec do formato antigo, cujo `spec.md` é o documento e não
/// a página refeita do arquivo de eventos. Dois sinais, e a diferença entre
/// eles importa:
///
/// - o `spec.md` com a seção de critérios de aceitação, de onde o fluxo antigo
///   lia os critérios, marca a spec como antiga sempre;
/// - o `meta.json` marca só quando não há `spec.ndjson`: uma spec aberta pelo
///   `open` pode ganhar um `meta.json` de uma porta antiga e continua nova.
///
/// Numa pasta assim o binário não grava nada: nem o evento, nem a página, que
/// refeita do arquivo de eventos apagaria o texto da spec. A única conferência
/// disso.
pub(crate) fn old_format_spec(root: &Path, spec: &str) -> bool {
    let Ok(paths) = ClaudePaths::for_project(root).and_then(|paths| paths.for_spec(spec.trim())) else {
        return false;
    };
    let criteria_section = std::fs::read_to_string(paths.spec_md_path()).ok().and_then(|md| crate::commands::review::qa_run::extract_ac_section(&md)).is_some();
    criteria_section || (paths.meta_json_path().is_file() && !paths.spec_ndjson_path().is_file())
}

/// A economia do rtk no projeto, só dos dias já fechados tanto na hora local
/// quanto na universal: o rtk pode contar o dia por qualquer uma delas.
/// Um aviso a mais na lista `warnings` da resposta de um passo, que nasce
/// quando falta.
pub(crate) fn push_warning(report: &mut Value, reason: &str, hint: &str) {
    let warning = json!({ "reason": reason, "hint": hint });
    match report.get_mut("warnings").and_then(Value::as_array_mut) {
        Some(list) => list.push(warning),
        None => report["warnings"] = json!([warning]),
    }
}

/// O item que guarda um trecho com cara de segredo, na resposta de um passo
/// da spec `spec`: o código de cada um em `withheld` e, em `warnings`, a
/// ordem de expurgá-lo. Sem item assim, a resposta não muda.
pub(crate) fn note_withheld(report: &mut Value, spec: &str, withheld: &[String], lang: Locale) {
    if withheld.is_empty() {
        return;
    }
    report["withheld"] = json!(withheld);
    push_warning(report, "page-check", &purge_pending(spec, withheld, lang));
}

/// A ordem de expurgar os itens `withheld` da spec `spec`.
fn purge_pending(spec: &str, withheld: &[String], lang: Locale) -> String {
    translate("page.purge_pending", lang).replace("{codes}", &withheld.join(", ")).replace("{spec}", spec.trim())
}

/// O caminho relativo ao projeto, com barras normais: a saída não traz o
/// caminho da máquina.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    /// Os dois sinais de uma spec do formato antigo: a pasta só com o
    /// `meta.json`, e o `spec.md` com a seção de critérios de aceitação, mesmo
    /// com arquivo de eventos ao lado. `run write` já recusa toda gravação
    /// numa spec assim
    /// (`run_write_refuses_the_binary_author_and_every_write_to_an_old_format_spec`,
    /// em `spec_events/write.rs`).
    #[test]
    fn the_two_signals_of_an_old_format_spec_are_recognized() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let text = "# Rascunho\n\nO texto da spec.\n";
        let with_criteria = "# Rascunho\n\n## Acceptance Criteria\n\n- [ ] AC-1: passa — Command: `cd .`\n";
        for (spec, md, with_events) in [("so-meta", text, false), ("com-criterios", with_criteria, true)] {
            let spec_dir = root.join(".claude").join("spec").join(spec);
            std::fs::create_dir_all(&spec_dir).unwrap();
            std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();
            std::fs::write(spec_dir.join("spec.md"), md).unwrap();
            if with_events {
                let plan = serde_json::json!({ "phase": "plan" });
                store::write(&spec_dir.join("spec.ndjson"), "state", plan.as_object().cloned().unwrap(), &[]).unwrap();
            }
            assert!(old_format_spec(root, spec), "{spec}");
        }
    }

    /// Uma spec com arquivo de eventos nunca é do formato antigo por um
    /// `meta.json` que apareceu ao lado.
    #[test]
    fn a_spec_with_an_event_file_is_never_old_format_even_with_a_meta_json() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let spec_dir = root.join(".claude").join("spec").join("nova");
        std::fs::create_dir_all(&spec_dir).unwrap();
        let survey = serde_json::json!({ "phase": "survey" });
        store::write(&spec_dir.join("spec.ndjson"), "state", survey.as_object().cloned().unwrap(), &[]).unwrap();
        std::fs::write(spec_dir.join("meta.json"), r#"{"scope":"light","stage":"Plan"}"#).unwrap();

        assert!(!old_format_spec(root, "nova"));
    }

    fn put(root: &Path, spec: &str, event_type: &str, draft: serde_json::Value) -> u64 {
        let path = root.join(".claude").join("spec").join(spec).join("spec.ndjson");
        store::write(&path, event_type, draft.as_object().cloned().unwrap(), &[]).unwrap().id
    }

    fn read(root: &Path, relative: &str) -> String {
        std::fs::read_to_string(root.join(relative)).unwrap()
    }

    /// A publicação da página do projeto, gravada pelo `run write` na spec em
    /// que o passo corre, leva o endereço para a linha do projeto do índice,
    /// de onde a barra de status o lê; o link da página da spec não muda.
    #[test]
    fn the_project_page_address_is_recorded_on_the_project_line() {
        use crate::commands::spec_events::write::{WriteOpts, write_at};

        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
        put(root, "busca", "state", serde_json::json!({"phase": "plan"}));
        let publish = |page: &str, url: &str| {
            write_at(&WriteOpts {
                root: root.to_path_buf(),
                spec: Some("busca".into()),
                event_type: "publish".into(),
                json: serde_json::json!({"page": page, "milestone": "approval", "ok": true, "url": url}).to_string(),
            })
        };
        let spec = publish("spec", "https://claude.ai/code/artifact/busca");
        assert_eq!(spec["ok"], serde_json::json!(true), "{spec}");
        let project = publish("project", "https://claude.ai/code/artifact/projeto");
        assert_eq!(project["ok"], serde_json::json!(true), "{project}");

        let index = read(root, ".claude/spec/index.ndjson");
        assert_eq!(mustard_core::domain::spec_index::project_url(&index).as_deref(), Some("https://claude.ai/code/artifact/projeto"), "{index}");
        let rows = mustard_core::io::spec_index::read_rows(root);
        assert_eq!(rows[0].url.as_deref(), Some("https://claude.ai/code/artifact/busca"), "{rows:?}");
    }

}
