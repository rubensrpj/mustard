//! Reading a Bash `command` string, shared by the Bash gate family.
//!
//! Two faces. [`segments`] reads the command the way the terminal does —
//! words, quotes, separators, redirects, substitutions, heredocs, comments —
//! and hands back each simple command it would run. The older text helpers
//! below it (`split_after`, `mask_quoted_operators`, `strip_leading_rtk`, …)
//! still serve the gates that scan the raw text. No verdicts are produced here.

use mustard_core::domain::text::{Boundaries, WordChars};

/// How deep the reader descends into `$(…)`, backticks and `bash -c "…"`.
/// Past this depth the rest of the substitution is kept as plain text.
const MAX_DEPTH: usize = 16;

/// Words that open or close a compound command. Written plain at the head of
/// a command they are skipped, so the real command behind them is the program
/// (`if x; then rm -rf y; fi` runs `rm`).
const STRUCTURE_WORDS: &[&str] =
    &["!", "{", "}", "if", "then", "else", "elif", "fi", "do", "done", "while", "until"];

/// The redirect operators, longest first so `>>` is never read as `>`.
const REDIRECT_OPERATORS: &[&str] =
    &["&>>", "&>", "<<<", "<<-", "<<", "<>", "<&", "<", ">>", ">&", ">|", ">"];

/// One word of a command: `text` is what the program receives (no quotes, no
/// escaping backslash); `raw` is the word as it was written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Word {
    pub text: String,
    pub raw: String,
}

impl Word {
    /// `true` when the word was written with no quote and no escape, so what
    /// was typed is what the shell sees. Only such a word is a keyword.
    fn is_plain(&self) -> bool {
        self.text == self.raw
    }

    /// The word as written, without one layer of surrounding quotes. A path
    /// check reads this one: outside quotes the shell eats the backslashes, so
    /// the `text` of `C:\Atiz\x` is `C:Atizx`.
    pub fn raw_unquoted(&self) -> &str {
        let raw = self.raw.as_str();
        for quote in ['"', '\''] {
            if let Some(rest) = raw.strip_prefix(quote) {
                return rest.strip_suffix(quote).unwrap_or(rest);
            }
        }
        raw
    }
}

/// One redirect (`>`, `>>`, `2>`, `&>`, `<`, `>&`, `<<`, `<<<`, …) and its
/// target. For a heredoc the target is the delimiter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Redirect {
    pub op: String,
    pub target: Word,
}

/// One simple command of the line, split the way the terminal splits it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Segment {
    /// The program, after the `NAME=value` assignments, the structure words
    /// and the wrappers (`rtk`, `sudo`, `env`, `command`, `exec`, `nohup`,
    /// `time`, `nice`, `timeout`, `xargs`). Empty when the command only
    /// assigns or only redirects.
    pub program: Word,
    /// The arguments, in order, without the redirects.
    pub args: Vec<Word>,
    /// The redirects of the command, kept apart from the arguments.
    pub redirects: Vec<Redirect>,
}

impl Segment {
    /// The program's name without its folder: `/bin/rm` is `rm`.
    pub fn name(&self) -> &str {
        base_name(&self.program.text)
    }
}

/// The commands the terminal would run for `cmd`, in the order it meets them.
///
/// Quoted text is never a command, with the two exceptions the terminal
/// itself makes: what sits inside `$(…)` or backticks (also inside double
/// quotes), and the line handed to `bash -c`, `sh -c`, `zsh -c`, `dash -c` or
/// `eval`. Those commands come right after the command that holds them. A
/// heredoc body and a comment are never commands. The reader walks the text
/// once, never panics, and stops descending past [`MAX_DEPTH`] levels.
pub(super) fn segments(cmd: &str) -> Vec<Segment> {
    read_line(cmd, 0)
}

fn read_line(cmd: &str, depth: usize) -> Vec<Segment> {
    let mut reader = Reader { chars: cmd.chars().collect(), pos: 0, heredocs: Vec::new() };
    reader.list(Close::End, depth)
}

/// What ends the list being read: the end of the text, the `)` of a `$(…)`
/// or the closing backtick.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Close {
    End,
    Paren,
    Backtick,
}

/// The command being read: its words, its redirects and the commands found
/// inside it, which come right after it in the result.
#[derive(Default)]
struct Pending {
    words: Vec<Word>,
    redirects: Vec<Redirect>,
    inner: Vec<Segment>,
}

struct Reader {
    chars: Vec<char>,
    pos: usize,
    /// Heredocs opened on the current line, waiting for its end: the
    /// delimiter and whether leading tabs are dropped (`<<-`).
    heredocs: Vec<(String, bool)>,
}

impl Reader {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.chars.len());
    }

    fn spelled_since(&self, start: usize) -> String {
        self.chars.get(start..self.pos).unwrap_or(&[]).iter().collect()
    }

    /// Consume `token` when the text continues with it.
    fn eat(&mut self, token: &str) -> bool {
        let matches = token.chars().enumerate().all(|(k, c)| self.peek(k) == Some(c));
        if matches {
            self.advance(token.chars().count());
        }
        matches
    }

    /// Read commands until `close`, splitting on the separators that sit
    /// outside quotes.
    fn list(&mut self, close: Close, depth: usize) -> Vec<Segment> {
        let mut out = Vec::new();
        let mut cur = Pending::default();
        let mut parens = 0usize;
        while let Some(c) = self.peek(0) {
            let before = self.pos;
            match c {
                ' ' | '\t' => self.advance(1),
                '\\' if self.peek(1) == Some('\n') => self.advance(2),
                '\n' => {
                    self.advance(1);
                    finish(&mut cur, &mut out, depth);
                    self.skip_heredoc_bodies();
                }
                '\r' | ';' => {
                    self.advance(1);
                    finish(&mut cur, &mut out, depth);
                }
                '&' if self.peek(1) == Some('>') => self.redirect(&mut cur, String::new(), close, depth),
                '&' | '|' => {
                    self.advance(1);
                    if matches!(self.peek(0), Some('&' | '|')) {
                        self.advance(1);
                    }
                    finish(&mut cur, &mut out, depth);
                }
                '<' | '>' if self.peek(1) != Some('(') => {
                    self.redirect(&mut cur, String::new(), close, depth);
                }
                '(' => {
                    self.advance(1);
                    finish(&mut cur, &mut out, depth);
                    parens += 1;
                }
                ')' => {
                    self.advance(1);
                    finish(&mut cur, &mut out, depth);
                    if parens > 0 {
                        parens -= 1;
                    } else if close == Close::Paren {
                        return out;
                    }
                }
                '`' if close == Close::Backtick => {
                    self.advance(1);
                    finish(&mut cur, &mut out, depth);
                    return out;
                }
                '#' => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.advance(1);
                    }
                }
                _ => {
                    let word = self.word(&mut cur.inner, close, depth);
                    let is_fd = !word.raw.is_empty() && word.raw.bytes().all(|b| b.is_ascii_digit());
                    if is_fd && matches!(self.peek(0), Some('<' | '>')) && self.peek(1) != Some('(') {
                        self.redirect(&mut cur, word.raw, close, depth);
                    } else {
                        cur.words.push(word);
                    }
                }
            }
            if self.pos == before {
                self.advance(1);
            }
        }
        finish(&mut cur, &mut out, depth);
        out
    }

    /// Read one redirect operator (with its file-descriptor prefix `fd`) and
    /// its target word. A heredoc queues its delimiter for the end of the line.
    fn redirect(&mut self, cur: &mut Pending, fd: String, close: Close, depth: usize) {
        let Some(op) = REDIRECT_OPERATORS.iter().copied().find(|op| self.eat(op)) else {
            self.advance(1);
            return;
        };
        while matches!(self.peek(0), Some(' ' | '\t')) {
            self.advance(1);
        }
        let starts_word = self.peek(0).is_some_and(|c| !"\n\r;&|()<>".contains(c))
            || (matches!(self.peek(0), Some('<' | '>')) && self.peek(1) == Some('('));
        let target = if starts_word { self.word(&mut cur.inner, close, depth) } else { Word::default() };
        if op == "<<" || op == "<<-" {
            self.heredocs.push((target.text.clone(), op == "<<-"));
        }
        cur.redirects.push(Redirect { op: format!("{fd}{op}"), target });
    }

    /// Read one word up to the first blank or operator outside quotes. The
    /// commands met inside it (`$(…)`, backticks, `<(…)`) go to `inner`.
    fn word(&mut self, inner: &mut Vec<Segment>, close: Close, depth: usize) -> Word {
        let mut w = Word::default();
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' | '\n' | '\r' | ';' | '&' | '|' | '(' | ')' => break,
                '`' if close == Close::Backtick => break,
                '<' | '>' => {
                    if self.peek(1) != Some('(') {
                        break;
                    }
                    let start = self.pos;
                    self.advance(2);
                    self.substitution(start, &mut w, inner, Close::Paren, depth);
                }
                '\\' => match self.peek(1) {
                    Some('\n') => self.advance(2),
                    Some(next) => {
                        w.text.push(next);
                        w.raw.push('\\');
                        w.raw.push(next);
                        self.advance(2);
                    }
                    None => {
                        w.text.push('\\');
                        w.raw.push('\\');
                        self.advance(1);
                    }
                },
                '\'' => self.single_quoted(&mut w),
                '"' => self.double_quoted(&mut w, inner, depth),
                '$' => self.dollar(&mut w, inner, depth, false),
                '`' => {
                    let start = self.pos;
                    self.advance(1);
                    self.substitution(start, &mut w, inner, Close::Backtick, depth);
                }
                _ => {
                    w.text.push(c);
                    w.raw.push(c);
                    self.advance(1);
                }
            }
        }
        w
    }

    fn single_quoted(&mut self, w: &mut Word) {
        w.raw.push('\'');
        self.advance(1);
        while let Some(c) = self.peek(0) {
            self.advance(1);
            w.raw.push(c);
            if c == '\'' {
                return;
            }
            w.text.push(c);
        }
    }

    /// Double quotes keep the word whole; only `$(…)` and backticks inside
    /// them run, and the backslash escapes only `$`, the backtick, `"`, `\`
    /// and the line break.
    fn double_quoted(&mut self, w: &mut Word, inner: &mut Vec<Segment>, depth: usize) {
        w.raw.push('"');
        self.advance(1);
        while let Some(c) = self.peek(0) {
            match c {
                '"' => {
                    w.raw.push('"');
                    self.advance(1);
                    return;
                }
                '\\' => match self.peek(1) {
                    Some('\n') => self.advance(2),
                    Some(next @ ('$' | '`' | '"' | '\\')) => {
                        w.text.push(next);
                        w.raw.push('\\');
                        w.raw.push(next);
                        self.advance(2);
                    }
                    _ => {
                        w.text.push('\\');
                        w.raw.push('\\');
                        self.advance(1);
                    }
                },
                '$' => self.dollar(w, inner, depth, true),
                '`' => {
                    let start = self.pos;
                    self.advance(1);
                    self.substitution(start, w, inner, Close::Backtick, depth);
                }
                _ => {
                    w.text.push(c);
                    w.raw.push(c);
                    self.advance(1);
                }
            }
        }
    }

    /// `$(…)` is a command; `$((…))` is arithmetic and `${…}` a variable,
    /// both kept as text; `$'…'` quotes (outside double quotes only).
    fn dollar(&mut self, w: &mut Word, inner: &mut Vec<Segment>, depth: usize, in_double: bool) {
        let start = self.pos;
        match (self.peek(1), self.peek(2)) {
            (Some('('), Some('(')) => {
                self.advance(3);
                let mut open = 0usize;
                while let Some(c) = self.peek(0) {
                    self.advance(1);
                    match c {
                        '(' => open += 1,
                        ')' if open > 0 => open -= 1,
                        ')' if self.peek(0) == Some(')') => {
                            self.advance(1);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            (Some('('), _) => {
                self.advance(2);
                self.substitution(start, w, inner, Close::Paren, depth);
                return;
            }
            (Some('{'), _) => {
                self.advance(2);
                let mut open = 0usize;
                while let Some(c) = self.peek(0) {
                    self.advance(1);
                    match c {
                        '{' => open += 1,
                        '}' if open == 0 => break,
                        '}' => open -= 1,
                        _ => {}
                    }
                }
            }
            (Some('\''), _) if !in_double => {
                self.advance(2);
                while let Some(c) = self.peek(0) {
                    self.advance(if c == '\\' { 2 } else { 1 });
                    if c == '\'' {
                        break;
                    }
                }
            }
            _ => self.advance(1),
        }
        let spelled = self.spelled_since(start);
        w.text.push_str(&spelled);
        w.raw.push_str(&spelled);
    }

    /// Read the commands of a substitution that opened at `start`, then keep
    /// its whole spelling inside the word that holds it.
    fn substitution(&mut self, start: usize, w: &mut Word, inner: &mut Vec<Segment>, close: Close, depth: usize) {
        if depth < MAX_DEPTH {
            let found = self.list(close, depth + 1);
            inner.extend(found);
        } else {
            self.skip_as_text(close);
        }
        let spelled = self.spelled_since(start);
        w.text.push_str(&spelled);
        w.raw.push_str(&spelled);
    }

    /// Past the depth limit: move to the end of the substitution without
    /// reading its commands.
    fn skip_as_text(&mut self, close: Close) {
        let mut open = 0usize;
        let mut quote: Option<char> = None;
        while let Some(c) = self.peek(0) {
            self.advance(if c == '\\' { 2 } else { 1 });
            match (quote, c) {
                (Some(q), _) if c == q => quote = None,
                (Some(_), _) | (None, '\\') => {}
                (None, '\'' | '"') => quote = Some(c),
                (None, '`') if close == Close::Backtick => return,
                (None, '(') => open += 1,
                (None, ')') if close == Close::Paren => {
                    if open == 0 {
                        return;
                    }
                    open -= 1;
                }
                _ => {}
            }
        }
    }

    /// After a line break: the bodies of the heredocs opened on that line,
    /// each up to the line equal to its delimiter. A body is never a command.
    fn skip_heredoc_bodies(&mut self) {
        for (delimiter, strip_tabs) in std::mem::take(&mut self.heredocs) {
            while self.pos < self.chars.len() {
                let start = self.pos;
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.advance(1);
                }
                let line = self.spelled_since(start);
                self.advance(1);
                let line = line.strip_suffix('\r').unwrap_or(&line);
                let line = if strip_tabs { line.trim_start_matches('\t') } else { line };
                if line == delimiter {
                    break;
                }
            }
        }
    }
}

/// Close the command being read: resolve its program and push it, followed by
/// the commands found inside it.
fn finish(cur: &mut Pending, out: &mut Vec<Segment>, depth: usize) {
    let Pending { words, redirects, inner } = std::mem::take(cur);
    if !words.is_empty() || !redirects.is_empty() {
        let start = command_start(&words);
        let mut rest = words.into_iter().skip(start);
        let program = rest.next().unwrap_or_default();
        let segment = Segment { program, args: rest.collect(), redirects };
        let nested = shell_line(&segment, depth);
        out.push(segment);
        out.extend(inner);
        out.extend(nested);
    } else {
        out.extend(inner);
    }
}

/// Where the command really starts: past the structure words, the
/// `NAME=value` assignments and the wrappers with their options.
fn command_start(words: &[Word]) -> usize {
    let mut i = 0;
    while let Some(w) = words.get(i) {
        if (w.is_plain() && STRUCTURE_WORDS.contains(&w.text.as_str())) || is_assignment(w) {
            i += 1;
            continue;
        }
        match wrapper_end(words, i) {
            Some(next) if next > i => i = next,
            _ => break,
        }
    }
    i
}

/// `NAME=value` (or `NAME+=value`), with the name written plain.
fn is_assignment(w: &Word) -> bool {
    let Some((name, _)) = w.raw.split_once('=') else {
        return false;
    };
    let name = name.strip_suffix('+').unwrap_or(name);
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// When `words[i]` is a wrapper that runs the command after it, the index
/// just past the wrapper and its options.
fn wrapper_end(words: &[Word], i: usize) -> Option<usize> {
    let after = i + 1;
    let end = match base_name(&words.get(i)?.text) {
        "rtk" => {
            let j = skip_options(words, after, "", &[]);
            if words.get(j).is_some_and(|w| w.text == "proxy") { j + 1 } else { j }
        }
        "sudo" => skip_options(
            words,
            after,
            "ughpCDrtUT",
            &["user", "group", "host", "prompt", "chdir", "role", "type", "other-user", "close-from", "command-timeout"],
        ),
        "env" => skip_options(words, after, "uCS", &["unset", "chdir", "split-string"]),
        "command" | "nohup" => skip_options(words, after, "", &[]),
        "exec" => skip_options(words, after, "a", &[]),
        "time" => skip_options(words, after, "fo", &["format", "output"]),
        "nice" => skip_options(words, after, "n", &["adjustment"]),
        // The first word after the options is the duration.
        "timeout" => skip_options(words, after, "sk", &["signal", "kill-after"]) + 1,
        "xargs" => skip_options(
            words,
            after,
            "InPdaLsE",
            &["max-args", "max-procs", "delimiter", "arg-file", "max-lines", "max-chars", "eof", "replace"],
        ),
        _ => return None,
    };
    Some(end)
}

/// Skip the options starting at `i`. A short option listed in `short_values`
/// (or a long one in `long_values`) takes the next word as its value when the
/// value is not attached.
fn skip_options(words: &[Word], mut i: usize, short_values: &str, long_values: &[&str]) -> usize {
    while let Some(w) = words.get(i) {
        let text = w.text.as_str();
        if text == "--" {
            return i + 1;
        }
        if let Some(long) = text.strip_prefix("--") {
            i += if long_values.contains(&long) { 2 } else { 1 };
            continue;
        }
        match text.strip_prefix('-') {
            Some(cluster) if !cluster.is_empty() => {
                let takes = cluster.char_indices().find(|(_, c)| short_values.contains(*c));
                i += match takes {
                    Some((at, c)) if at + c.len_utf8() == cluster.len() => 2,
                    _ => 1,
                };
            }
            _ => break,
        }
    }
    i
}

/// The line a shell is told to run — `bash -c "…"`, `sh -c "…"`, `eval …` —
/// read as commands, because the terminal runs it.
fn shell_line(seg: &Segment, depth: usize) -> Vec<Segment> {
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    let line = match seg.name() {
        "eval" => seg.args.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
        "bash" | "sh" | "zsh" | "dash" => match dash_c_line(&seg.args) {
            Some(line) => line.to_string(),
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    read_line(&line, depth + 1)
}

/// The command string of a shell called with `-c` (alone or in a group like
/// `-lc`): the first word after the options.
fn dash_c_line(args: &[Word]) -> Option<&str> {
    let mut has_c = false;
    let mut i = 0;
    while let Some(w) = args.get(i) {
        let text = w.text.as_str();
        match text {
            "--" | "-" => {
                i += 1;
                break;
            }
            "-o" | "+o" | "-O" | "+O" | "--rcfile" | "--init-file" => i += 2,
            _ if text.starts_with("--") => i += 1,
            _ if text.starts_with('-') || text.starts_with('+') => {
                has_c |= text.starts_with('-') && text.contains('c');
                i += 1;
            }
            _ => break,
        }
    }
    if has_c { args.get(i).map(|w| w.text.as_str()) } else { None }
}

/// A program name without its folder.
fn base_name(text: &str) -> &str {
    text.rsplit('/').next().unwrap_or(text)
}

/// The word boundaries of the command guard, for
/// [`mustard_core::domain::text::has_word_sequence`]: a letter or digit is a
/// word char and `_` is not (`git_push` has a boundary before `push`). With
/// two words it is the `\bA\s+B\b` shape of `bash-safety.js`.
pub(super) const SHELL_WORDS: Boundaries =
    Boundaries { word_chars: WordChars::Alphanumeric, left: true, right: true };

/// Truncate a string to `max` bytes (char-boundary safe).
pub(super) fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The whitespace-separated tokens that appear *after* the first occurrence of
/// `anchor` as a word. Empty when `anchor` is absent.
pub(super) fn split_after<'a>(cmd: &'a str, anchor: &str) -> Vec<&'a str> {
    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if let Some(pos) = tokens.iter().position(|t| *t == anchor) {
        tokens[pos + 1..].to_vec()
    } else {
        Vec::new()
    }
}

/// Replace shell metacharacters that appear *inside single/double quotes* with
/// spaces, leaving everything else (including the quote chars and the byte
/// length) intact. Used so that a quoted argument like a Grep alternation
/// pattern (`"emit-pipeline|emit-phase"`) is not mistaken for a real shell
/// pipe by the operator and segment scans. Only single ASCII operator bytes
/// are swapped for a single ASCII space, so the result is always valid UTF-8
/// and byte-aligned with the input.
pub(super) fn mask_quoted_operators(cmd: &str) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(cmd.len());
    let mut quote: Option<u8> = None;
    for &b in cmd.as_bytes() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
                out.push(b);
            } else if matches!(b, b'&' | b'|' | b';' | b'>' | b'<' | b'`' | b'\n' | b'\r') {
                out.push(b' ');
            } else {
                out.push(b);
            }
        } else {
            if b == b'\'' || b == b'"' {
                quote = Some(b);
            }
            out.push(b);
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| cmd.to_string())
}

/// `true` when `c` separates one shell command from the next: `&`, `|`, `;`,
/// or a newline. Newlines matter because the Bash tool routinely receives
/// multi-line `command` strings (a sanity `echo` on line 1, the real `rtk …`
/// on line 2); bash treats the line break exactly like `;`, so the segment
/// splitters must too — otherwise an `rtk`-prefixed later line is invisible to
/// the "already wrapped" short-circuit and the gate wrongly denies the whole
/// command.
pub(super) fn is_cmd_separator(c: char) -> bool {
    c == '&' || c == '|' || c == ';' || c == '\n' || c == '\r'
}

/// Strip a single leading `rtk ` wrapper token, returning the rest. When `cmd`
/// is not `rtk`-prefixed it is returned unchanged.
pub(super) fn strip_leading_rtk(cmd: &str) -> &str {
    let trimmed = cmd.trim_start();
    if let Some(rest) = trimmed.strip_prefix("rtk")
        && rest.starts_with(char::is_whitespace) {
            return rest.trim_start();
        }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn programs(cmd: &str) -> Vec<String> {
        segments(cmd).into_iter().map(|s| s.program.text).collect()
    }

    fn texts(words: &[Word]) -> Vec<&str> {
        words.iter().map(|w| w.text.as_str()).collect()
    }

    #[test]
    fn quoted_text_is_one_word_and_never_a_command() {
        let found = segments(r#"git commit -m "limpa com rm -rf build""#);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].program.text, "git");
        assert_eq!(texts(&found[0].args), ["commit", "-m", "limpa com rm -rf build"]);
    }

    #[test]
    fn separators_split_segments_outside_quotes_only() {
        assert_eq!(programs("a && b || c; d | e & f\ng"), ["a", "b", "c", "d", "e", "f", "g"]);
        assert_eq!(programs("a |& b\r\nc"), ["a", "b", "c"]);
        let quoted = segments(r#"echo "a;b|c" 'd&&e'"#);
        assert_eq!(quoted.len(), 1, "{quoted:?}");
        assert_eq!(texts(&quoted[0].args), ["a;b|c", "d&&e"]);
    }

    #[test]
    fn env_assignments_and_wrappers_are_not_the_program() {
        let found = segments(r#"FOO=1 BAR="x y" sudo -u root rtk rm -rf x"#);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].program.text, "rm");
        assert_eq!(texts(&found[0].args), ["-rf", "x"]);
        for cmd in [
            "env -i HOME=/x rm -rf x",
            "command rm -rf x",
            "exec -a nome rm -rf x",
            "nohup rm -rf x",
            "time -p rm -rf x",
            "nice -n 10 rm -rf x",
            "timeout -s KILL 30 rm -rf x",
            "xargs -I {} rm -rf {}",
            "rtk proxy rm -rf x",
            "/bin/rm -rf x",
        ] {
            let found = segments(cmd);
            assert_eq!(found[0].name(), "rm", "{cmd}: {found:?}");
        }
    }

    /// The value of an assignment keeps its substitution whole: cutting
    /// `D=$(mktemp -d)` at the space once left `D` empty and sent the next
    /// commands to the current folder.
    #[test]
    fn an_env_value_with_a_substitution_stays_one_word() {
        let found = segments(r#"D=$(mktemp -d) && [ -n "$D" ] && cd "$D" && git init -q ."#);
        let names: Vec<&str> = found.iter().map(|s| s.program.text.as_str()).collect();
        assert_eq!(names, ["", "mktemp", "[", "cd", "git"], "{found:?}");
        assert_eq!(texts(&found[1].args), ["-d"]);
        assert_eq!(texts(&found[3].args), ["$D"]);
        // Nested substitutions close on their own parenthesis, and a quoted
        // value keeps its spaces.
        assert_eq!(programs("A=$(a $(b c)) rest"), ["rest", "a", "b"]);
        assert_eq!(programs(r#"A="um dois" rest"#), ["rest"]);
        assert_eq!(programs("A=b rest"), ["rest"]);
    }

    #[test]
    fn a_command_substitution_is_read_as_its_own_command() {
        for cmd in ["echo $(rm -rf x)", r#"echo "$(rm -rf x)""#, "echo `rm -rf x`", r#"git commit -m "$(rm -rf x)""#] {
            let found = segments(cmd);
            assert!(found.iter().any(|s| s.name() == "rm" && texts(&s.args) == ["-rf", "x"]), "{cmd}: {found:?}");
        }
        // The command inside comes right after the one holding it.
        assert_eq!(programs("echo $(a) ; b"), ["echo", "a", "b"]);
        // Arithmetic and a braced variable are text, not commands.
        assert_eq!(programs("echo $((1 + (2 * 3))) ${X:-y}"), ["echo"]);
    }

    #[test]
    fn single_quotes_keep_a_substitution_as_text() {
        assert_eq!(programs("echo '$(rm -rf x)'"), ["echo"]);
        assert_eq!(programs("echo '`rm -rf x`'"), ["echo"]);
    }

    #[test]
    fn a_heredoc_body_is_text() {
        let file = "git commit -F - <<'EOF'\nrm -rf x\nEOF\necho fim";
        assert_eq!(programs(file), ["git", "echo"]);
        let inline = "git commit -m \"$(cat <<'EOF'\nlimpa: rm -rf x\n\nEOF\n)\"";
        assert_eq!(programs(inline), ["git", "cat"]);
        let tabs = "cat <<-FIM\n\trm -rf x\n\tFIM\nls";
        assert_eq!(programs(tabs), ["cat", "ls"]);
        let two = "cat <<A <<\"B\"\nrm -rf x\nA\nrm -rf y\nB\nls";
        assert_eq!(programs(two), ["cat", "ls"]);
        // A here-string is one word of text.
        assert_eq!(programs("cat <<< 'rm -rf x'"), ["cat"]);
    }

    #[test]
    fn a_comment_is_not_a_command() {
        assert_eq!(programs("ls # rm -rf x"), ["ls"]);
        assert_eq!(programs("# rm -rf x\nls"), ["ls"]);
        assert_eq!(programs("echo a#b"), ["echo"]);
    }

    #[test]
    fn redirects_are_kept_apart_from_arguments() {
        let found = segments(r#"cmd 2> err.txt > "C:\x y\out.txt" 2>&1"#);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].args.is_empty(), "{found:?}");
        let ops: Vec<&str> = found[0].redirects.iter().map(|r| r.op.as_str()).collect();
        assert_eq!(ops, ["2>", ">", "2>&"]);
        assert_eq!(found[0].redirects[1].target.raw, r#""C:\x y\out.txt""#);
        assert_eq!(found[0].redirects[1].target.raw_unquoted(), r"C:\x y\out.txt");
        // Outside quotes the shell eats the backslash; the raw spelling keeps it.
        let bare = segments(r"cmd > C:\Atiz\x");
        assert_eq!(bare[0].redirects[0].target.text, "C:Atizx");
        assert_eq!(bare[0].redirects[0].target.raw, r"C:\Atiz\x");
    }

    #[test]
    fn structure_words_do_not_hide_the_command() {
        assert!(programs("if true; then rm -rf x; fi").contains(&"rm".to_string()));
        assert_eq!(programs("(cd a && rm -rf b)"), ["cd", "rm"]);
        assert_eq!(programs("{ rm -rf x; }"), ["rm", ""]);
        assert!(programs("for f in a b; do rm -rf $f; done").contains(&"rm".to_string()));
        // A quoted keyword is a program name, not a keyword.
        assert_eq!(programs("'if' x"), ["if"]);
    }

    #[test]
    fn an_unclosed_quote_keeps_the_rest_as_one_word() {
        let found = segments(r#"echo "abc; rm -rf x"#);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(texts(&found[0].args), ["abc; rm -rf x"]);
        assert_eq!(programs("echo 'abc && rm -rf x"), ["echo"]);
    }

    #[test]
    fn a_shell_dash_c_argument_is_read_as_a_command() {
        for cmd in [
            r#"bash -c "rm -rf x""#,
            "sh -c 'rm -rf x'",
            r#"sudo bash -lc "cd a && rm -rf x""#,
            "zsh -o pipefail -c 'rm -rf x'",
            r#"eval "rm -rf x""#,
        ] {
            let found = segments(cmd);
            assert!(found.iter().any(|s| s.name() == "rm"), "{cmd}: {found:?}");
        }
        // Without `-c` the argument is a script path, not a command line.
        assert_eq!(programs("bash script.sh 'rm -rf x'"), ["bash"]);
    }

    #[test]
    fn a_deep_nesting_stops_descending_and_never_panics() {
        let deep = format!("{}rm -rf x{}", "$(".repeat(40), ")".repeat(40));
        let found = segments(&format!("echo {deep}"));
        assert_eq!(found[0].program.text, "echo");
        assert!(found.len() <= MAX_DEPTH + 1, "{}", found.len());
        for odd in ["", "\\", "'", "\"", "$(", "`", "<<", "2>", "&>", ")", "((", "$((", "${", "$'", "cat <<EOF"] {
            let _ = segments(odd);
        }
    }
}
