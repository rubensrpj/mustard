//! `mustard-rt run read <bloco> [--spec <nome>]` — devolve só o bloco pedido
//! do arquivo de eventos da spec, sem os itens removidos ou substituídos e sem
//! o campo `search`.
//!
//! Sem `--spec`, lê a spec atual, pela mesma escada de todas as portas: a
//! variável `MUSTARD_ACTIVE_SPEC`, depois a branch do checkout, depois a spec
//! ligada à sessão. Sem nenhuma, recusa.
//!
//! A saída é um JSON com um evento por linha, na ordem do arquivo:
//!
//! ```text
//! {"ok":true,"spec":"teste","block":"wave-2","count":2,"events":[
//! {"v":1,"id":19,…,"type":"wave",…},
//! {"v":1,"id":20,…,"type":"task",…}
//! ]}
//! ```
//!
//! Uma linha do arquivo que não se entende entra em `warnings`, no idioma do
//! projeto, e o resto é lido.
//!
//! Com `--term`, a leitura devolve o que o termo acha, do mais forte para o
//! menos forte: um código de item devolve aquele item, e qualquer outro termo
//! passa pela busca por nota, que não exige que o item tenha todas as
//! palavras.
//!
//! O bloco `lessons` foge dessa regra: fica fora da spec, vive no banco do
//! projeto (`.claude/spec/lessons.ndjson`) e o `--term` é o número da lição
//! no banco, não uma busca. É assim que o pedido de uma onda leva a lição só
//! pelo número dela, sem copiar o texto.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::domain::lessons::kept;
use mustard_core::domain::spec_events::{found_by, shown_line, Block, BlockQuery, Refusal, SpecEvent};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::Locale;
use mustard_core::ClaudePaths;
use serde_json::{json, Value};

use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// Options for `mustard-rt run read`.
pub struct ReadOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec pedida; sem ela, a spec atual.
    pub spec: Option<String>,
    pub block: String,
    pub term: Option<String>,
}

/// O núcleo testável de [`run`]: a saída pronta, ou a recusa. A sessão vem
/// do ambiente. Nunca entra em pânico.
pub(crate) fn read_at(opts: &ReadOpts) -> Result<String, Value> {
    read_for(opts, session_from_env().as_deref())
}

/// [`read_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn read_for(opts: &ReadOpts, session: Option<&str>) -> Result<String, Value> {
    let project = super::project(&opts.root);
    let lang = project.lang;
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);

    let block = opts.block.trim();
    if block == "lessons" {
        return read_lessons(&project.root, opts.spec.as_deref(), opts.term.as_deref(), lang);
    }
    let Some(query) = BlockQuery::parse(block) else {
        return Err(refuse(Refusal::UnknownBlock { found: block.to_string() }));
    };
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root))
            .active(session)
            .ok_or_else(|| refuse(Refusal::NoCurrentSpec))?,
    };
    let path = store::spec_file(&project.root, &spec).map_err(refuse)?;
    let Some(log) = store::read(&path).map_err(refuse)? else {
        return Err(refuse(Refusal::NoSpecFile { spec }));
    };
    let codes = log.codes();
    let term = opts.term.as_deref().unwrap_or_default();
    // O pedido de cada envio é o texto maior da spec: a lista das ondas e o
    // painel mostram o envio sem ele, e só a leitura de uma onda o traz
    // inteiro.
    let brief = matches!(query, BlockQuery::Block(Block::Waves | Block::Metrics));
    let events: Vec<String> =
        found_by(log.block(query), term, &codes).into_iter().map(|e| shown_with_code(e, &codes, brief)).collect();
    let warnings: Vec<String> = log.skipped.iter().map(|s| s.message(lang)).collect();
    Ok(render(&spec, block, &events, &warnings))
}

/// O bloco `lessons`: fora da spec, no banco do projeto. `--term` é o número
/// da lição no banco — não uma busca, como no resto dos blocos —, porque é
/// assim que o pedido de uma onda a leva, sem copiar o texto dela. Sem
/// número, ou sem lição vigente com esse número, a lista vem vazia; sem
/// banco no disco, o mesmo.
fn read_lessons(root: &Path, spec: Option<&str>, term: Option<&str>, lang: Locale) -> Result<String, Value> {
    let refuse = move |refusal: Refusal| super::refused(&refusal, lang);
    let paths = ClaudePaths::for_project(root).map_err(|e| refuse(Refusal::Io { detail: e.to_string() }))?;
    let bank = mustard_core::io::lessons::read(&paths.lessons_path()).map_err(refuse)?.unwrap_or_default();
    let wanted = term.and_then(|t| t.trim().parse::<u64>().ok());
    let events: Vec<String> = kept(&bank)
        .into_iter()
        .filter(|lesson| wanted.is_some_and(|id| lesson.id == id))
        .map(|lesson| shown_line(&lesson.fields))
        .collect();
    Ok(render(spec.unwrap_or_default(), "lessons", &events, &[]))
}

/// O checkout em que o comando roda, cuja branch diz qual é a spec atual.
pub(crate) fn checkout(start: &Path) -> PathBuf {
    let start = std::path::absolute(start).unwrap_or_else(|_| start.to_path_buf());
    mustard_core::io::workspace::workspace_root_or_self(&start)
}

/// A linha como a leitura mostra, com o código do item (`MSTD-<sigla>-<NNNN>`),
/// que é o jeito de citá-lo e o endereço dele na página: o gravado na linha
/// ou, numa linha sem código, o que a leitura dá a ela. Com `brief`, o envio
/// sai sem o pedido (`text`).
fn shown_with_code(event: &SpecEvent, codes: &BTreeMap<u64, String>, brief: bool) -> String {
    let mut fields = event.fields.clone();
    if let Some(code) = codes.get(&event.id) {
        fields.insert("code".into(), Value::String(code.clone()));
    }
    if brief && event.event_type == "send" {
        fields.remove("text");
    }
    shown_line(&fields)
}

fn render(spec: &str, block: &str, events: &[String], warnings: &[String]) -> String {
    let mut out = format!(
        "{{\"ok\":true,\"spec\":{},\"block\":{},\"count\":{},\"events\":[",
        json!(spec),
        json!(block),
        events.len()
    );
    for (i, event) in events.iter().enumerate() {
        out.push_str(if i == 0 { "\n" } else { ",\n" });
        out.push_str(event);
    }
    if !events.is_empty() {
        out.push('\n');
    }
    out.push(']');
    if !warnings.is_empty() {
        out.push_str(",\"warnings\":");
        out.push_str(&json!(warnings).to_string());
    }
    out.push('}');
    out
}

/// Run `read` and print the block; exit 1 on a refusal.
pub fn run(opts: &ReadOpts) {
    match read_at(opts) {
        Ok(report) => println!("{report}"),
        Err(refusal) => {
            println!("{}", serde_json::to_string_pretty(&refusal).unwrap_or_else(|_| "{}".into()));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{seed_at, WriteOpts};
    use tempfile::tempdir;

    fn opts(root: &std::path::Path, block: &str, term: Option<&str>) -> ReadOpts {
        ReadOpts {
            root: root.to_path_buf(),
            spec: Some("teste".into()),
            block: block.into(),
            term: term.map(str::to_string),
        }
    }

    fn without_spec(root: &std::path::Path, block: &str) -> ReadOpts {
        ReadOpts { spec: None, ..opts(root, block, None) }
    }

    fn put(root: &std::path::Path, event_type: &str, fields: Value) -> u64 {
        // A spec já aberta: o arquivo de eventos existe antes da gravação,
        // como o comando que abre a spec o deixa.
        let path = store::spec_file(root, "teste").expect("spec file");
        if !path.exists() {
            std::fs::create_dir_all(path.parent().expect("spec folder")).expect("spec folder");
            std::fs::File::create(&path).expect("the event file");
        }
        let out = seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("teste".into()),
            event_type: event_type.into(),
            json: fields.to_string(),
        });
        assert_eq!(out["ok"], json!(true), "{out}");
        out["id"].as_u64().unwrap()
    }

    fn events(report: &str) -> Vec<Value> {
        let parsed: Value = serde_json::from_str(report).expect("the report is JSON");
        parsed["events"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn reading_one_wave_brings_only_that_wave_and_never_the_search_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        let c1 = put(root, "criterion", json!({"when": "a", "then": "b", "proof": "p", "form": "ubiquitous", "origin": said}));
        let c2 = put(root, "criterion", json!({"when": "c", "then": "d", "proof": "q", "form": "ubiquitous", "origin": said}));
        put(root, "wave", json!({"n": 1, "text": "Um.", "criteria": [c1], "done_when": "x", "origin": said}));
        put(root, "task", json!({"wave": 1, "text": "T1.", "files": [{"path": "a.rs"}], "depends_on": [], "origin": said}));
        put(root, "wave", json!({"n": 2, "text": "Dois.", "criteria": [c2], "done_when": "y", "origin": said}));
        put(root, "task", json!({"wave": 2, "text": "T2.", "files": [{"path": "b.rs"}], "depends_on": [], "origin": said}));

        let report = read_at(&opts(root, "wave-2", None)).unwrap();
        let got = events(&report);
        assert_eq!(got.len(), 2, "{report}");
        assert_eq!(got[0]["code"], json!("MSTD-WAVE-0002"), "each event carries its code");
        assert_eq!(got[1]["code"], json!("MSTD-TASK-0002"));
        for event in &got {
            let wave = event.get("n").or_else(|| event.get("wave")).and_then(Value::as_u64);
            assert_eq!(wave, Some(2), "{event}");
        }
        assert!(!report.contains("\"search\""), "{report}");
    }

    #[test]
    fn a_term_filters_the_conversation() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "apagando a pasta"}));
        put(root, "message", json!({"author": "user", "text": "outro assunto"}));
        let got = events(&read_at(&opts(root, "conversation", Some("apagar"))).unwrap());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["text"], json!("apagando a pasta"));
    }

    /// Uma frase inteira não exige que o item tenha todas as palavras: a
    /// leitura devolve o que a nota acha, do mais forte para o menos forte,
    /// onde a exigência de todas as palavras não devolvia nada.
    #[test]
    fn a_whole_phrase_brings_the_strongest_items_first_instead_of_nothing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        let put_rule = |text: &str, key: &str| {
            put(root, "rule", json!({"text": text, "keys": [key], "example": "e", "origin": said}))
        };
        let weak = put_rule("A página mostra o commit da onda.", "página");
        let strong = put_rule("A rodada formata só os arquivos da rodada antes do commit.", "formatador");
        put_rule("O levantamento pergunta o tipo de trabalho.", "levantamento");

        let phrase = "a rodada formata os arquivos dela antes do commit da onda";
        let got = events(&read_at(&opts(root, "agreed", Some(phrase))).unwrap());
        assert!(!got.is_empty(), "the phrase finds something: {got:?}");
        let found: Vec<u64> = got.iter().filter_map(|e| e["id"].as_u64()).collect();
        assert_eq!(found.first(), Some(&strong), "the strongest comes first: {got:?}");
        assert!(found.contains(&weak), "a partial match still comes: {got:?}");
    }

    #[test]
    fn a_term_that_is_an_item_code_finds_that_item() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = put(root, "message", json!({"author": "user", "text": "o pedido"}));
        put(root, "criterion", json!({"when": "a", "then": "b", "proof": "p", "keys": ["C-1"], "form": "ubiquitous", "origin": said}));
        put(root, "criterion", json!({"when": "c", "then": "d", "proof": "q", "keys": ["C-2"], "form": "ubiquitous", "origin": said}));
        let got = events(&read_at(&opts(root, "criteria", Some("MSTD-CRIT-0002"))).unwrap());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["code"], json!("MSTD-CRIT-0002"));
        assert_eq!(got[0]["when"], json!("c"));
    }

    /// Uma lição gravada no banco no disco (`.claude/spec/lessons.ndjson`).
    fn write_lesson(bank: &std::path::Path, text: &str, keys: &[&str]) -> u64 {
        let draft = json!({"class": "defect", "text": text, "keys": keys,
            "applies_to": {"files": ["**"]}, "found_in": {"source": "apps/rt/CLAUDE.md"}});
        let Value::Object(draft) = draft else { unreachable!() };
        mustard_core::io::lessons::write(bank, draft, None).expect("a lição entra no banco").id
    }

    /// O bloco `lessons` devolve a lição pelo número dela no banco, fora da
    /// spec: `--term` é o número, não uma busca — mesmo um termo que casa o
    /// texto de outra lição não a traz, porque não é pelo texto que a lição
    /// se acha aqui.
    #[test]
    fn the_lessons_block_returns_a_lesson_by_its_number_not_by_search() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let bank = mustard_core::ClaudePaths::for_project(root).unwrap().lessons_path();
        let id = write_lesson(&bank, "Nunca comitar sem rodar a suíte inteira.", &["suíte", "commit"]);
        write_lesson(&bank, "Outra lição qualquer, sem relação com a suíte.", &["outra"]);

        let got = events(&read_at(&opts(root, "lessons", Some(&id.to_string()))).unwrap());
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["id"], json!(id));
        assert_eq!(got[0]["text"], json!("Nunca comitar sem rodar a suíte inteira."));

        // "suíte" acha as duas lições por texto; pelo número, não acha
        // nenhuma — a leitura de `lessons` nunca é busca por palavra.
        let by_word = events(&read_at(&opts(root, "lessons", Some("suíte"))).unwrap());
        assert!(by_word.is_empty(), "{by_word:?}");
    }

    #[test]
    fn an_unknown_block_and_a_spec_without_file_are_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let unknown = read_at(&opts(root, "everything", None)).unwrap_err();
        assert_eq!(unknown["reason"], json!("unknown-block"));
        assert!(unknown["hint"].as_str().unwrap().contains("everything"));
        let missing = read_at(&opts(root, "state", None)).unwrap_err();
        assert_eq!(missing["reason"], json!("no-spec-file"));
    }

    #[test]
    fn a_broken_line_shows_as_a_warning() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        let path = root.join(".claude").join("spec").join("teste").join("spec.ndjson");
        let mut raw = std::fs::read_to_string(&path).unwrap();
        raw.push_str("{quebrada\n");
        std::fs::write(&path, raw).unwrap();
        let report = read_at(&opts(root, "conversation", None)).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["count"], json!(1));
        assert_eq!(parsed["warnings"].as_array().map(Vec::len), Some(1), "{report}");
    }

    #[test]
    fn read_without_spec_reads_the_spec_of_the_current_branch() {
        // Uma sobreposição herdada responde primeiro, por desenho: o teste
        // pula, em vez de depender do shell que roda a suíte.
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        crate::shared::spec_state::stand_on_spec_branch(root, "teste");
        // Uma sessão ligada a outra spec não vence a branch.
        crate::shared::context::session::bind_session_spec(root.to_str().unwrap(), "s-leitura", "outra");

        let report = read_for(&without_spec(root, "conversation"), Some("s-leitura")).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["spec"], json!("teste"), "the resolved name is shown: {report}");
        assert_eq!(parsed["count"], json!(1), "{report}");
    }

    #[test]
    fn read_without_spec_reads_the_spec_bound_to_the_session() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        put(root, "message", json!({"author": "user", "text": "oi"}));
        crate::shared::context::session::bind_session_spec(root.to_str().unwrap(), "s-leitura", "teste");

        let report = read_for(&without_spec(root, "conversation"), Some("s-leitura")).unwrap();
        let parsed: Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["spec"], json!("teste"), "{report}");
    }

    #[test]
    fn read_without_spec_and_without_a_current_spec_is_refused_in_both_languages() {
        if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() {
            return;
        }
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".claude")).unwrap();

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"pt-BR"}}"#).unwrap();
        let pt = read_for(&without_spec(root, "state"), None).unwrap_err();
        assert_eq!(pt["ok"], json!(false));
        assert_eq!(pt["reason"], json!("no-current-spec"));
        assert!(pt["hint"].as_str().unwrap().starts_with("Nenhuma spec atual"), "{pt}");

        std::fs::write(root.join("mustard.json"), r#"{"language":{"text":"en-US"}}"#).unwrap();
        let en = read_for(&without_spec(root, "state"), None).unwrap_err();
        assert_eq!(en["reason"], json!("no-current-spec"));
        assert!(en["hint"].as_str().unwrap().starts_with("No current spec"), "{en}");
    }
}
