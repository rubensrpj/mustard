//! O motor de página do Mustard: o único lugar que escreve uma página HTML.
//!
//! A página de uma spec, a do projeto, uma página avulsa escrita em markdown
//! e os relatórios da face `run` saem daqui, no layout aprovado em 17/09
//! (mostarda e carvão): menu lateral com as seções e os grupos, barra com a
//! busca, grupos recolhidos e cada item numa linha. As fontes Geist e Geist
//! Mono são buscadas do Google Fonts; nenhuma fonte vai gravada dentro da
//! página, e quem abre o arquivo sem internet vê a fonte do sistema.
//!
//! - [`Report`] monta a moldura da página: o `<head>`, o estilo, o menu, a
//!   barra, o cabeçalho e o script.
//! - [`markdown`] é o único conversor de markdown do Mustard.
//! - [`Render`] escreve a árvore de `view::document` como `.md` ou `.html`.
//!
//! As funções daqui são puras: montam um `String` e nunca tocam no disco nem
//! encerram o processo.

use std::fmt::Write as _;
use std::str::FromStr;

use mustard_core::platform::i18n::{translate, Locale};

pub mod markdown;
mod render;

pub use render::Render;

/// Folha de estilo do layout do Mustard, a mesma para toda página. Mora em
/// `layout.css` para ser lida e revisada como CSS, não como literal Rust.
const STYLE: &str = include_str!("layout.css");

/// O script da página, o mesmo para toda página: o menu que acompanha a
/// rolagem, a busca e os botões de abrir e fechar. Mora em `layout.js`, ao
/// lado do estilo.
const SCRIPT: &str = include_str!("layout.js");

/// As fontes do layout, buscadas do Google Fonts. Sem internet, vale a pilha
/// do sistema declarada no estilo.
pub(crate) const FONTS: &str = "<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?\
family=Geist:wght@100..900&family=Geist+Mono:wght@100..900&display=swap\">";

/// A lupa da busca.
const SEARCH_ICON: &str = "<svg width=\"15\" height=\"15\" viewBox=\"0 0 16 16\" fill=\"none\" aria-hidden=\"true\">\
<circle cx=\"7\" cy=\"7\" r=\"4.8\" stroke=\"currentColor\" stroke-width=\"1.6\"/>\
<path d=\"M10.6 10.6 14 14\" stroke=\"currentColor\" stroke-width=\"1.6\" stroke-linecap=\"round\"/></svg>";

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

/// Uma seção no menu lateral: o endereço, o título em texto, quantos itens
/// ela tem e os grupos dela.
pub(crate) struct NavSection {
    pub id: String,
    pub title: String,
    pub count: usize,
    pub groups: Vec<NavGroup>,
}

/// Um grupo no menu lateral, debaixo da seção dele.
pub(crate) struct NavGroup {
    pub id: String,
    pub title: String,
    pub count: usize,
}

/// A moldura de uma página: o chamador acrescenta as seções, e a página
/// pronta traz o próprio estilo, o script e o link das fontes.
pub struct Report {
    title: String,
    subtitle: String,
    lang: String,
    /// O que vem depois de `Mustard · ` no menu, e no alto do cabeçalho,
    /// quando houver.
    kind: Option<String>,
    /// Itens extras da linha `.meta`, cada um já montado como `<li>…</li>`.
    meta: Vec<String>,
    /// As seções do menu lateral, na ordem da página.
    nav: Vec<NavSection>,
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
            nav: Vec::new(),
            body: String::new(),
        }
    }

    /// Troca o idioma do atributo `lang` do `<html>` (padrão `en`) — o resumo
    /// da spec sai em `pt-BR`, os relatórios técnicos seguem em inglês. Os
    /// textos da moldura seguem o mesmo idioma.
    #[must_use]
    pub fn with_lang(mut self, lang: impl Into<String>) -> Self {
        self.lang = lang.into();
        self
    }

    /// Diz que documento é este: `Mustard · {kind}` no menu e `{kind}` no
    /// alto do cabeçalho.
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

    /// Acrescenta HTML já montado pelo chamador, fora de uma seção — as
    /// seções da árvore de um documento, o rodapé.
    pub fn raw(&mut self, html: &str) -> &mut Self {
        self.body.push_str(html);
        self
    }

    /// Acrescenta uma seção ao menu lateral; o HTML dela vem por [`raw`].
    ///
    /// [`raw`]: Self::raw
    pub(crate) fn nav(&mut self, section: NavSection) -> &mut Self {
        self.nav.push(section);
        self
    }

    /// Acrescenta uma seção: um `h2` seguido do HTML interno já montado pelo
    /// chamador, com o endereço dela no menu lateral.
    pub fn section(&mut self, heading: &str, inner_html: &str) -> &mut Self {
        let id = format!("section-{}", self.nav.len() + 1);
        let title = escape(heading);
        let _ = write!(
            self.body,
            "<section id=\"{id}\" class=\"block\" data-crumb=\"{title}\"><h2><span>{title}</span></h2>{inner_html}</section>"
        );
        self.nav(NavSection { id, title: heading.to_string(), count: 0, groups: Vec::new() })
    }

    /// Render the finished standalone HTML document.
    #[must_use]
    pub fn render(&self) -> String {
        let lang = Locale::from_str(&self.lang).unwrap_or(Locale::EnUs);
        let t = |key: &str| escape(translate(key, lang));
        let brand = self.kind.as_deref().map_or_else(String::new, |k| format!(" · {}", escape(k)));
        let eyebrow = self.kind.as_deref().map_or_else(|| "Mustard".to_string(), escape);
        // Um subtítulo vazio não vira um `<li>` vazio: o resumo da spec monta a
        // linha só com os pares de `with_meta`.
        let mut meta = String::new();
        if !self.subtitle.is_empty() {
            let _ = write!(meta, "<li>{}</li>", escape(&self.subtitle));
        }
        for item in &self.meta {
            meta.push_str(item);
        }
        let mut nav = String::new();
        for section in &self.nav {
            let _ = write!(
                nav,
                "<li data-sec=\"{id}\"><button type=\"button\" data-go=\"{id}\" class=\"top\"><span>{title}</span><i>{count}</i></button><ol>",
                id = escape(&section.id),
                title = escape(&section.title),
                count = shown_count(section.count),
            );
            for group in &section.groups {
                let _ = write!(
                    nav,
                    "<li><button type=\"button\" data-go=\"{id}\" data-parent=\"{parent}\"><span>{title}</span><i>{count}</i></button></li>",
                    id = escape(&group.id),
                    parent = escape(&section.id),
                    title = escape(&group.title),
                    count = shown_count(group.count),
                );
            }
            nav.push_str("</ol></li>");
        }
        format!(
            "<!doctype html>\n<html lang=\"{lang_attr}\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">{fonts}\
<title>{title}</title><style>{style}</style></head><body>\n\
<div class=\"shell\" data-of=\"{of}\" data-one=\"{one}\" data-many=\"{many}\">\n\
<aside class=\"side\" id=\"side\" aria-label=\"{sections}\">\n\
<p class=\"brand\"><b>Mustard</b>{brand}</p>\n\
<nav><ol class=\"nav\">{nav}</ol></nav>\n\
<div class=\"tools\"><button type=\"button\" id=\"openAll\">{open_all}</button>\
<button type=\"button\" id=\"closeAll\">{close_all}</button></div>\n\
</aside>\n\
<div id=\"content\">\n\
<div class=\"bar\">\n\
<button type=\"button\" class=\"menu-btn\" id=\"menuBtn\" aria-controls=\"side\" aria-expanded=\"false\">{sections}</button>\n\
<div class=\"crumb\" aria-live=\"polite\"></div>\n\
<label class=\"search\" id=\"searchBox\">{icon}<input id=\"q\" type=\"search\" placeholder=\"{placeholder}\" \
autocomplete=\"off\" spellcheck=\"false\" aria-label=\"{search_label}\"><span class=\"aside\">\
<span class=\"hits\" id=\"hits\"></span><kbd>/</kbd></span></label>\n\
</div>\n\
<div class=\"page\">\n\
<header class=\"top\"><p class=\"eyebrow\">{eyebrow}</p><h1>{title}</h1><ul class=\"meta\">{meta}</ul></header>\n\
<p class=\"empty\" id=\"empty\" hidden>{not_found}</p>\n\
{body}\n\
</div>\n\
</div>\n\
</div>\n\
<script>{script}</script>\n\
</body></html>\n",
            lang_attr = escape(&self.lang),
            fonts = FONTS,
            title = escape(&self.title),
            style = STYLE,
            of = t("page.of"),
            one = t("page.count.one"),
            many = t("page.count.many"),
            sections = t("page.sections"),
            open_all = t("page.open_all"),
            close_all = t("page.close_all"),
            icon = SEARCH_ICON,
            placeholder = t("page.search.placeholder"),
            search_label = t("page.search.label"),
            not_found = t("page.not_found"),
            body = self.body,
            script = SCRIPT,
        )
    }
}

/// A contagem do menu: vazia quando não há item.
fn shown_count(count: usize) -> String {
    if count == 0 { String::new() } else { count.to_string() }
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
    assert_only_the_fonts_are_external_but(html, "https://fonts.googleapis.com/");
    assert_eq!(html.matches("href=\"https://").count(), 1, "only the fonts link leaves the page");
}

/// Como [`assert_only_the_fonts_are_external`], mas aceitando também os links
/// que começam por `links`, como os das páginas publicadas das specs: um link
/// leva quem lê para outra página, e não é nada que a página carregue.
#[cfg(test)]
pub(crate) fn assert_only_the_fonts_are_external_but(html: &str, links: &str) {
    for (at, _) in html.match_indices("://") {
        let start = html[..at].rfind('"').map_or(0, |q| q + 1);
        let address = &html[start..];
        let link = address.starts_with(links) && html[..start].ends_with("<a href=\"");
        assert!(
            address.starts_with("https://fonts.googleapis.com/") || link,
            "an external address other than the fonts: {}",
            &html[start..(at + 40).min(html.len())]
        );
    }
    assert!(!html.contains("src="), "the page loads a script or an image");
    assert_eq!(html.matches("<link ").count(), 1, "only the fonts are loaded");
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
        r.section("Raw", "<p>ok</p>");
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

        // Tokens mostarda e carvão, claro e escuro, e as cores de situação.
        for token in [
            "#FAFAF7", "#2B2B29", "#E1AD01", "#8A6700", "#FBF1CF", "#2E2E2B", "#1C1C1A", "#ECEAE3",
            "#E8B923", "#121211", "#2F7A4B", "#B3261E",
        ] {
            assert!(css.contains(token), "token {token} ausente do layout");
        }
        assert!(css.contains("\"Geist\"") && css.contains("\"Geist Mono\""));

        // Estrutura do layout: a seção com o endereço no menu, a tabela em
        // moldura.
        assert!(html.contains("<section id=\"section-1\" class=\"block\" data-crumb=\"Tabela\"><h2><span>Tabela</span></h2>"));
        assert!(html.contains("<div class=\"table\"><table>"));
        assert!(html.contains("<li data-sec=\"section-1\"><button type=\"button\" data-go=\"section-1\" class=\"top\"><span>Tabela</span><i></i></button><ol></ol></li>"));

        let rules = css_rules(css);
        let rule = |selector: &str| {
            rules
                .iter()
                .find(|(sel, _)| sel == selector)
                .map(|(_, decls)| decls.as_str())
                .unwrap_or_else(|| panic!("regra {selector} ausente"))
        };
        // A tabela larga rola dentro da própria moldura, e o código dentro
        // dela nunca quebra.
        let frame = rule(".table");
        assert!(frame.contains("overflow-x:auto") && frame.contains("max-width:100%"), ".table: {frame}");
        let table_code = rule("td code");
        assert!(table_code.contains("white-space:nowrap") && table_code.contains("overflow-wrap:normal"), "{table_code}");
        for (selector, decls) in &rules {
            let in_table = selector.split(',').any(|s| s.split_whitespace().any(|part| matches!(part, "td" | "th" | "table")));
            assert!(
                !(in_table && (decls.contains("word-break:break-all") || decls.contains("overflow-wrap:break-word"))),
                "a table rule breaks words: {selector}{{{decls}}}"
            );
        }
        // O conteúdo nunca alarga a página: a coluna dele pode encolher, e o
        // bloco de código rola.
        assert!(rule(".shell").contains("minmax(0,1fr)"), ".shell: {}", rule(".shell"));
        assert!(rule("#content").contains("min-width:0"), "#content: {}", rule("#content"));
        assert!(rule("dl.kv dd").contains("min-width:0"), "dl.kv dd: {}", rule("dl.kv dd"));
        assert!(rule("pre").contains("overflow-x:auto"), "pre: {}", rule("pre"));
        // Em tela estreita, o menu vira uma gaveta aberta por um botão.
        let somewhere = |selector: &str, decl: &str| rules.iter().any(|(sel, d)| sel == selector && d.contains(decl));
        assert!(somewhere(".side", "position:fixed") && somewhere(".menu-btn", "display:block"));
        // Só aprovado, reprovado e em andamento têm cor de situação.
        let colored: Vec<&str> = rules
            .iter()
            .filter(|(sel, decls)| {
                let tinted = ["color:var(--ok)", "color:var(--no)", "color:var(--mustard-text)"];
                sel.starts_with(".tag") && tinted.iter().any(|color| decls.contains(color))
            })
            .map(|(sel, _)| sel.as_str())
            .collect();
        assert_eq!(colored, [".tag.ok", ".tag.no", ".tag.run"], "{colored:?}");

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
        assert!(html.len() < 32_000, "a página vazia pesa {} bytes", html.len());
        assert_only_the_fonts_are_external(&html);
    }

    /// O menu e o cabeçalho dizem que documento é, e a linha `.meta` junta os
    /// pares com o valor em destaque; um subtítulo vazio não deixa `<li>`
    /// vazio.
    #[test]
    fn report_header_carries_kind_and_meta_pairs() {
        let html = Report::new("Resumo", "")
            .with_kind("spec para aprovar")
            .with_meta("spec", "demo")
            .with_note("aguardando aprovação")
            .render();
        assert!(html.contains("<p class=\"brand\"><b>Mustard</b> · spec para aprovar</p>"), "{html}");
        assert!(html.contains(
            "<header class=\"top\"><p class=\"eyebrow\">spec para aprovar</p><h1>Resumo</h1>\
             <ul class=\"meta\"><li>spec <b>demo</b></li><li>aguardando aprovação</li></ul></header>"
        ));
        assert!(!html.contains("<li></li>"));
    }

    /// Toda página tem o menu lateral, a barra fixa com a busca, os botões de
    /// abrir e fechar tudo e o script lido de `layout.js`, uma vez só; os
    /// textos da moldura e os do script seguem o idioma da página.
    #[test]
    fn every_page_has_the_side_menu_the_search_bar_and_the_page_script() {
        let pt = Report::new("demo", "").with_lang("pt-BR").render();
        for piece in [
            "<aside class=\"side\" id=\"side\" aria-label=\"Seções\">",
            "<nav><ol class=\"nav\">",
            "<button type=\"button\" id=\"openAll\">Abrir tudo</button><button type=\"button\" id=\"closeAll\">Fechar tudo</button>",
            "<div class=\"bar\">",
            "<button type=\"button\" class=\"menu-btn\" id=\"menuBtn\" aria-controls=\"side\" aria-expanded=\"false\">Seções</button>",
            "<input id=\"q\" type=\"search\" placeholder=\"Buscar texto ou código\"",
            "<kbd>/</kbd>",
            "<p class=\"empty\" id=\"empty\" hidden>Nada encontrado.",
            "data-of=\"{n} de {total}\" data-one=\"{n} item\" data-many=\"{n} itens\"",
        ] {
            assert!(pt.contains(piece), "{piece} is missing:\n{pt}");
        }
        assert_eq!(pt.matches("<script>").count(), 1, "one script");
        assert!(pt.contains(&format!("<script>{SCRIPT}</script>")), "the script comes from layout.js");
        for hook in ["getElementById('q')", "'Escape'", "'Enter'", "e.key==='/'", "openAll", "closeAll", "data-of"] {
            assert!(SCRIPT.contains(hook), "the script lost {hook}");
        }
        for text in ["' de '", "' item'", "' itens'"] {
            assert!(!SCRIPT.contains(text), "the script writes {text} itself");
        }

        let en = Report::new("demo", "").with_lang("en-US").render();
        for piece in ["placeholder=\"Search text or code\"", ">Open all</button>", "data-of=\"{n} of {total}\""] {
            assert!(en.contains(piece), "{piece} is missing in English");
        }
    }

    #[test]
    fn table_builds_rows() {
        let html = table(&["A", "B"], &[vec!["1".into(), "2".into()]]);
        assert!(html.contains("<th>A</th>"));
        assert!(html.contains("<td>1</td>"));
    }
}
