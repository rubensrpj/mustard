//! `reading` — a quarta conferência da trava de comandos: a leitura e a busca
//! pelo terminal que o Mustard responde melhor, ou que mostrariam a chave do
//! Jev.
//!
//! - **O arquivo de configuração com a chave.** Um programa que mostra o
//!   texto de um arquivo ([`READERS`]) com o `mustard.json` nos argumentos,
//!   por nome ou por um curinga que o terminal abre (`*.json`), ou qualquer
//!   programa que o recebe por `<`, é recusado quando o arquivo guarda a
//!   chave. O motivo traz o arquivo com a chave trocada por `***`.
//! - **A busca em pastas que passa pelo arquivo com a chave.** O `grep`
//!   recursivo numa pasta que guarda o `mustard.json`, e o `rg` que o lê
//!   mesmo com ele no `.git/info/exclude` (com `-u`, ou com um `-g` que casa
//!   com o nome), são recusados com a opção que deixa o arquivo de fora
//!   ([`config_key::swept`]). A busca que só lista arquivos ou conta
//!   linhas não mostra a chave, e passa.
//! - **A busca por palavra em pastas.** O `grep` recursivo, o `rg` e o
//!   `git grep`, numa pasta de código do projeto, recebem a marca do mapa
//!   ([`word_search`]). O `git grep` lê as opções de padrão do `grep` (`-e`,
//!   `-E`, `-F`, `-P`, `-i`, `-w`), busca a pasta em que roda quando não traz
//!   caminho e só lê o que o git rastreia. O caminho com curinga que o
//!   terminal abre (`src/*.ts`, `src/**/*.rs`; no `git grep`, também o entre
//!   aspas, que o próprio git abre) vale pela pasta antes do primeiro curinga
//!   e por um filtro de nome, como o `-g` do `rg`; o curinga no nome de uma
//!   pasta (`src/*/x.rs`) deixa a busca passar. Com o mapa cravado, a busca que mostra
//!   linhas é recusada com a resposta agrupada por função no lugar dela; com
//!   o parcial, a busca roda inteira, com a nota do mapa junto, só como
//!   contexto (o parcial passa antes pelo filtro do mapa, que entrega só as
//!   peças certas, e sem chave ou com o filtro falhando vale a triagem, com o
//!   aviso uma vez por sessão); a que só lista nomes ou conta (`-l`, `-c`)
//!   segue, sem filtro e com uma linha da marca; sem achado, ou com o filtro
//!   dizendo que nada serve, a busca segue com uma linha do que o mapa não
//!   achou. A busca num arquivo só, fora do projeto, em
//!   pasta sem código do mapa, com filtros de nome que deixam só documentos ou
//!   que tiram todo o código do mapa (`-g '!*.rs'`, `--exclude=*.rs`), com
//!   opção que esta leitura não entende (`-v`, `-x`) ou com a chave
//!   `search.answer` desligada passa.
//! - **A busca por nome de arquivo e o `find` que busca texto.** O `find`
//!   com `-name`, `-iname` ou `-path` recebe a linha da marca do mapa para as
//!   palavras do padrão (`-name '*payment*.ts'` dá `payment`) e roda como
//!   veio; o padrão sem palavra passa calado. O `find ... -exec grep ... {}`
//!   e o `find ... | xargs grep ...` são a busca de texto do `grep` recursivo
//!   nas pastas do `find`, com o `-name` dele como filtro de nome; expressão
//!   com `-o`, `!`, parênteses ou testes além de `-name` e `-type f` deixa a
//!   busca passar.
//!
//! Lê os comandos que [`super::lex::segments`] achou, nunca o texto cru, e
//! segue os `cd` da linha para saber de que pasta cada caminho parte.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Ctx, HookInput, Verdict};

use super::lex::{Segment, Word};
use crate::hooks::write::write_gate::{say, search_reply};
use crate::shared::code_route;
use crate::shared::config_key::{self, CONFIG_FILE, NameFilter, Walk};
use crate::shared::word_search::{self, Dialect, Reply};

/// Os programas que mostram o texto de um arquivo.
pub(crate) const READERS: &[&str] = &[
    "cat", "tac", "nl", "head", "tail", "less", "more", "bat", "grep", "egrep", "fgrep", "rg", "jq", "sed", "awk", "cut", "sort", "strings", "xxd", "od",
    "base64",
];

/// As opções do `grep` que levam um valor: as curtas, e as longas quando o
/// valor vem na palavra seguinte.
const GREP_SHORT_VALUE: &[char] = &['e', 'f', 'm', 'A', 'B', 'C', 'd', 'D'];
const GREP_LONG_VALUE: &[&str] = &[
    "regexp",
    "file",
    "max-count",
    "after-context",
    "before-context",
    "context",
    "include",
    "exclude",
    "exclude-from",
    "exclude-dir",
    "label",
    "binary-files",
    "devices",
    "directories",
    "group-separator",
];

/// As opções do `rg` que levam um valor, do mesmo jeito.
const RG_SHORT_VALUE: &[char] = &['e', 'f', 'g', 't', 'T', 'm', 'A', 'B', 'C', 'M', 'j', 'r', 'E'];
const RG_LONG_VALUE: &[&str] = &[
    "regexp",
    "file",
    "glob",
    "iglob",
    "type",
    "type-not",
    "type-add",
    "type-clear",
    "max-count",
    "after-context",
    "before-context",
    "context",
    "max-columns",
    "threads",
    "replace",
    "encoding",
    "max-depth",
    "max-filesize",
    "sort",
    "sortr",
    "color",
    "colors",
    "context-separator",
    "field-context-separator",
    "field-match-separator",
    "path-separator",
    "pre",
    "pre-glob",
    "engine",
    "dfa-size-limit",
    "regex-size-limit",
    "ignore-file",
    "hostname-bin",
    "hyperlink-format",
];

/// As opções do `git grep` que levam um valor, do mesmo jeito.
const GIT_GREP_SHORT_VALUE: &[char] = &['e', 'f', 'm', 'A', 'B', 'C'];
const GIT_GREP_LONG_VALUE: &[&str] = &["max-depth", "threads", "max-count", "after-context", "before-context", "context"];

/// A conferência inteira: a primeira recusa da linha, na ordem dos comandos.
pub(super) fn bash_reading(segments: &[Segment], cmd: &str, input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    let root = ctx.project_dir_or_cwd(input);
    let lang = ctx.config.language().text_or_default();
    let mut cwd = PathBuf::from(input.cwd.as_deref().filter(|cwd| !cwd.is_empty()).unwrap_or(&root));
    let mut note: Option<String> = None;
    // O `grep` que o `xargs` alimenta com a saída do `find` anterior já foi
    // lido junto com ele.
    let mut fed = false;
    for (at, segment) in segments.iter().enumerate() {
        if segment.name() == "cd" {
            if let Some(dir) = segment.args.first().map(|word| word.text.as_str()).filter(|dir| !dir.starts_with('-')) {
                cwd = cwd.join(dir);
            }
            continue;
        }
        if let Some(reason) = config_refusal(segment, &cwd, lang) {
            return Some(Verdict::Deny { reason });
        }
        if std::mem::take(&mut fed) {
            continue;
        }
        let found = find_read(segment);
        let from_find = found.as_ref().and_then(|find| find_text_search(find, segments.get(at + 1), cmd));
        fed = from_find.as_ref().is_some_and(|(_, feeds)| *feeds);
        let Some(search) = from_find.map(|(search, _)| search).or_else(|| text_search(segment)) else {
            if let Some(context) = found.and_then(|find| find_note(&find, &cwd, &root, input, ctx)) {
                note = Some(context);
            }
            continue;
        };
        let base = cwd.to_string_lossy();
        let searched = if search.paths.is_empty() { vec![".".to_string()] } else { search.paths.clone() };
        let folders: Vec<code_route::ProjectPath> =
            searched.iter().filter_map(|path| code_route::project_path(&root, &base, path)).filter(|path| path.abs.is_dir()).collect();
        // A busca só se responde quando todo caminho dela é pasta do projeto.
        if !search.unsupported && !search.patterns.is_empty() && folders.len() == searched.len() {
            let wanted = word_search::Search {
                patterns: &search.patterns,
                dialect: search.dialect,
                ignore_case: search.ignore_case,
                whole_word: search.whole_word,
                folders: &folders,
                filters: &search.filters,
                walk: search.walk,
                shows_lines: search.shows_lines,
            };
            match search_reply(&root, input, ctx, &wanted) {
                Reply::Answer(reason) => return Some(Verdict::Deny { reason }),
                Reply::Note(context) => note = Some(context),
                Reply::Pass => {}
            }
        }
        // O `git grep` só lê o que o git rastreia, e o arquivo da chave fica no
        // `.git/info/exclude`: só a busca que passa por cima disso o alcança.
        let reaches_ignored = !search.git || search.walk == (Walk::Rg { unignored: true });
        if !search.shows_lines || !reaches_ignored {
            continue;
        }
        let folders: Vec<PathBuf> = searched.iter().map(|path| cwd.join(path)).collect();
        if let Some(file) = config_key::swept(&folders, Path::new(&root), search.walk, &search.filters) {
            let fix = match search.walk {
                _ if search.git => format!("`':!{CONFIG_FILE}'`"),
                Walk::Grep => format!("`--exclude={CONFIG_FILE}`"),
                Walk::Rg { .. } => format!("`-g '!{CONFIG_FILE}'`"),
            };
            return Some(Verdict::Deny { reason: say("config_key.swept", lang, &[("{file}", &file), ("{fix}", &fix)]) });
        }
    }
    note.map(|context| Verdict::Inject { context })
}

/// O nome que o `find` busca: o padrão do `-name`, `-iname` ou `-path`.
struct FindName {
    glob: String,
    /// `-path`: o padrão vale para o caminho inteiro, não para o nome.
    whole_path: bool,
    /// `-iname` e `-ipath`: sem olhar maiúsculas.
    ignore_case: bool,
}

/// Um `find` como esta leitura o vê.
struct FindRead {
    /// As pastas em que ele busca; vazio quando a linha não traz nenhuma.
    paths: Vec<String>,
    /// O primeiro teste de nome da linha.
    name: Option<FindName>,
    /// `false` quando a expressão tem `-o`, `!`, `-not`, parênteses ou
    /// `-prune`: o teste de nome então não vale para todo arquivo achado.
    conjunction: bool,
    /// `true` quando os testes são só um de nome e `-type f`: a busca de texto
    /// que ele dispara lê os mesmos arquivos que o `grep` recursivo.
    plain: bool,
    /// O comando do `-exec` (ou `-execdir`, `-ok`, `-okdir`), sem o `{}` e
    /// sem o `;` ou `+` do fim.
    exec: Vec<Word>,
}

/// O `find` que `segment` roda; `None` em qualquer outro programa.
fn find_read(segment: &Segment) -> Option<FindRead> {
    if segment.name() != "find" {
        return None;
    }
    let mut args = segment.args.iter().peekable();
    while args.next_if(|word| matches!(word.text.as_str(), "-H" | "-L" | "-P")).is_some() {}
    let mut paths: Vec<String> = Vec::new();
    while let Some(word) = args.next_if(|word| !word.text.starts_with('-') && !matches!(word.text.as_str(), "(" | ")" | "!" | ",")) {
        paths.push(word.text.clone());
    }
    let (mut name, mut conjunction, mut plain, mut exec) = (None, true, true, Vec::new());
    while let Some(word) = args.next() {
        match word.text.as_str() {
            option @ ("-name" | "-iname" | "-path" | "-ipath" | "-wholename" | "-iwholename") => {
                let Some(glob) = args.next() else { break };
                if name.is_some() {
                    plain = false;
                } else {
                    name = Some(FindName {
                        glob: glob.text.clone(),
                        whole_path: option.contains("path") || option.contains("wholename"),
                        ignore_case: option.starts_with("-i"),
                    });
                }
            }
            "-type" => plain &= args.next().is_some_and(|kind| kind.text == "f"),
            "-o" | "-or" | "!" | "-not" | "(" | ")" | "-prune" | "," => {
                conjunction = false;
                plain = false;
            }
            "-exec" | "-execdir" | "-ok" | "-okdir" => {
                for inner in args.by_ref() {
                    if inner.text == ";" || inner.text == "+" {
                        break;
                    }
                    if inner.text != "{}" {
                        exec.push(inner.clone());
                    }
                }
            }
            "-print" | "-print0" | "-a" | "-and" => {}
            _ => plain = false,
        }
    }
    Some(FindRead { paths, name, conjunction, plain, exec })
}

/// A busca de texto que o `find` dispara, lida como a do `grep` recursivo nas
/// pastas do `find`, com o `-name` dele como filtro de nome: o `grep` do
/// `-exec`, ou o que vem em `next` com o `xargs` na linha `cmd`. O segundo
/// valor diz se o `next` foi lido junto. `None` sem `grep` ou `rg` para ler.
fn find_text_search(find: &FindRead, next: Option<&Segment>, cmd: &str) -> Option<(TextSearch, bool)> {
    let (program, args, feeds) = match (find.exec.split_first(), next) {
        (Some((program, args)), _) => (program.clone(), args.to_vec(), false),
        (None, Some(next)) if cmd.contains("xargs") => (next.program.clone(), next.args.clone(), true),
        _ => return None,
    };
    let mut reader = Segment { program, args, ..Segment::default() };
    let recursive = matches!(reader.name(), "grep" | "egrep" | "fgrep");
    if !recursive && reader.name() != "rg" {
        return None;
    }
    reader.args.retain(|word| word.text != "/dev/null");
    if recursive {
        reader.args.insert(0, Word { text: "-r".to_string(), raw: "-r".to_string() });
    }
    let mut search = text_search(&reader)?;
    search.paths.clone_from(&find.paths);
    match &find.name {
        Some(name) if !name.whole_path && !name.ignore_case => {
            search.filters.insert(0, NameFilter { exclude: false, glob: name.glob.clone() });
        }
        Some(_) => search.unsupported = true,
        None => {}
    }
    search.unsupported |= !find.plain || !find.conjunction;
    Some((search, feeds))
}

/// A linha da marca do mapa para a busca por nome do `find`, quando o padrão
/// tem palavra, a expressão é uma conjunção e as pastas são de código do
/// projeto. A busca roda como veio.
fn find_note(find: &FindRead, cwd: &Path, root: &str, input: &HookInput, ctx: &Ctx) -> Option<String> {
    let name = find.name.as_ref().filter(|_| find.conjunction)?;
    let words = word_search::name_words(&name.glob, name.whole_path);
    if words.is_empty() {
        return None;
    }
    let base = cwd.to_string_lossy();
    let searched = if find.paths.is_empty() { vec![".".to_string()] } else { find.paths.clone() };
    let folders: Vec<code_route::ProjectPath> =
        searched.iter().filter_map(|path| code_route::project_path(root, &base, path)).filter(|path| path.abs.is_dir()).collect();
    if folders.len() != searched.len() {
        return None;
    }
    let filters = word_search::extension_filters(&name.glob);
    match search_reply(root, input, ctx, &word_search::names_search(&words, &folders, &filters)) {
        Reply::Note(context) => Some(context),
        _ => None,
    }
}

/// A recusa do comando que mostraria o arquivo de configuração com a chave:
/// um leitor com o arquivo nos argumentos, ou qualquer programa com o arquivo
/// entrando por `<`, pelo nome ou por um curinga que o alcança.
fn config_refusal(segment: &Segment, cwd: &Path, lang: mustard_core::platform::i18n::Locale) -> Option<String> {
    let reader = READERS.contains(&segment.name());
    let named = segment.args.iter().filter(|_| reader);
    let fed = segment.redirects.iter().filter(|redirect| redirect.op == "<").map(|redirect| &redirect.target);
    named
        .chain(fed)
        .filter_map(|word| config_key::named_by(&word.text, word.text == word.raw))
        .find_map(|path| config_key::refusal(&path, &cwd.join(&path), lang))
}

/// Uma busca de texto do terminal em pastas.
pub(crate) struct TextSearch {
    /// Os padrões escritos na linha, um por `-e` ou o primeiro argumento sem
    /// opção; vazio quando lidos de arquivo.
    patterns: Vec<String>,
    /// Como o programa lê os padrões.
    dialect: Dialect,
    ignore_case: bool,
    whole_word: bool,
    /// A opção que muda o que a busca acha de um jeito que a resposta do mapa
    /// não acompanha (`-v`, `-x`, `-L`, `-U`, um filtro de tipo ou de pasta
    /// que ela não lê): a busca passa.
    unsupported: bool,
    pub(crate) paths: Vec<String>,
    /// Os filtros de nome de arquivo, de entrada e de saída, na ordem da
    /// linha: a busca de palavra e a da chave os leem.
    filters: Vec<NameFilter>,
    walk: Walk,
    /// `false` quando a busca só lista arquivos, conta ou fica quieta: a
    /// saída não traz as linhas.
    shows_lines: bool,
    /// `true` no `git grep`, que lê os arquivos que o git rastreia.
    git: bool,
}

/// Os argumentos do `git grep` de `args`, depois do `grep` e das opções do
/// `git` que não mudam a pasta. `None` em outro subcomando e em opção que
/// muda a pasta (`-C`).
fn git_grep_args(args: &[Word]) -> Option<&[Word]> {
    let mut at = 0;
    while let Some(word) = args.get(at) {
        match word.text.as_str() {
            "grep" => return Some(&args[at + 1..]),
            "--no-pager" | "-P" | "--paginate" | "-p" | "--no-optional-locks" => at += 1,
            "-c" => at += 2,
            _ => return None,
        }
    }
    None
}

/// A busca que `segment` faz, quando é um `grep` recursivo, um `rg` ou um
/// `git grep`. `None` em todo o resto: outro programa, `grep` sem recursão.
pub(crate) fn text_search(segment: &Segment) -> Option<TextSearch> {
    let mut words: &[Word] = &segment.args;
    let (short_value, long_value, rg, git) = match segment.name() {
        "grep" | "egrep" | "fgrep" => (GREP_SHORT_VALUE, GREP_LONG_VALUE, false, false),
        "rg" => (RG_SHORT_VALUE, RG_LONG_VALUE, true, false),
        "git" => {
            words = git_grep_args(&segment.args)?;
            (GIT_GREP_SHORT_VALUE, GIT_GREP_LONG_VALUE, false, true)
        }
        _ => return None,
    };
    // O `git grep` sempre desce pelas pastas, e o `rg` também.
    let mut recursive = rg || git;
    let (mut from_file, mut names_only, mut unignored) = (false, false, false);
    let (mut ignore_case, mut smart_case, mut whole_word, mut unsupported) = (false, false, false, false);
    let mut dialect = match segment.name() {
        "egrep" => Dialect::Extended,
        "fgrep" => Dialect::Fixed,
        _ if rg => Dialect::Rust,
        _ => Dialect::Basic,
    };
    let (mut patterns, mut positionals, mut filters) = (Vec::new(), Vec::new(), Vec::new());
    let mut args = words.iter();
    let mut options_done = false;
    while let Some(word) = args.next() {
        let arg = word.text.as_str();
        if options_done || arg == "-" || !arg.starts_with('-') {
            positionals.push((arg.to_string(), word.text == word.raw));
            continue;
        }
        if arg == "--" {
            options_done = true;
            continue;
        }
        let (option, value) = if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = long.split_once('=').map_or((long, None), |(name, value)| (name, Some(value.to_string())));
            let value = value.or_else(|| long_value.contains(&name).then(|| args.next().map(|word| word.text.clone())).flatten());
            (name.to_string(), value)
        } else {
            let cluster = &arg[1..];
            let found = cluster.char_indices().find(|(_, c)| short_value.contains(c));
            let flags = found.map_or(cluster, |(at, _)| &cluster[..at]);
            recursive |= !rg && flags.contains(['r', 'R']);
            // No `rg`, o `-L` segue os atalhos; no `grep`, lista os arquivos.
            names_only |= flags.contains(['l', 'c', 'q']) || (!rg && flags.contains('L'));
            unignored |= rg && flags.contains('u');
            ignore_case |= flags.contains('i');
            whole_word |= flags.contains('w');
            smart_case |= rg && flags.contains('S');
            unsupported |= flags.contains(['v', 'x', 'z', 'U']) || (!rg && flags.contains('L'));
            if flags.contains('F') {
                dialect = Dialect::Fixed;
            } else if !rg && flags.contains('E') {
                dialect = Dialect::Extended;
            } else if !rg && flags.contains('P') {
                dialect = Dialect::Rust;
            }
            let Some((at, short)) = found else { continue };
            let rest = &cluster[at + short.len_utf8()..];
            let value = if rest.is_empty() { args.next().map(|word| word.text.clone()) } else { Some(rest.to_string()) };
            (short.to_string(), value)
        };
        match (option.as_str(), value) {
            ("e" | "regexp", Some(value)) => patterns.push(value),
            ("f" | "file", _) => from_file = true,
            ("recursive" | "dereference-recursive", _) if !rg => recursive = true,
            ("d" | "directories", Some(value)) if !rg && value == "recurse" => recursive = true,
            ("files-with-matches" | "name-only" | "count" | "quiet" | "silent", _) => names_only = true,
            ("files-without-match", _) => {
                names_only = true;
                unsupported = true;
            }
            ("count-matches", _) if rg => names_only = true,
            ("files", _) if rg => {
                names_only = true;
                unsupported = true;
            }
            ("no-ignore" | "no-ignore-vcs" | "no-ignore-exclude" | "unrestricted", _) if rg => unignored = true,
            // O `git grep` fora do índice, ou sem o que o git ignora, lê tudo.
            ("no-index" | "no-exclude-standard", _) if git => {
                unignored = true;
                unsupported = true;
            }
            // Expressão com `--and`/`--not`, índice, submódulos e limite de
            // profundidade mudam o que a busca acha.
            ("and" | "or" | "not" | "all-match" | "cached" | "untracked" | "recurse-submodules" | "max-depth", _) if git => {
                unsupported = true;
            }
            ("include", Some(value)) if !rg => filters.push(NameFilter { exclude: false, glob: value }),
            ("exclude", Some(value)) if !rg => filters.push(NameFilter { exclude: true, glob: value }),
            ("g" | "glob" | "iglob", Some(value)) if rg => filters.push(NameFilter::rg(&value)),
            ("t" | "type", Some(kind)) if rg => match word_search::type_filters(&kind) {
                Some(typed) => filters.extend(typed),
                None => unsupported = true,
            },
            ("ignore-case", _) => ignore_case = true,
            ("word-regexp", _) => whole_word = true,
            ("smart-case", _) if rg => smart_case = true,
            ("fixed-strings", _) => dialect = Dialect::Fixed,
            ("basic-regexp", _) if !rg => dialect = Dialect::Basic,
            ("extended-regexp", _) if !rg => dialect = Dialect::Extended,
            ("perl-regexp", _) if !rg => dialect = Dialect::Rust,
            ("invert-match" | "line-regexp" | "null-data" | "multiline" | "type-not" | "T" | "pre" | "exclude-dir" | "exclude-from" | "include-from", _) => {
                unsupported = true;
            }
            _ => {}
        }
    }
    if !recursive {
        return None;
    }
    if !from_file && patterns.is_empty() && !positionals.is_empty() {
        patterns.push(positionals.remove(0).0);
    }
    if from_file {
        patterns.clear();
    }
    // O `-S` do `rg` ignora a caixa quando o padrão não tem maiúscula.
    ignore_case |= smart_case && !patterns.iter().any(|pattern| pattern.chars().any(char::is_uppercase));
    let walk = if rg || git { Walk::Rg { unignored } } else { Walk::Grep };
    if git {
        positionals.retain(|(path, _)| match git_exclusion(path) {
            Some(glob) => {
                filters.push(NameFilter { exclude: true, glob: glob.to_string() });
                false
            }
            None => true,
        });
    }
    let paths = search_paths(positionals, git, &mut filters);
    Some(TextSearch { patterns, dialect, ignore_case, whole_word, unsupported, paths, filters, walk, shows_lines: !names_only, git })
}

/// O nome que o filtro de caminho `path` do `git grep` deixa de fora (`:!x`,
/// `:^x`, `:(exclude)x`); `None` em qualquer outro caminho.
fn git_exclusion(path: &str) -> Option<&str> {
    [":!", ":^", ":(exclude)"].iter().find_map(|magic| path.strip_prefix(magic))
}

/// Os caminhos da busca. Todo caminho com curinga do terminal (`dir/*.rs`,
/// `src/**/*.ts`) vira a pasta antes do primeiro curinga, e o nome que vem
/// depois entra como filtro de entrada, como o `-g` do `rg`. O `git grep` abre
/// o curinga sozinho, também entre aspas. Os caminhos ficam como vieram
/// quando algum não tem curinga, quando o nome varia de um caminho a outro ou
/// quando a linha já traz filtro de entrada: um filtro só não diz de qual
/// pasta cada nome vale. Cada `(caminho, sem_aspas)` traz se a palavra foi
/// escrita sem aspas. Chamado depois de tirar os filtros de caminho do git.
fn search_paths(positionals: Vec<(String, bool)>, git: bool, filters: &mut Vec<NameFilter>) -> Vec<String> {
    let split: Option<Vec<(String, String)>> = positionals.iter().map(|(path, plain)| if *plain || git { wildcard_path(path) } else { None }).collect();
    match split {
        Some(split) if !split.is_empty() && split.iter().all(|(_, name)| *name == split[0].1) && filters.iter().all(|filter| filter.exclude) => {
            if split[0].1 != "*" {
                filters.insert(0, NameFilter { exclude: false, glob: split[0].1.clone() });
            }
            let mut folders: Vec<String> = Vec::new();
            for (folder, _) in split {
                if !folders.contains(&folder) {
                    folders.push(folder);
                }
            }
            folders
        }
        _ => positionals.into_iter().map(|(path, _)| path).collect(),
    }
}

/// A pasta e o filtro de nome do caminho com curinga `path`: a pasta que vem
/// antes do primeiro `*` ou `?`, e o último trecho do caminho. Um `**` entre
/// os dois vale por qualquer profundidade. `None` no caminho sem curinga e
/// com curinga no nome de uma pasta (`src/*/x.rs`), com chaves ou colchetes e
/// no filtro de caminho do git (`:!x`).
fn wildcard_path(path: &str) -> Option<(String, String)> {
    let first = path.find(['*', '?'])?;
    if path.starts_with(':') || path.contains(['{', '[', '\\']) {
        return None;
    }
    let (folder, rest) = match path[..first].rfind('/') {
        Some(slash) => (&path[..slash], &path[slash + 1..]),
        None => (".", path),
    };
    let mut parts: Vec<&str> = rest.split('/').collect();
    while parts.len() > 1 && parts[0] == "**" {
        parts.remove(0);
    }
    let [name] = parts[..] else { return None };
    let name = if name == "**" { "*" } else { name };
    Some((if folder.is_empty() { "/" } else { folder }.to_string(), name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::super::lex::segments;
    use super::*;
    use crate::shared::code_route::fixture;
    use mustard_core::domain::model::contract::Trigger;

    /// Os padrões, os caminhos e os filtros de nome de arquivo que a busca de
    /// uma linha lê, na ordem; o de saída leva o `!` do começo.
    fn read(cmd: &str) -> Option<(Vec<String>, Vec<String>, Vec<String>)> {
        let shown = |filter: &NameFilter| {
            if filter.exclude { format!("!{}", filter.glob) } else { filter.glob.clone() }
        };
        segments(cmd).iter().find_map(text_search).map(|s| (s.patterns, s.paths, s.filters.iter().map(shown).collect()))
    }

    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    /// O `grep` só conta como busca em pastas com a recursão; o `rg`, sempre.
    /// As opções com valor não viram padrão nem caminho, e o valor colado na
    /// opção também se lê.
    #[test]
    fn the_search_of_a_line_is_read_like_the_program_reads_it() {
        let one = |pattern: &str, paths: &[&str], filters: &[&str]| Some((owned(&[pattern]), owned(paths), owned(filters)));
        assert_eq!(read("grep -rn Alpha src/"), one("Alpha", &["src/"], &[]));
        assert_eq!(read("grep -n Alpha src/a.rs"), None, "without recursion grep reads the files named");
        assert_eq!(read("grep -A 3 -R --include=*.md Alpha docs"), one("Alpha", &["docs"], &["*.md"]));
        assert_eq!(read("grep -rA3 Alpha"), one("Alpha", &[], &[]));
        assert_eq!(read("grep -d recurse -e Alpha ."), one("Alpha", &["."], &[]));
        assert_eq!(read("rtk rg -n -g '*.rs' Alpha apps/scan"), one("Alpha", &["apps/scan"], &["*.rs"]));
        assert_eq!(read("rg --type rust -C 2 Alpha"), one("Alpha", &[], &["*.rs"]), "a file type is a name filter");
        assert_eq!(read("rg -g '!*.md' Alpha"), one("Alpha", &[], &["!*.md"]), "a negated filter leaves files out");
        assert_eq!(read("rg --glob='!*.md' -g '*.rs' --iglob '!*.txt' Alpha"), one("Alpha", &[], &["!*.md", "*.rs", "!*.txt"]));
        assert_eq!(read("grep -r --exclude=*.rs --include=*.md Alpha"), one("Alpha", &[], &["!*.rs", "*.md"]));
        assert_eq!(read("rg -- -Alpha src"), one("-Alpha", &["src"], &[]));
        assert_eq!(read("rg -e Alpha -e Beta src"), Some((owned(&["Alpha", "Beta"]), owned(&["src"]), vec![])), "each -e is a pattern of the same search");
        assert_eq!(read("grep -rf patterns.txt src"), Some((vec![], owned(&["src"]), vec![])), "patterns from a file are unknown");
        assert_eq!(read("cat x | grep Alpha"), None);
    }

    /// As opções que mudam o que a busca acha, ou o jeito de ler o padrão.
    #[test]
    fn the_options_that_change_what_a_search_finds_are_read() {
        let of = |cmd: &str| segments(cmd).iter().find_map(text_search).unwrap_or_else(|| panic!("{cmd} is a search"));
        for command in ["grep -rv Alpha src", "rg -x Alpha", "rg -t weird Alpha", "grep -rL Alpha .", "rg -U Alpha", "rg --files-without-match Alpha"] {
            assert!(of(command).unsupported, "{command}");
        }
        for command in ["rg -t rust Alpha", "grep -rn Alpha .", "rg -i Alpha"] {
            assert!(!of(command).unsupported, "{command}");
        }
        assert_eq!(of("grep -r a .").dialect, Dialect::Basic);
        assert_eq!(of("grep -rE 'a|b' .").dialect, Dialect::Extended);
        assert_eq!(of("egrep -r a .").dialect, Dialect::Extended);
        assert_eq!(of("grep -rF a.b .").dialect, Dialect::Fixed);
        assert_eq!(of("fgrep -r a .").dialect, Dialect::Fixed);
        assert_eq!(of("grep -rP a .").dialect, Dialect::Rust);
        assert_eq!(of("rg a").dialect, Dialect::Rust);
        assert!(of("grep -rni Alpha .").ignore_case && of("rg --ignore-case Alpha").ignore_case);
        assert!(of("rg -S alpha").ignore_case, "smart case ignores the case of a lowercase pattern");
        assert!(!of("rg -S Alpha").ignore_case);
        assert!(of("grep -rw Alpha .").whole_word);
        assert!(!of("grep -rl Alpha .").shows_lines && !of("rg -c Alpha").shows_lines);
        assert!(of("grep -rn Alpha .").shows_lines);
    }

    /// O `git grep` é uma busca em pastas: o padrão sai do `-e` ou do primeiro
    /// argumento sem opção, o valor de uma opção não vira caminho, os caminhos
    /// vêm depois do `--` ou soltos, e sem caminho a busca é da pasta em que
    /// roda. As opções do `git` que não mudam a pasta antes do `grep` valem; as
    /// que mudam, e os outros subcomandos, não.
    #[test]
    fn a_git_grep_is_read_like_the_grep_reads_its_patterns_and_paths() {
        let one = |pattern: &str, paths: &[&str], filters: &[&str]| Some((owned(&[pattern]), owned(paths), owned(filters)));
        assert_eq!(read("git grep -n Alpha -- src"), one("Alpha", &["src"], &[]));
        assert_eq!(read("git grep Alpha src apps/rt"), one("Alpha", &["src", "apps/rt"], &[]), "loose paths count");
        assert_eq!(read("git grep -n Alpha"), one("Alpha", &[], &[]), "no path searches the folder it runs in");
        assert_eq!(read("git grep -n -A 3 -C2 --max-depth 4 Alpha -- src"), one("Alpha", &["src"], &[]));
        assert_eq!(read("rtk git --no-pager grep -in Alpha -- src"), one("Alpha", &["src"], &[]));
        assert_eq!(read("git -c color.ui=never grep Alpha src"), one("Alpha", &["src"], &[]));
        assert_eq!(read("git grep -e Alpha -e Beta -- src"), Some((owned(&["Alpha", "Beta"]), owned(&["src"]), vec![])));
        assert_eq!(read("git grep -f patterns.txt -- src"), Some((vec![], owned(&["src"]), vec![])), "patterns from a file are unknown");
        assert_eq!(read("git grep -- -Alpha src"), one("-Alpha", &["src"], &[]));
        for command in ["git status", "git log --grep Alpha", "git -C other grep Alpha", "git diff Alpha src", "git"] {
            assert_eq!(read(command), None, "{command} is not a search of a folder");
        }
    }

    /// As opções de padrão do `git grep` valem as do `grep`; o que muda o que
    /// a busca acha (`-v`, `--and`, o índice, as pastas fora do git) a deixa
    /// passar, e só a busca fora do índice ou do que o git ignora lê tudo.
    #[test]
    fn the_options_of_a_git_grep_are_read_like_the_ones_of_the_grep() {
        let of = |cmd: &str| segments(cmd).iter().find_map(text_search).unwrap_or_else(|| panic!("{cmd} is a search"));
        assert_eq!(of("git grep a").dialect, Dialect::Basic);
        assert_eq!(of("git grep -E 'a|b'").dialect, Dialect::Extended);
        assert_eq!(of("git grep -F a.b").dialect, Dialect::Fixed);
        assert_eq!(of("git grep --fixed-strings a.b").dialect, Dialect::Fixed);
        assert_eq!(of("git grep -P a").dialect, Dialect::Rust);
        assert_eq!(of("git grep -n --perl-regexp a").dialect, Dialect::Rust);
        assert!(of("git grep -ni a").ignore_case && of("git grep --ignore-case a").ignore_case);
        assert!(of("git grep -nw a").whole_word && !of("git grep -n a").whole_word);
        assert!(of("git grep -n a").shows_lines && of("git grep -e a").shows_lines);
        assert!(!of("git grep -l a").shows_lines && !of("git grep -c a").shows_lines && !of("git grep --name-only a").shows_lines);
        for command in [
            "git grep -nv a",
            "git grep -e a --and -e b",
            "git grep --not -e a",
            "git grep --cached a",
            "git grep --untracked a",
            "git grep --no-index a",
            "git grep --max-depth 1 a",
        ] {
            assert!(of(command).unsupported, "{command}");
        }
        for command in ["git grep -n a", "git grep -inw -E a", "git grep -e a -e b -- src", "git grep -h a"] {
            assert!(!of(command).unsupported, "{command}");
        }
        assert_eq!(of("git grep -n a").walk, Walk::Rg { unignored: false }, "git reads what it tracks");
        assert_eq!(of("git grep --no-index a").walk, Walk::Rg { unignored: true });
        assert_eq!(of("git grep --no-exclude-standard --untracked a").walk, Walk::Rg { unignored: true });
    }

    /// O caminho com curinga do terminal vale pela pasta antes do primeiro
    /// curinga e por um filtro de nome, como o `-g` do `rg`; o `**` vale por
    /// qualquer profundidade. Os caminhos do mesmo nome dividem o filtro.
    #[test]
    fn a_path_with_a_wildcard_is_the_folder_before_it_and_a_name_filter() {
        let one = |paths: &[&str], filters: &[&str]| Some((owned(&["Alpha"]), owned(paths), owned(filters)));
        assert_eq!(read("grep -rn Alpha apps/rt/tests/*.rs"), one(&["apps/rt/tests"], &["*.rs"]));
        assert_eq!(read("rg Alpha src/**/*.ts"), one(&["src"], &["*.ts"]));
        assert_eq!(read("grep -rn Alpha *.rs"), one(&["."], &["*.rs"]));
        assert_eq!(read("grep -rn Alpha /abs/dir/*.rs"), one(&["/abs/dir"], &["*.rs"]));
        assert_eq!(read("grep -rn Alpha src/**/mod.rs"), one(&["src"], &["mod.rs"]));
        assert_eq!(read("grep -rn Alpha src/*"), one(&["src"], &[]), "a bare star takes every name");
        assert_eq!(read("grep -rn Alpha src/**"), one(&["src"], &[]));
        assert_eq!(read("grep -rn Alpha src/?.rs"), one(&["src"], &["?.rs"]));
        assert_eq!(read("grep -rn Alpha a/*.ts b/*.ts a/x/*.ts"), one(&["a", "b", "a/x"], &["*.ts"]));
        assert_eq!(read("grep -rn Alpha src/*orchestrator*.ts a/*orchestrator*.ts"), one(&["src", "a"], &["*orchestrator*.ts"]));
        assert_eq!(
            read("grep -rn --exclude=*.min.js Alpha src/*.js"),
            one(&["src"], &["*.js", "!*.min.js"]),
            "the filter of the path comes first, and an exclusion that follows still leaves files out"
        );
    }

    /// O curinga no nome de uma pasta, o caminho que mistura curinga e
    /// nome, nomes diferentes, o curinga entre aspas (o terminal não o abre),
    /// as chaves e a linha que já filtra por entrada ficam como vieram: o
    /// caminho não é pasta, e a busca passa.
    #[test]
    fn a_path_whose_wildcard_the_search_cannot_place_stays_as_it_came() {
        let one = |paths: &[&str], filters: &[&str]| Some((owned(&["Alpha"]), owned(paths), owned(filters)));
        assert_eq!(read("grep -rn Alpha src/*/x.rs"), one(&["src/*/x.rs"], &[]));
        assert_eq!(read("grep -rn Alpha src/mod*/x.rs"), one(&["src/mod*/x.rs"], &[]));
        assert_eq!(read("grep -rn Alpha src/**/foo/*.ts"), one(&["src/**/foo/*.ts"], &[]));
        assert_eq!(read("grep -rn Alpha src/*/"), one(&["src/*/"], &[]));
        assert_eq!(read("grep -rn Alpha 'src/*.rs'"), one(&["src/*.rs"], &[]), "the terminal does not open a quoted star");
        assert_eq!(read("rg Alpha \"src/*.rs\""), one(&["src/*.rs"], &[]));
        assert_eq!(read("grep -rn Alpha a/*.ts b/*.rs"), one(&["a/*.ts", "b/*.rs"], &[]), "one filter cannot tell the folders apart");
        assert_eq!(read("grep -rn Alpha src/*.rs docs"), one(&["src/*.rs", "docs"], &[]));
        assert_eq!(read("grep -rn Alpha src/*.{ts,tsx}"), one(&["src/*.{ts,tsx}"], &[]));
        assert_eq!(read("rg -t rust Alpha src/*.rs"), one(&["src/*.rs"], &["*.rs"]), "an input filter of the line would mix with the path one");
        assert_eq!(read("grep -rn --include=*.md Alpha src/*.rs"), one(&["src/*.rs"], &["*.md"]));
    }

    /// O `git grep` abre o curinga sozinho, também entre aspas: o filtro de
    /// caminho do git, sem pasta, vale pela pasta em que roda. O filtro que
    /// deixa um nome de fora (`:!x`) vira filtro de saída; uma revisão não é
    /// pasta e deixa o caminho como veio.
    #[test]
    fn a_git_grep_pathspec_with_a_wildcard_is_the_folder_and_a_name_filter() {
        let one = |paths: &[&str], filters: &[&str]| Some((owned(&["Alpha"]), owned(paths), owned(filters)));
        assert_eq!(read("git grep -n Alpha -- 'src/*.ts'"), one(&["src"], &["*.ts"]));
        assert_eq!(read("git grep -n Alpha -- 'src/**/*.ts'"), one(&["src"], &["*.ts"]));
        assert_eq!(read("git grep -n Alpha -- '*.prisma'"), one(&["."], &["*.prisma"]));
        assert_eq!(read("git grep -n Alpha -- src/*.ts lib/*.ts"), one(&["src", "lib"], &["*.ts"]));
        assert_eq!(read("git grep -n Alpha -- . ':!*.md'"), one(&["."], &["!*.md"]), "an exclusion is a filter that leaves names out");
        assert_eq!(read("git grep -n Alpha -- src ':^*.md' ':(exclude)*.lock'"), one(&["src"], &["!*.md", "!*.lock"]));
        assert_eq!(read("git grep -n Alpha -- 'src/*.ts' ':!*.d.ts'"), one(&["src"], &["*.ts", "!*.d.ts"]));
        assert_eq!(read("git grep -n Alpha -- ':!*.md'"), one(&[], &["!*.md"]), "only exclusions search the folder it runs in");
        assert_eq!(read("git grep -n Alpha HEAD -- 'src/*.ts'"), one(&["HEAD", "src/*.ts"], &[]), "a revision is not a folder");
    }

    /// A resposta do despachante ao comando `command`, rodado de `cwd`, numa
    /// sessão de nome `session`: o caminho que a sessão usa, pelo registro
    /// dos ganchos.
    fn run_in(cwd: &Path, command: &str, session: Option<&str>) -> Verdict {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: serde_json::json!({ "command": command }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            session_id: session.map(str::to_string),
            ..HookInput::default()
        };
        crate::dispatch::run_event(Some(Trigger::PreToolUse), &input).verdict
    }

    /// O mesmo, numa linha sem sessão: a trava da chave vale, a resposta do
    /// mapa não tem onde guardar a busca e passa.
    fn run(cwd: &Path, command: &str) -> Verdict {
        run_in(cwd, command, None)
    }

    /// O motivo de uma recusa; qualquer outra resposta derruba o teste.
    fn refused(verdict: Verdict, command: &str) -> String {
        match verdict {
            Verdict::Deny { reason } => reason,
            other => panic!("`{command}` is refused, got {other:?}"),
        }
    }

    /// A busca cujos filtros de saída deixam de fora todo o código do mapa nas
    /// pastas buscadas passa: o `rg` com `-g '!*.rs'` num projeto só de Rust,
    /// com chaves, com `**/`, com outro filtro de saída depois e com um de
    /// entrada que o último de saída derruba; o `grep` com `--exclude`.
    #[test]
    fn a_search_whose_exclusions_leave_out_all_the_mapped_code_passes() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for (n, command) in [
            "rg -g '!*.rs' calcular_frete",
            "rg --glob='!*.{rs,toml}' calcular_frete src",
            "rg -g '!**/*.rs' calcular_frete",
            "rg -g '!*.rs' -g '!*.md' calcular_frete",
            "rg -g '*.rs' -g '!*.rs' calcular_frete",
            "cd src && rg -g '!*.rs' calcular_frete .",
            "grep -rn --exclude=*.rs calcular_frete src",
            "grep -rn --include=*.rs --exclude=*.rs calcular_frete",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(run_in(&root, command, Some(&format!("x{n}"))), Verdict::Allow, "{command}");
        }
    }

    /// O `find` como a leitura o vê: as pastas, o teste de nome (`-name`,
    /// `-iname` e o `-path`, que vale para o caminho), se a expressão é uma
    /// conjunção e se ela é só nome e tipo, e o comando do `-exec`.
    #[test]
    fn a_find_is_read_with_its_folders_its_name_test_and_its_exec() {
        let find = |command: &str| segments(command).iter().find_map(find_read).expect("a find");
        let plain = find("find src lib -type f -name '*frete*.rs'");
        assert_eq!(plain.paths, owned(&["src", "lib"]));
        let name = plain.name.as_ref().expect("a name");
        assert_eq!((name.glob.as_str(), name.whole_path, name.ignore_case), ("*frete*.rs", false, false));
        assert!(plain.conjunction && plain.plain && plain.exec.is_empty());
        let named = find("find . -iname '*Frete*'");
        assert!(named.name.as_ref().is_some_and(|name| name.ignore_case), "-iname ignores the case");
        let by_path = find("find -path './src/*frete*'");
        assert!(by_path.paths.is_empty() && by_path.name.as_ref().is_some_and(|name| name.whole_path), "-path is the whole path");
        let with_exec = find("find src -name '*.rs' -exec grep -n calcular_frete {} +");
        assert!(with_exec.plain && with_exec.conjunction, "the exec does not change the tests");
        let words: Vec<&str> = with_exec.exec.iter().map(|word| word.text.as_str()).collect();
        assert_eq!(words, ["grep", "-n", "calcular_frete"], "the exec without the {{}} and the +");
        for command in ["find src -name '*a*' -o -name '*b*'", "find src \\( -name '*a*' \\)", "find src -not -name '*a*'", "find src -name '*a*' -prune"] {
            assert!(!find(command).conjunction, "{command}: the name test does not hold for every file");
        }
        assert!(!find("find src -name '*a*' -mtime -1").plain, "another test is not plain");
        assert!(!find("find src -name '*a*' -name '*b*'").plain, "two name tests are not plain");
        assert!(segments("ls src").iter().find_map(find_read).is_none(), "another program is not a find");
    }

    /// A busca num arquivo só, sem recursão, só em documentos ou fora do
    /// projeto passa.
    #[test]
    fn a_search_in_one_file_or_outside_the_code_passes() {
        let (_dir, root) = word_search::fixture::repo("{}");
        let outside = tempfile::tempdir().expect("tempdir");
        let away = format!("rg calcular_frete {}", outside.path().display());
        for (n, command) in [
            "grep -rn calcular_frete src/frete.rs",
            "grep -n calcular_frete src/frete.rs",
            "rg -g '*.md' calcular_frete",
            "grep -rn calcular_frete docs",
            away.as_str(),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(run_in(&root, command, Some(&format!("u{n}"))), Verdict::Allow, "{command}");
        }
    }

    /// O curinga que a busca não sabe pôr numa pasta deixa a busca passar como
    /// antes: no nome de uma pasta, entre aspas no `grep`, junto de um nome
    /// sem curinga, com nomes diferentes, com chaves.
    #[test]
    fn a_wildcard_the_search_cannot_place_passes_as_before() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for (n, command) in [
            "grep -rn calcular_frete src/*/frete.rs",
            "grep -rn calcular_frete src/fre*/x.rs",
            "grep -rn calcular_frete 'src/*.rs'",
            "rg calcular_frete \"src/*.rs\"",
            "grep -rn calcular_frete src/*.rs docs",
            "grep -rn calcular_frete src/*.rs docs/*.md",
            "grep -rn calcular_frete src/*.{rs,md}",
            "rg -t rust calcular_frete src/*.rs",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(run_in(&root, command, Some(&format!("k{n}"))), Verdict::Allow, "{command}");
        }
    }

    /// O comando que mostraria o `mustard.json` com a chave é recusado, e o
    /// motivo traz o arquivo com a chave trocada, nunca a chave; o que só
    /// lista o arquivo, e o arquivo sem a chave, passam.
    #[test]
    fn the_config_file_with_the_key_never_reaches_the_terminal() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        for command in ["cat mustard.json", "jq . < mustard.json", "grep -n jev ./mustard.json", "cd src && head ../mustard.json"] {
            let reason = refused(run(&root, command), command);
            assert!(!reason.contains(fixture::FAKE_KEY), "`{command}`: the key leaked");
            assert!(reason.contains(r#""key": "***""#), "{command}: {reason}");
        }
        assert_eq!(run(&root, "ls mustard.json"), Verdict::Allow);

        let (_plain, plain) = fixture::project("{}", true);
        assert_eq!(run(&plain, "cat mustard.json"), Verdict::Allow);
    }
    /// A busca recursiva que passaria pelo `mustard.json` com a chave é
    /// recusada com a opção que o deixa de fora, e o motivo nunca traz a
    /// chave: o `grep` recursivo na raiz, depois de um `cd` ou numa pasta
    /// acima dela, também com mais de um padrão, e o `rg` que lê o que o git
    /// ignora.
    #[test]
    fn a_recursive_search_through_the_config_file_with_the_key_is_refused() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        // O terminal come a barra invertida de uma palavra sem aspas: a pasta
        // de cima entra na linha com barras normais, como a pessoa a escreve.
        let above = format!("grep -rn jev {}", crate::shared::paths::canonical(&root.parent().expect("a folder above the project").to_string_lossy()));
        let (grep, rg) = ("`--exclude=mustard.json`", "`-g '!mustard.json'`");
        for (command, fix) in [
            ("grep -r jev .", grep),
            ("grep -rn jev", grep),
            ("cd src && grep -R key ..", grep),
            ("grep -r -e jev -e key .", grep),
            ("grep -r --include=*.json key .", grep),
            (above.as_str(), grep),
            ("rtk rg -u jev", rg),
            ("rg --no-ignore-vcs jev .", rg),
            ("rg -g '*.json' key", rg),
        ] {
            let reason = refused(run(&root, command), command);
            assert!(!reason.contains(fixture::FAKE_KEY), "`{command}`: the key leaked");
            assert!(reason.contains(fix) && reason.contains("mustard.json"), "{command}: {reason}");
        }
    }

    /// O leitor com um curinga sem aspas que o terminal abre e que alcança o
    /// `mustard.json` com a chave recebe o arquivo sem a chave.
    #[test]
    fn a_reader_with_a_wildcard_that_reaches_the_config_file_gets_it_without_the_key() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        for command in ["grep key *.json", "cat *", "cd src && head -5 ../m*"] {
            let reason = refused(run(&root, command), command);
            assert!(!reason.contains(fixture::FAKE_KEY), "`{command}`: the key leaked");
            assert!(reason.contains(r#""key": "***""#), "{command}: {reason}");
        }
    }

    /// A busca que deixa o arquivo de fora, a que não passa pela pasta dele,
    /// a que só lista arquivos ou conta, o `rg` que respeita o que o git
    /// ignora e o curinga entre aspas passam; sem a chave, a busca
    /// recursiva na raiz passa também.
    #[test]
    fn a_search_that_leaves_the_config_file_out_passes() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        for command in [
            "grep -r --exclude=mustard.json jev .",
            "grep -r jev . --exclude=mustard.json",
            "grep -r --include=*.rs jev .",
            "grep -r jev src",
            "grep -rl jev .",
            "grep -rc jev .",
            "rg jev",
            "rg -g '*.rs' key",
            "rg -u jev -g '!mustard.json'",
            "rg -l -g '*.json' key",
            "grep -n key 'm*'",
            "ls *",
        ] {
            assert_eq!(run(&root, command), Verdict::Allow, "{command}");
        }
        let (_plain, plain) = fixture::project("{}", true);
        assert_eq!(run(&plain, "grep -r jev ."), Verdict::Allow);
    }

    /// O `git grep` só lê o que o git rastreia, e o `mustard.json` fica no
    /// `.git/info/exclude`: a busca comum passa, mesmo com um filtro de nome
    /// que o casa. A que lê fora do índice ou o que o git ignora passaria pela
    /// chave e é recusada com o filtro de caminho do git que deixa o arquivo
    /// de fora; a que só lista nomes passa.
    #[test]
    fn a_git_grep_that_reads_what_git_ignores_is_refused_before_it_shows_the_key() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        for command in ["git grep -n jev", "git grep -n key -- '*.json'", "git grep -n jev -- src", "git grep --no-index -l jev", "git grep --no-index -c jev"]
        {
            assert_eq!(run(&root, command), Verdict::Allow, "{command}");
        }
        for command in ["git grep --no-index -n jev", "git grep -n --no-exclude-standard --untracked jev", "git grep --no-index -n jev -- '*.json'"] {
            let reason = refused(run(&root, command), command);
            assert!(!reason.contains(fixture::FAKE_KEY), "`{command}`: the key leaked");
            assert!(reason.contains("`':!mustard.json'`") && reason.contains("mustard.json"), "{command}: {reason}");
        }
        for command in [
            "git grep --no-index -n jev -- . ':!mustard.json'",
            "git grep --no-index -n jev ':^mustard.json'",
            "git grep --no-index -n jev -- ':(exclude)mustard.json'",
        ] {
            assert_eq!(run(&root, command), Verdict::Allow, "{command}: the pathspec that leaves the file out passes");
        }
        refused(run(&root, "git grep --no-index -n jev -- . ':!other.json'"), "an exclusion of another file");
    }
}
