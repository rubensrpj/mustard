//! O rascunho que a recusa do `run write` guarda.
//!
//! A gravação recusada, sozinha ou numa linha do `--batch`, fica guardada num
//! arquivo próprio da pasta da spec (`drafts.json`), fora do arquivo de
//! eventos, e a recusa devolve o número dela em `draft`. Quem corrige grava de
//! novo com `{"draft": <n>, …}` mandando só o campo errado: os campos do
//! rascunho e os mandados se juntam, o mandado vence, e a gravação passa pela
//! conferência inteira. A recusa de novo atualiza o mesmo rascunho e devolve o
//! mesmo número; a gravação que passa o tira. A dica de como corrigir vai em
//! `draft_hint`, e a `hint` da recusa fica como era. O número é sequencial por spec e
//! não volta a ser dado enquanto a spec corre, e o que sobrar sai no
//! fechamento ([`clear`]).
//!
//! Só a spec aberta e que ainda não fechou guarda rascunho: a gravação do
//! modelo nunca faz uma pasta de spec nascer, e o fechamento, que tira os que
//! sobram, já passou pela fechada. A recusa que campo nenhum conserta — a do
//! tipo, a do autor e a do lugar de onde se grava ([`NOT_BY_A_FIELD`]) — não
//! guarda. O rascunho nunca guarda segredo: o trecho com cara de segredo sai
//! como no expurgo. A lição tem banco próprio e fica de fora. Guardar é melhor
//! esforço: o disco que falha deixa a recusa sair como era, sem o número.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::domain::lessons::LESSON;
use mustard_core::domain::spec_state::{State, not_closed_yet};
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

use crate::shared::secret::value_without_secrets;

/// O arquivo dos rascunhos, na pasta da spec.
const FILE: &str = "drafts.json";

/// O campo da gravação que aponta o rascunho a juntar.
const FIELD: &str = "draft";

/// As razões das recusas que campo nenhum conserta: vêm do tipo, do autor ou
/// do lugar de onde se grava, e não do conteúdo do item. O rascunho junta
/// campos e não troca o tipo nem tira um campo, então a correção por ele
/// cairia na mesma recusa.
const NOT_BY_A_FIELD: &[&str] =
    &["unknown-type", "state-by-flow-only", "work-type-by-grill", "binary-only-type", "wave-by-backlog", "binary-author", "user-message-by-hook"];

/// Os rascunhos guardados de uma spec e o último número dado: tipo e campos
/// de cada um.
struct Book {
    next: u64,
    drafts: BTreeMap<u64, (String, Map<String, Value>)>,
}

impl Book {
    /// O livro lido do texto do arquivo; o texto vazio ou que não se lê é um
    /// livro vazio.
    fn parse(text: &str) -> Self {
        let root = serde_json::from_str::<Value>(text).ok();
        let next = root.as_ref().and_then(|root| root.get("next")).and_then(Value::as_u64).unwrap_or(0);
        let drafts = root
            .as_ref()
            .and_then(|root| root.get("drafts"))
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(number, kept)| {
                let fields = kept.get("fields")?.as_object()?.clone();
                Some((number.parse().ok()?, (kept.get("type")?.as_str()?.to_string(), fields)))
            })
            .collect();
        Self { next, drafts }
    }

    /// O texto que o arquivo passa a ter.
    fn render(&self) -> String {
        let drafts: Map<String, Value> = self.drafts.iter().map(|(number, (event_type, fields))| (number.to_string(), json!({ "type": event_type, "fields": fields }))).collect();
        json!({ "next": self.next, "drafts": drafts }).to_string()
    }

    /// O número seguinte, que nunca repete um dado antes nem um guardado.
    fn number(&mut self) -> u64 {
        self.next = self.next.max(self.drafts.keys().max().copied().unwrap_or(0)) + 1;
        self.next
    }
}

/// O arquivo dos rascunhos de uma spec aberta.
struct Place {
    file: PathBuf,
}

impl Place {
    /// O lugar dos rascunhos da spec `spec` do projeto em `root`; `None` quando
    /// a spec não foi aberta ou já fechou.
    fn of(root: &Path, spec: &str) -> Option<Self> {
        let events = store::spec_file(root, spec).ok().filter(|events| events.is_file())?;
        let log = store::read(&events).ok().flatten()?;
        State::from_log(&log).phase.is_none_or(not_closed_yet).then_some(())?;
        Some(Self { file: events.parent()?.join(FILE) })
    }

    /// O tipo e os campos do rascunho `number`, quando ele está guardado.
    fn get(&self, number: u64) -> Option<(String, Map<String, Value>)> {
        let mut locked = LockedFile::existing(&self.file).ok()?;
        Book::parse(&locked.read_to_string().ok()?).drafts.remove(&number)
    }

    /// Guarda os campos `fields` do tipo `event_type`: no rascunho `used`, que
    /// segue com o mesmo número, ou num rascunho novo. Devolve o número.
    fn keep(&self, used: Option<u64>, event_type: &str, fields: &Map<String, Value>) -> Option<u64> {
        let mut locked = LockedFile::exclusive(&self.file).ok()?;
        let mut book = Book::parse(&locked.read_to_string().ok()?);
        let number = used.filter(|number| book.drafts.contains_key(number)).unwrap_or_else(|| book.number());
        let Value::Object(kept) = value_without_secrets(&Value::Object(fields.clone())) else { return None };
        book.drafts.insert(number, (event_type.to_string(), kept));
        locked.replace(book.render().as_bytes()).ok()?;
        Some(number)
    }

    /// Tira o rascunho `number`, usado por uma gravação que passou.
    fn forget(&self, number: u64) {
        let Ok(mut locked) = LockedFile::existing(&self.file) else { return };
        let Ok(text) = locked.read_to_string() else { return };
        let mut book = Book::parse(&text);
        if book.drafts.remove(&number).is_some() {
            let _ = locked.replace(book.render().as_bytes());
        }
    }
}

/// Tira todos os rascunhos da spec `spec` do projeto em `root`, no fechamento,
/// já com a spec fechada.
pub(crate) fn clear(root: &Path, spec: &str) {
    if let Some(folder) = store::spec_file(root, spec).ok().as_deref().and_then(Path::parent) {
        let _ = std::fs::remove_file(folder.join(FILE));
    }
}

/// A resposta de uma recusa do rascunho, no formato das recusas da porta.
fn refusal(reason: &str, hint: String) -> Value {
    json!({ "ok": false, "reason": reason, "hint": hint })
}

/// Uma gravação com o rascunho que ela cita já juntado, à espera do resultado.
pub(crate) struct Resolved {
    /// O `--json` da gravação, com o rascunho juntado e sem o campo `draft`.
    json: String,
    /// O rascunho que a gravação usa.
    used: Option<u64>,
    /// Onde guardar; `None` sem spec aberta.
    place: Option<Place>,
    event_type: String,
    /// Os campos da gravação como o modelo os mandou, com o rascunho juntado.
    fields: Map<String, Value>,
    lang: Locale,
}

impl Resolved {
    /// A gravação do tipo `event_type` com os campos `json` na spec `spec`, ou
    /// na da conversa da sessão `session` quando não vem spec, juntada ao
    /// rascunho que `json` cita em `draft`.
    ///
    /// # Errors
    ///
    /// A recusa de um `draft` que a spec não guarda, por já ter sido usado ou
    /// por ser de outro tipo; nada é gravado.
    pub(crate) fn of(root: &Path, spec: Option<&str>, session: Option<&str>, event_type: &str, json: &str) -> Result<Self, Value> {
        let project = super::project(root);
        let (lang, event_type) = (project.lang, event_type.trim());
        let plain = |place: Option<Place>| Self { json: json.to_string(), used: None, place, event_type: event_type.to_string(), fields: Map::new(), lang };
        let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(json) else { return Ok(plain(None)) };
        if event_type == LESSON {
            return Ok(plain(None));
        }
        let spec = spec.map(str::to_string).or_else(|| super::session_spec::of_conversation(&project.root, session, event_type).ok());
        let place = spec.as_deref().and_then(|spec| Place::of(&project.root, spec));
        let Some(asked) = fields.remove(FIELD) else {
            return Ok(Self { fields, ..plain(place) });
        };
        let shown = asked.as_str().map_or_else(|| asked.to_string(), str::to_string);
        let Some((number, (kept_type, kept_fields))) = asked.as_u64().and_then(|number| Some((number, place.as_ref()?.get(number)?))) else {
            return Err(refusal("draft-unknown", translate("spec_events.draft_unknown", lang).replace("{draft}", &shown)));
        };
        if kept_type != event_type {
            let hint = translate("spec_events.draft_other_type", lang).replace("{draft}", &shown).replace("{found}", &kept_type).replace("{type}", event_type);
            return Err(refusal("draft-other-type", hint));
        }
        let mut joined = kept_fields;
        joined.extend(fields);
        Ok(Self { json: Value::Object(joined.clone()).to_string(), used: Some(number), fields: joined, ..plain(place) })
    }

    /// O `--json` da gravação, com o rascunho juntado.
    pub(crate) fn json(&self) -> &str {
        &self.json
    }

    /// A resposta `report` da gravação: a que passou tira o rascunho que usou;
    /// a recusada o guarda, ou o atualiza, e devolve o número dele em `draft`
    /// e, em `draft_hint`, como corrigir. A dica da recusa fica como era.
    pub(crate) fn settle(self, mut report: Value) -> Value {
        let Some(place) = &self.place else { return report };
        if report["ok"] == json!(true) {
            if let Some(number) = self.used {
                place.forget(number);
            }
            return report;
        }
        if report["reason"].as_str().is_some_and(|reason| NOT_BY_A_FIELD.contains(&reason)) {
            return report;
        }
        if let Some(number) = place.keep(self.used, &self.event_type, &self.fields) {
            report["draft"] = json!(number);
            report["draft_hint"] = json!(translate("spec_events.draft_kept", self.lang).replace("{draft}", &number.to_string()));
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_events::write::WriteOpts;
    use crate::commands::spec_events::write_door::write_triaged;
    use crate::shared::jev::reserve::Asked;
    use crate::shared::spec_state::seed_event;
    use tempfile::tempdir;

    /// A gravação do modelo pela porta, vista de `start`, na spec `teste`.
    fn by_model(start: &Path, event_type: &str, body: &Value) -> Value {
        let opts = WriteOpts { root: start.to_path_buf(), spec: Some("teste".into()), event_type: event_type.into(), json: body.to_string() };
        write_triaged(&opts, false, None, &|_, _, _| Asked::Off)
    }

    /// O arquivo dos rascunhos da spec `teste` do projeto em `root`.
    fn drafts(root: &Path) -> PathBuf {
        root.join(".claude").join("spec").join("teste").join(FILE)
    }

    /// A recusa do tipo, do autor ou do lugar de onde se grava não guarda
    /// rascunho nem dá a dica dele: corrigir um campo não a tira.
    #[test]
    fn a_refusal_no_field_fixes_keeps_no_draft() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let said = seed_event(root, "teste", "message", json!({"author": "user", "text": "o plano"}));
        let copy = root.join("copias").join("a");
        std::fs::create_dir_all(&copy).unwrap();
        seed_event(root, "teste", "send", json!({"author": "binary", "wave": 1, "role": "wave", "text": "pedido", "lines": 1,
            "chars": 6, "mustard": "0", "copy": mustard_core::io::wave_prompt::shown(&copy)}));
        let task = json!({"title": "Entregar o total", "text": "Somar.", "agent": "- somar", "files": [], "depends_on": [], "covers": [said], "origin": said});
        let cases = [
            (root, "wave", json!({"n": 2, "text": "Dois.", "done_when": "x", "origin": said}), "wave-by-backlog"),
            (root, "state", json!({"phase": "running"}), "state-by-flow-only"),
            (root, "send", json!({"wave": 2, "text": "pedido"}), "binary-only-type"),
            (root, "note", json!({"author": "binary", "title": "Anotar", "text": "Anotar."}), "binary-author"),
            (root, "message", json!({"author": "user", "text": "oi"}), "user-message-by-hook"),
            (root, "inventado", json!({"text": "x"}), "unknown-type"),
            (copy.as_path(), "task", task, "wave-by-backlog"),
        ];
        for (start, event_type, body, reason) in cases {
            let refused = by_model(start, event_type, &body);
            assert_eq!(refused["reason"], json!(reason), "{event_type}: {refused}");
            assert!(refused.get("draft").is_none() && refused.get("draft_hint").is_none(), "{event_type}: {refused}");
            assert!(!drafts(root).exists(), "{event_type}: nada ficou guardado");
        }
    }

    /// O rascunho da recusa guarda o texto sem o segredo, como o expurgo o
    /// deixaria, e a correção por ele grava o item já sem o segredo.
    #[test]
    fn a_kept_draft_never_holds_a_secret() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("mustard.json"), "{}").unwrap();
        let key = format!("ghp_{}", "a1".repeat(18));
        let said = seed_event(root, "teste", "message", json!({"author": "user", "text": "Quero a conferência de cada critério."}));
        let crit = seed_event(root, "teste", "criterion", json!({"when": "a obra fecha", "then": "cada critério é conferido", "proof": "git --version", "form": "ubiquitous"}));
        let refused = by_model(root, "task", &json!({"title": "Fechamento confere cada critério", "text": "Conferir.",
            "agent": format!("- conferir com o token {key}"), "depends_on": [], "covers": [crit], "origin": said}));
        let draft = refused["draft"].as_u64().unwrap_or_else(|| panic!("a recusa guarda o rascunho: {refused}"));
        let kept = std::fs::read_to_string(drafts(root)).unwrap();
        assert!(!kept.contains(&key) && kept.contains(mustard_core::domain::spec_events::PURGED_MARK), "{kept}");

        let fixed = by_model(root, "task", &json!({"draft": draft, "files": []}));
        let id = fixed["id"].as_u64().unwrap_or_else(|| panic!("a correção grava: {fixed}"));
        let log = store::read(&store::spec_file(root, "teste").unwrap()).unwrap().unwrap();
        let agent = log.get(id).and_then(|task| task.str_field("agent")).unwrap_or_default().to_string();
        assert!(!agent.contains(&key), "{agent}");
    }
}
