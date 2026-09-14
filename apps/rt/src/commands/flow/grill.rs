//! `mustard-rt run grill --kinds <tipos> [--spec <nome>] [--condensed]` — o
//! levantamento de uma spec: grava o tipo de trabalho e monta a lista de
//! pontos.
//!
//! Roda só numa spec em levantamento que já tem o objetivo, o primeiro
//! `context`, que é a resposta do usuário à pergunta do `open`. O tipo de
//! trabalho vem em `--kinds` (`feature`, `fix` ou `refactor`, mais de um no
//! pedido misto) e é gravado como `work_type`, com o autor `assistant` e a
//! mensagem do objetivo como origem; é a única porta que o grava. Os mesmos
//! tipos de novo não gravam nada; um tipo a mais grava a versão nova do
//! `work_type`, e a lista ganha só as lacunas que faltam; um tipo a menos é
//! recusado, porque ponto nenhum sai por isso.
//!
//! A lista (`mustard_core::domain::survey::build`) junta as lacunas dos
//! tipos, as lições do banco e as specs anteriores que casam com o objetivo e,
//! dentro desses pontos, até 3 lembretes: mensagens antigas do usuário que não
//! viraram registro. O banco, o índice, as specs anteriores e o mapa são os do
//! checkout principal, também vistos de um worktree. Com `--condensed`, o
//! pedido que cabe numa frase, todos os pontos vão para um bloco só.
//!
//! O `grill` não grava os pontos: quem os grava é o assistente, pelo `write`,
//! copiando cada item de `points`. O item já gravado traz o número, o código e
//! a situação do ponto que o registra. Enquanto falta ponto, a resposta pede
//! para gravá-los; com todos gravados, devolve em `next` o primeiro ponto
//! aberto ou, no condensado, pede para mostrar tudo de uma vez; sem ponto
//! aberto, lista em `unrouted` as mensagens do usuário sem destino. Chamar de
//! novo com os mesmos tipos não grava nada e devolve o mesmo passo.
//!
//! ```text
//! {"ok": true, "spec": "x", "work_type": 4, "kinds": ["feature"], "condensed": false,
//!  "points": [{"block": "context", "gap": "Quem usa e para quê", "from": "gap", "origin": 2}, …],
//!  "to_record": 9, "reminders": 0, "hint": "Grave cada ponto de `points` …"}
//! ```
//!
//! Recusa sai com exit 1 e `ok: false`, com a razão curta em `reason` e a
//! mensagem no idioma do projeto em `hint`.

use std::path::{Path, PathBuf};

use mustard_core::domain::spec_events::{Kind, Refusal, WORK_KINDS};
use mustard_core::domain::spec_state::{PhaseWriter, SpecState, State};
use mustard_core::domain::survey::{self, Sources};
use mustard_core::io::{lessons, project_map, spec_events as store, spec_index};
use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::ClaudePaths;
use serde_json::{json, Map, Value};

use crate::commands::spec_events::{self, read::checkout, shown, write::record};
use crate::shared::spec_state::{session_from_env, DiskSpecState};

/// Options for `mustard-rt run grill`.
pub struct GrillOpts {
    /// Qualquer pasta dentro do repositório.
    pub root: PathBuf,
    /// A spec do levantamento; sem ela, a spec atual.
    pub spec: Option<String>,
    /// Os tipos de trabalho, separados por vírgula.
    pub kinds: Option<String>,
    /// O pedido que cabe numa frase: todos os pontos de uma vez.
    pub condensed: bool,
}

/// As recusas do `grill`: as dele e as da leitura e da gravação da spec.
enum GrillRefusal {
    GoalMissing { spec: String },
    NotInSurvey { spec: String, phase: String },
    KindsMissing,
    KindsNarrowed { spec: String, recorded: String },
    Spec(Refusal),
}

impl GrillRefusal {
    /// A razão curta, estável, para quem lê a saída por máquina.
    fn reason(&self) -> &'static str {
        match self {
            Self::GoalMissing { .. } => "goal-missing",
            Self::NotInSurvey { .. } => "not-in-survey",
            Self::KindsMissing => "kinds-missing",
            Self::KindsNarrowed { .. } => "kinds-narrowed",
            Self::Spec(refusal) => refusal.reason(),
        }
    }

    /// A mensagem exata, no idioma pedido.
    fn message(&self, lang: Locale) -> String {
        let fill = |key: &str, slots: &[(&str, &str)]| {
            slots.iter().fold(translate(key, lang).to_string(), |text, (slot, value)| text.replace(slot, value))
        };
        match self {
            Self::GoalMissing { spec } => fill("grill.goal_missing", &[("{spec}", spec)]),
            Self::NotInSurvey { spec, phase } => fill("grill.not_in_survey", &[("{spec}", spec), ("{phase}", phase)]),
            Self::KindsMissing => fill("grill.kinds_missing", &[]),
            Self::KindsNarrowed { spec, recorded } => {
                fill("grill.kinds_narrowed", &[("{spec}", spec), ("{recorded}", recorded)])
            }
            Self::Spec(refusal) => refusal.message(lang),
        }
    }

    fn report(&self, lang: Locale) -> Value {
        json!({ "ok": false, "reason": self.reason(), "hint": self.message(lang) })
    }
}

/// O núcleo testável de [`run`]. A sessão vem do ambiente. Nunca entra em
/// pânico.
pub(crate) fn grill_at(opts: &GrillOpts) -> Value {
    grill_for(opts, session_from_env().as_deref())
}

/// [`grill_at`] com a sessão recebida, que é como um teste a escolhe.
pub(crate) fn grill_for(opts: &GrillOpts, session: Option<&str>) -> Value {
    let project = spec_events::project(&opts.root);
    let lang = project.lang;
    let refuse = |refusal: GrillRefusal| refusal.report(lang);

    let spec = match opts.spec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => spec.to_string(),
        None => match DiskSpecState::new(&checkout(&opts.root)).active(session) {
            Some(spec) => spec,
            None => return refuse(GrillRefusal::Spec(Refusal::NoCurrentSpec)),
        },
    };
    let path = match store::spec_file(&project.root, &spec) {
        Ok(path) => path,
        Err(refusal) => return refuse(GrillRefusal::Spec(refusal)),
    };
    let log = match store::read(&path) {
        Ok(Some(log)) => log,
        Ok(None) => return refuse(GrillRefusal::Spec(Refusal::NoSpecFile { spec })),
        Err(refusal) => return refuse(GrillRefusal::Spec(refusal)),
    };
    let phase = State::from_log(&log).phase;
    if phase != Some("survey") {
        return refuse(GrillRefusal::NotInSurvey { spec, phase: phase.unwrap_or("-").to_string() });
    }
    let Some(goal) = survey::goal(&log) else {
        return refuse(GrillRefusal::GoalMissing { spec });
    };
    let goal_text = goal.str_field("text").unwrap_or_default().trim().to_string();
    let origin = goal.int("origin");

    // Os tipos: os mesmos não gravam nada, um a mais grava a versão nova, um
    // a menos é recusado.
    let asked = match survey::parse_kinds(opts.kinds.as_deref().unwrap_or_default()) {
        Ok(kinds) if kinds.is_empty() => return refuse(GrillRefusal::KindsMissing),
        Ok(kinds) => kinds,
        Err(_) => {
            return refuse(GrillRefusal::Spec(Refusal::InvalidValue {
                event_type: "work_type".to_string(),
                field: "kinds".to_string(),
                expected: Kind::ManyOf(WORK_KINDS),
            }));
        }
    };
    let recorded = survey::work_type(&log).map(|event| (event.id, survey::kinds_of(event)));
    let (work_type, kinds) = match recorded {
        Some((_, before)) if before.iter().any(|kind| !asked.contains(kind)) => {
            return refuse(GrillRefusal::KindsNarrowed { spec, recorded: before.join(", ") });
        }
        Some((id, before)) if asked.iter().all(|kind| before.contains(kind)) => (id, before),
        recorded => match record_work_type(&opts.root, &spec, &asked, origin, recorded.map(|(id, _)| id)) {
            Ok(id) => (id, asked),
            Err(refusal) => return refuse(GrillRefusal::Spec(refusal)),
        },
    };
    let log = match store::read(&path) {
        Ok(Some(log)) => log,
        Ok(None) => return refuse(GrillRefusal::Spec(Refusal::NoSpecFile { spec })),
        Err(refusal) => return refuse(GrillRefusal::Spec(refusal)),
    };

    // A lista, do banco, do índice, das specs e do mapa do checkout
    // principal.
    let lessons_path = ClaudePaths::for_project(&project.root).ok().map(|paths| paths.lessons_path());
    let bank = lessons_path.as_deref().and_then(|path| lessons::read(path).ok().flatten());
    let lessons_file = lessons_path
        .as_deref()
        .and_then(|path| path.strip_prefix(&project.root).ok())
        .map_or_else(|| ".claude/spec/lessons.ndjson".to_string(), |rel| rel.to_string_lossy().replace('\\', "/"));
    let index = spec_index::read(&project.root);
    let prior = spec_index::read_specs(&project.root);
    let map = project_map::read(&project.root).ok();
    let condensed = opts.condensed || survey::condensed(&log);
    let list = survey::build(&Sources {
        kinds: &kinds,
        goal: &goal_text,
        current: &spec,
        bank: bank.as_ref(),
        lessons_file: &lessons_file,
        index: &index,
        prior: &prior,
        map: map.as_ref(),
        condensed,
        lang,
    });

    let codes = log.codes();
    let to_record = survey::missing(&log, &list).len();
    let points: Vec<Value> = list
        .iter()
        .map(|item| {
            let mut value = item.to_value(origin);
            // O número e a situação saem da leitura única dos pontos, a mesma
            // da página e da passagem para o plano.
            if let Some((point, status)) = item.standing(&log) {
                value["id"] = json!(point.id);
                if let Some(code) = codes.get(&point.id) {
                    value["code"] = json!(code);
                }
                value["status"] = json!(status);
            }
            value
        })
        .collect();
    let reminders: usize = list.iter().map(|item| item.reminders.len()).sum();
    let mut report = json!({
        "ok": true,
        "spec": spec,
        "work_type": work_type,
        "kinds": kinds,
        "condensed": condensed,
        "points": points,
        "to_record": to_record,
        "reminders": reminders,
    });
    let open = survey::open_points(&log);
    if to_record > 0 {
        report["hint"] = json!(translate("survey.record_points", lang).replace("{spec}", &spec));
    } else if let Some(first) = open.first() {
        if condensed {
            report["hint"] = json!(translate("survey.present_all", lang));
        } else {
            let code = codes.get(&first.id).cloned().unwrap_or_default();
            report["next"] = shown(first, &codes);
            report["hint"] = json!(translate("survey.present_point", lang)
                .replace("{code}", &code)
                .replace("{id}", &first.id.to_string()));
        }
    } else {
        let unrouted: Vec<Value> = survey::unrouted_messages(&log).into_iter().map(|m| shown(m, &codes)).collect();
        report["unrouted"] = json!(unrouted);
        report["hint"] = json!(translate("survey.done", lang));
    }
    report
}

/// Grava o tipo de trabalho, com a mensagem do objetivo como origem, pela
/// mesma gravação do `run write`; `replaces` quando é a versão nova de um
/// tipo já gravado. Devolve o número gravado.
fn record_work_type(
    start: &Path,
    spec: &str,
    kinds: &[&str],
    origin: Option<u64>,
    replaces: Option<u64>,
) -> Result<u64, Refusal> {
    let mut draft = Map::new();
    draft.insert("kinds".to_string(), json!(kinds));
    draft.insert("author".to_string(), json!("assistant"));
    if let Some(origin) = origin {
        draft.insert("origin".to_string(), json!(origin));
    }
    if let Some(id) = replaces {
        draft.insert("replaces".to_string(), json!(id));
    }
    record(start, spec, "work_type", draft, PhaseWriter::Binary).map(|recorded| recorded.written.id)
}

/// Run `grill` and print the JSON report; exit 1 on a refusal.
pub fn run(opts: &GrillOpts) {
    let report = grill_at(opts);
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into()));
    if report["ok"] != json!(true) {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::{record_birth, record_open, write_at, WriteOpts};
    use std::process::Command;
    use tempfile::tempdir;

    const GOAL: &str = "Travar o merge enquanto houver pendência aberta.";
    const LESSON: &str = "**Merge com pendência.** O merge não passa com pendência aberta.";
    const EN: &str = r#"{"language":{"text":"en-US"}}"#;

    fn write(root: &Path, spec: Option<&str>, event_type: &str, json: Value) -> Value {
        write_at(&WriteOpts {
            root: root.to_path_buf(),
            spec: spec.map(str::to_string),
            event_type: event_type.into(),
            json: json.to_string(),
        })
    }

    fn id_of(report: &Value) -> u64 {
        report["id"].as_u64().unwrap_or_else(|| panic!("not written: {report}"))
    }

    /// Uma spec em levantamento, com o objetivo gravado palavra por palavra.
    /// Devolve o número da mensagem do objetivo.
    fn surveyed(root: &Path, spec: &str) -> u64 {
        assert_eq!(record_open(root, spec, &format!("feature/{spec}"), "dev"), Ok(true));
        let said = id_of(&write(root, Some(spec), "message", json!({"author": "user", "text": GOAL})));
        id_of(&write(root, Some(spec), "context", json!({"text": GOAL, "origin": said})));
        said
    }

    fn grill(root: &Path, spec: &str, kinds: Option<&str>, condensed: bool) -> Value {
        let opts = GrillOpts {
            root: root.to_path_buf(),
            spec: Some(spec.to_string()),
            kinds: kinds.map(str::to_string),
            condensed,
        };
        grill_for(&opts, None)
    }

    fn events(root: &Path, spec: &str) -> String {
        std::fs::read_to_string(root.join(".claude").join("spec").join(spec).join("spec.ndjson")).unwrap_or_default()
    }

    fn items(report: &Value) -> &Vec<Value> {
        report["points"].as_array().unwrap_or_else(|| panic!("no points: {report}"))
    }

    /// Grava, como o assistente, cada item da lista que ainda não tem ponto:
    /// os campos como vieram, aberto, e um fato com a fonte quando a lista não
    /// trouxe nenhum.
    fn record_list(root: &Path, spec: &str, report: &Value) {
        for item in items(report) {
            if item.get("id").is_some() {
                continue;
            }
            let mut point = item.clone();
            point["status"] = json!("open");
            if point.get("facts").is_none() {
                point["facts"] = json!([{"text": GOAL, "source": format!("mensagem {}", item["origin"])}]);
            }
            let written = write(root, Some(spec), "point", point);
            assert_eq!(written["ok"], json!(true), "{written}");
        }
    }

    /// Uma lição que casa com o objetivo no banco, e uma spec anterior que
    /// casa, com uma regra, uma decisão e uma mensagem do usuário sem registro.
    fn lesson_and_prior_spec(root: &Path) {
        let lesson = write(
            root,
            None,
            "lesson",
            json!({"class": "defect", "text": LESSON, "keys": ["merge", "pendência"],
                   "applies_to": {"files": ["**"]}, "found_in": {"spec": "antiga"}}),
        );
        assert_eq!(lesson["ok"], json!(true), "{lesson}");
        let old = Some("antiga");
        let said = id_of(&write(root, old, "message", json!({"author": "user", "text": "Travar o merge com pendência aberta."})));
        id_of(&write(root, old, "context", json!({"text": "Travar o merge com pendência aberta.", "origin": said})));
        id_of(&write(
            root,
            old,
            "rule",
            json!({"text": "**Merge travado.** Pendência aberta barra o merge.", "keys": ["merge"], "example": "e", "origin": said}),
        ));
        id_of(&write(
            root,
            old,
            "decision",
            json!({"text": "A cobrança da pendência sai no merge.", "keys": ["pendência"], "why": "w", "origin": said}),
        ));
        id_of(&write(root, old, "message", json!({"author": "user", "text": "O merge com pendência aberta passou sem aviso."})));
    }

    /// O `grill` grava o tipo de trabalho, com o autor e a origem do
    /// objetivo, e lista um ponto por lacuna, na ordem dos blocos; gravados os
    /// pontos, devolve o primeiro aberto.
    #[test]
    fn grill_records_the_work_type_and_lists_one_point_per_gap_in_block_order() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let report = grill(root, "x", Some("feature"), false);
        assert_eq!(report["ok"], json!(true), "{report}");
        let log = mustard_core::domain::spec_events::parse_log(&events(root, "x"));
        let work_type = survey::work_type(&log).expect("the work type is recorded");
        assert_eq!(report["work_type"], json!(work_type.id));
        assert_eq!(survey::kinds_of(work_type), ["feature"]);
        assert_eq!(work_type.str_field("author"), Some("assistant"));
        assert_eq!(work_type.int("origin"), Some(said));

        let gaps: Vec<&str> = items(&report).iter().map(|item| item["gap"].as_str().unwrap()).collect();
        let expected: Vec<&str> =
            survey::gaps(&["feature"]).into_iter().map(|key| key.label(Locale::PtBr)).collect();
        assert_eq!(gaps, expected);
        assert!(items(&report).iter().all(|item| item["from"] == json!("gap") && item["origin"] == json!(said)));
        assert_eq!(report["to_record"], json!(9));
        assert!(report["hint"].as_str().unwrap().contains("mustard-rt run write point --spec x"), "{report}");
        assert!(!events(root, "x").contains("\"type\":\"point\""), "the grill never writes a point");

        record_list(root, "x", &report);
        let next = grill(root, "x", Some("feature"), false);
        assert_eq!(next["to_record"], json!(0), "{next}");
        assert_eq!(next["next"]["gap"], json!("Quem usa e para quê"), "{next}");
        assert_eq!(next["next"]["block"], json!("context"));
        let hint = next["hint"].as_str().unwrap();
        let code = next["next"]["code"].as_str().unwrap();
        assert!(hint.contains(code) && hint.contains(&next["next"]["id"].to_string()), "{hint}");
    }

    /// Repetir o `grill` com os mesmos tipos não grava nada e devolve o mesmo
    /// próximo ponto.
    #[test]
    fn running_grill_again_writes_nothing_and_returns_the_same_next_point() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root, "x");
        let first = grill(root, "x", Some("fix"), false);
        let bytes = events(root, "x");
        assert_eq!(grill(root, "x", Some("fix"), false), first, "the list is the same");
        assert_eq!(events(root, "x"), bytes);
        record_list(root, "x", &first);
        let bytes = events(root, "x");
        let one = grill(root, "x", Some("fix"), false);
        let two = grill(root, "x", Some("fix"), false);
        assert_eq!(one["next"], two["next"]);
        assert_eq!(one["next"]["gap"], json!("O sintoma"));
        assert_eq!(events(root, "x"), bytes, "nothing was written");
    }

    /// Um tipo a mais grava a versão nova do tipo de trabalho, e a lista
    /// ganha só as lacunas que faltam.
    #[test]
    fn a_wider_work_type_adds_only_the_missing_gaps() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root, "x");
        let first = grill(root, "x", Some("feature"), false);
        record_list(root, "x", &first);
        let wider = grill(root, "x", Some("fix,feature"), false);
        assert_eq!(wider["kinds"], json!(["feature", "fix"]), "{wider}");
        let log = mustard_core::domain::spec_events::parse_log(&events(root, "x"));
        let now = survey::work_type(&log).unwrap();
        assert_eq!(now.int("replaces"), first["work_type"].as_u64());
        let left: Vec<&str> = items(&wider)
            .iter()
            .filter(|item| item.get("id").is_none())
            .map(|item| item["gap"].as_str().unwrap())
            .collect();
        assert_eq!(left, ["O sintoma", "Como reproduzir", "O esperado contra o obtido", "A causa"]);
        assert_eq!(wider["to_record"], json!(4));
    }

    /// Um tipo a menos é recusado, e nada é gravado.
    #[test]
    fn a_narrower_work_type_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root, "x");
        assert_eq!(grill(root, "x", Some("feature,fix"), false)["ok"], json!(true));
        let bytes = events(root, "x");
        let narrower = grill(root, "x", Some("feature"), false);
        assert_eq!(narrower["reason"], json!("kinds-narrowed"), "{narrower}");
        assert!(narrower["hint"].as_str().unwrap().contains("feature, fix"), "{narrower}");
        assert_eq!(events(root, "x"), bytes);
    }

    /// Sem os tipos, ou com um tipo que não existe, o `grill` recusa, e nada
    /// é gravado.
    #[test]
    fn grill_without_kinds_or_with_an_unknown_kind_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root, "x");
        let bytes = events(root, "x");
        assert_eq!(grill(root, "x", None, false)["reason"], json!("kinds-missing"));
        assert_eq!(grill(root, "x", Some(" , "), false)["reason"], json!("kinds-missing"));
        let unknown = grill(root, "x", Some("feature,bug"), false);
        assert_eq!(unknown["reason"], json!("invalid-value"), "{unknown}");
        assert!(unknown["hint"].as_str().unwrap().contains("refactor"), "{unknown}");
        assert_eq!(events(root, "x"), bytes);
    }

    /// Sem o objetivo, o `grill` recusa nos dois idiomas, com a pergunta a
    /// fazer, e nada é gravado.
    #[test]
    fn grill_without_a_goal_is_refused_in_both_languages() {
        for (config, question) in [(None, "Qual o objetivo, numa frase?"), (Some(EN), "What is the goal, in one sentence?")] {
            let dir = tempdir().unwrap();
            let root = dir.path();
            if let Some(config) = config {
                std::fs::write(root.join("mustard.json"), config).unwrap();
            }
            assert_eq!(record_open(root, "x", "feature/x", "dev"), Ok(true));
            let bytes = events(root, "x");
            let report = grill(root, "x", Some("feature"), false);
            assert_eq!(report["reason"], json!("goal-missing"), "{report}");
            assert!(report["hint"].as_str().unwrap().contains(question), "{report}");
            assert_eq!(events(root, "x"), bytes);
        }
    }

    /// Fora do levantamento o `grill` recusa: na spec em plano, na aprovada e
    /// na que não tem estado.
    #[test]
    fn grill_outside_the_survey_is_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert_eq!(record_birth(root, "plano", None), Ok(true));
        assert_eq!(record_birth(root, "aprovada", None), Ok(true));
        let mut witness = Map::new();
        witness.insert("phase".into(), json!("approved"));
        witness.insert("witness".into(), json!({"question": "Aprovar a spec?", "answer": "Aprovar"}));
        assert!(record(root, "aprovada", "state", witness, PhaseWriter::Witness).is_ok());
        id_of(&write(root, Some("sem-estado"), "message", json!({"author": "user", "text": GOAL})));
        for (spec, phase) in [("plano", "plan"), ("aprovada", "approved"), ("sem-estado", "-")] {
            let bytes = events(root, spec);
            let report = grill(root, spec, Some("feature"), false);
            assert_eq!(report["reason"], json!("not-in-survey"), "{spec}: {report}");
            assert!(report["hint"].as_str().unwrap().contains(&format!("fase {phase}")), "{report}");
            assert_eq!(events(root, spec), bytes);
        }
    }

    /// No pedido que cabe numa frase, o `grill` devolve todas as lacunas de
    /// uma vez, num bloco só; gravados os pontos, pede um sim só.
    #[test]
    fn a_condensed_survey_returns_every_gap_at_once_in_one_block() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root, "x");
        let report = grill(root, "x", Some("feature,fix"), true);
        assert_eq!(report["condensed"], json!(true), "{report}");
        assert_eq!(items(&report).len(), 13);
        assert!(items(&report).iter().all(|item| item["block"] == json!(survey::CONDENSED)), "{report}");
        record_list(root, "x", &report);
        let all = grill(root, "x", Some("feature,fix"), false);
        assert_eq!(all["condensed"], json!(true), "the recorded points keep the survey condensed");
        assert!(all.get("next").is_none(), "{all}");
        assert_eq!(all["hint"], json!(translate("survey.present_all", Locale::PtBr)));
        assert!(items(&all).iter().all(|item| item["status"] == json!("open")));
    }

    /// A lição que casa vira item da lista, com o texto original como fato e
    /// a linha do banco como fonte, nunca o `search`; o ponto gravado com essa
    /// fonte passa na conferência das citações.
    #[test]
    fn a_matching_lesson_becomes_a_point_with_its_original_text_never_the_search_field() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        lesson_and_prior_spec(root);
        surveyed(root, "x");
        let report = grill(root, "x", Some("fix"), false);
        let lesson = items(&report).iter().find(|item| item["from"] == json!("lesson")).expect("a lesson point");
        assert_eq!(lesson["block"], json!("lessons"));
        assert_eq!(lesson["gap"], json!("Merge com pendência."));
        assert_eq!(lesson["facts"], json!([{"text": LESSON, "source": ".claude/spec/lessons.ndjson:1"}]));
        let bank = std::fs::read_to_string(root.join(".claude").join("spec").join("lessons.ndjson")).unwrap();
        let search = serde_json::from_str::<Value>(bank.lines().next().unwrap()).unwrap()["search"].as_str().unwrap().to_string();
        assert!(!report.to_string().contains(&search), "the search field never shows: {report}");
        record_list(root, "x", &report);
    }

    /// A spec anterior que casa vira item da lista, com as regras e as
    /// decisões que casam como fatos, e a lista a grava; a mensagem sem
    /// registro dela vira lembrete dentro de um ponto.
    #[test]
    fn a_matching_prior_spec_becomes_a_point_with_its_rules_and_decisions_as_facts() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        lesson_and_prior_spec(root);
        surveyed(root, "x");
        let report = grill(root, "x", Some("fix"), false);
        let prior = items(&report).iter().find(|item| item["from"] == json!("prior_spec")).expect("a prior spec point");
        assert_eq!(prior["block"], json!("prior_specs"));
        assert_eq!(prior["gap"], json!("antiga: Travar o merge com pendência aberta."));
        let facts = prior["facts"].as_array().unwrap();
        assert!(facts.contains(&json!({"text": "Merge travado.", "source": "mustard-rt run read agreed --spec antiga --term MSTD-RULE-0001"})), "{facts:?}");
        assert!(facts.iter().any(|fact| fact["text"] == json!("A cobrança da pendência sai no merge.")), "{facts:?}");
        assert_eq!(report["reminders"], json!(1));
        let reminders: Vec<&Value> = items(&report).iter().filter_map(|item| item.get("reminders")).flat_map(|r| r.as_array().unwrap()).collect();
        assert_eq!(reminders, [&json!({"spec": "antiga", "message": 5, "text": "O merge com pendência aberta passou sem aviso."})]);
        record_list(root, "x", &report);
    }

    /// A spec do levantamento nunca é a própria spec anterior, nem dá
    /// lembrete a si mesma.
    #[test]
    fn the_current_spec_is_never_its_own_prior_spec() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root, "x");
        id_of(&write(root, Some("x"), "message", json!({"author": "user", "text": "O merge com pendência aberta travou."})));
        let report = grill(root, "x", Some("fix"), false);
        assert!(items(&report).iter().all(|item| item["from"] == json!("gap")), "{report}");
        assert_eq!(report["reminders"], json!(0));
    }

    /// Sem banco de lições e sem outras specs, a lista traz só as lacunas.
    #[test]
    fn grill_without_lessons_or_index_lists_only_the_gaps() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        surveyed(root, "x");
        std::fs::remove_file(root.join(".claude").join("spec").join("index.ndjson")).unwrap();
        let report = grill(root, "x", Some("refactor"), false);
        assert_eq!(report["ok"], json!(true), "{report}");
        assert_eq!(items(&report).len(), 9);
        assert!(items(&report).iter().all(|item| item["from"] == json!("gap") && item.get("facts").is_none()));
        assert_eq!(report["reminders"], json!(0));
    }

    /// Com todos os pontos fechados, o `grill` não tem ponto a devolver e
    /// lista as mensagens do usuário que nenhum registro aponta.
    #[test]
    fn with_every_point_closed_grill_lists_the_messages_without_destination() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        let report = grill(root, "x", Some("fix"), false);
        record_list(root, "x", &report);
        let listed = grill(root, "x", Some("fix"), false);
        for item in items(&listed) {
            let closing = json!({"block": item["block"], "gap": item["gap"], "from": "gap", "status": "not_applicable",
                "closes": item["id"], "reason": "não se aplica", "origin": said});
            assert_eq!(write(root, Some("x"), "point", closing)["ok"], json!(true));
        }
        let loose = id_of(&write(root, Some("x"), "message", json!({"author": "user", "text": "E o painel?"})));
        let done = grill(root, "x", Some("fix"), false);
        assert!(done.get("next").is_none(), "{done}");
        assert_eq!(done["hint"], json!(translate("survey.done", Locale::PtBr)));
        let unrouted: Vec<u64> = done["unrouted"].as_array().unwrap().iter().map(|m| m["id"].as_u64().unwrap()).collect();
        assert_eq!(unrouted, [loose]);
    }

    /// Lado a lado: a página e o `grill` contam pela mesma leitura dos pontos
    /// abertos. O ponto revisto e fechado pela primeira versão, e o fechado
    /// por um ponto que grava outro texto na lacuna, saem fechados nos dois;
    /// com todos fechados, a página não mostra nenhum pendente e o `grill`
    /// passa ao fim.
    #[test]
    fn the_page_and_grill_count_the_same_open_points() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        record_list(root, "x", &grill(root, "x", Some("fix"), false));
        let listed = grill(root, "x", Some("fix"), false);
        let ids: Vec<u64> = items(&listed).iter().map(|item| item["id"].as_u64().unwrap()).collect();
        let close = |item: &Value, closes: u64| {
            let closing = json!({"block": item["block"], "gap": item["gap"], "from": "gap", "status": "not_applicable",
                "closes": closes, "reason": "não se aplica", "origin": said});
            assert_eq!(write(root, Some("x"), "point", closing)["ok"], json!(true));
        };
        let first = &items(&listed)[0];
        let revision = json!({"block": first["block"], "gap": first["gap"], "from": "gap", "status": "open",
            "replaces": ids[0], "origin": said, "facts": [{"text": "Revisto.", "source": format!("mensagem {said}")}]});
        assert_eq!(write(root, Some("x"), "point", revision)["ok"], json!(true));
        close(first, ids[0]);

        let page = || std::fs::read_to_string(root.join(".claude").join("spec").join("x").join("spec.html")).unwrap();
        let panel = |open: usize, closed: usize| {
            translate("page.metrics.points.value", Locale::PtBr)
                .replace("{open}", &open.to_string())
                .replace("{closed}", &closed.to_string())
        };
        let log = mustard_core::domain::spec_events::parse_log(&events(root, "x"));
        assert_eq!(survey::open_points(&log).len(), ids.len() - 1);
        assert!(page().contains(&panel(ids.len() - 1, 1)), "the revised point closed by its first number: {}", page());
        let next = grill(root, "x", Some("fix"), false);
        assert_eq!(next["next"]["id"], json!(ids[1]), "{next}");

        let second = &items(&listed)[1];
        let reworded = json!({"block": second["block"], "gap": "Outro texto na lacuna", "from": "gap",
            "status": "not_applicable", "closes": ids[1], "reason": "não se aplica", "origin": said});
        assert_eq!(write(root, Some("x"), "point", reworded)["ok"], json!(true));
        assert!(page().contains(&panel(ids.len() - 2, 2)), "{}", page());
        let after = grill(root, "x", Some("fix"), false);
        let open = items(&after).iter().filter(|item| item["status"] == json!("open")).count();
        assert_eq!(open, ids.len() - 2, "{after}");
        assert_eq!(items(&after)[1]["status"], json!("closed"), "{after}");
        assert_eq!(items(&after)[1]["id"], json!(ids[1]), "{after}");
        assert_eq!(after["next"]["id"], json!(ids[2]), "{after}");

        for (item, id) in items(&listed).iter().zip(&ids).skip(2) {
            close(item, *id);
        }
        assert!(page().contains(&panel(0, ids.len())), "{}", page());
        let done = grill(root, "x", Some("fix"), false);
        assert!(done.get("next").is_none() && done["unrouted"].is_array(), "{done}");
    }

    /// O segredo de um ponto sai assim: o ponto fecha, "não se aplica" com o
    /// motivo, e só depois o texto original é apagado. A lacuna continua
    /// coberta: o `grill` e a página contam o ponto como fechado, e a
    /// passagem para o plano não a pede de novo.
    #[test]
    fn a_point_closed_and_then_purged_still_covers_its_gap() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let said = surveyed(root, "x");
        record_list(root, "x", &grill(root, "x", Some("fix"), false));
        let listed = grill(root, "x", Some("fix"), false);
        let ids: Vec<u64> = items(&listed).iter().map(|item| item["id"].as_u64().unwrap()).collect();
        for (item, id) in items(&listed).iter().zip(&ids) {
            let closing = json!({"block": item["block"], "gap": item["gap"], "from": "gap", "status": "not_applicable",
                "closes": id, "reason": "O fato tinha um segredo.", "origin": said});
            assert_eq!(write(root, Some("x"), "point", closing)["ok"], json!(true));
        }
        let purged = write(root, Some("x"), "purge", json!({"targets": [ids[0]], "reason": "secret", "origin": said}));
        assert_eq!(purged["purged"], json!([ids[0]]), "{purged}");

        let after = grill(root, "x", Some("fix"), false);
        assert_eq!(after["to_record"], json!(0), "{after}");
        assert!(items(&after).iter().all(|item| item["status"] == json!("closed")), "{after}");
        let page = std::fs::read_to_string(root.join(".claude").join("spec").join("x").join("spec.html")).unwrap();
        let panel = translate("page.metrics.points.value", Locale::PtBr)
            .replace("{open}", "0")
            .replace("{closed}", &ids.len().to_string());
        assert!(page.contains(&panel), "{page}");

        let mut plan = Map::new();
        plan.insert("phase".into(), json!("plan"));
        plan.insert("author".into(), json!("binary"));
        assert!(record(root, "x", "state", plan, PhaseWriter::Binary).is_ok(), "the gap stays covered");
    }

    /// Lado a lado, a leitura única dos pontos: cada ponto é o par do
    /// original com o fechamento, e a página, o `grill`, o passo do `write` e
    /// a passagem para o plano o leem igual. Fechado com outro texto na
    /// lacuna, fechado e depois apagado, fechado e depois removido, e as duas
    /// coisas juntas: a lacuna segue coberta e o ponto conta como fechado. O
    /// fechamento grava a lacuna do original, e o `grill` mostra o número do
    /// original enquanto ele existe e, depois que ele sai, o do fechamento.
    #[test]
    fn a_closed_point_counts_the_same_on_the_page_grill_write_and_passage() {
        let cases = [(true, None), (false, Some("purge")), (false, Some("remove")), (true, Some("purge")), (true, Some("remove"))];
        for (reworded, leaves) in cases {
            let case = format!("reworded: {reworded}, original: {leaves:?}");
            let dir = tempdir().unwrap();
            let root = dir.path();
            let said = surveyed(root, "x");
            record_list(root, "x", &grill(root, "x", Some("fix"), false));
            let listed = grill(root, "x", Some("fix"), false);
            let ids: Vec<u64> = items(&listed).iter().map(|item| item["id"].as_u64().unwrap()).collect();
            let close = |item: &Value, closes: u64, gap: &str| {
                let closing = json!({"block": item["block"], "gap": gap, "from": "gap", "status": "not_applicable",
                    "closes": closes, "reason": "O fato tinha um segredo.", "origin": said});
                write(root, Some("x"), "point", closing)
            };
            let page = || std::fs::read_to_string(root.join(".claude").join("spec").join("x").join("spec.html")).unwrap();
            let panel = |open: usize, closed: usize| {
                translate("page.metrics.points.value", Locale::PtBr)
                    .replace("{open}", &open.to_string())
                    .replace("{closed}", &closed.to_string())
            };

            let first = &items(&listed)[0];
            let gap = first["gap"].as_str().unwrap();
            let mut last = close(first, ids[0], if reworded { "Outro texto na lacuna" } else { gap });
            let closing = id_of(&last);
            let log = mustard_core::domain::spec_events::parse_log(&events(root, "x"));
            assert_eq!(log.get(closing).and_then(|e| e.str_field("gap")), Some(gap), "{case}: the closing carries the gap");
            if let Some(kind) = leaves {
                let reason = if kind == "purge" { "secret" } else { "O fato tinha um segredo." };
                last = write(root, Some("x"), kind, json!({"targets": [ids[0]], "reason": reason}));
                assert_eq!(last["ok"], json!(true), "{case}: {last}");
            }

            let asked = write(root, Some("x"), "message", json!({"author": "user", "text": "E agora?"}));
            for report in [&last, &asked] {
                assert!(report.get("points").is_none(), "{case}: the gap is not asked again: {report}");
                assert_eq!(report["point"]["id"], json!(ids[1]), "{case}: {report}");
            }
            let after = grill(root, "x", Some("fix"), false);
            assert_eq!(after["to_record"], json!(0), "{case}: {after}");
            assert_eq!(items(&after)[0]["status"], json!("closed"), "{case}: {after}");
            let shown = if leaves.is_some() { closing } else { ids[0] };
            assert_eq!(items(&after)[0]["id"], json!(shown), "{case}: {after}");
            let open = items(&after).iter().filter(|item| item["status"] == json!("open")).count();
            assert_eq!(open, ids.len() - 1, "{case}: {after}");
            assert_eq!(after["next"]["id"], json!(ids[1]), "{case}: {after}");
            assert!(page().contains(&panel(ids.len() - 1, 1)), "{case}: {}", page());

            for (item, id) in items(&listed).iter().zip(&ids).skip(1) {
                assert_eq!(close(item, *id, item["gap"].as_str().unwrap())["ok"], json!(true), "{case}");
            }
            assert!(page().contains(&panel(0, ids.len())), "{case}: {}", page());
            let mut plan = Map::new();
            plan.insert("phase".into(), json!("plan"));
            plan.insert("author".into(), json!("binary"));
            assert!(record(root, "x", "state", plan, PhaseWriter::Binary).is_ok(), "{case}: the gap stays covered");
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let out = Command::new("git").args(args).current_dir(root).output().expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// Rodado de um worktree, o `grill` lê o banco, o índice e as specs do
    /// checkout principal, e o tipo de trabalho vai para a spec de lá.
    #[test]
    fn grill_from_a_linked_worktree_reads_the_bank_and_the_index_of_the_main_checkout() {
        let dir = tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "t"]);
        git(&main, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(main.join(".git").join("info").join("exclude"), ".claude/\nmustard.json\n").unwrap();
        std::fs::write(main.join("mustard.json"), "{}").unwrap();
        std::fs::write(main.join("README.md"), "oi\n").unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-q", "-m", "init"]);
        let worktree = dir.path().join("wt");
        git(&main, &["worktree", "add", "-q", "-b", "scratch", &worktree.to_string_lossy()]);

        lesson_and_prior_spec(&main);
        surveyed(&worktree, "x");
        let report = grill(&worktree, "x", Some("fix"), false);
        assert_eq!(report["ok"], json!(true), "{report}");
        let from: Vec<&str> = items(&report).iter().map(|item| item["from"].as_str().unwrap()).collect();
        assert!(from.contains(&"lesson") && from.contains(&"prior_spec"), "{report}");
        assert!(events(&main, "x").contains("\"type\":\"work_type\""));
        assert!(!worktree.join(".claude").exists(), "nothing of the Mustard inside the worktree");
        record_list(&worktree, "x", &report);
    }
}
