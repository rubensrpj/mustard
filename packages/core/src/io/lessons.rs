//! `lessons` — o banco de lições no disco (`.claude/spec/lessons.ndjson`, no
//! checkout principal).
//!
//! Só o binário escreve no banco, e do mesmo jeito que no arquivo de eventos
//! da spec: cada gravação pega a trava do banco, lê o maior número, soma 1 e
//! acrescenta a linha inteira de uma vez; outra gravação ao mesmo tempo espera
//! a trava e grava com o número seguinte. Uma lição recusada não toca no
//! arquivo. A leitura pega a trava compartilhada.
//!
//! As regras da lição moram em `domain::lessons`; aqui ficam o disco, a trava
//! e o relógio.

use std::path::Path;

use serde_json::{Map, Value};

use crate::domain::lessons as model;
use crate::domain::spec_events::{self as events, Refusal, SpecLog};
use crate::io::fs::lock::{read_shared, LockedFile};
use crate::platform::error::Error;

/// O que uma gravação deixou no banco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenLesson {
    /// O número da lição no banco.
    pub id: u64,
    /// A classe da lição.
    pub class: String,
}

/// Grava uma lição com a hora de agora. Veja [`write_at`].
pub fn write(path: &Path, draft: Map<String, Value>, spec: Option<&str>) -> Result<WrittenLesson, Refusal> {
    write_at(path, draft, spec, &crate::io::spec_events::now())
}

/// Grava a lição de `draft` no banco `path`, com a hora `at`. `spec`, quando
/// vem, diz em que spec a lição nasceu, se ela mesma não diz.
///
/// Recusa, sem tocar no arquivo: classe desconhecida, campo obrigatório
/// vazio, lição sem dizer onde vale ou onde nasceu, código mandado por quem
/// grava e lição substituída que não existe no banco.
pub fn write_at(path: &Path, draft: Map<String, Value>, spec: Option<&str>, at: &str) -> Result<WrittenLesson, Refusal> {
    let event = model::normalize(draft, spec);
    model::validate(&event)?;
    // Num banco que ainda não existe, nenhuma lição pode ser substituída: a
    // recusa sai antes de o arquivo nascer.
    if let Some(id) = event.get("replaces").and_then(Value::as_u64)
        && !path.is_file()
    {
        return Err(Refusal::UnknownLesson { id });
    }
    let mut file = LockedFile::exclusive(path).map_err(io_refusal)?;
    let content = file.read_to_string().map_err(io_refusal)?;
    let bank = events::parse_log(&content);
    model::check_against(&bank, &event)?;
    let id = bank.max_id().saturating_add(1);
    let class = event.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
    let line = events::render_line(&events::stamp(event, id, None, at));
    // Uma última linha pela metade fica sozinha na linha dela.
    let clean = content.is_empty() || content.ends_with('\n');
    let added = if clean { line } else { format!("\n{line}") };
    file.append_line(&added).map_err(io_refusal)?;
    drop(file);
    Ok(WrittenLesson { id, class })
}

/// Lê o banco inteiro, com a trava compartilhada. `Ok(None)` quando o banco
/// ainda não existe.
pub fn read(path: &Path) -> Result<Option<SpecLog>, Refusal> {
    match read_shared(path) {
        Ok(content) => Ok(Some(events::parse_log(&content))),
        Err(Error::NotFound(_)) => Ok(None),
        Err(e) => Err(io_refusal(e)),
    }
}

/// Recalcula o `search` de cada lição com o redutor de hoje, com a trava do
/// banco, e devolve quantas linhas mudaram. O arquivo só é reescrito quando
/// algo mudou; sem banco, nada é criado.
pub fn refresh_search(path: &Path) -> Result<usize, Refusal> {
    let mut file = match LockedFile::existing(path) {
        Ok(file) => file,
        Err(Error::NotFound(_)) => return Ok(0),
        Err(e) => return Err(io_refusal(e)),
    };
    let content = file.read_to_string().map_err(io_refusal)?;
    let (fixed, changed) = events::refresh_search_lines(&content);
    if changed > 0 {
        file.replace(fixed.as_bytes()).map_err(io_refusal)?;
    }
    Ok(changed)
}

/// Quantas lições têm o `search` calculado por outro redutor. Só lê; sem
/// banco, zero.
pub fn stale_search(path: &Path) -> Result<usize, Refusal> {
    match read_shared(path) {
        Ok(content) => Ok(events::refresh_search_lines(&content).1),
        Err(Error::NotFound(_)) => Ok(0),
        Err(e) => Err(io_refusal(e)),
    }
}

fn io_refusal(error: Error) -> Refusal {
    Refusal::Io { detail: error.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::lessons::matching;
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    fn at(hm: &str) -> String {
        format!("2026-09-12T{hm}:00-03:00")
    }

    fn defect(text: &str, keys: &[&str]) -> Value {
        json!({"class": "defect", "text": text, "keys": keys, "applies_to": {"subproject": "apps/rt"}, "found_in": {"spec": "s", "branch": "b", "commit": "abc1234"}})
    }

    fn put(path: &Path, draft: Value) -> WrittenLesson {
        write_at(path, obj(draft), None, &at("10:00")).unwrap_or_else(|r| panic!("refused: {r:?}"))
    }

    #[test]
    fn a_lesson_is_written_with_the_next_number_and_its_search_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        assert_eq!(put(&path, defect("Um", &["um"])), WrittenLesson { id: 1, class: "defect".into() });
        let second = write_at(&path, obj(defect("Dois", &["dois"])), Some("outra"), &at("10:01")).unwrap();
        assert_eq!(second.id, 2);

        let raw = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[1].starts_with(&format!(r#"{{"v":1,"id":2,"at":"{}","type":"defect","author":"assistant","applies_to":"#, at("10:01"))),
            "{}",
            lines[1]
        );
        assert!(!lines[1].contains("\"code\""), "a lesson has no item code: {}", lines[1]);
        assert!(lines[1].ends_with(&format!(r#""search":"{}"}}"#, events::search_field(Some("Dois"), &["dois"]))), "{}", lines[1]);
        assert!(lines[1].contains(r#""found_in":{"branch":"b","commit":"abc1234","spec":"s"}"#), "the lesson's own spec stays: {}", lines[1]);
        let bank = read(&path).unwrap().unwrap();
        assert_eq!(bank.events.len(), 2);
    }

    #[test]
    fn two_lesson_writes_at_once_get_consecutive_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec").join("lessons.ndjson");
        let each = 10;
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let writers: Vec<_> = (0..2)
            .map(|w| {
                let path = path.clone();
                let start = std::sync::Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    (0..each).map(|i| put(&path, defect(&format!("lição {w}-{i}"), &["k"])).id).collect::<Vec<u64>>()
                })
            })
            .collect();
        let mut ids: Vec<u64> = writers.into_iter().flat_map(|h| h.join().unwrap()).collect();
        ids.sort_unstable();
        assert_eq!(ids, (1..=2 * each).collect::<Vec<u64>>());
        let bank = read(&path).unwrap().unwrap();
        assert!(bank.skipped.is_empty(), "{:?}", bank.skipped);
    }

    #[test]
    fn a_refused_lesson_leaves_the_bank_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        put(&path, defect("Um", &["um"]));
        let before = std::fs::read(&path).unwrap();
        let mut no_class = defect("x", &["k"]);
        no_class.as_object_mut().unwrap().remove("class");
        let mut unknown_class = defect("x", &["k"]);
        unknown_class["class"] = json!("bug");
        let mut replaces = defect("x", &["k"]);
        replaces["replaces"] = json!(99);
        for draft in [no_class, unknown_class, replaces, json!({"class": "defect", "text": "x", "keys": ["k"], "found_in": {"spec": "s"}})] {
            assert!(write_at(&path, obj(draft.clone()), None, &at("11:00")).is_err(), "{draft}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn replacing_a_lesson_that_does_not_exist_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        let mut draft = defect("Nova", &["k"]);
        draft["replaces"] = json!(3);
        assert_eq!(write_at(&path, obj(draft.clone()), None, &at("10:00")).unwrap_err(), Refusal::UnknownLesson { id: 3 });
        assert!(!path.exists(), "the refusal created no bank");

        let first = put(&path, defect("Velha", &["k"]));
        let refusal = write_at(&path, obj(draft.clone()), None, &at("10:01")).unwrap_err();
        assert_eq!(refusal, Refusal::UnknownLesson { id: 3 });
        assert_eq!(refusal.reason(), "unknown-lesson");
        draft["replaces"] = json!(first.id);
        let newer = put(&path, draft);
        let bank = read(&path).unwrap().unwrap();
        assert_eq!(bank.visible().iter().map(|l| l.id).collect::<Vec<_>>(), [newer.id]);
    }

    /// Gravada pelo gravador de verdade, a lição com a chave "apagar" é
    /// achada por "apagando a pasta" entre as 5 mais fortes, no meio de
    /// outras.
    #[test]
    fn a_lesson_keyed_apagar_is_found_for_apagando_a_pasta() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        put(&path, defect("O cargo não está no PATH.", &["cargo", "PATH"]));
        put(&path, defect("A pasta temporária some depois do teste.", &["pasta", "teste"]));
        let target = put(&path, defect("Um rm -rf na pasta errada perde trabalho.", &["apagar", "rm"]));
        put(&path, defect("O título tem até 60 caracteres.", &["título"]));
        put(&path, defect("Os testes gravam numa pasta temporária.", &["pasta"]));
        put(&path, defect("Gancho não entra em pânico.", &["gancho"]));
        let bank = read(&path).unwrap().unwrap();
        let hits = matching(&bank, "apagando a pasta");
        assert!(hits.len() <= crate::domain::search::TOP);
        assert!(hits.iter().any(|h| h.id == target.id), "{hits:?}");
    }

    /// Com mais de cinco lições que casam o pedido, quase todas pela "pasta",
    /// a única gravada com a chave "apagar" continua entre as 5 mais fortes,
    /// mesmo gravada por último e sem "pasta" no texto: o termo raro pesa mais
    /// que o comum.
    #[test]
    fn the_lesson_keyed_apagar_stays_in_the_top_five_among_many_that_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        put(&path, defect("O cargo não está no PATH.", &["cargo"]));
        for text in [
            "A pasta temporária some depois do teste.",
            "Os testes gravam numa pasta temporária.",
            "Uma pasta de spec tem o arquivo de eventos.",
            "A pasta do plugin vai inteira para a instalação.",
            "A pasta de rascunho é da sessão.",
            "Nenhuma pasta aninhada guarda estado.",
            "A pasta target fica fora da varredura.",
        ] {
            put(&path, defect(text, &["pasta"]));
        }
        let target = put(&path, defect("Um rm -rf no diretório errado perde trabalho.", &["apagar", "rm"]));
        let bank = read(&path).unwrap().unwrap();

        let top = crate::domain::search::TOP;
        let pasta = &crate::domain::search::query_terms("pasta")[0];
        let with_pasta = bank
            .visible()
            .iter()
            .filter(|l| l.str_field("search").unwrap_or_default().split(' ').any(|w| w == pasta))
            .count();
        assert!(with_pasta > top, "more lessons match than come back: {with_pasta}");

        let hits = matching(&bank, "apagando a pasta");
        assert_eq!(hits.len(), top, "{hits:?}");
        assert!(hits.iter().any(|h| h.id == target.id), "the lesson keyed apagar is in the top five: {hits:?}");
    }

    #[test]
    fn the_search_of_the_bank_is_recomputed_only_where_it_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        assert_eq!(refresh_search(&path).unwrap(), 0);
        assert!(!path.exists(), "no bank is created");
        put(&path, defect("Um", &["um"]));
        put(&path, defect("Dois", &["dois"]));
        let original = std::fs::read_to_string(&path).unwrap();
        let stale = original.replacen("\"search\":\"um\"", "\"search\":\"velho\"", 1);
        assert_ne!(stale, original);
        std::fs::write(&path, &stale).unwrap();
        assert_eq!(stale_search(&path).unwrap(), 1);
        assert_eq!(refresh_search(&path).unwrap(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(stale_search(&path).unwrap(), 0);
    }
}
