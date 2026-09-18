//! `mustard-rt run index` — refaz o índice das specs
//! (`.claude/spec/index.ndjson` do checkout principal) a partir dos arquivos
//! de eventos e recalcula o campo `search` das linhas, quando o índice falta
//! ou diverge.
//!
//! Cada `write` já refaz a linha da spec dele; este é o conserto inteiro, o
//! que o `doctor` manda rodar quando acusa divergência. Refeito o índice, a
//! página do projeto, que sai só dele, é refeita também. A saída diz o índice e
//! a página, relativos ao projeto, quantas specs entraram, quantas linhas
//! tiveram o `search` recalculado e as pastas que ficaram fora (sem arquivo de
//! eventos):
//!
//! ```text
//! {"ok": true, "index": ".claude/spec/index.ndjson", "page": ".claude/spec/project.html", "specs": 3, "search_updated": 0, "lessons_search_updated": 0, "skipped": []}
//! ```
//!
//! A página que não pôde ser gravada não desfaz o índice: sai sem `page`, com
//! o motivo em `warnings`.
//!
//! Recusa só quando a trava ou a escrita falham (`io-failed`), com exit 1.

use std::path::PathBuf;

use mustard_core::io::spec_index;
use serde_json::{json, Value};

/// Options for `mustard-rt run index`.
pub struct IndexOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
}

/// O núcleo testável de [`run`]: o relatório do índice refeito ou a recusa.
/// Nunca entra em pânico.
pub(crate) fn index_at(opts: &IndexOpts) -> Value {
    let project = super::project(&opts.root);
    match spec_index::rebuild(&project.root) {
        Ok(rebuilt) => {
            let mut report = json!({
                "ok": true,
                "index": super::pages::relative(&project.root, &rebuilt.index),
                "specs": rebuilt.specs,
                "search_updated": rebuilt.search_updated,
                "lessons_search_updated": rebuilt.lessons_search_updated,
                "skipped": rebuilt.skipped,
            });
            match super::pages::refresh_project(&project.root, project.lang) {
                Ok((page, warnings)) => {
                    report["page"] = json!(page);
                    if !warnings.is_empty() {
                        report["warnings"] = json!(warnings);
                    }
                }
                Err(refusal) => report["warnings"] = json!([refusal.message(project.lang)]),
            }
            report
        }
        Err(refusal) => super::refused(&refusal, project.lang),
    }
}

/// Run `index` and print the JSON report; exit 1 on a refusal.
pub fn run(opts: &IndexOpts) {
    let report = index_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::io::spec_events::write_at;
    use serde_json::Map;
    use tempfile::tempdir;

    fn put(root: &std::path::Path, spec: &str, event_type: &str, draft: Value) {
        let path = root.join(".claude").join("spec").join(spec).join("spec.ndjson");
        let Value::Object(draft) = draft else { panic!("not an object") };
        write_at(&path, event_type, draft, &[], "2026-09-11T10:00:00-03:00").unwrap();
    }

    /// O relatório conta as specs, as linhas cujo `search` foi recalculado e
    /// as pastas do formato antigo que ficaram fora; a segunda rodada não
    /// tem o que recalcular.
    #[test]
    fn index_reports_the_specs_and_the_search_lines_it_fixed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "teste", "message", json!({"author": "user", "text": "combine"}));
        put(root, "teste", "note", json!({"text": "Apagar a pasta.", "keys": ["pasta"], "origin": 1}));
        put(root, "outra", "message", json!({"author": "user", "text": "outra"}));
        std::fs::create_dir_all(root.join(".claude").join("spec").join("velha").join(".events")).unwrap();
        let events = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let raw = std::fs::read_to_string(&events).unwrap();
        let note = raw.lines().nth(1).unwrap();
        let mut stale: Map<String, Value> = serde_json::from_str(note).unwrap();
        stale.insert("search".into(), json!("redutor antigo"));
        std::fs::write(&events, raw.replace(note, &Value::Object(stale).to_string())).unwrap();

        let report = index_at(&IndexOpts { root: root.to_path_buf() });
        assert_eq!(
            report,
            json!({
                "ok": true,
                "index": ".claude/spec/index.ndjson",
                "page": ".claude/spec/project.html",
                "specs": 2,
                "search_updated": 1,
                "lessons_search_updated": 0,
                "skipped": ["velha"],
            })
        );
        let page = std::fs::read_to_string(root.join(".claude/spec/project.html")).unwrap();
        assert!(page.contains("<code class=\"c\">outra</code>") && page.contains("<code class=\"c\">teste</code>"), "{page}");
        let again = index_at(&IndexOpts { root: root.to_path_buf() });
        assert_eq!(again["search_updated"], json!(0), "{again}");
    }
}
