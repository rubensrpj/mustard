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
//! texto, a spec que a escreveu e o blob do arquivo no mapa no momento da
//! escrita. Ninguém refaz a nota a partir do código: quem a escreve é o
//! agente, e a busca nunca chama modelo nenhum para escrevê-la.
//!
//! **A nota envelhece com o arquivo.** Ela vale enquanto o blob do arquivo no
//! mapa for o que ela guardou. O arquivo que mudou, ou que saiu do mapa, deixa
//! a nota velha: ela fica gravada e visível para quem lê o trecho, que a
//! reescreve, mas não entra mais na busca, porque uma nota que já não descreve
//! o código acharia o arquivo pelo motivo errado. Fora do git o mapa não tem
//! blob, e a nota não envelhece.
//!
//! Onde a nota entra na busca — no índice de palavras e no texto compilado de
//! cada declaração, de onde saem os vetores de sentido — quem lê é
//! [`crate::io::map_notes_fresh`], que só conhece o banco. Este módulo grava a
//! nota e, por isso, refaz o índice e os vetores: fica em cima da busca e do
//! sentido, e nenhum dos dois o usa.
//!
//! Mapa sem nenhuma nota, ou sem a tabela, responde como sempre.

use std::path::Path;

use rusqlite::{params, OptionalExtension};

use crate::domain::project_map::{clean_path, MapRefusal};
use crate::io::map_db::table_exists;
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

/// Uma nota como o mapa a guarda, com o dizer se ela já envelheceu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub file: String,
    pub name: String,
    pub text: String,
    pub spec: String,
    /// O arquivo mudou, ou saiu do mapa, depois que a nota foi escrita.
    pub stale: bool,
}

/// Grava a nota `new` no mapa em `model`, de projeto em `root`, no lugar da
/// que o mesmo arquivo e a mesma declaração já tinham, e devolve o que ficou
/// gravado. O blob que ela guarda é o do arquivo no mapa; a nota entra no
/// índice de busca na mesma transação, só nos documentos do arquivo, e os
/// vetores de sentido se refazem em seguida.
///
/// Recusa o texto vazio, o arquivo que o mapa não tem e a declaração que o
/// arquivo não tem.
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
    let Some(blob) = blob else { return Err(MapRefusal::UnknownFile { file }) };
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
            "INSERT INTO notes(file, name, text, spec, blob) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![note.file, note.name, note.text, note.spec, blob],
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
            "SELECT n.text, n.spec, f.path IS NULL OR f.blob IS NOT n.blob FROM notes n \
             LEFT JOIN files f ON f.path = n.file WHERE n.file = ?1 AND n.name = ?2",
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
    use crate::io::map_meaning::{fill_at, ranked_declarations};
    use crate::io::map_notes_fresh::fresh;
    use crate::io::map_search::{any_path, candidates_at, forget, search_at};
    use crate::io::map_triage::triage_at;
    use crate::io::project_map::{model_path, save_at};
    use serde_json::{json, Value};
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
        json!({ "modules": [
            {"path": "src/ids.rs", "blob": ids_blob, "declarations": [
                function("check_digits", 1, "Rejects a malformed number."),
                function("strip_marks", 10, "Removes dots and dashes.")]},
            {"path": "src/invoice.rs", "blob": "invoice-1", "declarations": [
                function("charge_invoice", 1, "Charge the customer invoice with the tax.")]},
            {"path": "src/config.rs", "blob": "config-1", "declarations": [
                function("parse_config", 1, "Parse the configuration file.")]}
        ]})
    }

    /// Um projeto gravado como o scan grava, com o arquivo de ids no blob dado.
    fn saved(ids_blob: &str) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        save_at(&model_path(dir.path()), &modules(ids_blob), "scan 1", &languages()).unwrap();
        dir
    }

    /// O scan que grava de novo o mesmo projeto, agora com o arquivo de ids no
    /// blob dado.
    fn rescanned(dir: &TempDir, ids_blob: &str) {
        save_at(&model_path(dir.path()), &modules(ids_blob), "scan 2", &languages()).unwrap();
        fill_at(&model_path(dir.path()), dir.path()).unwrap();
    }

    fn note_of(file: &str, name: &str, text: &str) -> NewNote<'static> {
        NewNote { file: Box::leak(file.into()), name: Box::leak(name.into()), text: Box::leak(text.into()), spec: "cadastro" }
    }

    fn write_note(dir: &TempDir, file: &str, name: &str, text: &str) -> Note {
        write(&model_path(dir.path()), dir.path(), &note_of(file, name, text)).unwrap()
    }

    /// Os caminhos da busca de arquivos por palavras.
    fn searched(dir: &TempDir, query: &str) -> Vec<String> {
        search_at(&model_path(dir.path()), query, &languages(), TOP).unwrap().into_iter().map(|found| found.path).collect()
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

    /// Num mapa sem blob (fora do git), a nota vale e não envelhece: a busca a
    /// acha, ela não vem como velha, e a passada seguinte do scan não a apaga.
    #[test]
    fn a_map_without_blobs_keeps_its_note_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let bare = json!({ "modules": [
            {"path": "src/ids.rs", "declarations": [function("check_digits", 1, "Rejects a malformed number.")]},
            {"path": "src/invoice.rs", "declarations": [function("charge_invoice", 1, "Charge the customer invoice.")]}
        ]});
        let model = model_path(dir.path());
        save_at(&model, &bare, "scan 1", &languages()).unwrap();
        write(&model, dir.path(), &note_of("src/ids.rs", "", NOTE)).unwrap();
        assert!(!of_at(&model, "src/ids.rs", "").unwrap().unwrap().stale);
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
        save_at(&model, &bare, "scan 2", &languages()).unwrap();
        assert!(!of_at(&model, "src/ids.rs", "").unwrap().unwrap().stale);
        assert_eq!(searched(&dir, ASK).first().map(String::as_str), Some("src/ids.rs"));
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
        let refreshed = search_at(&model_path(dir.path()), ASK, &languages(), TOP).unwrap();
        let candidates = candidates_at(&model_path(dir.path()), ASK, "", &languages(), any_path).unwrap();
        assert!(!refreshed.is_empty());

        let mut db = opened(&dir);
        db.write(|tx| forget(tx)).unwrap();
        drop(db);
        assert_eq!(search_at(&model_path(dir.path()), ASK, &languages(), TOP).unwrap(), refreshed);
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
