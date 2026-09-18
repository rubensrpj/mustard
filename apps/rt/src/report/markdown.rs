//! O conversor de markdown do motor de página, o único do Mustard.
//!
//! Lê o markdown que o assistente escreve (títulos, parágrafos, listas,
//! tabelas, blocos de código, citações) e devolve a árvore de blocos de
//! `view::document`. Converte o markdown de linha em HTML: código entre
//! crases, negrito entre `**` e link `[texto](endereço)`. Itálico com um
//! asterisco ou sublinhado não é lido: nome de arquivo e de variável usam os
//! dois. HTML escrito dentro do markdown não passa: sai escapado, como texto.
//!
//! Todo código do Mustard (`MSTD-RULE-0005`) cujo item está na página vira
//! link para o item, também dentro de um trecho entre crases; o resto do
//! texto fica como está.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use mustard_core::domain::mustard_id;
use mustard_core::view::document::{Node, Section, Table};

use super::escape;

/// Os blocos de um trecho de markdown, sem seções: todo título vira
/// subtítulo.
#[must_use]
pub fn blocks(md: &str) -> Vec<Node> {
    let lines: Vec<String> = md.lines().map(expand_tabs).collect();
    Parser { lines: &lines, at: 0 }.blocks()
}

/// Os blocos de uma página: cada título de nível 1 ou 2 abre uma seção, e o
/// que vem antes do primeiro fica fora de seção.
#[must_use]
pub fn page(md: &str) -> Vec<Node> {
    let mut out = Vec::new();
    let mut current: Option<Section> = None;
    for node in blocks(md) {
        match node {
            Node::Heading { level, text } if level <= 2 => {
                if let Some(section) = current.take() {
                    out.push(Node::Section(section));
                }
                current = Some(Section { anchor: None, heading: text, body: Vec::new() });
            }
            other => match current.as_mut() {
                Some(section) => section.body.push(other),
                None => out.push(other),
            },
        }
    }
    if let Some(section) = current {
        out.push(Node::Section(section));
    }
    out
}

/// A primeira linha `# Título` do markdown e o resto, sem ela. `None` quando
/// o texto não começa por um título de nível 1 (linhas em branco antes não
/// contam).
#[must_use]
pub fn leading_title(md: &str) -> Option<(String, String)> {
    let start = md.len() - md.trim_start().len();
    let first = md[start..].lines().next()?;
    let (level, text) = heading_of(first)?;
    if level != 1 || text.is_empty() {
        return None;
    }
    let rest = &md[start + first.len()..];
    Some((text, rest.to_string()))
}

// ---------------------------------------------------------------------------
// Blocos
// ---------------------------------------------------------------------------

struct Parser<'a> {
    lines: &'a [String],
    at: usize,
}

impl Parser<'_> {
    fn blocks(&mut self) -> Vec<Node> {
        let mut out = Vec::new();
        while let Some(line) = self.lines.get(self.at) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                self.at += 1;
            } else if trimmed.starts_with("<!--") {
                self.skip_comment();
            } else if let Some(fence) = fence_of(line) {
                out.push(self.code(fence));
            } else if let Some((level, text)) = heading_of(line) {
                out.push(Node::Heading { level, text });
                self.at += 1;
            } else if is_rule(line) {
                out.push(Node::Rule);
                self.at += 1;
            } else if self.table_starts() {
                out.push(self.table());
            } else if quote_of(line).is_some() {
                out.push(self.quote());
            } else if let Some(marker) = list_marker(line) {
                out.push(self.list(marker));
            } else {
                out.push(self.paragraph());
            }
        }
        out
    }

    /// Pula um comentário HTML, de uma linha ou de várias.
    fn skip_comment(&mut self) {
        while let Some(line) = self.lines.get(self.at) {
            self.at += 1;
            if line.contains("-->") {
                break;
            }
        }
    }

    fn code(&mut self, (mark, len): (char, usize)) -> Node {
        self.at += 1;
        let mut body = Vec::new();
        while let Some(line) = self.lines.get(self.at) {
            self.at += 1;
            let trimmed = line.trim();
            let closes = trimmed.len() >= len && trimmed.chars().all(|c| c == mark);
            if closes {
                break;
            }
            body.push(line.as_str());
        }
        Node::Code(body.join("\n"))
    }

    fn table_starts(&self) -> bool {
        let Some(next) = self.lines.get(self.at + 1) else {
            return false;
        };
        self.lines[self.at].contains('|') && is_delimiter_row(next)
    }

    fn table(&mut self) -> Node {
        let headers = cells(&self.lines[self.at]);
        self.at += 2;
        let mut rows = Vec::new();
        while let Some(line) = self.lines.get(self.at) {
            if line.trim().is_empty() || !line.contains('|') {
                break;
            }
            let mut row = cells(line);
            row.resize(headers.len(), String::new());
            rows.push(row);
            self.at += 1;
        }
        Node::Table(Table { headers, rows })
    }

    fn quote(&mut self) -> Node {
        let mut inner = Vec::new();
        while let Some(content) = self.lines.get(self.at).and_then(|l| quote_of(l)) {
            inner.push(content.to_string());
            self.at += 1;
        }
        Node::Quote(blocks(&inner.join("\n")))
    }

    fn list(&mut self, first: Marker) -> Node {
        let mut items = Vec::new();
        while let Some(marker) = self.lines.get(self.at).and_then(|l| list_marker(l)) {
            if marker.ordered != first.ordered || marker.indent != first.indent {
                break;
            }
            let mut body = vec![self.lines[self.at][marker.content..].to_string()];
            self.at += 1;
            while let Some(line) = self.lines.get(self.at) {
                if line.trim().is_empty() {
                    let next = self.lines[self.at..].iter().find(|l| !l.trim().is_empty());
                    if next.is_some_and(|l| indent(l) >= marker.content) {
                        body.push(String::new());
                        self.at += 1;
                        continue;
                    }
                    break;
                }
                let depth = indent(line);
                if depth >= marker.content {
                    body.push(line[marker.content..].to_string());
                } else if let Some(nested) = list_marker(line) {
                    if nested.indent <= marker.indent {
                        break;
                    }
                    body.push(line[nested.indent..].to_string());
                } else if body.last().is_some_and(|l| !l.trim().is_empty()) && !starts_block(line) {
                    body.push(line.trim().to_string());
                } else {
                    break;
                }
                self.at += 1;
            }
            items.push(blocks(&checkbox(&body.join("\n"))));
        }
        Node::List { ordered: first.ordered, items }
    }

    fn paragraph(&mut self) -> Node {
        let mut words = Vec::new();
        while let Some(line) = self.lines.get(self.at) {
            let ends = line.trim().is_empty()
                || (!words.is_empty() && (starts_block(line) || self.table_starts()));
            if ends {
                break;
            }
            words.push(line.trim());
            self.at += 1;
        }
        Node::Paragraph(words.join(" "))
    }
}

/// Um item de lista: onde começa, se é numerado e onde começa o texto dele.
#[derive(Debug, Clone, Copy)]
struct Marker {
    indent: usize,
    ordered: bool,
    content: usize,
}

fn expand_tabs(line: &str) -> String {
    let body = line.trim_start_matches([' ', '\t']);
    let lead = &line[..line.len() - body.len()];
    let width: usize = lead.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
    format!("{}{body}", " ".repeat(width))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn fence_of(line: &str) -> Option<(char, usize)> {
    if indent(line) > 3 {
        return None;
    }
    let trimmed = line.trim_start();
    let mark = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = trimmed.chars().take_while(|c| *c == mark).count();
    (len >= 3).then_some((mark, len))
}

fn heading_of(line: &str) -> Option<(u8, String)> {
    if indent(line) > 3 {
        return None;
    }
    let trimmed = line.trim();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    let rest = &trimmed[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    let text = rest.trim();
    let text = match text.trim_end_matches('#') {
        cut if cut.ends_with(' ') || cut.is_empty() => cut.trim_end(),
        _ => text,
    };
    u8::try_from(level).ok().map(|level| (level, text.to_string()))
}

fn is_rule(line: &str) -> bool {
    let marks: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    indent(line) <= 3
        && marks.len() >= 3
        && ['-', '*', '_'].iter().any(|m| marks.chars().all(|c| c == *m))
}

fn quote_of(line: &str) -> Option<&str> {
    if indent(line) > 3 {
        return None;
    }
    let rest = line.trim_start().strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

fn list_marker(line: &str) -> Option<Marker> {
    let at = indent(line);
    let rest = &line[at..];
    for bullet in ["- ", "* ", "+ "] {
        if rest.starts_with(bullet) {
            return Some(Marker { indent: at, ordered: false, content: at + 2 });
        }
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let after = &rest[digits..];
    if (1..=9).contains(&digits) && (after.starts_with(". ") || after.starts_with(") ")) {
        return Some(Marker { indent: at, ordered: true, content: at + digits + 2 });
    }
    None
}

/// Uma linha que abre outro bloco e, por isso, encerra um parágrafo.
fn starts_block(line: &str) -> bool {
    heading_of(line).is_some()
        || fence_of(line).is_some()
        || quote_of(line).is_some()
        || list_marker(line).is_some()
        || is_rule(line)
}

fn is_delimiter_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('-') && trimmed.chars().all(|c| matches!(c, '|' | ':' | '-' | ' '))
}

/// As células de uma linha de tabela; `\|` é uma barra dentro da célula.
fn cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let trimmed = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let trimmed = if trimmed.ends_with('|') && !trimmed.ends_with("\\|") {
        &trimmed[..trimmed.len() - 1]
    } else {
        trimmed
    };
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut chars = trimmed.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut cell).trim().to_string()),
            _ => cell.push(c),
        }
    }
    out.push(cell.trim().to_string());
    out
}

/// A caixinha de tarefa no começo de um item: `[ ]` some e `[x]` vira `✓`.
fn checkbox(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("[ ] ") {
        rest.to_string()
    } else if let Some(rest) = text.strip_prefix("[x] ").or_else(|| text.strip_prefix("[X] ")) {
        format!("✓ {rest}")
    } else {
        text.to_string()
    }
}

// ---------------------------------------------------------------------------
// Markdown de linha
// ---------------------------------------------------------------------------

enum Atom<'a> {
    Text(&'a str),
    Code(&'a str),
    Link { label: &'a str, url: &'a str },
}

/// O markdown de linha em HTML. Um código do Mustard vira link só quando o
/// item dele está em `anchors`, fora ou dentro de um trecho de código; uma
/// crase ou um `**` sem par sai como está.
#[must_use]
pub fn inline(text: &str, anchors: &BTreeSet<String>) -> String {
    let atoms = atoms(text);
    let delimiters: usize =
        atoms.iter().map(|a| if let Atom::Text(t) = a { t.matches("**").count() } else { 0 }).sum();
    let paired = delimiters - delimiters % 2;
    let mut seen = 0;
    let mut out = String::with_capacity(text.len() + 16);
    for atom in atoms {
        match atom {
            Atom::Code(code) => {
                let _ = write!(out, "<code>{}</code>", linked_codes(code, anchors));
            }
            Atom::Link { label, url } => {
                let _ = write!(out, "<a href=\"{}\">{}</a>", escape(url), inline(label, &BTreeSet::new()));
            }
            Atom::Text(text) => {
                for (k, piece) in text.split("**").enumerate() {
                    if k > 0 {
                        out.push_str(match (seen < paired, seen % 2) {
                            (true, 0) => "<strong>",
                            (true, _) => "</strong>",
                            (false, _) => "**",
                        });
                        seen += 1;
                    }
                    out.push_str(&linked_codes(piece, anchors));
                }
            }
        }
    }
    out
}

/// O texto escapado, com cada código do Mustard que tem endereço na página
/// virando link para ele.
fn linked_codes(text: &str, anchors: &BTreeSet<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for (start, end) in mustard_id::find(text) {
        let code = &text[start..end];
        out.push_str(&escape(&text[from..start]));
        if anchors.contains(code) {
            let _ = write!(out, "<a href=\"#{code}\">{code}</a>");
        } else {
            out.push_str(code);
        }
        from = end;
    }
    out.push_str(&escape(&text[from..]));
    out
}

fn atoms(text: &str) -> Vec<Atom<'_>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'`' => {
                let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
                if let Some(close) = closing_run(bytes, i + run, run) {
                    if plain < i {
                        out.push(Atom::Text(&text[plain..i]));
                    }
                    out.push(Atom::Code(trim_code(&text[i + run..close])));
                    i = close + run;
                    plain = i;
                } else {
                    i += run;
                }
            }
            b'[' => {
                if let Some((label, url, end)) = link_at(text, i) {
                    if plain < i {
                        out.push(Atom::Text(&text[plain..i]));
                    }
                    out.push(Atom::Link { label, url });
                    i = end;
                    plain = i;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    if plain < text.len() {
        out.push(Atom::Text(&text[plain..]));
    }
    out
}

/// Onde começa a próxima sequência de exatamente `run` crases.
fn closing_run(bytes: &[u8], from: usize, run: usize) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == b'`' {
            let len = bytes[j..].iter().take_while(|b| **b == b'`').count();
            if len == run {
                return Some(j);
            }
            j += len;
        } else {
            j += 1;
        }
    }
    None
}

/// Um espaço de cada lado sai quando os dois lados têm: é o que deixa
/// escrever uma crase dentro do código (`` `a` ``).
fn trim_code(code: &str) -> &str {
    match code.strip_prefix(' ').and_then(|c| c.strip_suffix(' ')) {
        Some(inner) if !inner.trim().is_empty() => inner,
        _ => code,
    }
}

/// `[texto](endereço)` começando em `at`: o texto, o endereço e onde o link
/// termina. Só entra endereço `http(s)://`, âncora `#…` ou caminho relativo.
fn link_at(text: &str, at: usize) -> Option<(&str, &str, usize)> {
    let close = at + text[at..].find(']')?;
    let label = &text[at + 1..close];
    let rest = text[close + 1..].strip_prefix('(')?;
    let end = rest.find(')')?;
    let url = &rest[..end];
    let safe = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with('#')
        || (!url.contains(':') && !url.starts_with("//"));
    let ok = !label.trim().is_empty() && !url.is_empty() && !url.contains(char::is_whitespace) && safe;
    ok.then_some((label, url, close + 2 + end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(text: &str) -> String {
        inline(text, &BTreeSet::new())
    }

    #[test]
    fn inline_turns_backticks_into_code_and_escapes_the_rest() {
        assert_eq!(html("use `a<b>` agora"), "use <code>a&lt;b&gt;</code> agora");
        assert_eq!(html("crase `sem par"), "crase `sem par");
        assert_eq!(html("- **RO-4.1** — ler `a**b`"), "- <strong>RO-4.1</strong> — ler <code>a**b</code>");
        assert_eq!(html("um ** sozinho"), "um ** sozinho");
        assert_eq!(html("**negrito com `código` dentro**"), "<strong>negrito com <code>código</code> dentro</strong>");
        assert_eq!(html("`` a`b ``"), "<code>a`b</code>");
        assert_eq!(html("<script>x</script>"), "&lt;script&gt;x&lt;/script&gt;");
    }

    #[test]
    fn links_keep_only_safe_addresses() {
        assert_eq!(html("[abrir](https://x.io/a)"), "<a href=\"https://x.io/a\">abrir</a>");
        assert_eq!(html("[item](#state)"), "<a href=\"#state\">item</a>");
        assert_eq!(html("[x](javascript:alert(1))"), "[x](javascript:alert(1))");
    }

    /// Só o código do Mustard com item na página vira link; letra com número
    /// fora do formato fica como está.
    #[test]
    fn a_mustard_code_links_to_its_item_and_nothing_else_does() {
        let anchors: BTreeSet<String> = ["MSTD-RULE-0005".to_string()].into();
        assert_eq!(
            inline("Veja MSTD-RULE-0005 e MSTD-CRIT-0001.", &anchors),
            "Veja <a href=\"#MSTD-RULE-0005\">MSTD-RULE-0005</a> e MSTD-CRIT-0001."
        );
        let plain = "Guardei no R2 da Cloudflare, no S3 e numa folha A4.";
        assert_eq!(inline(plain, &anchors), plain);
    }

    /// Dentro de um trecho entre crases, o código com item na página também
    /// vira link, e o resto do trecho continua escapado como código.
    #[test]
    fn a_mustard_code_inside_backticks_links_too() {
        let anchors: BTreeSet<String> = ["MSTD-RULE-0005".to_string()].into();
        assert_eq!(
            inline("`MSTD-RULE-0005`", &anchors),
            "<code><a href=\"#MSTD-RULE-0005\">MSTD-RULE-0005</a></code>"
        );
        assert_eq!(
            inline("`teste a<b> (MSTD-RULE-0005, MSTD-CRIT-0001)`", &anchors),
            "<code>teste a&lt;b&gt; (<a href=\"#MSTD-RULE-0005\">MSTD-RULE-0005</a>, MSTD-CRIT-0001)</code>"
        );
        assert_eq!(html("`MSTD-RULE-0005`"), "<code>MSTD-RULE-0005</code>", "without the item on the page");
    }

    #[test]
    fn blocks_read_headings_lists_tables_code_and_quotes() {
        let md = "## Título\n\nUm parágrafo\nem duas linhas.\n\n- [ ] item um\n- [x] item dois\n  continua\n  - aninhado\n\n\
                  1. primeiro\n2. segundo\n\n| a | b |\n|---|---|\n| `x \\| y` | 2 |\n\n```\nlet a = 1;\n```\n\n> nota\n\n<!-- some -->\n---\n";
        let nodes = blocks(md);
        assert_eq!(nodes[0], Node::Heading { level: 2, text: "Título".into() });
        assert_eq!(nodes[1], Node::Paragraph("Um parágrafo em duas linhas.".into()));
        let Node::List { ordered: false, items } = &nodes[2] else { panic!("{nodes:?}") };
        assert_eq!(items[0], [Node::Paragraph("item um".into())]);
        assert_eq!(items[1][0], Node::Paragraph("✓ item dois continua".into()));
        assert!(matches!(&items[1][1], Node::List { ordered: false, .. }));
        assert!(matches!(&nodes[3], Node::List { ordered: true, items } if items.len() == 2));
        let Node::Table(table) = &nodes[4] else { panic!("{nodes:?}") };
        assert_eq!(table.headers, ["a", "b"]);
        assert_eq!(table.rows, [["`x | y`", "2"]]);
        assert_eq!(nodes[5], Node::Code("let a = 1;".into()));
        assert_eq!(nodes[6], Node::Quote(vec![Node::Paragraph("nota".into())]));
        assert_eq!(nodes[7], Node::Rule);
        assert_eq!(nodes.len(), 8);
    }

    #[test]
    fn a_page_groups_blocks_under_its_top_headings() {
        let nodes = page("Antes.\n\n# Um\n\ntexto\n\n### Sub\n\n## Dois\n");
        assert_eq!(nodes[0], Node::Paragraph("Antes.".into()));
        let Node::Section(one) = &nodes[1] else { panic!("{nodes:?}") };
        assert_eq!(one.heading, "Um");
        assert_eq!(one.body.len(), 2);
        assert!(matches!(&nodes[2], Node::Section(s) if s.heading == "Dois" && s.body.is_empty()));
    }

    #[test]
    fn the_leading_title_is_taken_off_the_markdown() {
        assert_eq!(leading_title("\n# Plano\n\nCorpo."), Some(("Plano".into(), "\n\nCorpo.".into())));
        assert_eq!(leading_title("## Seção\n"), None);
        assert_eq!(leading_title("Texto."), None);
    }
}
