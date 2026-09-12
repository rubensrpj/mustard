//! `mustard-rt run write <tipo> --spec <nome> --json '{…}'` — grava um evento
//! no arquivo de eventos da spec, a única porta de escrita dele.
//!
//! O binário põe a versão do formato, o número, o código do item, a hora com o
//! fuso e o campo de busca; o resto vem do `--json`, que não pode trazer o
//! código. Quem aponta um item (`replaces`, os alvos de `remove` e `purge`)
//! usa o número do evento ou o código que a página mostra. A saída diz o
//! número e o código gravados e, num `remove` ou num `purge`, os números
//! afetados:
//!
//! ```text
//! {"ok": true, "spec": "teste", "id": 41, "type": "remove", "code": "MSTD-RMV-0002", "removed": [12, 13]}
//! ```
//!
//! A página e o `.md` da spec são refeitos a cada gravação, ainda com a trava
//! do arquivo de eventos presa.
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
    // A página e o `.md` acompanham cada gravação e são refeitos antes de a
    // trava soltar, do que acabou de ser gravado: a gravação seguinte, de
    // outra sessão, só entra depois, e refaz os dois por último.
    let mut pages = None;
    let written = store::write_then(&path, &opts.event_type, draft, &roots, |log| {
        pages = Some(super::pages::rebuild(&project.root, &opts.spec, log, lang));
    });
    match written {
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
            // Se não deu para gravar a página e o `.md`, o evento já está no
            // arquivo: fica o aviso.
            if let Some(Err(refusal)) = &pages {
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

    /// Remover pelo código que a página mostra tira o item da leitura, da
    /// página e do `.md`, e ele continua no arquivo com o motivo. Um código
    /// que não existe é recusado citando o código, e nada é gravado.
    #[test]
    fn removing_by_the_code_takes_the_item_out_of_the_reading_the_page_and_the_md() {
        use crate::commands::spec_events::read::{read_at, ReadOpts};
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "message", r#"{"author":"user","text":"combine as regras"}"#);
        for text in ["Regra um.", "Regra dois.", "Regra três."] {
            let json = json!({"text": text, "keys": ["k"], "example": "e", "origin": 1}).to_string();
            assert_eq!(write(root, "rule", &json)["ok"], json!(true));
        }
        let removal = write(root, "remove", r#"{"targets":["MSTD-RULE-0002"],"reason":"Regra repetida."}"#);
        assert_eq!(removal["removed"], json!([3]), "{removal}");
        assert!(removal.get("warnings").is_none(), "{removal}");

        let agreed = read_at(&ReadOpts {
            root: root.to_path_buf(),
            spec: "teste".into(),
            block: "agreed".into(),
            term: None,
        })
        .unwrap();
        assert!(!agreed.contains("Regra dois.") && agreed.contains("Regra três."), "{agreed}");
        let spec = root.join(".claude").join("spec").join("teste");
        for page in ["spec.md", "spec.html"] {
            let shown = std::fs::read_to_string(spec.join(page)).unwrap();
            assert!(!shown.contains("Regra dois."), "{page}: {shown}");
            assert!(shown.contains("Regra um.") && shown.contains("Regra três."), "{page}");
        }
        let events = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert!(events.contains("Regra dois.") && events.contains("Regra repetida."), "{events}");

        let unknown = write(root, "remove", r#"{"targets":["MSTD-RULE-0009"],"reason":"r"}"#);
        assert_eq!(unknown["reason"], json!("unknown-target"), "{unknown}");
        assert!(unknown["hint"].as_str().unwrap().contains("MSTD-RULE-0009"), "{unknown}");
        let revised = write(root, "rule", r#"{"text":"Regra três, revista.","keys":["k"],"example":"e","origin":1,"replaces":"MSTD-RULE-0003"}"#);
        assert_eq!(revised["code"], json!("MSTD-RULE-0003"), "{revised}");
        let with_code = write(root, "note", r#"{"text":"t","keys":["k"],"origin":1,"code":"MSTD-NOTE-0001"}"#);
        assert_eq!(with_code["reason"], json!("binary-only-field"), "{with_code}");
        let after = std::fs::read_to_string(spec.join("spec.ndjson")).unwrap();
        assert_eq!(after.lines().count(), events.lines().count() + 1, "only the revision was written");
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
