//! A árvore de uma página escrita como `.md` ou como `.html`.
//!
//! [`Render::Html`] é a única saída de página do Mustard: a página de uma
//! spec, a do projeto e uma página avulsa escrita em markdown passam por ela,
//! com o mesmo layout, o mesmo script e as mesmas fontes. [`Render::Md`]
//! escreve a mesma árvore como texto.
//!
//! No HTML, cada seção entra no menu lateral com os grupos dela; cada grupo
//! sai recolhido numa linha, com o título, o resumo e a contagem; cada item
//! sai numa linha com o código, o título, a situação e a data, e abre para
//! mostrar o texto e os campos em pares. O pedido enviado a um agente se lê
//! como um arquivo `.md`. Os títulos escritos dentro de um item ficam três
//! níveis abaixo, sob o título do grupo.
//!
//! As duas saídas são função só da árvore: a mesma árvore dá sempre os mesmos
//! bytes.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::view::document::{Document, Group, Item, Meta, Node, Overview, Section, Status, Table, Tone};

use super::markdown::{blocks, inline};
use super::{escape, NavGroup, NavSection, Report};

/// Quantos caracteres o título de uma linha recolhida mostra.
const TITLE_MAX_CHARS: usize = 260;

/// Em que forma a página sai.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Render {
    /// Texto em markdown.
    Md,
    /// A página no layout do Mustard.
    Html,
}

impl Render {
    /// A página `doc` nesta forma.
    #[must_use]
    pub fn render(self, doc: &Document) -> String {
        match self {
            Self::Md => md_document(doc),
            Self::Html => html_document(doc),
        }
    }
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

fn html_document(doc: &Document) -> String {
    let anchors = doc.anchors();
    let mut report = Report::new(doc.title.clone(), "").with_lang(doc.lang.clone());
    if let Some(kind) = &doc.kind {
        report = report.with_kind(kind.clone());
    }
    for meta in &doc.meta {
        report = match meta {
            Meta::Pair { label, value } => report.with_meta(label, value),
            Meta::Note(text) => report.with_note(text),
        };
    }
    let html = Html { anchors: &anchors, lang: doc.lang.parse().unwrap_or(Locale::EnUs) };
    let mut body = String::new();
    let mut numbered = 0;
    for node in &doc.body {
        if let Node::Section(section) = node {
            numbered += 1;
            let id = section.anchor.clone().unwrap_or_else(|| format!("section-{numbered}"));
            report.nav(html.nav(&id, section));
            html.section(&id, section, &mut body);
        } else {
            html.node(node, "", &mut body);
        }
    }
    if let Some(footer) = &doc.footer {
        let _ = write!(body, "<footer>{}</footer>", html.inline(footer));
    }
    report.raw(&body);
    report.render()
}

struct Html<'a> {
    anchors: &'a BTreeSet<String>,
    lang: Locale,
}

impl Html<'_> {
    fn inline(&self, text: &str) -> String {
        inline(text, self.anchors)
    }

    /// A seção no menu lateral: o título, quantos itens ela tem e os grupos.
    fn nav(&self, id: &str, section: &Section) -> NavSection {
        let groups = section
            .body
            .iter()
            .filter_map(|node| match node {
                Node::Group(group) => Some(NavGroup {
                    id: group.anchor.clone(),
                    title: plain(&group.title),
                    count: rows(&group.body),
                }),
                _ => None,
            })
            .collect();
        NavSection { id: id.to_string(), title: plain(&section.heading), count: rows(&section.body), groups }
    }

    /// Uma seção: o título com a contagem dos itens, e os blocos.
    fn section(&self, id: &str, section: &Section, out: &mut String) {
        let crumb = plain(&section.heading);
        let _ = write!(
            out,
            "<section id=\"{}\" class=\"block\" data-crumb=\"{}\"><h2><span>{}</span>{}</h2>",
            escape(id),
            escape(&crumb),
            self.inline(&section.heading),
            self.count(rows(&section.body)),
        );
        self.nodes(&section.body, &crumb, out);
        out.push_str("</section>");
    }

    /// A contagem das linhas de uma seção ou de um grupo; sem linha, nada.
    fn count(&self, n: usize) -> String {
        if n == 0 {
            return "<span class=\"count\"></span>".to_string();
        }
        let key = if n == 1 { "page.count.one" } else { "page.count.many" };
        format!("<span class=\"count\">{}</span>", escape(&translate(key, self.lang).replace("{n}", &n.to_string())))
    }

    /// Os blocos em ordem; `crumb` é o caminho até eles, para o título da
    /// barra.
    fn nodes(&self, nodes: &[Node], crumb: &str, out: &mut String) {
        for node in nodes {
            self.node(node, crumb, out);
        }
    }

    fn node(&self, node: &Node, crumb: &str, out: &mut String) {
        match node {
            Node::Section(section) => {
                let id = section.anchor.clone().unwrap_or_default();
                self.section(&id, section, out);
            }
            Node::Group(group) => self.group(group, crumb, out),
            Node::Overview(overview) => self.overview(overview, out),
            Node::Item(item) => self.item(item, out),
            Node::Heading { level, text } => {
                let level = (*level).clamp(3, 6);
                let _ = write!(out, "<h{level}>{}</h{level}>", self.inline(text));
            }
            Node::Paragraph(text) => {
                let _ = write!(out, "<p>{}</p>", self.inline(text));
            }
            Node::List { ordered, items } => {
                let tag = if *ordered { "ol" } else { "ul" };
                let _ = write!(out, "<{tag}>");
                for item in items {
                    out.push_str("<li>");
                    self.list_item(item, crumb, out);
                    out.push_str("</li>");
                }
                let _ = write!(out, "</{tag}>");
            }
            Node::Table(table) => self.table(table, out),
            Node::Code(text) => {
                let _ = write!(out, "<pre>{}</pre>", escape(text));
            }
            Node::Markdown(text) => {
                out.push_str("<div class=\"md\">");
                self.markdown(text, out);
                out.push_str("</div>");
            }
            Node::Details { summary, body, .. } => self.request(summary, body, crumb, out),
            Node::Quote(inner) => {
                out.push_str("<div class=\"callout\">");
                self.nodes(inner, crumb, out);
                out.push_str("</div>");
            }
            Node::Rule => out.push_str("<hr>"),
        }
    }

    /// Um item de lista curto sai sem parágrafo: o primeiro parágrafo vai
    /// solto no item, e uma lista aninhada vem logo depois dele.
    fn list_item(&self, body: &[Node], crumb: &str, out: &mut String) {
        match body.split_first() {
            Some((Node::Paragraph(first), rest))
                if !rest.iter().any(|n| matches!(n, Node::Paragraph(_))) =>
            {
                out.push_str(&self.inline(first));
                self.nodes(rest, crumb, out);
            }
            _ => self.nodes(body, crumb, out),
        }
    }

    fn table(&self, table: &Table, out: &mut String) {
        // A moldura `.table` dá a borda arredondada e a rolagem horizontal.
        out.push_str("<div class=\"table\"><table><thead><tr>");
        for header in &table.headers {
            let _ = write!(out, "<th>{}</th>", self.inline(header));
        }
        out.push_str("</tr></thead><tbody>");
        for row in &table.rows {
            out.push_str("<tr>");
            for cell in row {
                let _ = write!(out, "<td>{}</td>", self.inline(cell));
            }
            out.push_str("</tr>");
        }
        out.push_str("</tbody></table></div>");
    }

    /// Um grupo recolhido: a linha com o título, a situação e o resumo, e a
    /// contagem dos itens; os blocos abrem por baixo.
    fn group(&self, group: &Group, crumb: &str, out: &mut String) {
        let path = if crumb.is_empty() { plain(&group.title) } else { format!("{crumb} / {}", plain(&group.title)) };
        let lead = group.status.as_ref().map_or_else(String::new, |status| format!("{} ", tag(status)));
        let _ = write!(
            out,
            "<details class=\"group\" id=\"{}\" data-crumb=\"{}\"{}><summary><span class=\"gt\">{}</span>\
<span class=\"gs\">{lead}{}</span>{}</summary><div class=\"gbody\">",
            escape(&group.anchor),
            escape(&path),
            if group.open { " open" } else { "" },
            escape(&plain(&group.title)),
            escape(&plain(&group.summary)),
            self.count(rows(&group.body)),
        );
        self.nodes(&group.body, &path, out);
        out.push_str("</div></details>");
    }

    /// A visão geral: a conta das situações e uma ficha por parte, que leva
    /// ao grupo dela.
    fn overview(&self, overview: &Overview, out: &mut String) {
        let _ = write!(
            out,
            "<div class=\"overview\"><div class=\"ov-head\"><span>{}</span><span class=\"muted\">{}</span></div><ol class=\"wgrid\">",
            escape(&overview.title),
            escape(&overview.legend),
        );
        for card in &overview.cards {
            let _ = write!(
                out,
                "<li><button type=\"button\" class=\"{}\" data-go=\"{}\" title=\"{}\"><b>{}</b><span>{}</span></button></li>",
                classes("w", card.status.tone),
                escape(&card.target),
                escape(&plain(&card.hint)),
                escape(&card.label),
                escape(&card.status.label),
            );
        }
        out.push_str("</ol></div>");
    }

    /// O item numa linha recolhida: o código (que é o endereço dele), o
    /// título, a situação, de quem é e a data; o texto e os campos abrem por
    /// baixo. A versão antiga de um item sai apagada.
    fn item(&self, item: &Item, out: &mut String) {
        let old = item.status.as_ref().is_some_and(|s| s.tone == Tone::Old);
        let class = if old { "item old" } else { "item" };
        let id = if item.anchored { format!(" id=\"{}\"", escape(&item.code)) } else { String::new() };
        let _ = write!(
            out,
            "<details class=\"{class}\"{id}><summary><code class=\"c\">{}</code><span class=\"t\">{}</span><span class=\"tail\">",
            escape(&item.code),
            escape(&shortened(&plain(&item.title))),
        );
        if let Some(status) = &item.status {
            out.push_str(&tag(status));
        }
        if let Some(who) = item.who.as_deref().filter(|_| !old) {
            let _ = write!(out, "<span class=\"who\">{}</span>", escape(who));
        }
        if let Some(date) = &item.date {
            let _ = write!(out, "<span class=\"when\">{}</span>", escape(&day_and_minute(date)));
        }
        out.push_str("</span></summary><div class=\"body\">");
        if !item.text.is_empty() {
            out.push_str("<div class=\"prose\">");
            // Os títulos do texto ficam abaixo do título do grupo.
            self.nodes(&demoted(blocks(&item.text)), "", out);
            out.push_str("</div>");
        }
        if !item.fields.is_empty() {
            out.push_str("<dl class=\"kv\">");
            for field in &item.fields {
                let _ = write!(out, "<dt>{}</dt><dd>{}</dd>", escape(&field.label), self.inline(&field.value));
            }
            out.push_str("</dl>");
        }
        out.push_str("</div></details>");
    }

    /// Um trecho recolhido, como o pedido enviado a um agente: uma linha como
    /// a de um item, que abre para ler o pedido como um arquivo `.md`.
    fn request(&self, summary: &str, body: &[Node], crumb: &str, out: &mut String) {
        let _ = write!(
            out,
            "<details class=\"item prompt\"><summary><code class=\"c\">{}</code><span class=\"t\">{}</span>\
<span class=\"tail\"></span></summary><div class=\"body md\">",
            escape(translate("page.request", self.lang)),
            escape(&plain(summary)),
        );
        for node in body {
            match node {
                Node::Markdown(text) => self.markdown(text, out),
                other => self.node(other, crumb, out),
            }
        }
        out.push_str("</div></details>");
    }

    /// Um documento em markdown, formatado como um arquivo `.md`: cada título
    /// vira uma linha em destaque do tamanho dele.
    fn markdown(&self, text: &str, out: &mut String) {
        for node in blocks(text) {
            match node {
                Node::Heading { level, text } => {
                    let _ = write!(out, "<p class=\"mh h{}\">{}</p>", level.clamp(1, 6), self.inline(&text));
                }
                other => self.node(&other, "", out),
            }
        }
    }
}

/// Quantas linhas recolhidas os blocos têm, também dentro dos grupos: cada
/// item e cada pedido. É o que a busca da página conta.
fn rows(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Node::Item(_) | Node::Details { .. } => 1,
            Node::Group(group) => rows(&group.body),
            _ => 0,
        })
        .sum()
}

/// A etiqueta de uma situação.
fn tag(status: &Status) -> String {
    format!("<span class=\"{}\">{}</span>", classes("tag", status.tone), escape(&status.label))
}

/// A classe de base e a do tom, quando o tom tem uma.
fn classes(base: &str, tone: Tone) -> String {
    let extra = match tone {
        Tone::Plain | Tone::Old => "",
        Tone::Good => "ok",
        Tone::Bad => "no",
        Tone::Running => "run",
        Tone::Todo => "todo",
        Tone::Done => "done",
    };
    if extra.is_empty() { base.to_string() } else { format!("{base} {extra}") }
}

/// O markdown de linha como texto puro, em uma linha: sem crases, sem
/// negrito e com o texto de cada link.
fn plain(text: &str) -> String {
    let html = inline(text, &BTreeSet::new());
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// O título cortado no tamanho da linha recolhida, com "…" no fim.
fn shortened(title: &str) -> String {
    if title.chars().count() <= TITLE_MAX_CHARS {
        return title.to_string();
    }
    let cut: String = title.chars().take(TITLE_MAX_CHARS - 3).collect();
    format!("{}…", cut.trim_end())
}

/// "2026-09-11 21:03" como a linha recolhida mostra: "11/09 21:03".
fn day_and_minute(date: &str) -> String {
    match (date.get(5..7), date.get(8..10), date.get(11..16)) {
        (Some(month), Some(day), Some(time)) => format!("{day}/{month} {time}"),
        (Some(month), Some(day), None) => format!("{day}/{month}"),
        _ => date.to_string(),
    }
}

/// Os blocos com cada título três níveis abaixo: o `#` de um texto escrito
/// dentro de um item vira um título menor que o do grupo.
fn demoted(nodes: Vec<Node>) -> Vec<Node> {
    nodes
        .into_iter()
        .map(|node| match node {
            Node::Heading { level, text } => Node::Heading { level: level.saturating_add(3), text },
            Node::List { ordered, items } => {
                Node::List { ordered, items: items.into_iter().map(demoted).collect() }
            }
            Node::Quote(inner) => Node::Quote(demoted(inner)),
            other => other,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

fn md_document(doc: &Document) -> String {
    let mut out = format!("# {}\n", doc.title);
    if !doc.meta.is_empty() {
        let parts: Vec<String> = doc
            .meta
            .iter()
            .map(|meta| match meta {
                Meta::Pair { label, value } => format!("{label}: **{value}**"),
                Meta::Note(text) => text.clone(),
            })
            .collect();
        let _ = write!(out, "\n{}\n", parts.join(" · "));
    }
    let body = md_blocks(&doc.body);
    if !body.is_empty() {
        let _ = write!(out, "\n{body}\n");
    }
    if let Some(footer) = &doc.footer {
        let _ = write!(out, "\n---\n\n{footer}\n");
    }
    out
}

fn md_blocks(nodes: &[Node]) -> String {
    nodes.iter().map(md_node).collect::<Vec<_>>().join("\n\n")
}

fn md_node(node: &Node) -> String {
    match node {
        Node::Section(section) => titled(&format!("## {}", section.heading), &md_blocks(&section.body)),
        Node::Group(group) => titled(&format!("### {}", group.title), &md_blocks(&group.body)),
        Node::Overview(overview) => md_overview(overview),
        Node::Heading { level, text } => format!("{} {text}", "#".repeat(usize::from((*level).clamp(3, 6)))),
        Node::Paragraph(text) => text.clone(),
        Node::List { ordered, items } => items
            .iter()
            .enumerate()
            .map(|(i, body)| {
                let marker = if *ordered { format!("{}. ", i + 1) } else { "- ".to_string() };
                hang(&marker, &md_blocks(body))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Node::Table(table) => md_table(table),
        Node::Code(text) | Node::Markdown(text) => fenced(text),
        Node::Details { summary, body, .. } => titled(&format!("**{summary}**"), &md_blocks(body)),
        Node::Quote(inner) => md_blocks(inner)
            .lines()
            .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
            .collect::<Vec<_>>()
            .join("\n"),
        Node::Rule => "---".to_string(),
        Node::Item(item) => md_item(item),
    }
}

/// Um título e, quando há, os blocos abaixo dele.
fn titled(head: &str, body: &str) -> String {
    if body.is_empty() { head.to_string() } else { format!("{head}\n\n{body}") }
}

/// A visão geral: a conta das situações e uma linha por ficha.
fn md_overview(overview: &Overview) -> String {
    let cards: Vec<String> = overview
        .cards
        .iter()
        .map(|card| {
            let name = if card.hint.is_empty() { String::new() } else { format!(" — {}", plain(&card.hint)) };
            format!("- {}: {}{name}", card.label, card.status.label)
        })
        .collect();
    format!("**{}**: {}\n\n{}", overview.title, overview.legend, cards.join("\n"))
}

/// Um bloco cercado por crases a mais do que qualquer sequência de crases que
/// abre uma linha dele.
fn fenced(text: &str) -> String {
    let longest = text
        .lines()
        .map(|l| l.trim_start().chars().take_while(|c| *c == '`').count())
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}\n{text}\n{fence}")
}

/// O texto depois do marcador, e cada linha seguinte recuada até ele.
fn hang(marker: &str, text: &str) -> String {
    let pad = " ".repeat(marker.chars().count());
    let mut lines = text.lines();
    let mut out = format!("{marker}{}", lines.next().unwrap_or_default());
    for line in lines {
        out.push('\n');
        if !line.is_empty() {
            out.push_str(&pad);
            out.push_str(line);
        }
    }
    out
}

fn md_table(table: &Table) -> String {
    let row = |cells: &[String]| {
        let cells: Vec<String> =
            cells.iter().map(|c| c.split_whitespace().collect::<Vec<_>>().join(" ").replace('|', "\\|")).collect();
        format!("| {} |", cells.join(" | "))
    };
    let mut out = vec![row(&table.headers), format!("|{}", "---|".repeat(table.headers.len()))];
    out.extend(table.rows.iter().map(|r| row(r)));
    out.join("\n")
}

fn md_item(item: &Item) -> String {
    let mut head = format!("**{}**", item.code);
    if let Some(note) = item.note() {
        let _ = write!(head, " · {note}");
    }
    let text = &item.text;
    let mut body = if text.is_empty() {
        head
    } else if text.contains('\n') {
        format!("{head}\n\n{text}")
    } else {
        format!("{head} — {text}")
    };
    if !item.fields.is_empty() {
        let fields: Vec<String> = item.fields.iter().map(|f| format!("- {}: {}", f.label, f.value)).collect();
        let _ = write!(body, "\n\n{}", fields.join("\n"));
    }
    hang("- ", &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::view::document::Card;

    fn field(label: &str, value: &str) -> mustard_core::view::document::Field {
        mustard_core::view::document::Field { label: label.into(), value: value.into() }
    }

    /// Um item sem situação, sem marca e sem campo: só o que o teste precisa.
    fn item(code: &str, anchored: bool, title: &str, text: &str) -> Item {
        Item {
            code: code.into(),
            anchored,
            title: title.into(),
            status: None,
            who: None,
            mark: None,
            date: None,
            text: text.into(),
            fields: Vec::new(),
        }
    }

    fn section(anchor: &str, heading: &str, body: Vec<Node>) -> Node {
        Node::Section(Section { anchor: Some(anchor.into()), heading: heading.into(), body })
    }

    fn group(anchor: &str, title: &str, status: Option<Status>, summary: &str, open: bool, body: Vec<Node>) -> Node {
        Node::Group(Group { anchor: anchor.into(), title: title.into(), status, summary: summary.into(), open, body })
    }

    fn doc(body: Vec<Node>) -> Document {
        Document { lang: "pt-BR".into(), kind: None, title: "demo".into(), meta: Vec::new(), footer: None, body }
    }

    /// A mesma árvore dá sempre os mesmos bytes, nas duas formas.
    #[test]
    fn the_same_events_give_the_same_bytes_twice() {
        let tree = doc(vec![section("agreed", "Combinado", vec![Node::Item(item("MSTD-RULE-0001", true, "t", "Regra."))])]);
        assert_eq!(Render::Md.render(&tree), Render::Md.render(&tree));
        assert_eq!(Render::Html.render(&tree), Render::Html.render(&tree));
    }

    /// Uma decisão revista: a página e o `.md` mostram a versão nova no
    /// combinado; a antiga aparece só na conversa, apagada na página e
    /// marcada como substituída no `.md`.
    #[test]
    fn a_revised_decision_shows_only_the_new_version_outside_the_conversation() {
        let new = item("MSTD-DEC-0001", true, "t", "Versão nova, que segue.");
        let old = Item {
            status: Some(Status { label: "versão antiga".into(), tone: Tone::Old }),
            mark: Some("versão substituída".into()),
            date: Some("2026-09-11 09:01".into()),
            ..item("MSTD-DEC-0001", false, "Versão antiga da decisão.", "Versão antiga da decisão.")
        };
        let tree = doc(vec![
            section("agreed", "Combinado", vec![Node::Item(new)]),
            section("conversation", "Conversa", vec![Node::Item(old)]),
        ]);
        for (page, conversation, marker) in [
            (Render::Html.render(&tree), "<section id=\"conversation\" class=\"block\"", "<span class=\"tag\">versão antiga</span>"),
            (Render::Md.render(&tree), "## Conversa", "versão substituída"),
        ] {
            let (before, talk) = page.split_once(conversation).unwrap_or_else(|| panic!("{page}"));
            assert!(before.contains("Versão nova"), "{before}");
            assert!(!before.contains("Versão antiga"), "{before}");
            assert!(talk.contains("Versão antiga da decisão."), "{talk}");
            assert!(talk.contains(marker), "{talk}");
        }
        let html = Render::Html.render(&tree);
        assert_eq!(html.matches("id=\"MSTD-DEC-0001\"").count(), 1, "one address per code");
        assert!(
            html.contains("<details class=\"item old\"><summary><code class=\"c\">MSTD-DEC-0001</code><span class=\"t\">Versão antiga da decisão.</span><span class=\"tail\"><span class=\"tag\">versão antiga</span><span class=\"when\">11/09 09:01</span></span></summary>"),
            "{html}"
        );
    }

    /// Um registro de remoção, que fica na conversa, diz o código do que
    /// tirou; o texto do item removido não é dele.
    #[test]
    fn a_removed_item_leaves_the_page_and_the_md() {
        let removal = Item { fields: vec![field("Motivo", "engano"), field("Alvo", "MSTD-NOTE-0001")], ..item("MSTD-RMV-0001", true, "t", "") };
        let tree = doc(vec![section("conversation", "Conversa", vec![Node::Item(removal)])]);
        for page in [Render::Html.render(&tree), Render::Md.render(&tree)] {
            assert!(!page.contains("Anotação que sai."), "{page}");
            assert!(page.contains("MSTD-NOTE-0001"), "the removal names what it took out");
        }
    }

    /// No HTML, todo código com item na página vira link para ele; o item
    /// leva o código como endereço. O `.md` mostra o código como texto.
    #[test]
    fn codes_link_to_their_items_on_the_page() {
        let rule =
            Item { fields: vec![field("Exemplo", "`rm -rf pasta` é barrado."), field("Origem", "MSTD-MSG-0001")], ..item("MSTD-RULE-0001", true, "t", "A trava lê o comando.") };
        let decision = item("MSTD-DEC-0001", true, "t", "Versão nova, que segue MSTD-RULE-0001.");
        let tree = doc(vec![section("agreed", "Combinado", vec![Node::Item(rule), Node::Item(decision)])]);
        let html = Render::Html.render(&tree);
        assert!(html.contains("<details class=\"item\" id=\"MSTD-RULE-0001\"><summary><code class=\"c\">MSTD-RULE-0001</code>"), "{html}");
        assert!(html.contains("segue <a href=\"#MSTD-RULE-0001\">MSTD-RULE-0001</a>."), "{html}");
        let md = Render::Md.render(&tree);
        assert!(md.contains("- **MSTD-RULE-0001** — A trava lê o comando."), "{md}");
        assert!(md.contains("  - Exemplo: `rm -rf pasta` é barrado."), "{md}");
        assert!(md.contains("  - Origem: MSTD-MSG-0001"), "{md}");
    }

    /// Um código citado na prova de um critério vira link para o item, também
    /// quando a prova inteira sai como código.
    #[test]
    fn a_code_cited_in_a_criterion_proof_links_to_its_item() {
        let criterion = Item { fields: vec![field("Prova", "`teste da trava (MSTD-RULE-0001)`")], ..item("MSTD-CRIT-0001", true, "t", "") };
        let rule = item("MSTD-RULE-0001", true, "t", "");
        let tree = doc(vec![section("criteria", "Critérios", vec![Node::Item(criterion), Node::Item(rule)])]);
        let html = Render::Html.render(&tree);
        assert!(html.contains("<code>teste da trava (<a href=\"#MSTD-RULE-0001\">MSTD-RULE-0001</a>)</code>"), "{html}");
    }

    /// Nada do relógio nem da máquina: a página só tem o que os eventos
    /// gravaram.
    #[test]
    fn the_page_carries_no_clock_and_no_machine_path() {
        let tree = doc(vec![section("agreed", "Combinado", vec![Node::Item(item("MSTD-RULE-0001", true, "t", "Regra."))])]);
        for page in [Render::Html.render(&tree), Render::Md.render(&tree)] {
            assert!(!page.contains("/home/") && !page.contains("C:\\"), "{page}");
            assert!(!page.contains(&mustard_core::time::now_iso8601()[..10]));
        }
    }

    /// Cada seção entra no menu lateral com os grupos dela e a contagem; cada
    /// grupo sai recolhido numa linha com o título, o resumo e a contagem, e
    /// só a medição abre aberta.
    #[test]
    fn the_menu_lists_the_sections_and_their_groups_and_groups_come_collapsed() {
        let tree = doc(vec![
            section("progress", "Andamento", vec![group("progress-metrics", "Medição", None, "", true, vec![])]),
            section(
                "agreed",
                "Combinado",
                vec![
                    group("agreed-rule", "Regras", None, "", false, vec![Node::Item(item("MSTD-RULE-0001", true, "t", ""))]),
                    group("agreed-decision", "Decisões", None, "", false, vec![Node::Item(item("MSTD-DEC-0001", true, "t", ""))]),
                ],
            ),
        ]);
        let html = Render::Html.render(&tree);
        assert!(
            html.contains(
                "<li data-sec=\"agreed\"><button type=\"button\" data-go=\"agreed\" class=\"top\"><span>Combinado</span><i>2</i></button><ol>\
                 <li><button type=\"button\" data-go=\"agreed-rule\" data-parent=\"agreed\"><span>Regras</span><i>1</i></button></li>\
                 <li><button type=\"button\" data-go=\"agreed-decision\" data-parent=\"agreed\"><span>Decisões</span><i>1</i></button></li></ol></li>"
            ),
            "{html}"
        );
        assert!(
            html.contains(
                "<section id=\"agreed\" class=\"block\" data-crumb=\"Combinado\"><h2><span>Combinado</span><span class=\"count\">2 itens</span></h2>\
                 <details class=\"group\" id=\"agreed-rule\" data-crumb=\"Combinado / Regras\"><summary><span class=\"gt\">Regras</span>\
                 <span class=\"gs\"></span><span class=\"count\">1 item</span></summary><div class=\"gbody\"><details class=\"item\" id=\"MSTD-RULE-0001\">"
            ),
            "{html}"
        );
        let open: Vec<&str> = html.match_indices("<details class=\"group\"").map(|(at, _)| &html[at..]).filter(|tail| {
            tail.split_once('>').is_some_and(|(head, _)| head.ends_with(" open"))
        }).map(|tail| tail.split('"').nth(3).unwrap_or_default()).collect();
        assert_eq!(open, ["progress-metrics"], "only the measurement opens");
    }

    /// Cada item é uma linha com o código, o título, a situação e a data, e
    /// abre para mostrar o texto e os campos em pares de rótulo e valor. O
    /// título sai sem marcas de markdown e cortado no tamanho da linha.
    #[test]
    fn each_item_is_one_row_with_code_title_status_and_date() {
        let long = "palavra ".repeat(60);
        let verdict = Item {
            status: Some(Status { label: "reprovada".into(), tone: Tone::Bad }),
            date: Some("2026-09-12 10:03".into()),
            fields: vec![field("Onda", "1")],
            ..item("MSTD-VERD-0001", true, "Faltou o teste.", "**Faltou** o `teste`.\n\nO resto.")
        };
        let note = item("MSTD-NOTE-0001", true, &long, &long);
        let message = Item {
            who: Some("mensagem · usuário".into()),
            date: Some("2026-09-11 08:40".into()),
            ..item("MSTD-MSG-0001", true, "t", "Revise tudo")
        };
        let tree = doc(vec![
            section("review", "Revisão e QA", vec![group("review-1", "Onda 1", None, "1 reprovada", false, vec![Node::Item(verdict)])]),
            section("notes", "Anotações", vec![Node::Item(note)]),
            section("conversation", "Conversa", vec![Node::Item(message)]),
        ]);
        let html = Render::Html.render(&tree);
        assert!(
            html.contains(
                "<details class=\"item\" id=\"MSTD-VERD-0001\"><summary><code class=\"c\">MSTD-VERD-0001</code>\
                 <span class=\"t\">Faltou o teste.</span><span class=\"tail\"><span class=\"tag no\">reprovada</span>\
                 <span class=\"when\">12/09 10:03</span></span></summary><div class=\"body\"><div class=\"prose\">\
                 <p><strong>Faltou</strong> o <code>teste</code>.</p><p>O resto.</p></div><dl class=\"kv\"><dt>Onda</dt><dd>1</dd>"
            ),
            "{html}"
        );
        assert!(html.contains("<details class=\"group\" id=\"review-1\" data-crumb=\"Revisão e QA / Onda 1\"><summary><span class=\"gt\">Onda 1</span><span class=\"gs\">1 reprovada</span>"), "{html}");
        let cut = format!("<span class=\"t\">{}p…</span>", "palavra ".repeat(32));
        assert!(html.contains(&cut), "the long title is cut: {html}");
        assert!(
            html.contains("<span class=\"tail\"><span class=\"who\">mensagem · usuário</span><span class=\"when\">11/09 08:40</span></span>"),
            "the conversation says what and whose it is: {html}"
        );
    }

    /// A visão das ondas abre o andamento: uma ficha por onda, com o estado
    /// dela na cor do tom e o atalho para o grupo dela; o grupo da onda abre
    /// com o estado e o nome.
    #[test]
    fn the_wave_overview_shows_each_state_and_leads_to_its_group() {
        let overview = Overview {
            title: "Ondas".into(),
            legend: "1 reprovada · 1 a fazer".into(),
            cards: vec![
                Card { target: "waves-1".into(), label: "1".into(), status: Status { label: "reprovada".into(), tone: Tone::Bad }, hint: "A trava nova.".into() },
                Card { target: "waves-2".into(), label: "2".into(), status: Status { label: "a fazer".into(), tone: Tone::Todo }, hint: "Dois.".into() },
            ],
        };
        let tree = doc(vec![
            section(
                "progress",
                "Andamento",
                vec![Node::Overview(overview), group("progress-state", "Fases e publicações", None, "", false, vec![Node::Item(item("MSTD-STATE-0001", true, "t", "Fase."))])],
            ),
            section(
                "waves",
                "Ondas",
                vec![
                    group("waves-1", "Onda 1", Some(Status { label: "reprovada".into(), tone: Tone::Bad }), "A trava nova.", false, vec![Node::Item(item("MSTD-WAVE-0001", true, "t", "A trava nova."))]),
                    group("waves-2", "Onda 2", None, "", false, vec![]),
                ],
            ),
        ]);
        let html = Render::Html.render(&tree);
        assert!(
            html.contains(
                "<h2><span>Andamento</span><span class=\"count\">1 item</span></h2><div class=\"overview\"><div class=\"ov-head\">\
                 <span>Ondas</span><span class=\"muted\">1 reprovada · 1 a fazer</span></div><ol class=\"wgrid\">\
                 <li><button type=\"button\" class=\"w no\" data-go=\"waves-1\" title=\"A trava nova.\"><b>1</b><span>reprovada</span></button></li>\
                 <li><button type=\"button\" class=\"w todo\" data-go=\"waves-2\" title=\"Dois.\"><b>2</b><span>a fazer</span></button></li></ol></div>"
            ),
            "{html}"
        );
        assert!(
            html.contains("<details class=\"group\" id=\"waves-1\" data-crumb=\"Ondas / Onda 1\"><summary><span class=\"gt\">Onda 1</span><span class=\"gs\"><span class=\"tag no\">reprovada</span> A trava nova.</span>"),
            "{html}"
        );
        let md = Render::Md.render(&tree);
        assert!(md.contains("**Ondas**: 1 reprovada · 1 a fazer\n\n- 1: reprovada — A trava nova.\n- 2: a fazer — Dois."), "{md}");
        assert!(md.contains("### Onda 1\n\n- **MSTD-WAVE-0001**"), "{md}");
    }

    /// O pedido recolhido sai numa linha como a de um item e abre para ser
    /// lido como um arquivo `.md`, com cada código como link; no `.md` ele vai
    /// cercado, linha por linha.
    #[test]
    fn the_request_reads_like_an_md_file() {
        let details = Node::Details {
            summary: "O pedido da onda 1 · 5 linhas, como o agente as recebe".into(),
            body: vec![Node::Markdown("# demo — onda 1\n\n## Combinado\n\n- MSTD-RULE-0001 (regra) — `ler MSTD-RULE-0001`".into())],
            owner: None,
        };
        let rule = item("MSTD-RULE-0001", true, "t", "");
        let tree = doc(vec![section("waves", "Ondas", vec![group("waves-1", "Onda 1", None, "", false, vec![details, Node::Item(rule)])])]);
        let html = Render::Html.render(&tree);
        assert!(
            html.contains(
                "<details class=\"item prompt\"><summary><code class=\"c\">pedido</code><span class=\"t\">O pedido da onda 1 · 5 linhas, como o agente as recebe</span>\
                 <span class=\"tail\"></span></summary><div class=\"body md\"><p class=\"mh h1\">demo — onda 1</p><p class=\"mh h2\">Combinado</p>\
                 <ul><li><a href=\"#MSTD-RULE-0001\">MSTD-RULE-0001</a> (regra) — <code>ler <a href=\"#MSTD-RULE-0001\">MSTD-RULE-0001</a></code></li></ul></div></details>"
            ),
            "{html}"
        );
        assert!(html.contains("<span class=\"count\">2 itens</span></summary>"), "the request counts as a row: {html}");
        let md = Render::Md.render(&tree);
        assert!(md.contains("**O pedido da onda 1 · 5 linhas, como o agente as recebe**\n\n```\n# demo — onda 1\n"), "{md}");
    }

    /// Um título escrito dentro do texto de um item fica abaixo do título do
    /// grupo que cerca o item.
    #[test]
    fn a_heading_inside_an_item_sits_below_the_section_headings() {
        let note = item("MSTD-NOTE-0001", true, "t", "# Grande\n\n## Menor\n\nTexto.");
        let tree = doc(vec![section("notes", "Anotações", vec![Node::Item(note)])]);
        let html = Render::Html.render(&tree);
        assert!(html.contains("<div class=\"prose\"><h4>Grande</h4><h5>Menor</h5><p>Texto.</p></div>"), "{html}");
    }

    /// A página de uma spec e uma página avulsa em markdown saem da mesma
    /// função, com o mesmo estilo, o mesmo script e as fontes do Google
    /// Fonts, sem fonte gravada; a avulsa também tem as seções no menu.
    #[test]
    fn spec_and_standalone_pages_share_one_engine() {
        let spec = Render::Html.render(&doc(vec![section("agreed", "Combinado", vec![Node::Item(item("MSTD-RULE-0001", true, "t", "Regra."))])]));
        let loose = Render::Html.render(&Document {
            lang: "pt-BR".into(),
            kind: None,
            title: "Avulsa".into(),
            meta: Vec::new(),
            body: crate::report::markdown::page("Abertura.\n\n## Seção\n\nTexto com `código`."),
            footer: None,
        });
        let between = |html: &str, open: &str, close: &str| {
            html.split_once(open)
                .and_then(|(_, tail)| tail.split_once(close))
                .map(|(inside, _)| inside.to_string())
                .expect("a page without the piece")
        };
        assert_eq!(between(&spec, "<style>", "</style>"), between(&loose, "<style>", "</style>"));
        assert_eq!(between(&spec, "<script>", "</script>"), between(&loose, "<script>", "</script>"));
        for html in [&spec, &loose] {
            assert!(html.contains(crate::report::FONTS), "{html}");
            assert!(!html.contains("@font-face") && !html.contains("data:font"), "a font written into the page");
        }
        assert!(
            loose.contains("<p>Abertura.</p><section id=\"section-1\" class=\"block\" data-crumb=\"Seção\"><h2><span>Seção</span><span class=\"count\"></span></h2><p>Texto com <code>código</code>.</p></section>"),
            "{loose}"
        );
        assert!(loose.contains("<li data-sec=\"section-1\"><button type=\"button\" data-go=\"section-1\" class=\"top\"><span>Seção</span><i></i></button><ol></ol></li>"), "{loose}");
    }
}
