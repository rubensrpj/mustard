//! As sobras de uma volta: o que o agente de onda viu e deixou de fora da
//! entrega, no campo `leftovers`. Toda sobra vira tarefa da spec, no fim do
//! backlog, sem pergunta: o que nasce na obra se resolve nela, e a rodada
//! nunca passa um achado à lista de pendências do projeto. A sobra que o
//! agente marca como limpeza (`cleanup: true`) vira tarefa com a mesma marca,
//! e a rodada a segura até o fim da obra, para sair junto das outras
//! limpezas numa onda só. O campo `kind` que uma volta antiga ainda traga é
//! aceito e ignorado.

use std::path::Path;

use mustard_core::domain::project_map::cited_paths;
use mustard_core::domain::spec_events::{Kind, Refusal, SpecEvent, SpecLog};
use mustard_core::io::wave_prompt;
use serde_json::{json, Map, Value};

use super::report::own_copy_relative;

/// O campo que marca a limpeza, na sobra da volta e na tarefa que ela vira:
/// o nome mora só aqui.
const CLEANUP: &str = "cleanup";

/// Uma sobra da volta: o título e o detalhe dela, e se o agente a marcou
/// como limpeza — só muda comentário, documentação ou texto de ajuda, sem
/// mudar comportamento nem o que um teste espera.
pub(crate) struct Leftover {
    pub title: String,
    pub detail: String,
    pub cleanup: bool,
}

/// As sobras da lista `leftovers` da volta, na ordem. A sobra sem título ou
/// sem detalhe fica de fora — a gravação já a recusa —, e o `kind` que ela
/// ainda traga não é lido. A marca de limpeza ausente vale "não"; a que não é
/// sim nem não é recusada, pelo número da sobra na lista, a contar de um.
///
/// # Errors
///
/// [`Refusal::InvalidValue`] quando a marca de limpeza de uma sobra não é um
/// booleano.
pub(super) fn leftovers_of(items: &[Value]) -> Result<Vec<Leftover>, Refusal> {
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(|t| t.trim().to_string());
    let mut leftovers = Vec::new();
    for (at, item) in items.iter().enumerate() {
        let cleanup = match item.get(CLEANUP) {
            None => false,
            Some(Value::Bool(marked)) => *marked,
            Some(_) => {
                return Err(Refusal::InvalidValue {
                    event_type: "delivered".into(),
                    field: format!("leftovers[{}].{CLEANUP}", at + 1),
                    expected: Kind::Bool,
                });
            }
        };
        if let (Some(title), Some(detail)) = (field(item, "title"), field(item, "detail")) {
            leftovers.push(Leftover { title, detail, cleanup });
        }
    }
    Ok(leftovers)
}

/// `true` quando a tarefa é limpeza: nasceu de uma sobra que o agente marcou
/// assim. É a leitura única de "é limpeza", e a fila da rodada lê só daqui.
pub(super) fn is_cleanup(task: &SpecEvent) -> bool {
    task.fields.get(CLEANUP) == Some(&Value::Bool(true))
}

/// A tarefa que a sobra vira: o título e o detalhe dela, sem dependência, com
/// o autor da onda, e os arquivos que o detalhe cita entre crases e que
/// existem no repositório ou na cópia da onda — o caminho de dentro da cópia
/// vira o relativo ao repositório, e o de fora dos dois não entra. A tarefa
/// cobre os critérios da onda que a apontou, na versão atual dela: é por eles
/// que a onda do conserto, formada do backlog, ganha a prova que diz quando
/// está pronta. Onda sem critério deixa a tarefa sem `covers`. A sobra
/// marcada como limpeza dá a tarefa com a mesma marca; a sem marca, a tarefa
/// sem o campo.
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
    let mut task = task.as_object().cloned().unwrap_or_default();
    if leftover.cleanup {
        task.insert(CLEANUP.into(), json!(true));
    }
    let criteria = log
        .visible()
        .into_iter()
        .find(|e| e.event_type == "wave" && e.wave() == Some(wave))
        .map(|e| e.ints("criteria"))
        .unwrap_or_default();
    if !criteria.is_empty() {
        task.insert("covers".into(), json!(criteria));
    }
    task
}
