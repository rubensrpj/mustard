//! A página do projeto, montada só das linhas do índice das specs.
//!
//! Cada spec é uma linha recolhida, com o nome, o objetivo, o estado e a
//! última mudança, num grupo por fase; em cima, quantas specs há em cada
//! fase. Abrindo a linha, a branch, as datas e o link da página dela quando
//! ela já foi publicada. Depois, as specs abertas sem mudança desde antes de
//! hoje e os títulos das regras e das decisões de cada spec; no rodapé, o
//! caminho do arquivo do índice. Não há link para o arquivo: a página mora
//! no claude.ai e não alcança o disco da máquina.
//!
//! Nada vem do relógio a não ser a data de hoje, que chega pronta: a mesma
//! entrada dá sempre a mesma página.

use chrono::NaiveDate;

use super::{Document, Field, Group, Item, Meta, Node, Section, Status, Tone};
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
    let page = Page { lang };
    let mut body = Vec::new();
    let mut specs = Vec::new();
    if lines.is_empty() {
        specs.push(Node::Paragraph(page.t("page.empty").to_string()));
    } else {
        specs.push(Node::Paragraph(stages(lines, lang)));
        let known = PHASES.iter().map(|phase| Some(*phase));
        for phase in known.chain([None]) {
            let of: Vec<&ProjectRow> = lines
                .iter()
                .filter(|line| match phase {
                    Some(phase) => line.phase.as_deref() == Some(phase),
                    None => line.phase.as_deref().is_none_or(|p| !PHASES.contains(&p)),
                })
                .collect();
            if of.is_empty() {
                continue;
            }
            let title = phase.map_or_else(|| page.t("project.no_phase").to_string(), |p| page.phase(Some(p)));
            let rows = of.iter().map(|line| Node::Item(page.spec_row(line))).collect();
            specs.push(group(format!("specs-{}", phase.unwrap_or("none")), capitalized(&title), rows));
        }
    }
    body.push(section("specs", page.t("project.specs"), specs));

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
        let rows = stalled
            .iter()
            .map(|(days, line)| {
                let mut row = page.row(line);
                row.title = page
                    .t("project.stalled.line")
                    .replace("{spec}", &code(&line.name))
                    .replace("{phase}", &page.phase(line.phase.as_deref()))
                    .replace("{days}", &days.to_string())
                    .replace("{since}", &day(line.updated.as_deref()));
                Node::Item(row)
            })
            .collect();
        let title = page.t("project.stalled");
        body.push(section("stalled", title, vec![group("stalled-all".into(), title.into(), rows)]));
    }

    let titled: Vec<Node> = lines
        .iter()
        .filter(|line| !line.titles.is_empty())
        .map(|line| {
            let mut row = page.row(line);
            row.title = page
                .t("project.titles.summary")
                .replace("{spec}", &line.name)
                .replace("{count}", &line.titles.len().to_string());
            row.text = line.titles.iter().map(|title| format!("- {}", one_line(title))).collect::<Vec<_>>().join("\n");
            Node::Item(row)
        })
        .collect();
    if !titled.is_empty() {
        let title = page.t("project.titles");
        body.push(section("titles", title, vec![group("titles-all".into(), title.into(), titled)]));
    }

    Document {
        lang: lang.as_str().to_string(),
        kind: Some(page.t("project.kind").to_string()),
        title: project.to_string(),
        meta: vec![
            Meta::Pair { label: page.t("project.meta.specs").to_string(), value: lines.len().to_string() },
            Meta::Pair { label: page.t("project.meta.today").to_string(), value: today.to_string() },
        ],
        body,
        footer: Some(page.t("project.footer").replace("{path}", &code(index_path))),
    }
}

struct Page {
    lang: Locale,
}

impl Page {
    fn t(&self, key: &str) -> &'static str {
        translate(key, self.lang)
    }

    fn phase(&self, phase: Option<&str>) -> String {
        phase.map_or_else(|| "—".to_string(), |p| self.t(&format!("page.phase.{p}")).to_string())
    }

    /// A linha recolhida de uma spec: o nome, o estado e a última mudança.
    /// O nome não é endereço: a mesma spec aparece em mais de uma seção.
    fn row(&self, line: &ProjectRow) -> Item {
        let tone = match line.phase.as_deref() {
            Some("running") => Tone::Running,
            Some("approved") => Tone::Good,
            _ => Tone::Plain,
        };
        Item {
            code: line.name.clone(),
            anchored: false,
            title: String::new(),
            status: Some(Status { label: self.phase(line.phase.as_deref()), tone }),
            who: None,
            mark: None,
            date: line.updated.as_deref().map(minute),
            text: String::new(),
            fields: Vec::new(),
        }
    }

    /// A spec na lista das specs: o objetivo no título e, ao abrir, o estado,
    /// a branch, as datas e o link da página publicada.
    fn spec_row(&self, line: &ProjectRow) -> Item {
        let mut row = self.row(line);
        row.title = line.goal.clone().unwrap_or_default();
        let field = |key: &str, value: String| Field { label: self.t(key).to_string(), value };
        row.fields = vec![
            field("project.col.state", self.phase(line.phase.as_deref())),
            field("project.col.branch", line.branch.as_deref().map_or_else(|| "—".to_string(), code)),
            field("project.col.created", day(line.created.as_deref())),
            field("project.col.updated", day(line.updated.as_deref())),
        ];
        if let Some(url) = &line.url {
            row.fields.push(field("page.field.url", format!("[{}]({url})", line.name)));
        }
        row
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
    Node::Section(Section { anchor: Some(anchor.to_string()), heading: heading.to_string(), body })
}

fn group(anchor: String, title: String, body: Vec<Node>) -> Node {
    Node::Group(Group { anchor, title, status: None, summary: String::new(), open: false, body })
}

/// O texto com a primeira letra maiúscula: "em execução" vira "Em execução".
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// A data de uma hora gravada: `2026-09-11`.
fn day(at: Option<&str>) -> String {
    at.and_then(|a| a.get(..10)).map_or_else(|| "—".to_string(), str::to_string)
}

/// Uma hora gravada até o minuto: "2026-09-11 21:03".
fn minute(at: &str) -> String {
    at.get(..16).unwrap_or(at).replace('T', " ")
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

/// Um texto em uma linha só.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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

    fn section_of<'a>(doc: &'a Document, anchor: &str) -> Option<&'a Section> {
        doc.body.iter().find_map(|n| match n {
            Node::Section(s) if s.anchor.as_deref() == Some(anchor) => Some(s),
            _ => None,
        })
    }

    fn field<'a>(item: &'a Item, label: &str) -> Option<&'a str> {
        item.fields.iter().find(|f| f.label == label).map(|f| f.value.as_str())
    }

    /// Cada linha do índice vira uma linha recolhida, no grupo da fase dela,
    /// com o objetivo, o estado, a última mudança e, ao abrir, o link da
    /// página publicada; a spec ainda sem página sai sem link, e o caminho do
    /// índice vai no rodapé, sem link.
    #[test]
    fn each_index_line_is_one_row_with_its_state_and_its_page_link() {
        let lines = [
            line("busca", "running", Some("https://claude.ai/code/artifact/busca"), "2026-09-17"),
            line("trava", "plan", None, "2026-09-17"),
        ];
        let doc = project_document("loja", &lines, ".claude/spec/index.ndjson", "2026-09-17", Locale::PtBr);
        assert_eq!(doc.title, "loja");
        let specs = section_of(&doc, "specs").expect("the specs section");
        let groups: Vec<(&str, &str)> = specs
            .body
            .iter()
            .filter_map(|n| match n {
                Node::Group(g) => Some((g.anchor.as_str(), g.title.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(groups, [("specs-plan", "Plano"), ("specs-running", "Em execução")], "the flow order");
        let rows = Node::items(&specs.body);
        let busca = rows.iter().find(|r| r.code == "busca").expect("busca");
        assert_eq!(busca.title, "Objetivo de busca.");
        assert_eq!(busca.status, Some(Status { label: "em execução".into(), tone: Tone::Running }));
        assert_eq!(busca.date.as_deref(), Some("2026-09-17 10:00"));
        assert!(!busca.anchored, "the spec name is not an address");
        assert_eq!(field(busca, "Endereço"), Some("[busca](https://claude.ai/code/artifact/busca)"));
        let trava = rows.iter().find(|r| r.code == "trava").expect("trava");
        assert_eq!(trava.status.as_ref().map(|s| s.label.as_str()), Some("plano"));
        assert_eq!(field(trava, "Endereço"), None);
        assert_eq!(doc.footer.as_deref(), Some("Índice das specs: `.claude/spec/index.ndjson`"));
        let english = project_document("loja", &lines, ".claude/spec/index.ndjson", "2026-09-17", Locale::EnUs);
        let rows = Node::items(&section_of(&english, "specs").unwrap().body).into_iter().cloned().collect::<Vec<_>>();
        assert_eq!(rows.iter().find(|r| r.code == "busca").and_then(|r| r.status.clone()).map(|s| s.label), Some("running".into()));
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
        let stalled = section_of(&doc, "stalled").expect("the stalled section");
        let said: Vec<&str> = Node::items(&stalled.body).iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            said,
            ["`antiga` (em execução): parada desde 2026-09-10, há 7 d", "`ontem` (plano): parada desde 2026-09-16, há 1 d"]
        );

        let fresh = project_document("loja", &lines[1..2], "i", "2026-09-17", Locale::PtBr);
        assert!(section_of(&fresh, "stalled").is_none());
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

    /// Os títulos das regras e das decisões de cada spec abrem pela linha da
    /// spec, que diz quantos são.
    #[test]
    fn each_spec_titles_open_from_its_own_row() {
        let doc = project_document("loja", &[line("busca", "plan", None, "2026-09-17")], "i", "2026-09-17", Locale::PtBr);
        let titles = section_of(&doc, "titles").expect("the titles section");
        let rows = Node::items(&titles.body);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "busca · 1 títulos");
        assert_eq!(rows[0].text, "- Regra de busca.");
    }
}
