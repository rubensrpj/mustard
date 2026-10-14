//! As sobras de uma volta: o que o agente de onda viu e deixou de fora da
//! entrega, no campo `leftovers`. Toda sobra vai ao backlog da spec, sem
//! pergunta: o que nasce na obra se resolve nela, e a rodada nunca passa um
//! achado à lista de pendências do projeto. A sobra vira um pedido próprio,
//! com o título e o relato dela, e uma tarefa que cobre esse pedido — nunca
//! os critérios da onda que a relatou, que quase nunca têm a ver com ela.
//! Quando todos os arquivos dela já estão numa tarefa aberta do backlog, a
//! tarefa é essa, que ganha uma linha a mais e passa a cobrir o pedido: a
//! mesma sobra vista por duas ondas não vira duas tarefas. A sobra que repete
//! uma tarefa ainda por entregar — os mesmos arquivos e o mesmo título ou o
//! mesmo texto — vira nota que aponta essa tarefa, sem pedido, tarefa nova
//! nem versão nova dela; a que repete outra sobra da mesma volta vira só uma
//! linha na tarefa que a primeira ganhou. Os arquivos da sobra são os que ela
//! declara em `files`; sem a lista, os que o detalhe cita. A sobra sem
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

/// O campo da sobra com os arquivos que ela pede para mudar.
const FILES: &str = "files";

/// O tipo do evento que a sobra vira quando é trabalho: a tarefa nova ou a
/// versão nova de uma tarefa aberta.
const TASK: &str = "task";

/// O tipo do evento que a sobra vira quando repete uma tarefa ainda por
/// entregar ([`repeats`]).
const NOTE: &str = "note";

/// O tipo do evento que guarda o relato da sobra, e que a tarefa cobre.
const REQUEST: &str = "request";

/// Uma sobra da volta: o título e o detalhe dela, os arquivos que ela
/// declara mudar (`None` quando a volta não traz a lista), e se o agente a
/// marcou como limpeza — só muda comentário, documentação ou texto de ajuda,
/// sem mudar comportamento nem o que um teste espera.
pub(crate) struct Leftover {
    pub title: String,
    pub detail: String,
    pub files: Option<Vec<String>>,
    pub cleanup: bool,
}

/// As sobras da lista `leftovers` da volta, na ordem. A sobra sem título ou
/// sem detalhe fica de fora — a gravação já a recusa —, e o `kind` que ela
/// ainda traga não é lido. A marca de limpeza ausente vale "não"; a que não é
/// sim nem não é recusada, como a lista de arquivos que não é uma lista de
/// textos, pelo número da sobra na lista, a contar de um.
///
/// # Errors
///
/// [`Refusal::InvalidValue`] quando a marca de limpeza de uma sobra não é um
/// booleano, ou os arquivos dela não são uma lista de textos.
pub(super) fn leftovers_of(items: &[Value]) -> Result<Vec<Leftover>, Refusal> {
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(|t| t.trim().to_string());
    let invalid = |at: usize, name: &str, expected: Kind| Refusal::InvalidValue {
        event_type: "delivered".into(),
        field: format!("leftovers[{}].{name}", at + 1),
        expected,
    };
    let mut leftovers = Vec::new();
    for (at, item) in items.iter().enumerate() {
        let cleanup = match item.get(CLEANUP) {
            None => false,
            Some(Value::Bool(marked)) => *marked,
            Some(_) => return Err(invalid(at, CLEANUP, Kind::Bool)),
        };
        let files = match item.get(FILES) {
            None => None,
            Some(Value::Array(listed)) => Some(
                listed
                    .iter()
                    .map(|path| path.as_str().map(|path| path.trim().to_string()))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| invalid(at, FILES, Kind::Texts))?,
            ),
            Some(_) => return Err(invalid(at, FILES, Kind::Texts)),
        };
        if let (Some(title), Some(detail)) = (field(item, "title"), field(item, "detail")) {
            leftovers.push(Leftover { title, detail, files, cleanup });
        }
    }
    Ok(leftovers)
}

/// `true` quando a tarefa é limpeza: nasceu de uma sobra que o agente marcou
/// assim. É a leitura única de "é limpeza", e a fila da rodada lê só daqui.
pub(super) fn is_cleanup(task: &SpecEvent) -> bool {
    task.fields.get(CLEANUP) == Some(&Value::Bool(true))
}

/// Um evento que as sobras da volta viram, com a onda que a apontou e o tipo
/// dele. `asks` são os pedidos que a tarefa passa a cobrir, pela posição de
/// cada um na mesma lista: o número de um pedido só existe depois que ele é
/// gravado, e todos vêm antes das tarefas.
pub(super) struct LeftoverEvent {
    pub(super) wave: u64,
    pub(super) kind: &'static str,
    pub(super) draft: Map<String, Value>,
    pub(super) asks: Vec<usize>,
}

/// Os eventos que as sobras `found` viram, cada uma com a onda que a apontou:
/// primeiro o pedido de cada sobra que é trabalho novo ([`leftover_request`]),
/// na ordem das sobras, e depois as tarefas e as notas. A sobra que repete
/// uma tarefa ainda por entregar lida em `log` ([`repeats`]), fora as tarefas
/// das ondas em `returning`, que a volta entrega agora, vira nota que aponta
/// essa tarefa ([`repeated_note`]), e a tarefa fica como está. A que repete
/// uma sobra anterior desta volta — os mesmos arquivos e o mesmo título ou o
/// mesmo detalhe — vira só uma linha na tarefa que aquela ganhou, sem pedido
/// novo. A sobra cujos arquivos ([`leftover_files`]) estão todos entre os de
/// uma tarefa aberta do backlog não vira tarefa nova: essa tarefa ganha uma
/// versão nova, que a substitui, com a sobra numa linha da parte do agente
/// ([`join`]), no idioma `lang`, e passa a cobrir o pedido dela. A tarefa que
/// outra sobra da mesma volta acabou de criar vale como aberta também. Com
/// mais de uma tarefa que sirva, fica a de menor número. O resto vira tarefa
/// nova ([`leftover_task`]) que cobre o pedido, presa até ser planejada
/// quando a sobra não tem arquivo nenhum. O pedido aponta em `origin` a
/// entrega da onda em `delivered`, quando ela está lá.
pub(super) fn leftover_events(
    root: &Path,
    log: &SpecLog,
    found: &[(u64, &Leftover)],
    returning: &BTreeSet<u64>,
    delivered: &BTreeMap<u64, u64>,
    lang: Locale,
) -> Vec<LeftoverEvent> {
    let open: Vec<(&SpecEvent, BTreeSet<String>)> = backlog_left(log)
        .into_iter()
        .filter_map(|id| log.get(id))
        .map(|task| (task, task_files(task)))
        .filter(|(_, files)| !files.is_empty())
        .collect();
    let unfinished: Vec<&SpecEvent> = tasks_not_delivered(log, returning).into_iter().filter_map(|id| log.get(id)).collect();
    let mut requests: Vec<LeftoverEvent> = Vec::new();
    let mut work: Vec<LeftoverEvent> = Vec::new();
    // Onde está, em `work`, a versão nova de cada tarefa aberta que já ganhou
    // uma linha nesta volta, pelo número dela; cada tarefa nascida nesta
    // volta, pelos arquivos dela; e cada sobra desta volta que virou pedido,
    // com os arquivos dela e a tarefa que a recebeu.
    let mut joined: BTreeMap<u64, usize> = BTreeMap::new();
    let mut born: Vec<(BTreeSet<String>, usize)> = Vec::new();
    let mut asked: Vec<(BTreeSet<String>, &Leftover, usize)> = Vec::new();
    for &(wave, leftover) in found {
        let files: BTreeSet<String> = leftover_files(root, log, wave, leftover).into_iter().collect();
        if let Some(task) = unfinished.iter().find(|task| repeats(&task_files(task), task.str_field("title"), task.str_field("text"), &files, leftover)) {
            work.push(LeftoverEvent { wave, kind: NOTE, draft: repeated_note(log, wave, leftover, task, lang), asks: Vec::new() });
            continue;
        }
        if let Some(&(_, _, at)) =
            asked.iter().find(|(own, earlier, _)| repeats(own, Some(&earlier.title), Some(&earlier.detail), &files, leftover))
        {
            join(&mut work[at].draft, wave, leftover, lang);
            continue;
        }
        let request = requests.len();
        requests.push(LeftoverEvent { wave, kind: REQUEST, draft: leftover_request(leftover, &files, delivered.get(&wave).copied()), asks: Vec::new() });
        let at = if files.is_empty() {
            None
        } else if let Some((task, _)) = open.iter().find(|(_, own)| files.is_subset(own)) {
            Some(*joined.entry(task.id).or_insert_with(|| {
                work.push(LeftoverEvent { wave, kind: TASK, draft: backlog_return(task), asks: Vec::new() });
                work.len() - 1
            }))
        } else {
            born.iter().find(|(own, _)| files.is_subset(own)).map(|(_, at)| *at)
        };
        let at = match at {
            Some(at) => {
                join(&mut work[at].draft, wave, leftover, lang);
                work[at].asks.push(request);
                at
            }
            None => {
                if !files.is_empty() {
                    born.push((files.clone(), work.len()));
                }
                work.push(LeftoverEvent { wave, kind: TASK, draft: leftover_task(root, log, wave, leftover, lang), asks: vec![request] });
                work.len() - 1
            }
        };
        asked.push((files, leftover, at));
    }
    requests.extend(work);
    requests
}

/// `true` quando a sobra `leftover`, com os arquivos `files`, repete o
/// trabalho de arquivos `own`, título `title` e texto `text` — uma tarefa ou
/// outra sobra: os arquivos são exatamente os mesmos — sem arquivo nenhum
/// dos dois lados também vale — e o título é o da sobra, ou o texto é o
/// detalhe dela, na forma em que dois textos se comparam ([`comparable`]).
/// Só o arquivo em comum não basta: a sobra diferente nos mesmos arquivos
/// entra na tarefa como linha ([`join`]).
fn repeats(own: &BTreeSet<String>, title: Option<&str>, text: Option<&str>, files: &BTreeSet<String>, leftover: &Leftover) -> bool {
    let same = |own: Option<&str>, said: &str| {
        let said = comparable(said);
        !said.is_empty() && own.is_some_and(|own| comparable(own) == said)
    };
    own == files && (same(title, &leftover.title) || same(text, &leftover.detail))
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

/// O pedido que a sobra `leftover` vira: o título e o relato dela, para
/// ondas novas, com o título e os arquivos `files` como chaves de busca, e a
/// entrega `origin` que a relatou, quando se sabe qual.
fn leftover_request(leftover: &Leftover, files: &BTreeSet<String>, origin: Option<u64>) -> Map<String, Value> {
    let keys: Vec<&str> = std::iter::once(leftover.title.as_str()).chain(files.iter().map(String::as_str)).collect();
    let mut request = json!({ "title": leftover.title, "text": leftover.detail, "effect": "new_waves", "keys": keys, "author": "wave" });
    if let Some(origin) = origin {
        request["origin"] = json!(origin);
    }
    request.as_object().cloned().unwrap_or_default()
}

/// Junta a sobra `leftover`, apontada pela onda `wave`, à versão `draft` de
/// uma tarefa: a parte do agente ganha uma linha com o título e o detalhe
/// dela, numa linha só, no idioma `lang`; e a marca de limpeza fica só quando
/// a sobra também a traz — a tarefa que passa a mudar comportamento não
/// espera o fim da obra. Os critérios da tarefa ficam como estão.
fn join(draft: &mut Map<String, Value>, wave: u64, leftover: &Leftover, lang: Locale) {
    let line = translate("round.leftover_joined", lang)
        .replace("{wave}", &wave.to_string())
        .replace("{title}", &leftover.title)
        .replace("{detail}", &one_line(&leftover.detail));
    append_agent_line(draft, &line);
    if !leftover.cleanup {
        draft.remove(CLEANUP);
    }
}

/// Os arquivos da sobra `leftover`, apontada pela onda `wave`, sem repetir:
/// os que ela declara em `files`, quando traz a lista; sem a lista, os que o
/// detalhe cita entre crases e que existem no repositório `root` ou na cópia
/// da onda, lida em `log` — o exemplo citado no texto não vira arquivo da
/// tarefa quando a lista existe. O caminho de dentro da cópia vira o
/// relativo ao repositório, e o absoluto de fora dela não entra.
fn leftover_files(root: &Path, log: &SpecLog, wave: u64, leftover: &Leftover) -> Vec<String> {
    let copy = wave_prompt::recorded_copy(log, wave).map(|copy| Path::new(&copy.path).to_path_buf());
    let on_disk = |path: &str| root.join(path).exists() || copy.as_ref().is_some_and(|copy| copy.join(path).exists());
    let (cited, check): (Vec<String>, bool) = match &leftover.files {
        Some(declared) => (declared.clone(), false),
        None => (cited_paths(&leftover.detail), true),
    };
    let mut files: Vec<String> = Vec::new();
    for cited in cited {
        let path = own_copy_relative(log, wave, &cited.replace('\\', "/"));
        if !path.is_empty() && !Path::new(&path).is_absolute() && (!check || on_disk(&path)) && !files.contains(&path) {
            files.push(path);
        }
    }
    files
}

/// A tarefa nova que a sobra vira: o título e o detalhe dela, sem
/// dependência, com o autor da onda, e os arquivos dela
/// ([`leftover_files`]). Ela não herda os critérios da onda que a apontou:
/// quem a liga ao pedido da sobra é a rodada, ao gravar os dois. A sobra
/// marcada como limpeza dá a tarefa com a mesma marca; a sem marca, a tarefa
/// sem o campo. A sobra sem arquivo nenhum dá a tarefa presa (`blocked_by`),
/// com o motivo no idioma `lang`: um agente partiria sem saber o que tocar,
/// e a revisão não teria escopo a conferir. A fila não a despacha até uma
/// versão nova, gravada por quem conduz a obra com os arquivos e as
/// dependências, sair sem a trava.
fn leftover_task(root: &Path, log: &SpecLog, wave: u64, leftover: &Leftover, lang: Locale) -> Map<String, Value> {
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
        Leftover { title: title.into(), detail: detail.into(), files: None, cleanup }
    }

    /// A spec `x` gravada à mão com as linhas `events`, já lida.
    fn log_of(events: &[Value]) -> SpecLog {
        parse_log(&events.iter().map(Value::to_string).collect::<Vec<_>>().join("\n"))
    }

    /// Os eventos do tipo `kind` entre os que as sobras viraram.
    fn of_kind<'a>(events: &'a [LeftoverEvent], kind: &str) -> Vec<&'a LeftoverEvent> {
        events.iter().filter(|event| event.kind == kind).collect()
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
    /// lista que a parte do agente já trazia, com o detalhe numa linha só, e
    /// a versão passa a cobrir o pedido de cada uma, sem a marca de limpeza
    /// quando uma delas não é limpeza; a sobra que cita arquivo novo vira
    /// tarefa nova, e a que cita só os arquivos dessa tarefa nova vira linha
    /// nela, sem outra tarefa. Os pedidos vêm antes das tarefas. A marca de
    /// limpeza da tarefa aberta fica quando a sobra também é limpeza.
    #[test]
    fn leftovers_on_the_files_of_an_open_task_join_it_instead_of_making_a_new_one() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for file in ["src/commit.rs", "src/novo.rs"] {
            std::fs::write(root.join(file), "fn one() {}\n").unwrap();
        }
        let log = log_of(&[
            json!({"v":1,"id":1,"type":"task","title":"O índice some","text":"Some.","agent":"- Olhe o índice.",
                "files":[{"path":"src/commit.rs"}, {"path":"src/c.rs"}],"depends_on":[],"cleanup":true}),
        ]);
        let first = leftover("Para sem índice", "Em `src/commit.rs`,\n  para.", false);
        let fresh = leftover("Arquivo novo", "Em `src/novo.rs`.", false);
        let fresh_again = leftover("Arquivo novo de novo", "De novo `src/novo.rs`.", true);
        let second = leftover("Para outra vez", "Ainda `src/commit.rs`.", true);
        let found = [(1, &first), (1, &fresh), (1, &fresh_again), (1, &second)];

        let events = leftover_events(root, &log, &found, &BTreeSet::new(), &BTreeMap::new(), Locale::PtBr);
        let kinds: Vec<&str> = events.iter().map(|event| event.kind).collect();
        assert_eq!(kinds, [REQUEST, REQUEST, REQUEST, REQUEST, TASK, TASK]);
        let joined = &events[4];
        assert_eq!(joined.draft.get("replaces"), Some(&json!(1)), "{:?}", joined.draft);
        let agent = format!(
            "- Olhe o índice.\n- {}\n- {}",
            line("Para sem índice", "Em `src/commit.rs`, para."),
            line("Para outra vez", "Ainda `src/commit.rs`.")
        );
        assert_eq!(joined.draft.get("agent"), Some(&json!(agent)), "{:?}", joined.draft);
        assert_eq!(joined.asks, [0, 3], "a tarefa cobre o pedido de cada sobra que entrou nela");
        assert!(!joined.draft.contains_key(CLEANUP), "uma sobra muda comportamento: {:?}", joined.draft);
        let born = &events[5];
        assert!(!born.draft.contains_key("replaces"), "{:?}", born.draft);
        assert_eq!(born.draft.get("title"), Some(&json!("Arquivo novo")), "{:?}", born.draft);
        assert_eq!(born.draft.get("files"), Some(&json!([{"path": "src/novo.rs"}])), "{:?}", born.draft);
        assert_eq!(born.draft.get("agent"), Some(&json!(line("Arquivo novo de novo", "De novo `src/novo.rs`."))), "{:?}", born.draft);
        assert_eq!(born.asks, [1, 2]);

        let tidy = leftover("Comentário velho", "Em `src/commit.rs`.", true);
        let events = leftover_events(root, &log, &[(1, &tidy)], &BTreeSet::new(), &BTreeMap::new(), Locale::PtBr);
        let tasks = of_kind(&events, TASK);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].draft.get("replaces"), Some(&json!(1)), "{:?}", tasks[0].draft);
        assert_eq!(tasks[0].draft.get(CLEANUP), Some(&json!(true)), "limpeza sobre limpeza segue limpeza: {:?}", tasks[0].draft);
    }

    /// A onda que relata as sobras tem critério. A tarefa nova que uma sobra
    /// vira não cobre esse critério, e a tarefa aberta que recebe outra sobra
    /// segue cobrindo só o que já cobria: cada uma passa a cobrir só o pedido
    /// da própria sobra.
    #[test]
    fn a_leftover_task_never_inherits_the_wave_criteria() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        for file in ["src/a.ts", "src/b.ts"] {
            std::fs::write(root.join(file), "export const one = 1;\n").unwrap();
        }
        let log = log_of(&[
            json!({"v":1,"id":1,"type":"criterion","when":"a","then":"b","proof":"npm test -- a","form":"ubiquitous"}),
            json!({"v":1,"id":2,"type":"criterion","when":"c","then":"d","proof":"npm test -- b","form":"ubiquitous"}),
            json!({"v":1,"id":3,"type":"wave","n":1,"text":"t","criteria":[1],"done_when":"d"}),
            json!({"v":1,"id":4,"type":"task","title":"B","text":"B.","files":[{"path":"src/b.ts"}],"depends_on":[],"covers":[2]}),
        ]);
        let born = Leftover { title: "A some".into(), detail: "Some.".into(), files: Some(vec!["src/a.ts".into()]), cleanup: false };
        let joining = Leftover { title: "B some".into(), detail: "Some.".into(), files: Some(vec!["src/b.ts".into()]), cleanup: false };

        let events = leftover_events(root, &log, &[(1, &born), (1, &joining)], &BTreeSet::new(), &BTreeMap::new(), Locale::PtBr);
        let tasks = of_kind(&events, TASK);
        assert_eq!(tasks.len(), 2);
        let new = tasks.iter().find(|task| !task.draft.contains_key("replaces")).unwrap();
        assert!(!new.draft.contains_key("covers"), "a tarefa nova não herda o critério da onda: {:?}", new.draft);
        assert_eq!(new.asks, [0]);
        let open = tasks.iter().find(|task| task.draft.contains_key("replaces")).unwrap();
        assert_eq!(open.draft.get("covers"), Some(&json!([2])), "a tarefa aberta não ganha o critério da onda: {:?}", open.draft);
        assert_eq!(open.asks, [1]);
    }

    /// A sobra que declara os arquivos a mudar dá a tarefa só com eles: os
    /// caminhos que o texto cita como exemplo, mesmo existindo, ficam de
    /// fora, e o caminho absoluto de dentro da cópia vira o relativo.
    #[test]
    fn a_leftover_with_declared_files_ignores_the_paths_its_text_cites() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for file in ["src/soma.ts", "web/layout.css", "scripts/install.ps1", "apps/rt/Cargo.toml"] {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), "x\n").unwrap();
        }
        let log = log_of(&[json!({"v":1,"id":1,"type":"wave","n":1,"text":"t","criteria":[],"done_when":"d"})]);
        let detail = "O acerto errado aparece em `web/layout.css`, `scripts/install.ps1` e `apps/rt/Cargo.toml`; o conserto é em `src/soma.ts`.";
        let declared = Leftover { title: "Acerto errado".into(), detail: detail.into(), files: Some(vec!["src/soma.ts".into()]), cleanup: false };

        let events = leftover_events(root, &log, &[(1, &declared)], &BTreeSet::new(), &BTreeMap::new(), Locale::PtBr);
        let task = of_kind(&events, TASK)[0];
        assert_eq!(task.draft.get("files"), Some(&json!([{"path": "src/soma.ts"}])), "{:?}", task.draft);
        assert!(!task.draft.contains_key("blocked_by"), "{:?}", task.draft);

        let cited = leftover("Acerto errado", detail, false);
        let events = leftover_events(root, &log, &[(1, &cited)], &BTreeSet::new(), &BTreeMap::new(), Locale::PtBr);
        let files = of_kind(&events, TASK)[0].draft.get("files").and_then(Value::as_array).map_or(0, Vec::len);
        assert_eq!(files, 4, "sem a lista, o texto ainda dá os arquivos");
    }

    /// A onda 1, com critério, volta com uma sobra que declara um arquivo
    /// TypeScript. A rodada grava um pedido com o título e o relato dela,
    /// para ondas novas, que aponta a entrega da onda como origem, e uma
    /// tarefa que cobre só esse pedido, pelo número gravado.
    #[test]
    fn a_leftover_becomes_a_request_and_a_task_covering_it() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/soma.ts"), "export const soma = 1;\n").unwrap();
        approved(root, "x", &[(1, &["src/a.rs"], &[])]);
        git_at(root, &["checkout", "-q", "-b", "feature/x"]);
        round(root, "x", None);
        std::fs::write(root.join("src/a.rs"), "fn one() {}\n// A soma saiu.\n").unwrap();
        let detail = "A soma perde o sinal. Prova: `npm test -- soma`.";
        let wrote = returned(
            root,
            json!({"wave": 1, "text": "A soma saiu.", "files": ["src/a.rs"], "commit": "a soma sai",
                "leftovers": [{"title": "A soma perde o sinal", "detail": detail, "files": ["src/soma.ts"]}]}),
        );
        assert_eq!(wrote["ok"], json!(true), "{wrote}");
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        let requests = recorded(root, &out, REQUEST);
        assert_eq!(requests.len(), 1, "{out}");
        let request = &requests[0];
        assert_eq!(request.str_field("title"), Some("A soma perde o sinal"), "{:?}", request.fields);
        assert_eq!(request.str_field("text"), Some(detail), "{:?}", request.fields);
        assert_eq!(request.str_field("effect"), Some("new_waves"), "{:?}", request.fields);
        let delivered = recorded(root, &out, "delivered");
        assert_eq!(request.int("origin"), Some(delivered[0].id), "o pedido nasce da entrega: {:?}", request.fields);
        let tasks = recorded(root, &out, TASK);
        assert_eq!(tasks.len(), 1, "{out}");
        assert_eq!(tasks[0].fields["covers"], json!([request.id]), "a tarefa cobre só o pedido da sobra: {:?}", tasks[0].fields);
        assert_eq!(tasks[0].fields["files"], json!([{"path": "src/soma.ts"}]), "{:?}", tasks[0].fields);
    }

    /// As ondas 1 e 2 voltam juntas, antes da mesma rodada, com a mesma sobra
    /// sem arquivo. A rodada grava um pedido e uma tarefa só, presa a
    /// planejar, e a sobra da onda 2 entra nela como linha.
    #[test]
    fn the_same_leftover_from_two_returns_in_one_round_makes_one_task() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        approved(root, "x", &[(1, &["src/a.rs"], &[]), (2, &["src/b.rs"], &[])]);
        git_at(root, &["checkout", "-q", "-b", "feature/x"]);
        assert_eq!(waves_in(&round(root, "x", None), "dispatch"), vec![1, 2]);
        let bare = json!({"title": "O disco enche", "detail": "As cópias lotam o disco durante a obra."});
        for (wave, file) in [(1, "src/a.rs"), (2, "src/b.rs")] {
            std::fs::write(root.join(file), "fn one() {}\n// Saiu.\n").unwrap();
            let wrote = returned(
                root,
                json!({"wave": wave, "text": "Saiu.", "files": [file], "commit": "sai", "leftovers": [bare]}),
            );
            assert_eq!(wrote["ok"], json!(true), "{wrote}");
        }
        let out = round(root, "x", None);
        assert_eq!(out["ok"], json!(true), "{out}");
        assert_eq!(recorded(root, &out, REQUEST).len(), 1, "{out}");
        let tasks = recorded(root, &out, TASK);
        assert_eq!(tasks.len(), 1, "{out}");
        assert!(tasks[0].str_field("blocked_by").is_some(), "{:?}", tasks[0].fields);
        let joined = translate("round.leftover_joined", Locale::PtBr)
            .replace("{wave}", "2")
            .replace("{title}", "O disco enche")
            .replace("{detail}", "As cópias lotam o disco durante a obra.");
        assert!(tasks[0].str_field("agent").is_some_and(|agent| agent.contains(&joined)), "{:?}", tasks[0].fields);
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
