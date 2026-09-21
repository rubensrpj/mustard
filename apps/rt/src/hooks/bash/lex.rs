//! Reading a Bash `command` string, shared by the Bash gate family.
//!
//! [`segments`] reads the command the way the terminal does — words, quotes,
//! separators, redirects, substitutions, heredocs, comments — and hands back
//! each simple command it would run. No verdicts are produced here.

/// How deep the reader descends into `$(…)`, backticks and `bash -c "…"`.
/// Past this depth the rest of the substitution is kept as plain text.
const MAX_DEPTH: usize = 16;

/// Words that open or close a compound command. Written plain at the head of
/// a command they are skipped, so the real command behind them is the program
/// (`if x; then rm -rf y; fi` runs `rm`).
const STRUCTURE_WORDS: &[&str] =
    &["!", "{", "}", "if", "then", "else", "elif", "fi", "do", "done", "while", "until"];

/// The word that opens a function definition. Written plain at the head of a
/// command it is skipped together with the function's name, so the body is
/// read (`function f { rm -rf x; }` runs `rm`).
const FUNCTION_WORD: &str = "function";

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
    /// The structure words ([`STRUCTURE_WORDS`]) written plain right before
    /// the program, in order: `while pgrep x` is `["while"]`, `do sleep 1` is
    /// `["do"]`. A wrapper or an assignment in between is skipped, not kept.
    pub leading: Vec<String>,
}

impl Segment {
    /// The program's name without its folder: `/bin/rm` is `rm`.
    pub fn name(&self) -> &str {
        base_name(&self.program.text)
    }
}

/// The commands the terminal would run for `cmd`, in the order it meets them.
///
/// Quoted text is never a command, with the exceptions the terminal itself
/// makes: what sits inside `$(…)`, backticks or `<(…)` (also inside double
/// quotes, a `${…}` or a `$((…))`), and the line handed to `bash -c`,
/// `sh -c`, `zsh -c`, `dash -c` or `eval`. The command of a `find -exec`
/// (`-execdir`, `-ok`, `-okdir`) is read too, since `find` runs it. Those
/// commands come right after the command that holds them. A comment is never
/// a command, and neither is a heredoc body, except the `$(…)` and backticks
/// of a body whose delimiter has no quote (`<<EOF`), which the terminal runs;
/// `<<'EOF'` keeps the whole body as text. The reader walks the text once,
/// never panics, and stops descending past [`MAX_DEPTH`] levels.
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

/// What a `${…}` or `$((…))` is, which tells where it closes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expansion {
    /// `${…}`, closed by its `}`.
    Braced,
    /// `$((…))`, closed by `))`.
    Arithmetic,
}

/// A heredoc opened on the current line, waiting for the line to end.
struct Heredoc {
    delimiter: String,
    /// `<<-` drops the leading tabs of each body line.
    strip_tabs: bool,
    /// The delimiter has no quote and no backslash, so the terminal runs the
    /// substitutions of the body.
    expands: bool,
}

struct Reader {
    chars: Vec<char>,
    pos: usize,
    heredocs: Vec<Heredoc>,
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
                    self.heredoc_bodies(&mut out, depth);
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
            self.heredocs.push(Heredoc {
                delimiter: target.text.clone(),
                strip_tabs: op == "<<-",
                expands: target.is_plain(),
            });
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

    /// `$(…)` is a command. `$((…))` (arithmetic) and `${…}` (a variable)
    /// are text, but the `$(…)` and backticks inside them run, so their
    /// commands are read too. `$'…'` quotes (outside double quotes only).
    fn dollar(&mut self, w: &mut Word, inner: &mut Vec<Segment>, depth: usize, in_double: bool) {
        let start = self.pos;
        match (self.peek(1), self.peek(2)) {
            (Some('('), Some('(')) => {
                self.advance(3);
                self.expansion(Expansion::Arithmetic, inner, depth, in_double);
            }
            (Some('('), _) => {
                self.advance(2);
                self.substitution(start, w, inner, Close::Paren, depth);
                return;
            }
            (Some('{'), _) => {
                self.advance(2);
                self.expansion(Expansion::Braced, inner, depth, in_double);
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

    /// The inside of a `${…}` or `$((…))` whose opening was just read, up to
    /// its closing `}` or `))`. The `$(…)` and backticks met there go to
    /// `inner`; each level counts toward [`MAX_DEPTH`], and past it the rest
    /// is only walked through as text.
    fn expansion(&mut self, kind: Expansion, inner: &mut Vec<Segment>, depth: usize, in_double: bool) {
        let depth = depth + 1;
        let reads = depth < MAX_DEPTH;
        // The spelling is kept by the caller; this word only takes what the
        // nested readers write.
        let mut text = Word::default();
        let mut open = 0usize;
        while let Some(c) = self.peek(0) {
            match (kind, c) {
                (_, '\\') => self.advance(2),
                (_, '$') if reads => self.dollar(&mut text, inner, depth, in_double),
                (_, '`') if reads => {
                    let start = self.pos;
                    self.advance(1);
                    self.substitution(start, &mut text, inner, Close::Backtick, depth);
                }
                (_, '"') if reads => self.double_quoted(&mut text, inner, depth),
                (Expansion::Braced, '\'') if !in_double => self.single_quoted(&mut text),
                (Expansion::Braced, '{') | (Expansion::Arithmetic, '(') => {
                    open += 1;
                    self.advance(1);
                }
                (Expansion::Braced, '}') | (Expansion::Arithmetic, ')') if open > 0 => {
                    open -= 1;
                    self.advance(1);
                }
                (Expansion::Braced, '}') => {
                    self.advance(1);
                    return;
                }
                (Expansion::Arithmetic, ')') if self.peek(1) == Some(')') => {
                    self.advance(2);
                    return;
                }
                _ => self.advance(1),
            }
        }
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
    /// each up to the line equal to its delimiter. A body is text; when its
    /// delimiter has no quote, the commands of its substitutions go to `out`.
    fn heredoc_bodies(&mut self, out: &mut Vec<Segment>, depth: usize) {
        for doc in std::mem::take(&mut self.heredocs) {
            let body_start = self.pos;
            let mut body_end = self.chars.len();
            while self.pos < self.chars.len() {
                let start = self.pos;
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.advance(1);
                }
                let line = self.spelled_since(start);
                self.advance(1);
                let line = line.strip_suffix('\r').unwrap_or(&line);
                let line = if doc.strip_tabs { line.trim_start_matches('\t') } else { line };
                if line == doc.delimiter {
                    body_end = start;
                    break;
                }
            }
            if doc.expands {
                let body: String = self.chars.get(body_start..body_end).unwrap_or(&[]).iter().collect();
                out.extend(heredoc_commands(&body, depth));
            }
        }
    }
}

/// The commands the terminal runs inside a heredoc body whose delimiter has
/// no quote: its `$(…)` and backticks, also inside a `${…}` or `$((…))`.
/// The rest of the body is text, quotes included, and a backslash keeps the
/// next character as text. The body is read on its own, so a quote left open
/// inside a substitution never reaches past the body.
fn heredoc_commands(body: &str, depth: usize) -> Vec<Segment> {
    let mut reader = Reader { chars: body.chars().collect(), pos: 0, heredocs: Vec::new() };
    let mut inner = Vec::new();
    let mut text = Word::default();
    while let Some(c) = reader.peek(0) {
        match c {
            '\\' => reader.advance(2),
            '$' => reader.dollar(&mut text, &mut inner, depth, true),
            '`' => {
                let start = reader.pos;
                reader.advance(1);
                reader.substitution(start, &mut text, &mut inner, Close::Backtick, depth);
            }
            _ => reader.advance(1),
        }
    }
    inner
}

/// Close the command being read: resolve its program and push it, followed by
/// the commands found inside it.
fn finish(cur: &mut Pending, out: &mut Vec<Segment>, depth: usize) {
    let Pending { words, redirects, inner } = std::mem::take(cur);
    if !words.is_empty() || !redirects.is_empty() {
        let segment = simple_command(words, redirects);
        let nested = run_by(&segment, depth);
        out.push(segment);
        out.extend(inner);
        out.extend(nested);
    } else {
        out.extend(inner);
    }
}

/// The command `words` spell: the program found past the structure words,
/// the assignments and the wrappers, then its arguments.
fn simple_command(words: Vec<Word>, redirects: Vec<Redirect>) -> Segment {
    // `command_start` can name an index past the last word (a wrapper's own
    // options run out with none of its own left, `function` with nothing
    // after it): `skip` reads past the end fine, but slicing does not.
    let start = command_start(&words).min(words.len());
    let leading = words[..start]
        .iter()
        .filter(|w| w.is_plain() && STRUCTURE_WORDS.contains(&w.text.as_str()))
        .map(|w| w.text.clone())
        .collect();
    let mut rest = words.into_iter().skip(start);
    let program = rest.next().unwrap_or_default();
    Segment { program, args: rest.collect(), redirects, leading }
}

/// Where the command really starts: past the structure words, a function
/// definition's head, the `NAME=value` assignments and the wrappers with
/// their options.
fn command_start(words: &[Word]) -> usize {
    let mut i = 0;
    while let Some(w) = words.get(i) {
        if (w.is_plain() && STRUCTURE_WORDS.contains(&w.text.as_str())) || is_assignment(w) {
            i += 1;
            continue;
        }
        if w.is_plain() && w.text == FUNCTION_WORD {
            i += 2;
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

/// The commands a command runs by itself, read as commands: the line a shell
/// is told to run (`bash -c "…"`, `sh -c "…"`, `eval …`) and the command of
/// each `find` action.
fn run_by(seg: &Segment, depth: usize) -> Vec<Segment> {
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    let line = match seg.name() {
        "eval" => seg.args.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
        "bash" | "sh" | "zsh" | "dash" => match dash_c_line(&seg.args) {
            Some(line) => line.to_string(),
            None => return Vec::new(),
        },
        "find" => return find_actions(&seg.args, depth),
        _ => return Vec::new(),
    };
    read_line(&line, depth + 1)
}

/// The command of each `-exec`, `-execdir`, `-ok` and `-okdir` of a `find`,
/// up to the `;` (or the `+` right after `{}`) that ends it, each followed by
/// the commands it runs in turn.
fn find_actions(args: &[Word], depth: usize) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if !matches!(arg.text.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
            continue;
        }
        let mut words: Vec<Word> = Vec::new();
        for w in rest.by_ref() {
            let ends = w.text == ";" || (w.text == "+" && words.last().is_some_and(|last| last.text == "{}"));
            if ends {
                break;
            }
            words.push(w.clone());
        }
        if words.is_empty() {
            continue;
        }
        let action = simple_command(words, Vec::new());
        let nested = run_by(&action, depth + 1);
        out.push(action);
        out.extend(nested);
    }
    out
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

    /// The structure words right before the program are kept, in order, on
    /// [`Segment::leading`] — `while`/`until` mark the start of a loop's
    /// condition, `do` marks its body, `done` closes it. A wrapper in
    /// between is skipped from `leading`, same as it is from the program.
    #[test]
    fn leading_keeps_the_structure_words_before_the_program() {
        let found = segments("until rtk pgrep -f x; do sleep 1; done");
        let leading: Vec<Vec<&str>> = found.iter().map(|s| s.leading.iter().map(String::as_str).collect()).collect();
        assert_eq!(leading, [vec!["until"], vec!["do"], vec!["done"]], "{found:?}");
        assert_eq!(found[0].name(), "pgrep");
        assert_eq!(found[1].name(), "sleep");

        // A plain command, with no structure word ahead, has an empty leading.
        assert_eq!(segments("pgrep -f x")[0].leading, Vec::<String>::new());
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
    fn a_substitution_inside_a_braced_variable_or_arithmetic_is_read() {
        for cmd in [
            "echo ${X:-$(rm -rf x)}",
            r#"echo "${X:-$(rm -rf x)}""#,
            "echo ${X:-`rm -rf x`}",
            "echo ${X:-${Y:-$(rm -rf x)}}",
            "echo $(( $(rm -rf x) + 1 ))",
            r#"echo "$(( `rm -rf x` * 2 ))""#,
        ] {
            let found = segments(cmd);
            assert!(found.iter().any(|s| s.name() == "rm" && texts(&s.args) == ["-rf", "x"]), "{cmd}: {found:?}");
        }
        // The expansion stays one word of text around the command it holds.
        let found = segments("echo ${X:-$(a)} b; c");
        assert_eq!(programs("echo ${X:-$(a)} b; c"), ["echo", "a", "c"]);
        assert_eq!(texts(&found[0].args), ["${X:-$(a)}", "b"]);
        // Single quotes keep the substitution as text, inside or around it.
        assert_eq!(programs("echo ${X:-'$(rm -rf x)'}"), ["echo"]);
        assert_eq!(programs("echo '${X:-$(rm -rf x)}'"), ["echo"]);
    }

    #[test]
    fn a_heredoc_with_an_unquoted_delimiter_runs_its_substitutions() {
        for cmd in [
            "cat <<EOF\n$(rm -rf x)\nEOF",
            "cat <<EOF\nantes `rm -rf x` depois\nEOF",
            "cat <<-EOF\n\t${X:-$(rm -rf x)}\n\tEOF",
            "git commit -F - <<EOF\nfix: $(rm -rf x)\nEOF",
        ] {
            let found = segments(cmd);
            assert!(found.iter().any(|s| s.name() == "rm" && texts(&s.args) == ["-rf", "x"]), "{cmd:?}: {found:?}");
        }
        // The commands of the body come after the line that opened it.
        assert_eq!(programs("cat <<EOF; b\n$(a)\nEOF\nc"), ["cat", "b", "a", "c"]);
        // A quoted or escaped delimiter keeps the whole body as text, and an
        // escaped `$` is text in any body.
        for cmd in [
            "cat <<'EOF'\n$(rm -rf x)\nEOF",
            "cat <<\"EOF\"\n`rm -rf x`\nEOF",
            "cat <<\\EOF\n$(rm -rf x)\nEOF",
            "cat <<EOF\n\\$(rm -rf x)\nEOF",
            "cat <<EOF\nrm -rf x\nEOF",
        ] {
            assert_eq!(programs(cmd), ["cat"], "{cmd:?}");
        }
        // A quote left open inside a substitution of the body stays there.
        assert_eq!(programs("cat <<EOF\n$(echo it's)\nEOF\nls"), ["cat", "echo", "ls"]);
    }

    #[test]
    fn a_find_action_is_read_as_its_own_command() {
        let found = segments("find . -type d -name build -exec rm -rf {} + -print");
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].name(), found[1].name()), ("find", "rm"));
        assert_eq!(texts(&found[1].args), ["-rf", "{}"]);
        for cmd in [
            r"find . -execdir rm -rf {} \;",
            "find . -ok rm -rf {} ';'",
            "find . -okdir sudo rm -rf {} +",
            r#"find . -exec bash -c 'rm -rf "$1"' _ {} \;"#,
            r"find . -exec echo {} \; -exec rm -rf {} +",
        ] {
            let found = segments(cmd);
            assert!(found.iter().any(|s| s.name() == "rm"), "{cmd}: {found:?}");
        }
        assert_eq!(programs("find . -name '*.rs' -print"), ["find"]);
        // A `+` that does not follow `{}` belongs to the command.
        let plus = segments(r"find . -exec expr 1 + 1 \;");
        assert_eq!(texts(&plus[1].args), ["1", "+", "1"]);
    }

    #[test]
    fn a_function_definition_does_not_hide_its_body() {
        assert_eq!(programs("function f { rm -rf x; }; f"), ["rm", "", "f"]);
        let found = segments("function limpa() { rm -rf x; }");
        assert!(found.iter().any(|s| s.name() == "rm" && texts(&s.args) == ["-rf", "x"]), "{found:?}");
        // Quoted, the word is a program name like any other.
        assert_eq!(programs("'function' f"), ["function"]);
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

    /// Braced variables, arithmetic and heredoc bodies nest too; each level
    /// counts toward the same limit, so a very deep text neither panics nor
    /// runs out of stack.
    #[test]
    fn deep_expansions_and_heredocs_stop_descending_and_never_panic() {
        let braced = format!("echo {}$(rm -rf x){}", "${X:-".repeat(40), "}".repeat(40));
        assert_eq!(programs(&braced), ["echo"]);
        let shallow = format!("echo {}$(rm -rf x){}", "${X:-".repeat(4), "}".repeat(4));
        assert_eq!(programs(&shallow), ["echo", "rm"]);
        for deep in [
            "${".repeat(10_000),
            "$((".repeat(10_000),
            "echo \"${X:-\"".repeat(5_000),
            "cat <<EOF\n$(cat <<EOF\n".repeat(200),
            "find . -exec ".repeat(1_000),
            "function ".repeat(1_000),
        ] {
            let _ = segments(&deep);
        }
        for odd in ["${$(", "$(( $(", "${`", "cat <<EOF\n$(", "cat <<EOF\n`", "cat <<EOF\n${", "find -exec", "function"] {
            let _ = segments(odd);
        }
    }
}
