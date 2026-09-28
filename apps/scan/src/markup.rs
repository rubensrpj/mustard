//! A língua escrita dentro de um arquivo de marcação: a página que mistura o
//! texto da tela com trechos de código. O registro (`markup` em
//! languages.toml) diz onde o código está; aqui ele sai como o texto que a
//! gramática da língua lê. O resto do arquivo vira espaço e cada linha fica no
//! lugar dela, para que as linhas do mapa sejam as do arquivo.
//!
//! O código dos blocos e das linhas de membro mora num tipo com o nome do
//! arquivo, aberto no começo da linha do primeiro código e fechado logo
//! depois do último. O código das linhas de cabeça fica fora desse tipo,
//! antes dele. O arquivo sem código nenhum sai só com espaços.

/// Onde mora o código num arquivo de marcação, como o registro o declara.
pub(crate) struct Markup {
    /// Os marcadores seguidos de um bloco entre chaves: o de dentro das
    /// chaves é código.
    pub blocks: &'static [&'static str],
    /// O marcador que abre uma linha de código dentro do tipo do arquivo e a
    /// forma dela, com `{}` no lugar do resto da linha.
    pub lines: &'static [(&'static str, &'static str)],
    /// Como `lines`, mas o código vai antes do tipo do arquivo.
    pub head: &'static [(&'static str, &'static str)],
    /// O texto que abre o tipo do arquivo, com `{file}` no lugar do nome
    /// dele sem a extensão.
    pub open: &'static str,
    /// O texto que fecha o tipo do arquivo.
    pub close: &'static str,
}

/// Um trecho de código achado no arquivo.
enum Piece {
    /// Um bloco: o byte da chave que abre e o da que fecha; sem a que fecha,
    /// o fim do arquivo.
    Block { open: usize, close: usize },
    /// Uma linha: do começo do marcador ao fim da linha, com o código que ela
    /// vira e se ele vai antes do tipo do arquivo.
    Line { start: usize, end: usize, code: String, head: bool },
}

impl Piece {
    fn start(&self) -> usize {
        match self {
            Piece::Block { open, .. } => *open,
            Piece::Line { start, .. } => *start,
        }
    }

    fn in_type(&self) -> bool {
        !matches!(self, Piece::Line { head: true, .. })
    }
}

impl Markup {
    /// O texto que a gramática lê do arquivo `src`, no caminho `path`: só o
    /// código, com as linhas no lugar delas. O código de cabeça escrito antes
    /// do primeiro código do tipo fica na linha dele; o escrito depois vai
    /// para a linha em que o tipo abre, antes dele.
    pub(crate) fn code_of(&self, src: &str, path: &str) -> String {
        let pieces = self.pieces(src);
        let first_in_type = pieces.iter().position(Piece::in_type);
        let last_in_type = pieces.iter().rposition(Piece::in_type);
        let mut out = String::with_capacity(src.len() + 64);
        let mut at = 0;
        for (i, piece) in pieces.iter().enumerate() {
            if first_in_type == Some(i) {
                let line_start = src[..piece.start()].rfind('\n').map_or(0, |n| n + 1).max(at);
                blank(&mut out, &src[at..line_start]);
                for later in &pieces[i..] {
                    if let Piece::Line { code, head: true, .. } = later {
                        out.push_str(code);
                        out.push(' ');
                    }
                }
                out.push_str(&self.open.replace("{file}", file_stem(path)));
                at = line_start;
            }
            let moved = first_in_type.is_some_and(|first| i > first);
            let closes = last_in_type == Some(i);
            match piece {
                Piece::Block { open, close } => {
                    blank(&mut out, &src[at..=*open]);
                    out.push_str(&src[open + 1..*close]);
                    at = *close;
                }
                Piece::Line { end, code, head, .. } => {
                    blank(&mut out, &src[at..*end]);
                    if !(*head && moved) {
                        out.push_str(code);
                    }
                    at = *end;
                }
            }
            if closes {
                out.push_str(self.close);
            }
        }
        blank(&mut out, &src[at..]);
        out
    }

    /// Os trechos de código de `src`, na ordem do arquivo. A linha de código
    /// começa pelo marcador, depois dos espaços do começo da linha; o bloco
    /// começa pelo marcador escrito fora de outra palavra e segue na chave
    /// que vem depois dele, passados os espaços.
    fn pieces(&self, src: &str) -> Vec<Piece> {
        let bytes = src.as_bytes();
        let mut out = Vec::new();
        let mut at = 0;
        let mut line_start = true;
        while at < bytes.len() {
            if line_start {
                let first = at + bytes[at..].iter().take_while(|b| matches!(b, b' ' | b'\t')).count();
                if let Some((line, end)) = self.line_at(src, first) {
                    out.push(line);
                    at = end;
                    line_start = false;
                    continue;
                }
            }
            if let Some(open) = self.block_at(src, at) {
                let close = matching_brace(bytes, open);
                out.push(Piece::Block { open, close });
                at = (close + 1).min(bytes.len());
                line_start = false;
                continue;
            }
            line_start = bytes[at] == b'\n';
            at += 1;
        }
        out
    }

    /// A linha de código que começa no byte `first` de `src`, com o byte em
    /// que ela termina: o marcador, um espaço e o resto da linha, que não
    /// pode ser vazio.
    fn line_at(&self, src: &str, first: usize) -> Option<(Piece, usize)> {
        let rest = &src[first..];
        let (marker, form, head) = self
            .lines
            .iter()
            .map(|(marker, form)| (marker, form, false))
            .chain(self.head.iter().map(|(marker, form)| (marker, form, true)))
            .find(|(marker, ..)| rest.starts_with(**marker) && rest[marker.len()..].starts_with([' ', '\t']))?;
        let end = first + rest.find('\n').unwrap_or(rest.len());
        let argument = src[first + marker.len()..end].trim();
        let code = form.replacen("{}", argument, 1);
        (!argument.is_empty()).then_some((Piece::Line { start: first, end, code, head }, end))
    }

    /// O byte da chave que abre o bloco cujo marcador começa no byte `at` de
    /// `src`. O marcador não pode estar colado a outra palavra, nem antes nem
    /// depois.
    fn block_at(&self, src: &str, at: usize) -> Option<usize> {
        let bytes = src.as_bytes();
        if at > 0 && is_word(bytes[at - 1]) {
            return None;
        }
        let marker = self.blocks.iter().find(|marker| src[at..].starts_with(**marker))?;
        let after = at + marker.len();
        if bytes.get(after).is_some_and(|b| is_word(*b)) {
            return None;
        }
        let open = after + bytes[after..].iter().take_while(|b| b.is_ascii_whitespace()).count();
        (bytes.get(open) == Some(&b'{')).then_some(open)
    }
}

/// O byte da chave que fecha a que abre no byte `open`, sem contar as chaves
/// escritas dentro de texto entre aspas, de caractere entre apóstrofos e de
/// comentário de linha (`//`) ou de bloco (`/* */`). Sem ela, o fim do texto.
fn matching_brace(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    let mut at = open;
    while at < bytes.len() {
        match bytes[at] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return at;
                }
            }
            quote @ (b'"' | b'\'') => {
                at += 1;
                while at < bytes.len() && bytes[at] != quote {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at += 2;
                while at + 1 < bytes.len() && !(bytes[at] == b'*' && bytes[at + 1] == b'/') {
                    at += 1;
                }
                at += 1;
            }
            _ => {}
        }
        at += 1;
    }
    bytes.len()
}

/// O byte é parte de uma palavra: letra, dígito ou `_`.
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Põe em `out` um espaço por caractere de `text`, com as quebras de linha
/// no lugar.
fn blank(out: &mut String, text: &str) {
    out.extend(text.chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
}

/// O nome do arquivo de `path`, sem as pastas e sem a extensão.
fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A página de Blazor como o registro a descreve.
    const PAGE: Markup = Markup {
        blocks: &["@code", "@functions"],
        lines: &[("@inject", "{};")],
        head: &[("@using", "using {};")],
        open: "partial class {file} {",
        close: "}",
    };

    /// As linhas do texto lido, com os espaços juntados num só.
    fn lines(text: &str) -> Vec<String> {
        text.lines().map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ")).collect()
    }

    #[test]
    fn the_code_keeps_its_lines_and_the_markup_becomes_spaces() {
        let src = "@page \"/pedidos\"\n@using System.Net.Http.Json\n@inject HttpClient Http\n\n<h3>Pedidos</h3>\n\n\
                   @code {\n    private string? pedido;\n}\n";
        let code = PAGE.code_of(src, "Web/Pages/Pedidos.razor");
        assert_eq!(
            lines(&code),
            [
                "",
                "using System.Net.Http.Json;",
                "partial class Pedidos { HttpClient Http;",
                "",
                "",
                "",
                "",
                "private string? pedido;",
                "}",
            ]
        );
        assert!(code.ends_with('\n'), "{code:?}");
    }

    #[test]
    fn a_brace_inside_a_text_a_character_or_a_comment_does_not_close_the_block() {
        let src = "@code {\n    string a = \"}\";\n    char b = '}';\n    // }\n    /* } */\n    int c;\n}\n<p>depois</p>\n";
        let code = PAGE.code_of(src, "P.razor");
        let got = lines(&code);
        assert_eq!(got[5].trim(), "int c;");
        assert_eq!(got[6].trim(), "}", "the block closes on its own brace: {code:?}");
        assert_eq!(got[7].trim(), "", "the markup after it is blank: {code:?}");
    }

    #[test]
    fn a_marker_glued_to_another_word_or_without_a_brace_is_not_a_block() {
        let src = "<a href=\"mailto:x@code.com\">x</a>\n<p>@codex { nada }</p>\n<p>@code sem chave</p>\n";
        assert_eq!(PAGE.code_of(src, "P.razor").trim(), "");
    }

    #[test]
    fn two_blocks_live_in_one_type_that_closes_after_the_last() {
        let src = "@code {\n    int a;\n}\n<p>meio</p>\n@functions {\n    int b;\n}\n";
        let code = PAGE.code_of(src, "Dois.razor");
        let got = lines(&code);
        assert_eq!(got[0].trim(), "partial class Dois {");
        assert_eq!(got[2].trim(), "", "the first block's brace is blank: {code:?}");
        assert_eq!(got[3].trim(), "");
        assert_eq!(got[6].trim(), "}", "{code:?}");
        assert_eq!(code.matches('{').count(), 1, "{code:?}");
        assert_eq!(code.matches('}').count(), 1, "{code:?}");
    }

    #[test]
    fn a_head_line_written_after_the_first_member_goes_before_the_type() {
        let code = PAGE.code_of("@inject Loja.Carrinho Carrinho\n@using Loja\n@code { }\n", "Compra.razor");
        assert_eq!(lines(&code), ["using Loja; partial class Compra { Loja.Carrinho Carrinho;", "", "}"]);
    }

    #[test]
    fn a_file_with_only_head_lines_opens_no_type() {
        let code = PAGE.code_of("@using Loja.Servicos\n<p>oi</p>\n", "_Imports.razor");
        assert_eq!(lines(&code), ["using Loja.Servicos;", ""]);
    }
}
