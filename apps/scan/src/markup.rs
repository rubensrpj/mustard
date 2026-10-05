//! A língua escrita dentro de um arquivo de marcação: a página que mistura o
//! texto da tela com trechos de código. O registro (`markup` em
//! languages.toml) diz onde o código está; aqui ele sai como o texto que a
//! gramática da língua lê. O resto do arquivo vira espaço e cada linha fica no
//! lugar dela, para que as linhas do mapa sejam as do arquivo.
//!
//! O código dos blocos e das linhas de membro mora num tipo com o nome do
//! arquivo, aberto no começo da linha do primeiro código e fechado logo
//! depois do último, com as bases que as linhas de base dão a ele. O código
//! das linhas de cabeça fica fora desse tipo, antes dele. O bloco de corpo e
//! o código em linha, escrito no meio da marcação, trazem comandos, e não
//! membros: moram num método do tipo, e os seguidos, sem membro entre eles,
//! dividem o mesmo método. O comentário da marcação vira comentário do código, no lugar
//! dele. O arquivo sem código nenhum sai com o tipo vazio no começo da
//! primeira linha, e o resto em espaços e comentários; o arquivo de imports
//! da pasta, sem tipo.
//!
//! O comando de controle escrito na marcação (`@if (x) { … }`) é código do
//! método, e a marcação escrita no meio do código dele ou de um bloco de corpo
//! vira espaço, com o código em linha dela no lugar ([`code`]). As linhas do
//! arquivo de imports da pasta que valem nos arquivos dela entram no tipo de
//! cada um ([`folder`]).

mod code;
mod folder;

pub(crate) use folder::imports_above;

/// Onde mora o código num arquivo de marcação, como o registro o declara.
pub(crate) struct Markup {
    /// Os marcadores seguidos de um bloco entre chaves: o de dentro das
    /// chaves é código.
    pub blocks: &'static [&'static str],
    /// Como `blocks`, mas o de dentro das chaves é o corpo de um método.
    pub bodies: &'static [&'static str],
    /// O texto que abre e o que fecha o método em que o código de `bodies`
    /// e do código em linha mora; vazio sem `bodies`.
    pub method: [&'static str; 2],
    /// O marcador que abre uma linha de código dentro do tipo do arquivo e a
    /// forma dela, com `{}` no lugar do resto da linha.
    pub lines: &'static [(&'static str, &'static str)],
    /// Como `lines`, mas o código vai antes do tipo do arquivo.
    pub head: &'static [(&'static str, &'static str)],
    /// O texto que abre o tipo do arquivo, com `{file}` no lugar do nome
    /// dele sem a extensão e `{bases}` no lugar da lista de bases.
    pub open: &'static str,
    /// O texto que fecha o tipo do arquivo.
    pub close: &'static str,
    /// O marcador que abre código em linha no meio da marcação e a forma do
    /// comando em que ele vira, com `{}` no lugar dele; vazios sem código em
    /// linha.
    pub expression: [&'static str; 2],
    /// O texto que se lê como o marcador escrito na tela, e não como código;
    /// vazio sem ele.
    pub escape: &'static str,
    /// Os nomes que, logo depois do marcador do código em linha, abrem uma
    /// diretiva ou um comando de controle, e não código em linha.
    pub keywords: &'static [&'static str],
    /// Os nomes que, logo depois do marcador do código em linha, seguem de
    /// um espaço e do código, que os leva junto.
    pub prefixes: &'static [&'static str],
    /// O texto que abre e o que fecha o comentário da marcação, e os que o
    /// trocam no código; vazios sem comentário.
    pub comment: [&'static str; 4],
    /// Os marcadores de linha cujo resto é uma base do tipo do arquivo.
    pub bases: &'static [&'static str],
    /// O texto antes da lista de bases e o que fica entre duas delas.
    pub base_list: [&'static str; 2],
    /// O nome, sem a extensão, do arquivo cujos imports valem também nos
    /// arquivos da mesma língua da pasta dele e das de baixo; vazio sem ele.
    pub imports_file: &'static str,
    /// Os marcadores de linha cujas linhas, escritas no arquivo de imports da
    /// pasta, valem também no tipo dos arquivos da mesma língua da pasta dele
    /// e das de baixo.
    pub folder: &'static [&'static str],
    /// O marcador de `folder` cujo resto, herdado, ganha os nomes das pastas
    /// entre a do arquivo de imports e a do arquivo, e o texto que os junta;
    /// vazios sem ele.
    pub folder_path: [&'static str; 2],
    /// O tipo (`kind`) do manifesto cujo nome de projeto começa o marcador de
    /// `folder_path` que nenhum arquivo de imports escreve: vale como uma
    /// linha dele escrita num arquivo de imports na pasta do manifesto;
    /// vazio sem ele.
    pub folder_root: &'static str,
    /// Os nomes que, logo depois do marcador do código em linha, abrem um
    /// comando de controle, com o cabeçalho e as chaves dele.
    pub controls: &'static [&'static str],
    /// Os nomes que, depois das chaves de um comando de controle, o
    /// continuam.
    pub chains: &'static [&'static str],
    /// Os nomes que, entre o cabeçalho de um trecho de um comando de controle
    /// (ou no lugar dele) e as chaves, abrem uma condição entre parênteses.
    pub filters: &'static [&'static str],
    /// A forma do comando em que vira a tag de componente, com `{}` no lugar
    /// do nome dela; vazia sem ela.
    pub component: &'static str,
    /// O texto que abre um elemento de marcação no meio do código de um
    /// bloco de corpo e o que abre uma linha de marcação ali; vazios sem
    /// marcação no código.
    pub code_markup: [&'static str; 2],
}

/// O que uma linha de código é no tipo do arquivo.
#[derive(Clone, Copy)]
enum LineKind {
    /// Um membro do tipo, escrito no lugar da linha.
    Member,
    /// Código de antes do tipo.
    Head,
    /// Uma base do tipo, com a posição do marcador dela na lista de bases.
    Base(usize),
}

/// Um trecho de código achado no arquivo.
enum Piece {
    /// Um bloco: o byte da chave que abre e o da que fecha, sem a que fecha
    /// o fim do arquivo, e se o de dentro é o corpo de um método.
    Block { open: usize, close: usize, body: bool },
    /// Uma linha: do começo do marcador ao fim da linha, com o marcador, o
    /// resto dela, o código que ela vira (na linha de base, a base) e o que
    /// ela é.
    Line { start: usize, end: usize, marker: &'static str, argument: String, code: String, kind: LineKind },
    /// Código em linha, escrito no meio da marcação: do marcador ao fim dele,
    /// com o comando que ele vira.
    Expr { start: usize, end: usize, code: String },
    /// Um comentário da marcação: do texto que o abre ao fim do que o fecha,
    /// ou ao fim do arquivo quando nada o fecha.
    Comment { start: usize, end: usize },
    /// Um comando de controle: do marcador ao fim da chave que fecha o último
    /// trecho dele. O que vem depois do marcador é código, com a marcação de
    /// dentro das chaves fora dele.
    Control { start: usize, end: usize },
}

impl Piece {
    fn start(&self) -> usize {
        match self {
            Piece::Block { open, .. } => *open,
            Piece::Line { start, .. }
            | Piece::Expr { start, .. }
            | Piece::Comment { start, .. }
            | Piece::Control { start, .. } => *start,
        }
    }

    /// O trecho faz parte do tipo do arquivo: ele abre o tipo, quando é o
    /// primeiro, e o fecha, quando é o último.
    fn in_type(&self) -> bool {
        match self {
            Piece::Block { .. } | Piece::Expr { .. } | Piece::Control { .. } => true,
            Piece::Line { kind, .. } => !matches!(kind, LineKind::Head),
            Piece::Comment { .. } => false,
        }
    }

    /// O trecho escreve código no corpo do tipo: um membro, que fecha o
    /// método dos comandos, ou um comando, que mora nele.
    fn writes_in_type(&self) -> bool {
        match self {
            Piece::Block { .. } | Piece::Expr { .. } | Piece::Control { .. } => true,
            Piece::Line { kind, .. } => matches!(kind, LineKind::Member),
            Piece::Comment { .. } => false,
        }
    }

    fn is_body(&self) -> bool {
        matches!(self, Piece::Block { body: true, .. } | Piece::Expr { .. } | Piece::Control { .. })
    }
}

impl Markup {
    /// O arquivo `path` é o de imports da pasta: os imports dele valem nos
    /// arquivos da mesma língua da pasta dele e das de baixo.
    pub(crate) fn is_imports_file(&self, path: &str) -> bool {
        !self.imports_file.is_empty() && file_stem(path) == self.imports_file
    }

    /// O texto que a gramática lê do arquivo `src`, no caminho `path`: só o
    /// código, com as linhas no lugar delas. O código de cabeça escrito antes
    /// do primeiro código do tipo fica na linha dele; o escrito depois vai
    /// para a linha em que o tipo abre, antes dele. O método dos comandos
    /// abre no primeiro comando cujo código anterior no tipo não é comando, e
    /// fecha no fim do comando cujo código seguinte no tipo não é. As linhas
    /// herdadas dos arquivos de imports da pasta (`folder`, com o caminho e o
    /// texto de cada um, da pasta de cima para a de baixo) entram na linha em
    /// que o tipo abre: as de cabeça antes dele, as bases na lista dele e os
    /// membros logo depois de abri-lo. O arquivo sem código do tipo, que não
    /// é o de imports da pasta, é o tipo mesmo assim: ele abre e fecha no
    /// começo da primeira linha, depois de todo o código de cabeça.
    pub(crate) fn code_of(&self, src: &str, path: &str, folder: &[(&str, &str)]) -> String {
        let pieces = self.pieces(src);
        let first_in_type = pieces.iter().position(Piece::in_type);
        let last_in_type = pieces.iter().rposition(Piece::in_type);
        let body_before = |i: usize| pieces[..i].iter().rev().find(|piece| piece.writes_in_type()).is_some_and(Piece::is_body);
        let body_after = |i: usize| pieces[i + 1..].iter().find(|piece| piece.writes_in_type()).is_some_and(Piece::is_body);
        let mut out = String::with_capacity(src.len() + 64);
        let empty_type = first_in_type.is_none() && !self.open.is_empty() && !self.is_imports_file(path);
        if empty_type {
            self.open_type(&mut out, &pieces, 0, path, folder);
            out.push_str(self.close);
        }
        let mut at = 0;
        for (i, piece) in pieces.iter().enumerate() {
            if first_in_type == Some(i) {
                let line_start = src[..piece.start()].rfind('\n').map_or(0, |n| n + 1).max(at);
                blank(&mut out, &src[at..line_start]);
                self.open_type(&mut out, &pieces, i, path, folder);
                at = line_start;
            }
            let moved = empty_type || first_in_type.is_some_and(|first| i > first);
            let closes = last_in_type == Some(i);
            match piece {
                Piece::Block { open, close, body } => {
                    blank(&mut out, &src[at..=*open]);
                    if *body && !body_before(i) {
                        out.push_str(self.method[0]);
                    }
                    if *body {
                        self.push_code(&mut out, src, open + 1, *close);
                    } else {
                        out.push_str(&src[open + 1..*close]);
                    }
                    at = *close;
                    if *body && !body_after(i) {
                        out.push_str(self.method[1]);
                    }
                }
                Piece::Line { end, code, kind, .. } => {
                    blank(&mut out, &src[at..*end]);
                    if matches!(kind, LineKind::Member) || (matches!(kind, LineKind::Head) && !moved) {
                        out.push_str(code);
                    }
                    at = *end;
                }
                Piece::Expr { start, end, code } => {
                    blank(&mut out, &src[at..*start]);
                    if !body_before(i) {
                        out.push_str(self.method[0]);
                    }
                    out.push_str(code);
                    at = *end;
                    if !body_after(i) {
                        out.push_str(self.method[1]);
                    }
                }
                Piece::Comment { start, end } => {
                    blank(&mut out, &src[at..*start]);
                    self.push_comment(&mut out, &src[*start..*end]);
                    at = *end;
                }
                Piece::Control { start, end } => {
                    blank(&mut out, &src[at..*start]);
                    if !body_before(i) {
                        out.push_str(self.method[0]);
                    }
                    let code = start + self.expression[0].len();
                    blank(&mut out, &src[*start..code]);
                    self.push_code(&mut out, src, code, *end);
                    at = *end;
                    if !body_after(i) {
                        out.push_str(self.method[1]);
                    }
                }
            }
            if closes {
                out.push_str(self.close);
            }
        }
        blank(&mut out, &src[at..]);
        out
    }

    /// Põe em `out` a abertura do tipo do arquivo `path`, cujos trechos são
    /// `pieces`: o código de cabeça dos trechos a partir do de posição
    /// `first` e o herdado da pasta (`folder`), o texto que abre o tipo, com
    /// as bases, e os membros herdados.
    fn open_type(&self, out: &mut String, pieces: &[Piece], first: usize, path: &str, folder: &[(&str, &str)]) {
        for later in &pieces[first..] {
            if let Piece::Line { code, kind: LineKind::Head, .. } = later {
                out.push_str(code);
                out.push(' ');
            }
        }
        let inherited = self.inherited(pieces, path, folder);
        for (kind, code) in &inherited {
            if matches!(kind, LineKind::Head) {
                out.push_str(code);
                out.push(' ');
            }
        }
        out.push_str(&self.open.replace("{file}", file_stem(path)).replace("{bases}", &self.bases_of(pieces, &inherited)));
        for (kind, code) in &inherited {
            if matches!(kind, LineKind::Member) {
                out.push(' ');
                out.push_str(code);
            }
        }
    }

    /// As bases que as linhas de base dão ao tipo, na ordem dos marcadores
    /// e, com o mesmo marcador, na do arquivo, depois as herdadas da pasta
    /// (`inherited`), já na forma de `base_list`; vazio sem nenhuma.
    fn bases_of(&self, pieces: &[Piece], inherited: &[(LineKind, String)]) -> String {
        let own = pieces.iter().filter_map(|piece| match piece {
            Piece::Line { code, kind: LineKind::Base(order), .. } => Some((*order, code.as_str())),
            _ => None,
        });
        let from_folder = inherited.iter().filter_map(|(kind, code)| match kind {
            LineKind::Base(order) => Some((*order, code.as_str())),
            _ => None,
        });
        let mut named: Vec<(usize, &str)> = own.chain(from_folder).collect();
        if named.is_empty() {
            return String::new();
        }
        named.sort_by_key(|(order, _)| *order);
        let names: Vec<&str> = named.into_iter().map(|(_, name)| name).collect();
        format!("{}{}", self.base_list[0], names.join(self.base_list[1]))
    }

    /// Põe em `out` o comentário da marcação `written`, com os textos que o
    /// abrem e o fecham trocados pelos do código. O texto que fecharia o
    /// comentário do código antes da hora vira espaço.
    fn push_comment(&self, out: &mut String, written: &str) {
        let [open, close, code_open, code_close] = self.comment;
        let inner = written.strip_prefix(open).unwrap_or(written);
        let inner = inner.strip_suffix(close).unwrap_or(inner);
        out.push_str(code_open);
        let mut rest = inner;
        while let Some(at) = rest.find(code_close) {
            out.push_str(&rest[..at]);
            blank(out, code_close);
            rest = &rest[at + code_close.len()..];
        }
        out.push_str(rest);
        out.push_str(code_close);
    }

    /// Os trechos de código de `src`, na ordem do arquivo. A linha de código
    /// começa pelo marcador, depois dos espaços do começo da linha; o
    /// comentário, pelo texto que o abre; o bloco começa pelo marcador escrito
    /// fora de outra palavra e segue na chave que vem depois dele, passados os
    /// espaços; o código em linha, pelo marcador escrito fora de outra
    /// palavra; o comando de controle, pelo marcador seguido de um nome de
    /// `controls`; a tag de componente, pelo `<` seguido de letra maiúscula.
    /// O marcador escrito na tela (`escape`) não abre nada.
    fn pieces(&self, src: &str) -> Vec<Piece> {
        self.pieces_between(src, 0, src.len(), true)
    }

    /// Os trechos de código de `src` entre os bytes `from` e `to`, como em
    /// [`Self::pieces`]; as linhas de código só com `lines`. O trecho que
    /// passaria de `to` não conta.
    fn pieces_between(&self, src: &str, from: usize, to: usize, lines: bool) -> Vec<Piece> {
        let bytes = src.as_bytes();
        let mut out = Vec::new();
        let mut at = from;
        let mut line_start = lines;
        while at < to {
            if line_start {
                // O comando de controle vence a linha de mesmo marcador:
                // `@using (…) { … }` é comando, e `@using Loja`, linha.
                let first = at + bytes[at..].iter().take_while(|b| matches!(b, b' ' | b'\t')).count();
                if self.control_at(src, first).is_none()
                    && let Some((line, end)) = self.line_at(src, first)
                {
                    out.push(line);
                    at = end;
                    line_start = false;
                    continue;
                }
            }
            if let Some(end) = self.comment_at(src, at).filter(|&end| end <= to) {
                out.push(Piece::Comment { start: at, end });
                at = end;
                line_start = false;
                continue;
            }
            if !self.escape.is_empty() && bytes[at..].starts_with(self.escape.as_bytes()) {
                at += self.escape.len();
                line_start = false;
                continue;
            }
            if let Some((open, body)) = self.block_at(src, at) {
                let close = if body { self.code_close(src, open) } else { matching(bytes, open) };
                if close < to || to == src.len() {
                    out.push(Piece::Block { open, close, body });
                    at = (close + 1).min(bytes.len());
                    line_start = false;
                    continue;
                }
            }
            if let Some(end) = self.control_at(src, at).filter(|&end| end <= to) {
                out.push(Piece::Control { start: at, end });
                at = end;
                line_start = false;
                continue;
            }
            if let Some((end, code)) = self.component_at(src, at).or_else(|| self.expression_at(src, at)).filter(|(end, _)| *end <= to) {
                out.push(Piece::Expr { start: at, end, code });
                at = end;
                line_start = false;
                continue;
            }
            line_start = lines && bytes[at] == b'\n';
            at += 1;
        }
        out
    }

    /// A linha de código que começa no byte `first` de `src`, com o byte em
    /// que ela termina: o marcador, um espaço e o resto da linha, que não
    /// pode ser vazio.
    fn line_at(&self, src: &str, first: usize) -> Option<(Piece, usize)> {
        let rest = &src[first..];
        let (marker, form, kind): (&'static str, &'static str, LineKind) = self
            .lines
            .iter()
            .map(|(marker, form)| (*marker, *form, LineKind::Member))
            .chain(self.head.iter().map(|(marker, form)| (*marker, *form, LineKind::Head)))
            .chain(self.bases.iter().enumerate().map(|(order, marker)| (*marker, "{}", LineKind::Base(order))))
            .find(|(marker, ..)| rest.starts_with(marker) && rest[marker.len()..].starts_with([' ', '\t']))?;
        let end = first + rest.find('\n').unwrap_or(rest.len());
        let argument = src[first + marker.len()..end].trim();
        let code = form.replacen("{}", argument, 1);
        let line = Piece::Line { start: first, end, marker, argument: argument.to_string(), code, kind };
        (!argument.is_empty()).then_some((line, end))
    }

    /// O byte em que termina o comentário cujo texto de abrir começa no byte
    /// `at` de `src`: logo depois do texto que o fecha ou, sem ele, o fim do
    /// arquivo.
    fn comment_at(&self, src: &str, at: usize) -> Option<usize> {
        let [open, close, ..] = self.comment;
        if open.is_empty() || !src.as_bytes()[at..].starts_with(open.as_bytes()) {
            return None;
        }
        let from = at + open.len();
        Some(src[from..].find(close).map_or(src.len(), |n| from + n + close.len()))
    }

    /// O byte da chave que abre o bloco cujo marcador começa no byte `at` de
    /// `src`, e se o bloco é de corpo. O marcador não pode estar colado a
    /// outra palavra, nem antes nem depois; o que começa como outro marcador
    /// (`@` e `@functions`) vale pelo que tem a chave depois dele. O byte
    /// `at` pode cair no meio de um caractere: a comparação é por bytes.
    fn block_at(&self, src: &str, at: usize) -> Option<(usize, bool)> {
        let bytes = src.as_bytes();
        if at > 0 && is_word(bytes[at - 1]) {
            return None;
        }
        let markers = self.blocks.iter().map(|marker| (marker, false)).chain(self.bodies.iter().map(|marker| (marker, true)));
        markers.filter(|(marker, _)| bytes[at..].starts_with(marker.as_bytes())).find_map(|(marker, body)| {
            let after = at + marker.len();
            if bytes.get(after).is_some_and(|b| is_word(*b)) {
                return None;
            }
            let open = after + bytes[after..].iter().take_while(|b| b.is_ascii_whitespace()).count();
            (bytes.get(open) == Some(&b'{')).then_some((open, body))
        })
    }

    /// O código em linha cujo marcador começa no byte `at` de `src`, com o
    /// byte em que ele termina e o comando que ele vira. Seguido de `(`, o
    /// marcador abre o de dentro dos parênteses; seguido de um nome, `=` e um
    /// valor entre aspas, o valor; seguido de um nome de `prefixes`, esse nome
    /// e o código depois dele; seguido de outro nome, fora de `keywords`, o
    /// código escrito sem parênteses ([`implicit_end`]). O marcador colado
    /// depois de outra palavra não abre nada.
    fn expression_at(&self, src: &str, at: usize) -> Option<(usize, String)> {
        let [marker, statement] = self.expression;
        if marker.is_empty() || !src.as_bytes()[at..].starts_with(marker.as_bytes()) || word_before(src, at) {
            return None;
        }
        let from = at + marker.len();
        let (code, end) = if src[from..].starts_with('(') {
            let close = matching(src.as_bytes(), from);
            (close < src.len()).then_some((&src[from..=close], close + 1))?
        } else if let Some((value, end)) = self.attribute_value(src, from) {
            (value, end)
        } else {
            let word = name_len(&src[from..]);
            let name = &src[from..from + word];
            if word == 0 || self.keywords.contains(&name) {
                return None;
            }
            let mut start = from + word;
            if self.prefixes.contains(&name) {
                let gap = src[start..].len() - src[start..].trim_start_matches([' ', '\t']).len();
                if gap == 0 || name_len(&src[start + gap..]) == 0 {
                    return None;
                }
                start += gap;
            } else {
                start = from;
            }
            let end = implicit_end(src, start);
            (&src[from..end], end)
        };
        (!code.trim().is_empty()).then(|| (end, statement.replacen("{}", code.trim(), 1)))
    }

    /// O valor do atributo escrito logo depois do marcador, no byte `from`
    /// de `src`: o nome dele (com `-` e `:`), `=` e o valor entre aspas, com
    /// o byte logo depois da aspa que o fecha. O valor que começa pelo
    /// marcador perde o marcador; o que começa por ele e `(` vai até o `)`
    /// que fecha o parêntese, com as aspas escritas dentro dele.
    fn attribute_value<'s>(&self, src: &'s str, from: usize) -> Option<(&'s str, usize)> {
        let marker = self.expression[0];
        if name_len(&src[from..]) == 0 {
            return None;
        }
        let name = src[from..].find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | ':'))).map_or(src.len(), |n| from + n);
        let rest = src[name..].strip_prefix('=')?;
        let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let value = name + 2;
        let code = src[value..].strip_prefix(marker).map_or(value, |_| value + marker.len());
        let close = if src[code..].starts_with('(') {
            let paren = matching(src.as_bytes(), code);
            (paren < src.len()).then_some(paren + 1)?
        } else {
            code
        };
        let quote_at = close + src[close..].find(quote)?;
        Some((&src[code..quote_at], quote_at + 1))
    }
}

/// O byte em que termina o código em linha escrito sem parênteses que
/// começa no byte `start` de `src`: os nomes ligados por `.` ou `?.`, cada um seguido dos
/// parênteses e colchetes escritos colados a ele (`Html.Raw(x)`,
/// `itens?[0]`). O ponto sem nome depois dele é da marcação, e o parêntese
/// que não fecha também.
fn implicit_end(src: &str, start: usize) -> usize {
    let bytes = src.as_bytes();
    let mut at = start + name_len(&src[start..]);
    loop {
        let open = match (bytes.get(at), bytes.get(at + 1)) {
            (Some(b'(' | b'['), _) => at,
            (Some(b'?'), Some(b'[')) => at + 1,
            (Some(b'.'), _) if name_len(&src[at + 1..]) > 0 => {
                at += 1 + name_len(&src[at + 1..]);
                continue;
            }
            (Some(b'?'), Some(b'.')) if name_len(&src[at + 2..]) > 0 => {
                at += 2 + name_len(&src[at + 2..]);
                continue;
            }
            _ => return at,
        };
        let close = matching(bytes, open);
        if close >= bytes.len() {
            return at;
        }
        at = close + 1;
    }
}

/// O tamanho, em bytes, do nome escrito no começo de `text`: uma letra ou
/// `_`, seguida de letras, dígitos e `_`. Zero sem nome.
fn name_len(text: &str) -> usize {
    let mut chars = text.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_alphabetic() || c == '_' => {}
        _ => return 0,
    }
    chars.find(|(_, c)| !(c.is_alphanumeric() || *c == '_')).map_or(text.len(), |(at, _)| at)
}

/// O caractere logo antes do byte `at` de `src` é parte de uma palavra:
/// letra, dígito ou `_`.
fn word_before(src: &str, at: usize) -> bool {
    src[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// O byte que fecha o parêntese, o colchete ou a chave que abre no byte
/// `open`, sem contar os escritos dentro de texto entre aspas, de caractere
/// entre apóstrofos e de comentário de linha (`//`) ou de bloco (`/* */`).
/// Sem ele, o fim do texto.
fn matching(bytes: &[u8], open: usize) -> usize {
    let (opens, closes) = match bytes[open] {
        b'(' => (b'(', b')'),
        b'[' => (b'[', b']'),
        _ => (b'{', b'}'),
    };
    let mut depth = 0usize;
    let mut at = open;
    while at < bytes.len() {
        match bytes[at] {
            b if b == opens => depth += 1,
            b if b == closes => {
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

    /// O registro sem nada além do tipo do arquivo.
    const PLAIN: Markup = Markup {
        blocks: &[],
        bodies: &[],
        method: ["", ""],
        lines: &[],
        head: &[],
        open: "partial class {file} {",
        close: "}",
        expression: ["", ""],
        escape: "",
        keywords: &[],
        prefixes: &[],
        comment: ["", "", "", ""],
        bases: &[],
        base_list: ["", ""],
        imports_file: "",
        folder: &[],
        folder_path: ["", ""],
        folder_root: "",
        controls: &[],
        chains: &[],
        filters: &[],
        component: "",
        code_markup: ["", ""],
    };

    /// A página de Blazor só com os blocos e as linhas.
    const PAGE: Markup = Markup {
        blocks: &["@code", "@functions"],
        lines: &[("@inject", "{};")],
        head: &[("@using", "using {};")],
        ..PLAIN
    };

    /// As linhas do texto lido, com os espaços juntados num só.
    fn lines(text: &str) -> Vec<String> {
        text.lines().map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ")).collect()
    }

    #[test]
    fn the_code_keeps_its_lines_and_the_markup_becomes_spaces() {
        let src = "@page \"/pedidos\"\n@using System.Net.Http.Json\n@inject HttpClient Http\n\n<h3>Pedidos</h3>\n\n\
                   @code {\n    private string? pedido;\n}\n";
        let code = PAGE.code_of(src, "Web/Pages/Pedidos.razor", &[]);
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
        let code = PAGE.code_of(src, "P.razor", &[]);
        let got = lines(&code);
        assert_eq!(got[5].trim(), "int c;");
        assert_eq!(got[6].trim(), "}", "the block closes on its own brace: {code:?}");
        assert_eq!(got[7].trim(), "", "the markup after it is blank: {code:?}");
    }

    #[test]
    fn a_marker_glued_to_another_word_or_without_a_brace_is_not_a_block() {
        let src = "<a href=\"mailto:x@code.com\">x</a>\n<p>@codex { nada }</p>\n<p>@code sem chave</p>\n";
        assert_eq!(PAGE.code_of(src, "P.razor", &[]).trim(), "partial class P {}", "only the empty type of the file");
    }

    #[test]
    fn two_blocks_live_in_one_type_that_closes_after_the_last() {
        let src = "@code {\n    int a;\n}\n<p>meio</p>\n@functions {\n    int b;\n}\n";
        let code = PAGE.code_of(src, "Dois.razor", &[]);
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
        let code = PAGE.code_of("@inject Loja.Carrinho Carrinho\n@using Loja\n@code { }\n", "Compra.razor", &[]);
        assert_eq!(lines(&code), ["using Loja; partial class Compra { Loja.Carrinho Carrinho;", "", "}"]);
    }

    /// A view do Razor só com os blocos e as linhas.
    const VIEW: Markup = Markup {
        blocks: &["@functions"],
        bodies: &["@"],
        method: ["async Task ExecuteAsync() {", "}"],
        lines: &[("@inject", "{};"), ("@model", "{} Model;")],
        head: &[("@using", "using {};")],
        open: "class {file} {",
        ..PLAIN
    };

    #[test]
    fn a_body_block_is_the_body_of_a_method_of_the_type() {
        let src = "@page\n@model IndexModel\n@{\n    ViewData[\"Title\"] = \"Início\";\n}\n<h1>@ViewData[\"Title\"]</h1>\n";
        let code = VIEW.code_of(src, "Pages/Index.cshtml", &[]);
        assert_eq!(
            lines(&code),
            ["", "class Index { IndexModel Model;", "async Task ExecuteAsync() {", "ViewData[\"Title\"] = \"Início\";", "}}", ""]
        );
    }

    #[test]
    fn body_blocks_in_a_row_share_one_method_and_a_member_between_them_opens_another() {
        let src = "@{ var a = 1; }\n<p>@a</p>\n@using Loja\n@{ a++; }\n@inject Loja.Carrinho Carrinho\n@{ var b = 2; }\n";
        let code = VIEW.code_of(src, "Views/Home/V.cshtml", &[]);
        assert_eq!(
            lines(&code),
            [
                "using Loja; class V { async Task ExecuteAsync() { var a = 1;",
                "",
                "",
                "a++; }",
                "Loja.Carrinho Carrinho;",
                "async Task ExecuteAsync() { var b = 2; }}",
            ]
        );
    }

    #[test]
    fn a_member_block_whose_marker_starts_like_the_body_marker_stays_a_member() {
        let src = "@{ var total = Dobro(2); }\n@functions { int Dobro(int x) => x * 2; }\n<a href=\"mailto:x@{y}\">x</a>\n";
        let code = VIEW.code_of(src, "V.cshtml", &[]);
        assert_eq!(
            lines(&code),
            ["class V { async Task ExecuteAsync() { var total = Dobro(2); }", "int Dobro(int x) => x * 2; }", ""]
        );
    }

    #[test]
    fn a_page_written_with_accents_reads_its_code() {
        let src = "<h3>Ação à vista</h3>\n@code {\n    int preço;\n}\n";
        let code = PAGE.code_of(src, "Preço.razor", &[]);
        assert_eq!(lines(&code), ["", "partial class Preço {", "int preço;", "}"]);
    }

    #[test]
    fn the_imports_file_with_only_head_lines_opens_no_type() {
        let folder = Markup { imports_file: "_Imports", ..PAGE };
        let code = folder.code_of("@using Loja.Servicos\n<p>oi</p>\n", "_Imports.razor", &[]);
        assert_eq!(lines(&code), ["using Loja.Servicos;", ""]);
    }

    #[test]
    fn a_page_without_code_opens_its_empty_type_on_the_first_line_after_the_head_code() {
        let code = PAGE.code_of("<p>oi</p>\n@using Loja.Servicos\n<p>tchau</p>\n", "Pagina.razor", &[]);
        assert_eq!(lines(&code), ["using Loja.Servicos; partial class Pagina {}", "", ""]);
    }

    /// A view do Razor com as expressões, os comentários e as bases.
    const FULL: Markup = Markup {
        blocks: &["@functions"],
        bodies: &["@"],
        method: ["void Draw() {", "}"],
        lines: &[("@model", "{} Model;")],
        head: &[("@using", "using {};")],
        open: "class {file}{bases} {",
        expression: ["@", "_ = {};"],
        escape: "@@",
        keywords: &["page", "if", "functions"],
        prefixes: &["await"],
        comment: ["@*", "*@", "/*", "*/"],
        bases: &["@inherits", "@implements"],
        base_list: [" : ", ", "],
        imports_file: "_ViewImports",
        ..PLAIN
    };

    /// As expressões de uma linha de marcação, como os comandos que viram.
    fn commands(src: &str) -> Vec<String> {
        let code = FULL.code_of(src, "V.cshtml", &[]);
        code.split("_ = ").skip(1).map(|rest| rest[..rest.find(';').unwrap()].to_string()).collect()
    }

    #[test]
    fn an_implicit_expression_takes_the_names_joined_by_dots_and_what_is_glued_to_them() {
        assert_eq!(commands("<p>@Model.Total.</p>"), ["Model.Total"], "the dot with no name after it is text");
        assert_eq!(commands("<p>@Html.Raw(\"a)b\")!</p>"), ["Html.Raw(\"a)b\")"], "a parenthesis inside a text does not close");
        assert_eq!(commands("<p>@itens?[0]?.Nome e @lista[1]</p>"), ["itens?[0]?.Nome", "lista[1]"]);
        assert_eq!(commands("<p>@preço, @Model .Total</p>"), ["preço", "Model"], "the space ends the expression");
        assert_eq!(commands("<p>@Abrir(</p>"), ["Abrir"], "the parenthesis that does not close is text");
    }

    #[test]
    fn an_explicit_expression_an_attribute_and_an_awaited_expression_are_code() {
        assert_eq!(commands("<p>@(a + (b * 2))</p>"), ["(a + (b * 2))"]);
        assert_eq!(commands("<button @onclick=\"Salvar\" @bind-Value='valor'>"), ["Salvar", "valor"]);
        assert_eq!(commands("<button @onclick=\"@(() => Dizer(\"oi\"))\">"), ["(() => Dizer(\"oi\"))"]);
        assert_eq!(commands("<a href=\"@Url.Action(\"X\")\">@await Html.PartialAsync(\"_Menu\")</a>"), [
            "Url.Action(\"X\")",
            "await Html.PartialAsync(\"_Menu\")"
        ]);
    }

    #[test]
    fn an_address_an_escaped_marker_and_a_keyword_are_not_expressions() {
        assert_eq!(commands("<p>ajuda@loja.com, @@loja, @page, @if (x) { }, @1, @functions sem chave</p>"), Vec::<String>::new());
        assert_eq!(FULL.code_of("<p>ajuda@loja.com</p>\n", "V.cshtml", &[]).trim(), "class V {}", "only the empty type of the file");
    }

    #[test]
    fn expressions_live_in_the_method_with_the_body_blocks_and_a_member_between_them_splits_it() {
        let src = "@{ var a = 1; }\n<p>@a</p>\n@functions { int b; }\n<p>@Model.Total</p>\n";
        assert_eq!(
            lines(&FULL.code_of(src, "V.cshtml", &[])),
            ["class V { void Draw() { var a = 1;", "_ = a;}", "int b;", "void Draw() {_ = Model.Total;}}"]
        );
    }

    #[test]
    fn a_markup_comment_becomes_a_code_comment_and_nothing_in_it_is_code() {
        let src = "@* Mostra o total *@\n<p>@Model.Total</p>\n@* <p>@Esconder()</p>\n@functions { int c; } */ *@\n";
        let code = FULL.code_of(src, "V.cshtml", &[]);
        assert_eq!(
            lines(&code),
            ["/* Mostra o total */", "class V { void Draw() {_ = Model.Total;}}", "/* <p>@Esconder()</p>", "@functions { int c; } */"]
        );
        assert_eq!(code.matches("*/").count(), 2, "the closing text inside the comment is blank: {code:?}");
        assert!(FULL.code_of("<p>a</p>@* sem fim\n@Abrir()\n", "V.cshtml", &[]).contains("/* sem fim\n@Abrir()\n*/"));
    }

    #[test]
    fn the_base_lines_become_the_bases_of_the_type_the_inherited_first() {
        let src = "@implements IDisposable\n@inherits Base<Pedido>\n@implements IFechavel\n<p>@Model</p>\n";
        let code = FULL.code_of(src, "Tela.cshtml", &[]);
        assert_eq!(lines(&code)[0], "class Tela : Base<Pedido>, IDisposable, IFechavel {");
        assert_eq!(lines(&FULL.code_of("@inherits Base\n<p>oi</p>\n", "So.cshtml", &[])), ["class So : Base { }", ""], "a base alone opens the type");
    }

    /// A página com os comandos de controle, a marcação no meio do código,
    /// as tags de componente e as linhas que valem na pasta.
    const CONTROLS: Markup = Markup {
        blocks: &["@code"],
        bodies: &["@"],
        method: ["void Draw() {", "}"],
        lines: &[("@inject", "{};")],
        head: &[("@using", "using {};"), ("@namespace", "namespace {};")],
        open: "class {file}{bases} {",
        expression: ["@", "_ = {};"],
        keywords: &["if", "foreach", "code", "inject", "inherits", "namespace"],
        comment: ["@*", "*@", "/*", "*/"],
        bases: &["@inherits"],
        base_list: [" : ", ", "],
        imports_file: "_Imports",
        folder: &["@inject", "@inherits", "@namespace"],
        folder_path: ["@namespace", "."],
        controls: &["if", "foreach"],
        chains: &["else"],
        component: "_ = typeof({});",
        code_markup: ["<", "@:"],
        ..PLAIN
    };

    #[test]
    fn a_control_is_code_of_the_method_with_its_header_and_the_markup_inside_it_is_blank() {
        let src = "@if (a > 0) {\n    <p>@b</p>\n} else if (c) {\n    var d = 1;\n} else {\n    <p>nada</p>\n}\n<p>@foreach sem chave</p>\n";
        assert_eq!(
            lines(&CONTROLS.code_of(src, "V.razor", &[])),
            ["class V {void Draw() { if (a > 0) {", "_ = b;", "} else if (c) {", "var d = 1;", "} else {", "", "}}}", ""]
        );
    }

    /// O registro dos comandos com o `try`, as cláusulas dele e a condição
    /// que uma delas abre.
    const TRIES: Markup = Markup {
        controls: &["if", "foreach", "try"],
        chains: &["else", "catch", "finally"],
        filters: &["when"],
        ..CONTROLS
    };

    #[test]
    fn a_filter_after_the_header_of_a_chain_is_code_of_the_same_part() {
        let src = "@try {\n    a();\n} catch (E e) when (b(e)) {\n    <p>@c</p>\n} catch when (g() == \")\") {\n    h();\n} finally {\n    d();\n}\n<p>@f</p>\n";
        assert_eq!(
            lines(&TRIES.code_of(src, "V.razor", &[])),
            [
                "class V {void Draw() { try {",
                "a();",
                "} catch (E e) when (b(e)) {",
                "_ = c;",
                "} catch when (g() == \")\") {",
                "h();",
                "} finally {",
                "d();",
                "}",
                "_ = f;}}"
            ]
        );
    }

    #[test]
    fn a_name_that_is_not_a_filter_does_not_continue_the_part() {
        let src = "@try {\n    a();\n} catch (E e) quando (b(e)) {\n    <p>@c</p>\n}\n<p>@f</p>\n";
        let code = TRIES.code_of(src, "V.razor", &[]);
        assert!(!code.contains("catch") && !code.contains("quando"), "the part that does not close is not code: {code}");
        let without = Markup { filters: &[], ..TRIES }.code_of(
            "@try {\n    a();\n} catch (E e) when (b(e)) {\n    d();\n}\n<p>@f</p>\n",
            "V.razor",
            &[],
        );
        assert!(!without.contains("when (b(e))"), "without the registered filter the part stops before it: {without}");
    }

    #[test]
    fn the_markup_inside_a_body_block_is_blank_and_its_code_stays() {
        let src = "@{\n    var a = 1;\n    <div><div>Don't { Salvar(); }</div></div>\n    <br />\n    @:texto Salvar() @a\n    \
                   if (a > 0) { <b>@(a + 1)</b> }\n    a++;\n}\n<p>fim}</p>\n";
        assert_eq!(
            lines(&CONTROLS.code_of(src, "V.razor", &[])),
            ["class V { void Draw() {", "var a = 1;", "", "", "_ = a;", "if (a > 0) { _ = (a + 1); }", "a++;", "}}", ""]
        );
    }

    #[test]
    fn a_component_tag_is_a_use_of_the_type_it_names() {
        let src = "<Contador Valor=\"@x\" />\n<Loja.Titulo>oi</Loja.Titulo>\n<p>@y</p>\n";
        let code = CONTROLS.code_of(src, "V.razor", &[]);
        let commands: Vec<&str> = code.split("_ = ").skip(1).map(|rest| &rest[..rest.find(';').unwrap()]).collect();
        assert_eq!(commands, ["typeof(Contador)", "x", "typeof(Loja.Titulo)", "y"]);
    }

    #[test]
    fn the_folder_lines_of_the_imports_files_above_enter_the_type_of_the_page() {
        let folder = [
            ("Web/_Imports.razor", "@namespace Loja\n@inject Carrinho Compras\n@inherits Base\n@using Loja.Servicos\n"),
            ("Web/Pages/_Imports.razor", "@inherits Outra\n@inject Relogio Hora\n@inject Carrinho Compras\n"),
        ];
        let page = CONTROLS.code_of("<p>@Compras</p>\n", "Web/Pages/Admin/Painel.razor", &folder);
        assert_eq!(lines(&page)[0], "namespace Loja.Pages.Admin; class Painel : Outra { Carrinho Compras; Relogio Hora; void Draw() {_ = Compras;}}");
        let own = CONTROLS.code_of("@namespace Meu\n@inherits Minha\n<p>@Hora</p>\n", "Web/Pages/Admin/Painel.razor", &folder);
        assert_eq!(lines(&own), ["namespace Meu;", "class Painel : Minha { Carrinho Compras; Relogio Hora;", "void Draw() {_ = Hora;}}"]);
        assert_eq!(
            lines(&CONTROLS.code_of("<p>oi</p>\n", "Web/Pages/Estatica.razor", &folder)),
            ["namespace Loja.Pages; class Estatica : Outra { Carrinho Compras; Relogio Hora;}"],
            "the page without code gets the folder lines in its empty type"
        );
    }

    #[test]
    fn only_the_imports_file_of_the_registry_is_the_folders() {
        assert!(FULL.is_imports_file("Web/Pages/_ViewImports.cshtml"));
        assert!(!FULL.is_imports_file("Web/Pages/_ViewStart.cshtml"));
        assert!(!PAGE.is_imports_file("Web/Pages/_ViewImports.cshtml"));
    }
}
