//! O que a versão nova de uma tarefa herda da versão que ela substitui.
//!
//! Quem revê uma tarefa manda só o que muda: os outros campos do modelo
//! (`title`, `text`, `agent`, `files`, `depends_on`, `covers`, `skill`,
//! `must_read` e os demais que o tipo declara) vêm da versão vigente, e a
//! conferência da gravação olha a tarefa já inteira. Campo mandado vence o da
//! versão antiga, vazio inclusive: `"files": []` limpa a lista.
//!
//! Os campos do programa não se herdam por aqui. O envelope (`v`, `id`, `at`,
//! `code`, `author`, `session`) e o `changed` da leitura nem são campos do
//! tipo; a onda segue a regra própria de [`super::task_wave::inherit_wave`], que
//! só a mantém enquanto o agente dela trabalha; e `blocked_by` e `retry_guard`
//! são travas que o programa põe e tira, e a versão nova do modelo as solta.
//!
//! A versão que o programa grava leva só o título: o resto ele já manda por
//! inteiro, e a falta de um campo ali é de propósito, como a onda que volta ao
//! backlog.

use mustard_core::domain::spec_events::{SpecLog, referenced_id, type_spec};
use serde_json::{Map, Value};

/// Os campos da tarefa que o programa decide, e que a versão nova não herda.
const PROGRAM_FIELDS: &[&str] = &["wave", "blocked_by", "retry_guard"];

/// Completa o rascunho `draft` de uma versão nova de tarefa com os campos da
/// versão que `replaces` aponta no arquivo lido em `log`. Com `by_model`, é a
/// versão do modelo: herda todo campo do tipo que não mandou, menos os do
/// programa; sem ele, só o título que faltar. O rascunho sem `replaces`, ou
/// que aponta o que não é tarefa ou o que o arquivo não tem, sai como entrou:
/// a conferência da gravação recusa o que faltar.
pub(super) fn inherit_task(log: &SpecLog, by_model: bool, draft: &mut Map<String, Value>) {
    let Some(old) = referenced_id(log, "task", draft, "replaces").and_then(|id| log.get(id)).filter(|old| old.event_type == "task") else {
        return;
    };
    if !by_model {
        let sent = draft.get("title").and_then(Value::as_str).is_some_and(|title| !title.trim().is_empty());
        if let Some(title) = old.fields.get("title").filter(|title| title.is_string() && !sent) {
            draft.insert("title".into(), title.clone());
        }
        return;
    }
    let declared = type_spec("task").map(|spec| spec.fields).unwrap_or_default();
    for field in declared.iter().map(|field| field.name).filter(|name| !PROGRAM_FIELDS.contains(name)) {
        if let Some(value) = old.fields.get(field).filter(|_| !draft.contains_key(field)) {
            draft.insert(field.to_string(), value.clone());
        }
    }
}
