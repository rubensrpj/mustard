//! `mustard-rt run move-records` — tira de uma spec antiga os registros que
//! hoje nascem no arquivo de operações (o que os ganchos fizeram, as
//! injeções, as chamadas e o tempo das etapas da rodada) e os põe lá. Só a
//! pedido: sem `--apply`, diz quantos eventos e bytes sairiam e não muda
//! nada; com ele, a spec vai inteira antes para uma cópia de segurança em
//! `.claude/mustard/backups/`.
//!
//! Quem lê o que o programa observou já junta as duas fontes
//! ([`super::conversation::observed`]), então a leitura depois da mudança dá
//! o mesmo resultado; os registros movidos ganham, no arquivo de operações, o
//! número e o código seguintes de lá e guardam a hora deles.
//!
//! ```text
//! {"ok": true, "spec": "x", "applied": false, "events": 812, "bytes": 640000, "spec_bytes": 900000, "by_type": {"call": 700, "hook": 112}, "hint": "…"}
//! ```
//!
//! Recusa a spec sem arquivo de eventos (`no-spec-file`), o nome inválido
//! (`bad-spec-name`) e a falha de disco (`io-failed`), com exit 1.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::Refusal;
use mustard_core::io::spec_events::{self as store, records};
use mustard_core::platform::i18n::translate;
use serde_json::{Value, json};

/// Options for `mustard-rt run move-records`.
pub struct MoveRecordsOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec de que os registros saem.
    pub spec: String,
    /// Faz a mudança; sem ele, só mostra o que faria.
    pub apply: bool,
}

/// O caminho livre da cópia de segurança da spec `spec`:
/// `.claude/mustard/backups/<spec>.spec.ndjson`, com `-2`, `-3`… quando o
/// anterior já existe. Nenhuma cópia é sobrescrita.
fn free_backup(root: &Path, spec: &str) -> PathBuf {
    let folder = root.join(".claude/mustard/backups");
    let mut path = folder.join(format!("{spec}.spec.ndjson"));
    let mut n = 2;
    while path.exists() {
        path = folder.join(format!("{spec}-{n}.spec.ndjson"));
        n += 1;
    }
    path
}

/// Os KB de `bytes`, arredondados para cima.
fn kb(bytes: u64) -> String {
    bytes.div_ceil(1024).to_string()
}

/// O núcleo testável de [`run`]: o relatório da mudança, ou do que ela faria,
/// ou a recusa. Nunca entra em pânico.
pub(crate) fn move_records_at(opts: &MoveRecordsOpts) -> Value {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let spec = opts.spec.trim();
    let path = match store::spec_file(&project.root, spec) {
        Ok(path) => path,
        Err(refusal) => return super::refused(&refusal, lang),
    };
    let Some(operations) = store::operations_file(&project.root, spec) else {
        return super::refused(&Refusal::BadSpecName { spec: spec.to_string() }, lang);
    };
    let backup = opts.apply.then(|| free_backup(&project.root, spec));
    let moved = match records::move_records(&path, &operations, backup.as_deref()) {
        Ok(Some(moved)) => moved,
        Ok(None) => return super::refused(&Refusal::NoSpecFile { spec: spec.to_string() }, lang),
        Err(refusal) => return super::refused(&refusal, lang),
    };
    let fill = |key: &str| translate(key, lang).replace("{events}", &moved.events.to_string()).replace("{kb}", &kb(moved.bytes));
    let mut out = json!({
        "ok": true,
        "spec": spec,
        "applied": moved.applied,
        "events": moved.events,
        "bytes": moved.bytes,
        "spec_bytes": moved.spec_bytes,
        "by_type": moved.by_type,
    });
    let hint = match backup.as_deref().filter(|_| moved.applied) {
        _ if moved.events == 0 => translate("spec_events.move_records_none", lang).to_string(),
        Some(backup) => {
            let shown = super::pages::relative(&project.root, backup);
            out["backup"] = json!(shown);
            fill("spec_events.move_records_done").replace("{backup}", &shown)
        }
        None => fill("spec_events.move_records_preview"),
    };
    out["hint"] = json!(hint);
    out
}

/// Run `move-records` and print the JSON report; exit 1 on a refusal.
pub fn run(opts: &MoveRecordsOpts) {
    let report = move_records_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::conversation::observed;
    use mustard_core::domain::spec_events::SpecEvent;

    /// O que o programa observou, como os leitores o veem: o tipo, a hora e
    /// os campos, sem o número e o código, que o arquivo de operações dá de
    /// novo.
    fn seen(root: &Path) -> Vec<(String, String, Value)> {
        let log = store::read(&store::spec_file(root, "antiga").unwrap()).unwrap().unwrap();
        let shown = |event: &SpecEvent| {
            let mut fields = event.fields.clone();
            for key in ["id", "code", "search"] {
                fields.remove(key);
            }
            (event.event_type.clone(), event.at().to_string(), Value::Object(fields))
        };
        observed(root, "antiga", &log).iter().map(shown).collect()
    }

    /// Uma spec antiga com os registros dentro e um arquivo de operações que
    /// já começou: depois da mudança, a spec perde os registros, e o que o
    /// programa observou, lido pelas duas fontes, é o mesmo de antes.
    #[test]
    fn the_observed_reading_is_the_same_after_the_records_move() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let spec = store::spec_file(root, "antiga").unwrap();
        std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
        let lines = [
            json!({"v":1,"id":1,"code":"MSTD-MSG-0001","at":"2026-09-01T10:00:00-03:00","type":"message","author":"user","text":"arrume"}),
            json!({"v":1,"id":2,"code":"MSTD-HOOK-0001","at":"2026-09-01T10:01:00-03:00","type":"hook","author":"hook","hook":"command_guard","action":"block","tool":"Bash","reason":"apaga"}),
            json!({"v":1,"id":3,"code":"MSTD-INJ-0001","at":"2026-09-01T10:02:00-03:00","type":"injection","author":"hook","hook":"prompt_entry","chars":3,"text":"uma"}),
            json!({"v":1,"id":4,"code":"MSTD-CALL-0001","at":"2026-09-01T10:03:00-03:00","type":"call","author":"binary","command":"round","ms":3,"result":"refused","refusal":"not-approved"}),
        ];
        std::fs::write(&spec, lines.iter().map(|line| line.to_string() + "\n").collect::<String>()).unwrap();
        let operations = store::operations_file(root, "antiga").unwrap();
        std::fs::create_dir_all(operations.parent().unwrap()).unwrap();
        let newer = json!({"v":1,"id":1,"code":"MSTD-CALL-0001","at":"2026-10-01T10:00:00-03:00","type":"call","author":"binary","command":"plan","ms":1,"result":"ok"});
        std::fs::write(&operations, format!("{newer}\n")).unwrap();
        let before = seen(root);
        assert_eq!(before.len(), 4);

        let opts = |apply| MoveRecordsOpts { root: root.to_path_buf(), spec: "antiga".into(), apply };
        let shown = move_records_at(&opts(false));
        assert_eq!((shown["applied"].clone(), shown["events"].clone()), (json!(false), json!(3)), "{shown}");
        let applied = move_records_at(&opts(true));
        assert_eq!(applied["backup"], json!(".claude/mustard/backups/antiga.spec.ndjson"), "{applied}");
        assert_eq!(store::read(&spec).unwrap().unwrap().events.len(), 1, "only the message stays");
        assert_eq!(seen(root), before);
    }
}
