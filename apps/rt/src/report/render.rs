//! A árvore de uma página escrita como `.md` ou como `.html`.
//!
//! [`Render::Html`] é a única saída de página do Mustard: a página de uma
//! spec e uma página avulsa escrita em markdown passam por ela,
//! com o mesmo layout e as mesmas fontes. [`Render::Md`] escreve a mesma
//! árvore como texto.
//!
//! As duas saídas são função só da árvore: a mesma árvore dá sempre os mesmos
//! bytes.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use mustard_core::view::document::{Document, Item, Meta, Node, Table};

use super::markdown::{blocks, inline};
use super::{escape, Report};

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

/// Um trecho de markdown como HTML, sem a página em volta: o miolo de uma
/// página que o chamador ainda monta seção por seção.
#[must_use]
pub fn markdown_html(md: &str) -> String {
    let anchors = BTreeSet::new();
    let mut out = String::new();
    Html { anchors: &anchors }.nodes(&blocks(md), &mut out);
    out
}

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
    let html = Html { anchors: &anchors };
    let mut body = String::new();
    html.nodes(&doc.body, &mut body);
    if let Some(footer) = &doc.footer {
        let _ = write!(body, "<footer>{}</footer>", html.inline(footer));
    }
    report.raw(&body);
    report.render()
}

struct Html<'a> {
    anchors: &'a BTreeSet<String>,
}

impl Html<'_> {
    fn inline(&self, text: &str) -> String {
        inline(text, self.anchors)
    }

    /// Os blocos em ordem; itens seguidos ficam numa lista de definições só.
    fn nodes(&self, nodes: &[Node], out: &mut String) {
        let mut i = 0;
        while i < nodes.len() {
            if let Node::Item(_) = &nodes[i] {
                out.push_str("<dl class=\"wrap-code\">");
                while let Some(Node::Item(item)) = nodes.get(i) {
                    self.item(item, out);
                    i += 1;
                }
                out.push_str("</dl>");
            } else {
                self.node(&nodes[i], out);
                i += 1;
            }
        }
    }

    fn node(&self, node: &Node, out: &mut String) {
        match node {
            Node::Section(section) => {
                match &section.anchor {
                    Some(anchor) => {
                        let _ = write!(out, "<section id=\"{}\">", escape(anchor));
                    }
                    None => out.push_str("<section>"),
                }
                let _ = write!(out, "<h2>{}</h2>", self.inline(&section.heading));
                if let Some(summary) = &section.collapsed {
                    let _ = write!(out, "<details><summary>{}</summary>", self.inline(summary));
                    self.nodes(&section.body, out);
                    out.push_str("</details>");
                } else {
                    self.nodes(&section.body, out);
                }
                out.push_str("</section>");
            }
            Node::Heading { level, text } => {
                let level = (*level).clamp(3, 6);
                let _ = write!(out, "<h{level}>{}</h{level}>", self.inline(text));
            }
            Node::Paragraph(text) => {
                let _ = write!(out, "<p>{}</p>", self.inline(text));
            }
            Node::List { ordered, items } => {
                let tag = if *ordered { "ol" } else { "ul" };
                let _ = write!(out, "<{tag} class=\"wrap-code\">");
                for item in items {
                    out.push_str("<li>");
                    self.list_item(item, out);
                    out.push_str("</li>");
                }
                let _ = write!(out, "</{tag}>");
            }
            Node::Table(table) => self.table(table, out),
            Node::Code(text) => {
                let _ = write!(out, "<pre>{}</pre>", escape(text));
            }
            Node::Quote(inner) => {
                out.push_str("<div class=\"callout\">");
                self.nodes(inner, out);
                out.push_str("</div>");
            }
            Node::Rule => out.push_str("<hr>"),
            Node::Item(_) => self.nodes(std::slice::from_ref(node), out),
        }
    }

    /// Um item de lista curto sai sem parágrafo: o primeiro parágrafo vai
    /// solto no item, e uma lista aninhada vem logo depois dele.
    fn list_item(&self, body: &[Node], out: &mut String) {
        match body.split_first() {
            Some((Node::Paragraph(first), rest))
                if !rest.iter().any(|n| matches!(n, Node::Paragraph(_))) =>
            {
                out.push_str(&self.inline(first));
                self.nodes(rest, out);
            }
            _ => self.nodes(body, out),
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

    /// O código do item é o endereço dele; o texto e cada campo vêm abaixo.
    fn item(&self, item: &Item, out: &mut String) {
        if item.anchored {
            let _ = write!(out, "<div id=\"{}\">", escape(&item.code));
        } else {
            out.push_str("<div>");
        }
        let _ = write!(out, "<dt><code>{}</code>", escape(&item.code));
        if let Some(note) = &item.note {
            let _ = write!(out, " <span class=\"muted\">{}</span>", self.inline(note));
        }
        out.push_str("</dt>");
        if !item.text.is_empty() {
            out.push_str("<dd>");
            self.nodes(&blocks(&item.text), out);
            out.push_str("</dd>");
        }
        for field in &item.fields {
            let _ = write!(
                out,
                "<dd><span class=\"label\">{}:</span> {}</dd>",
                escape(&field.label),
                self.inline(&field.value)
            );
        }
        out.push_str("</div>");
    }
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
        Node::Section(section) => {
            let body = md_blocks(&section.body);
            if body.is_empty() {
                format!("## {}", section.heading)
            } else {
                format!("## {}\n\n{body}", section.heading)
            }
        }
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
        Node::Code(text) => {
            let longest = text
                .lines()
                .map(|l| l.trim_start().chars().take_while(|c| *c == '`').count())
                .max()
                .unwrap_or(0);
            let fence = "`".repeat(longest.max(2) + 1);
            format!("{fence}\n{text}\n{fence}")
        }
        Node::Quote(inner) => md_blocks(inner)
            .lines()
            .map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") })
            .collect::<Vec<_>>()
            .join("\n"),
        Node::Rule => "---".to_string(),
        Node::Item(item) => md_item(item),
    }
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
    if let Some(note) = &item.note {
        let _ = write!(head, " · {note}");
    }
    let mut body = if item.text.is_empty() {
        head
    } else if item.text.contains('\n') {
        format!("{head}\n\n{}", item.text)
    } else {
        format!("{head} — {}", item.text)
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
    use mustard_core::domain::spec_events::parse_log;
    use mustard_core::platform::i18n::Locale;
    use mustard_core::view::document::{spec_document, WavePrompts};

    const LOG: &str = concat!(
        "{\"v\":1,\"id\":1,\"at\":\"2026-09-11T08:40:00-03:00\",\"type\":\"state\",\"author\":\"binary\",\"phase\":\"survey\",\"branch\":\"feature/demo\",\"base\":\"dev\"}\n",
        "{\"v\":1,\"id\":2,\"at\":\"2026-09-11T08:40:30-03:00\",\"type\":\"message\",\"author\":\"user\",\"text\":\"Revise tudo\"}\n",
        "{\"v\":1,\"id\":3,\"at\":\"2026-09-11T09:00:00-03:00\",\"type\":\"rule\",\"author\":\"assistant\",\"text\":\"A trava lê o comando.\",\"keys\":[\"trava\"],\"example\":\"`rm -rf pasta` é barrado.\",\"origin\":2}\n",
        "{\"v\":1,\"id\":4,\"at\":\"2026-09-11T09:01:00-03:00\",\"type\":\"decision\",\"author\":\"assistant\",\"text\":\"Versão antiga da decisão.\",\"keys\":[\"d\"],\"why\":\"w\",\"origin\":2}\n",
        "{\"v\":1,\"id\":5,\"at\":\"2026-09-11T09:02:00-03:00\",\"type\":\"decision\",\"author\":\"assistant\",\"text\":\"Versão nova, que segue MSTD-RULE-0001.\",\"keys\":[\"d\"],\"why\":\"w\",\"origin\":2,\"replaces\":4}\n",
        "{\"v\":1,\"id\":6,\"at\":\"2026-09-11T09:03:00-03:00\",\"type\":\"note\",\"author\":\"assistant\",\"text\":\"Anotação que sai.\",\"keys\":[\"n\"],\"origin\":2}\n",
        "{\"v\":1,\"id\":7,\"at\":\"2026-09-11T09:04:00-03:00\",\"type\":\"remove\",\"author\":\"assistant\",\"targets\":[6],\"reason\":\"engano\"}\n",
    );

    fn spec_page(render: Render) -> String {
        render.render(&spec_document("demo", &parse_log(LOG), &WavePrompts::new(), Locale::PtBr))
    }

    /// A mesma árvore dá sempre os mesmos bytes, nas duas formas.
    #[test]
    fn the_same_events_give_the_same_bytes_twice() {
        assert_eq!(spec_page(Render::Md), spec_page(Render::Md));
        assert_eq!(spec_page(Render::Html), spec_page(Render::Html));
    }

    /// Uma decisão revista: a página e o `.md` mostram a versão nova no
    /// combinado; a antiga aparece só na conversa, marcada como substituída.
    #[test]
    fn a_revised_decision_shows_only_the_new_version_outside_the_conversation() {
        for (page, conversation) in [
            (spec_page(Render::Html), "<section id=\"conversation\">"),
            (spec_page(Render::Md), "## Conversa"),
        ] {
            let (before, talk) = page.split_once(conversation).unwrap_or_else(|| panic!("{page}"));
            assert!(before.contains("Versão nova"), "{before}");
            assert!(!before.contains("Versão antiga"), "{before}");
            assert!(talk.contains("Versão antiga da decisão."), "{talk}");
            assert!(talk.contains("versão substituída"), "{talk}");
        }
        let html = spec_page(Render::Html);
        assert_eq!(html.matches("id=\"MSTD-DEC-0001\"").count(), 1, "one address per code");
    }

    /// Um item removido sai da página e do `.md`.
    #[test]
    fn a_removed_item_leaves_the_page_and_the_md() {
        for page in [spec_page(Render::Html), spec_page(Render::Md)] {
            assert!(!page.contains("Anotação que sai."), "{page}");
            assert!(page.contains("MSTD-NOTE-0001"), "the removal names what it took out");
        }
    }

    /// No HTML, todo código com item na página vira link para ele; o item
    /// leva o código como endereço. O `.md` mostra o código como texto.
    #[test]
    fn codes_link_to_their_items_on_the_page() {
        let html = spec_page(Render::Html);
        assert!(html.contains("<div id=\"MSTD-RULE-0001\"><dt><code>MSTD-RULE-0001</code>"), "{html}");
        assert!(html.contains("segue <a href=\"#MSTD-RULE-0001\">MSTD-RULE-0001</a>."), "{html}");
        let md = spec_page(Render::Md);
        assert!(md.contains("- **MSTD-RULE-0001** — A trava lê o comando."), "{md}");
        assert!(md.contains("  - Exemplo: `rm -rf pasta` é barrado."), "{md}");
        assert!(md.contains("  - Origem: MSTD-MSG-0001"), "{md}");
    }

    /// Um código citado na prova de um critério vira link para o item, também
    /// quando a prova inteira sai como código.
    #[test]
    fn a_code_cited_in_a_criterion_proof_links_to_its_item() {
        let log = [
            LOG,
            "{\"v\":1,\"id\":8,\"at\":\"2026-09-11T09:05:00-03:00\",\"type\":\"criterion\",\"author\":\"assistant\",\"when\":\"w\",\"then\":\"t\",\"proof\":\"teste da trava (MSTD-RULE-0001)\",\"origin\":2}\n",
        ]
        .concat();
        let html = Render::Html.render(&spec_document("demo", &parse_log(&log), &WavePrompts::new(), Locale::PtBr));
        assert!(
            html.contains("<code>teste da trava (<a href=\"#MSTD-RULE-0001\">MSTD-RULE-0001</a>)</code>"),
            "{html}"
        );
    }

    /// Nada do relógio nem da máquina: a página só tem o que os eventos
    /// gravaram.
    #[test]
    fn the_page_carries_no_clock_and_no_machine_path() {
        for page in [spec_page(Render::Html), spec_page(Render::Md)] {
            assert!(!page.contains("/home/") && !page.contains("C:\\"), "{page}");
            assert!(!page.contains(&mustard_core::time::now_iso8601()[..10]) || page.contains("2026-09-11"));
        }
    }

    /// A página de uma spec e uma página avulsa em markdown saem da mesma
    /// função, com o mesmo estilo e as fontes do Google Fonts, sem fonte
    /// gravada.
    #[test]
    fn spec_and_standalone_pages_share_one_engine() {
        let spec = spec_page(Render::Html);
        let loose = Render::Html.render(&Document {
            lang: "pt-BR".into(),
            kind: None,
            title: "Avulsa".into(),
            meta: Vec::new(),
            body: crate::report::markdown::page("## Seção\n\nTexto com `código`."),
            footer: None,
        });
        let style = |html: &str| {
            html.split_once("<style>")
                .and_then(|(_, tail)| tail.split_once("</style>"))
                .map(|(css, _)| css.to_string())
                .expect("a page without style")
        };
        assert_eq!(style(&spec), style(&loose));
        for html in [&spec, &loose] {
            assert!(html.contains(crate::report::FONTS), "{html}");
            assert!(!html.contains("@font-face") && !html.contains("data:font"), "a font written into the page");
        }
        assert!(loose.contains("<section><h2>Seção</h2><p>Texto com <code>código</code>.</p></section>"), "{loose}");
    }
}
