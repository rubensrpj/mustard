//! As sobras de uma volta: o que o agente de onda viu e deixou de fora da
//! entrega, no campo `leftovers`. Toda sobra vai ao backlog da spec, sem
//! pergunta: o que nasce na obra se resolve nela, e a rodada nunca passa um
//! achado à lista de pendências do projeto. A sobra vira tarefa nova, ou,
//! quando todos os arquivos que ela cita já estão numa tarefa aberta do
//! backlog, uma linha a mais nessa tarefa: a mesma sobra vista por duas ondas
//! não vira duas tarefas. A sobra que o agente marca como limpeza
//! (`cleanup: true`) vira tarefa com a mesma marca, e a rodada a segura até o
//! fim da obra, para sair junto das outras limpezas numa onda só. O campo
//! `kind` que uma volta antiga ainda traga é aceito e ignorado.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::project_map::cited_paths;
use mustard_core::domain::spec_events::{Kind, Refusal, SpecEvent, SpecLog};
use mustard_core::io::wave_prompt;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::queue::{backlog_left, task_files};
use super::report::{backlog_return, own_copy_relative};
use super::stops::append_agent_line;

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

/// As tarefas que as sobras `found` viram, cada uma com a onda que a
/// apontou, na ordem das sobras. A sobra cujos arquivos citados
/// ([`leftover_files`]) estão todos entre os de uma tarefa aberta do backlog
/// lido em `log` não vira tarefa nova: essa tarefa ganha uma versão nova, que
/// a substitui, com a sobra numa linha da parte do agente ([`join`]), no
/// idioma `lang`. A tarefa que outra sobra da mesma volta acabou de criar vale
/// como aberta também, e ganha a linha na própria versão nova. Com mais de uma
/// tarefa que sirva, fica a de menor número. A comparação é só pelos
/// arquivos, nunca pelo texto; a sobra que não cita arquivo nenhum não tem o
/// que comparar e sempre vira tarefa nova ([`leftover_task`]).
pub(super) fn leftover_tasks(
    root: &Path,
    log: &SpecLog,
    found: &[(u64, &Leftover)],
    lang: Locale,
) -> Vec<(u64, Map<String, Value>)> {
    let open: Vec<(&SpecEvent, BTreeSet<String>)> = backlog_left(log)
        .into_iter()
        .filter_map(|id| log.get(id))
        .map(|task| (task, task_files(task)))
        .filter(|(_, files)| !files.is_empty())
        .collect();
    let mut out: Vec<(u64, Map<String, Value>)> = Vec::new();
    // Onde está, em `out`, a versão nova de cada tarefa aberta que já ganhou
    // uma linha nesta volta, pelo número dela; e cada tarefa nascida nesta
    // volta, pelos arquivos dela.
    let mut joined: BTreeMap<u64, usize> = BTreeMap::new();
    let mut born: Vec<(BTreeSet<String>, usize)> = Vec::new();
    for &(wave, leftover) in found {
        let files: BTreeSet<String> = leftover_files(root, log, wave, leftover).into_iter().collect();
        let at = if files.is_empty() {
            None
        } else if let Some((task, _)) = open.iter().find(|(_, own)| files.is_subset(own)) {
            Some(*joined.entry(task.id).or_insert_with(|| {
                out.push((wave, backlog_return(task)));
                out.len() - 1
            }))
        } else {
            born.iter().find(|(own, _)| files.is_subset(own)).map(|(_, at)| *at)
        };
        match at {
            Some(at) => join(&mut out[at].1, log, wave, leftover, lang),
            None => {
                if !files.is_empty() {
                    born.push((files, out.len()));
                }
                out.push((wave, leftover_task(root, log, wave, leftover)));
            }
        }
    }
    out
}

/// Junta a sobra `leftover`, apontada pela onda `wave`, à versão `draft` de
/// uma tarefa: a parte do agente ganha uma linha com o título e o detalhe
/// dela, numa linha só, no idioma `lang`; a tarefa passa a cobrir também os
/// critérios da onda, lidos em `log`, sem repetir; e a marca de limpeza fica
/// só quando a sobra também a traz — a tarefa que passa a mudar
/// comportamento não espera o fim da obra.
fn join(draft: &mut Map<String, Value>, log: &SpecLog, wave: u64, leftover: &Leftover, lang: Locale) {
    let detail = leftover.detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = translate("round.leftover_joined", lang)
        .replace("{wave}", &wave.to_string())
        .replace("{title}", &leftover.title)
        .replace("{detail}", &detail);
    append_agent_line(draft, &line);
    let mut covers: Vec<u64> =
        draft.get("covers").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
    for id in wave_criteria(log, wave) {
        if !covers.contains(&id) {
            covers.push(id);
        }
    }
    if !covers.is_empty() {
        draft.insert("covers".into(), json!(covers));
    }
    if !leftover.cleanup {
        draft.remove(CLEANUP);
    }
}

/// Os arquivos que o detalhe da sobra `leftover` cita entre crases e que
/// existem no repositório `root` ou na cópia da onda `wave`, lida em `log`,
/// sem repetir: o caminho de dentro da cópia vira o relativo ao repositório,
/// e o de fora dos dois não entra.
fn leftover_files(root: &Path, log: &SpecLog, wave: u64, leftover: &Leftover) -> Vec<String> {
    let copy = wave_prompt::recorded_copy(log, wave).map(|copy| Path::new(&copy.path).to_path_buf());
    let mut files: Vec<String> = Vec::new();
    for cited in cited_paths(&leftover.detail) {
        let path = own_copy_relative(log, wave, &cited);
        let on_disk = root.join(&path).exists() || copy.as_ref().is_some_and(|copy| copy.join(&path).exists());
        if !Path::new(&path).is_absolute() && on_disk && !files.contains(&path) {
            files.push(path);
        }
    }
    files
}

/// Os critérios da versão atual da onda `wave`, lidos em `log`; vazio quando
/// a onda não tem nenhum.
fn wave_criteria(log: &SpecLog, wave: u64) -> Vec<u64> {
    log.visible()
        .into_iter()
        .find(|e| e.event_type == "wave" && e.wave() == Some(wave))
        .map(|e| e.ints("criteria"))
        .unwrap_or_default()
}

/// A tarefa nova que a sobra vira: o título e o detalhe dela, sem
/// dependência, com o autor da onda, e os arquivos que o detalhe cita
/// ([`leftover_files`]). A tarefa cobre os critérios da onda que a apontou, na versão atual dela: é por eles
/// que a onda do conserto, formada do backlog, ganha a prova que diz quando
/// está pronta. Onda sem critério deixa a tarefa sem `covers`. A sobra
/// marcada como limpeza dá a tarefa com a mesma marca; a sem marca, a tarefa
/// sem o campo.
pub(super) fn leftover_task(root: &Path, log: &SpecLog, wave: u64, leftover: &Leftover) -> Map<String, Value> {
    let files: Vec<Value> =
        leftover_files(root, log, wave, leftover).into_iter().map(|path| json!({ "path": path })).collect();
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
    let criteria = wave_criteria(log, wave);
    if !criteria.is_empty() {
        task.insert("covers".into(), json!(criteria));
    }
    task
}

#[cfg(test)]
mod tests {
    use mustard_core::domain::spec_events::parse_log;
    use tempfile::tempdir;

    use super::*;

    /// A sobra de título `title`, com o detalhe `detail` e a marca de limpeza
    /// `cleanup`.
    fn leftover(title: &str, detail: &str, cleanup: bool) -> Leftover {
        Leftover { title: title.into(), detail: detail.into(), cleanup }
    }

    /// A linha que a sobra de título `title` e detalhe `detail`, da onda 1,
    /// deixa na parte do agente da tarefa que a recebe.
    fn line(title: &str, detail: &str) -> String {
        translate("round.leftover_joined", Locale::PtBr)
            .replace("{wave}", "1")
            .replace("{title}", title)
            .replace("{detail}", detail)
    }

    /// Na mesma volta, as sobras que citam só arquivos de uma tarefa aberta
    /// do backlog viram linhas numa versão nova dela, uma por sobra, no fim da
    /// lista que a parte do agente já trazia, com o detalhe numa linha só e os
    /// critérios da onda somados e sem a marca de limpeza quando uma delas
    /// não é limpeza; a sobra que cita arquivo novo vira tarefa nova, e a que
    /// cita só os arquivos dessa tarefa nova vira linha nela, sem outra
    /// tarefa. A marca de limpeza da tarefa aberta fica quando a sobra também
    /// é limpeza.
    #[test]
    fn leftovers_on_the_files_of_an_open_task_join_it_instead_of_making_a_new_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for file in ["src/commit.rs", "src/novo.rs"] {
            std::fs::write(root.join(file), "fn one() {}\n").unwrap();
        }
        let log = parse_log(
            &[
                json!({"v":1,"id":1,"type":"criterion","when":"a","then":"b","proof":"git --version","form":"ubiquitous"}),
                json!({"v":1,"id":2,"type":"wave","n":1,"text":"t","criteria":[1],"done_when":"d"}),
                json!({"v":1,"id":3,"type":"task","title":"O índice some","text":"Some.","agent":"- Olhe o índice.",
                    "files":[{"path":"src/commit.rs"}, {"path":"src/c.rs"}],"depends_on":[],"cleanup":true}),
            ]
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        );
        let first = leftover("Para sem índice", "Em `src/commit.rs`,\n  para.", false);
        let fresh = leftover("Arquivo novo", "Em `src/novo.rs`.", false);
        let fresh_again = leftover("Arquivo novo de novo", "De novo `src/novo.rs`.", true);
        let second = leftover("Para outra vez", "Ainda `src/commit.rs`.", true);
        let found = [(1, &first), (1, &fresh), (1, &fresh_again), (1, &second)];

        let tasks = leftover_tasks(root, &log, &found, Locale::PtBr);
        assert_eq!(tasks.len(), 2, "{tasks:?}");
        let (_, joined) = &tasks[0];
        assert_eq!(joined.get("replaces"), Some(&json!(3)), "{joined:?}");
        let agent = format!(
            "- Olhe o índice.\n- {}\n- {}",
            line("Para sem índice", "Em `src/commit.rs`, para."),
            line("Para outra vez", "Ainda `src/commit.rs`.")
        );
        assert_eq!(joined.get("agent"), Some(&json!(agent)), "{joined:?}");
        assert_eq!(joined.get("covers"), Some(&json!([1])), "{joined:?}");
        assert!(!joined.contains_key(CLEANUP), "uma sobra muda comportamento: {joined:?}");
        let (_, born) = &tasks[1];
        assert!(!born.contains_key("replaces"), "{born:?}");
        assert_eq!(born.get("title"), Some(&json!("Arquivo novo")), "{born:?}");
        assert_eq!(born.get("files"), Some(&json!([{"path": "src/novo.rs"}])), "{born:?}");
        assert_eq!(born.get("agent"), Some(&json!(line("Arquivo novo de novo", "De novo `src/novo.rs`."))), "{born:?}");

        let tidy = leftover("Comentário velho", "Em `src/commit.rs`.", true);
        let tasks = leftover_tasks(root, &log, &[(1, &tidy)], Locale::PtBr);
        assert_eq!(tasks.len(), 1, "{tasks:?}");
        assert_eq!(tasks[0].1.get("replaces"), Some(&json!(3)), "{tasks:?}");
        assert_eq!(tasks[0].1.get(CLEANUP), Some(&json!(true)), "limpeza sobre limpeza segue limpeza: {tasks:?}");
    }
}
