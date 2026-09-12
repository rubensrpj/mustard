//! `mustard-rt run write <tipo> --spec <nome> --json '{…}'` — grava um evento
//! no arquivo de eventos da spec, a única porta de escrita dele.
//!
//! O binário põe a versão do formato, o número, a hora com o fuso e o campo de
//! busca; o resto vem do `--json`. A saída diz o número gravado e, num
//! `remove` ou num `purge`, os números afetados:
//!
//! ```text
//! {"ok": true, "spec": "teste", "id": 41, "type": "remove", "removed": [12, 13]}
//! ```
//!
//! Num worktree, o evento vai para o arquivo do checkout principal. As
//! citações de arquivo de um ponto são conferidas a partir de onde o comando
//! roda.

use std::path::PathBuf;

use mustard_core::domain::spec_events::Refusal;
use mustard_core::io::spec_events as store;
use serde_json::{json, Value};

/// Options for `mustard-rt run write`.
pub struct WriteOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    pub spec: String,
    pub event_type: String,
    /// Os campos do evento, num objeto JSON.
    pub json: String,
}

/// O núcleo testável de [`run`]: o relatório da gravação ou a recusa. Nunca
/// entra em pânico.
pub(crate) fn write_at(opts: &WriteOpts) -> Value {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let draft = match serde_json::from_str::<Value>(&opts.json) {
        Ok(Value::Object(map)) => map,
        Ok(other) => {
            let shown: String = other.to_string().chars().take(80).collect();
            return refuse(Refusal::NotAnObject { detail: shown });
        }
        Err(e) => return refuse(Refusal::NotAnObject { detail: e.to_string() }),
    };
    let path = match store::spec_file(&project.root, &opts.spec) {
        Ok(path) => path,
        Err(refusal) => return refuse(refusal),
    };
    let roots = store::citation_roots(&opts.root, &project.root);
    match store::write(&path, &opts.event_type, draft, &roots) {
        Ok(written) => {
            let mut report = json!({
                "ok": true,
                "spec": opts.spec.trim(),
                "id": written.id,
                "type": opts.event_type.trim(),
            });
            if let Some(code) = &written.code {
                report["code"] = json!(code);
            }
            if !written.removed.is_empty() {
                report["removed"] = json!(written.removed);
            }
            if !written.purged.is_empty() {
                report["purged"] = json!(written.purged);
            }
            // A página e o `.md` acompanham cada gravação. Se não der para
            // gravá-los, o evento já está no arquivo: fica o aviso.
            if let Err(refusal) = super::pages::refresh(&project.root, &opts.spec, lang) {
                report["warnings"] = json!([refusal.message(lang)]);
            }
            report
        }
        Err(refusal) => refuse(refusal),
    }
}

/// Run `write` and print the JSON report; exit 1 on a refusal.
pub fn run(opts: &WriteOpts) {
    let report = write_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(root: &std::path::Path, event_type: &str, json: &str) -> Value {
        write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: "teste".into(),
            event_type: event_type.into(),
            json: json.into(),
        })
    }

    #[test]
    fn a_write_reports_its_number_and_what_a_removal_took_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first = write(root, "message", r#"{"author":"user","text":"um"}"#);
        assert_eq!(
            first,
            json!({"ok": true, "spec": "teste", "id": 1, "type": "message", "code": "MSTD-MSG-0001"})
        );
        write(root, "message", r#"{"author":"user","text":"dois"}"#);
        let removal = write(root, "remove", r#"{"targets":[1,2],"reason":"engano"}"#);
        assert_eq!(removal["removed"], json!([1, 2]), "{removal}");
        assert!(root.join(".claude").join("spec").join("teste").join("spec.ndjson").is_file());
    }

    /// Cada gravação refaz a página e o `.md` da spec. Uma decisão revista
    /// mostra só a versão nova fora da conversa, onde a antiga aparece
    /// marcada como substituída; um item removido some dos dois e continua
    /// no arquivo de eventos, com o motivo.
    #[test]
    fn every_write_rebuilds_the_page_and_the_md() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"decida"}"#);
        write(root, "decision", r#"{"text":"Texto antigo.","keys":["k"],"why":"w","origin":1}"#);
        let revised =
            write(root, "decision", r#"{"text":"Texto novo.","keys":["k"],"why":"w","origin":1,"replaces":2}"#);
        assert_eq!(revised["code"], json!("MSTD-DEC-0001"), "the new version keeps the code");
        write(root, "note", r#"{"text":"Anotação que sai.","keys":["n"],"origin":1}"#);
        let removal = write(root, "remove", r#"{"targets":[4],"reason":"engano"}"#);
        assert!(removal.get("warnings").is_none(), "{removal}");

        let spec = root.join(".claude").join("spec").join("teste");
        let md = std::fs::read_to_string(spec.join("spec.md")).unwrap();
        let html = std::fs::read_to_string(spec.join("spec.html")).unwrap();
        let (html_before, html_talk) = html.split_once("<section id=\"conversation\">").unwrap();
        let (md_before, md_talk) = md.rsplit_once("\n## ").unwrap();
        for (before, talk) in [(html_before, html_talk), (md_before, md_talk)] {
            assert!(before.contains("Texto novo.") && !before.contains("Texto antigo."), "{before}");
            assert!(talk.contains("Texto antigo."), "{talk}");
            assert!(!before.contains("Anotação que sai.") && !talk.contains("Anotação que sai."));
        }
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert!(events.contains("Anotação que sai.") && events.contains("engano"), "{events}");
    }

    #[test]
    fn what_is_not_a_json_object_is_refused() {
        let dir = tempdir().unwrap();
        for json in ["[1,2]", "{quebrado", "\"texto\""] {
            let out = write(dir.path(), "note", json);
            assert_eq!(out["reason"], json!("not-an-object"), "{json}: {out}");
        }
        assert!(!dir.path().join(".claude").exists(), "a refusal writes nothing");
    }

    #[test]
    fn an_unknown_type_and_a_missing_field_are_refused_by_name() {
        let dir = tempdir().unwrap();
        let unknown = write(dir.path(), "lesson", r#"{"text":"x"}"#);
        assert_eq!(unknown["reason"], json!("unknown-type"));
        assert!(unknown["hint"].as_str().unwrap().contains("lesson"));
        let missing = write(dir.path(), "rule", r#"{"text":"t","keys":["k"],"origin":1}"#);
        assert_eq!(missing["reason"], json!("missing-field"));
        assert!(missing["hint"].as_str().unwrap().contains("example"));
    }
}
