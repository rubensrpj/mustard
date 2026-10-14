//! As sobras de uma volta: o que o agente de onda viu e deixou de fora da
//! entrega, no campo `leftovers`. Toda sobra vai ao backlog da spec, sem
//! pergunta: o que nasce na obra se resolve nela, e a rodada nunca passa um
//! achado à lista de pendências do projeto. A sobra vira tarefa nova, ou,
//! quando todos os arquivos que ela cita já estão numa tarefa aberta do
//! backlog, uma linha a mais nessa tarefa: a mesma sobra vista por duas ondas
//! não vira duas tarefas. A sobra que repete uma tarefa ainda por entregar —
//! os mesmos arquivos e o mesmo título ou o mesmo texto — vira nota que aponta
//! essa tarefa, sem tarefa nova nem versão nova dela. A sobra que não cita
//! arquivo nenhum vira tarefa presa, a planejar: ela não sai numa onda até
//! quem conduz a obra gravar uma versão nova com os arquivos e as
//! dependências. A sobra que o agente marca como limpeza (`cleanup: true`)
//! vira tarefa com a mesma marca, e a rodada a segura até o fim da obra, para
//! sair junto das outras limpezas numa onda só. O campo `kind` que uma volta
//! antiga ainda traga é aceito e ignorado.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mustard_core::domain::project_map::cited_paths;
use mustard_core::domain::spec_events::{Kind, Refusal, SpecEvent, SpecLog};
use mustard_core::domain::text;
use mustard_core::io::wave_prompt;
use mustard_core::platform::i18n::{translate, Locale};
use serde_json::{json, Map, Value};

use super::queue::{backlog_left, task_files, tasks_not_delivered};
use super::report::{backlog_return, own_copy_relative};
use super::stops::append_agent_line;

/// O campo que marca a limpeza, na sobra da volta e na tarefa que ela vira:
/// o nome mora só aqui.
const CLEANUP: &str = "cleanup";

/// O tipo do evento que a sobra vira quando é trabalho: a tarefa nova ou a
/// versão nova de uma tarefa aberta.
const TASK: &str = "task";

/// O tipo do evento que a sobra vira quando repete uma tarefa ainda por
/// entregar ([`repeats`]).
const NOTE: &str = "note";

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

/// Os eventos que as sobras `found` viram, cada um com a onda que a apontou e
/// o tipo dele, na ordem das sobras. A sobra que repete uma tarefa ainda por
/// entregar lida em `log` ([`repeats`]), fora as tarefas das ondas em
/// `returning`, que a volta entrega agora, vira nota que aponta essa tarefa
/// ([`repeated_note`]), e a tarefa fica como está. A sobra cujos arquivos
/// citados ([`leftover_files`]) estão todos entre os de uma tarefa aberta do
/// backlog não vira tarefa nova: essa tarefa ganha uma versão nova, que a
/// substitui, com a sobra numa linha da parte do agente ([`join`]), no idioma
/// `lang`. A tarefa que outra sobra da mesma volta acabou de criar vale como
/// aberta também, e ganha a linha na própria versão nova. Com mais de uma
/// tarefa que sirva, fica a de menor número. O resto vira tarefa nova
/// ([`leftover_task`]), presa até ser planejada quando a sobra não cita
/// arquivo nenhum.
pub(super) fn leftover_events(
    root: &Path,
    log: &SpecLog,
    found: &[(u64, &Leftover)],
    returning: &BTreeSet<u64>,
    lang: Locale,
) -> Vec<(u64, &'static str, Map<String, Value>)> {
    let open: Vec<(&SpecEvent, BTreeSet<String>)> = backlog_left(log)
        .into_iter()
        .filter_map(|id| log.get(id))
        .map(|task| (task, task_files(task)))
        .filter(|(_, files)| !files.is_empty())
        .collect();
    let unfinished: Vec<&SpecEvent> = tasks_not_delivered(log, returning).into_iter().filter_map(|id| log.get(id)).collect();
    let mut out: Vec<(u64, &'static str, Map<String, Value>)> = Vec::new();
    // Onde está, em `out`, a versão nova de cada tarefa aberta que já ganhou
    // uma linha nesta volta, pelo número dela; e cada tarefa nascida nesta
    // volta, pelos arquivos dela.
    let mut joined: BTreeMap<u64, usize> = BTreeMap::new();
    let mut born: Vec<(BTreeSet<String>, usize)> = Vec::new();
    for &(wave, leftover) in found {
        let files: BTreeSet<String> = leftover_files(root, log, wave, leftover).into_iter().collect();
        if let Some(task) = unfinished.iter().find(|task| repeats(task, &files, leftover)) {
            out.push((wave, NOTE, repeated_note(log, wave, leftover, task, lang)));
            continue;
        }
        let at = if files.is_empty() {
            None
        } else if let Some((task, _)) = open.iter().find(|(_, own)| files.is_subset(own)) {
            Some(*joined.entry(task.id).or_insert_with(|| {
                out.push((wave, TASK, backlog_return(task)));
                out.len() - 1
            }))
        } else {
            born.iter().find(|(own, _)| files.is_subset(own)).map(|(_, at)| *at)
        };
        match at {
            Some(at) => join(&mut out[at].2, log, wave, leftover, lang),
            None => {
                if !files.is_empty() {
                    born.push((files, out.len()));
                }
                out.push((wave, TASK, leftover_task(root, log, wave, leftover, lang)));
            }
        }
    }
    out
}

/// `true` quando a sobra `leftover`, que cita os arquivos `files`, repete a
/// tarefa `task`: a tarefa declara exatamente esses arquivos — sem arquivo
/// nenhum dos dois lados também vale — e o título dela é o da sobra, ou o
/// texto dela é o detalhe da sobra, na forma em que dois textos se comparam
/// ([`comparable`]). Só o arquivo em comum não basta: a sobra diferente nos
/// mesmos arquivos entra na tarefa como linha ([`join`]).
fn repeats(task: &SpecEvent, files: &BTreeSet<String>, leftover: &Leftover) -> bool {
    let same = |field: &str, said: &str| {
        let said = comparable(said);
        !said.is_empty() && task.str_field(field).is_some_and(|own| comparable(own) == said)
    };
    task_files(task) == *files && (same("title", &leftover.title) || same("text", &leftover.detail))
}

/// O texto na forma em que a sobra e a tarefa se comparam: as palavras dele,
/// em minúsculas e sem acento, separadas por um espaço só — a pontuação, a
/// crase e a quebra de linha não contam.
fn comparable(said: &str) -> String {
    text::words(&text::fold(said)).collect::<Vec<_>>().join(" ")
}

/// O texto em uma linha só, com um espaço entre as palavras.
fn one_line(said: &str) -> String {
    said.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A nota que a sobra `leftover`, apontada pela onda `wave`, deixa na tarefa
/// `task` que ela repete, no idioma `lang`: o título da sobra e um texto com a
/// onda, o código da tarefa, lido em `log`, e o detalhe numa linha só. O
/// código vai também no rótulo e na chave de busca: é por ele que a nota
/// aponta a tarefa, sem mudar nada nela.
fn repeated_note(log: &SpecLog, wave: u64, leftover: &Leftover, task: &SpecEvent, lang: Locale) -> Map<String, Value> {
    let code = log.codes().get(&task.id).cloned().unwrap_or_else(|| task.id.to_string());
    let text = translate("round.leftover_repeated", lang)
        .replace("{wave}", &wave.to_string())
        .replace("{task}", &code)
        .replace("{detail}", &one_line(&leftover.detail));
    let note = json!({ "title": leftover.title, "text": text, "keys": [code], "label": code, "author": "wave" });
    note.as_object().cloned().unwrap_or_default()
}

/// Junta a sobra `leftover`, apontada pela onda `wave`, à versão `draft` de
/// uma tarefa: a parte do agente ganha uma linha com o título e o detalhe
/// dela, numa linha só, no idioma `lang`; a tarefa passa a cobrir também os
/// critérios da onda, lidos em `log`, sem repetir; e a marca de limpeza fica
/// só quando a sobra também a traz — a tarefa que passa a mudar
/// comportamento não espera o fim da obra.
fn join(draft: &mut Map<String, Value>, log: &SpecLog, wave: u64, leftover: &Leftover, lang: Locale) {
    let line = translate("round.leftover_joined", lang)
        .replace("{wave}", &wave.to_string())
        .replace("{title}", &leftover.title)
        .replace("{detail}", &one_line(&leftover.detail));
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
/// sem o campo. A sobra que não cita arquivo nenhum dá a tarefa presa
/// (`blocked_by`), com o motivo no idioma `lang`: um agente partiria sem
/// saber o que tocar, e a revisão não teria escopo a conferir. A fila não a
/// despacha até uma versão nova, gravada por quem conduz a obra com os
/// arquivos e as dependências, sair sem a trava.
pub(super) fn leftover_task(root: &Path, log: &SpecLog, wave: u64, leftover: &Leftover, lang: Locale) -> Map<String, Value> {
    let files: Vec<Value> =
        leftover_files(root, log, wave, leftover).into_iter().map(|path| json!({ "path": path })).collect();
    let unplanned = files.is_empty();
    let task = json!({
        "title": leftover.title,
        "text": leftover.detail,
        "files": files,
        "depends_on": [],
        "author": "wave",
    });
    let mut task = task.as_object().cloned().unwrap_or_default();
    if unplanned {
        task.insert("blocked_by".into(), json!(translate("round.leftover_without_files", lang)));
    }
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
    use mustard_core::domain::spec_state::PhaseWriter;
    use mustard_core::io::spec_events as store;
    use tempfile::tempdir;

    use super::super::tests::{approved, git_at, returned, round, waves_in};
    use super::*;

    /// A spec `x` como está gravada agora.
    fn spec_x(root: &Path) -> SpecLog {
        store::read(&store::spec_file(root, "x").unwrap()).unwrap().unwrap()
    }

    /// Os eventos do tipo `kind` que a rodada `out` gravou, lidos na spec.
    fn recorded(root: &Path, out: &Value, kind: &str) -> Vec<SpecEvent> {
        let log = spec_x(root);
        out["recorded"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["type"] == json!(kind))
            .map(|r| log.get(r["id"].as_u64().unwrap()).cloned().unwrap_or_else(|| panic!("{out}")))
            .collect()
    }

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

        let tasks = leftover_events(root, &log, &found, &BTreeSet::new(), Locale::PtBr);
        assert_eq!(tasks.len(), 2, "{tasks:?}");
        let (_, _, joined) = &tasks[0];
        assert_eq!(joined.get("replaces"), Some(&json!(3)), "{joined:?}");
        let agent = format!(
            "- Olhe o índice.\n- {}\n- {}",
            line("Para sem índice", "Em `src/commit.rs`, para."),
            line("Para outra vez", "Ainda `src/commit.rs`.")
        );
        assert_eq!(joined.get("agent"), Some(&json!(agent)), "{joined:?}");
        assert_eq!(joined.get("covers"), Some(&json!([1])), "{joined:?}");
        assert!(!joined.contains_key(CLEANUP), "uma sobra muda comportamento: {joined:?}");
        let (_, _, born) = &tasks[1];
        assert!(!born.contains_key("replaces"), "{born:?}");
        assert_eq!(born.get("title"), Some(&json!("Arquivo novo")), "{born:?}");
        assert_eq!(born.get("files"), Some(&json!([{"path": "src/novo.rs"}])), "{born:?}");
        assert_eq!(born.get("agent"), Some(&json!(line("Arquivo novo de novo", "De novo `src/novo.rs`."))), "{born:?}");

        let tidy = leftover("Comentário velho", "Em `src/commit.rs`.", true);
        let tasks = leftover_events(root, &log, &[(1, &tidy)], &BTreeSet::new(), Locale::PtBr);
        assert_eq!(tasks.len(), 1, "{tasks:?}");
        assert_eq!(tasks[0].2.get("replaces"), Some(&json!(3)), "{tasks:?}");
        assert_eq!(tasks[0].2.get(CLEANUP), Some(&json!(true)), "limpeza sobre limpeza segue limpeza: {tasks:?}");
    }

    /// A volta traz uma sobra que não cita arquivo nenhum. A tarefa que ela
    /// vira fica presa, com o motivo, a fila a mostra a planejar, e a rodada
    /// seguinte não despacha onda nenhuma com ela. Só depois de quem conduz a
    /// obra gravar uma versão nova com os arquivos, sem a trava, ela sai numa
    /// onda.
    #[test]
    fn a_leftover_without_files_waits_for_planning() {
        // A onda que sai do backlog nasce do mapa que o scan grava: sem o
        // scan compilado junto, nenhuma sairia, e o teste passaria à toa.
        assert!(
            mustard_core::Scan::locate().is_compiled_alongside(),
            "o teste precisa do scan compilado junto com ele: rode `cargo build -p scan` antes de `cargo test -p mustard-rt`"
        );
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/c.rs"), "fn main() {}\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        git_at(root, &["checkout", "-q", "-b", "feature/x"]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let wrote = returned(
            root,
            json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai",
                "leftovers": [{"title": "O disco enche", "detail": "As cópias lotam o disco durante a obra."}]}),
        );
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let out = round(root, "x", None);
        let tasks = recorded(root, &out, "task");
        assert_eq!(tasks.len(), 1, "{out}");
        let task = &tasks[0];
        assert_eq!(task.fields["files"], json!([]), "{:?}", task.fields);
        assert_eq!(task.str_field("blocked_by"), Some(translate("round.leftover_without_files", Locale::PtBr)), "{:?}", task.fields);
        let code = spec_x(root).codes()[&task.id].clone();
        let queue = super::super::scheduling::snapshot(root, &spec_x(root));
        let row = queue["tasks"].as_array().unwrap().iter().find(|row| row["task"] == json!(code)).cloned();
        assert_eq!(row.map(|row| row["reason"].clone()), Some(json!("needs-planning")), "{queue}");

        let held = round(root, "x", None);
        assert!(waves_in(&held, "dispatch").is_empty(), "a tarefa sem arquivo não sai como onda: {held}");

        let mut planned = backlog_return(task);
        planned.remove("blocked_by");
        planned.insert("files".into(), json!([{"path": "src/c.rs"}]));
        crate::commands::spec_events::write::record(root, "x", "task", planned, PhaseWriter::Binary).unwrap();
        let next = round(root, "x", None);
        assert_eq!(waves_in(&next, "dispatch"), vec![2], "planejada, a tarefa sai: {next}");
    }

    /// A onda 1 volta com duas sobras, uma num arquivo e outra sem arquivo,
    /// e cada uma vira tarefa. A onda 2 volta depois com as mesmas duas e com
    /// uma de mesmo título que cita um arquivo a mais. As repetidas viram nota
    /// que aponta a tarefa pelo código, sem tarefa nova e sem versão nova da
    /// tarefa; a de um arquivo a mais vira tarefa nova.
    #[test]
    fn a_leftover_matching_an_open_task_becomes_a_note() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/c.rs"), "fn main() {}\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        git_at(root, &["checkout", "-q", "-b", "feature/x"]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let on_file = json!({"title": "A leitura para sem o índice", "detail": "Sem o índice, `src/c.rs` para."});
        let bare = json!({"title": "O log não gira", "detail": "O arquivo de log cresce sem limite."});

        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let wrote = returned(
            root,
            json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai",
                "leftovers": [on_file, bare]}),
        );
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let first = recorded(root, &round(root, "x", None), "task");
        assert_eq!(first.len(), 2, "{first:?}");
        let codes = spec_x(root).codes();
        let (file_task, bare_task) = (codes[&first[0].id].clone(), codes[&first[1].id].clone());

        std::fs::write(root.join("src/b.rs"), "fn one() {}\n// O dois saiu.\n").unwrap();
        let wider = json!({"title": "A leitura para sem o índice", "detail": "Sem o índice, `src/c.rs` e `src/b.rs` param."});
        let wrote = returned(
            root,
            json!({"wave": 2, "text": "O dois saiu.", "files": ["src/b.rs"], "commit": "o dois sai",
                "leftovers": [on_file, bare, wider]}),
        );
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let again = round(root, "x", None);
        assert_eq!(again["ok"], json!(true), "{again}");
        let notes = recorded(root, &again, "note");
        let labels: Vec<Option<&str>> = notes.iter().map(|note| note.str_field("label")).collect();
        assert_eq!(labels, [Some(file_task.as_str()), Some(bare_task.as_str())], "{again}");
        assert_eq!(notes[1].str_field("title"), Some("O log não gira"), "{:?}", notes[1].fields);
        let text = translate("round.leftover_repeated", Locale::PtBr)
            .replace("{wave}", "2")
            .replace("{task}", &bare_task)
            .replace("{detail}", "O arquivo de log cresce sem limite.");
        assert_eq!(notes[1].str_field("text"), Some(text.as_str()), "{:?}", notes[1].fields);
        let tasks = recorded(root, &again, "task");
        assert_eq!(tasks.len(), 1, "só a de um arquivo a mais vira tarefa: {again}");
        assert!(!tasks[0].fields.contains_key("replaces"), "nenhuma tarefa ganha versão nova: {again}");
        let files: BTreeSet<&str> = tasks[0].fields["files"].as_array().into_iter().flatten().filter_map(|f| f["path"].as_str()).collect();
        assert_eq!(files, BTreeSet::from(["src/b.rs", "src/c.rs"]), "{:?}", tasks[0].fields);
    }
}
