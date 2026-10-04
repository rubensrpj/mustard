//! `terminal_read` — o comando do terminal que só mostra linhas de um arquivo.
//!
//! O aviso de releitura do agente de onda ([`crate::hooks::session::reread`])
//! trata a leitura pelo terminal como a da ferramenta `Read`: o arquivo e a
//! faixa de linhas. Esta leitura diz qual é o arquivo e qual é a faixa quando
//! o comando é só um destes, sozinho na linha (um `cd` antes dele conta, e um
//! redirecionamento para `/dev/null` também):
//!
//! - `cat [-n] <arquivo>` e `nl [-ba] <arquivo>`: o arquivo inteiro;
//! - `head [-n N | -N] <arquivo>`: as primeiras N linhas (10 sem número);
//! - `tail [-n N | -N] <arquivo>`: as últimas N linhas (10 sem número), e
//!   `tail -n +K <arquivo>`: da linha K até o fim;
//! - `sed -n 'A,Bp' <arquivo>`, `sed -n 'Ap' <arquivo>` e
//!   `sed -n 'A,$p' <arquivo>`: as linhas pedidas.
//!
//! Qualquer outro comando, programa, opção, curinga, variável, pipe ou segundo
//! comando na linha não é uma leitura deste tipo: a conferência não opina.

use std::path::{Path, PathBuf};

use super::lex::{self, Segment, Word};

/// A faixa de linhas que o comando mostra, contadas de 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Span {
    /// Da linha `from` à `to`, as duas inclusas; sem `to`, até o fim.
    Lines { from: u64, to: Option<u64> },
    /// As últimas `n` linhas.
    Last(u64),
}

/// A leitura que o comando faz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalRead {
    /// O arquivo como o comando o escreveu, para dizer ao agente.
    pub(crate) typed: String,
    /// O caminho do arquivo, partindo da pasta em que o comando roda.
    pub(crate) file: PathBuf,
    /// As linhas que o comando mostra.
    pub(crate) span: Span,
}

/// A leitura de arquivo que `cmd` faz, rodando em `cwd`; `None` quando o
/// comando é outra coisa.
pub(crate) fn terminal_read(cmd: &str, cwd: &Path) -> Option<TerminalRead> {
    let segments = lex::segments(cmd);
    let mut dir = cwd.to_path_buf();
    let mut reader: Option<(&Segment, PathBuf)> = None;
    for segment in &segments {
        if segment.name() == "cd" {
            if reader.is_none() {
                dir = dir.join(path_operand(&segment.args)?);
            }
            continue;
        }
        if reader.is_some() {
            return None;
        }
        reader = Some((segment, dir.clone()));
    }
    let (segment, dir) = reader?;
    let quiet = segment.redirects.iter().all(|redirect| redirect.target.text == "/dev/null");
    if !segment.leading.is_empty() || !quiet {
        return None;
    }
    let (file, span) = match segment.name() {
        "cat" => (lone_file(&segment.args, &["-n"])?, Span::Lines { from: 1, to: None }),
        "nl" => (lone_file(&segment.args, &["-ba"])?, Span::Lines { from: 1, to: None }),
        "head" => {
            let (count, file) = counted(&segment.args)?;
            let count = count.map_or(Some(10), |text| number(&text))?;
            (file, Span::Lines { from: 1, to: Some(count) })
        }
        "tail" => {
            let (count, file) = counted(&segment.args)?;
            let span = match count.as_deref() {
                None => Span::Last(10),
                Some(text) => match text.strip_prefix('+') {
                    Some(from) => Span::Lines { from: number(from)?, to: None },
                    None => Span::Last(number(text)?),
                },
            };
            (file, span)
        }
        "sed" => sed(&segment.args)?,
        _ => return None,
    };
    let typed = path_text(file)?.to_string();
    Some(TerminalRead { file: dir.join(&typed), typed, span })
}

/// O caminho que a palavra escreve, quando o terminal o entrega como está:
/// sem variável, substituição, curinga, til nem opção.
fn path_text(word: &Word) -> Option<&str> {
    let plain = !word.text.is_empty()
        && !word.text.starts_with('-')
        && !word.raw.contains(['$', '`', '*', '?', '[', '{', '(', '~']);
    plain.then_some(word.text.as_str())
}

/// O único argumento, que é um caminho como [`path_text`] o aceita.
fn path_operand(args: &[Word]) -> Option<&str> {
    match args {
        [only] => path_text(only),
        _ => None,
    }
}

/// Um número inteiro maior que zero.
fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|count| *count > 0)
}

/// O arquivo, quando `args` é só ele e as opções em `flags`; `None` com
/// qualquer outra opção ou com mais de um arquivo.
fn lone_file<'a>(args: &'a [Word], flags: &[&str]) -> Option<&'a Word> {
    let mut file = None;
    for word in args {
        if word.text.starts_with('-') {
            flags.contains(&word.text.as_str()).then_some(())?;
        } else if file.replace(word).is_some() {
            return None;
        }
    }
    file
}

/// O número de linhas e o arquivo de um `head` ou `tail`: `-n N`, `-nN` ou
/// `-N`. O número vem como foi escrito (o `tail` aceita `+K`); sem ele, `None`.
fn counted(args: &[Word]) -> Option<(Option<String>, &Word)> {
    let mut count: Option<String> = None;
    let mut file: Option<&Word> = None;
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let text = word.text.as_str();
        let given = if text == "-n" {
            Some(words.next()?.text.clone())
        } else if let Some(rest) = text.strip_prefix("-n") {
            Some(rest.to_string())
        } else if let Some(rest) = text.strip_prefix('-') {
            if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some(rest.to_string())
        } else {
            None
        };
        if let Some(given) = given {
            if count.replace(given).is_some() {
                return None;
            }
        } else if file.replace(word).is_some() {
            return None;
        }
    }
    Some((count, file?))
}

/// O arquivo e as linhas de um `sed -n '<script>' <arquivo>`, com o script
/// `Ap`, `A,Bp` ou `A,$p`.
fn sed(args: &[Word]) -> Option<(&Word, Span)> {
    let mut quiet = false;
    let mut operands = Vec::new();
    for word in args {
        match word.text.as_str() {
            "-n" => quiet = true,
            text if text.starts_with('-') => return None,
            _ => operands.push(word),
        }
    }
    let [script, file] = operands[..] else { return None };
    if !quiet {
        return None;
    }
    let body = script.text.strip_suffix('p')?;
    let (from, to) = match body.split_once(',') {
        Some((from, "$")) => (number(from)?, None),
        Some((from, to)) => (number(from)?, Some(number(to)?)),
        None => (number(body)?, Some(number(body)?)),
    };
    if to.is_some_and(|to| to < from) {
        return None;
    }
    Some((file, Span::Lines { from, to }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(cmd: &str) -> Option<(String, Span)> {
        terminal_read(cmd, Path::new("/w")).map(|read| (read.file.to_string_lossy().into_owned(), read.span))
    }

    fn lines(from: u64, to: Option<u64>) -> Span {
        Span::Lines { from, to }
    }

    /// Cada leitura que o aviso entende dá o arquivo, partindo da pasta do
    /// comando, e as linhas certas.
    #[test]
    fn each_reader_gives_its_file_and_its_lines() {
        let cases = [
            ("cat a.rs", lines(1, None)),
            ("cat -n a.rs", lines(1, None)),
            ("nl -ba a.rs", lines(1, None)),
            ("head a.rs", lines(1, Some(10))),
            ("head -n 20 a.rs", lines(1, Some(20))),
            ("head -n20 a.rs", lines(1, Some(20))),
            ("head -5 a.rs", lines(1, Some(5))),
            ("tail a.rs", Span::Last(10)),
            ("tail -n 30 a.rs", Span::Last(30)),
            ("tail -7 a.rs", Span::Last(7)),
            ("tail -n +40 a.rs", lines(40, None)),
            ("sed -n '10,20p' a.rs", lines(10, Some(20))),
            ("sed -n 5p a.rs", lines(5, Some(5))),
            ("sed -n '30,$p' a.rs", lines(30, None)),
            ("rtk cat a.rs", lines(1, None)),
            ("cat a.rs 2>/dev/null", lines(1, None)),
        ];
        for (cmd, span) in cases {
            assert_eq!(read(cmd), Some(("/w/a.rs".to_string(), span)), "{cmd}");
        }
    }

    /// O `cd` antes do comando muda a pasta de onde o arquivo parte; o
    /// caminho absoluto fica como veio.
    #[test]
    fn a_cd_before_the_reader_moves_the_folder() {
        assert_eq!(read("cd src && cat a.rs").map(|read| read.0), Some("/w/src/a.rs".to_string()));
        assert_eq!(read("cd /x; cat /y/a.rs").map(|read| read.0), Some("/y/a.rs".to_string()));
        assert_eq!(read("cd $HOME && cat a.rs"), None);
    }

    /// O que não é só uma leitura de um arquivo, com as linhas que ela mostra,
    /// não é lido: pipe, segundo comando, opção desconhecida, curinga,
    /// variável, redirecionamento e `sed` sem `-n` ou com script que não é
    /// de faixa.
    #[test]
    fn anything_else_is_not_a_read() {
        for cmd in [
            "cat a.rs | head -5",
            "cat a.rs && cat b.rs",
            "cat a.rs b.rs",
            "cat -A a.rs",
            "cat *.rs",
            "cat $FILE",
            "cat a.rs > out.txt",
            "cat < a.rs",
            "cat",
            "head -c 100 a.rs",
            "head -n -5 a.rs",
            "tail -f a.rs",
            "sed '10,20p' a.rs",
            "sed -n '/x/p' a.rs",
            "sed -n '20,10p' a.rs",
            "sed -i 's/a/b/' a.rs",
            "less a.rs",
            "grep x a.rs",
            "if true; then cat a.rs; fi",
        ] {
            assert_eq!(read(cmd), None, "{cmd}");
        }
    }
}
