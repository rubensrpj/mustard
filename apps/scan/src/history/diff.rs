//! A saída do `git log -p -U0` do projeto inteiro, lida aos poucos: de cada
//! commit, o cabeçalho e, de cada arquivo que ele mexeu, os trechos com as
//! linhas tiradas e postas.
//!
//! Da linha só interessa o texto sem os espaços das pontas, que a leitura
//! resume num número: é por ele que a linha de um commit se reconhece no
//! commit seguinte. Nenhum texto de código fica guardado.
//!
//! O commit de junção traz o diff combinado (`--cc`): só as linhas que
//! nasceram nele, as que nenhum dos pais tinha, contam; as outras já vieram
//! pelos commits dos ramos.

use std::io::BufRead;

use crate::refresh::unquote;

/// Uma linha tirada ou posta: o resumo do texto sem os espaços das pontas, o
/// resumo do texto sem espaço nenhum, e se ela não diz nada por si só.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Line {
    pub hash: u64,
    pub squeezed: u64,
    pub trivial: bool,
}

/// Um trecho de um arquivo: onde começa do lado antigo, as linhas tiradas e as
/// postas, na ordem em que aparecem.
#[derive(Debug, Default)]
pub(super) struct Hunk {
    /// A linha do arquivo antigo (a partir de 1) onde as tiradas começam; sem
    /// nenhuma tirada, a linha depois da qual as postas entram.
    pub old_start: usize,
    pub removed: Vec<Line>,
    pub added: Vec<Line>,
}

/// O que um commit fez a um arquivo. O caminho antigo falta no arquivo que
/// nasceu, e o novo no que morreu. No commit de junção, `born` traz as linhas
/// que só ele tem, cada uma com a posição (a partir de 0) onde entra no
/// arquivo que a junção deixou.
#[derive(Debug, Default)]
pub(super) struct FilePatch {
    pub old: Option<String>,
    pub new: Option<String>,
    pub hunks: Vec<Hunk>,
    pub combined: bool,
    pub born: Vec<(usize, Line)>,
}

/// Um commit lido: o hash inteiro, a data, o título e o que ele fez a cada
/// arquivo.
#[derive(Debug, Default)]
pub(super) struct CommitPatch {
    pub sha: String,
    pub at: i64,
    pub title: String,
    pub files: Vec<FilePatch>,
}

/// O leitor da saída do git: entrega um commit por vez.
pub(super) struct Patches<R: BufRead> {
    input: R,
    /// A linha lida que já pertence ao commit seguinte.
    held: Option<Vec<u8>>,
    buffer: Vec<u8>,
}

impl<R: BufRead> Patches<R> {
    pub(super) fn new(input: R) -> Self {
        Patches { input, held: None, buffer: Vec::new() }
    }

    /// A próxima linha, sem a quebra no fim; `None` no fim da entrada.
    fn line(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        if let Some(line) = self.held.take() {
            return Ok(Some(line));
        }
        self.buffer.clear();
        if self.input.read_until(b'\n', &mut self.buffer)? == 0 {
            return Ok(None);
        }
        if self.buffer.last() == Some(&b'\n') {
            self.buffer.pop();
        }
        Ok(Some(std::mem::take(&mut self.buffer)))
    }

    /// O próximo commit da saída, `None` quando ela acabou.
    ///
    /// # Errors
    ///
    /// Quando a leitura da saída do git falha.
    pub(super) fn next_commit(&mut self) -> std::io::Result<Option<CommitPatch>> {
        let mut commit = loop {
            let Some(line) = self.line()? else { return Ok(None) };
            if let Some(commit) = line.strip_prefix(&[0u8]).and_then(header) {
                break commit;
            }
        };
        let mut file: Option<FilePatch> = None;
        while let Some(line) = self.line()? {
            match line.first() {
                Some(0) => {
                    self.held = Some(line);
                    break;
                }
                Some(b'd') if line.starts_with(b"diff --git ") => {
                    commit.files.extend(file.take());
                    file = Some(plain_file(&String::from_utf8_lossy(&line[11..])));
                }
                Some(b'd') if line.starts_with(b"diff --cc ") || line.starts_with(b"diff --combined ") => {
                    commit.files.extend(file.take());
                    let path = line.splitn(3, |&b| b == b' ').nth(2).unwrap_or(&[]);
                    file = Some(combined_file(&String::from_utf8_lossy(path)));
                }
                Some(b'@') => {
                    let Some(patch) = file.as_mut() else { continue };
                    self.hunk(&line, patch)?;
                }
                _ => {
                    if let Some(patch) = file.as_mut() {
                        extended_header(&line, patch);
                    }
                }
            }
        }
        commit.files.extend(file.take());
        Ok(Some(commit))
    }

    /// Lê o trecho que `head` abre, para dentro de `patch`.
    fn hunk(&mut self, head: &[u8], patch: &mut FilePatch) -> std::io::Result<()> {
        let head = String::from_utf8_lossy(head);
        if patch.combined {
            let Some(result) = head.split_whitespace().find(|part| part.starts_with('+')) else { return Ok(()) };
            let columns = head.bytes().take_while(|&b| b == b'@').count().saturating_sub(1).max(1);
            let mut at = range_of(result.trim_start_matches('+')).0.saturating_sub(1);
            while let Some(line) = self.line()? {
                let marks = line.get(..columns).unwrap_or(&[]);
                if marks.len() < columns || !marks.iter().all(|m| matches!(m, b' ' | b'+' | b'-')) {
                    self.held = Some(line);
                    break;
                }
                if marks.contains(&b'-') {
                    continue;
                }
                if marks.iter().all(|&m| m == b'+') {
                    patch.born.push((at, summary(&line[columns..])));
                }
                at += 1;
            }
            return Ok(());
        }
        let mut ranges = head.split_whitespace().skip(1);
        let (old_start, old_count) = range_of(ranges.next().unwrap_or("").trim_start_matches('-'));
        let (_, new_count) = range_of(ranges.next().unwrap_or("").trim_start_matches('+'));
        let mut hunk = Hunk { old_start: if old_count == 0 { old_start } else { old_start.max(1) }, ..Hunk::default() };
        let (mut left_removed, mut left_added) = (old_count, new_count);
        while left_removed > 0 || left_added > 0 {
            let Some(line) = self.line()? else { break };
            match line.first() {
                Some(b'-') if left_removed > 0 => {
                    hunk.removed.push(summary(&line[1..]));
                    left_removed -= 1;
                }
                Some(b'+') if left_added > 0 => {
                    hunk.added.push(summary(&line[1..]));
                    left_added -= 1;
                }
                Some(b'\\') => {}
                _ => {
                    self.held = Some(line);
                    break;
                }
            }
        }
        patch.hunks.push(hunk);
        Ok(())
    }
}

/// O cabeçalho de um commit, sem o byte nulo que o abre: o hash, a data e os
/// pais (que aqui não interessam), um separador e o título.
fn header(line: &[u8]) -> Option<CommitPatch> {
    let line = String::from_utf8_lossy(line);
    let (ids, title) = line.split_once('\x1f').unwrap_or((&line, ""));
    let mut ids = ids.split_whitespace();
    let sha = ids.next()?.to_string();
    let at = ids.next()?.parse().unwrap_or(0);
    Some(CommitPatch { sha, at, title: title.trim().to_string(), files: Vec::new() })
}

/// O arquivo de um `diff --git a/velho b/novo`, com os caminhos que a linha dá;
/// as linhas seguintes os corrigem quando o git os diz melhor.
fn plain_file(rest: &str) -> FilePatch {
    let mut patch = FilePatch::default();
    if let Some((old, new)) = rest.split_once(" b/") {
        patch.old = Some(unquote(old.strip_prefix("a/").unwrap_or(old)));
        patch.new = Some(unquote(new));
    }
    patch
}

/// O arquivo de um `diff --cc caminho`: o mesmo dos dois lados.
fn combined_file(path: &str) -> FilePatch {
    let path = unquote(path.trim());
    FilePatch { old: Some(path.clone()), new: Some(path), combined: true, ..FilePatch::default() }
}

/// As linhas entre o `diff --git` e o primeiro trecho: as renomeações, o
/// arquivo que nasceu ou morreu e os caminhos dos dois lados.
fn extended_header(line: &[u8], patch: &mut FilePatch) {
    let line = String::from_utf8_lossy(line);
    if let Some(path) = line.strip_prefix("rename from ") {
        patch.old = Some(unquote(path));
    } else if let Some(path) = line.strip_prefix("rename to ") {
        patch.new = Some(unquote(path));
    } else if line.starts_with("new file mode") {
        patch.old = None;
    } else if line.starts_with("deleted file mode") {
        patch.new = None;
    } else if let Some(path) = line.strip_prefix("--- ") {
        patch.old = side_path(path, "a/");
    } else if let Some(path) = line.strip_prefix("+++ ") {
        patch.new = side_path(path, "b/");
    }
}

/// O caminho de um lado do diff, sem o prefixo; `None` quando o lado não
/// existe.
fn side_path(raw: &str, prefix: &str) -> Option<String> {
    let path = unquote(raw.trim_end_matches('\t'));
    if path == "/dev/null" {
        return None;
    }
    Some(path.strip_prefix(prefix).unwrap_or(&path).to_string())
}

/// A linha e a quantidade de um lado de um trecho, `12,3` ou `12`.
fn range_of(range: &str) -> (usize, usize) {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    (start.parse().unwrap_or(0), count.parse().unwrap_or(1))
}

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | 0x0b | 0x0c)
}

/// O resumo de `text`: sem os espaços das pontas, sem todos os espaços e se a
/// linha não diz nada.
pub(super) fn summary(text: &[u8]) -> Line {
    let start = text.iter().position(|&b| !is_space(b)).unwrap_or(text.len());
    let end = text.iter().rposition(|&b| !is_space(b)).map_or(start, |at| at + 1);
    let trimmed = &text[start..end];
    let (mut hash, mut squeezed) = (OFFSET, OFFSET);
    for &byte in trimmed {
        hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
        if !is_space(byte) {
            squeezed = (squeezed ^ u64::from(byte)).wrapping_mul(PRIME);
        }
    }
    Line { hash, squeezed, trivial: is_trivial(trimmed) }
}

/// O resumo do texto de uma linha do arquivo como a versão da ponta a tem.
pub(super) fn hash_of(text: &str) -> u64 {
    summary(text.as_bytes()).hash
}

/// As linhas que só uma palavra curta da linguagem escreve: sem letra nem
/// número, ou só `else`, `end`, `return` e as parecidas. Elas repetem-se por
/// todo o arquivo e por isso não dizem de que declaração vêm.
fn is_trivial(trimmed: &[u8]) -> bool {
    const WORDS: [&str; 22] = [
        "else", "end", "return", "break", "continue", "try", "finally", "do", "then", "fi", "done", "esac", "default", "begin",
        "pass", "endif", "none", "null", "nil", "true", "false", "ok",
    ];
    let mut word = [0u8; 8];
    let mut length = 0;
    for &byte in trimmed {
        if byte.is_ascii_alphanumeric() || byte >= 0x80 {
            if length == word.len() {
                return false;
            }
            word[length] = byte.to_ascii_lowercase();
            length += 1;
        }
    }
    length == 0 || WORDS.iter().any(|known| known.as_bytes() == &word[..length])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Vec<CommitPatch> {
        let mut patches = Patches::new(text.as_bytes());
        let mut all = Vec::new();
        while let Some(commit) = patches.next_commit().unwrap() {
            all.push(commit);
        }
        all
    }

    #[test]
    fn a_line_is_summed_up_without_its_edge_spaces_and_the_trivial_ones_are_told() {
        assert_eq!(summary(b"  let x = 1;\t").hash, summary(b"let x = 1;").hash);
        assert_ne!(summary(b"let x = 1;").hash, summary(b"let x  =  1;").hash, "the spaces inside count");
        assert_eq!(summary(b"let x = 1;").squeezed, summary(b"let  x=1;").squeezed, "without any space they are the same");
        for trivial in ["}", "  });", "", "else", "} else {", "return;", "Ok(())", "break;"] {
            assert!(summary(trivial.as_bytes()).trivial, "{trivial:?} says nothing by itself");
        }
        for meaningful in ["let x = 1;", "return value;", "fn a() {", "x + 2", "pub fn alpha() {}"] {
            assert!(!summary(meaningful.as_bytes()).trivial, "{meaningful:?} says something");
        }
    }

    #[test]
    fn the_commits_the_files_and_the_hunks_of_a_log_come_one_by_one() {
        let text = "\0aaaa 10 \x1fcria\n\ndiff --git a/src/a.rs b/src/a.rs\nnew file mode 100644\nindex 000..111\n--- /dev/null\n+++ b/src/a.rs\n@@ -0,0 +1,2 @@\n+um\n+dois\n\
                    \0bbbb 20 aaaa\x1fmuda (#3)\n\ndiff --git a/src/a.rs b/src/a.rs\nindex 111..222 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -2 +2 @@\n-dois\n+DOIS\n\\ No newline at end of file\n\
                    diff --git a/src/b.rs b/src/c.rs\nsimilarity index 100%\nrename from src/b.rs\nrename to src/c.rs\n";
        let commits = read(text);
        assert_eq!(commits.len(), 2);
        let (first, second) = (&commits[0], &commits[1]);
        assert_eq!((first.sha.as_str(), first.at, first.title.as_str()), ("aaaa", 10, "cria"));
        let created = &first.files[0];
        assert_eq!((created.old.as_deref(), created.new.as_deref()), (None, Some("src/a.rs")));
        assert_eq!((created.hunks[0].old_start, created.hunks[0].removed.len(), created.hunks[0].added.len()), (0, 0, 2));

        assert_eq!(second.title, "muda (#3)");
        let changed = &second.files[0];
        assert_eq!((changed.old.as_deref(), changed.new.as_deref()), (Some("src/a.rs"), Some("src/a.rs")));
        assert_eq!(changed.hunks[0].old_start, 2);
        assert_eq!(changed.hunks[0].removed, vec![summary(b"dois")]);
        assert_eq!(changed.hunks[0].added, vec![summary(b"DOIS")]);
        let renamed = &second.files[1];
        assert_eq!((renamed.old.as_deref(), renamed.new.as_deref()), (Some("src/b.rs"), Some("src/c.rs")));
        assert!(renamed.hunks.is_empty());
    }

    #[test]
    fn a_removed_line_that_starts_with_dashes_is_a_line_and_not_the_header_of_a_file() {
        let text = "\0aaaa 10 \x1ft\n\ndiff --git a/x.rs b/x.rs\nindex 1..2 100644\n--- a/x.rs\n+++ b/x.rs\n@@ -3,2 +3,1 @@\n--- a/y\n-b\n+c\n";
        let commits = read(text);
        let hunk = &commits[0].files[0].hunks[0];
        assert_eq!(hunk.removed, vec![summary(b"-- a/y"), summary(b"b")]);
        assert_eq!(hunk.added, vec![summary(b"c")]);
    }

    #[test]
    fn a_merge_keeps_only_the_lines_no_parent_had() {
        let text = "\0mmmm 30 aaaa bbbb\x1fjunta\n\ndiff --cc src/a.rs\nindex 111,222..333\n--- a/src/a.rs\n+++ b/src/a.rs\n\
                    @@@ -5,0 -7,3 +7,4 @@@ contexto\n +da direita\n++nasceu na junção\n- tirada\n++outra\n";
        let commits = read(text);
        let merge = &commits[0];
        let file = &merge.files[0];
        assert!(file.combined);
        assert_eq!(file.new.as_deref(), Some("src/a.rs"));
        assert_eq!(
            file.born,
            vec![(7, summary("nasceu na junção".as_bytes())), (8, summary(b"outra"))],
            "only the lines added against both parents, at the place they take in the result"
        );
    }
}
