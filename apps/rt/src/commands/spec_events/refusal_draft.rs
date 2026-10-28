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
//! Só a spec aberta guarda rascunho: a gravação do modelo nunca faz uma pasta
//! de spec nascer. A lição tem banco próprio e fica de fora. Guardar é melhor
//! esforço: o disco que falha deixa a recusa sair como era, sem o número.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mustard_core::domain::lessons::LESSON;
use mustard_core::io::fs::lock::LockedFile;
use mustard_core::io::spec_events as store;
use mustard_core::platform::i18n::{Locale, translate};
use serde_json::{Map, Value, json};

/// O arquivo dos rascunhos, na pasta da spec.
const FILE: &str = "drafts.json";

/// O campo da gravação que aponta o rascunho a juntar.
const FIELD: &str = "draft";

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
    /// a spec não foi aberta.
    fn of(root: &Path, spec: &str) -> Option<Self> {
        let events = store::spec_file(root, spec).ok().filter(|events| events.is_file())?;
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
        book.drafts.insert(number, (event_type.to_string(), fields.clone()));
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

/// Tira todos os rascunhos da spec `spec` do projeto em `root`, no fechamento.
pub(crate) fn clear(root: &Path, spec: &str) {
    if let Some(place) = Place::of(root, spec) {
        let _ = std::fs::remove_file(place.file);
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
        if let Some(number) = place.keep(self.used, &self.event_type, &self.fields) {
            report["draft"] = json!(number);
            report["draft_hint"] = json!(translate("spec_events.draft_kept", self.lang).replace("{draft}", &number.to_string()));
        }
        report
    }
}
