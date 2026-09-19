//! A árvore de uma página avulsa escrita como `.html`.
//!
//! [`Render::Html`] é a única saída de página do Mustard: uma página avulsa
//! escrita em markdown passa por ela, no mesmo layout, o mesmo script e as
//! mesmas fontes de sempre.
//!
//! Cada seção entra no menu lateral. A saída é função só da árvore: a mesma
//! árvore dá sempre os mesmos bytes.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use mustard_core::platform::i18n::{translate, Locale};
use mustard_core::view::document::{Document, Meta, Node, Section, Table};

use super::markdown::inline;
use super::{escape, NavSection, Report};

/// Em que forma a página sai.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Render {
    /// A página no layout do Mustard.
    Html,
}

impl Render {
    /// A página `doc` nesta forma.
    #[must_use]
    pub fn render(self, doc: &Document) -> String {
        match self {
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
            html.node(node, &mut body);
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

    /// A seção no menu lateral: o título e quantas linhas ela tem.
    fn nav(&self, id: &str, section: &Section) -> NavSection {
        NavSection { id: id.to_string(), title: plain(&section.heading), count: rows(&section.body), groups: Vec::new() }
    }

    /// Uma seção: o título com a contagem das linhas, e os blocos.
    fn section(&self, id: &str, section: &Section, out: &mut String) {
        let _ = write!(
            out,
            "<section id=\"{}\" class=\"block\" data-crumb=\"{}\"><h2><span>{}</span>{}</h2>",
            escape(id),
            escape(&plain(&section.heading)),
            self.inline(&section.heading),
            self.count(rows(&section.body)),
        );
        self.nodes(&section.body, out);
        out.push_str("</section>");
    }

    /// A contagem das linhas de uma seção; sem linha, nada.
    fn count(&self, n: usize) -> String {
        if n == 0 {
            return "<span class=\"count\"></span>".to_string();
        }
        let key = if n == 1 { "page.count.one" } else { "page.count.many" };
        format!("<span class=\"count\">{}</span>", escape(&translate(key, self.lang).replace("{n}", &n.to_string())))
    }

    /// Os blocos em ordem.
    fn nodes(&self, nodes: &[Node], out: &mut String) {
        for node in nodes {
            self.node(node, out);
        }
    }

    fn node(&self, node: &Node, out: &mut String) {
        match node {
            Node::Section(section) => {
                let id = section.anchor.clone().unwrap_or_default();
                self.section(&id, section, out);
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
                let _ = write!(out, "<{tag}>");
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
}

/// Quantas linhas recolhidas os blocos têm. É o que a busca da página conta;
/// uma página avulsa em markdown não tem nenhuma.
fn rows(_nodes: &[Node]) -> usize {
    0
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

#[cfg(test)]
mod tests {
    use super::*;

    fn section(anchor: Option<&str>, heading: &str, body: Vec<Node>) -> Node {
        Node::Section(Section { anchor: anchor.map(str::to_string), heading: heading.into(), body })
    }

    fn doc(body: Vec<Node>) -> Document {
        Document { lang: "pt-BR".into(), kind: None, title: "demo".into(), meta: Vec::new(), footer: None, body }
    }

    /// A mesma árvore dá sempre os mesmos bytes.
    #[test]
    fn the_same_tree_gives_the_same_bytes_twice() {
        let tree = doc(vec![section(Some("intro"), "Abertura", vec![Node::Paragraph("Texto.".into())])]);
        assert_eq!(Render::Html.render(&tree), Render::Html.render(&tree));
    }

    /// Nada do relógio nem da máquina: a página só tem o que a árvore leva.
    #[test]
    fn the_page_carries_no_clock_and_no_machine_path() {
        let tree = doc(vec![section(Some("intro"), "Abertura", vec![Node::Paragraph("Texto.".into())])]);
        let html = Render::Html.render(&tree);
        assert!(!html.contains("/home/") && !html.contains("C:\\"), "{html}");
        assert!(!html.contains(&mustard_core::time::now_iso8601()[..10]));
    }

    /// Sem endereço próprio, a seção ganha um numerado pela ordem em que
    /// aparece, e entra no menu lateral do mesmo jeito.
    #[test]
    fn a_section_without_its_own_anchor_gets_a_numbered_id() {
        let tree = doc(vec![
            section(None, "Um", vec![]),
            section(None, "Dois", vec![]),
        ]);
        let html = Render::Html.render(&tree);
        assert!(html.contains("<section id=\"section-1\" class=\"block\" data-crumb=\"Um\">"), "{html}");
        assert!(html.contains("<section id=\"section-2\" class=\"block\" data-crumb=\"Dois\">"), "{html}");
        assert!(html.contains("<li data-sec=\"section-1\"><button type=\"button\" data-go=\"section-1\""), "{html}");
    }

    /// Um destaque (`> `) sai como um `callout` com os blocos de dentro.
    #[test]
    fn a_quote_becomes_a_callout() {
        let tree = doc(vec![section(Some("nota"), "Nota", vec![Node::Quote(vec![Node::Paragraph("Atenção.".into())])])]);
        let html = Render::Html.render(&tree);
        assert!(html.contains("<div class=\"callout\"><p>Atenção.</p></div>"), "{html}");
    }

    /// A página de uma spec e uma página avulsa em markdown saem da mesma
    /// função, com o mesmo estilo, o mesmo script e as fontes do Google
    /// Fonts, sem fonte gravada; a avulsa também tem as seções no menu.
    #[test]
    fn spec_and_standalone_pages_share_one_engine() {
        let spec = Render::Html.render(&doc(vec![section(Some("agreed"), "Combinado", vec![Node::Paragraph("Regra.".into())])]));
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
