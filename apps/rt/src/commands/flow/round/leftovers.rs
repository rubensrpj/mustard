//! As sobras de uma volta: o que o agente de onda viu e deixou de fora da
//! entrega, no campo `leftovers`. Toda sobra vira tarefa da spec, no fim do
//! backlog, sem pergunta: o que nasce na obra se resolve nela, e a rodada
//! nunca passa um achado à lista de pendências do projeto. O campo `kind` que
//! uma volta antiga ainda traga é aceito e ignorado.

use std::path::Path;

use mustard_core::domain::project_map::cited_paths;
use mustard_core::domain::spec_events::SpecLog;
use mustard_core::io::wave_prompt;
use serde_json::{json, Map, Value};

use super::report::own_copy_relative;

/// Uma sobra da volta: o título e o detalhe dela.
pub(crate) struct Leftover {
    pub title: String,
    pub detail: String,
}

/// As sobras da lista `leftovers` da volta, na ordem. A sobra sem título ou
/// sem detalhe fica de fora — a gravação já a recusa —, e o `kind` que ela
/// ainda traga não é lido.
pub(super) fn leftovers_of(items: &[Value]) -> Vec<Leftover> {
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(|t| t.trim().to_string());
    items
        .iter()
        .filter_map(|item| Some(Leftover { title: field(item, "title")?, detail: field(item, "detail")? }))
        .collect()
}

/// A tarefa que a sobra vira: o título e o detalhe dela, sem dependência, com
/// o autor da onda, e os arquivos que o detalhe cita entre crases e que
/// existem no repositório ou na cópia da onda — o caminho de dentro da cópia
/// vira o relativo ao repositório, e o de fora dos dois não entra. A tarefa
/// cobre os critérios da onda que a apontou, na versão atual dela: é por eles
/// que a onda do conserto, formada do backlog, ganha a prova que diz quando
/// está pronta. Onda sem critério deixa a tarefa sem `covers`.
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
