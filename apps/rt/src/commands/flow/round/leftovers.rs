//! As sobras de uma volta: o que o agente de onda viu e deixou de fora da
//! entrega, no campo `leftovers`. Cada uma diz o que é, no campo `kind`, e a
//! rodada a grava sem perguntar quando a spec sabe o destino: a que quebra
//! vira tarefa da spec, no backlog; a cosmética vira pendência já passada ao
//! projeto, na lista de depois. Só a sem `kind` vira pendência da spec com a
//! pergunta de destino ao usuário.

use std::path::Path;

use mustard_core::domain::project_map::cited_paths;
use mustard_core::domain::spec_events::{Kind, Refusal, SpecLog};
use mustard_core::io::wave_prompt;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::report::{own_copy_relative, WaveReport};
use crate::commands::event::pending::{hand_to_project, pending_at, PendingOpts};

/// Os valores do campo `kind` de uma sobra; outro valor é recusado na
/// gravação da volta.
const LEFTOVER_KINDS: &[&str] = &["breaks", "cosmetic"];

/// O que a sobra é, no campo `kind` da volta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeftoverKind {
    /// Algo deixa de funcionar sem ela: vira tarefa da spec, no backlog.
    Breaks,
    /// Nada quebra: vira pendência já passada ao projeto, na lista de depois.
    Cosmetic,
}

/// Uma sobra da volta. Sem `kind`, a spec não sabe classificá-la, e ela vira
/// pendência da spec com a pergunta de destino ao usuário.
pub(crate) struct Leftover {
    pub title: String,
    pub detail: String,
    pub kind: Option<LeftoverKind>,
}

/// As sobras da lista `leftovers` da volta, na ordem. A sobra sem título ou
/// sem detalhe fica de fora — a gravação já a recusa —, e o `kind` que não é
/// `breaks` nem `cosmetic` é recusado, com a posição da sobra.
pub(super) fn leftovers_of(items: &[Value]) -> Result<Vec<Leftover>, Refusal> {
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(|t| t.trim().to_string());
    let mut leftovers = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let (Some(title), Some(detail)) = (field(item, "title"), field(item, "detail")) else { continue };
        let kind = match item.get("kind") {
            None | Some(Value::Null) => None,
            Some(Value::String(k)) if k.trim() == "breaks" => Some(LeftoverKind::Breaks),
            Some(Value::String(k)) if k.trim() == "cosmetic" => Some(LeftoverKind::Cosmetic),
            Some(_) => {
                return Err(Refusal::InvalidValue {
                    event_type: "delivered".into(),
                    field: format!("leftovers[{}].kind", i + 1),
                    expected: Kind::OneOf(LEFTOVER_KINDS),
                });
            }
        };
        leftovers.push(Leftover { title, detail, kind });
    }
    Ok(leftovers)
}

/// A tarefa que a sobra que quebra vira: o título e o detalhe dela, sem
/// dependência, com o autor da onda, e os arquivos que o detalhe cita entre
/// crases e que existem no repositório ou na cópia da onda — o caminho de
/// dentro da cópia vira o relativo ao repositório, e o de fora dos dois não
/// entra.
pub(super) fn leftover_task(root: &Path, log: &SpecLog, wave: u64, leftover: &Leftover) -> Map<String, Value> {
    let copy = wave_prompt::recorded_copy(log, wave).map(|copy| Path::new(&copy.path).to_path_buf());
    let mut files: Vec<String> = Vec::new();
    for cited in cited_paths(&leftover.detail) {
        let path = own_copy_relative(log, wave, &cited);
        let on_disk = root.join(&path).exists() || copy.as_ref().is_some_and(|copy| copy.join(&path).exists());
        if !Path::new(&path).is_absolute() && on_disk && !files.contains(&path) {
            files.push(path);
        }
    }
    let files: Vec<Value> = files.into_iter().map(|path| json!({ "path": path })).collect();
    let task = json!({
        "title": leftover.title,
        "text": leftover.detail,
        "files": files,
        "depends_on": [],
        "author": "wave",
    });
    task.as_object().cloned().unwrap_or_default()
}

/// Cada sobra das voltas assumidas que não quebra nada vira pendência, pela
/// mesma porta do `pending --add` — a que quebra já virou tarefa no backlog.
/// A cosmética passa ao projeto na hora, com o motivo que diz a onda que a
/// apontou, e fica na lista de depois sem pergunta; a sem `kind` fica da
/// spec, com a pergunta de destino. A que repete o título de uma pendência
/// aberta devolve a que já está aberta, sem duplicar. A que a lista recusa
/// por outro motivo vira aviso, e a entrega segue gravada. Devolve o que foi
/// aberto, com a pergunta de destino de cada pendência nova da spec, e os
/// avisos.
pub(super) fn open_leftovers(
    start: &Path,
    spec: &str,
    waves: &[WaveReport],
    lang: Locale,
) -> (Vec<Value>, Vec<Value>) {
    let mut opened = Vec::new();
    let mut warnings = Vec::new();
    for wave in waves {
        for leftover in wave.leftovers.iter().filter(|l| l.kind != Some(LeftoverKind::Breaks)) {
            let out = pending_at(&PendingOpts {
                root: start.to_path_buf(),
                add: true,
                title: Some(leftover.title.clone()),
                detail: Some(leftover.detail.clone()),
                ..PendingOpts::default()
            });
            let mut entry = json!({ "wave": wave.wave, "type": "pending", "id": out["id"] });
            if out["ok"] == json!(true) {
                // A cosmética que a lista não consegue passar ao projeto fica
                // da spec, e a pergunta de destino volta a valer para ela.
                let later = (leftover.kind == Some(LeftoverKind::Cosmetic))
                    .then(|| translate("round.leftover_cosmetic", lang).replace("{wave}", &wave.wave.to_string()));
                let handed = match (&later, out["id"].as_str()) {
                    (Some(reason), Some(id)) => hand_to_project(start, id, spec, Some(reason)),
                    _ => false,
                };
                if handed {
                    entry["owner"] = json!("project");
                    entry["later"] = json!(later);
                } else if let Some(question) = out.get("pending_question") {
                    entry["question"] = question.clone();
                }
                opened.push(entry);
            } else if out["reason"] == json!("duplicate") {
                entry["open"] = json!(true);
                opened.push(entry);
            } else {
                warnings.push(json!({ "reason": "leftover-not-opened", "wave": wave.wave, "hint": out["hint"] }));
            }
        }
    }
    (opened, warnings)
}
