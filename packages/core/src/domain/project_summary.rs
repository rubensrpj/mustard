//! O resumo do projeto: o que cada spec entregue deixa ao projeto, tirado do
//! texto que a spec já tem. Nenhum modelo escreve nada aqui: o título é o do
//! pull request ou o objetivo, o que saiu é o resumo do pull request, e cada
//! regra do projeto é o texto que a spec gravou para a decisão ou a regra.
//!
//! A decisão de qual item vale para o projeto inteiro vem de fora, como uma
//! chance de sim por item; aqui só se lê a spec, se recorta e se agrupa.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::domain::spec_events::{SpecEvent, SpecLog, pr_message};
use crate::domain::spec_state::State;
use crate::domain::text::{cut_line, one_line};
use crate::platform::i18n::{Locale, translate};

/// O teto do que saiu, em caracteres.
pub const DELIVERED_CHARS: usize = 600;

/// O teto do título, em caracteres.
pub const TITLE_CHARS: usize = 200;

/// O teto de cada regra, o porquê junto, em caracteres.
pub const RULE_CHARS: usize = 280;

/// O teto do porquê ou do exemplo que acompanha a regra: meia linha.
pub const NOTE_CHARS: usize = 140;

/// O menor espaço que ainda leva o porquê; abaixo dele a regra vai sozinha.
const NOTE_MIN_CHARS: usize = 40;

/// Quantas regras sem caminho uma spec deixa, no máximo: a regra sem caminho
/// carrega em toda conversa.
pub const UNSCOPED_RULES: usize = 3;

/// A chance de sim a partir da qual a resposta vale; abaixo, é dúvida, e a
/// dúvida fica de fora.
pub const SURE: f64 = 0.8;

/// O teto das entregas que o condutor recebe com o objetivo da spec nova,
/// em caracteres: uns 400 tokens.
pub const GOAL_CHARS: usize = 1_600;

/// O pull request da spec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pr {
    pub number: u64,
    #[serde(default)]
    pub url: Option<String>,
}

/// Um arquivo de regras que a spec deixou, com os caminhos do cabeçalho e o
/// texto curto de cada regra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleFile {
    pub file: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub rules: Vec<String>,
}

/// A linha de uma spec no resumo do projeto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub spec: String,
    pub delivered_at: String,
    #[serde(default)]
    pub pr: Option<Pr>,
    pub title: String,
    pub delivered: String,
    #[serde(default)]
    pub rules: Vec<RuleFile>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub cost: Option<Value>,
}

/// Uma decisão ou uma regra da spec, com o texto gravado e os caminhos a que
/// ela se liga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreed {
    pub id: u64,
    pub code: String,
    pub kind: String,
    pub title: String,
    pub text: String,
    /// O porquê da decisão ou o exemplo da regra.
    pub note: String,
    /// Os caminhos do item; vazio quando ele vale para o projeto todo ou não
    /// se liga a arquivo nenhum.
    pub paths: Vec<String>,
}

/// O corpo do pull request numa linha: cada linha sem o marcador de lista,
/// separada da seguinte por ponto e vírgula quando não termina em pontuação.
fn body_line(body: &str) -> String {
    let mut out = String::new();
    for line in body.lines().map(str::trim).map(|line| line.trim_start_matches(['-', '*']).trim()).filter(|line| !line.is_empty()) {
        if !out.is_empty() {
            out.push_str(if out.ends_with(['.', '!', '?', ':', ';']) { " " } else { "; " });
        }
        out.push_str(line);
    }
    out
}

/// O pull request gravado por último na spec.
#[must_use]
pub fn pr_of(log: &SpecLog) -> Option<Pr> {
    log.visible().into_iter().rev().filter(|event| event.event_type == "state").find_map(|event| {
        let pr = event.fields.get("pr")?;
        let number = pr.get("number").and_then(Value::as_u64)?;
        let url = pr.get("url").and_then(Value::as_str).map(str::trim).filter(|url| !url.is_empty()).map(str::to_string);
        Some(Pr { number, url })
    })
}

/// As skills que a spec criou ou atualizou e não descartou depois, em
/// `log`, na ordem.
#[must_use]
pub fn skills_of(log: &SpecLog) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for skill in log.visible().into_iter().filter(|e| e.event_type == "skill") {
        let Some(name) = skill.str_field("name").map(str::trim) else { continue };
        out.retain(|had| had != name);
        if matches!(skill.str_field("action"), Some("created" | "updated")) {
            out.push(name.to_string());
        }
    }
    out
}

/// A spec tem linha no resumo: entregue, ou fechada com o pull request
/// gravado.
#[must_use]
pub fn summarizable(log: &SpecLog) -> bool {
    match State::from_log(log).phase {
        Some("delivered") => true,
        Some("closed" | "pr_open") => pr_of(log).is_some(),
        _ => false,
    }
}

/// Quando a spec foi entregue: a hora do `state` da entrega ou, sem ele, a do
/// pull request aberto; sem nenhum dos dois, a do último evento.
#[must_use]
pub fn delivered_at(log: &SpecLog) -> String {
    let visible = log.visible();
    let at_phase = |phase: &str| {
        visible.iter().rev().find(|e| e.event_type == "state" && e.str_field("phase") == Some(phase)).map(|e| e.at().to_string())
    };
    at_phase("delivered")
        .or_else(|| at_phase("pr_open"))
        .or_else(|| log.events.last().map(|e| e.at().to_string()))
        .unwrap_or_default()
}

/// O título e o que saiu da spec `spec`, em `lang`: o título do pull request
/// ou, sem ele, o objetivo; o resumo que o assistente gravou para o pull
/// request ou, sem ele, o corpo do pull request, cortado em
/// [`DELIVERED_CHARS`] sem partir palavra.
#[must_use]
pub fn title_and_delivered(log: &SpecLog, spec: &str, lang: Locale) -> (String, String) {
    let goal = crate::domain::spec_index::goal_of(log);
    let message = pr_message(log, lang).ok();
    let title = message.as_ref().map(|(title, _)| title.clone()).or_else(|| goal.clone()).unwrap_or_else(|| spec.to_string());
    let summary = log
        .visible()
        .into_iter()
        .rev()
        .find(|e| e.event_type == "pr_summary")
        .and_then(|e| e.str_field("text"))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    let body = summary
        .or_else(|| message.map(|(_, body)| body_line(&body)).filter(|body| !body.is_empty()))
        .or(goal)
        .unwrap_or_default();
    (cut_line(&title, TITLE_CHARS), cut_line(&body, DELIVERED_CHARS))
}

/// Os caminhos de uma lista `files`, como objetos com o caminho ou como texto.
fn listed_paths(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().or_else(|| item.get("path").and_then(Value::as_str)))
        .filter_map(clean_path)
        .collect()
}

/// O caminho como o cabeçalho de uma regra o leva: barras normais, sem o
/// `./` do começo. O vazio e o que não cabe entre aspas ficam de fora.
fn clean_path(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    let path = path.strip_prefix("./").unwrap_or(&path).to_string();
    (!path.is_empty() && !path.contains(['"', '\n', '\r'])).then_some(path)
}

/// O padrão que cobre o projeto inteiro.
fn whole_project(path: &str) -> bool {
    matches!(path, "**" | "**/*" | "*" | ".")
}

/// Os caminhos que o `applies_to` do item diz: `Some` vazio quando ele vale
/// para o projeto todo; `None` quando não diz caminho nenhum, e a ligação vem
/// das tarefas.
fn applied_paths(value: Option<&Value>) -> Option<Vec<String>> {
    let listed: Vec<String> = match value? {
        Value::String(path) if !path.trim().contains(char::is_whitespace) => clean_path(path).into_iter().collect(),
        Value::Object(object) => listed_paths(object.get("files")),
        _ => Vec::new(),
    };
    if listed.iter().any(|path| whole_project(path)) {
        return Some(Vec::new());
    }
    (!listed.is_empty()).then_some(listed)
}

/// Os caminhos do item `item`: os do `applies_to` dele, quando há; senão os
/// arquivos das tarefas que o cobrem; senão os arquivos dos commits das ondas
/// dessas tarefas e das que o item diz.
fn item_paths(log: &SpecLog, item: &SpecEvent, tasks: &[&SpecEvent], commits: &[&SpecEvent]) -> Vec<String> {
    if let Some(applied) = applied_paths(item.fields.get("applies_to")) {
        return applied;
    }
    let covering: Vec<&SpecEvent> = tasks
        .iter()
        .copied()
        .filter(|task| task.ints("covers").into_iter().filter_map(|id| log.current(id).map(|e| e.id)).any(|id| id == item.id))
        .collect();
    let mut paths: BTreeSet<String> = covering.iter().flat_map(|task| listed_paths(task.fields.get("files"))).collect();
    if paths.is_empty() {
        let waves: BTreeSet<u64> = covering.iter().filter_map(|task| task.wave()).chain(item.ints("waves")).collect();
        for commit in commits.iter().filter(|commit| commit.ints("waves").iter().any(|wave| waves.contains(wave))) {
            paths.extend(listed_paths(commit.fields.get("files")));
        }
    }
    paths.into_iter().filter(|path| !whole_project(path)).collect()
}

/// As decisões e as regras vigentes da spec, com o texto gravado, o porquê ou
/// o exemplo, e os caminhos de cada uma.
#[must_use]
pub fn agreed_items(log: &SpecLog) -> Vec<Agreed> {
    let codes = log.codes();
    let visible = log.visible();
    let tasks: Vec<&SpecEvent> = visible.iter().copied().filter(|e| e.event_type == "task").collect();
    let commits: Vec<&SpecEvent> = visible.iter().copied().filter(|e| e.event_type == "commit").collect();
    visible
        .iter()
        .copied()
        .filter(|e| matches!(e.event_type.as_str(), "decision" | "rule"))
        .filter_map(|item| {
            let text = one_line(item.str_field("text").unwrap_or_default());
            if text.is_empty() {
                return None;
            }
            let note_field = if item.event_type == "decision" { "why" } else { "example" };
            Some(Agreed {
                id: item.id,
                code: codes.get(&item.id).cloned().unwrap_or_default(),
                kind: item.event_type.clone(),
                title: one_line(item.str_field("title").unwrap_or_default()),
                text,
                note: one_line(item.str_field(note_field).unwrap_or_default()),
                paths: item_paths(log, item, &tasks, &commits),
            })
        })
        .collect()
}

/// A linha da regra no arquivo: o texto gravado e, se couber, o porquê ou o
/// exemplo em meia linha, tudo em [`RULE_CHARS`].
#[must_use]
pub fn rule_line(item: &Agreed) -> String {
    let text = cut_line(&item.text, RULE_CHARS);
    let room = RULE_CHARS.saturating_sub(text.chars().count() + 3).min(NOTE_CHARS);
    if item.note.is_empty() || room < NOTE_MIN_CHARS {
        return text;
    }
    format!("{text} — {}", cut_line(&item.note, room))
}

/// As regras de um mesmo conjunto de caminhos, que vão num arquivo só.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleGroup {
    pub paths: Vec<String>,
    pub lines: Vec<String>,
}

/// As regras do projeto entre `items`, pela chance de sim de cada um em
/// `chances`: só as de [`SURE`] para cima; das sem caminho, só as
/// [`UNSCOPED_RULES`] de maior chance. As de mesmo conjunto de caminhos se
/// juntam, na ordem dos itens na spec.
#[must_use]
pub fn group_rules(items: &[Agreed], chances: &BTreeMap<u64, f64>) -> Vec<RuleGroup> {
    let sure = |item: &&Agreed| chances.get(&item.id).is_some_and(|chance| *chance >= SURE);
    let mut unscoped: Vec<&Agreed> = items.iter().filter(sure).filter(|item| item.paths.is_empty()).collect();
    unscoped.sort_by(|a, b| chances[&b.id].total_cmp(&chances[&a.id]).then(a.id.cmp(&b.id)));
    let unscoped: BTreeSet<u64> = unscoped.into_iter().take(UNSCOPED_RULES).map(|item| item.id).collect();
    let mut kept: Vec<&Agreed> = items.iter().filter(sure).filter(|item| !item.paths.is_empty() || unscoped.contains(&item.id)).collect();
    kept.sort_by_key(|item| item.id);
    let mut groups: Vec<RuleGroup> = Vec::new();
    for item in kept {
        let line = rule_line(item);
        match groups.iter_mut().find(|group| group.paths == item.paths) {
            Some(group) => group.lines.push(line),
            None => groups.push(RuleGroup { paths: item.paths.clone(), lines: vec![line] }),
        }
    }
    groups
}

/// O arquivo de regras do grupo `group` da spec `spec`: o cabeçalho com os
/// caminhos, quando há, para o Claude Code carregar a regra só com um arquivo
/// que casa; o título com o nome da spec, em `lang`; uma linha por regra.
#[must_use]
pub fn render_rules(spec: &str, group: &RuleGroup, lang: Locale) -> String {
    let mut out = String::new();
    if !group.paths.is_empty() {
        out.push_str("---\npaths:\n");
        for path in &group.paths {
            let _ = writeln!(out, "  - \"{path}\"");
        }
        out.push_str("---\n\n");
    }
    let _ = write!(out, "# {}\n\n", translate("spec_index.rules_title", lang).replace("{spec}", spec));
    for line in &group.lines {
        let _ = writeln!(out, "- {line}");
    }
    out
}

/// A entrega como o condutor e o painel a leem: a spec, o dia, o pull
/// request, o título e o que saiu.
#[must_use]
pub fn shown(entry: &Entry) -> Value {
    json!({
        "spec": entry.spec,
        "delivered_at": entry.delivered_at.get(..10).unwrap_or(&entry.delivered_at),
        "pr": entry.pr.as_ref().map(|pr| pr.number),
        "title": entry.title,
        "delivered": entry.delivered,
    })
}

/// As entregas de `entries`, na ordem, enquanto cabem juntas em `max`
/// caracteres ([`shown`]). A que não cabe sai, e as seguintes ainda podem
/// entrar.
#[must_use]
pub fn within<'a>(entries: impl IntoIterator<Item = &'a Entry>, max: usize) -> Vec<Value> {
    let mut used = 0;
    let mut out = Vec::new();
    for entry in entries {
        let view = shown(entry);
        let size = view.to_string().chars().count();
        if used + size <= max {
            used += size;
            out.push(view);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::spec_events::parse_log;

    fn log(lines: &[Value]) -> SpecLog {
        let mut id = 0;
        let text: Vec<String> = lines
            .iter()
            .map(|line| {
                id += 1;
                let mut line = line.clone();
                line["v"] = json!(1);
                line["id"] = json!(id);
                if line.get("at").is_none() {
                    line["at"] = json!(format!("2026-10-0{}T10:00:00Z", id.min(9)));
                }
                line.to_string()
            })
            .collect();
        parse_log(&text.join("\n"))
    }

    /// A decisão sem `applies_to` se liga aos arquivos das tarefas que a
    /// cobrem; a regra do projeto todo fica sem caminho; a regra sem tarefa
    /// pega os arquivos dos commits das ondas que ela diz.
    #[test]
    fn each_item_takes_its_own_paths_then_the_tasks_then_the_commits() {
        let log = log(&[
            json!({"type":"decision","text":"Os  cartões   ficam no núcleo.","why":"Um lugar só.","keys":["a"]}),
            json!({"type":"rule","text":"Toda resposta sai em JSON.","example":"{\"ok\":true}","keys":["b"],"applies_to":{"files":["**"]}}),
            json!({"type":"rule","text":"Teste novo em inglês.","example":"fn a_test()","keys":["c"],"waves":[2]}),
            json!({"type":"decision","text":"Rotas na pasta api.","why":"Padrão.","keys":["d"],"applies_to":"src/api/**"}),
            json!({"type":"task","title":"t","text":"x","wave":1,"covers":[1],"files":[{"path":"src/cards.rs"},"./src/core.rs"]}),
            json!({"type":"commit","sha":"abc","title":"c","waves":[2],"files":["tests/a.rs"],"repo":"x"}),
        ]);
        let items = agreed_items(&log);
        let paths: Vec<(&str, Vec<&str>)> = items.iter().map(|i| (i.text.as_str(), i.paths.iter().map(String::as_str).collect())).collect();
        assert_eq!(
            paths,
            vec![
                ("Os cartões ficam no núcleo.", vec!["src/cards.rs", "src/core.rs"]),
                ("Toda resposta sai em JSON.", vec![]),
                ("Teste novo em inglês.", vec!["tests/a.rs"]),
                ("Rotas na pasta api.", vec!["src/api/**"]),
            ]
        );
        assert_eq!(items[0].note, "Um lugar só.");
        assert_eq!(items[2].note, "fn a_test()");
    }

    /// Só a chance de 0,8 para cima vira regra; das sem caminho, só as três de
    /// maior chance; as de mesmo caminho vão juntas.
    #[test]
    fn only_sure_items_become_rules_and_at_most_three_without_paths() {
        let item = |id: u64, paths: &[&str]| Agreed {
            id,
            code: String::new(),
            kind: "rule".into(),
            title: String::new(),
            text: format!("Regra {id}."),
            note: String::new(),
            paths: paths.iter().map(|p| (*p).to_string()).collect(),
        };
        let items = vec![item(1, &[]), item(2, &["a.rs"]), item(3, &[]), item(4, &[]), item(5, &[]), item(6, &["a.rs"]), item(7, &["b.rs"])];
        let chances: BTreeMap<u64, f64> = [(1, 0.95), (2, 0.9), (3, 0.79), (4, 0.81), (5, 0.99), (6, 0.8), (7, 0.5)].into_iter().collect();
        // 3 fica em dúvida, 7 fica só na spec; sem caminho, 1, 4 e 5.
        let groups = group_rules(&items, &chances);
        assert_eq!(
            groups,
            vec![
                RuleGroup { paths: vec![], lines: vec!["Regra 1.".into(), "Regra 4.".into(), "Regra 5.".into()] },
                RuleGroup { paths: vec!["a.rs".into()], lines: vec!["Regra 2.".into(), "Regra 6.".into()] },
            ]
        );
        let mut more = items.clone();
        more.push(item(8, &[]));
        let mut chances = chances;
        chances.insert(8, 0.97);
        let unscoped = &group_rules(&more, &chances)[0].lines;
        assert_eq!(unscoped, &vec!["Regra 1.".to_string(), "Regra 5.".into(), "Regra 8.".into()], "the three most likely");
    }

    /// A regra longa é cortada no fim de uma palavra dentro do teto, e o
    /// porquê só vai quando cabe.
    #[test]
    fn a_rule_line_keeps_the_ceiling_and_adds_the_reason_when_it_fits() {
        let short = Agreed {
            id: 1,
            code: String::new(),
            kind: "decision".into(),
            title: String::new(),
            text: "A fila sai por tipo.".into(),
            note: "Assim a ordem é previsível.".into(),
            paths: vec![],
        };
        assert_eq!(rule_line(&short), "A fila sai por tipo. — Assim a ordem é previsível.");
        let long = Agreed { text: "palavra ".repeat(60), ..short };
        let line = rule_line(&long);
        assert!(line.chars().count() <= RULE_CHARS, "{line}");
        assert!(line.ends_with("palavra…"), "{line}");
        assert!(!line.contains('—'), "no room left for the reason: {line}");
    }

    /// O arquivo com caminhos leva o cabeçalho que o Claude Code lê; o sem
    /// caminho não leva cabeçalho e carrega sempre.
    #[test]
    fn the_rules_file_has_paths_only_when_the_group_has_them() {
        let scoped = RuleGroup { paths: vec!["src/api/**".into(), "src/a.rs".into()], lines: vec!["Rotas na pasta api.".into()] };
        assert_eq!(
            render_rules("obra", &scoped, Locale::PtBr),
            "---\npaths:\n  - \"src/api/**\"\n  - \"src/a.rs\"\n---\n\n# Regras da spec obra\n\n- Rotas na pasta api.\n"
        );
        let always = RuleGroup { paths: vec![], lines: vec!["Toda resposta sai em JSON.".into()] };
        assert_eq!(render_rules("obra", &always, Locale::EnUs), "# Rules from spec obra\n\n- Toda resposta sai em JSON.\n");
    }

    /// O título é o do pull request; o que saiu é o resumo gravado para ele,
    /// cortado no teto sem partir palavra; sem resumo, o corpo numa linha.
    #[test]
    fn the_title_and_the_delivery_come_from_the_pull_request_text() {
        let goal = json!({"type":"context","text":"Gravar o resumo do projeto quando a spec é entregue.","title":"Resumo do projeto na entrega"});
        let summary = json!({"type":"pr_summary","text":format!("Grava o resumo. {}", "palavra ".repeat(120))});
        let (title, delivered) = title_and_delivered(&log(&[goal.clone(), summary]), "x", Locale::PtBr);
        assert_eq!(title, "Resumo do projeto na entrega");
        assert!(delivered.starts_with("Grava o resumo. palavra"), "{delivered}");
        assert!(delivered.chars().count() <= DELIVERED_CHARS && delivered.ends_with("palavra…"), "{delivered}");

        let (_, without) = title_and_delivered(&log(&[goal]), "x", Locale::PtBr);
        assert!(!without.is_empty() && !without.contains('\n'), "{without}");
    }

    /// Entregue, ou fechada com o pull request gravado, rende linha; a spec
    /// em obra não.
    #[test]
    fn only_a_delivered_spec_or_one_closed_with_a_pull_request_is_summarized() {
        let state = |phase: &str, pr: Option<Value>| {
            let mut line = json!({"type":"state","phase":phase});
            if let Some(pr) = pr {
                line["pr"] = pr;
            }
            line
        };
        assert!(summarizable(&log(&[state("delivered", None)])));
        let with_pr = log(&[state("closed", None), state("pr_open", Some(json!({"number":12,"url":"https://x/12"})))]);
        assert!(summarizable(&with_pr));
        assert_eq!(pr_of(&with_pr), Some(Pr { number: 12, url: Some("https://x/12".into()) }));
        assert!(!summarizable(&log(&[state("closed", None)])));
        assert!(!summarizable(&log(&[state("running", None)])));
        assert_eq!(delivered_at(&log(&[state("closed", None), json!({"type":"state","phase":"delivered","at":"2026-10-19T09:00:00Z"})])), "2026-10-19T09:00:00Z");
    }

    /// O que cabe no teto entra na ordem; a entrega que passa sai e a seguinte
    /// ainda pode entrar.
    #[test]
    fn the_entries_for_the_goal_fit_the_ceiling() {
        let entry = |spec: &str, chars: usize| Entry {
            spec: spec.into(),
            delivered_at: "2026-10-19T09:00:00Z".into(),
            pr: None,
            title: "T".into(),
            delivered: "x".repeat(chars),
            rules: vec![],
            skills: vec![],
            cost: None,
        };
        let entries = [entry("a", 700), entry("b", 900), entry("c", 600)];
        let kept = within(&entries, GOAL_CHARS);
        let specs: Vec<&str> = kept.iter().map(|v| v["spec"].as_str().unwrap()).collect();
        assert_eq!(specs, vec!["a", "c"]);
        assert_eq!(kept[0]["delivered_at"], "2026-10-19");
    }
}
