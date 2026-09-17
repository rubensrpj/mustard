//! `spec_index` — o índice das specs (`.claude/spec/index.ndjson`): uma linha
//! por spec, para achar uma spec sem abrir o arquivo de eventos dela.
//!
//! A primeira linha é a do projeto, que guarda o endereço da página do
//! projeto quando ele existe: `{"v":1,"type":"project"}`. Depois vem uma linha
//! por spec, em ordem de nome, com o nome, a hora do primeiro e a do último
//! evento, a fase, a branch, o objetivo numa frase, o endereço em que a página
//! da spec foi publicada por último, os títulos das regras e das decisões
//! vigentes e o campo `search` calculado deles. A página do projeto sai só
//! destas linhas.
//!
//! Cada linha de spec sai só do arquivo de eventos dela e é montada por
//! `render_line`: os mesmos eventos dão sempre os mesmos bytes, e o índice
//! refeito do zero sai igual ao que as gravações deixaram.
//!
//! O objetivo é a primeira frase do primeiro `context` da spec, na versão
//! vigente dele. A linha que é só título (um cabeçalho `#` ou um trecho em
//! negrito sozinho na linha) não é frase e é pulada, e o negrito sai do texto.
//! O título de uma regra ou de uma decisão é o trecho em negrito do começo do
//! texto; sem negrito, a primeira frase, cortada em 120 caracteres.
//!
//! Função pura: sem disco e sem relógio. A trava e a gravação moram em
//! `io::spec_index`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::domain::spec_events::{render_line, search_field, Refusal, SpecEvent, SpecLog, FORMAT_VERSION};
use crate::domain::spec_state::State;
use crate::platform::i18n::{translate, Locale};

/// O tipo da primeira linha, a do projeto.
pub const PROJECT_TYPE: &str = "project";

/// O tipo de cada linha de spec.
pub const SPEC_TYPE: &str = "spec";

/// Até quantos caracteres vai o título tirado da primeira frase.
pub const TITLE_CHARS: usize = 120;

/// Os tipos cujos itens dão título na linha da spec.
const TITLED_TYPES: &[&str] = &["rule", "decision"];

/// A página de uma spec, no campo `page` da publicação.
pub const SPEC_PAGE: &str = "spec";

/// A página do projeto, no campo `page` da publicação. Ela é gravada na spec
/// em que o passo corre, e o endereço vai para a linha do projeto.
pub const PROJECT_PAGE: &str = "project";

/// A linha do projeto, com o endereço da página dele quando há.
#[must_use]
pub fn project_line(url: Option<&str>) -> String {
    let mut line = Map::new();
    line.insert("v".into(), Value::from(FORMAT_VERSION));
    line.insert("type".into(), Value::from(PROJECT_TYPE));
    if let Some(url) = url.map(str::trim).filter(|u| !u.is_empty()) {
        line.insert("url".into(), Value::from(url));
    }
    render_line(&line)
}

/// O endereço da página do projeto, lido da linha do projeto do índice
/// `content`, pelo mesmo leitor da gravação. `None` sem linha do projeto ou
/// sem endereço nela.
#[must_use]
pub fn project_url(content: &str) -> Option<String> {
    let line = read_lines(content).project?;
    let parsed = serde_json::from_str::<Value>(line).ok()?;
    parsed.get("url").and_then(Value::as_str).map(str::trim).filter(|url| !url.is_empty()).map(str::to_string)
}

/// O índice `content` com a linha do projeto trocada pela que leva `url`; as
/// linhas das specs e as que não se entendem ficam como estão.
#[must_use]
pub fn with_project_url(content: &str, url: &str) -> String {
    let lines = read_lines(content);
    let project = project_line(Some(url));
    let specs: Vec<&str> = lines.specs.values().copied().collect();
    let other: Vec<&str> = lines.other.iter().map(|(_, l)| *l).collect();
    assemble(Some(&project), &specs, &other)
}

/// O endereço de `event` quando ele é uma publicação da página `page` que deu
/// certo.
#[must_use]
pub fn published_to<'a>(event: &'a SpecEvent, page: &str) -> Option<&'a str> {
    let published = event.event_type == "publish"
        && event.str_field("page") == Some(page)
        && event.fields.get("ok").and_then(Value::as_bool) == Some(true);
    event.str_field("url").map(str::trim).filter(|url| published && !url.is_empty())
}

/// A última publicação da página do projeto que deu certo nesta spec: a hora
/// e o endereço.
#[must_use]
pub fn project_publish(log: &SpecLog) -> Option<(&str, &str)> {
    log.visible().into_iter().filter_map(|e| published_to(e, PROJECT_PAGE).map(|url| (e.at(), url))).next_back()
}

/// A linha da spec `name`, montada do arquivo de eventos dela. `None` quando
/// o arquivo não tem evento que se entenda: a spec fica fora do índice.
#[must_use]
pub fn spec_line(name: &str, log: &SpecLog) -> Option<String> {
    let first = log.events.first()?;
    let last = log.events.last()?;
    let visible = log.visible();
    // A fase e a branch são as do estado dobrado, a mesma leitura das travas:
    // um `state` só com a branch, ou uma revisão de um `state` antigo, não
    // muda a fase que o índice mostra.
    let state = State::from_log(log);
    let phase = state.phase;
    let branch = state.branch;
    let goal = goal_of(log);
    let url = spec_page_url(log);
    let mut titled: Vec<&SpecEvent> =
        visible.iter().copied().filter(|e| TITLED_TYPES.contains(&e.event_type.as_str())).collect();
    titled.sort_by_key(|e| e.id);
    let titles: Vec<String> = titled.into_iter().filter_map(title_of).collect();

    let mut line = Map::new();
    line.insert("v".into(), Value::from(FORMAT_VERSION));
    line.insert("type".into(), Value::from(SPEC_TYPE));
    line.insert("name".into(), Value::from(name));
    line.insert("created".into(), Value::from(first.at()));
    line.insert("updated".into(), Value::from(last.at()));
    if let Some(phase) = phase {
        line.insert("phase".into(), Value::from(phase));
    }
    if let Some(branch) = branch {
        line.insert("branch".into(), Value::from(branch));
    }
    if let Some(goal) = &goal {
        line.insert("goal".into(), Value::from(goal.as_str()));
    }
    if let Some(url) = url {
        line.insert("url".into(), Value::from(url));
    }
    if !titles.is_empty() {
        line.insert("titles".into(), Value::from(titles.clone()));
    }
    let keys: Vec<&str> = std::iter::once(name).chain(titles.iter().map(String::as_str)).collect();
    let search = search_field(goal.as_deref(), &keys);
    if !search.is_empty() {
        line.insert("search".into(), Value::String(search));
    }
    Some(render_line(&line))
}

/// O endereço da última publicação da página da spec que deu certo. Uma
/// publicação que falhou não apaga o endereço de antes, e a página refeita e
/// publicada num endereço novo passa a valer no lugar dele. A publicação da
/// página do projeto, gravada na mesma spec, não conta.
#[must_use]
pub fn spec_page_url(log: &SpecLog) -> Option<&str> {
    log.visible().into_iter().filter_map(|e| published_to(e, SPEC_PAGE)).next_back()
}

/// O objetivo da spec numa frase: a primeira frase do primeiro `context`, na
/// versão vigente dele, depois das linhas que são só título e sem o negrito.
/// `None` sem `context` ou quando o texto é só título.
#[must_use]
pub fn goal_of(log: &SpecLog) -> Option<String> {
    let text = crate::domain::survey::goal(log).and_then(|e| e.str_field("text"))?;
    let plain = after_titles(text).replace("**", "");
    let goal = first_sentence(&plain);
    (!goal.is_empty()).then(|| goal.to_string())
}

/// O texto a partir da primeira linha que não é só título: pula as linhas em
/// branco, os cabeçalhos `#` e as linhas que são só um trecho em negrito.
pub(crate) fn after_titles(text: &str) -> &str {
    let mut rest = text.trim_start();
    while let Some(line) = rest.lines().next() {
        if !is_title_line(line.trim()) {
            break;
        }
        rest = rest[line.len()..].trim_start();
    }
    rest
}

/// Uma linha que é só título: um cabeçalho `#` ou um trecho em negrito
/// sozinho na linha.
fn is_title_line(line: &str) -> bool {
    let heading = line.starts_with('#') && line.trim_start_matches('#').chars().next().is_none_or(char::is_whitespace);
    let bold = line
        .strip_prefix("**")
        .and_then(|rest| rest.strip_suffix("**"))
        .is_some_and(|inner| !inner.trim().is_empty() && !inner.contains("**"));
    heading || bold
}

/// O título de uma regra ou de uma decisão: o trecho em negrito do começo do
/// texto; sem negrito, a primeira frase, cortada em [`TITLE_CHARS`]
/// caracteres. `None` para o item sem texto, como o expurgado.
#[must_use]
pub fn title_of(event: &SpecEvent) -> Option<String> {
    let text = event.str_field("text")?.trim();
    let bold = text
        .strip_prefix("**")
        .and_then(|rest| rest.split_once("**"))
        .map(|(bold, _)| bold.trim())
        .filter(|bold| !bold.is_empty());
    if let Some(bold) = bold {
        return Some(bold.to_string());
    }
    let sentence = first_sentence(text);
    (!sentence.is_empty()).then(|| cut(sentence, TITLE_CHARS))
}

/// A primeira frase de um texto: até o primeiro ponto final, de exclamação ou
/// de interrogação seguido de espaço ou do fim, ou até a primeira quebra de
/// linha. Um ponto no meio de um nome, como `spec.ndjson`, não corta.
#[must_use]
pub fn first_sentence(text: &str) -> &str {
    let text = text.trim();
    for (i, c) in text.char_indices() {
        if c == '\n' {
            return text[..i].trim_end();
        }
        if matches!(c, '.' | '!' | '?') {
            let end = i + c.len_utf8();
            if text[end..].chars().next().is_none_or(char::is_whitespace) {
                return &text[..end];
            }
        }
    }
    text
}

/// O texto com no máximo `max` caracteres; o cortado termina em reticências.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// O índice lido linha a linha.
struct Lines<'a> {
    /// A primeira linha do projeto.
    project: Option<&'a str>,
    /// A linha de cada spec, pelo nome.
    specs: BTreeMap<String, &'a str>,
    /// As linhas que não se entendem, cada uma com o número dela no arquivo.
    other: Vec<(usize, &'a str)>,
}

fn read_lines(content: &str) -> Lines<'_> {
    let mut out = Lines { project: None, specs: BTreeMap::new(), other: Vec::new() };
    for (i, raw) in content.split('\n').enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<Value>(line).ok();
        let field = |key: &str| parsed.as_ref().and_then(|v| v.get(key)).and_then(Value::as_str);
        match (field("type"), field("name")) {
            (Some(PROJECT_TYPE), _) if out.project.is_none() => out.project = Some(line),
            (Some(SPEC_TYPE), Some(name)) if !out.specs.contains_key(name) => {
                out.specs.insert(name.to_string(), line);
            }
            _ => out.other.push((i + 1, line)),
        }
    }
    out
}

/// A linha de uma spec como o índice a guarda, lida de volta: o nome, o
/// objetivo, a fase, os títulos das regras e das decisões e o campo `search`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexLine {
    pub name: String,
    pub goal: Option<String>,
    pub phase: Option<String>,
    pub titles: Vec<String>,
    pub search: String,
}

/// A linha de uma spec como a página do projeto a mostra: o nome, as datas, a
/// fase, a branch, o objetivo, o endereço da página e os títulos.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectRow {
    pub name: String,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub phase: Option<String>,
    pub branch: Option<String>,
    pub goal: Option<String>,
    pub url: Option<String>,
    pub titles: Vec<String>,
}

/// Cada linha de spec do índice `content`, lida, em ordem de nome, pelo mesmo
/// leitor da gravação: a linha do projeto e a que não se entende ficam de
/// fora, e um nome repetido vale pela primeira linha.
fn parsed_specs(content: &str) -> Vec<(String, Value)> {
    read_lines(content)
        .specs
        .into_iter()
        .filter_map(|(name, line)| serde_json::from_str::<Value>(line).ok().map(|parsed| (name, parsed)))
        .collect()
}

fn text_of(parsed: &Value, key: &str) -> Option<String> {
    parsed.get(key).and_then(Value::as_str).map(str::to_string)
}

fn titles_of(parsed: &Value) -> Vec<String> {
    parsed
        .get("titles")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// As linhas de spec do índice `content`, em ordem de nome, pelo mesmo leitor
/// da gravação: a linha do projeto e a que não se entende ficam de fora, e um
/// nome repetido vale pela primeira linha.
#[must_use]
pub fn spec_lines(content: &str) -> Vec<IndexLine> {
    parsed_specs(content)
        .into_iter()
        .map(|(name, parsed)| IndexLine {
            goal: text_of(&parsed, "goal"),
            phase: text_of(&parsed, "phase"),
            titles: titles_of(&parsed),
            search: text_of(&parsed, "search").unwrap_or_default(),
            name,
        })
        .collect()
}

/// As linhas de spec do índice `content` como a página do projeto as mostra,
/// pelo mesmo leitor de [`spec_lines`].
#[must_use]
pub fn project_rows(content: &str) -> Vec<ProjectRow> {
    parsed_specs(content)
        .into_iter()
        .map(|(name, parsed)| ProjectRow {
            created: text_of(&parsed, "created"),
            updated: text_of(&parsed, "updated"),
            phase: text_of(&parsed, "phase"),
            branch: text_of(&parsed, "branch"),
            goal: text_of(&parsed, "goal"),
            url: text_of(&parsed, "url"),
            titles: titles_of(&parsed),
            name,
        })
        .collect()
}

/// O arquivo: a linha do projeto (a de agora ou, sem ela, a sem endereço),
/// as linhas das specs e as outras, cada uma com o `\n` no fim.
fn assemble(project: Option<&str>, specs: &[&str], other: &[&str]) -> String {
    let default = project_line(None);
    let mut out = String::new();
    for line in std::iter::once(project.unwrap_or(&default)).chain(specs.iter().copied()).chain(other.iter().copied()) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// O índice com a linha da spec `name` trocada por `line`, posta quando
/// faltava ou tirada quando `line` é `None`. A linha do projeto fica em
/// primeiro (e nasce quando falta), as specs seguem em ordem de nome, e as
/// linhas que não se entendem ficam como estão, no fim.
#[must_use]
pub fn merge(content: &str, name: &str, line: Option<&str>) -> String {
    let lines = read_lines(content);
    let mut specs: BTreeMap<&str, &str> = lines.specs.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    match line {
        Some(line) => {
            specs.insert(name, line);
        }
        None => {
            specs.remove(name);
        }
    }
    let specs: Vec<&str> = specs.into_values().collect();
    let other: Vec<&str> = lines.other.iter().map(|(_, l)| *l).collect();
    assemble(lines.project, &specs, &other)
}

/// O índice só com a linha do projeto e as linhas das specs que `keep`
/// aceita: sai a linha de spec que não existe mais e a que não se entende.
#[must_use]
pub fn prune(content: &str, keep: impl Fn(&str) -> bool) -> String {
    let lines = read_lines(content);
    let specs: Vec<&str> = lines.specs.iter().filter(|(name, _)| keep(name)).map(|(_, l)| *l).collect();
    assemble(lines.project, &specs, &[])
}

/// O índice esperado: a linha do projeto de `current` e a linha de cada spec
/// de `specs`, em ordem de nome.
#[must_use]
pub fn canonical(current: &str, specs: &BTreeMap<String, String>) -> String {
    let specs: Vec<&str> = specs.values().map(String::as_str).collect();
    assemble(read_lines(current).project, &specs, &[])
}

/// Onde o índice `current` difere de `expected`: o nome de cada spec cuja
/// linha falta, sobra ou é outra, e `#<n>` para cada linha `n` que não se
/// entende (`#1` quando falta a linha do projeto).
#[must_use]
pub fn diff(current: &str, expected: &str) -> Vec<String> {
    let (now, want) = (read_lines(current), read_lines(expected));
    let mut out = Vec::new();
    if now.project.is_none() {
        out.push("#1".to_string());
    }
    let names: BTreeSet<&String> = now.specs.keys().chain(want.specs.keys()).collect();
    for name in names {
        if now.specs.get(name) != want.specs.get(name) {
            out.push(name.clone());
        }
    }
    out.extend(now.other.iter().map(|(n, _)| format!("#{n}")));
    out
}

/// O aviso de uma gravação cuja linha no índice não foi refeita: o evento já
/// está gravado, e o `index` refaz o índice.
#[must_use]
pub fn write_warning(refusal: &Refusal, lang: Locale) -> String {
    let detail = match refusal {
        Refusal::Io { detail } => detail.clone(),
        other => other.message(lang),
    };
    translate("spec_index.write_warning", lang).replace("{detail}", &detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::{normalize, parse_log, search_terms, stamp};
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    fn at(hm: &str) -> String {
        format!("2026-09-11T{hm}:00-03:00")
    }

    /// Uma linha como o gravador deixa.
    fn ev(id: u64, hm: &str, event_type: &str, draft: Value) -> String {
        format!("{}\n", render_line(&stamp(normalize(obj(draft), event_type), id, None, &at(hm))))
    }

    fn parsed(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    fn long_decision() -> String {
        format!("A página é publicada só nos marcos, {}até o fim. Segunda frase.", "e mais uma parte comprida ".repeat(8))
    }

    fn spec() -> String {
        [
            ev(1, "08:40", "state", json!({"author": "binary", "phase": "survey", "branch": "feature/teste", "base": "dev"})),
            ev(2, "08:41", "message", json!({"author": "user", "text": "Revise tudo"})),
            ev(3, "08:42", "context", json!({"text": "Deixar o Mustard enxuto. O resto vem depois.", "origin": 2})),
            ev(4, "08:43", "rule", json!({"text": "**Índice das unidades.**\n- Um arquivo só.", "keys": ["índice"], "example": "e", "origin": 2})),
            ev(5, "08:44", "decision", json!({"text": long_decision(), "why": "w", "keys": ["k"], "origin": 2})),
            ev(6, "08:45", "state", json!({"author": "binary", "phase": "running"})),
        ]
        .concat()
    }

    /// O endereço da página do projeto sai da linha do projeto, em qualquer
    /// lugar do arquivo; sem a linha ou sem endereço, nada.
    #[test]
    fn the_project_page_address_comes_from_the_project_line() {
        let spec = r#"{"type":"spec","name":"a","url":"https://x/spec"}"#;
        assert_eq!(project_url(&format!("{}\n{spec}\n", project_line(Some("https://x/p")))).as_deref(), Some("https://x/p"));
        assert_eq!(project_url(&format!("{spec}\n{}\n", project_line(Some(" https://x/q ")))).as_deref(), Some("https://x/q"));
        assert_eq!(project_url(&format!("{}\n{spec}\n", project_line(None))), None);
        assert_eq!(project_url(spec), None);
        assert_eq!(project_url(""), None);
    }

    #[test]
    fn the_index_line_carries_name_dates_phase_branch_goal_and_titles() {
        let line = spec_line("teste", &parse_log(&spec())).unwrap();
        assert!(line.starts_with(r#"{"v":1,"type":"spec","branch":"feature/teste","created":"#), "{line}");
        let got = parsed(&line);
        assert_eq!(got["name"], json!("teste"));
        assert_eq!(got["created"], json!(at("08:40")));
        assert_eq!(got["updated"], json!(at("08:45")));
        assert_eq!(got["phase"], json!("running"), "the last state wins");
        assert_eq!(got["branch"], json!("feature/teste"), "the last state that has a branch");
        assert_eq!(got["goal"], json!("Deixar o Mustard enxuto."));
        let titles = got["titles"].as_array().unwrap();
        assert_eq!(titles[0], json!("Índice das unidades."), "the bold opening is the title");
        let cut = titles[1].as_str().unwrap();
        assert_eq!(cut.chars().count(), TITLE_CHARS, "{cut}");
        assert!(cut.starts_with("A página é publicada") && cut.ends_with('…'), "{cut}");
        let search: Vec<&str> = got["search"].as_str().unwrap().split(' ').collect();
        for word in ["enxuto", "teste", "unidades"] {
            assert!(search.contains(&search_terms(word)[0].as_str()), "{word}: {search:?}");
        }
        assert!(line.ends_with(&format!(r#","updated":"{}","search":"{}"}}"#, at("08:45"), got["search"].as_str().unwrap())));
    }

    /// Lado a lado — a fase e a branch do índice são as do estado dobrado: um
    /// `state` só com a branch e uma revisão de um `state` antigo dão a mesma
    /// resposta nas duas leituras, e nenhum dos dois muda a fase.
    #[test]
    fn the_index_phase_and_branch_are_the_folded_state() {
        let branch_only = ev(7, "08:46", "state", json!({"author": "binary", "branch": "feature/outra"}));
        let revision = ev(
            7,
            "08:46",
            "state",
            json!({"author": "binary", "phase": "survey", "branch": "feature/certa", "replaces": 1}),
        );
        for body in [spec(), [spec(), branch_only].concat(), [spec(), revision].concat()] {
            let log = parse_log(&body);
            let state = State::from_log(&log);
            let got = parsed(&spec_line("teste", &log).unwrap());
            assert_eq!(got.get("phase").and_then(Value::as_str), state.phase, "{body}");
            assert_eq!(got.get("branch").and_then(Value::as_str), state.branch.as_deref(), "{body}");
            assert_eq!(got["phase"], json!("running"), "the phase stays: {body}");
        }
    }

    #[test]
    fn the_same_events_give_the_same_index_line_bytes() {
        let content = spec();
        let one = spec_line("teste", &parse_log(&content));
        assert_eq!(one, spec_line("teste", &parse_log(&content.clone())));
        let more = format!("{content}{}", ev(7, "09:00", "message", json!({"author": "user", "text": "mais"})));
        let two = spec_line("teste", &parse_log(&more)).unwrap();
        let (a, b) = (parsed(one.as_deref().unwrap()), parsed(&two));
        assert_eq!(b["updated"], json!(at("09:00")));
        assert_eq!((a["titles"].clone(), a["goal"].clone()), (b["titles"].clone(), b["goal"].clone()));
        assert!(spec_line("teste", &parse_log("")).is_none(), "a spec with no event has no line");
    }

    /// Só a versão vigente de uma regra dá título; a regra removida sai. O
    /// objetivo acompanha a versão nova do primeiro contexto.
    #[test]
    fn a_removed_or_replaced_rule_leaves_the_titles() {
        let content = [
            spec(),
            ev(7, "09:00", "rule", json!({"text": "**Título novo.** O resto.", "keys": ["k"], "example": "e", "replaces": 4, "origin": 2})),
            ev(8, "09:01", "remove", json!({"targets": [5], "reason": "engano"})),
            ev(9, "09:02", "context", json!({"text": "Objetivo revisto! Detalhe.", "replaces": 3, "origin": 2})),
            ev(10, "09:03", "context", json!({"text": "Um segundo contexto.", "origin": 2})),
        ]
        .concat();
        let got = parsed(&spec_line("teste", &parse_log(&content)).unwrap());
        assert_eq!(got["titles"], json!(["Título novo."]));
        assert_eq!(got["goal"], json!("Objetivo revisto!"));

        let without = [ev(1, "08:40", "message", json!({"author": "user", "text": "oi"}))].concat();
        let bare = parsed(&spec_line("s", &parse_log(&without)).unwrap());
        for field in ["goal", "titles", "phase", "branch"] {
            assert!(bare.get(field).is_none(), "{field}: {bare}");
        }
    }

    /// A linha leva o endereço da última publicação da página que deu certo:
    /// a que falhou porque a página foi apagada não o tira, e a página
    /// publicada de novo num endereço novo passa a valer. Sem publicação que
    /// deu certo, a linha fica sem endereço.
    #[test]
    fn the_index_line_points_at_the_last_page_that_was_published() {
        let publish = |id: u64, hm: &str, ok: bool, url: &str| {
            let draft = if ok {
                json!({"page": "spec", "milestone": "round", "ok": true, "url": url})
            } else {
                json!({"page": "spec", "milestone": "round", "ok": false, "reason": "a página foi apagada"})
            };
            ev(id, hm, "publish", draft)
        };
        let url = |content: &str| parsed(&spec_line("teste", &parse_log(content)).unwrap()).get("url").cloned();
        let failed_only = [spec(), publish(7, "09:00", false, "")].concat();
        assert_eq!(url(&failed_only), None);
        let first = [spec(), publish(7, "09:00", true, "https://claude.ai/a")].concat();
        assert_eq!(url(&first), Some(json!("https://claude.ai/a")));
        let deleted = [first.clone(), publish(8, "09:10", false, "")].concat();
        assert_eq!(url(&deleted), Some(json!("https://claude.ai/a")), "a failed publish keeps the address");
        let again = [deleted, publish(9, "09:20", true, "https://claude.ai/b")].concat();
        assert_eq!(url(&again), Some(json!("https://claude.ai/b")), "the new address wins");
        let line = spec_line("teste", &parse_log(&again)).unwrap();
        let read = project_rows(&format!("{}\n{line}\n", project_line(None)));
        assert_eq!(read[0].url.as_deref(), Some("https://claude.ai/b"));
        assert_eq!(read[0].branch.as_deref(), Some("feature/teste"));
        assert_eq!(read[0].created.as_deref(), Some(at("08:40").as_str()));
        assert_eq!(read[0].updated.as_deref(), Some(at("09:20").as_str()));
    }

    /// A publicação da página do projeto, gravada numa spec, não vira o link
    /// da página dessa spec: a última dela vai para a linha do projeto, e as
    /// linhas das specs e as que não se entendem ficam como estavam.
    #[test]
    fn the_project_page_publish_goes_only_to_the_project_line() {
        let project = |id: u64, hm: &str, url: &str| {
            ev(id, hm, "publish", json!({"page": "project", "milestone": "approval", "ok": true, "url": url}))
        };
        let only_project = [spec(), project(7, "09:00", "https://claude.ai/p0")].concat();
        let log = parse_log(&only_project);
        assert_eq!(spec_page_url(&log), None, "the project page is not the spec page");
        assert!(parsed(&spec_line("teste", &log).unwrap()).get("url").is_none());

        let spec_page = ev(8, "09:01", "publish",
            json!({"page": "spec", "milestone": "approval", "ok": true, "url": "https://claude.ai/spec"}));
        let failed = ev(10, "09:03", "publish",
            json!({"page": "project", "milestone": "round", "ok": false, "reason": "caiu"}));
        let content = [only_project, spec_page, project(9, "09:02", "https://claude.ai/p1"), failed].concat();
        let log = parse_log(&content);
        assert_eq!(spec_page_url(&log), Some("https://claude.ai/spec"));
        let line = spec_line("teste", &log).unwrap();
        assert_eq!(parsed(&line)["url"], json!("https://claude.ai/spec"));
        let when = at("09:02");
        assert_eq!(project_publish(&log), Some((when.as_str(), "https://claude.ai/p1")), "the last one that worked");

        let index = format!("{}
{line}
lixo
", project_line(None));
        let moved = with_project_url(&index, "https://claude.ai/p1");
        assert_eq!(moved, format!("{}
{line}
lixo
", project_line(Some("https://claude.ai/p1"))));
        assert_eq!(project_url(&moved).as_deref(), Some("https://claude.ai/p1"));
    }

    /// O objetivo de uma spec cujo primeiro contexto tem `text`.
    fn goal_with(text: &str) -> Option<String> {
        goal_of(&parse_log(&ev(1, "08:40", "context", json!({"text": text, "origin": 1}))))
    }

    /// Uma linha que é só título, em negrito ou com `#`, não é frase: o
    /// objetivo é a frase que vem depois, sem a marcação. Sem título, é a
    /// primeira frase, como sempre.
    #[test]
    fn the_goal_skips_a_title_line_and_drops_the_bold() {
        assert_eq!(
            goal_with("**Quem usa e para quê ✓**\n\nQuem programa descreve um desejo. O Mustard conduz o resto."),
            Some("Quem programa descreve um desejo.".to_string())
        );
        assert_eq!(goal_with("## Objetivo\r\nDeixar enxuto. Depois."), Some("Deixar enxuto.".to_string()));
        assert_eq!(goal_with("Deixar o Mustard enxuto. O resto vem depois."), Some("Deixar o Mustard enxuto.".to_string()));
        assert_eq!(goal_with("Deixar o **Mustard** enxuto. Depois."), Some("Deixar o Mustard enxuto.".to_string()));
        assert_eq!(goal_with("**Título.** A frase segue."), Some("Título.".to_string()), "a bold opening followed by text on the same line is not a title line");
        assert_eq!(goal_with("#hashtag na frase. Depois."), Some("#hashtag na frase.".to_string()), "a `#` glued to a word is not a heading");
        assert_eq!(goal_with("**Só um título**\n"), None, "a text that is only a title has no sentence");
    }

    #[test]
    fn merging_keeps_the_project_line_first_and_the_specs_sorted_by_name() {
        let b = r#"{"v":1,"type":"spec","name":"b"}"#;
        let a = r#"{"v":1,"type":"spec","name":"a"}"#;
        let first = merge("", "b", Some(b));
        assert_eq!(first, format!("{}\n{b}\n", project_line(None)));
        let second = merge(&first, "a", Some(a));
        assert_eq!(second, format!("{}\n{a}\n{b}\n", project_line(None)));

        // The project line with its address is kept as it is, and a line that
        // does not parse stays, at the end.
        let with_url = format!("{b}\ngarbage\n{}\n", project_line(Some("https://x/p")));
        let merged = merge(&with_url, "a", Some(a));
        assert_eq!(merged, format!("{}\n{a}\n{b}\ngarbage\n", project_line(Some("https://x/p"))));
        assert_eq!(merge(&merged, "b", None), format!("{}\n{a}\ngarbage\n", project_line(Some("https://x/p"))));
        assert_eq!(prune(&merged, |name| name == "b"), format!("{}\n{b}\n", project_line(Some("https://x/p"))));
    }

    #[test]
    fn the_difference_names_the_missing_extra_and_changed_lines() {
        let expected = format!("{}\n{}\n{}\n", project_line(None), r#"{"type":"spec","name":"a"}"#, r#"{"type":"spec","name":"b","v":1}"#);
        let current = format!(
            "{}\n{}\n{}\nlixo\n",
            r#"{"type":"spec","name":"b","v":2}"#,
            r#"{"type":"spec","name":"c"}"#,
            project_line(None),
        );
        assert_eq!(diff(&current, &expected), ["a", "b", "c", "#4"]);
        assert!(diff(&expected, &expected).is_empty());
        assert_eq!(diff("", &expected), ["#1", "a", "b"]);
    }

    #[test]
    fn a_sentence_ends_at_its_stop_never_inside_a_name() {
        assert_eq!(first_sentence("Grava o spec.ndjson inteiro. Depois."), "Grava o spec.ndjson inteiro.");
        assert_eq!(first_sentence("  Sem ponto final  "), "Sem ponto final");
        assert_eq!(first_sentence("Linha um\nLinha dois."), "Linha um");
    }

    #[test]
    fn the_write_warning_says_the_event_is_in_and_names_the_index_command() {
        let refusal = Refusal::Io { detail: "Is a directory".into() };
        let pt = write_warning(&refusal, Locale::PtBr);
        let en = write_warning(&refusal, Locale::EnUs);
        assert!(pt.contains("O evento foi gravado") && pt.contains("Is a directory"), "{pt}");
        assert!(en.contains("The event was written") && en.contains("mustard-rt run index"), "{en}");
    }

    /// O leitor das linhas devolve cada spec pelo nome, com o objetivo, a
    /// fase, os títulos e o `search`, pelo mesmo leitor da gravação: a linha
    /// do projeto, a que não se entende e a de spec sem nome ficam de fora.
    #[test]
    fn an_index_line_that_does_not_parse_is_skipped() {
        let line = spec_line("teste", &parse_log(&spec())).unwrap();
        let content = format!("{}\n{line}\nlixo\n{}\n", project_line(None), r#"{"v":1,"type":"spec"}"#);
        let lines = spec_lines(&content);
        assert_eq!(lines.len(), 1, "{lines:?}");
        let got = &lines[0];
        assert_eq!(got.name, "teste");
        assert_eq!(got.goal.as_deref(), Some("Deixar o Mustard enxuto."));
        assert_eq!(got.phase.as_deref(), Some("running"));
        assert_eq!(got.titles.len(), 2);
        assert_eq!(got.search, parsed(&line)["search"].as_str().unwrap());
    }
}
