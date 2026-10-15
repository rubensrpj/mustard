//! As frases das funções do mapa em lote: `notes-package` monta o pacote das
//! funções sem frase em dia que o condutor entrega ao agente de análise, e
//! `notes-write` grava as frases que o agente devolveu, uma por função. O
//! agente só lê; quem grava é o condutor, com a resposta dele.

use std::path::Path;

use mustard_core::domain::project_map::MapRefusal;
use mustard_core::io::map_notes::{self, NewNote};
use mustard_core::io::map_notes_package::{self as notes_package, PACKAGE_CHARS};
use mustard_core::io::project_map as store;
use mustard_core::platform::i18n::Locale;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{MapOpts, Reader, after_the_map, required};

/// O pacote das funções sem frase em dia, até `--limit` caracteres (por
/// padrão, o que deixa o pedido inteiro ao agente de análise no teto dele),
/// com as dos arquivos mudados desde o commit de `--changed-since` logo
/// depois das de nota velha. O commit que o git não conhece é recusado.
pub(super) fn package(opts: &MapOpts, root: &Path) -> Result<Value, MapRefusal> {
    let since = opts.changed_since.as_deref().map(str::trim).filter(|commit| !commit.is_empty());
    let changed = match since.map(|commit| changed_since(root, commit)).transpose() {
        Ok(changed) => changed.unwrap_or_default(),
        Err(detail) => {
            return Ok(json!({ "ok": false, "question": "notes-package", "reason": "unknown-commit", "commit": since, "detail": detail }));
        }
    };
    let limit = opts.limit.unwrap_or(PACKAGE_CHARS);
    let found = notes_package::package(root, &store::model_path(root), &changed, limit)?;
    Ok(json!({
        "ok": true,
        "question": "notes-package",
        "limit": limit,
        "chars": found.chars,
        "waiting": found.waiting,
        "declarations": found.entries,
        "too_large": found.too_large,
    }))
}

/// Os arquivos do projeto em `root` mudados desde `commit`, no commit seguinte
/// ou ainda não comitados, com o caminho a partir de `root`; o motivo, quando
/// o git não conhece o commit.
fn changed_since(root: &Path, commit: &str) -> Result<Vec<String>, String> {
    if commit.starts_with('-') {
        return Err(format!("{commit} is not a commit"));
    }
    let diff = mustard_core::platform::git::run(root, &["diff", "--name-only", "--relative", commit, "--"]);
    if !diff.ok {
        return Err(diff.stderr.trim().to_string());
    }
    Ok(diff.stdout.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect())
}

/// Uma frase devolvida pelo agente: o arquivo, a função (vazia na frase do
/// arquivo inteiro) e o texto.
#[derive(Deserialize)]
struct Sentence {
    file: String,
    #[serde(default)]
    name: String,
    text: String,
}

/// Grava as frases de `--json`, uma lista de `{"file","name","text"}`, na
/// spec atual da sessão, com as recusas da nota de uma frase só (arquivo
/// mudado depois do mapa, declaração que o arquivo não tem, texto vazio), e
/// responde as gravadas e as recusadas, cada uma com o motivo. A recusa de
/// uma não impede as outras; o texto que não é uma lista dessas é recusado
/// inteiro, sem gravar nada.
pub(super) fn write(opts: &MapOpts, root: &Path, lang: Locale, read: &Reader<'_>) -> Result<Value, MapRefusal> {
    let given = after_the_map(required(opts.json.as_deref(), opts.question, "--json"), read)?;
    let sentences: Vec<Sentence> = match serde_json::from_str(&given) {
        Ok(sentences) => sentences,
        Err(error) => return Ok(json!({ "ok": false, "question": "notes-write", "reason": "invalid-json", "detail": error.to_string() })),
    };
    let session = opts.session.as_deref().filter(|session| !session.is_empty());
    let spec = crate::shared::spec_state::active_spec(&root.to_string_lossy(), session).unwrap_or_default();
    let batch: Vec<NewNote<'_>> = sentences.iter().map(|s| NewNote { file: &s.file, name: &s.name, text: &s.text, spec: &spec }).collect();
    let (mut written, mut refused) = (Vec::new(), Vec::new());
    for (sentence, result) in sentences.iter().zip(map_notes::write_all(&store::model_path(root), root, &batch)) {
        match result {
            Ok(note) => written.push(json!({ "file": note.file, "name": note.name })),
            Err(refusal) => refused.push(json!({
                "file": sentence.file,
                "name": sentence.name,
                "reason": refusal.reason(),
                "hint": refusal.message(lang),
            })),
        }
    }
    Ok(json!({ "ok": true, "question": "notes-write", "spec": spec, "written": written, "refused": refused }))
}

#[cfg(test)]
mod tests {
    use super::super::{GrepSearch, MapOpts, Question, map_at};
    use mustard_core::domain::map_filter::FilterError;
    use mustard_core::domain::scan::ScanReport;
    use mustard_core::io::project_map as store;
    use serde_json::{Value, json};
    use std::path::Path;

    fn asked(root: &Path, question: Question) -> MapOpts {
        MapOpts {
            root: root.to_path_buf(),
            question,
            grep: None::<GrepSearch>,
            file: None,
            query: None,
            intent: None,
            described: None,
            said: None,
            name: None,
            pr: None,
            session: None,
            limit: None,
            changed_since: None,
            json: None,
        }
    }

    fn answered(opts: &MapOpts) -> Value {
        map_at(opts, &|_, _| Ok(ScanReport::default()), &|_, _, _, _| panic!("no history"), &|_, _| Err(FilterError::MissingKey))
    }

    fn names(report: &Value) -> Vec<&str> {
        report["declarations"].as_array().into_iter().flatten().filter_map(|entry| entry["name"].as_str()).collect()
    }

    /// Sem notas, o pacote vai por arquivo e linha; com `--changed-since`, a
    /// função do arquivo mudado desde o commit vem primeiro, com o texto que
    /// ela tem no disco. O commit que o git não conhece é recusado.
    #[test]
    fn notes_package_puts_the_functions_of_files_changed_since_the_commit_first() {
        let (_dir, root) = crate::shared::word_search::fixture::repo("{}");
        let plain = answered(&asked(&root, Question::NotesPackage));
        assert_eq!(plain["ok"], json!(true), "{plain}");
        assert_eq!(names(&plain), ["calcular_frete", "desconto_frete", "fechar_pedido"], "{plain}");
        assert_eq!(plain["waiting"], json!(3));

        let order = root.join("src/pedido.rs");
        let text = std::fs::read_to_string(&order).unwrap().replace("frete + 1", "frete + 2");
        std::fs::write(&order, &text).unwrap();
        let since = MapOpts { changed_since: Some("HEAD".into()), ..asked(&root, Question::NotesPackage) };
        let changed = answered(&since);
        assert_eq!(names(&changed), ["fechar_pedido", "calcular_frete", "desconto_frete"], "{changed}");
        assert_eq!(changed["declarations"][0]["text"], json!(text.trim_end()), "{changed}");
        assert!(changed["chars"].as_u64().unwrap() <= changed["limit"].as_u64().unwrap());

        let unknown = answered(&MapOpts { changed_since: Some("no-such-commit".into()), ..asked(&root, Question::NotesPackage) });
        assert_eq!((&unknown["ok"], &unknown["reason"]), (&json!(false), &json!("unknown-commit")), "{unknown}");
    }

    /// Um arquivo em TypeScript gravado no disco, e o módulo que o mapa
    /// guarda dele, com o SHA-256 do conteúdo e o de cada função.
    fn module(root: &Path, path: &str, names: &[&str]) -> Value {
        let content: String = names.iter().map(|name| format!("function {name}() {{\n  return 1;\n}}\n")).collect();
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        std::fs::write(root.join(path), &content).unwrap();
        let spans: Vec<(&str, usize, usize)> = names.iter().enumerate().map(|(at, name)| (*name, at * 3 + 1, at * 3 + 3)).collect();
        let declarations: Vec<Value> =
            spans.iter().map(|(name, line, end)| json!({"kind": "function", "name": name, "line": line, "end_line": end})).collect();
        let mut hash = mustard_core::io::sha256::Sha256::new();
        hash.update(content.as_bytes());
        let digests = mustard_core::io::map_notes::declaration_digests(&content, spans);
        json!({"path": path, "blob": path, "language": "typescript", "declarations": declarations,
               "analysis": {"content_sha256": hash.hex_digest(), "declaration_sha256": digests}})
    }

    /// `notes-write` grava as duas frases do arquivo em dia e recusa a do
    /// arquivo mudado depois do mapa, dizendo o motivo de cada recusa; o
    /// texto que não é a lista de frases é recusado inteiro.
    #[test]
    fn notes_write_answers_the_written_and_the_refused_sentences_with_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let map = json!({"modules": [module(root, "src/billing.ts", &["charge", "refund"]), module(root, "src/stock.ts", &["reserve"])]});
        store::write_text(root, &map.to_string()).unwrap();
        std::fs::write(root.join("src/stock.ts"), "function reserve() {\n  return 2;\n}\n").unwrap();
        let sentences = json!([
            {"file": "src/billing.ts", "name": "charge", "text": "Charges the customer."},
            {"file": "src/stock.ts", "name": "reserve", "text": "Reserves the stock of an order."},
            {"file": "src/billing.ts", "name": "refund", "text": "Gives the customer the money back."}
        ]);
        let report = answered(&MapOpts { json: Some(sentences.to_string()), ..asked(root, Question::NotesWrite) });
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(
            report["written"],
            json!([{"file": "src/billing.ts", "name": "charge"}, {"file": "src/billing.ts", "name": "refund"}]),
            "{report}"
        );
        let refused = &report["refused"];
        assert_eq!((&refused[0]["file"], &refused[0]["name"]), (&json!("src/stock.ts"), &json!("reserve")), "{report}");
        assert!(refused[0]["hint"].as_str().unwrap().contains("note-source-changed"), "{report}");
        assert_eq!(refused.as_array().map(Vec::len), Some(1));
        let model = store::model_path(root);
        assert!(!mustard_core::io::map_notes::of_at(&model, "src/billing.ts", "refund").unwrap().unwrap().stale);

        let broken = answered(&MapOpts { json: Some("{\"file\": 1}".into()), ..asked(root, Question::NotesWrite) });
        assert_eq!((&broken["ok"], &broken["reason"]), (&json!(false), &json!("invalid-json")), "{broken}");
    }
}
