//! `lessons` — o banco de lições (`.claude/spec/lessons.ndjson`).
//!
//! Uma lição é o que o projeto aprendeu e não pode esquecer: um defeito que
//! pode se repetir (`defect`), uma regra do projeto (`project_rule`), uma
//! armadilha do ambiente (`environment_trap`) ou uma preferência do usuário
//! (`user_preference`). O banco fica fora das pastas das specs e é escrito só
//! pelo binário, pelo `write lesson`. Ele mora só na máquina de quem programa
//! e não vai ao git: por isso o `write lesson` recusa a lição de defeito
//! ([`is_defect`]), que viraria regra presa numa máquina só. O defeito que
//! pode se repetir vira conserto no código, com o teste que falha se ele
//! voltar, e esse conserto vai ao git com a obra. As lições de defeito já
//! guardadas seguem na leitura, e a retirada as tira como qualquer outra.
//! Cada linha tem o mesmo envelope dos
//! eventos da spec (`v`, `id`, `at`, `type`, `author`), sem código de item e
//! sem `origin`: o `type` guarda a classe da lição, e a lição é apontada pelo
//! número dela no banco.
//!
//! Toda lição diz onde vale (`applies_to`: o subprojeto, os arquivos ou a
//! skill) e onde nasceu (`found_in`: a spec, a branch e o commit, ou o arquivo
//! de onde veio, em `source`). A lição que vale no projeto todo diz isso com
//! os arquivos `["**"]`, e é achada para qualquer caminho, subprojeto ou
//! skill.
//!
//! Duas buscas, só do Rust: por escopo ([`in_scope`]), pelo caminho dos
//! arquivos, pelo subprojeto e pela skill; e por palavras ([`matching`]), com
//! o BM25 de `domain::search` sobre o `search`, que devolve as 5 mais fortes.
//! O pedido de uma onda leva, de cada classe, só as lições mais ligadas às
//! tarefas ([`related_to_tasks`]), porque uma pasta pode ter
//! centenas delas: a mesma busca, mas sobre as palavras-chave de cada lição,
//! e não sobre o texto inteiro, em que quase toda lição longa divide alguma
//! palavra comum com qualquer tarefa. Delas, ficam só as que servem à onda
//! ([`serving_wave`]): a que cita um arquivo vai só à onda que mexe nele, e a
//! onda só de texto ([`text_only`]) não recebe lição do projeto todo nem do
//! subprojeto. O item combinado sem dono é escolhido para a onda pelas mesmas
//! duas leituras ([`tied_to_wave`]): as palavras-chave dele ligadas às
//! tarefas, ou o arquivo que ele cita e a onda mexe.
//! Quem mostra uma lição mostra o texto original ([`shown`]), nunca o
//! `search`.
//!
//! Uma lição nunca entra repetida: a que tem o mesmo texto de outra já
//! guardada, depois de igualar espaços, maiúsculas e acentos
//! ([`comparable`]), é recusada apontando a que existe ([`repeated`]). A
//! gravação do assistente e a importação das instruções do projeto passam
//! pela mesma comparação.
//!
//! O banco se enxuga pelo mesmo gravador. A lição que junta outras numa só
//! aponta todas elas em `replaces`, e as antigas saem da leitura, como a
//! versão nova de uma lição. A lição que já não vale sai por uma linha de
//! retirada ([`RETIRE`]), com `targets` e `reason`, do mesmo jeito que o
//! `remove` tira um item da spec: a linha fica no arquivo, e a lição some da
//! leitura. A retirada não aceita outro campo, nem o filtro por hora nem a
//! substituição do `remove` da spec: só sai da leitura a lição que ela
//! aponta, e a saída diz qual. Só se junta ou retira a lição que a leitura
//! ainda mostra.
//!
//! O scan aponta o que enxugar e não muda o banco: os grupos de lições
//! parecidas ([`similar`]), pela mesma busca por palavras-chave, entre lições
//! da mesma classe e do mesmo lugar; e as lições que citam um caminho que o
//! projeto já não tem ([`citing_missing_paths`]).
//!
//! Função pura: sem disco e sem relógio. A gravação mora em `io::lessons`.

use serde_json::{Map, Value};

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::config::glob_matches;
use crate::domain::project_map::{cited_paths, written_paths};
use crate::domain::search::{self, Hit, SearchIndex};
use crate::domain::text::fold;
use crate::domain::spec_events::{
    check_field, is_empty, opt, req, search_terms, shown_line, Kind, Refusal, SpecEvent, SpecLog, AUTHORS,
    BINARY_FIELDS, DEFAULT_AUTHOR, PURGED_FIELD, REFUSED_FIELDS,
};

/// O tipo com que o `write` recebe uma lição.
pub const LESSON: &str = "lesson";

/// As classes de lição, gravadas no `type` da linha. A de defeito ([`DEFECT`])
/// continua na lista porque as linhas antigas dela seguem válidas na leitura;
/// quem a recusa na gravação é o `write lesson`.
pub const CLASSES: &[&str] = &[DEFECT, "project_rule", "environment_trap", "user_preference"];

/// A classe do defeito que pode se repetir.
pub const DEFECT: &str = "defect";

/// O rascunho que o `write lesson` recebe grava uma lição de defeito, sozinha
/// ou juntando outras em `replaces`: a classe dele (`class`) é [`DEFECT`]. A
/// retirada, que não traz classe, nunca é.
#[must_use]
pub fn is_defect(draft: &Map<String, Value>) -> bool {
    draft.get("class").and_then(Value::as_str).map(str::trim) == Some(DEFECT)
}

/// O tipo da linha que retira lições do banco: o mesmo `remove` da spec, com
/// as lições em `targets` e o motivo em `reason`. Quem grava pelo `write
/// lesson` manda só esses dois campos, sem `class`.
pub const RETIRE: &str = "remove";

/// Os campos que a retirada aceita: as lições que saem, o motivo e o autor,
/// além do tipo que o binário põe. O `filter` e o `replaces` que o `remove`
/// da spec aceita tirariam da leitura lições que a conferência não olhou e
/// que a saída não diz; por isso a retirada os recusa pelo nome.
const RETIRE_FIELDS: &[&str] = &["targets", "reason", "author"];

/// O padrão de arquivos da lição que vale no projeto todo.
pub const WHOLE_PROJECT: &str = "**";

/// Os campos de `applies_to`: onde a lição vale.
const SCOPE_FIELDS: &[(&str, Kind)] = &[("subproject", Kind::Text), ("files", Kind::Texts), ("skill", Kind::Text)];

/// Os campos de `found_in`: onde a lição nasceu.
const ORIGIN_FIELDS: &[&str] = &["spec", "branch", "commit", "source"];

/// O rascunho de quem grava, pronto para a conferência: sem os campos que só
/// o binário escreve, com a classe (`class`) no `type`, com o autor (o
/// assistente, quando quem grava não diz) e, quando o `write` recebeu uma
/// spec, com ela em `found_in.spec`, se faltava. O rascunho sem classe que
/// aponta lições em `targets` é uma retirada ([`RETIRE`]), e não ganha
/// `found_in`.
#[must_use]
pub fn normalize(mut draft: Map<String, Value>, spec: Option<&str>) -> Map<String, Value> {
    for field in BINARY_FIELDS.iter().filter(|f| !REFUSED_FIELDS.contains(f)) {
        draft.remove(*field);
    }
    draft.remove(PURGED_FIELD);
    let class = draft.remove("class");
    draft.remove("type");
    if let Some(class) = class {
        draft.insert("type".into(), class);
    } else if draft.contains_key("targets") {
        draft.insert("type".into(), Value::from(RETIRE));
    }
    if draft.get("author").is_none_or(is_empty) {
        draft.insert("author".into(), Value::String(DEFAULT_AUTHOR.into()));
    }
    if is_retirement(&draft) {
        return draft;
    }
    if let Some(spec) = spec.map(str::trim).filter(|s| !s.is_empty()) {
        match draft.get_mut("found_in") {
            Some(Value::Object(found)) => {
                if found.get("spec").is_none_or(is_empty) {
                    found.insert("spec".into(), Value::from(spec));
                }
            }
            None | Some(Value::Null) => {
                let mut found = Map::new();
                found.insert("spec".into(), Value::from(spec));
                draft.insert("found_in".into(), Value::Object(found));
            }
            Some(_) => {}
        }
    }
    draft
}

/// Confere uma lição sozinha: a classe é uma das quatro, o texto e as chaves
/// estão preenchidos, ela diz onde vale e onde nasceu, e cada campo tem a
/// forma certa. O que depende do banco (a lição substituída existe) fica para
/// [`check_against`].
pub fn validate(event: &Map<String, Value>) -> Result<(), Refusal> {
    if let Some(field) = REFUSED_FIELDS.iter().find(|f| event.contains_key(**f)) {
        return Err(Refusal::BinaryOnlyField { field: (*field).to_string() });
    }
    if is_retirement(event) {
        for field in [req("author", Kind::OneOf(AUTHORS)), req("targets", Kind::Ints), req("reason", Kind::Text)] {
            check_field(event, LESSON, field)?;
        }
        // A retirada tira da leitura só as lições de `targets`, que a
        // conferência contra o banco olha e a saída devolve: qualquer outro
        // campo é recusado pelo nome.
        if let Some(extra) = event.keys().find(|key| *key != "type" && !RETIRE_FIELDS.contains(&key.as_str())) {
            return Err(Refusal::UnknownField {
                event_type: RETIRE.to_string(),
                field: extra.clone(),
                accepted: RETIRE_FIELDS.join(", "),
            });
        }
        return Ok(());
    }
    match event.get("type") {
        Some(class) if !is_empty(class) => {
            if !Kind::OneOf(CLASSES).accepts(class) {
                return Err(invalid("class", Kind::OneOf(CLASSES)));
            }
        }
        _ => return Err(missing("class")),
    }
    for field in [
        req("author", Kind::OneOf(AUTHORS)),
        req("text", Kind::Text),
        req("keys", Kind::Texts),
        opt("label", Kind::Text),
    ] {
        check_field(event, LESSON, field)?;
    }
    check_replaces(event)?;
    check_applies_to(event)?;
    check_found_in(event)
}

/// A linha é uma retirada de lições, e não uma lição.
#[must_use]
pub fn is_retirement(event: &Map<String, Value>) -> bool {
    event.get("type").and_then(Value::as_str) == Some(RETIRE)
}

/// As lições que a gravação tira da leitura: as que a lição nova substitui
/// ou junta (`replaces`, um número ou uma lista) ou as que a retirada aponta
/// (`targets`).
#[must_use]
pub fn hidden_by(event: &Map<String, Value>) -> Vec<u64> {
    let field = if is_retirement(event) { "targets" } else { "replaces" };
    match event.get(field) {
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_u64).collect(),
        Some(one) => one.as_u64().into_iter().collect(),
        None => Vec::new(),
    }
}

/// `replaces`, quando vem, é o número de uma lição ou a lista, não vazia, das
/// lições que a nova junta numa só.
fn check_replaces(event: &Map<String, Value>) -> Result<(), Refusal> {
    match event.get("replaces") {
        None | Some(Value::Null) => Ok(()),
        Some(value) if Kind::Int.accepts(value) => Ok(()),
        Some(value) if Kind::Ints.accepts(value) && !is_empty(value) => Ok(()),
        Some(_) => Err(invalid("replaces", Kind::Ints)),
    }
}

fn missing(field: &str) -> Refusal {
    Refusal::MissingField { event_type: LESSON.to_string(), field: field.to_string() }
}

fn invalid(field: &str, expected: Kind) -> Refusal {
    Refusal::InvalidValue { event_type: LESSON.to_string(), field: field.to_string(), expected }
}

/// Um campo de dentro de `owner`, quando veio, tem a forma `kind`.
fn check_inner(object: &Map<String, Value>, owner: &str, name: &str, kind: Kind) -> Result<(), Refusal> {
    match object.get(name) {
        Some(value) if !is_empty(value) && !kind.accepts(value) => Err(invalid(&format!("{owner}.{name}"), kind)),
        _ => Ok(()),
    }
}

/// Onde a lição vale: um objeto com pelo menos o subprojeto, os arquivos ou
/// a skill. Sem exceção: a que vale no projeto todo diz os arquivos `["**"]`.
fn check_applies_to(event: &Map<String, Value>) -> Result<(), Refusal> {
    let Some(value) = event.get("applies_to").filter(|v| !is_empty(v)) else {
        return Err(missing("applies_to"));
    };
    let Some(scope) = value.as_object() else {
        return Err(invalid("applies_to", Kind::Object));
    };
    for (name, kind) in SCOPE_FIELDS {
        check_inner(scope, "applies_to", name, *kind)?;
    }
    if SCOPE_FIELDS.iter().all(|(name, _)| scope.get(*name).is_none_or(is_empty)) {
        return Err(missing("applies_to"));
    }
    Ok(())
}

/// Onde a lição nasceu: a spec, a branch e o commit, ou o arquivo de onde ela
/// veio, com pelo menos um deles.
fn check_found_in(event: &Map<String, Value>) -> Result<(), Refusal> {
    let Some(value) = event.get("found_in").filter(|v| !is_empty(v)) else {
        return Err(Refusal::LessonOriginMissing);
    };
    let Some(found) = value.as_object() else {
        return Err(invalid("found_in", Kind::Object));
    };
    for name in ORIGIN_FIELDS {
        check_inner(found, "found_in", name, Kind::Text)?;
    }
    if ORIGIN_FIELDS.iter().all(|name| found.get(*name).is_none_or(is_empty)) {
        return Err(Refusal::LessonOriginMissing);
    }
    Ok(())
}

/// Confere a lição contra o banco como está: cada lição que ela substitui ou
/// junta (`replaces`), ou que a retirada aponta, é uma que a leitura mostra,
/// e nenhuma outra lição guardada tem o mesmo texto. As que ela substitui não
/// contam: a versão nova pode só pôr os acentos.
pub fn check_against(bank: &SpecLog, event: &Map<String, Value>) -> Result<(), Refusal> {
    let gone = hidden_by(event);
    let shown: BTreeSet<u64> = kept(bank).iter().map(|lesson| lesson.id).collect();
    if let Some(id) = gone.iter().copied().find(|id| !shown.contains(id)) {
        return Err(Refusal::UnknownLesson { id });
    }
    if is_retirement(event) {
        return Ok(());
    }
    let text = event.get("text").and_then(Value::as_str).unwrap_or_default();
    match repeated(bank, text, &gone) {
        Some(same) => Err(Refusal::LessonRepeated {
            id: same.id,
            text: same.str_field("text").unwrap_or_default().to_string(),
        }),
        None => Ok(()),
    }
}

/// O texto na forma em que duas lições se comparam: minúsculas, sem acento e
/// com um espaço só entre as palavras. `"Não  REPITA"` e `"nao repita"` são o
/// mesmo texto.
#[must_use]
pub fn comparable(text: &str) -> String {
    fold(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A lição vigente do banco cujo texto é o mesmo de `text`, pela forma de
/// [`comparable`], de qualquer classe. `except` são as lições que a nova
/// substitui, que não contam. Texto vazio não repete nada.
#[must_use]
pub fn repeated<'a>(bank: &'a SpecLog, text: &str, except: &[u64]) -> Option<&'a SpecEvent> {
    let wanted = comparable(text);
    if wanted.is_empty() {
        return None;
    }
    kept(bank)
        .into_iter()
        .filter(|lesson| !except.contains(&lesson.id))
        .find(|lesson| lesson.str_field("text").is_some_and(|own| comparable(own) == wanted))
}

/// As lições que a leitura do banco mostra, em ordem: as das quatro classes
/// que nenhuma versão nova substituiu e nenhuma retirada tirou. A linha da
/// retirada não é lição.
#[must_use]
pub fn kept(bank: &SpecLog) -> Vec<&SpecEvent> {
    bank.visible().into_iter().filter(|lesson| CLASSES.contains(&lesson.event_type.as_str())).collect()
}

/// Onde se procura lição: os arquivos, o subprojeto e a skill de uma onda.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub files: Vec<String>,
    pub subproject: Option<String>,
    pub skill: Option<String>,
}

/// As lições vigentes que valem para `scope`, em ordem de número. Entra a
/// lição do projeto todo e toda lição em que um arquivo do escopo casa um
/// padrão dos arquivos dela, em que o subprojeto é o dela ou um arquivo do
/// escopo fica dentro dele, ou em que a skill é a dela.
#[must_use]
pub fn in_scope<'a>(bank: &'a SpecLog, scope: &Scope) -> Vec<&'a SpecEvent> {
    let mut found: Vec<&SpecEvent> = kept(bank).into_iter().filter(|lesson| applies(lesson, scope)).collect();
    found.sort_by_key(|lesson| lesson.id);
    found
}

/// A classe da lição que guarda uma regra do projeto: vale sempre, então
/// não vira pergunta no levantamento.
pub const PROJECT_RULE: &str = "project_rule";

/// O "onde vale" de um evento casa com `scope`? A mesma leitura serve à lição
/// e ao item combinado, que declaram o campo do mesmo jeito: sem ela, o
/// recorte dos itens por onda e a busca de lições discordariam sobre o mesmo
/// campo.
#[must_use]
pub fn applies_to(event: &SpecEvent, scope: &Scope) -> bool {
    applies(event, scope)
}

fn applies(lesson: &SpecEvent, scope: &Scope) -> bool {
    let Some(at) = lesson.fields.get("applies_to").and_then(Value::as_object) else {
        return false;
    };
    let patterns = file_patterns(at);
    if patterns.iter().any(|p| whole_project(p)) {
        return true;
    }
    let files: Vec<String> = scope.files.iter().map(|f| clean_path(f)).filter(|f| !f.is_empty()).collect();
    if files.iter().any(|file| patterns.iter().any(|p| path_matches(p, file))) {
        return true;
    }
    if let Some(sub) = at.get("subproject").and_then(Value::as_str).map(clean_path).filter(|s| !s.is_empty()) {
        if scope.subproject.as_deref().map(clean_path).is_some_and(|s| s == sub) {
            return true;
        }
        if files.iter().any(|file| path_matches(&sub, file)) {
            return true;
        }
    }
    let skill = at.get("skill").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    skill.is_some_and(|skill| scope.skill.as_deref().map(str::trim) == Some(skill))
}

/// Os padrões de arquivo de um `applies_to`, já com barras normais.
fn file_patterns(at: &Map<String, Value>) -> Vec<String> {
    at.get("files")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(clean_path).collect())
        .unwrap_or_default()
}

/// O padrão que vale no projeto todo: só curingas, como `**`.
fn whole_project(pattern: &str) -> bool {
    !pattern.is_empty() && pattern.chars().all(|c| c == '*')
}

/// O caminho com barras normais, sem `./` no começo e sem barra no fim.
fn clean_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let path = path.strip_prefix("./").unwrap_or(&path);
    path.trim_end_matches('/').to_string()
}

/// `file` casa o padrão com `*`, ou é o próprio caminho, ou fica dentro dele.
fn path_matches(pattern: &str, file: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if pattern.contains('*') {
        return glob_matches(pattern, file);
    }
    file == pattern || file.strip_prefix(pattern).is_some_and(|rest| rest.starts_with('/'))
}

/// As lições vigentes cujo `search` casa as palavras do pedido, as 5 mais
/// fortes, pelo BM25.
#[must_use]
pub fn matching(bank: &SpecLog, words: &str) -> Vec<Hit> {
    matching_among(&kept(bank), words)
}

/// A mesma busca de [`matching`], só entre `lessons`: quem já separou as
/// lições que interessam não deixa as outras tomarem o lugar delas entre as
/// 5 mais fortes.
#[must_use]
pub fn matching_among(lessons: &[&SpecEvent], words: &str) -> Vec<Hit> {
    search::search(lessons.iter().map(|lesson| (lesson.id, lesson.str_field("search").unwrap_or_default())), words)
}

/// As lições que o pedido de uma onda leva, entre as `found`
/// que valem para ela: de cada classe, só as 5 mais ligadas às palavras das
/// tarefas (`words`), pela busca de [`matching`] feita sobre as palavras-chave
/// de cada lição ([`by_keys`]), e não sobre o texto dela. A lição sem
/// palavra-chave em comum com as tarefas fica fora, seja qual for a classe,
/// mesmo que o texto dela divida palavras com elas. Em ordem de número. A
/// mesma escolha serve ao item combinado sem dono ([`tied_to_wave`]), que diz
/// as palavras-chave do mesmo jeito; a classe dele é o tipo do item.
#[must_use]
pub fn related_to_tasks<'a>(found: Vec<&'a SpecEvent>, words: &str) -> Vec<&'a SpecEvent> {
    let mut by_class: BTreeMap<&str, Vec<&SpecEvent>> = BTreeMap::new();
    for lesson in found {
        by_class.entry(lesson.event_type.as_str()).or_default().push(lesson);
    }
    let mut taken: Vec<&SpecEvent> = Vec::new();
    for lessons in by_class.into_values() {
        let related = by_keys(&lessons, words);
        taken.extend(lessons.into_iter().filter(|lesson| related.iter().any(|hit| hit.id == lesson.id)));
    }
    taken.sort_by_key(|lesson| lesson.id);
    taken
}

/// As lições de `lessons` que servem à onda que mexe em `files`, com as
/// skills `skills`, na mesma ordem. Duas regras tiram lição:
///
/// - a lição cujo texto cita arquivo — caminho com extensão, pela leitura de
///   [`cited_paths`] — só fica se a onda mexe num desses arquivos
///   ([`same_file`]); a pasta citada não conta, porque uma pasta casaria com
///   quase toda onda do lugar dela;
/// - a onda só de texto ([`text_only`]) não recebe a lição do projeto todo
///   nem a do subprojeto: fica só a que casa um arquivo dela por um padrão
///   que não é o do projeto todo, ou a da skill que as tarefas nomeiam.
///
/// É o que o pedido de uma onda leva, depois da escolha por palavras-chave
/// ([`related_to_tasks`]).
#[must_use]
pub fn serving_wave<'a>(lessons: Vec<&'a SpecEvent>, files: &[String], skills: &[String]) -> Vec<&'a SpecEvent> {
    let files: Vec<String> = files.iter().map(|f| clean_path(f)).filter(|f| !f.is_empty()).collect();
    let text = text_only(&files);
    lessons
        .into_iter()
        .filter(|lesson| touches_cited_file(lesson, &files))
        .filter(|lesson| !text || by_pattern_or_skill(lesson, &files, skills))
        .collect()
}

/// A onda só de texto: todos os arquivos dela são markdown (`.md`), texto
/// (`.txt`) ou arquivo de ignorar (`.gitignore`, `.dockerignore`). A lista
/// é fechada de propósito: a view, a página, o SQL, o script e o estilo
/// ficam fora dela e recebem lição como código. A onda sem arquivo nenhum
/// não é só de texto: sem saber o que ela toca, ela recebe o que casar.
#[must_use]
pub fn text_only(files: &[String]) -> bool {
    !files.is_empty() && files.iter().all(|file| text_file(file))
}

/// O arquivo é markdown, texto ou arquivo de ignorar, pelo nome.
fn text_file(path: &str) -> bool {
    let path = clean_path(path);
    let name = path.rsplit('/').next().unwrap_or_default().to_lowercase();
    name.ends_with(".md") || name.ends_with(".txt") || (name.starts_with('.') && name.ends_with("ignore"))
}

/// `true` quando o arquivo que um texto cita é `file`: o mesmo caminho ou o
/// fim dele (`spec_events/mod.rs`), porque o texto pode citar o caminho a
/// partir do subprojeto.
#[must_use]
pub fn same_file(cited: &str, file: &str) -> bool {
    file == cited || file.ends_with(&format!("/{cited}"))
}

/// A lição não cita arquivo, ou a onda mexe num dos que ela cita. A lição
/// cita o arquivo entre crases ([`cited_paths`]).
fn touches_cited_file(lesson: &SpecEvent, files: &[String]) -> bool {
    let cited = files_only(cited_paths(lesson.str_field("text").unwrap_or_default()));
    cited.is_empty() || cites_one_of(&cited, files)
}

/// Dos caminhos citados, só os de arquivo, com barras normais. A pasta
/// citada não conta, porque casaria com quase toda onda do lugar dela.
fn files_only(paths: Vec<String>) -> Vec<String> {
    paths.into_iter().filter(|path| !path.ends_with('/')).map(|path| clean_path(&path)).collect()
}

/// Algum dos arquivos citados em `cited` é um dos `files` ([`same_file`]).
fn cites_one_of(cited: &[String], files: &[String]) -> bool {
    cited.iter().any(|path| files.iter().any(|file| same_file(path, file)))
}

/// Os eventos de `found` que servem à onda pelo que dizem, na mesma ordem:
/// os que as palavras-chave ligam ao texto das tarefas (`words`), pela
/// escolha de [`related_to_tasks`], e os que citam no texto um arquivo que a
/// onda mexe (`files`), pela mesma comparação com que [`serving_wave`]
/// segura a lição que cita arquivo. Basta uma das duas ligações. O item cita
/// o arquivo entre crases ou solto no texto, com a linha
/// ([`written_paths`]). É a escolha do item
/// combinado sem dono que uma onda julga antes do envio: o item diz as
/// palavras-chave e o texto como a lição, e o que não se liga à onda por
/// nenhuma das duas não é candidato dela.
#[must_use]
pub fn tied_to_wave<'a>(found: Vec<&'a SpecEvent>, words: &str, files: &[String]) -> Vec<&'a SpecEvent> {
    let files: Vec<String> = files.iter().map(|f| clean_path(f)).filter(|f| !f.is_empty()).collect();
    let related: BTreeSet<u64> = related_to_tasks(found.clone(), words).into_iter().map(|event| event.id).collect();
    found
        .into_iter()
        .filter(|event| {
            related.contains(&event.id)
                || cites_one_of(&files_only(written_paths(event.str_field("text").unwrap_or_default())), &files)
        })
        .collect()
}

/// A lição casa um arquivo de `files` por um padrão dos arquivos dela que
/// não é o do projeto todo, ou vale para uma das `skills`.
fn by_pattern_or_skill(lesson: &SpecEvent, files: &[String], skills: &[String]) -> bool {
    let Some(at) = lesson.fields.get("applies_to").and_then(Value::as_object) else {
        return false;
    };
    let by_pattern = file_patterns(at)
        .iter()
        .filter(|p| !whole_project(p))
        .any(|p| files.iter().any(|file| path_matches(p, file)));
    let skill = at.get("skill").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    by_pattern || skill.is_some_and(|skill| skills.iter().any(|named| named.trim() == skill))
}

/// Cada palavra-chave de uma lição como um termo só da busca: as raízes de
/// todas as palavras dela, ligadas por `_`. "ao mesmo tempo" vira um termo, e
/// não três: a lição só é ligada a uma tarefa quando a palavra-chave inteira
/// aparece nela, e não só um pedaço, como "tempo".
fn key_terms(lesson: &SpecEvent) -> Vec<String> {
    let keys = lesson.fields.get("keys").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    keys.iter().filter_map(Value::as_str).filter_map(key_term).collect()
}

/// Uma palavra-chave como termo da busca; `None` quando ela só tem palavras
/// funcionais, que qualquer texto tem.
fn key_term(key: &str) -> Option<String> {
    if search::query_terms(key).is_empty() {
        return None;
    }
    Some(search_terms(key).join("_"))
}

/// A busca de [`matching_among`], só que sobre as palavras-chave de cada
/// lição: as 5 mais fortes para as palavras `words`. O pedido é feito com as
/// palavras-chave que aparecem inteiras em `words`, com todas as palavras
/// delas, na forma reduzida do `search`.
#[must_use]
pub fn by_keys(lessons: &[&SpecEvent], words: &str) -> Vec<Hit> {
    let said: BTreeSet<String> = search_terms(words).into_iter().collect();
    let docs: Vec<(u64, String)> = lessons.iter().map(|lesson| (lesson.id, key_terms(lesson).join(" "))).collect();
    let asked: Vec<String> = lessons
        .iter()
        .flat_map(|lesson| key_terms(lesson))
        .filter(|term| term.split('_').all(|root| said.contains(root)))
        .collect();
    SearchIndex::build(docs.iter().map(|(id, terms)| (*id, terms.as_str()))).top(&asked, search::TOP)
}

/// Os grupos de lições parecidas que o scan manda juntar, cada um em ordem de
/// número, e os grupos em ordem da primeira lição. Só se comparam lições da
/// mesma classe e do mesmo lugar ([`place`]), pela busca de [`by_keys`]: as
/// palavras-chave de uma são o pedido, e as das outras, o que se procura.
/// Duas lições são parecidas quando cada uma acha a outra com pelo menos
/// metade da nota com que acha a si mesma: a que divide só a palavra do
/// subprojeto, que toda lição dali tem, fica longe da metade. Lições ligadas
/// por uma corrente de pares parecidos ficam no mesmo grupo. As lições de
/// `leaving`, que o scan já manda retirar, não entram em grupo nenhum.
#[must_use]
pub fn similar(bank: &SpecLog, leaving: &[u64]) -> Vec<Vec<u64>> {
    let mut buckets: BTreeMap<(String, String), Vec<&SpecEvent>> = BTreeMap::new();
    for lesson in kept(bank).into_iter().filter(|lesson| !leaving.contains(&lesson.id)) {
        buckets.entry((lesson.event_type.clone(), place(lesson))).or_default().push(lesson);
    }
    let mut groups: Vec<Vec<u64>> = Vec::new();
    for lessons in buckets.into_values().filter(|lessons| lessons.len() > 1) {
        let scores = key_scores(&lessons);
        let ids: Vec<u64> = lessons.iter().map(|lesson| lesson.id).collect();
        let mut group_of: BTreeMap<u64, usize> = BTreeMap::new();
        let mut bucket_groups: Vec<BTreeSet<u64>> = Vec::new();
        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                if !(finds(&scores, *a, *b) && finds(&scores, *b, *a)) {
                    continue;
                }
                match (group_of.get(a).copied(), group_of.get(b).copied()) {
                    (Some(x), Some(y)) if x != y => {
                        let moved = std::mem::take(&mut bucket_groups[y]);
                        for id in &moved {
                            group_of.insert(*id, x);
                        }
                        bucket_groups[x].extend(moved);
                    }
                    (Some(_), Some(_)) => {}
                    (Some(x), None) => {
                        bucket_groups[x].insert(*b);
                        group_of.insert(*b, x);
                    }
                    (None, Some(y)) => {
                        bucket_groups[y].insert(*a);
                        group_of.insert(*a, y);
                    }
                    (None, None) => {
                        group_of.insert(*a, bucket_groups.len());
                        group_of.insert(*b, bucket_groups.len());
                        bucket_groups.push([*a, *b].into());
                    }
                }
            }
        }
        groups.extend(bucket_groups.into_iter().filter(|g| g.len() > 1).map(|g| g.into_iter().collect()));
    }
    groups.sort();
    groups
}

/// A nota com que as palavras-chave de cada lição de `lessons` acham cada
/// uma delas, a própria inclusive: de quem pede para quem é achada.
fn key_scores(lessons: &[&SpecEvent]) -> BTreeMap<u64, BTreeMap<u64, u64>> {
    let terms: Vec<(u64, Vec<String>)> = lessons.iter().map(|lesson| (lesson.id, key_terms(lesson))).collect();
    let docs: Vec<(u64, String)> = terms.iter().map(|(id, own)| (*id, own.join(" "))).collect();
    let index = SearchIndex::build(docs.iter().map(|(id, own)| (*id, own.as_str())));
    terms
        .iter()
        .map(|(id, own)| {
            let hits = index.top(own, lessons.len());
            (*id, hits.into_iter().map(|hit| (hit.id, hit.score)).collect())
        })
        .collect()
}

/// A lição `from` acha a `to` com pelo menos metade da nota com que acha a
/// si mesma.
fn finds(scores: &BTreeMap<u64, BTreeMap<u64, u64>>, from: u64, to: u64) -> bool {
    let Some(hits) = scores.get(&from) else { return false };
    let own = hits.get(&from).copied().unwrap_or_default();
    hits.get(&to).is_some_and(|score| own > 0 && score.saturating_mul(2) >= own)
}

/// Onde a lição vale, numa forma que não depende da ordem dos campos: o
/// subprojeto, os arquivos em ordem e a skill. Duas lições do mesmo lugar
/// têm a mesma forma.
fn place(lesson: &SpecEvent) -> String {
    let Some(at) = lesson.fields.get("applies_to").and_then(Value::as_object) else {
        return String::new();
    };
    let text = |name: &str| at.get(name).and_then(Value::as_str).map(clean_path).unwrap_or_default();
    let mut files: Vec<String> = at
        .get("files")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(clean_path).collect())
        .unwrap_or_default();
    files.sort();
    files.dedup();
    format!("{}|{}|{}", text("subproject"), files.join(","), text("skill"))
}

/// Uma lição que cita caminhos que o projeto já não tem, com esses caminhos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingPaths {
    pub id: u64,
    pub paths: Vec<String>,
}

/// As lições que citam um caminho que o projeto já não tem, em ordem de
/// número, cada uma com os caminhos que faltam. Conta o arquivo citado entre
/// crases no texto e o lugar em que a lição vale: o subprojeto e cada
/// caminho dos arquivos dela. O caminho com curinga (`hooks/**`) é conferido
/// pelo prefixo literal antes do `*`; sem prefixo (`**`, o projeto todo),
/// nada é conferido. A pasta citada no texto não conta: a
/// lição cita justamente a pasta que não deve existir, como a que a
/// instalação de dependências cria. Quem responde se o caminho existe é
/// `found`, com o subprojeto da lição, porque a lição de um subprojeto pode
/// citar o caminho a partir dele.
#[must_use]
pub fn citing_missing_paths(bank: &SpecLog, found: impl Fn(&str, Option<&str>) -> bool) -> Vec<MissingPaths> {
    let mut out = Vec::new();
    for lesson in kept(bank) {
        let at = lesson.fields.get("applies_to").and_then(Value::as_object);
        let subproject =
            at.and_then(|at| at.get("subproject")).and_then(Value::as_str).map(clean_path).filter(|s| !s.is_empty());
        let mut cited: Vec<String> = cited_paths(lesson.str_field("text").unwrap_or_default())
            .into_iter()
            .filter(|path| !path.ends_with('/'))
            .collect();
        cited.extend(subproject.clone());
        cited.extend(
            at.and_then(|at| at.get("files"))
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(Value::as_str)
                .map(clean_path)
                .filter_map(|p| {
                    let prefix = p.split('*').next().unwrap_or_default().trim_end_matches('/');
                    (!prefix.is_empty()).then(|| prefix.to_string())
                }),
        );
        let mut paths: Vec<String> = Vec::new();
        for path in cited {
            let inside = subproject.as_deref().filter(|sub| *sub != path);
            if !found(&path, inside) && !paths.contains(&path) {
                paths.push(path);
            }
        }
        if !paths.is_empty() {
            out.push(MissingPaths { id: lesson.id, paths });
        }
    }
    out
}

/// A lição como é mostrada: a linha com o texto original, sem o `search`.
#[must_use]
pub fn shown(lesson: &SpecEvent) -> String {
    shown_line(&lesson.fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{parse_log, render_line, stamp};
    use crate::platform::i18n::Locale;
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    fn checked(draft: Value) -> Result<(), Refusal> {
        validate(&normalize(obj(draft), None))
    }

    /// Uma lição como o gravador deixa, com o número `id`.
    fn lesson(id: u64, draft: Value) -> String {
        let event = normalize(obj(draft), None);
        validate(&event).unwrap_or_else(|r| panic!("lesson {id} refused: {r:?}"));
        format!("{}\n", render_line(&stamp(event, id, None, "2026-09-12T10:00:00-03:00")))
    }

    fn base(applies_to: Value) -> Value {
        json!({"class": "project_rule", "text": "t", "keys": ["k"], "applies_to": applies_to, "found_in": {"source": "apps/rt/CLAUDE.md"}})
    }

    fn bank() -> SpecLog {
        parse_log(
            &[
                lesson(1, base(json!({"files": ["apps/rt/src/hooks/**"]}))),
                lesson(2, base(json!({"subproject": "packages/core"}))),
                lesson(3, base(json!({"skill": "add-run-command"}))),
                lesson(4, json!({"class": "user_preference", "text": "Resposta curta.", "keys": ["resposta"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}})),
                lesson(5, base(json!({"files": ["apps/cli/src/main.rs"]}))),
            ]
            .concat(),
        )
    }

    fn found(bank: &SpecLog, scope: &Scope) -> Vec<u64> {
        in_scope(bank, scope).iter().map(|l| l.id).collect()
    }

    fn files(paths: &[&str]) -> Scope {
        Scope { files: paths.iter().map(|p| (*p).to_string()).collect(), ..Scope::default() }
    }

    /// A lição é achada pelo padrão dos arquivos, pelo subprojeto e pela
    /// skill; a do projeto todo é achada em qualquer escopo. Um caminho exato
    /// não casa por pedaço.
    #[test]
    fn a_lesson_is_found_by_file_pattern_subproject_and_skill() {
        let bank = bank();
        assert_eq!(found(&bank, &files(&["apps/rt/src/hooks/write/scope_guard.rs"])), [1, 4]);
        assert_eq!(found(&bank, &files(&["apps\\rt\\src\\hooks\\x.rs"])), [1, 4], "a Windows path matches too");
        let sub = Scope { subproject: Some("packages/core".into()), ..Scope::default() };
        assert_eq!(found(&bank, &sub), [2, 4]);
        let skill = Scope { skill: Some("add-run-command".into()), ..Scope::default() };
        assert_eq!(found(&bank, &skill), [3, 4]);
        assert_eq!(found(&bank, &files(&["apps/cli/src/main.rs"])), [4, 5]);
        assert_eq!(found(&bank, &files(&["apps/cli/src/main.rs.bak"])), [4], "an exact path does not match a piece");
    }

    #[test]
    fn a_file_inside_the_lesson_subproject_finds_it() {
        let bank = bank();
        assert_eq!(found(&bank, &files(&["./packages/core/src/domain/search.rs"])), [2, 4]);
        assert_eq!(found(&bank, &files(&["packages/core-extra/src/lib.rs"])), [4]);
    }

    #[test]
    fn a_replaced_lesson_is_no_longer_found() {
        let mut content = [
            lesson(1, json!({"class": "defect", "text": "Apagar a pasta perde trabalho.", "keys": ["apagar"], "applies_to": {"files": ["apps/rt/src/hooks/**"]}, "found_in": {"spec": "s"}})),
        ]
        .concat();
        content.push_str(&lesson(2, json!({"class": "defect", "text": "Remover a pasta perde trabalho.", "keys": ["remover"], "applies_to": {"files": ["apps/rt/src/hooks/**"]}, "found_in": {"spec": "s"}, "replaces": 1})));
        let bank = parse_log(&content);
        assert_eq!(found(&bank, &files(&["apps/rt/src/hooks/x.rs"])), [2]);
        assert!(matching(&bank, "apagando a pasta").iter().all(|hit| hit.id != 1));
    }

    /// A lição achada pelas palavras é mostrada com o texto original, e o
    /// campo de busca não aparece.
    #[test]
    fn a_found_lesson_shows_the_original_text_never_the_search_field() {
        let content = [
            lesson(1, base(json!({"subproject": "apps/rt"}))),
            lesson(2, json!({"class": "defect", "text": "Um rm -rf na pasta errada perde trabalho.", "keys": ["apagar", "rm"], "applies_to": {"subproject": "apps/rt"}, "found_in": {"spec": "s", "branch": "b", "commit": "abc1234"}})),
        ]
        .concat();
        let bank = parse_log(&content);
        let hits = matching(&bank, "apagando a pasta");
        assert_eq!(hits.first().map(|h| h.id), Some(2), "{hits:?}");
        let lesson = bank.get(2).unwrap();
        let shown = shown(lesson);
        assert!(shown.contains("\"text\":\"Um rm -rf na pasta errada perde trabalho.\""), "{shown}");
        assert!(!shown.contains("search"), "{shown}");
        assert!(!shown.contains(lesson.str_field("search").unwrap()), "{shown}");
    }

    /// De cada classe ficam só as 5 mais ligadas às palavras das tarefas, e a
    /// lição sem palavra em comum com elas sai, seja qual for a classe; tudo
    /// volta em ordem de número. Com seis defeitos ligados, o sexto, o mais
    /// fraco, perde o lugar: cinco é o último número que passa inteiro. Cada
    /// palavra da lição é aqui uma palavra-chave dela, porque é nas
    /// palavras-chave que a busca procura.
    #[test]
    fn of_each_class_only_the_five_lessons_closest_to_the_tasks_are_kept() {
        let of = |id: u64, class: &str, text: &str| {
            let keys: Vec<&str> = text.trim_end_matches('.').split(' ').collect();
            lesson(id, json!({"class": class, "text": text, "keys": keys, "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"source": "CLAUDE.md"}}))
        };
        let mut content = vec![of(1, "environment_trap", "O cargo não está no PATH.")];
        for id in 2..=7 {
            content.push(of(id, "project_rule", &format!("A fatura soma o total {id}.")));
        }
        content.push(of(8, "project_rule", "O módulo declara o dono."));
        for id in 9..=13 {
            content.push(of(id, "defect", &format!("A fatura somou o total errado no relatório {id}.")));
        }
        content.push(of(14, "defect", "A fatura antiga ficou de fora."));
        content.push(of(15, "defect", "Apagar a pasta perde trabalho."));
        content.push(of(16, "user_preference", "O total da fatura sai em reais."));
        let bank = parse_log(&content.concat());
        let kept: Vec<u64> =
            related_to_tasks(bank.visible(), "Somar o total da fatura no relatório").iter().map(|l| l.id).collect();
        let rules: Vec<u64> = kept.iter().copied().filter(|id| (2..=8).contains(id)).collect();
        assert_eq!(rules.len(), 5, "cinco regras: {kept:?}");
        assert!(!kept.contains(&8), "a regra sem palavra em comum sai: {kept:?}");
        let defects: Vec<u64> = kept.iter().copied().filter(|id| (9..=15).contains(id)).collect();
        assert_eq!(defects, [9, 10, 11, 12, 13], "os cinco defeitos mais ligados; o sexto e o sem palavra saem: {kept:?}");
        assert!(!kept.contains(&1), "a armadilha sem palavra em comum sai: {kept:?}");
        assert!(kept.contains(&16), "a preferência ligada fica: {kept:?}");
        assert!(kept.windows(2).all(|w| w[0] < w[1]), "{kept:?}");
    }

    /// A lição entra no pedido pelas palavras-chave, e não pelo texto: a
    /// lição longa que divide palavras do texto com a tarefa, sem nenhuma
    /// palavra-chave nela, fica de fora; a que tem uma palavra-chave na
    /// tarefa entra. Na divisa, a palavra-chave de várias palavras só conta
    /// inteira: "ao mesmo tempo" não aparece numa tarefa que diz só "tempo",
    /// e aparece numa que diz "ao mesmo tempo".
    #[test]
    fn a_lesson_is_tied_to_the_tasks_by_its_whole_keywords_and_never_by_its_text() {
        let of = |id: u64, text: &str, keys: &[&str]| lesson(id, json!({"class": "defect", "text": text, "keys": keys, "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}}));
        let bank = parse_log(
            &[
                of(1, "A primeira linha da lista mostra o uso e o tempo da conversa inteira.", &["pedido", "subagente"]),
                of(2, "Rode o teste em primeiro plano.", &["teste"]),
                of(3, "Duas rodadas gravam juntas.", &["ao mesmo tempo"]),
            ]
            .concat(),
        );
        let task = "A primeira linha da barra mostra o uso da conversa e o tempo. O teste confere a linha.";
        let ids = |words: &str| -> Vec<u64> { related_to_tasks(bank.visible(), words).iter().map(|l| l.id).collect() };
        assert_eq!(ids(task), [2], "só a lição com palavra-chave na tarefa entra");
        assert!(matching(&bank, task).iter().any(|hit| hit.id == 1), "pelo texto, a lição 1 entraria");
        assert_eq!(ids(&format!("{task} As duas gravam ao mesmo tempo.")), [2, 3], "a palavra-chave inteira entra");
    }

    /// Uma lição com o texto `text`, que vale em `applies_to` e tem a
    /// palavra-chave "gravar", que a tarefa de [`sent`] diz.
    fn placed(id: u64, class: &str, text: &str, applies_to: Value) -> String {
        lesson(id, json!({"class": class, "text": text, "keys": ["gravar"], "applies_to": applies_to, "found_in": {"spec": "s"}}))
    }

    /// As lições que o pedido de uma onda leva, pelo mesmo caminho da rodada
    /// e do pedido: a onda tem uma tarefa que mexe em `files`, nomeia a skill
    /// `skill` e diz "gravar", a palavra-chave de toda lição destes testes.
    fn sent(bank: &SpecLog, files: &[&str], skill: Option<&str>) -> Vec<u64> {
        let paths: Vec<Value> = files.iter().map(|path| json!({"path": path})).collect();
        let mut task = json!({"wave": 1, "text": "Gravar o arquivo.", "files": paths});
        if let Some(skill) = skill {
            task["skill"] = json!(skill);
        }
        let events = [("wave", json!({"n": 1, "text": "A onda", "criteria": [], "done_when": "passa"})), ("task", task)];
        let mut content = String::new();
        for (i, (event_type, body)) in events.iter().enumerate() {
            let mut map =
                crate::domain::spec_events::normalize(body.as_object().cloned().unwrap_or_default(), event_type);
            map.insert("type".into(), json!(event_type));
            content.push_str(&render_line(&stamp(map, i as u64 + 1, None, "2026-09-23T10:00:00-03:00")));
            content.push('\n');
        }
        let log = parse_log(&content);
        crate::io::wave_prompt::wave_lessons(bank, &log, 1).iter().map(|l| l.id).collect()
    }

    /// A lição que cita arquivo vai só à onda que mexe num dos arquivos
    /// citados, com o caminho inteiro ou o fim dele, e o `:linha` no fim da
    /// citação não atrapalha. Na divisa, o arquivo de nome parecido
    /// (`xdomain/config.rs`, `config.rs.bak`) não é o citado. A lição sem
    /// arquivo citado continua indo a toda onda do lugar dela.
    #[test]
    fn licao_que_cita_arquivo_so_vai_a_onda_que_mexe_nele() {
        let core = json!({"subproject": "packages/core"});
        let bank = parse_log(
            &[
                placed(1, "project_rule", "`ProjectConfig` (`domain/config.rs`) é o dono único do schema de `mustard.json`.", core.clone()),
                placed(2, "project_rule", "O retrato da lista fica em `apps/rt/tests/fixtures/run-surface.txt`, conferido por `apps/rt/tests/run_command_surface.rs:12`.", json!({"files": ["**"]})),
                placed(3, "project_rule", "Trate ausência de arquivo como `Error::NotFound`.", core),
            ]
            .concat(),
        );
        assert_eq!(sent(&bank, &["packages/core/src/domain/config.rs"], None), [1, 3]);
        assert_eq!(sent(&bank, &["packages/core/src/platform/code_tools.rs"], None), [3], "a onda que não mexe no arquivo citado não recebe a lição");
        assert_eq!(sent(&bank, &["packages/core/src/xdomain/config.rs"], None), [3]);
        assert_eq!(sent(&bank, &["packages/core/src/domain/config.rs.bak"], None), [3]);
        assert_eq!(sent(&bank, &["apps/rt/tests/run_command_surface.rs"], None), [2], "o `:linha` sai da citação");
        assert_eq!(sent(&bank, &["apps/rt/tests/fixtures/run-surface.txt", "apps/rt/src/a.rs"], None), [2]);
        assert!(sent(&bank, &["apps/rt/src/commands/maint/upsert.rs"], None).is_empty());
    }

    /// A pasta citada não conta como arquivo citado: a regra do núcleo que
    /// cita só a pasta `vocabulary/` continua indo à onda de código do
    /// núcleo que não mexe nela, e a que cita um arquivo de dentro da mesma
    /// pasta não vai.
    #[test]
    fn licao_que_cita_so_pasta_continua_indo_a_onda_do_subprojeto() {
        let core = json!({"subproject": "packages/core"});
        let bank = parse_log(
            &[
                placed(1, "project_rule", "`unwrap()`/`expect()` são `deny` no workspace fora de teste; propague `Result`. O automaton Aho-Corasick (`vocabulary/`) é único — reúse `KeyedAutomaton`, não instancie outro.", core.clone()),
                placed(2, "project_rule", "`KeyedAutomaton` (`domain/vocabulary/aho.rs`) usa `MatchKind::LeftmostFirst` com case-sensitive.", core),
            ]
            .concat(),
        );
        assert_eq!(sent(&bank, &["packages/core/src/platform/code_tools.rs"], None), [1]);
        assert_eq!(sent(&bank, &["packages/core/src/domain/vocabulary/aho.rs"], None), [1, 2]);
    }

    /// A onda só de texto — markdown, texto ou arquivo de ignorar — não
    /// recebe a lição do projeto todo nem a do subprojeto; recebe a que casa
    /// um arquivo dela por padrão e a da skill que a tarefa nomeia. Na
    /// divisa, um arquivo de código entre os de texto devolve tudo. A onda só
    /// em views Razor ou só na página HTML não é só de texto e recebe as
    /// lições como qualquer onda de código.
    #[test]
    fn onda_so_de_texto_nao_recebe_licao_do_projeto_nem_do_subprojeto() {
        let bank = parse_log(
            &[
                placed(1, "defect", "O teste tem de falhar quando o código está errado.", json!({"files": [WHOLE_PROJECT]})),
                placed(2, "project_rule", "Escreva arquivos sempre pela escrita atômica.", json!({"subproject": "packages/core"})),
                placed(3, "project_rule", "O molde diz o passo inteiro.", json!({"files": ["packages/core/templates/**"]})),
                placed(4, "project_rule", "O molde tem as quatro seções.", json!({"skill": "moldes"})),
            ]
            .concat(),
        );
        let template = "packages/core/templates/agents/pt-BR/wave.md";
        assert_eq!(sent(&bank, &[template], Some("moldes")), [3, 4]);
        assert_eq!(sent(&bank, &[template], None), [3]);
        assert_eq!(sent(&bank, &["docs/guia.md", "LEIA.TXT", ".gitignore", "packages/core/templates/.dockerignore"], None), [3]);
        assert!(sent(&bank, &["packages/core/README.md"], None).is_empty(), "nem a do subprojeto");
        assert_eq!(sent(&bank, &[template, "packages/core/src/lib.rs"], None), [1, 2, 3], "um arquivo de código devolve tudo");
        assert_eq!(sent(&bank, &["packages/core/Views/Home/Index.cshtml"], None), [1, 2], "a onda só em views Razor recebe");
        assert_eq!(sent(&bank, &["packages/core/templates/pages/spec.html"], None), [1, 2, 3], "a onda só na página HTML recebe");
        assert!(!text_only(&[]), "a onda sem arquivo não é só de texto");
    }

    /// Uma lição como o importador das instruções deixa no banco, com o
    /// texto e as palavras-chave reais do banco deste projeto.
    fn rule(id: u64, subproject: &str, text: &str, keys: &[&str]) -> String {
        lesson(id, json!({"class": "project_rule", "text": text, "keys": keys, "applies_to": {"subproject": subproject}, "found_in": {"source": format!("{subproject}/CLAUDE.md")}}))
    }

    /// As regras reais do subprojeto `apps/rt`: a 7 e a 78 são a mesma regra
    /// escrita em dois arquivos de instruções, com palavras diferentes; as
    /// outras dividem com elas só a palavra do subprojeto.
    fn rt_rules(other_place_for_78: Option<&str>) -> Vec<String> {
        let sub = "apps/rt";
        vec![
            rule(5, sub, "Hook nunca pode entrar em pânico nem barrar a sessão por erro próprio.", &["rt", "nunca", "entrar", "pânico", "barrar", "sessão", "próprio"]),
            rule(6, sub, "`clippy::unwrap_used`/`expect_used` são `deny` em todo o crate.", &["rt", "clippy", "unwrap", "expect", "crate", "degrade"]),
            rule(7, sub, "Subcomando novo de `run` exige QUATRO registros; esquecer qualquer um compila mas quebra algo em silêncio.", &["rt", "subcomando", "exige", "quatro", "registros", "esquecer", "qualquer"]),
            rule(9, sub, "A face `run` NÃO lê o stdin do harness.", &["rt", "stdin", "harness", "despachada", "antes", "leitura", "check"]),
            rule(78, other_place_for_78.unwrap_or(sub), "Subcomando novo de `run` exige QUATRO registros (variante no enum, braço no `dispatch()`, entrada na lista trancada e um chamador).", &["rt", "subcomando", "exige", "quatro", "registros", "variante", "família"]),
            rule(79, sub, "`notify`, `sha2` e `rayon` já foram removidos por não terem import nenhum consumindo.", &["rt", "notify", "rayon", "foram", "removidos", "terem", "import"]),
        ]
    }

    /// As duas regras com o mesmo assunto em palavras diferentes formam um
    /// grupo, e as vizinhas do mesmo lugar, que dividem só a palavra do
    /// subprojeto, ficam fora. A mesma regra em outro lugar, a que já vai
    /// sair e a de outra classe não formam grupo.
    #[test]
    fn two_lessons_on_the_same_subject_in_other_words_form_one_group() {
        let bank = parse_log(&rt_rules(None).concat());
        assert_eq!(similar(&bank, &[]), vec![vec![7, 78]]);
        assert!(similar(&bank, &[78]).is_empty(), "a lição que já vai sair não entra em grupo");

        let elsewhere = parse_log(&rt_rules(Some("packages/core")).concat());
        assert!(similar(&elsewhere, &[]).is_empty(), "outro lugar, outra lição");

        let mut other_class = rt_rules(None);
        other_class[4] = other_class[4].replace("\"type\":\"project_rule\"", "\"type\":\"defect\"");
        assert!(similar(&parse_log(&other_class.concat()), &[]).is_empty(), "outra classe, outra lição");
    }

    /// Uma regra do subprojeto `apps/rt` só com as palavras-chave `keys`: o
    /// que compara duas lições são elas.
    fn keyed(id: u64, keys: &[&str]) -> String {
        rule(id, "apps/rt", &format!("A regra {id} do subprojeto."), keys)
    }

    /// As duas notas que comparam a lição `from` com a `to`, no banco
    /// `bank`: a nota com que as palavras-chave de `from` acham `to`, e a nota
    /// com que acham a própria `from`.
    fn notes(bank: &SpecLog, from: u64, to: u64) -> (u64, u64) {
        let scores = key_scores(&kept(bank));
        let hits = &scores[&from];
        (hits.get(&to).copied().unwrap_or_default(), hits[&from])
    }

    /// Duas lições do mesmo lugar: as palavras-chave `shared`, que as duas
    /// têm, mais as `first` na primeira e as `second` na segunda.
    fn pair(shared: &[&str], first: &[&str], second: &[&str]) -> SpecLog {
        let first: Vec<&str> = shared.iter().chain(first).copied().collect();
        let second: Vec<&str> = shared.iter().chain(second).copied().collect();
        parse_log(&[keyed(1, &first), keyed(2, &second)].concat())
    }

    /// Duas lições são parecidas quando cada uma acha a outra com pelo menos
    /// metade da nota com que acha a si mesma. Na divisa: com três
    /// palavras-chave em comum e duas só de cada uma, a nota de uma para a
    /// outra é exatamente a metade, e as duas formam grupo; com cinco em comum
    /// e três só de cada uma, um pouco acima da metade, também; com sete em
    /// comum e cinco só de cada uma, um pouco abaixo, não.
    #[test]
    fn two_lessons_are_similar_from_half_of_the_score_up_and_not_just_below_it() {
        let half = pair(&["trava", "pasta", "suíte"], &["cache", "barra"], &["página", "versão"]);
        for (from, to) in [(1, 2), (2, 1)] {
            let (score, own) = notes(&half, from, to);
            assert_eq!(score * 2, own, "exatamente a metade, de {from} para {to}");
        }
        assert_eq!(similar(&half, &[]), vec![vec![1, 2]], "a metade ainda é parecida");

        let above = pair(&["trava", "pasta", "suíte", "cache", "barra"], &["página", "versão", "branch"], &["commit", "gancho", "sessão"]);
        for (from, to) in [(1, 2), (2, 1)] {
            let (score, own) = notes(&above, from, to);
            assert!(score * 2 > own && score * 20 < own * 11, "um pouco acima da metade, de {from} para {to}: {score} de {own}");
        }
        assert_eq!(similar(&above, &[]), vec![vec![1, 2]]);

        let below = pair(
            &["trava", "pasta", "suíte", "cache", "barra", "página", "versão"],
            &["branch", "commit", "gancho", "hook", "sessão"],
            &["banco", "lição", "prova", "teste", "limite"],
        );
        for (from, to) in [(1, 2), (2, 1)] {
            let (score, own) = notes(&below, from, to);
            assert!(score * 2 < own && score * 20 > own * 9, "um pouco abaixo da metade, de {from} para {to}: {score} de {own}");
        }
        assert!(similar(&below, &[]).is_empty(), "abaixo da metade não é parecida");
    }

    /// Quando só uma das duas acha a outra, elas não são parecidas: a lição
    /// de três palavras-chave, todas dentro da outra, acha a de oito com mais
    /// da metade da nota; a de oito acha a de três com menos da metade.
    #[test]
    fn a_lesson_that_finds_the_other_without_being_found_back_forms_no_group() {
        let bank = pair(&["trava", "pasta", "suíte"], &[], &["cache", "barra", "página", "versão", "banco"]);
        let (score, own) = notes(&bank, 1, 2);
        assert!(score * 2 >= own, "a curta acha a longa: {score} de {own}");
        let (score, own) = notes(&bank, 2, 1);
        assert!(score * 2 < own, "a longa não acha a curta: {score} de {own}");
        assert!(similar(&bank, &[]).is_empty());
    }

    /// Lições ligadas por uma corrente de pares parecidos ficam num grupo só,
    /// mesmo quando a corrente liga dois grupos que já existiam: a 1 é
    /// parecida com a 3, a 2 com a 4, e só depois a 3 com a 4. A 1 e a 2 não
    /// dividem palavra-chave nenhuma.
    #[test]
    fn two_groups_linked_by_a_similar_pair_become_one() {
        let bank = parse_log(
            &[
                keyed(1, &["gancho", "trava", "pasta"]),
                keyed(2, &["cache", "barra"]),
                keyed(3, &["trava", "pasta", "suíte", "commit"]),
                keyed(4, &["suíte", "commit", "cache", "barra"]),
            ]
            .concat(),
        );
        for (a, b) in [(1, 3), (2, 4), (3, 4)] {
            for (from, to) in [(a, b), (b, a)] {
                let (score, own) = notes(&bank, from, to);
                assert!(score * 2 >= own, "{from} acha {to}: {score} de {own}");
            }
        }
        for (a, b) in [(1, 2), (1, 4), (2, 3)] {
            assert_eq!(notes(&bank, a, b).0, 0, "{a} não acha {b}");
        }
        assert_eq!(similar(&bank, &[]), vec![vec![1, 2, 3, 4]]);
    }

    /// A lição que cita um arquivo que o projeto já não tem é apontada com o
    /// caminho; a que cita um arquivo que existe, a pasta que não deve
    /// existir e o endereço de um pacote não. O subprojeto que saiu também
    /// é apontado, e o arquivo citado a partir do subprojeto é achado nele.
    #[test]
    fn a_lesson_citing_a_path_the_project_no_longer_has_is_pointed_out() {
        let bank = parse_log(
            &[
                rule(1, "packages/core", "Trate a contagem de `domain/economy/estimator.rs` como aproximação.", &["tokens"]),
                rule(2, "packages/core", "Escreva por `io/fs.rs`, nunca direto; o `target/` fica fora.", &["escrita"]),
                rule(3, "apps/scan/tests/fixtures/flutter_app", "Importe `package:flutter/material.dart` e nunca `lib/counter.g.dart`.", &["flutter"]),
                rule(4, "apps/mcp", "O `main.rs` só chama a biblioteca.", &["mcp"]),
            ]
            .concat(),
        );
        let exists = [
            "packages/core",
            "packages/core/src/io/fs.rs",
            "apps/scan/tests/fixtures/flutter_app",
            "apps/scan/tests/fixtures/flutter_app/lib/counter.g.dart",
        ];
        let found = |path: &str, inside: Option<&str>| {
            exists.contains(&path)
                || inside.is_some_and(|sub| exists.contains(&format!("{sub}/{path}").as_str()))
                || exists.iter().any(|known| known.ends_with(&format!("/{path}")))
        };
        let missing = citing_missing_paths(&bank, found);
        assert_eq!(
            missing,
            vec![
                MissingPaths { id: 1, paths: vec!["domain/economy/estimator.rs".into()] },
                MissingPaths { id: 4, paths: vec!["apps/mcp".into()] },
            ]
        );
    }

    /// O `files` com curinga (`apps/rt/src/hooks/**`) não escapa mais da
    /// conferência: é conferido pelo prefixo literal antes do `*`. O curinga
    /// sozinho (`**`, a lição do projeto todo) continua fora, porque não
    /// sobra prefixo nenhum para conferir.
    #[test]
    fn a_lesson_citing_a_wildcard_files_pattern_is_checked_by_its_literal_prefix() {
        let bank = parse_log(&lesson(1, base(json!({"files": ["apps/rt/src/hooks/**"]}))));
        let prefix_exists = |path: &str, _inside: Option<&str>| path == "apps/rt/src/hooks";
        assert_eq!(citing_missing_paths(&bank, prefix_exists), vec![], "o prefixo existe, a lição fica");

        let nothing_exists = |_path: &str, _inside: Option<&str>| false;
        assert_eq!(
            citing_missing_paths(&bank, nothing_exists),
            vec![MissingPaths { id: 1, paths: vec!["apps/rt/src/hooks".into()] }],
            "o prefixo não existe, o curinga é apontado como caminho que falta"
        );

        let whole_project = parse_log(&lesson(
            2,
            json!({"class": "user_preference", "text": "t", "keys": ["k"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}}),
        ));
        assert_eq!(citing_missing_paths(&whole_project, nothing_exists), vec![], "sem prefixo, nada para conferir");
    }

    /// A retirada é o rascunho sem classe com as lições em `targets` e o
    /// motivo em `reason`: sem um deles, é recusada pelo nome. A lição que
    /// junta outras aponta todas em `replaces`; a lista vazia é recusada.
    #[test]
    fn a_retirement_needs_targets_and_reason_and_a_merge_a_list_of_lessons() {
        let retire = normalize(obj(json!({"targets": [3], "reason": "cita um arquivo que saiu"})), Some("s"));
        assert_eq!(retire["type"], json!(RETIRE));
        assert!(retire.get("found_in").is_none(), "a retirada não nasce em lugar nenhum");
        assert!(validate(&retire).is_ok());
        let no_reason = normalize(obj(json!({"targets": [3]})), None);
        assert_eq!(validate(&no_reason).unwrap_err(), missing("reason"));
        let no_targets = normalize(obj(json!({"targets": [], "reason": "r"})), None);
        assert_eq!(validate(&no_targets).unwrap_err(), missing("targets"));

        let merge = |replaces: Value| checked(json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"spec": "x"}, "replaces": replaces}));
        assert!(merge(json!([1, 2])).is_ok());
        assert!(merge(json!(1)).is_ok());
        assert_eq!(merge(json!([])).unwrap_err(), invalid("replaces", Kind::Ints));
        assert_eq!(merge(json!("1")).unwrap_err(), invalid("replaces", Kind::Ints));
    }

    /// O mesmo texto, com outros espaços, maiúsculas e acentos, é a mesma
    /// lição: a nova é recusada apontando a que existe, de qualquer classe. A
    /// que ela substitui não conta, e a que já foi substituída também não.
    #[test]
    fn a_lesson_repeating_the_text_of_one_already_kept_is_refused_naming_it() {
        let bank = parse_log(
            &[
                lesson(1, json!({"class": "defect", "text": "Não apague a pasta.", "keys": ["apagar"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}})),
                lesson(2, json!({"class": "defect", "text": "Rode em primeiro plano.", "keys": ["rodar"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}})),
                lesson(3, json!({"class": "defect", "text": "Rode tudo em primeiro plano.", "keys": ["rodar"], "applies_to": {"files": [WHOLE_PROJECT]}, "found_in": {"spec": "s"}, "replaces": 2})),
            ]
            .concat(),
        );
        let draft = |text: &str, extra: Value| {
            let mut event = normalize(obj(json!({"class": "project_rule", "text": text, "keys": ["k"], "applies_to": {"subproject": "apps/rt"}, "found_in": {"source": "apps/rt/CLAUDE.md"}})), None);
            if let Value::Object(more) = extra {
                event.extend(more);
            }
            event
        };
        let refusal = check_against(&bank, &draft("  NAO   apague a PASTA. ", json!({}))).unwrap_err();
        assert_eq!(refusal, Refusal::LessonRepeated { id: 1, text: "Não apague a pasta.".into() });
        assert_eq!(refusal.reason(), "lesson-repeated");
        assert!(refusal.message(Locale::PtBr).contains("lição 1"), "{}", refusal.message(Locale::PtBr));
        assert!(refusal.message(Locale::PtBr).contains("Não apague a pasta."), "{}", refusal.message(Locale::PtBr));
        assert!(refusal.message(Locale::EnUs).contains("lesson 1"), "{}", refusal.message(Locale::EnUs));
        assert!(check_against(&bank, &draft("Não apague a pasta errada.", json!({}))).is_ok(), "outro texto entra");
        assert!(check_against(&bank, &draft("Nao apague a pasta.", json!({"replaces": 1}))).is_ok(), "a versão nova da própria lição entra");
        assert!(check_against(&bank, &draft("Rode em primeiro plano.", json!({}))).is_ok(), "a lição substituída não conta");
    }

    #[test]
    fn a_lesson_without_class_text_keys_or_origin_is_refused_by_name() {
        let full = json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"spec": "x"}});
        assert!(checked(full.clone()).is_ok());
        for (field, refusal) in [
            ("class", missing("class")),
            ("text", missing("text")),
            ("keys", missing("keys")),
            ("applies_to", missing("applies_to")),
            ("found_in", Refusal::LessonOriginMissing),
        ] {
            let mut draft = full.clone();
            draft.as_object_mut().unwrap().remove(field);
            assert_eq!(checked(draft).unwrap_err(), refusal, "without {field}");
        }
        let empty_scope = json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"file": "x"}, "found_in": {"spec": "x"}});
        assert_eq!(checked(empty_scope).unwrap_err(), missing("applies_to"));
        let no_origin = json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"why": "x"}});
        let refusal = checked(no_origin).unwrap_err();
        assert_eq!(refusal, Refusal::LessonOriginMissing);
        assert_eq!(refusal.reason(), "lesson-origin-missing");
        assert!(refusal.message(Locale::PtBr).contains("onde nasceu"), "{}", refusal.message(Locale::PtBr));
        assert!(refusal.message(Locale::EnUs).contains("where it was born"), "{}", refusal.message(Locale::EnUs));
        let with_code = json!({"class": "defect", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"spec": "x"}, "code": "MSTD-X-0001"});
        assert_eq!(checked(with_code).unwrap_err(), Refusal::BinaryOnlyField { field: "code".into() });
    }

    #[test]
    fn an_unknown_class_is_refused_with_the_four_accepted_ones() {
        let draft = json!({"class": "bug", "text": "t", "keys": ["k"], "applies_to": {"skill": "s"}, "found_in": {"spec": "x"}});
        let refusal = checked(draft).unwrap_err();
        assert_eq!(refusal, invalid("class", Kind::OneOf(CLASSES)));
        for lang in [Locale::PtBr, Locale::EnUs] {
            let message = refusal.message(lang);
            assert!(message.contains("defect, project_rule, environment_trap, user_preference"), "{message}");
        }
    }

    /// A spec que o `write` recebeu diz onde a lição nasceu, quando a lição
    /// não diz; a que a lição diz fica.
    #[test]
    fn the_spec_given_to_the_write_fills_where_the_lesson_was_found() {
        let bare = normalize(obj(json!({"class": "defect", "text": "t", "keys": ["k"]})), Some("minha-spec"));
        assert_eq!(bare["found_in"], json!({"spec": "minha-spec"}));
        assert_eq!(bare["type"], json!("defect"));
        let own = normalize(obj(json!({"found_in": {"spec": "outra", "commit": "abc"}})), Some("minha-spec"));
        assert_eq!(own["found_in"], json!({"spec": "outra", "commit": "abc"}));
        let partial = normalize(obj(json!({"found_in": {"commit": "abc"}})), Some("minha-spec"));
        assert_eq!(partial["found_in"], json!({"spec": "minha-spec", "commit": "abc"}));
    }
}
