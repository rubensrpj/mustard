//! `map_notes` — a nota de sentido de cada arquivo e de cada função, escrita
//! pelo agente que leu o trecho.
//!
//! O código diz o que faz; a nota diz para que serve, nas palavras de quem
//! pede: "valida CPF e CNPJ na entrada do cadastro". A busca por palavras e a
//! por sentido só conhecem o que o código escreve, então quem pergunta em
//! palavras de negócio, sem citar nome de função, não chega ao arquivo certo.
//! A nota é a ponte: uma frase curta, guardada uma vez, que a busca lê junto
//! do resto.
//!
//! O bloco `notes` do mapa ([`crate::io::project_map::NOTES`]) guarda, de cada
//! nota, o arquivo, o nome da declaração (vazio na nota do arquivo inteiro), o
//! texto, a spec que a escreveu, o blob e o SHA-256 do arquivo no mapa no
//! momento da escrita e, na nota de uma declaração, o SHA-256 do texto dela
//! ([`declaration_digests`]). Ninguém refaz a nota a partir do código: quem a
//! escreve é o agente, e a busca nunca chama modelo nenhum para escrevê-la.
//!
//! **A nota envelhece com o que ela descreve.** A do arquivo inteiro vale
//! enquanto o SHA-256 do arquivo no mapa for o do conteúdo que ela guardou; a
//! de uma declaração vale também enquanto o texto da própria declaração for o
//! que ela guardou, mesmo com outra linha do arquivo mudada. O arquivo que
//! saiu do mapa, a declaração que sumiu ou mudou e o arquivo inteiro mudado na
//! nota dele deixam a nota velha: ela fica gravada e visível para quem lê o
//! trecho, que a reescreve, mas não entra mais na busca, porque uma nota que
//! já não descreve o código acharia o arquivo pelo motivo errado. Fora do git
//! o mapa não tem blob; o SHA-256 do conteúdo mantém a mesma invalidação.
//!
//! Onde a nota entra na busca — no índice de palavras e no texto compilado de
//! cada declaração, de onde saem os vetores de sentido — quem lê é
//! [`crate::io::map_notes_fresh`], que só conhece o banco. Este módulo grava a
//! nota e, por isso, refaz o índice e os vetores: fica em cima da busca e do
//! sentido, e nenhum dos dois o usa.
//!
//! Mapa sem nenhuma nota, ou sem a tabela, responde como sempre.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{OptionalExtension,params};
use serde_json::{Map, Value};

use crate::domain::project_map::{MapRefusal,clean_path};
use crate::io::map_db::table_exists;
use crate::io::map_notes_fresh::FRESH;
use crate::io::map_search::refresh_files;
use crate::io::project_map::{open_existing, unreadable};
use crate::io::{map_meaning, map_revision};

/// A nota que se grava: o arquivo, a declaração (vazia na nota do arquivo
/// inteiro), o texto e a spec que a escreve.
#[derive(Debug, Clone, Copy)]
pub struct NewNote<'a> {
    pub file: &'a str,
    pub name: &'a str,
    pub text: &'a str,
    pub spec: &'a str,
}

/// O SHA-256 do texto de cada nome declarado em `content`, como o scan o
/// guarda no texto analisado do arquivo (`declaration_sha256`) e a nota de
/// uma declaração o confere: as linhas `line..=end_line` de toda declaração
/// com aquele nome, na ordem das linhas. O fim desconhecido (`0`) vale a
/// linha do começo, e a linha além do arquivo não entra.
#[must_use]
pub fn declaration_digests<'a>(content: &str, declarations: impl IntoIterator<Item = (&'a str, usize, usize)>) -> Map<String, Value> {
    let lines: Vec<&str> = content.lines().collect();
    let mut spans: BTreeMap<&str, Vec<(usize, usize)>> = BTreeMap::new();
    for (name, line, end_line) in declarations {
        spans.entry(name).or_default().push((line, end_line.max(line)));
    }
    spans
        .into_iter()
        .map(|(name, mut spans)| {
            spans.sort_unstable();
            let mut hash = crate::io::sha256::Sha256::new();
            for (line, end_line) in spans {
                let text = lines.get(line.saturating_sub(1)..end_line.min(lines.len())).unwrap_or_default();
                hash.update(text.join("\n").as_bytes());
                hash.update(b"\0");
            }
            (name.to_string(), Value::String(hash.hex_digest()))
        })
        .collect()
}

/// Uma nota como o mapa a guarda, com o dizer se ela já envelheceu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub file: String,
    pub name: String,
    pub text: String,
    pub spec: String,
    /// A nota não está em dia pela regra que a busca lê
    /// (`io::map_notes_fresh`): o que ela descreve — o arquivo inteiro ou o
    /// texto da declaração — mudou, o arquivo saiu do mapa ou perdeu a
    /// declaração depois que ela foi escrita.
    pub stale: bool,
}

/// Grava a nota `new` no mapa em `model`, de projeto em `root`, no lugar da
/// que o mesmo arquivo e a mesma declaração já tinham, e devolve o que ficou
/// gravado. O blob e o SHA-256 que ela guarda são os do arquivo no mapa e, na
/// nota de uma declaração, também o SHA-256 do texto dela que o scan leu; a
/// nota entra no índice de busca na mesma transação, só nos documentos do
/// arquivo, e os vetores de sentido se refazem em seguida.
///
/// Recusa o texto vazio, o arquivo que o mapa não tem, o arquivo cujo texto
/// analisado não traz o SHA-256 do conteúdo — também quando ele não é JSON —
/// e a declaração que o arquivo não tem.
///
/// # Errors
///
/// Sem o mapa, com ele ilegível ou quando a gravação falha.
pub fn write(model: &Path, root: &Path, new: &NewNote<'_>) -> std::result::Result<Note, MapRefusal> {
    let file = clean_path(new.file);
    let name = new.name.trim().to_string();
    let text = new.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(MapRefusal::MissingArgument { question: "note".to_string(), flag: "text".to_string() });
    }
    let mut db = open_existing(model)?;
    let blob = db
        .conn()
        .query_row("SELECT blob FROM files WHERE path = ?1", [&file], |row| row.get::<_, Option<String>>(0))
        .optional()
        .map_err(|err| unreadable(err.into()))?;
    let Some(blob) = blob else {
        return Err(MapRefusal::UnknownFile { file });
    };
    // O texto analisado que não é JSON não derruba a leitura: ele fica sem
    // SHA-256, e a nota é recusada como a do arquivo que o scan não leu.
    let indexed: Option<(Option<String>, Option<String>)> = db
        .conn()
        .query_row(
            "SELECT CASE WHEN json_valid(t.analysis) THEN json_extract(t.analysis, '$.content_sha256') END, \
             (SELECT d.value FROM json_each(CASE WHEN json_valid(t.analysis) THEN t.analysis ELSE '{}' END, '$.declaration_sha256') d \
              WHERE d.key = ?2) FROM texts t WHERE t.path = ?1",
            [&file, &name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|err| unreadable(err.into()))?;
    let (indexed, declared_sha256) = indexed.unwrap_or_default();
    let sha256 = indexed.filter(|sha| sha.len() == 64).ok_or_else(|| MapRefusal::MapUnreadable { detail: "note-needs-current-scan".into() })?;
    let declared_sha256 = declared_sha256.filter(|sha| !name.is_empty() && sha.len() == 64);
    let source = crate::domain::knowledge::Source { file: file.clone(), line: 1, end_line: 1, sha256: sha256.clone() };
    if !crate::io::knowledge::current(root, &source, &mut Default::default()) {
        return Err(MapRefusal::MapUnreadable { detail: "note-source-changed; scan before recording".into() });
    }
    if !name.is_empty() {
        let declared = db
            .conn()
            .query_row("SELECT 1 FROM decls WHERE file = ?1 AND name = ?2 LIMIT 1", [&file, &name], |_| Ok(()))
            .optional()
            .map_err(|err| unreadable(err.into()))?;
        if declared.is_none() {
            return Err(MapRefusal::UnknownDeclaration { file: Some(file), name });
        }
    }
    let note = Note { file, name, text, spec: new.spec.trim().to_string(), stale: false };
    db.write(|tx| {
        tx.execute("DELETE FROM notes WHERE file = ?1 AND name = ?2", params![note.file, note.name])?;
        tx.execute(
            "INSERT INTO notes(file, name, text, spec, blob, sha256, declaration_sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![note.file, note.name, note.text, note.spec, blob, sha256, declared_sha256],
        )?;
        refresh_files(tx, &[note.file.as_str()])?;
        map_revision::bump(tx)?;
        Ok(())
    })
    .map_err(unreadable)?;
    drop(db);
    // Os vetores de sentido se refazem só das declarações cujo texto mudou; se
    // não se refazem agora, o scan seguinte os refaz.
    let _ = map_meaning::fill_at(model, root);
    Ok(note)
}

/// A nota do arquivo `file` — a da declaração `name`, ou a do arquivo inteiro
/// com o nome vazio — no mapa em `model`, com o dizer se ela envelheceu; `None`
/// quando ela não existe, e também quando o mapa é de antes das notas.
///
/// # Errors
///
/// Sem o mapa ou com ele ilegível.
pub fn of_at(model: &Path, file: &str, name: &str) -> std::result::Result<Option<Note>, MapRefusal> {
    let db = open_existing(model)?;
    let file = clean_path(file);
    if !table_exists(db.conn(), "notes").map_err(unreadable)? {
        return Ok(None);
    }
    let found = db
        .conn()
        .query_row(
            &format!("SELECT n.text, n.spec, NOT ({FRESH}) FROM notes n WHERE n.file = ?1 AND n.name = ?2"),
            [&file, name.trim()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?)),
        )
        .optional()
        .map_err(|err| unreadable(err.into()))?;
    Ok(found.map(|(text, spec, stale)| Note { file, name: name.trim().to_string(), text, spec, stale }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::normalize::Languages;
    use crate::domain::search::TOP;
    use crate::io::map_db::MapDb;
    use crate::io::map_lists::ranked_files_near;
    use crate::io::map_meaning::{fill_at, ranked_declarations};
    use crate::io::map_notes_fresh::fresh;
    use crate::io::map_search::{any_path, candidates_at, forget, indexed};
    use crate::io::map_sense::Near;
    use crate::io::map_triage::triage_at;
    use crate::io::project_map::{SEARCHED,model_path, save_at};
    use serde_json::{Value,json};
    use tempfile::TempDir;

    /// O pedido, em palavras de negócio, que nenhum nome do código escreve.
    const ASK: &str = "validar CPF e CNPJ no cadastro do cliente";

    /// A nota que diz para que serve o arquivo, com as palavras do pedido.
    const NOTE: &str = "Valida CPF e CNPJ na entrada do cadastro do cliente.";

    fn languages() -> Languages {
        Languages::new(["pt-BR", "en-US"])
    }

    fn function(name: &str, line: u64, doc: &str) -> Value {
        json!({"kind": "function", "name": name, "line": line, "end_line": line + 5,
               "signature": format!("fn {name}()"), "doc": doc})
    }

    /// O mapa do projeto: `src/ids.rs` com o blob `ids_blob`, que rejeita
    /// número malformado sem dizer nada de CPF, de CNPJ nem de cadastro, e
    /// mais dois arquivos de outro assunto.
    fn modules(ids_blob: &str) -> Value {
        let mut raw = json!({ "modules": [
            {"path": "src/ids.rs", "blob": ids_blob, "declarations": [
                function("check_digits", 1, "Rejects a malformed number."),
                function("strip_marks", 10, "Removes dots and dashes.")]},
            {"path": "src/invoice.rs", "blob": "invoice-1", "declarations": [
                function("charge_invoice", 1, "Charge the customer invoice with the tax.")]},
            {"path": "src/config.rs", "blob": "config-1", "declarations": [
                function("parse_config", 1, "Parse the configuration file.")]}
        ]});
        for module in raw["modules"].as_array_mut().unwrap() {
            module["analysis"] = analysis_of(&code(module["blob"].as_str().unwrap()), &module["declarations"]);
        }
        raw
    }
    fn code(blob: &str) -> String { format!("// {blob}\nfn sample() {{}}\n") }
    /// O texto analisado que o scan grava para o conteúdo `content` com estas
    /// declarações: o SHA-256 dele e o do texto de cada declaração.
    fn analysis_of(content: &str, declarations: &Value) -> Value {
        let spans = declarations.as_array().into_iter().flatten().map(|d| {
            let line = |key: &str| usize::try_from(d[key].as_u64().unwrap_or(0)).unwrap();
            (d["name"].as_str().unwrap_or_default(), line("line"), line("end_line"))
        });
        json!({"content_sha256": digest(content.as_bytes()), "declaration_sha256": declaration_digests(content, spans)})
    }
    fn digest(bytes: &[u8]) -> String { let mut hash=crate::io::sha256::Sha256::new(); hash.update(bytes); hash.hex_digest() }
    fn source_files(root: &Path, raw: &Value) {
        for module in raw["modules"].as_array().unwrap() {
            let path=root.join(module["path"].as_str().unwrap());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, code(module["blob"].as_str().unwrap_or("ids-1"))).unwrap();
        }
    }

    /// Um projeto gravado como o scan grava, com o arquivo de ids no blob dado.
    fn saved(ids_blob: &str) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mustard.json"), r#"{"ai":{"vectors":true}}"#).unwrap();
        source_files(dir.path(), &modules(ids_blob));
        save_at(&model_path(dir.path()), &modules(ids_blob), "scan 1", &languages()).unwrap();
        dir
    }

    /// O scan que grava de novo o mesmo projeto, agora com o arquivo de ids no
    /// blob dado.
    fn rescanned(dir: &TempDir, ids_blob: &str) {
        source_files(dir.path(), &modules(ids_blob));
        save_at(&model_path(dir.path()), &modules(ids_blob), "scan 2", &languages()).unwrap();
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
    }

    fn note_of(file: &str, name: &str, text: &str) -> NewNote<'static> {
        NewNote { file: Box::leak(file.into()), name: Box::leak(name.into()), text: Box::leak(text.into()), spec: "cadastro" }
    }

    fn write_note(dir: &TempDir, file: &str, name: &str, text: &str) -> Note {
        write(&model_path(dir.path()), dir.path(), &note_of(file, name, text)).unwrap()
    }

    /// Os caminhos da busca de arquivos por palavras, na ordem da nota.
    fn searched(dir: &TempDir, query: &str) -> Vec<String> {
        let db = indexed(&model_path(dir.path()), &languages(), &SEARCHED).unwrap();
        ranked_files_near(db.conn(), query, &languages(), TOP, &Near::none())
            .unwrap()
            .into_iter()
            .map(|found| found.path)
            .collect()
    }

    /// Os caminhos da triagem, a busca que o comando e o gancho respondem.
    fn triaged(dir: &TempDir, query: &str) -> Vec<String> {
        let triage = triage_at(&model_path(dir.path()), (query, ""), &languages(), TOP).unwrap();
        triage.files.into_iter().map(|found| found.path).collect()
    }

    fn opened(dir: &TempDir) -> MapDb {
        MapDb::open(&model_path(dir.path()), dir.path(), &[]).unwrap()
    }

    fn count(dir: &TempDir, sql: &str) -> i64 {
        opened(dir).conn().query_row(sql, [], |row| row.get(0)).unwrap()
    }

    /// A nota gravada para um arquivo o faz achado pelo pedido em palavras de
    /// negócio, sem citar o nome de função nenhuma: a busca por palavras e a
    /// triagem não o achavam, e o acham depois da nota.
    #[test]
    fn a_note_written_for_a_file_is_found_by_business_words_that_name_no_function() {
        let dir = saved("ids-1");
        assert!(!searched(&dir, ASK).contains(&"src/ids.rs".to_string()), "the code says nothing of it");
        assert!(!triaged(&dir, ASK).contains(&"src/ids.rs".to_string()), "the triage neither");
        write_note(&dir, "src/ids.rs", "", NOTE);
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
        assert_eq!(triaged(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
        assert!(!searched(&dir, "charge the invoice tax").contains(&"src/ids.rs".to_string()), "other subjects stay apart");
    }

    /// A nota de uma função a põe na frente dos candidatos; sem a nota, ela não
    /// era a primeira.
    #[test]
    fn a_note_written_for_a_function_puts_that_function_first_among_the_candidates() {
        let dir = saved("ids-1");
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
        let names = |dir: &TempDir| -> Vec<String> {
            let found = candidates_at(&model_path(dir.path()), ASK, "", &languages(), any_path).unwrap();
            found.candidates.into_iter().map(|candidate| candidate.name).collect()
        };
        let before = names(&dir);
        assert!(!before.is_empty());
        assert_ne!(before.first().map(String::as_str), Some("strip_marks"));
        write_note(&dir, "src/ids.rs", "strip_marks", NOTE);
        let lifted = names(&dir);
        assert_eq!(lifted.first().map(String::as_str), Some("strip_marks"), "{lifted:?}");
    }

    /// O arquivo mudado marca a nota como velha e a tira da busca: as respostas
    /// voltam a ser as de antes da nota. A nota continua gravada, e a mesma
    /// frase escrita de novo vale outra vez.
    #[test]
    fn a_changed_file_marks_its_note_stale_and_takes_it_out_of_the_search() {
        let dir = saved("ids-1");
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
        let before = (searched(&dir, ASK), triaged(&dir, ASK));
        write_note(&dir, "src/ids.rs", "", NOTE);
        let model = model_path(dir.path());
        assert!(!of_at(&model, "src/ids.rs", "").unwrap().unwrap().stale);
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
        rescanned(&dir, "ids-1");
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"), "the same blob keeps the note");
        assert_eq!(triaged(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));

        rescanned(&dir, "ids-2");
        let stale = of_at(&model, "src/ids.rs", "").unwrap().unwrap();
        assert!(stale.stale, "{stale:?}");
        assert_eq!(stale.text, NOTE, "the stale note stays written for whoever reads the piece");
        assert_eq!((searched(&dir, ASK), triaged(&dir, ASK)), before, "a stale note finds nothing");

        write_note(&dir, "src/ids.rs", "", NOTE);
        assert!(!of_at(&model, "src/ids.rs", "").unwrap().unwrap().stale, "rewritten against the new blob");
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
        assert_eq!(triaged(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
    }

    #[test]
    fn legacy_notes_survive_migration_but_are_not_restamped_as_current() {
        let dir=saved("ids-1");let model=model_path(dir.path());
        write(&model,dir.path(),&note_of("src/ids.rs","",NOTE)).unwrap();
        let db=crate::io::project_map::open_existing(&model).unwrap();
        db.conn().execute_batch("ALTER TABLE notes DROP COLUMN sha256; UPDATE blocks SET version=1 WHERE name='notes';").unwrap();
        drop(db);
        let old=of_at(&model,"src/ids.rs","").unwrap().unwrap();
        assert!(old.stale,"an old note has no evidence of having read the current bytes");
        assert_eq!(old.text,NOTE,"migration preserves its content for inspection");
        write(&model,dir.path(),&note_of("src/ids.rs","",NOTE)).unwrap();
        assert!(!of_at(&model,"src/ids.rs","").unwrap().unwrap().stale);
    }

    /// Without git, actual content hashes retain and invalidate notes equally.
    #[test]
    fn a_map_without_blobs_keeps_its_note_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let mut bare = modules("ids-1");
        source_files(dir.path(), &bare);
        for module in bare["modules"].as_array_mut().unwrap() { module.as_object_mut().unwrap().remove("blob"); }
        let model = model_path(dir.path());
        save_at(&model, &bare, "scan 1", &languages()).unwrap();
        write(&model, dir.path(), &note_of("src/ids.rs", "", NOTE)).unwrap();
        assert!(!of_at(&model, "src/ids.rs", "").unwrap().unwrap().stale);
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
        save_at(&model, &bare, "scan 2", &languages()).unwrap();
        assert!(!of_at(&model, "src/ids.rs", "").unwrap().unwrap().stale);
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
        std::fs::write(dir.path().join("src/ids.rs"), "fn changed() {}\n").unwrap();
        assert!(write(&model, dir.path(), &note_of("src/ids.rs", "", NOTE)).is_err());
        bare["modules"][0]["analysis"]["content_sha256"] = json!(digest(b"fn changed() {}\n"));
        save_at(&model, &bare, "scan 3", &languages()).unwrap();
        assert!(of_at(&model, "src/ids.rs", "").unwrap().unwrap().stale);
        assert!(searched(&dir, ASK).is_empty());
    }

    /// A busca, o dizer de nota velha e a auditoria do mapa dão as mesmas
    /// notas em dia: as do arquivo da fatura, cujo conteúdo não mudou. A da
    /// função do arquivo que mudou e a do arquivo cujo texto analisado não é
    /// JSON ficam velhas nas três, e nenhuma delas cai por causa do texto
    /// estragado.
    #[test]
    fn the_search_the_stale_flag_and_the_audit_read_the_same_fresh_notes() {
        let dir = saved("ids-1");
        let notes = [("src/invoice.rs", ""), ("src/invoice.rs", "charge_invoice"), ("src/ids.rs", "check_digits"), ("src/config.rs", "parse_config")];
        for (file, name) in notes {
            write_note(&dir, file, name, NOTE);
        }
        rescanned(&dir, "ids-2");
        opened(&dir).conn().execute("UPDATE texts SET analysis = 'not-json' WHERE path = 'src/config.rs'", []).unwrap();
        let in_search: Vec<(String, String)> =
            fresh(opened(&dir).conn(), None).unwrap().into_iter().map(|note| (note.file, note.name)).collect();
        let model = model_path(dir.path());
        let not_stale: Vec<(String, String)> = notes
            .iter()
            .filter(|(file, name)| !of_at(&model, file, name).unwrap().unwrap().stale)
            .map(|(file, name)| ((*file).to_string(), (*name).to_string()))
            .collect();
        let expected = vec![("src/invoice.rs".to_string(), String::new()), ("src/invoice.rs".to_string(), "charge_invoice".to_string())];
        assert_eq!((&in_search, &not_stale), (&expected, &expected));
        let audit = crate::io::knowledge::audit::run(dir.path()).unwrap();
        let counts = &audit["counts"];
        assert_eq!(
            (&counts["notes_fresh"], &counts["notes_stale"], &counts["functions_without_note"]),
            (&json!(2), &json!(2), &json!(3)),
            "{audit}"
        );
    }

    /// O arquivo que saiu do mapa deixa a nota velha, e ela não é achada.
    #[test]
    fn a_file_gone_from_the_map_leaves_its_note_stale() {
        let dir = saved("ids-1");
        write_note(&dir, "src/ids.rs", "", NOTE);
        let without = json!({ "modules": [
            {"path": "src/invoice.rs", "blob": "invoice-1", "declarations": [function("charge_invoice", 1, "Charge the invoice.")]}
        ]});
        save_at(&model_path(dir.path()), &without, "scan 2", &languages()).unwrap();
        assert!(of_at(&model_path(dir.path()), "src/ids.rs", "").unwrap().unwrap().stale);
        assert!(searched(&dir, ASK).is_empty());
    }

    /// Mapa sem nenhuma nota responde como sempre: com a tabela vazia, e
    /// também com um mapa em que a tabela nem existe, as respostas são as
    /// mesmas.
    #[test]
    fn a_map_without_notes_answers_as_it_did() {
        let dir = saved("ids-1");
        let query = "charge invoice tax";
        let (words, triage) = (searched(&dir, query), triaged(&dir, query));
        assert_eq!(words.first().map(String::as_str), Some("src/invoice.rs"));
        assert!(fresh(opened(&dir).conn(), None).unwrap().is_empty());

        let mut db = opened(&dir);
        db.write(|tx| {
            tx.execute_batch("DROP TABLE notes")?;
            Ok(())
        })
        .unwrap();
        forget(db.conn()).unwrap();
        drop(db);
        assert!(fresh(opened(&dir).conn(), None).unwrap().is_empty(), "no table, no note");
        assert_eq!((searched(&dir, query), triaged(&dir, query)), (words, triage));
        assert_eq!(of_at(&model_path(dir.path()), "src/ids.rs", "").unwrap(), None);
    }

    /// O mapa gravado antes de existir o bloco das notas não o tem nem na lista
    /// de blocos: a abertura o cria vazio, a leitura não acha nota, e a
    /// primeira nota escrita nele vale e é achada pela busca.
    #[test]
    fn a_map_recorded_before_the_notes_block_takes_its_first_note() {
        let dir = saved("ids-1");
        let mut db = opened(&dir);
        db.write(|tx| {
            tx.execute_batch("DROP TABLE notes; DELETE FROM blocks WHERE name = 'notes'")?;
            Ok(())
        })
        .unwrap();
        forget(db.conn()).unwrap();
        drop(db);
        let model = model_path(dir.path());
        assert_eq!(of_at(&model, "src/ids.rs", "").unwrap(), None);
        assert!(!searched(&dir, ASK).contains(&"src/ids.rs".to_string()));
        write_note(&dir, "src/ids.rs", "", NOTE);
        assert!(!of_at(&model, "src/ids.rs", "").unwrap().unwrap().stale);
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
    }

    /// A nota velha nem a nota de outro arquivo mexem no que a busca achava:
    /// escrita a nota do arquivo de ids, as respostas sobre a fatura são as de
    /// antes.
    #[test]
    fn a_note_for_one_file_leaves_the_answers_about_the_others_as_they_were() {
        let dir = saved("ids-1");
        let before = (searched(&dir, "charge invoice tax"), triaged(&dir, "charge invoice tax"));
        write_note(&dir, "src/ids.rs", "", NOTE);
        assert_eq!((searched(&dir, "charge invoice tax"), triaged(&dir, "charge invoice tax")), before);
    }

    /// O índice que a gravação da nota acerta, só nos documentos do arquivo, é
    /// o que a montagem inteira faz: a busca depois de esvaziar o índice, que a
    /// refaz do zero, dá o mesmo.
    #[test]
    fn the_index_a_write_refreshes_is_the_one_a_full_rebuild_makes() {
        let dir = saved("ids-1");
        write_note(&dir, "src/ids.rs", "", NOTE);
        write_note(&dir, "src/ids.rs", "check_digits", "Confere os dígitos do documento.");
        let refreshed = searched(&dir, ASK);
        let candidates = candidates_at(&model_path(dir.path()), ASK, "", &languages(), any_path).unwrap();
        assert!(!refreshed.is_empty());

        let mut db = opened(&dir);
        db.write(|tx| forget(tx)).unwrap();
        drop(db);
        assert_eq!(searched(&dir, ASK), refreshed);
        assert_eq!(candidates_at(&model_path(dir.path()), ASK, "", &languages(), any_path).unwrap(), candidates);
    }

    /// Com o índice esvaziado, a nota grava sem refazê-lo, e a primeira busca o
    /// refaz com ela.
    #[test]
    fn a_note_written_over_an_emptied_index_is_read_by_the_search_that_rebuilds_it() {
        let dir = saved("ids-1");
        let mut db = opened(&dir);
        db.write(|tx| forget(tx)).unwrap();
        drop(db);
        write_note(&dir, "src/ids.rs", "", NOTE);
        assert_eq!(count(&dir, "SELECT count(*) FROM search_meta"), 0, "the write leaves the index emptied");
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
    }

    /// Escrever de novo o mesmo arquivo e a mesma declaração troca a nota, e a
    /// nota do arquivo e a da função convivem.
    #[test]
    fn a_rewrite_replaces_the_note_and_the_file_and_its_function_each_keep_theirs() {
        let dir = saved("ids-1");
        write_note(&dir, "src/ids.rs", "", "Primeira frase do arquivo.");
        let second = write_note(&dir, "src/ids.rs", "", NOTE);
        write_note(&dir, "src/ids.rs", "check_digits", "Frase da função.");
        assert_eq!(count(&dir, "SELECT count(*) FROM notes"), 2);
        assert_eq!(second.spec, "cadastro");
        let model = model_path(dir.path());
        assert_eq!(of_at(&model, "src/ids.rs", "").unwrap().unwrap().text, NOTE);
        assert_eq!(of_at(&model, "src/ids.rs", "check_digits").unwrap().unwrap().text, "Frase da função.");
        assert_eq!(of_at(&model, "src/ids.rs", "strip_marks").unwrap(), None);
    }

    /// A nota grava o blob do arquivo como o mapa o tem, e uma segunda passada
    /// do scan sobre o mesmo conteúdo não a apaga.
    #[test]
    fn the_note_keeps_the_blob_of_the_map_and_survives_a_scan_of_the_same_content() {
        let dir = saved("ids-1");
        write_note(&dir, "src/ids.rs", "", NOTE);
        let blob: String = opened(&dir).conn().query_row("SELECT blob FROM notes", [], |row| row.get(0)).unwrap();
        assert_eq!(blob, "ids-1");
        rescanned(&dir, "ids-1");
        assert_eq!(count(&dir, "SELECT count(*) FROM notes"), 1);
    }

    /// O texto vazio, o arquivo que o mapa não tem e a função que o arquivo não
    /// tem são recusados, e nada fica gravado.
    #[test]
    fn a_note_with_no_text_no_file_or_no_such_function_is_refused_and_nothing_is_written() {
        let dir = saved("ids-1");
        let model = model_path(dir.path());
        let refused = |new: NewNote<'_>| write(&model, dir.path(), &new).unwrap_err();
        assert!(matches!(refused(note_of("src/ids.rs", "", "  \n ")), MapRefusal::MissingArgument { .. }));
        assert!(matches!(refused(note_of("src/gone.rs", "", NOTE)), MapRefusal::UnknownFile { .. }));
        assert!(matches!(refused(note_of("src/ids.rs", "no_such", NOTE)), MapRefusal::UnknownDeclaration { .. }));
        assert_eq!(count(&dir, "SELECT count(*) FROM notes"), 0);
    }

    /// O vetor de sentido da declaração passa a trazer a nota: só o dela é
    /// refeito, e o pedido em palavras de negócio a põe na frente das outras.
    #[test]
    fn the_note_of_a_function_moves_only_its_vector_and_puts_it_first_for_the_business_words() {
        let dir = saved("ids-1");
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
        let hashes = |dir: &TempDir| -> Vec<(String, i64)> {
            let db = opened(dir);
            let mut statement = db.conn().prepare("SELECT name, hash FROM decl_vectors ORDER BY name").unwrap();
            statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(|row| row.unwrap()).collect()
        };
        let before = hashes(&dir);
        let id = |dir: &TempDir| -> i64 {
            count(dir, "SELECT rowid FROM decls WHERE name = 'check_digits'")
        };
        let cosine_before = ranked_declarations(opened(&dir).conn(), ASK).unwrap().iter().find(|s| s.id == id(&dir)).map(|s| s.cosine);

        write_note(&dir, "src/ids.rs", "check_digits", NOTE);
        let after = hashes(&dir);
        let moved: Vec<&str> =
            before.iter().zip(&after).filter(|(old, new)| old != new).map(|(_, new)| new.0.as_str()).collect();
        assert_eq!(moved, ["check_digits"], "only the declaration of the note is read again");
        let ranked = ranked_declarations(opened(&dir).conn(), ASK).unwrap();
        assert_eq!(ranked.first().map(|s| s.id), Some(id(&dir)), "{ranked:?}");
        assert!(ranked[0].cosine > cosine_before.unwrap_or(0.0) + 0.2, "{cosine_before:?} then {ranked:?}");
    }

    /// A nota do arquivo inteiro entra no vetor de todas as declarações dele.
    #[test]
    fn the_note_of_a_whole_file_reaches_the_vector_of_every_declaration_in_it() {
        let dir = saved("ids-1");
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
        write_note(&dir, "src/ids.rs", "", NOTE);
        let ranked = ranked_declarations(opened(&dir).conn(), ASK).unwrap();
        let ids_of_file: Vec<i64> = {
            let db = opened(&dir);
            let mut statement = db.conn().prepare("SELECT rowid FROM decls WHERE file = 'src/ids.rs'").unwrap();
            statement.query_map([], |row| row.get(0)).unwrap().map(|row| row.unwrap()).collect()
        };
        let first_two: Vec<i64> = ranked.iter().take(2).map(|s| s.id).collect();
        assert!(ids_of_file.iter().all(|id| first_two.contains(id)), "{first_two:?} against {ids_of_file:?}");
    }

    /// O arquivo de pagamentos com duas funções, a segunda devolvendo `second`,
    /// gravado no disco e no mapa como o scan grava.
    fn save_payments(dir: &TempDir, second: &str, mark: &str) {
        let content = format!("fn charge() {{\n    1\n}}\nfn refund() {{\n    {second}\n}}\n");
        std::fs::write(dir.path().join("src/pay.rs"), &content).unwrap();
        let declarations = json!([
            {"kind": "function", "name": "charge", "line": 1, "end_line": 3, "signature": "fn charge()"},
            {"kind": "function", "name": "refund", "line": 4, "end_line": 6, "signature": "fn refund()"}
        ]);
        let mut raw = modules("ids-1");
        raw["modules"].as_array_mut().unwrap().push(json!({
            "path": "src/pay.rs", "blob": format!("pay-{second}"), "declarations": declarations,
            "analysis": analysis_of(&content, &declarations)
        }));
        save_at(&model_path(dir.path()), &raw, mark, &languages()).unwrap();
    }

    /// Num arquivo com duas funções e a nota de cada uma, mudar o texto da
    /// segunda envelhece só a nota dela: a da primeira segue em dia, no dizer
    /// de nota velha e na busca. A nota do arquivo inteiro envelhece com ele.
    #[test]
    fn a_changed_function_ages_only_its_own_note() {
        let dir = saved("ids-1");
        save_payments(&dir, "2", "scan 1");
        let model = model_path(dir.path());
        write_note(&dir, "src/pay.rs", "charge", "Cobra o cliente.");
        write_note(&dir, "src/pay.rs", "refund", "Devolve o dinheiro do cliente.");
        write_note(&dir, "src/pay.rs", "", "Pagamentos do cliente.");
        let stale = |name: &str| of_at(&model, "src/pay.rs", name).unwrap().unwrap().stale;
        assert_eq!((stale("charge"), stale("refund"), stale("")), (false, false, false));

        save_payments(&dir, "3", "scan 2");
        assert_eq!((stale("charge"), stale("refund"), stale("")), (false, true, true));
        let in_search: Vec<String> =
            fresh(opened(&dir).conn(), Some(&["src/pay.rs"])).unwrap().into_iter().map(|note| note.name).collect();
        assert_eq!(in_search, ["charge"]);
    }

    /// O arquivo cujo texto analisado não é JSON recusa a nota como o que o
    /// scan não leu, sem cair na leitura, e nada fica gravado.
    #[test]
    fn a_file_whose_analysis_is_not_json_refuses_the_note() {
        let dir = saved("ids-1");
        opened(&dir).conn().execute("UPDATE texts SET analysis = 'not-json' WHERE path = 'src/ids.rs'", []).unwrap();
        let refused = write(&model_path(dir.path()), dir.path(), &note_of("src/ids.rs", "check_digits", NOTE)).unwrap_err();
        assert!(matches!(&refused, MapRefusal::MapUnreadable { detail } if detail == "note-needs-current-scan"), "{refused:?}");
        assert_eq!(count(&dir, "SELECT count(*) FROM notes"), 0);
    }

    /// A nota velha sai do vetor: depois que o arquivo muda, o texto compilado
    /// volta ao de antes da nota.
    #[test]
    fn a_stale_note_leaves_the_vector_when_the_vectors_are_filled_again() {
        let dir = saved("ids-1");
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
        let hash = |dir: &TempDir| -> i64 { count(dir, "SELECT hash FROM decl_vectors WHERE name = 'check_digits'") };
        let plain = hash(&dir);
        write_note(&dir, "src/ids.rs", "check_digits", NOTE);
        assert_ne!(hash(&dir), plain);
        rescanned(&dir, "ids-2");
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
        assert_eq!(hash(&dir), plain, "the stale note no longer counts");
    }
}
