//! `lessons` — o banco de lições no disco (`.claude/spec/lessons.ndjson`, no
//! checkout principal).
//!
//! Só o binário escreve no banco, e do mesmo jeito que no arquivo de eventos
//! da spec: cada gravação pega a trava do banco, lê o maior número, soma 1 e
//! acrescenta a linha inteira de uma vez; outra gravação ao mesmo tempo espera
//! a trava e grava com o número seguinte. A lição que repete o texto de outra
//! já guardada é conferida com a trava presa, sobre o banco que acabou de ser
//! lido: duas gravações do mesmo texto ao mesmo tempo deixam uma lição só.
//! Juntar lições e retirar uma lição passam pela mesma trava: a conferência
//! de que a lição apontada ainda está na leitura é feita sobre o banco lido
//! com a trava presa, então duas rodadas que juntam ou retiram a mesma lição
//! ao mesmo tempo deixam uma gravação só, e a outra é recusada.
//! Uma lição recusada não toca no arquivo. A leitura pega a trava
//! compartilhada.
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
    /// A classe da lição; numa retirada, o tipo dela (`remove`).
    pub class: String,
    /// As lições que saíram da leitura com esta gravação: as que a lição nova
    /// substitui ou junta, ou as que a retirada aponta.
    pub hidden: Vec<u64>,
}

/// Grava uma lição com a hora de agora. Veja [`write_at`].
pub fn write(path: &Path, draft: Map<String, Value>, spec: Option<&str>) -> Result<WrittenLesson, Refusal> {
    write_at(path, draft, spec, &crate::io::spec_events::now())
}

/// Grava a lição de `draft` no banco `path`, com a hora `at`. `spec`, quando
/// vem, diz em que spec a lição nasceu, se ela mesma não diz.
///
/// Com `replaces` apontando várias lições, a lição nova junta todas elas numa
/// só; um rascunho só com `targets` e `reason` retira as lições apontadas.
///
/// Recusa, sem tocar no arquivo: classe desconhecida, campo obrigatório
/// vazio, lição sem dizer onde vale ou onde nasceu, código mandado por quem
/// grava, lição substituída, junta ou retirada que a leitura do banco não
/// mostra e lição com o mesmo texto de outra já guardada
/// (`domain::lessons::repeated`).
pub fn write_at(path: &Path, draft: Map<String, Value>, spec: Option<&str>, at: &str) -> Result<WrittenLesson, Refusal> {
    let event = model::normalize(draft, spec);
    model::validate(&event)?;
    let hidden = model::hidden_by(&event);
    // Num banco que ainda não existe, nenhuma lição pode ser substituída nem
    // retirada: a recusa sai antes de o arquivo nascer.
    if let Some(id) = hidden.first().copied()
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
    Ok(WrittenLesson { id, class, hidden })
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
        assert_eq!(put(&path, defect("Um", &["um"])), WrittenLesson { id: 1, class: "defect".into(), hidden: vec![] });
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

    /// O mesmo texto, com outros espaços, maiúsculas e acentos, não entra de
    /// novo: a recusa aponta a lição que já existe, e o banco fica com os
    /// mesmos bytes.
    #[test]
    fn a_lesson_repeating_one_in_the_bank_is_refused_and_the_bank_keeps_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        let first = put(&path, defect("Não apague a pasta de outra sessão.", &["apagar"]));
        let before = std::fs::read(&path).unwrap();
        let mut again = defect("NAO apague  a pasta de outra sessao.", &["pasta"]);
        again["class"] = json!("project_rule");
        let refusal = write_at(&path, obj(again), None, &at("10:05")).unwrap_err();
        assert_eq!(refusal, Refusal::LessonRepeated { id: first.id, text: "Não apague a pasta de outra sessão.".into() });
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let other = put(&path, defect("Não apague a pasta de outra sessão sem perguntar.", &["apagar"]));
        assert_eq!(other.id, first.id + 1, "outro texto entra");
    }

    /// Duas gravações do mesmo texto ao mesmo tempo: a trava cobre a leitura
    /// do banco, a comparação e a gravação, então só uma entra e a outra é
    /// recusada apontando a primeira.
    #[test]
    fn two_writes_of_the_same_lesson_at_once_leave_one_lesson() {
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("spec").join("lessons.ndjson");
            let start = std::sync::Arc::new(std::sync::Barrier::new(2));
            let writers: Vec<_> = (0..2)
                .map(|_| {
                    let path = path.clone();
                    let start = std::sync::Arc::clone(&start);
                    std::thread::spawn(move || {
                        start.wait();
                        write_at(&path, obj(defect("A mesma lição, gravada duas vezes.", &["k"])), None, &at("10:00"))
                    })
                })
                .collect();
            let results: Vec<Result<WrittenLesson, Refusal>> = writers.into_iter().map(|h| h.join().unwrap()).collect();
            let written: Vec<u64> = results.iter().filter_map(|r| r.as_ref().ok().map(|w| w.id)).collect();
            assert_eq!(written, [1], "{results:?}");
            assert!(
                results.iter().any(|r| matches!(r, Err(Refusal::LessonRepeated { id: 1, .. }))),
                "{results:?}"
            );
            assert_eq!(read(&path).unwrap().unwrap().events.len(), 1);
        }
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

    /// Uma lição que vale no projeto todo, com as palavras-chave `keys`.
    fn everywhere(text: &str, keys: &[&str]) -> Value {
        json!({"class": "defect", "text": text, "keys": keys, "applies_to": {"files": ["**"]}, "found_in": {"spec": "s"}})
    }

    /// Os textos das lições que o pedido da onda 1 e o da revisão dela
    /// levam, montados como a rodada os monta, do banco do projeto `root`.
    fn requested(root: &Path) -> (String, String) {
        let log = events::parse_log(&format!(
            "{}\n{}\n",
            r#"{"v":1,"id":1,"at":"2026-09-18T10:00:00-03:00","type":"wave","n":1,"text":"A onda","criteria":[],"done_when":"passa"}"#,
            r#"{"v":1,"id":2,"at":"2026-09-18T10:00:00-03:00","type":"task","wave":1,"text":"Apagar a pasta velha e rodar a suíte em primeiro plano.","files":[{"path":"src/a.rs"}]}"#,
        ));
        let built = crate::io::wave_prompt::prompts(root, "teste", &log, crate::platform::i18n::Locale::PtBr, &Default::default());
        (built[0].text.clone(), built[0].review.clone())
    }

    /// Pelo gravador do comando de gravar lição: duas lições parecidas viram
    /// uma só, que aponta as duas em `replaces`, e a que já não vale é
    /// retirada com o motivo. A leitura do banco mostra a lição junta no lugar
    /// das duas e deixa de mostrar a retirada; o pedido da onda e o da
    /// revisão, que levavam as três, passam a levar só a junta. Nenhuma linha
    /// sai do arquivo.
    #[test]
    fn merging_two_lessons_and_retiring_one_leave_the_bank_lean_and_the_requests_without_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = crate::ClaudePaths::for_project(root).unwrap().lessons_path();
        let first = put(&path, everywhere("Apagar a pasta de outra sessão perde o trabalho dela.", &["apagar", "pasta"]));
        let second = put(&path, everywhere("Remover a pasta alheia joga fora o que a outra sessão fez.", &["pasta", "remover"]));
        let old = put(&path, everywhere("A suíte roda em segundo plano no servidor antigo.", &["suíte", "primeiro plano"]));
        let (text, review) = requested(root);
        for lesson in ["Apagar a pasta de outra sessão", "Remover a pasta alheia", "A suíte roda em segundo plano"] {
            assert!(text.contains(lesson) && review.contains(lesson), "antes, o pedido leva {lesson}: {text}");
        }

        let mut merged = everywhere("Nunca apague a pasta de outra sessão: o trabalho dela se perde.", &["apagar", "pasta"]);
        merged["replaces"] = json!([first.id, second.id]);
        let merged = write_at(&path, obj(merged), None, &at("10:10")).unwrap();
        assert_eq!(merged.hidden, [first.id, second.id]);
        let retired = write_at(&path, obj(json!({"targets": [old.id], "reason": "o servidor antigo saiu"})), None, &at("10:11")).unwrap();
        assert_eq!((retired.class.as_str(), retired.hidden.as_slice()), (model::RETIRE, [old.id].as_slice()));

        let bank = read(&path).unwrap().unwrap();
        assert_eq!(model::kept(&bank).iter().map(|l| l.id).collect::<Vec<_>>(), [merged.id]);
        assert_eq!(bank.events.len(), 5, "as linhas antigas ficam no arquivo");
        let (text, review) = requested(root);
        assert!(text.contains("Nunca apague a pasta de outra sessão") && review.contains("Nunca apague a pasta de outra sessão"), "{text}");
        for gone in ["Apagar a pasta de outra sessão", "Remover a pasta alheia", "A suíte roda em segundo plano"] {
            assert!(!text.contains(gone) && !review.contains(gone), "{gone} saiu dos dois pedidos: {text}");
        }
    }

    /// Só se junta ou retira a lição que a leitura ainda mostra: a lição já
    /// junta ou retirada, e a que nunca existiu, são recusadas pelo número, e
    /// o banco fica com os mesmos bytes.
    #[test]
    fn a_lesson_already_merged_or_retired_is_not_merged_or_retired_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        assert_eq!(write_at(&path, obj(json!({"targets": [1], "reason": "r"})), None, &at("10:00")).unwrap_err(), Refusal::UnknownLesson { id: 1 });
        assert!(!path.exists(), "a recusa não criou o banco");
        let a = put(&path, defect("Um", &["um"]));
        let b = put(&path, defect("Dois", &["dois"]));
        let mut merged = defect("Um e dois", &["um", "dois"]);
        merged["replaces"] = json!([a.id, b.id]);
        put(&path, merged);
        let gone = put(&path, json!({"targets": [3], "reason": "saiu"}));
        let before = std::fs::read(&path).unwrap();
        for (draft, id) in [
            (json!({"targets": [a.id], "reason": "de novo"}), a.id),
            (json!({"targets": [3], "reason": "de novo"}), 3),
            (json!({"targets": [gone.id], "reason": "a linha da retirada"}), gone.id),
            (json!({"class": "defect", "text": "Três", "keys": ["k"], "applies_to": {"subproject": "apps/rt"}, "found_in": {"spec": "s"}, "replaces": [b.id, 99]}), b.id),
        ] {
            assert_eq!(write_at(&path, obj(draft.clone()), None, &at("10:20")).unwrap_err(), Refusal::UnknownLesson { id }, "{draft}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    /// A retirada leva só `targets` e `reason`. Com o filtro por hora do
    /// `remove` da spec, que pegaria as três lições, com uma substituição que
    /// apontaria outra ou com um texto, ela é recusada pelo nome do campo, e o
    /// banco fica com os mesmos bytes e as três lições na leitura. A mesma
    /// retirada sem o campo a mais entra e tira da leitura só a lição
    /// apontada, a que a gravação devolve.
    #[test]
    fn a_retirement_with_a_filter_or_a_replacement_is_refused_and_hides_only_what_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lessons.ndjson");
        let a = write_at(&path, obj(defect("Um", &["um"])), None, &at("10:00")).unwrap();
        let b = write_at(&path, obj(defect("Dois", &["dois"])), None, &at("10:01")).unwrap();
        let c = write_at(&path, obj(defect("Três", &["tres"])), None, &at("10:02")).unwrap();
        let kept = |path: &Path| model::kept(&read(path).unwrap().unwrap()).iter().map(|l| l.id).collect::<Vec<_>>();
        let before = std::fs::read(&path).unwrap();
        let every_lesson = json!({"type": "defect", "from": "2026-09-12T10:00", "to": "2026-09-12T10:02"});
        for (extra, value) in [
            ("filter", every_lesson),
            ("replaces", json!([a.id])),
            ("replaces", json!(b.id)),
            ("text", json!("O servidor antigo saiu.")),
        ] {
            let mut draft = json!({"targets": [c.id], "reason": "o servidor antigo saiu"});
            draft[extra] = value;
            let refused = write_at(&path, obj(draft.clone()), None, &at("10:10"));
            let expected = Refusal::UnknownField {
                event_type: model::RETIRE.into(),
                field: extra.into(),
                accepted: "targets, reason, author".into(),
            };
            assert_eq!(refused, Err(expected), "{draft}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), before, "a recusa não gravou nada");
        assert_eq!(kept(&path), [a.id, b.id, c.id]);

        let draft = json!({"targets": [c.id], "reason": "o servidor antigo saiu", "author": "assistant"});
        let retired = write_at(&path, obj(draft), None, &at("10:10")).unwrap();
        assert_eq!(retired.hidden, [c.id]);
        assert_eq!(kept(&path), [a.id, b.id]);
    }

    /// Duas rodadas que juntam as mesmas lições ao mesmo tempo, com textos
    /// diferentes, e duas que retiram a mesma lição ao mesmo tempo: a trava
    /// cobre a leitura do banco, a conferência e a gravação, então só uma de
    /// cada par grava, e a outra é recusada apontando a lição que já saiu.
    #[test]
    fn two_merges_or_two_retirements_of_the_same_lessons_at_once_leave_one() {
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("spec").join("lessons.ndjson");
            let a = put(&path, defect("Um", &["um"]));
            let b = put(&path, defect("Dois", &["dois"]));
            let c = put(&path, defect("Três", &["tres"]));
            let start = std::sync::Arc::new(std::sync::Barrier::new(4));
            let writers: Vec<_> = (0..4)
                .map(|w| {
                    let path = path.clone();
                    let start = std::sync::Arc::clone(&start);
                    std::thread::spawn(move || {
                        let draft = if w < 2 {
                            let mut merged = defect(&format!("Um e dois, versão {w}"), &["um", "dois"]);
                            merged["replaces"] = json!([a.id, b.id]);
                            merged
                        } else {
                            json!({"targets": [c.id], "reason": format!("rodada {w}")})
                        };
                        start.wait();
                        (w < 2, write_at(&path, obj(draft), None, &at("10:30")))
                    })
                })
                .collect();
            let results: Vec<(bool, Result<WrittenLesson, Refusal>)> = writers.into_iter().map(|h| h.join().unwrap()).collect();
            for merge in [true, false] {
                let pair: Vec<&Result<WrittenLesson, Refusal>> = results.iter().filter(|(m, _)| *m == merge).map(|(_, r)| r).collect();
                assert_eq!(pair.iter().filter(|r| r.is_ok()).count(), 1, "{results:?}");
                assert!(pair.iter().any(|r| matches!(r, Err(Refusal::UnknownLesson { .. }))), "{results:?}");
            }
            let bank = read(&path).unwrap().unwrap();
            assert_eq!(model::kept(&bank).len(), 1, "só a lição junta fica: {:?}", bank.events);
            assert!(bank.skipped.is_empty(), "{:?}", bank.skipped);
        }
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
