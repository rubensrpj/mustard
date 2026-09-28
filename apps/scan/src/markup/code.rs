//! O código escrito no meio da marcação e a marcação escrita no meio do
//! código. O comando de controle (`@if (x) { … }`, `@foreach (…) { … }`) é
//! código, do cabeçalho à chave que fecha o último trecho dele (`else`
//! incluído). Dentro das chaves dele e das de um bloco de corpo (`@{ … }`),
//! o que começa um comando pelo texto de `code_markup` é marcação: o
//! elemento até a tag que o fecha (`<p>…</p>`) e a linha de marcação até o
//! fim dela (`@:texto`). Essa marcação vira espaço, e o código em linha, os
//! comandos de controle, os blocos e as tags de componente escritos nela
//! viram código no lugar deles. A tag de componente é a tag da marcação cujo
//! nome começa com letra maiúscula (`<Contador />`): o nome dela é uso.

use super::{blank, matching, name_len, word_before, Markup, Piece};

impl Markup {
    /// O fim do comando de controle cujo marcador começa no byte `at` de
    /// `src`: o marcador seguido de um nome de `controls`, do cabeçalho entre
    /// parênteses e das chaves, cada trecho seguinte aberto por um nome de
    /// `chains` (com o cabeçalho quando o nome seguinte é de `controls`,
    /// como em `else if (y)`) e as chaves dele. `None` sem as chaves logo
    /// depois do cabeçalho.
    pub(super) fn control_at(&self, src: &str, at: usize) -> Option<usize> {
        let marker = self.expression[0];
        if self.controls.is_empty() || !src.as_bytes()[at..].starts_with(marker.as_bytes()) || word_before(src, at) {
            return None;
        }
        let from = at + marker.len();
        let name = &src[from..from + name_len(&src[from..])];
        if !self.controls.contains(&name) {
            return None;
        }
        let mut end = self.control_part(src, from + name.len(), true)?;
        loop {
            let next = skip_space(src, end);
            let word = &src[next..next + name_len(&src[next..])];
            if !self.chains.contains(&word) {
                return Some(end);
            }
            let after = skip_space(src, next + word.len());
            let inner = &src[after..after + name_len(&src[after..])];
            let headed = self.controls.contains(&inner);
            let from = if headed { after + inner.len() } else { next + word.len() };
            match self.control_part(src, from, headed) {
                Some(part) => end = part,
                None => return Some(end),
            }
        }
    }

    /// Um trecho do comando de controle, a partir do byte `from` de `src`:
    /// o cabeçalho entre parênteses, com `headed`, e as chaves. O byte logo
    /// depois da chave que fecha; `None` sem os parênteses pedidos ou sem a
    /// chave que abre.
    fn control_part(&self, src: &str, from: usize, headed: bool) -> Option<usize> {
        let bytes = src.as_bytes();
        let mut at = skip_space(src, from);
        if headed {
            if bytes.get(at) != Some(&b'(') {
                return None;
            }
            let close = matching(bytes, at);
            if close >= bytes.len() {
                return None;
            }
            at = skip_space(src, close + 1);
        }
        if bytes.get(at) != Some(&b'{') {
            return None;
        }
        Some((self.code_close(src, at) + 1).min(bytes.len()))
    }

    /// A tag de componente que começa no byte `at` de `src`: o `<` seguido
    /// do nome que começa com letra maiúscula, com os nomes ligados por `.`,
    /// e o comando em que ela vira pela forma de `component`, com o byte em
    /// que o nome termina. O resto da tag segue como marcação.
    pub(super) fn component_at(&self, src: &str, at: usize) -> Option<(usize, String)> {
        if self.component.is_empty() || src.as_bytes()[at] != b'<' {
            return None;
        }
        let from = at + 1;
        if !src[from..].chars().next().is_some_and(char::is_uppercase) {
            return None;
        }
        let mut end = from + name_len(&src[from..]);
        while src[end..].starts_with('.') && name_len(&src[end + 1..]) > 0 {
            end += 1 + name_len(&src[end + 1..]);
        }
        Some((end, self.component.replacen("{}", &src[from..end], 1)))
    }

    /// O byte da chave que fecha o bloco de código cuja chave que abre está
    /// no byte `open` de `src`, ou o fim do texto sem ela. A chave escrita em
    /// texto, em comentário ou na marcação do meio do código não conta.
    pub(super) fn code_close(&self, src: &str, open: usize) -> usize {
        self.scan_code(src, open + 1, src.len()).0
    }

    /// Põe em `out` o código de `src` entre os bytes `from` e `to`: o
    /// código como está, e cada trecho de marcação no meio dele como espaço,
    /// com o código dos trechos escritos nela no lugar deles.
    pub(super) fn push_code(&self, out: &mut String, src: &str, from: usize, to: usize) {
        let (_, holes) = self.scan_code(src, from, to);
        let mut at = from;
        for (start, end) in holes {
            out.push_str(&src[at..start]);
            self.push_markup(out, src, start, end);
            at = end;
        }
        out.push_str(&src[at..to]);
    }

    /// Põe em `out` a marcação de `src` entre os bytes `from` e `to`, escrita
    /// no meio do código: espaço, com o código em linha, o das tags de
    /// componente, o dos comandos de controle e o dos blocos de corpo no
    /// lugar deles, e o comentário como comentário.
    fn push_markup(&self, out: &mut String, src: &str, from: usize, to: usize) {
        let mut at = from;
        for piece in self.pieces_between(src, from, to, false) {
            blank(out, &src[at..piece.start()]);
            at = match piece {
                Piece::Expr { end, code, .. } => {
                    out.push_str(&code);
                    end
                }
                Piece::Comment { start, end } => {
                    self.push_comment(out, &src[start..end]);
                    end
                }
                Piece::Control { start, end } => {
                    let code = start + self.expression[0].len();
                    blank(out, &src[start..code]);
                    self.push_code(out, src, code, end);
                    end
                }
                Piece::Block { open, close, body } => {
                    out.push(' ');
                    if body {
                        self.push_code(out, src, open + 1, close);
                    } else {
                        blank(out, &src[open + 1..close]);
                    }
                    close
                }
                Piece::Line { start, .. } => start,
            };
        }
        blank(out, &src[at..to]);
    }

    /// Percorre o código de `src` a partir do byte `from`, até `to` ou até a
    /// chave que fecha o bloco em que ele está, e devolve o byte em que
    /// parou e os trechos de marcação achados no caminho, cada um do começo
    /// ao fim. O texto entre aspas, o caractere entre apóstrofos e o
    /// comentário de linha e de bloco são pulados. O comentário da marcação
    /// é um trecho de marcação onde estiver; o elemento e a linha de marcação
    /// (`code_markup`), só no começo de um comando: no começo do bloco ou
    /// depois de `{`, `}`, `;` ou `:`.
    fn scan_code(&self, src: &str, from: usize, to: usize) -> (usize, Vec<(usize, usize)>) {
        let bytes = src.as_bytes();
        let [element, line] = self.code_markup;
        let mut holes = Vec::new();
        let mut depth = 0usize;
        let mut statement = true;
        let mut at = from;
        while at < to {
            let rest = &bytes[at..];
            if let Some(end) = self.comment_at(src, at) {
                holes.push((at, end.min(to)));
                at = end;
                continue;
            }
            if statement && !element.is_empty() {
                let end = if rest.starts_with(element.as_bytes()) && rest.get(element.len()).is_some_and(u8::is_ascii_alphabetic) {
                    Some(element_end(src, at, element.len()))
                } else if rest.starts_with(line.as_bytes()) {
                    Some(src[at..].find('\n').map_or(src.len(), |n| at + n))
                } else {
                    None
                };
                if let Some(end) = end {
                    holes.push((at, end.min(to)));
                    at = end;
                    continue;
                }
            }
            match bytes[at] {
                b if b.is_ascii_whitespace() => {}
                b'/' if rest.get(1) == Some(&b'/') => {
                    at += rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
                    continue;
                }
                b'/' if rest.get(1) == Some(&b'*') => {
                    at += 2 + src[at + 2..].find("*/").map_or(src.len() - at - 2, |n| n + 2);
                    continue;
                }
                quote @ (b'"' | b'\'') => {
                    let verbatim = quote == b'"' && at > 0 && bytes[at - 1] == b'@';
                    at = text_end(bytes, at, verbatim);
                    statement = false;
                    continue;
                }
                b'{' => {
                    depth += 1;
                    statement = true;
                }
                b'}' => {
                    if depth == 0 {
                        return (at, holes);
                    }
                    depth -= 1;
                    statement = true;
                }
                b';' | b':' => statement = true,
                _ => statement = false,
            }
            at += 1;
        }
        (at.min(to), holes)
    }
}

/// O byte logo depois do texto entre aspas ou do caractere entre
/// apóstrofos que abre no byte `open` de `bytes`: a barra invertida pula o
/// que vem depois dela; no texto literal (`verbatim`), a aspa dobrada é
/// parte do texto.
fn text_end(bytes: &[u8], open: usize, verbatim: bool) -> usize {
    let quote = bytes[open];
    let mut at = open + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' if !verbatim => at += 2,
            b if b == quote && verbatim && bytes.get(at + 1) == Some(&quote) => at += 2,
            b if b == quote => return at + 1,
            b'\n' if !verbatim => return at,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// O byte logo depois do elemento de marcação cuja tag abre no byte `at` de
/// `src`, com `opener` bytes antes do nome: a tag que se fecha sozinha
/// (`<br />`) termina nela; a outra, na tag que fecha o mesmo nome, contadas
/// as do mesmo nome abertas dentro dele, ou, sem ela, no fim da tag que o
/// abre (`<input>`).
fn element_end(src: &str, at: usize, opener: usize) -> usize {
    let bytes = src.as_bytes();
    let from = at + opener;
    let name = &src[from..from + tag_name_len(&src[from..])];
    let Some(open_end) = tag_end(bytes, from + name.len()) else { return src.len() };
    if bytes[open_end - 2] == b'/' {
        return open_end;
    }
    let mut depth = 1usize;
    let mut next = open_end;
    while let Some(found) = src[next..].find('<') {
        let lt = next + found;
        if src[lt + 1..].starts_with('/') && names_tag(&src[lt + 2..], name) {
            let Some(close_end) = tag_end(bytes, lt + 2 + name.len()) else { return src.len() };
            depth -= 1;
            if depth == 0 {
                return close_end;
            }
            next = close_end;
        } else if names_tag(&src[lt + 1..], name) {
            let Some(inner_end) = tag_end(bytes, lt + 1 + name.len()) else { return src.len() };
            if bytes[inner_end - 2] != b'/' {
                depth += 1;
            }
            next = inner_end;
        } else {
            next = lt + 1;
        }
    }
    open_end
}

/// O tamanho do nome de tag escrito no começo de `text`: letras, dígitos,
/// `_`, `-`, `.` e `:`.
fn tag_name_len(text: &str) -> usize {
    text.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))).unwrap_or(text.len())
}

/// `text` começa pelo nome de tag `name`, sem outra letra do nome depois.
fn names_tag(text: &str, name: &str) -> bool {
    text.starts_with(name) && tag_name_len(&text[name.len()..]) == 0
}

/// O byte logo depois do `>` que fecha a tag, a partir do byte `from` de
/// `bytes`, pulado o `>` escrito no valor de um atributo entre aspas.
/// `None` sem ele.
fn tag_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    while at < bytes.len() {
        match bytes[at] {
            b'>' => return Some(at + 1),
            quote @ (b'"' | b'\'') => {
                at += 1 + bytes[at + 1..].iter().position(|&b| b == quote)?;
            }
            _ => {}
        }
        at += 1;
    }
    None
}

/// O primeiro byte de `src` a partir de `from` que não é espaço.
fn skip_space(src: &str, from: usize) -> usize {
    from + src[from..].bytes().take_while(u8::is_ascii_whitespace).count()
}
