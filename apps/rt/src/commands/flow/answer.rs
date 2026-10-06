//! `mustard-rt run answer` — grava de uma vez a resposta de um ponto do
//! levantamento: o item da resposta, o ponto que fecha o aberto e o passo
//! seguinte.
//!
//! O ponto é o de `--point`, pelo número ou pelo código, ou, sem ele, o
//! primeiro ponto aberto. A resposta vem num item novo (`--type` com
//! `--json`, nos mesmos campos do `run write`), em itens já gravados
//! (`--result`, números ou códigos separados por vírgula) ou nos dois. O ponto
//! que não se aplica fecha com `--not-applicable` e o motivo em `--reason`,
//! sem item. O item sem `origin` vem da última mensagem do usuário, e o
//! fechamento aponta a mesma mensagem que o item.
//!
//! Tudo acontece sob a trava do arquivo de eventos da spec, nesta ordem: a
//! conferência de que o ponto está aberto, a gravação do item pela mesma
//! porta do `run write`, com toda regra e recusa dele, e a gravação do ponto
//! que fecha o aberto. O item recusado não fecha o ponto, e o fechamento
//! recusado tira o item do arquivo: um nunca fica sem o outro.
//!
//! A saída traz o item gravado, o fechamento e o passo seguinte do
//! levantamento, como o `run write` o mostra. No levantamento condensado,
//! `points` traz só o código, o número e a lacuna de cada ponto aberto, e
//! `point` traz o próximo inteiro:
//!
//! ```text
//! {"ok": true, "spec": "x", "id": 12, "type": "decision", "code": "MSTD-DEC-0003",
//!  "closed": {"id": 13, "code": "MSTD-POINT-0007", "closes": 5, "status": "closed", "result": [12], …},
//!  "next": "Apresente o ponto MSTD-POINT-0003 …", "point": {"id": 6, …}}
//! ```
//!
//! Recusa sai com `ok: false`, a razão curta em `reason` e a mensagem no
//! idioma do projeto em `hint`, e nada fica gravado.

use std::path::PathBuf;

use mustard_core::domain::spec_events::{EventRef, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::spec_state::SpecState;
use mustard_core::domain::{spec_index, survey};
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use crate::commands::spec_events::{self, shown, write};
use crate::shared::spec_state::{checkout, session_from_env, DiskSpecState};

/// Options for `mustard-rt run answer`.
pub struct AnswerOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec do levantamento; sem ela, a spec atual.
    pub spec: Option<String>,
    /// O ponto respondido, pelo número ou pelo código; sem ele, o primeiro
    /// aberto.
    pub point: Option<String>,
    /// O tipo do item da resposta, que vem junto com `json`.
    pub event_type: Option<String>,
    /// Os campos do item da resposta, num objeto JSON, como no `run write`.
    pub json: Option<String>,
    /// Os itens já gravados que respondem o ponto, por número ou código,
    /// separados por vírgula.
    pub result: Option<String>,
    /// O ponto não se aplica: fecha sem item, com o motivo.
    pub not_applicable: bool,
    /// O motivo do fechamento.
    pub reason: Option<String>,
}

/// As recusas do `answer`: as dele e as da leitura e da gravação da spec.
enum AnswerRefusal {
    /// Nem item, nem itens gravados, nem "não se aplica"; ou o tipo sem os
    /// campos, ou os campos sem o tipo.
    Options,
    /// "Não se aplica" junto de uma resposta.
    NotApplicableMixed,
    NoOpenPoint { spec: String },
    Spec(Refusal),
}

impl From<Refusal> for AnswerRefusal {
    fn from(refusal: Refusal) -> Self {
        Self::Spec(refusal)
    }
}

impl AnswerRefusal {
    /// A razão curta, estável, para quem lê a saída por máquina.
    fn reason(&self) -> &'static str {
        match self {
            Self::Options => "answer-options",
            Self::NotApplicableMixed => "answer-not-applicable-mixed",
            Self::NoOpenPoint { .. } => "no-open-point",
            Self::Spec(refusal) => refusal.reason(),
        }
    }

    /// A mensagem exata, no idioma pedido.
    fn message(&self, lang: Locale) -> String {
        match self {
            Self::Options => translate("survey.answer_options", lang).to_string(),
            Self::NotApplicableMixed => translate("survey.answer_not_applicable_mixed", lang).to_string(),
            Self::NoOpenPoint { spec } => translate("survey.answer_no_open_point", lang).replace("{spec}", spec),
            Self::Spec(refusal) => refusal.message(lang),
        }
    }
}

/// A resposta que os argumentos pedem.
struct Asked {
    /// O tipo e os campos do item novo.
    item: Option<(String, Map<String, Value>)>,
    /// Os itens já gravados, como vieram: número ou código.
    result: Vec<String>,
    reason: Option<String>,
    not_applicable: bool,
}

/// O núcleo testável do comando. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn answer_at(opts: &AnswerOpts) -> Value {
    answer_for(opts, session_from_env().as_deref())
}

/// [`answer_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn answer_for(opts: &AnswerOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    match answer_in(&project, opts, session) {
        Ok(report) => report,
        Err(refusal) => json!({ "ok": false, "reason": refusal.reason(), "hint": refusal.message(project.lang) }),
    }
}

/// A resposta inteira: os argumentos, a spec e as duas gravações sob a trava.
fn answer_in(
    project: &spec_events::Project,
    opts: &AnswerOpts,
    session: Option<&str>,
) -> Result<Value, AnswerRefusal> {
    let mut asked = asked(opts)?;
    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => DiskSpecState::new(&checkout(&opts.root)).active(session).ok_or(Refusal::NoCurrentSpec)?,
    };
    if let Some((event_type, draft)) = asked.item.as_mut() {
        write::draft_rules(&opts.root, &spec, event_type, draft, false)?;
    }
    let path = store::spec_file(&project.root, &spec)?;
    let answered =
        store::with_locked_writer(&path, |locked| answer_locked(locked, &spec, opts, asked, project.lang))?;
    let report = answered.ok_or_else(|| Refusal::NoSpecFile { spec: spec.clone() })??;
    // O item e o fechamento entram no bloco das specs do mapa. A gravação na
    // spec já está feita: a falha no mapa não a desfaz, e a próxima resposta
    // do mapa tenta de novo.
    let _ = mustard_core::io::map_specs::sync_spec(&project.root, &spec, &project.languages);
    Ok(report)
}

/// A resposta que os argumentos pedem, conferida antes de ler a spec: um item
/// inteiro, itens já gravados ou os dois; ou "não se aplica", sozinho.
fn asked(opts: &AnswerOpts) -> Result<Asked, AnswerRefusal> {
    let filled = |value: Option<&str>| value.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let item = match (filled(opts.event_type.as_deref()), opts.json.as_deref()) {
        (Some(event_type), Some(json)) => Some((event_type, write::draft_object(json)?)),
        (None, None) => None,
        _ => return Err(AnswerRefusal::Options),
    };
    let result: Vec<String> =
        opts.result.as_deref().unwrap_or_default().split(',').filter_map(|raw| filled(Some(raw))).collect();
    // O "não se aplica" sem motivo é recusado pela conferência do ponto que
    // fecha, a mesma do `run write`.
    if opts.not_applicable && (item.is_some() || !result.is_empty()) {
        return Err(AnswerRefusal::NotApplicableMixed);
    }
    if !opts.not_applicable && item.is_none() && result.is_empty() {
        return Err(AnswerRefusal::Options);
    }
    Ok(Asked { item, result, reason: filled(opts.reason.as_deref()), not_applicable: opts.not_applicable })
}

/// As gravações sob a trava do arquivo de eventos: confere o ponto e os itens
/// apontados, grava o item e o fechamento e monta a saída. O fechamento
/// recusado volta o arquivo ao que era antes do item.
fn answer_locked(
    locked: &mut store::LockedLog,
    spec: &str,
    opts: &AnswerOpts,
    asked: Asked,
    lang: Locale,
) -> Result<Value, AnswerRefusal> {
    let before = locked.log().clone();
    let open = survey::open_points(&before);
    let point = match opts.point.as_deref() {
        Some(raw) => chosen(&before, &open, raw)?,
        None => *open.first().ok_or_else(|| AnswerRefusal::NoOpenPoint { spec: spec.to_string() })?,
    };
    let mut result = asked.result.iter().map(|raw| recorded(&before, raw)).collect::<Result<Vec<u64>, _>>()?;
    let saved = locked.content().to_string();
    let mut report = json!({ "ok": true, "spec": spec });
    let mut origin = None;
    if let Some((event_type, draft)) = asked.item {
        let item = write::record_locked_by_model(locked, &opts.root, spec, &event_type, draft)?.written;
        origin = locked.log().get(item.id).and_then(|event| event.int("origin"));
        report["id"] = json!(item.id);
        report["type"] = json!(event_type);
        if let Some(code) = item.code {
            report["code"] = json!(code);
        }
        result.insert(0, item.id);
    }
    let draft = closing(point, &result, asked.reason, asked.not_applicable, origin);
    let closed = match write::record_locked_by_model(locked, &opts.root, spec, "point", draft) {
        Ok(closed) => closed.written,
        Err(refusal) => {
            locked.restore(saved)?;
            return Err(refusal.into());
        }
    };
    let after = locked.log();
    let codes = after.codes();
    // O fechamento sai sem a hora: a mesma resposta dá a mesma saída.
    if let Some(event) = after.get(closed.id) {
        let fields = shown(event, &codes);
        let kept = ["id", "code", "closes", "status", "result", "reason", "origin"]
            .into_iter()
            .filter_map(|field| fields.get(field).map(|value| (field.to_string(), value.clone())));
        report["closed"] = Value::Object(kept.collect());
    }
    if let Some(refusal) = &closed.index_warning {
        report["warnings"] = json!([spec_index::write_warning(refusal, lang)]);
    }
    for (key, value) in write::survey_report(spec, &before, after, lang, true).unwrap_or_default() {
        report[key.as_str()] = value;
    }
    Ok(report)
}

/// O ponto aberto que `raw` aponta, pelo número de qualquer versão dele ou
/// pelo código.
///
/// # Errors
///
/// [`Refusal::PointNotOpen`], com o código, o número e a lacuna dos pontos
/// abertos, quando `raw` não aponta um deles.
fn chosen<'a>(log: &'a SpecLog, open: &[&'a SpecEvent], raw: &str) -> Result<&'a SpecEvent, Refusal> {
    let target = reference(raw).and_then(|target| newest(log, &target));
    let current = target.and_then(|id| log.current(id));
    if let Some(point) = current.and_then(|current| open.iter().copied().find(|point| point.id == current.id)) {
        return Ok(point);
    }
    let id = match target {
        Some(id) => log.codes().get(&id).map_or_else(|| id.to_string(), |code| format!("{code} ({id})")),
        None => raw.trim().to_string(),
    };
    Err(Refusal::PointNotOpen { id, open: survey::describe(log, open) })
}

/// O número do item gravado que `raw` aponta, pelo número ou pelo código.
///
/// # Errors
///
/// [`Refusal::UnknownTarget`] quando a spec não tem o item.
fn recorded(log: &SpecLog, raw: &str) -> Result<u64, Refusal> {
    let target = reference(raw).unwrap_or_else(|| EventRef::Code(raw.trim().to_string()));
    newest(log, &target).ok_or(Refusal::UnknownTarget { target })
}

/// Como `raw` aponta um evento: um número ou um código; `None` para outra
/// coisa.
fn reference(raw: &str) -> Option<EventRef> {
    let raw = raw.trim();
    EventRef::from_value(&raw.parse::<u64>().map_or_else(|_| json!(raw), |id| json!(id)))
}

/// O número que `target` aponta no arquivo: o próprio número, quando ele
/// existe, ou o da versão mais nova do código, como a gravação resolve um
/// código.
fn newest(log: &SpecLog, target: &EventRef) -> Option<u64> {
    match target {
        EventRef::Id(id) => log.get(*id).map(|event| event.id),
        EventRef::Code(code) => {
            let codes = log.codes();
            log.events.iter().rev().find(|event| codes.get(&event.id) == Some(code)).map(|event| event.id)
        }
    }
}

/// O ponto que fecha `point`: o bloco, a lacuna e a origem do ponto, a
/// situação, os itens que respondem, o motivo e a mensagem de onde a
/// resposta veio. Sem `origin`, a gravação aponta a última mensagem do
/// usuário.
fn closing(
    point: &SpecEvent,
    result: &[u64],
    reason: Option<String>,
    not_applicable: bool,
    origin: Option<u64>,
) -> Map<String, Value> {
    let mut draft = Map::new();
    for field in ["block", "gap", "from"] {
        if let Some(value) = point.fields.get(field) {
            draft.insert(field.to_string(), value.clone());
        }
    }
    let status = if not_applicable { "not_applicable" } else { "closed" };
    draft.insert("status".to_string(), json!(status));
    draft.insert("closes".to_string(), json!(point.id));
    if !result.is_empty() {
        draft.insert("result".to_string(), json!(result));
    }
    if let Some(reason) = reason {
        draft.insert("reason".to_string(), Value::String(reason));
    }
    if let Some(origin) = origin {
        draft.insert("origin".to_string(), json!(origin));
    }
    draft
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::commands::spec_events::write::{record, record_open, seed_at, WriteOpts};
    use mustard_core::domain::spec_state::PhaseWriter;
    use tempfile::tempdir;

    const GOAL: &str = "Travar o merge enquanto houver pendência aberta.";
    const DECISION: &str = r#"{"title": "O merge espera a pendência", "text": "O merge espera a pendência fechar.",
        "agent": "- conferir pelo teste", "keys": ["merge"], "why": "O usuário pediu."}"#;

    fn write(root: &Path, event_type: &str, json: Value) -> Value {
        seed_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            event_type: event_type.into(),
            json: json.to_string(),
        })
    }

    fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("not written: {report}"))
    }

    /// Uma spec em levantamento, com o objetivo apontando a mensagem do
    /// usuário e `count` pontos abertos no bloco `block`, cada um com um fato.
    /// Devolve os números dos pontos.
    fn surveyed(root: &Path, block: &str, count: usize) -> Vec<u64> {
        assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
        let said = id_of(&write(root, "message", json!({"author": "user", "text": GOAL})));
        id_of(&write(root, "context", json!({"text": GOAL, "origin": said})));
        (1..=count)
            .map(|n| {
                let fact = json!({"text": GOAL, "source": format!("mensagem {said}")});
                let point = json!({"block": block, "gap": format!("Lacuna {n}"), "from": "gap", "status": "open",
                    "origin": said, "facts": [fact]});
                id_of(&write(root, "point", point))
            })
            .collect()
    }

    fn opts(root: &Path) -> AnswerOpts {
        AnswerOpts {
            root: root.to_path_buf(),
            spec: Some("x".into()),
            point: None,
            event_type: None,
            json: None,
            result: None,
            not_applicable: false,
            reason: None,
        }
    }

    /// A resposta num item novo, uma decisão com os campos `json`.
    fn with_item(root: &Path, json: &str) -> AnswerOpts {
        AnswerOpts { event_type: Some("decision".into()), json: Some(json.into()), ..opts(root) }
    }

    /// A resposta pelos itens já gravados `result`.
    fn with_result(root: &Path, result: &str) -> AnswerOpts {
        AnswerOpts { result: Some(result.into()), ..opts(root) }
    }

    /// Uma decisão gravada pela porta do `run write`, sem `origin`.
    fn decided(root: &Path) -> u64 {
        id_of(&write(root, "decision", json!({"text": "O merge espera.", "keys": ["merge"], "why": "w"})))
    }

    fn answer(opts: &AnswerOpts) -> Value {
        answer_for(opts, None)
    }

    fn events_path(root: &Path) -> PathBuf {
        store::spec_file(&store::spec_root(root), "x").expect("spec file")
    }

    fn log_of(root: &Path) -> SpecLog {
        store::read(&events_path(root)).expect("read").expect("the event file")
    }

    /// O arquivo de eventos e o índice das specs, byte a byte.
    fn snapshot(root: &Path) -> (Vec<u8>, Vec<u8>) {
        let events = events_path(root);
        let index = mustard_core::io::spec_index::index_for(&events).expect("the index").0;
        (std::fs::read(&events).expect("events"), std::fs::read(&index).unwrap_or_default())
    }

    fn open_ids(root: &Path) -> Vec<u64> {
        survey::open_points(&log_of(root)).iter().map(|point| point.id).collect()
    }

    /// A resposta com um item novo grava o item, fecha o ponto apontando o
    /// item e devolve o próximo ponto aberto. Sem `origin`, o item e o
    /// fechamento apontam a última mensagem do usuário.
    #[test]
    fn an_answer_records_the_item_closes_the_point_and_returns_the_next() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let points = surveyed(root, "rules", 2);
        let said = id_of(&write(root, "message", json!({"author": "user", "text": "Vale para os dois casos."})));

        let report = answer(&with_item(root, DECISION));
        let item = id_of(&report);
        assert_eq!(report["type"], json!("decision"), "{report}");
        let closed = &report["closed"];
        assert_eq!(closed["closes"], json!(points[0]), "{report}");
        assert_eq!(closed["status"], json!("closed"), "{report}");
        assert_eq!(closed["result"], json!([item]), "{report}");
        assert_eq!(closed["origin"], json!(said), "{report}");
        let log = log_of(root);
        assert_eq!(log.get(item).and_then(|event| event.int("origin")), Some(said));
        assert_eq!(report["point"]["id"], json!(points[1]), "{report}");
        let code = log.codes()[&points[1]].clone();
        assert!(report["next"].as_str().unwrap().contains(&code), "{report}");
        assert_eq!(open_ids(root), vec![points[1]]);
    }

    /// O item que a porta do `run write` recusa, pela forma ou pelo autor do
    /// programa, não fecha o ponto, e nada é gravado.
    #[test]
    fn a_refused_item_leaves_the_point_open() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let points = surveyed(root, "rules", 1);
        let before = snapshot(root);
        let untitled = r#"{"text": "O merge espera.", "agent": "- conferir", "keys": ["merge"], "why": "pedido"}"#;
        let report = answer(&with_item(root, untitled));
        assert_eq!(report["reason"], json!("item-form-missing"), "{report}");
        let mut forged: Value = serde_json::from_str(DECISION).unwrap();
        forged["author"] = json!("binary");
        let report = answer(&with_item(root, &forged.to_string()));
        assert_eq!(report["reason"], json!("binary-author"), "{report}");
        assert_eq!(snapshot(root), before, "a refusal writes nothing");
        assert_eq!(open_ids(root), points);
    }

    /// O fechamento recusado tira o item do arquivo: o ponto que o `grill`
    /// gravou sem fato não fecha com resposta, e a resposta não fica solta.
    #[test]
    fn a_refused_closing_takes_the_item_back_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root, "rules", 0);
        let mut bare = Map::new();
        for (field, value) in [("block", "rules"), ("gap", "Lacuna sem fato"), ("from", "gap"), ("status", "open")] {
            bare.insert(field.to_string(), json!(value));
        }
        bare.insert("author".to_string(), json!("binary"));
        let point = record(root, "x", "point", bare, PhaseWriter::Binary).expect("the binary records the point");
        let before = snapshot(root);

        let report = answer(&with_item(root, DECISION));
        assert_eq!(report["reason"], json!("point-without-facts"), "{report}");
        assert_eq!(snapshot(root), before, "the item went back out");
        assert_eq!(open_ids(root), vec![point.written.id]);
    }

    /// O ponto fecha com resposta ou como não se aplica. Sem resposta, com os
    /// dois juntos ou com "não se aplica" sem motivo, a recusa diz o que falta
    /// e nada é gravado; com o motivo, o ponto fecha sem item.
    #[test]
    fn a_point_closes_with_an_answer_or_as_not_applicable_with_its_reason() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let points = surveyed(root, "rules", 2);
        let before = snapshot(root);
        let cases = [
            (opts(root), "answer-options"),
            (AnswerOpts { event_type: Some("decision".into()), ..opts(root) }, "answer-options"),
            (AnswerOpts { not_applicable: true, ..with_result(root, "1") }, "answer-not-applicable-mixed"),
            (AnswerOpts { not_applicable: true, ..opts(root) }, "not-applicable-needs-reason"),
        ];
        for (asked, reason) in &cases {
            let report = answer(asked);
            assert_eq!(report["reason"], json!(reason), "{report}");
        }
        let options = answer(&cases[0].0);
        assert_eq!(options["hint"], json!(translate("survey.answer_options", Locale::PtBr)), "{options}");
        assert_eq!(snapshot(root), before, "a refusal writes nothing");

        let skipped = AnswerOpts { not_applicable: true, reason: Some("Não há regra nova.".into()), ..opts(root) };
        let report = answer(&skipped);
        assert!(report.get("id").is_none(), "no item: {report}");
        assert_eq!(report["closed"]["status"], json!("not_applicable"), "{report}");
        assert_eq!(report["closed"]["reason"], json!("Não há regra nova."), "{report}");
        assert_eq!(report["point"]["id"], json!(points[1]), "{report}");
    }

    /// A resposta por itens já gravados fecha o ponto apontado pelo código. O
    /// ponto que já fechou é recusado com a lista dos abertos, e, sem ponto
    /// aberto, a recusa diz que não há o que responder.
    #[test]
    fn a_closed_point_is_refused_with_the_open_ones_listed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let points = surveyed(root, "rules", 2);
        let decided = decided(root);
        let codes = log_of(root).codes();
        let by_code = AnswerOpts { point: Some(codes[&points[1]].clone()), ..with_result(root, &decided.to_string()) };
        let report = answer(&by_code);
        assert_eq!(report["closed"]["closes"], json!(points[1]), "{report}");
        assert_eq!(report["closed"]["result"], json!([decided]), "{report}");

        let before = snapshot(root);
        let again = answer(&AnswerOpts { point: Some(points[1].to_string()), ..by_code });
        assert_eq!(again["reason"], json!("point-not-open"), "{again}");
        let hint = again["hint"].as_str().unwrap();
        assert!(hint.contains(&format!("{} ({})", codes[&points[0]], points[0])), "{hint}");
        assert_eq!(snapshot(root), before, "a refusal writes nothing");

        let last = with_result(root, &codes[&decided]);
        assert_eq!(answer(&last)["closed"]["closes"], json!(points[0]));
        assert_eq!(answer(&last)["reason"], json!("no-open-point"));
    }

    /// No levantamento condensado, a resposta não repete o texto dos pontos
    /// abertos: `points` traz o código, o número e a lacuna de cada um, e
    /// `point` traz inteiro só o próximo. O passo seguinte manda responder
    /// cada ponto pelo mesmo comando e diz o que a resposta devolve.
    #[test]
    fn a_condensed_answer_lists_the_open_points_briefly() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let points = surveyed(root, survey::CONDENSED, 3);
        let report = answer(&with_result(root, &decided(root).to_string()));
        let log = log_of(root);
        let open = survey::open_points(&log);
        assert_eq!(report["points"], json!(survey::describe(&log, &open)), "{report}");
        assert!(!report["points"].as_str().unwrap().contains(GOAL), "no fact in the list: {report}");
        assert_eq!(report["point"]["id"], json!(points[1]), "{report}");
        assert_eq!(report["point"]["facts"][0]["text"], json!(GOAL), "{report}");
        let next = report["next"].as_str().unwrap();
        assert!(next.contains("`mustard-rt run answer --point"), "{next}");
        assert!(next.contains("o código, o número e a lacuna de cada ponto ainda aberto"), "{next}");
    }
}
