//! O motor de página do Mustard: o único lugar que escreve uma página HTML.
//!
//! A página de uma spec, a do projeto, uma página avulsa escrita em markdown e
//! os relatórios da face `run` saem daqui, no layout padrão do Mustard (v4,
//! mostarda e carvão), com as fontes Geist e Geist Mono buscadas do Google
//! Fonts. Nenhuma fonte vai gravada dentro da página; quem abre o arquivo sem
//! internet vê a fonte do sistema.
//!
//! - [`Report`] monta a moldura da página: o `<head>`, o estilo, o cabeçalho.
//! - [`markdown`] é o único conversor de markdown do Mustard.
//! - [`Render`] escreve a árvore de `view::document` como `.md` ou `.html`.
//!
//! As funções daqui são puras: montam um `String` e nunca tocam no disco nem
//! encerram o processo.

use std::fmt::Write as _;

pub mod markdown;
mod render;

pub use render::{markdown_html, Render};

/// Folha de estilo do layout padrão do Mustard (v4, mostarda e carvão), a
/// mesma para toda página. Mora em `layout.css` para ser lida e revisada como
/// CSS, não como literal Rust.
const STYLE: &str = include_str!("layout.css");

/// As fontes do layout, buscadas do Google Fonts. Sem internet, vale a pilha
/// do sistema declarada no estilo.
pub(crate) const FONTS: &str = "<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?\
family=Geist:wght@100..900&family=Geist+Mono:wght@100..900&display=swap\">";

/// Idioma do atributo `lang` quando o chamador não pede outro.
const DEFAULT_LANG: &str = "en";

/// HTML-escape a string for safe interpolation into element text / attributes.
#[must_use]
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// A moldura de uma página: o chamador acrescenta as seções, e a página
/// pronta traz o próprio estilo e o link das fontes.
pub struct Report {
    title: String,
    subtitle: String,
    lang: String,
    /// O que vem depois de `Mustard · ` na faixa do cabeçalho, quando houver.
    kind: Option<String>,
    /// Itens extras da linha `.meta`, cada um já montado como `<li>…</li>`.
    meta: Vec<String>,
    body: String,
}

impl Report {
    /// Start a report page with a title and a subtitle (shown as `.meta`).
    #[must_use]
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            lang: DEFAULT_LANG.to_string(),
            kind: None,
            meta: Vec::new(),
            body: String::new(),
        }
    }

    /// Troca o idioma do atributo `lang` do `<html>` (padrão `en`) — o resumo
    /// da spec sai em `pt-BR`, os relatórios técnicos seguem em inglês.
    #[must_use]
    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = lang.into();
        self
    }

    /// Diz que documento é este na faixa do cabeçalho: `Mustard · {kind}`.
    #[must_use]
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Acrescenta à linha `.meta` um par rótulo + valor; o valor sai em
    /// destaque (`<b>`), como `spec <b>slug</b>` no layout aprovado.
    #[must_use]
    pub fn with_meta(mut self, label: &str, value: &str) -> Self {
        self.meta.push(format!("<li>{} <b>{}</b></li>", escape(label), escape(value)));
        self
    }

    /// Acrescenta à linha `.meta` um texto solto, sem valor em destaque.
    #[must_use]
    pub fn with_note(mut self, text: &str) -> Self {
        self.meta.push(format!("<li>{}</li>", escape(text)));
        self
    }

    /// Acrescenta HTML já montado pelo chamador, fora de uma seção — o destaque
    /// de abertura, subtítulos `h3` dentro de uma seção longa, o rodapé.
    pub fn raw(&mut self, html: &str) -> &mut Self {
        self.body.push_str(html);
        self
    }

    /// Acrescenta uma seção: um `h2` (com o traço mostarda do layout) seguido
    /// do HTML interno já montado pelo chamador.
    pub fn section(&mut self, heading: &str, inner_html: &str) -> &mut Self {
        self.body.push_str("<section><h2>");
        self.body.push_str(&escape(heading));
        self.body.push_str("</h2>");
        self.body.push_str(inner_html);
        self.body.push_str("</section>");
        self
    }

    /// Append a `.card` whose body is a `<pre>` block of escaped text — used
    /// to embed the raw JSON projection alongside the rendered view.
    pub fn pre_section(&mut self, heading: &str, text: &str) -> &mut Self {
        let inner = format!("<pre>{}</pre>", escape(text));
        self.section(heading, &inner)
    }

    /// Render the finished standalone HTML document.
    #[must_use]
    pub fn render(&self) -> String {
        let kind = self
            .kind
            .as_deref()
            .map_or_else(|| "Mustard".to_string(), |k| format!("Mustard · {}", escape(k)));
        // Um subtítulo vazio não vira um `<li>` vazio: o resumo da spec monta a
        // linha só com os pares de `with_meta`.
        let mut meta = String::new();
        if !self.subtitle.is_empty() {
            let _ = write!(meta, "<li>{}</li>", escape(&self.subtitle));
        }
        for item in &self.meta {
            meta.push_str(item);
        }
        format!(
            "<!doctype html>\n<html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">{fonts}\
<title>{title}</title><style>{style}</style></head><body><main>\
<header class=\"doc\"><p class=\"kind\">{kind}</p><h1>{title}</h1>\
<ul class=\"meta\">{meta}</ul></header>{body}</main></body></html>\n",
            lang = escape(&self.lang),
            fonts = FONTS,
            title = escape(&self.title),
            style = STYLE,
            body = self.body,
        )
    }
}

/// Build a `<table>` from a header row and string cells. Each row is rendered
/// verbatim as escaped text — callers that need status colouring should use
/// [`table_with_classes`] instead.
#[must_use]
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    // A moldura `.table` dá a borda arredondada e a rolagem horizontal do layout.
    let mut html = String::from("<div class=\"table\"><table><thead><tr>");
    for h in headers {
        let _ = write!(html, "<th>{}</th>", escape(h));
    }
    html.push_str("</tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr>");
        for cell in row {
            let _ = write!(html, "<td>{}</td>", escape(cell));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></div>");
    html
}

/// Os endereços de fora que uma página do motor carrega: só o das fontes.
#[cfg(test)]
pub(crate) fn assert_only_the_fonts_are_external(html: &str) {
    for (at, _) in html.match_indices("://") {
        let start = html[..at].rfind('"').map_or(0, |q| q + 1);
        assert!(
            html[start..].starts_with("https://fonts.googleapis.com/"),
            "an external address other than the fonts: {}",
            &html[start..(at + 40).min(html.len())]
        );
    }
    assert!(!html.contains("src="), "the page loads a script or an image");
    assert_eq!(html.matches("href=\"https://").count(), 1, "only the fonts link leaves the page");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_neutralizes_markup() {
        assert_eq!(escape("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }

    #[test]
    fn report_renders_standalone_document() {
        let mut r = Report::new("QA", "spec: demo");
        r.pre_section("Raw", "{\"ok\":true}");
        let html = r.render();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<style>"));
        assert_only_the_fonts_are_external(&html);
        assert!(html.contains("spec: demo"));
        assert!(html.ends_with("</html>\n"));
    }

    /// Pares (seletor, declarações) de cada regra folha do CSS, inclusive as
    /// aninhadas em `@media`; comentários são descartados antes.
    fn css_rules(css: &str) -> Vec<(String, String)> {
        let mut clean = String::new();
        let mut rest = css;
        while let Some(start) = rest.find("/*") {
            clean.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            rest = after.find("*/").map_or("", |end| &after[end + 2..]);
        }
        clean.push_str(rest);

        let mut rules = Vec::new();
        let mut prelude_start = 0;
        let mut open: Option<(String, usize)> = None;
        for (i, c) in clean.char_indices() {
            match c {
                '{' => {
                    open = Some((clean[prelude_start..i].trim().to_string(), i + 1));
                    prelude_start = i + 1;
                }
                '}' => {
                    if let Some((selector, body_start)) = open.take() {
                        rules.push((selector, clean[body_start..i].replace([' ', '\n'], "")));
                    }
                    prelude_start = i + 1;
                }
                _ => {}
            }
        }
        rules
    }

    /// Verdadeiro quando algum composto do seletor mira um `li`
    /// (`li`, `li.done`, `li::before`...).
    fn targets_li(selector: &str) -> bool {
        selector
            .split(|c: char| c.is_whitespace() || matches!(c, ',' | '>' | '+' | '~'))
            .any(|compound| {
                compound
                    .strip_prefix("li")
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with(['.', ':', '[', '#']))
            })
    }

    #[test]
    fn report_renders_the_standard_mustard_layout() {
        let mut r = Report::new("Resumo da spec", "spec: demo").with_lang("pt-BR");
        r.section("Tabela", &table(&["A"], &[vec!["`x`".into()]]));
        let html = r.render();

        // Idioma pedido no <html>; sem pedido, vale o padrão `en`.
        assert!(html.contains("<html lang=\"pt-BR\">"), "lang pedido ausente");
        assert!(Report::new("QA", "x").render().contains("<html lang=\"en\">"));

        let css = html
            .split_once("<style>")
            .and_then(|(_, tail)| tail.split_once("</style>"))
            .map(|(style, _)| style)
            .expect("página sem <style>");

        // Tokens mostarda e carvão, claro e escuro.
        for token in [
            "#FAFAF7", "#2B2B29", "#E1AD01", "#8A6700", "#FBF1CF", "#2E2E2B", "#1C1C1A", "#ECEAE3",
            "#E8B923", "#121211",
        ] {
            assert!(css.contains(token), "token {token} ausente do layout");
        }
        assert!(css.contains("\"Geist\"") && css.contains("\"Geist Mono\""));
        assert!(css.contains("max-width:860px"));

        // Estrutura do layout: faixa carvão no topo, tabela em moldura.
        assert!(html.contains("<header class=\"doc\">"));
        assert!(html.contains("<div class=\"table\"><table>"));

        let rules = css_rules(css);
        // Código inline nunca quebra no meio da palavra.
        let code = rules.iter().find(|(sel, _)| sel == "code").expect("regra code ausente");
        assert!(code.1.contains("white-space:nowrap"), "code inline sem nowrap: {}", code.1);
        assert!(!css.contains("overflow-wrap:anywhere"), "overflow-wrap:anywhere proibido");
        // A coluna Onde quebra a linha e fica em 40%, sem espremer a primeira.
        let place = rules.iter().find(|(sel, _)| sel == "td.where").expect("regra td.where ausente");
        assert!(
            place.1.contains("white-space:normal") && place.1.contains("width:40%"),
            "td.where: {}",
            place.1
        );

        // Nenhum li (nem pseudo-elemento dele) vira grid.
        for (selector, decls) in &rules {
            assert!(
                !(targets_li(selector) && decls.contains("display:grid")),
                "li em grid: {selector}{{{decls}}}"
            );
        }
    }

    /// Geist e Geist Mono vêm do Google Fonts, por um link no `<head>`, e o
    /// estilo guarda a fonte do sistema como reserva; nenhuma fonte vai
    /// gravada dentro da página, que fica pequena.
    #[test]
    fn report_links_the_geist_fonts_from_google_fonts() {
        let html = Report::new("QA", "x").render();
        let head = html.split_once("</head>").map(|(head, _)| head).expect("página sem <head>");
        assert!(head.contains(FONTS), "o link das fontes fica no <head>");
        assert!(FONTS.contains("family=Geist:wght@100..900") && FONTS.contains("family=Geist+Mono:wght@100..900"));
        assert!(!html.contains("@font-face"), "nenhuma fonte declarada dentro da página");
        assert!(!html.contains("data:font") && !html.contains("base64"), "nenhuma fonte gravada");
        assert!(STYLE.contains("--sans:\"Geist\",") && STYLE.contains("system-ui"));
        assert!(STYLE.contains("--mono:\"Geist Mono\",") && STYLE.contains("ui-monospace"));
        assert!(html.len() < 12_000, "a página vazia pesa {} bytes", html.len());
        assert_only_the_fonts_are_external(&html);
    }

    /// A faixa do cabeçalho diz que documento é, e a linha `.meta` junta os
    /// pares com o valor em destaque; um subtítulo vazio não deixa `<li>` vazio.
    #[test]
    fn report_header_carries_kind_and_meta_pairs() {
        let html = Report::new("Resumo", "")
            .with_kind("spec para aprovar")
            .with_meta("spec", "demo")
            .with_note("aguardando aprovação")
            .render();
        assert!(html.contains("<p class=\"kind\">Mustard · spec para aprovar</p>"), "{html}");
        assert!(html.contains(
            "<ul class=\"meta\"><li>spec <b>demo</b></li><li>aguardando aprovação</li></ul>"
        ));
        assert!(!html.contains("<li></li>"));
    }

    #[test]
    fn table_builds_rows() {
        let html = table(&["A", "B"], &[vec!["1".into(), "2".into()]]);
        assert!(html.contains("<th>A</th>"));
        assert!(html.contains("<td>1</td>"));
    }
}
