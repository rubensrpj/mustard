//! A página do projeto, montada só das linhas do índice das specs.
//!
//! Uma linha por spec, com o estado e o link da página dela quando ela já foi
//! publicada; em cima, quantas specs há em cada fase e as que estão abertas
//! sem mudança desde antes de hoje; no rodapé, o caminho do arquivo do índice.
//! Não há link para o arquivo: a página mora no claude.ai e não alcança o
//! disco da máquina. Os títulos das regras e das decisões de cada spec vêm
//! recolhidos.
//!
//! Nada vem do relógio a não ser a data de hoje, que chega pronta: a mesma
//! entrada dá sempre a mesma página.

use chrono::NaiveDate;

use super::{Document, Meta, Node, Section, Table};
use crate::domain::spec_events::PHASES;
use crate::domain::spec_index::ProjectRow;
use crate::platform::i18n::{translate, Locale};

/// As fases de uma spec que não anda mais: a que está parada nelas não é
/// alerta.
const FINISHED: &[&str] = &["closed", "pr_open", "delivered", "discarded"];

/// A página do projeto `project`, com as linhas `lines` do índice que mora em
/// `index_path` (relativo ao projeto), vista no dia `today` (`2026-09-17`).
#[must_use]
pub fn project_document(project: &str, lines: &[ProjectRow], index_path: &str, today: &str, lang: Locale) -> Document {
    let t = |key: &str| translate(key, lang);
    let phase = |line: &ProjectRow| {
        line.phase.as_deref().map_or_else(|| "—".to_string(), |p| t(&format!("page.phase.{p}")).to_string())
    };

    let mut body = Vec::new();
    let mut specs = Vec::new();
    if lines.is_empty() {
        specs.push(Node::Paragraph(t("page.empty").to_string()));
    } else {
        specs.push(Node::Paragraph(stages(lines, lang)));
        let rows = lines
            .iter()
            .map(|line| {
                vec![
                    line.url.as_deref().map_or_else(|| code(&line.name), |url| format!("[{}]({url})", line.name)),
                    phase(line),
                    line.branch.as_deref().map_or_else(|| "—".to_string(), code),
                    line.goal.clone().unwrap_or_default(),
                    day(line.created.as_deref()),
                    day(line.updated.as_deref()),
                ]
            })
            .collect();
        specs.push(Node::Table(Table {
            headers: [
                "project.col.spec",
                "project.col.state",
                "project.col.branch",
                "project.col.goal",
                "project.col.created",
                "project.col.updated",
            ]
            .iter()
            .map(|k| t(k).to_string())
            .collect(),
            rows,
        }));
    }
    body.push(section("specs", t("project.specs"), specs));

    let stalled: Vec<(i64, &ProjectRow)> = {
        let mut found: Vec<(i64, &ProjectRow)> = lines
            .iter()
            .filter(|line| line.phase.as_deref().is_none_or(|p| !FINISHED.contains(&p)))
            .filter_map(|line| days_between(line.updated.as_deref()?, today).filter(|d| *d > 0).map(|d| (d, line)))
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        found
    };
    if !stalled.is_empty() {
        let items = stalled
            .iter()
            .map(|(days, line)| {
                vec![Node::Paragraph(
                    t("project.stalled.line")
                        .replace("{spec}", &code(&line.name))
                        .replace("{phase}", &phase(line))
                        .replace("{days}", &days.to_string())
                        .replace("{since}", &day(line.updated.as_deref())),
                )]
            })
            .collect();
        body.push(section("stalled", t("project.stalled"), vec![Node::List { ordered: false, items }]));
    }

    let titled: Vec<Node> = lines
        .iter()
        .filter(|line| !line.titles.is_empty())
        .map(|line| Node::Details {
            summary: t("project.titles.summary")
                .replace("{spec}", &line.name)
                .replace("{count}", &line.titles.len().to_string()),
            body: vec![Node::List {
                ordered: false,
                items: line.titles.iter().map(|title| vec![Node::Paragraph(title.clone())]).collect(),
            }],
        })
        .collect();
    if !titled.is_empty() {
        body.push(section("titles", t("project.titles"), titled));
    }

    Document {
        lang: lang.as_str().to_string(),
        kind: Some(t("project.kind").to_string()),
        title: project.to_string(),
        meta: vec![
            Meta::Pair { label: t("project.meta.specs").to_string(), value: lines.len().to_string() },
            Meta::Pair { label: t("project.meta.today").to_string(), value: today.to_string() },
        ],
        body,
        footer: Some(t("project.footer").replace("{path}", &code(index_path))),
    }
}

/// "Por fase: 1 em execução, 1 fechada, 1 descartada.", as fases na ordem do
/// fluxo.
fn stages(lines: &[ProjectRow], lang: Locale) -> String {
    let counted = PHASES
        .iter()
        .map(|phase| (phase, lines.iter().filter(|l| l.phase.as_deref() == Some(*phase)).count()))
        .filter(|(_, n)| *n > 0)
        .map(|(phase, n)| format!("{n} {}", translate(&format!("page.phase.{phase}"), lang)));
    let unknown = lines.iter().filter(|l| l.phase.as_deref().is_none_or(|p| !PHASES.contains(&p))).count();
    let mut parts: Vec<String> = counted.collect();
    if unknown > 0 {
        parts.push(format!("{unknown} {}", translate("project.no_phase", lang)));
    }
    translate("project.stages", lang).replace("{stages}", &parts.join(", "))
}

fn section(anchor: &str, heading: &str, body: Vec<Node>) -> Node {
    Node::Section(Section { anchor: Some(anchor.to_string()), heading: heading.to_string(), collapsed: None, body })
}

/// A data de uma hora gravada: `2026-09-11`.
fn day(at: Option<&str>) -> String {
    at.and_then(|a| a.get(..10)).map_or_else(|| "—".to_string(), str::to_string)
}

/// Quantos dias inteiros vão da data da hora `at` até `today`.
fn days_between(at: &str, today: &str) -> Option<i64> {
    let from = NaiveDate::parse_from_str(at.get(..10)?, "%Y-%m-%d").ok()?;
    let to = NaiveDate::parse_from_str(today.get(..10)?, "%Y-%m-%d").ok()?;
    Some((to - from).num_days())
}

/// Um nome como código em markdown.
fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(name: &str, phase: &str, url: Option<&str>, updated: &str) -> ProjectRow {
        ProjectRow {
            name: name.to_string(),
            created: Some("2026-09-01T10:00:00-03:00".to_string()),
            updated: Some(format!("{updated}T10:00:00-03:00")),
            goal: Some(format!("Objetivo de {name}.")),
            phase: Some(phase.to_string()),
            branch: Some(format!("feature/{name}")),
            url: url.map(str::to_string),
            titles: vec![format!("Regra de {name}.")],
        }
    }

    fn cells(doc: &Document) -> Vec<Vec<String>> {
        doc.body
            .iter()
            .find_map(|n| match n {
                Node::Section(s) if s.anchor.as_deref() == Some("specs") => s.body.iter().find_map(|b| match b {
                    Node::Table(t) => Some(t.rows.clone()),
                    _ => None,
                }),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Cada linha do índice vira uma linha da tabela, com o estado e o link
    /// da página publicada; a spec ainda sem página sai sem link, e o caminho
    /// do índice vai no rodapé, sem link.
    #[test]
    fn each_index_line_is_one_row_with_its_state_and_its_page_link() {
        let lines = [
            line("busca", "running", Some("https://claude.ai/code/artifact/busca"), "2026-09-17"),
            line("trava", "plan", None, "2026-09-17"),
        ];
        let doc = project_document("loja", &lines, ".claude/spec/index.ndjson", "2026-09-17", Locale::PtBr);
        assert_eq!(doc.title, "loja");
        let rows = cells(&doc);
        assert_eq!(rows[0][0], "[busca](https://claude.ai/code/artifact/busca)");
        assert_eq!(rows[0][1], "em execução");
        assert_eq!(rows[1][0], "`trava`");
        assert_eq!(rows[1][1], "plano");
        assert_eq!(doc.footer.as_deref(), Some("Índice das specs: `.claude/spec/index.ndjson`"));
        let english = project_document("loja", &lines, ".claude/spec/index.ndjson", "2026-09-17", Locale::EnUs);
        assert_eq!(cells(&english)[0][1], "running");
    }

    /// A spec aberta sem mudança desde antes de hoje aparece nas paradas, da
    /// mais antiga para a mais nova; a fechada ou descartada, não.
    #[test]
    fn an_open_spec_without_news_since_before_today_is_flagged() {
        let lines = [
            line("antiga", "running", None, "2026-09-10"),
            line("nova", "survey", None, "2026-09-17"),
            line("fechada", "closed", None, "2026-09-01"),
            line("ontem", "plan", None, "2026-09-16"),
        ];
        let doc = project_document("loja", &lines, "i", "2026-09-17", Locale::PtBr);
        let stalled = doc
            .body
            .iter()
            .find_map(|n| match n {
                Node::Section(s) if s.anchor.as_deref() == Some("stalled") => Some(s),
                _ => None,
            })
            .expect("the stalled section");
        let Node::List { items, .. } = &stalled.body[0] else { panic!("{stalled:?}") };
        let said: Vec<&Node> = items.iter().map(|i| &i[0]).collect();
        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(*said[0], Node::Paragraph("`antiga` (em execução): parada desde 2026-09-10, há 7 d".into()));
        assert_eq!(*said[1], Node::Paragraph("`ontem` (plano): parada desde 2026-09-16, há 1 d".into()));

        let fresh = project_document("loja", &lines[1..2], "i", "2026-09-17", Locale::PtBr);
        assert!(fresh.body.iter().all(|n| !matches!(n, Node::Section(s) if s.anchor.as_deref() == Some("stalled"))));
    }

    /// A contagem por fase segue a ordem do fluxo, e um índice vazio diz que
    /// não há nada.
    #[test]
    fn the_stage_count_follows_the_flow_order() {
        let lines = [line("a", "discarded", None, "2026-09-17"), line("b", "running", None, "2026-09-17")];
        let doc = project_document("loja", &lines, "i", "2026-09-17", Locale::PtBr);
        let Node::Section(specs) = &doc.body[0] else { panic!() };
        assert_eq!(specs.body[0], Node::Paragraph("Por fase: 1 em execução, 1 descartada.".into()));
        let empty = project_document("loja", &[], "i", "2026-09-17", Locale::PtBr);
        let Node::Section(specs) = &empty.body[0] else { panic!() };
        assert_eq!(specs.body, [Node::Paragraph("Nada registrado ainda.".into())]);
    }
}
