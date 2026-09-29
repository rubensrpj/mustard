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
//! - **A busca por palavra em pastas.** O `grep` recursivo e o `rg`, numa
//!   pasta de código do projeto, recebem a marca do mapa
//!   ([`word_search`]): com o mapa cravado ou parcial, a busca que mostra
//!   linhas é recusada com a resposta agrupada por função no lugar dela; a que
//!   só lista nomes ou conta (`-l`, `-c`) segue, com uma linha da marca; sem
//!   achado, a busca segue com uma linha do que o mapa não achou. A busca num arquivo só, fora do projeto, em
//!   pasta sem código do mapa, com filtros de nome que deixam só documentos ou
//!   que tiram todo o código do mapa (`-g '!*.rs'`, `--exclude=*.rs`), com
//!   opção que esta leitura não entende (`-v`, `-x`) ou com a chave
//!   `search.answer` desligada passa.
//!
//! Lê os comandos que [`super::lex::segments`] achou, nunca o texto cru, e
//! segue os `cd` da linha para saber de que pasta cada caminho parte.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Ctx, HookInput, Verdict};

use super::lex::Segment;
use crate::hooks::write::write_gate::say;
use crate::shared::code_route;
use crate::shared::config_key::{self, NameFilter, Walk, CONFIG_FILE};
use crate::shared::word_search::{self, Dialect, Reply};

/// Os programas que mostram o texto de um arquivo.
const READERS: &[&str] = &[
    "cat", "tac", "nl", "head", "tail", "less", "more", "bat", "grep", "egrep", "fgrep", "rg", "jq", "sed", "awk",
    "cut", "sort", "strings", "xxd", "od", "base64",
];

/// As opções do `grep` que levam um valor: as curtas, e as longas quando o
/// valor vem na palavra seguinte.
const GREP_SHORT_VALUE: &[char] = &['e', 'f', 'm', 'A', 'B', 'C', 'd', 'D'];
const GREP_LONG_VALUE: &[&str] = &[
    "regexp", "file", "max-count", "after-context", "before-context", "context", "include", "exclude",
    "exclude-from", "exclude-dir", "label", "binary-files", "devices", "directories", "group-separator",
];

/// As opções do `rg` que levam um valor, do mesmo jeito.
const RG_SHORT_VALUE: &[char] = &['e', 'f', 'g', 't', 'T', 'm', 'A', 'B', 'C', 'M', 'j', 'r', 'E'];
const RG_LONG_VALUE: &[&str] = &[
    "regexp", "file", "glob", "iglob", "type", "type-not", "type-add", "type-clear", "max-count", "after-context",
    "before-context", "context", "max-columns", "threads", "replace", "encoding", "max-depth", "max-filesize",
    "sort", "sortr", "color", "colors", "context-separator", "field-context-separator", "field-match-separator",
    "path-separator", "pre", "pre-glob", "engine", "dfa-size-limit", "regex-size-limit", "ignore-file",
    "hostname-bin", "hyperlink-format",
];

/// A conferência inteira: a primeira recusa da linha, na ordem dos comandos.
pub(super) fn bash_reading(segments: &[Segment], input: &HookInput, ctx: &Ctx) -> Option<Verdict> {
    let root = ctx.project_dir_or_cwd(input);
    let lang = ctx.config.language().text_or_default();
    let mut cwd = PathBuf::from(input.cwd.as_deref().filter(|cwd| !cwd.is_empty()).unwrap_or(&root));
    let mut note: Option<String> = None;
    for segment in segments {
        if segment.name() == "cd" {
            if let Some(dir) = segment.args.first().map(|word| word.text.as_str()).filter(|dir| !dir.starts_with('-')) {
                cwd = cwd.join(dir);
            }
            continue;
        }
        if let Some(reason) = config_refusal(segment, &cwd, lang) {
            return Some(Verdict::Deny { reason });
        }
        let Some(search) = text_search(segment) else { continue };
        let base = cwd.to_string_lossy();
        let searched = if search.paths.is_empty() { vec![".".to_string()] } else { search.paths.clone() };
        let folders: Vec<code_route::ProjectPath> = searched
            .iter()
            .filter_map(|path| code_route::project_path(&root, &base, path))
            .filter(|path| path.abs.is_dir())
            .collect();
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
            match word_search::hook_reply(&root, input, ctx, &wanted) {
                Reply::Answer(reason) => return Some(Verdict::Deny { reason }),
                Reply::Note(context) => note = Some(context),
                Reply::Pass => {}
            }
        }
        if !search.shows_lines {
            continue;
        }
        let folders: Vec<PathBuf> = searched.iter().map(|path| cwd.join(path)).collect();
        if let Some(file) = config_key::swept(&folders, Path::new(&root), search.walk, &search.filters) {
            let fix = match search.walk {
                Walk::Grep => format!("`--exclude={CONFIG_FILE}`"),
                Walk::Rg { .. } => format!("`-g '!{CONFIG_FILE}'`"),
            };
            return Some(Verdict::Deny { reason: say("config_key.swept", lang, &[("{file}", &file), ("{fix}", &fix)]) });
        }
    }
    note.map(|context| Verdict::Inject { context })
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
struct TextSearch {
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
    paths: Vec<String>,
    /// Os filtros de nome de arquivo, de entrada e de saída, na ordem da
    /// linha: a busca de palavra e a da chave os leem.
    filters: Vec<NameFilter>,
    walk: Walk,
    /// `false` quando a busca só lista arquivos, conta ou fica quieta: a
    /// saída não traz as linhas.
    shows_lines: bool,
}

/// A busca que `segment` faz, quando é um `grep` recursivo ou um `rg`. `None`
/// em todo o resto: outro programa, `grep` sem recursão.
fn text_search(segment: &Segment) -> Option<TextSearch> {
    let (short_value, long_value, rg) = match segment.name() {
        "grep" | "egrep" | "fgrep" => (GREP_SHORT_VALUE, GREP_LONG_VALUE, false),
        "rg" => (RG_SHORT_VALUE, RG_LONG_VALUE, true),
        _ => return None,
    };
    let mut recursive = rg;
    let (mut from_file, mut names_only, mut unignored) = (false, false, false);
    let (mut ignore_case, mut smart_case, mut whole_word, mut unsupported) = (false, false, false, false);
    let mut dialect = match segment.name() {
        "egrep" => Dialect::Extended,
        "fgrep" => Dialect::Fixed,
        _ if rg => Dialect::Rust,
        _ => Dialect::Basic,
    };
    let (mut patterns, mut positionals, mut filters) = (Vec::new(), Vec::new(), Vec::new());
    let mut args = segment.args.iter().map(|word| word.text.as_str());
    let mut options_done = false;
    while let Some(arg) = args.next() {
        if options_done || arg == "-" || !arg.starts_with('-') {
            positionals.push(arg.to_string());
            continue;
        }
        if arg == "--" {
            options_done = true;
            continue;
        }
        let (option, value) = if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = long.split_once('=').map_or((long, None), |(name, value)| (name, Some(value.to_string())));
            let value = value.or_else(|| long_value.contains(&name).then(|| args.next().map(str::to_string)).flatten());
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
            let value = if rest.is_empty() { args.next().map(str::to_string) } else { Some(rest.to_string()) };
            (short.to_string(), value)
        };
        match (option.as_str(), value) {
            ("e" | "regexp", Some(value)) => patterns.push(value),
            ("f" | "file", _) => from_file = true,
            ("recursive" | "dereference-recursive", _) if !rg => recursive = true,
            ("d" | "directories", Some(value)) if !rg && value == "recurse" => recursive = true,
            ("files-with-matches" | "count" | "quiet" | "silent", _) => names_only = true,
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
            ("extended-regexp", _) if !rg => dialect = Dialect::Extended,
            ("perl-regexp", _) if !rg => dialect = Dialect::Rust,
            (
                "invert-match" | "line-regexp" | "null-data" | "multiline" | "type-not" | "T" | "pre" | "exclude-dir"
                | "exclude-from" | "include-from",
                _,
            ) => unsupported = true,
            _ => {}
        }
    }
    if !recursive {
        return None;
    }
    if !from_file && patterns.is_empty() && !positionals.is_empty() {
        patterns.push(positionals.remove(0));
    }
    if from_file {
        patterns.clear();
    }
    // O `-S` do `rg` ignora a caixa quando o padrão não tem maiúscula.
    ignore_case |= smart_case && !patterns.iter().any(|pattern| pattern.chars().any(char::is_uppercase));
    let walk = if rg { Walk::Rg { unignored } } else { Walk::Grep };
    Some(TextSearch { patterns, dialect, ignore_case, whole_word, unsupported, paths: positionals, filters, walk, shows_lines: !names_only })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lex::segments;
    use crate::shared::code_route::fixture;
    use mustard_core::domain::model::contract::Trigger;

    /// Os padrões, os caminhos e os filtros de nome de arquivo que a busca de
    /// uma linha lê, na ordem; o de saída leva o `!` do começo.
    fn read(cmd: &str) -> Option<(Vec<String>, Vec<String>, Vec<String>)> {
        let shown = |filter: &NameFilter| if filter.exclude { format!("!{}", filter.glob) } else { filter.glob.clone() };
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
        assert_eq!(
            read("rg -e Alpha -e Beta src"),
            Some((owned(&["Alpha", "Beta"]), owned(&["src"]), vec![])),
            "each -e is a pattern of the same search"
        );
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

    /// A busca recursiva de um nome do mapa numa pasta de código, pelo `grep`
    /// ou pelo `rg`, é respondida no lugar dela, agrupada por função com a
    /// linha de começo e a de fim, também depois de um `cd`, atrás de um
    /// envoltório, com a opção de caixa e com um filtro do `rg` que só deixa
    /// documentos de fora.
    #[test]
    fn a_recursive_search_for_a_mapped_name_is_answered_by_function() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for (n, command) in [
            "grep -rn calcular_frete src/",
            "rtk rg calcular_frete",
            "cd src && grep -R calcular_frete .",
            "rg -g '*.rs' calcular_frete",
            "rg -g '!*.md' calcular_frete",
            "rg --glob='!*.md' calcular_frete src",
            "rg -g '*.rs' -g '!*.md' calcular_frete",
            "grep -rniE 'CALCULAR_FRETE' .",
        ]
        .into_iter()
        .enumerate()
        {
            let reason = refused(run_in(&root, command, Some(&format!("s{n}"))), command);
            assert!(reason.starts_with("Cravado."), "{command}: {reason}");
            assert!(reason.contains("src/frete.rs\n  2-6 calcular_frete (2)"), "{command}: {reason}");
            assert!(reason.contains("src/pedido.rs\n  1-4 fechar_pedido (2)"), "{command}: {reason}");
        }
    }

    /// A busca que só lista nomes de arquivo ou conta (`-l`, `-c`, `-q`,
    /// `--files-with-matches`, `--count`) roda como veio, com uma linha só da
    /// marca, cravada ou parcial. Ela não vale como respondida: a busca que
    /// mostra as linhas, logo depois, recebe a resposta por função.
    #[test]
    fn a_search_that_only_lists_names_or_counts_runs_plain_with_one_line_of_the_mark() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for command in [
            "grep -rl calcular_frete src/",
            "grep -rc calcular_frete .",
            "rg -l calcular_frete",
            "rg --count calcular_frete src",
            "grep -r --files-with-matches calcular_frete .",
        ] {
            match run_in(&root, command, Some("nomes")) {
                Verdict::Inject { context } => {
                    assert_eq!(context.lines().count(), 1, "{command}: {context}");
                    assert!(context.starts_with("Cravado."), "{command}: {context}");
                    assert!(!context.contains("src/frete.rs"), "{command}: no answer goes with it: {context}");
                }
                other => panic!("{command}: the plain search runs with a line, got {other:?}"),
            }
        }
        match run_in(&root, "grep -rlE 'calcular_frete|imposto' .", Some("nomes")) {
            Verdict::Inject { context } => {
                assert!(context.starts_with("Cravado.") && !context.contains("imposto"), "{context}");
            }
            other => panic!("the pinned search runs with a line, got {other:?}"),
        }
        let reason = refused(run_in(&root, "grep -rn calcular_frete src/", Some("nomes")), "the search that shows lines");
        assert!(reason.contains("src/frete.rs\n  2-6 calcular_frete (2)"), "{reason}");
    }

    /// O mesmo nome buscado por quem escreve o padrão em inglês recebe a
    /// resposta na língua do texto do projeto.
    #[test]
    fn the_answer_speaks_the_text_language_of_the_project() {
        let (_dir, root) = word_search::fixture::repo(r#"{"language":{"text":"en-US"}}"#);
        let reason = refused(run_in(&root, "grep -rn calcular_frete src", Some("en")), "an english project");
        assert!(reason.starts_with("Pinned."), "{reason}");
        assert!(reason.contains("Each function comes with its first and last line"), "{reason}");
    }

    /// Com o texto do projeto em português e os nomes do código em inglês, a
    /// palavra `frete` acha `getFrete` e `users` acha `UserRepository`: as
    /// palavras da busca passam pela mesma normalização das duas línguas.
    #[test]
    fn a_word_in_the_text_language_finds_the_names_in_the_code_language() {
        let config = r#"{"language":{"text":"pt-BR","code":"en-US"}}"#;
        let files = [
            ("src/frete.ts", "export function getFrete(peso: number): number {\n  return peso * 2;\n}\n"),
            ("src/user.ts", "export class UserRepository {\n  find(id: string) { return id; }\n}\n"),
        ];
        let map = serde_json::json!({ "modules": [
            { "path": "src/frete.ts", "language": "typescript", "declarations": [
                { "kind": "function", "name": "getFrete", "line": 1, "end_line": 3 }] },
            { "path": "src/user.ts", "language": "typescript", "declarations": [
                { "kind": "class", "name": "UserRepository", "line": 1, "end_line": 3 }] }
        ] });
        let (_dir, root) = word_search::fixture::repo_with(config, &files, map);
        let frete = refused(run_in(&root, "grep -rni frete src", Some("frete")), "frete finds getFrete");
        assert!(frete.contains("src/frete.ts\n  1-3 getFrete (1)"), "{frete}");
        let users = refused(run_in(&root, "grep -rn users src", Some("users")), "users finds UserRepository");
        assert!(users.contains("`src/user.ts`"), "{users}");
    }

    /// A busca com palavra que o primeiro arquivo do mapa não traz em campo
    /// forte continua cravada e não pede nova busca, no `grep` básico (`\|`),
    /// no estendido e no `rg`.
    #[test]
    fn a_search_with_a_word_the_map_lacks_is_answered_as_pinned() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for (n, command) in [
            r"grep -r 'calcular_frete\|imposto' .",
            "grep -rE 'calcular_frete|imposto' .",
            "rg 'calcular_frete|imposto'",
            "rg -e calcular_frete -e imposto",
        ]
        .into_iter()
        .enumerate()
        {
            let reason = refused(run_in(&root, command, Some(&format!("p{n}"))), command);
            assert!(reason.starts_with("Cravado."), "{command}: {reason}");
            assert!(!reason.contains("Falta"), "{command}: {reason}");
            assert!(reason.contains("src/frete.rs\n  2-6 calcular_frete (2, 3)"), "{command}: {reason}");
            assert!(reason.contains("docs/notas.md\n  1: O calcular_frete soma o imposto."), "{command}: {reason}");
        }
    }

    /// Sem achado no mapa a busca comum roda, com uma linha do que o mapa não
    /// achou; repetida, roda sem a linha.
    #[test]
    fn a_search_the_map_finds_nothing_for_runs_plain_with_one_line() {
        let (_dir, root) = word_search::fixture::repo("{}");
        let command = "grep -rn zzznada src";
        match run_in(&root, command, Some("nada")) {
            Verdict::Inject { context } => {
                assert_eq!(context.lines().count(), 1, "{context}");
                assert!(context.starts_with("Não achei"), "{context}");
                assert!(context.contains(r#"grep -rniE "zzznada" ."#), "{context}");
            }
            other => panic!("the plain search runs with a line, got {other:?}"),
        }
        assert_eq!(run_in(&root, command, Some("nada")), Verdict::Allow);
    }

    /// A mesma busca repetida na sessão passa, com outro programa, outra
    /// forma de mostrar ou outro filtro; outra pasta, outro padrão ou outra
    /// sessão é outra busca e recebe a resposta.
    #[test]
    fn the_same_search_repeated_in_the_session_passes() {
        let (_dir, root) = word_search::fixture::repo("{}");
        refused(run_in(&root, "grep -rn calcular_frete src", Some("rep")), "the first search");
        for command in ["grep -rn calcular_frete src", "rg -n calcular_frete src", "grep -rl calcular_frete src", "rg -i calcular_frete src"] {
            assert_eq!(run_in(&root, command, Some("rep")), Verdict::Allow, "{command}");
        }
        for command in ["grep -rn calcular_frete .", "grep -rn fechar_pedido src"] {
            refused(run_in(&root, command, Some("rep")), command);
        }
        refused(run_in(&root, "grep -rn calcular_frete src", Some("outra")), "another session");
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

    /// A busca cujos filtros de saída não deixam o código do mapa de fora é
    /// respondida: o filtro de outro tipo de arquivo, o de um arquivo só, o de
    /// entrada que vem depois do de saída e traz o código de volta, as chaves
    /// no `grep` (que as lê como texto) e, num projeto de duas linguagens, o
    /// filtro que tira só uma delas. O filtro com pasta, que a leitura não
    /// entende, deixa a busca comum passar.
    #[test]
    fn a_search_whose_exclusions_leave_mapped_code_in_is_answered() {
        let (_dir, root) = word_search::fixture::repo("{}");
        for (n, command) in [
            "rg -g '!*.md' calcular_frete",
            "rg -g '!frete.rs' calcular_frete",
            "rg -g '!*.rs' -g '*.rs' calcular_frete",
            "rg -g '!*.rs' -g '*.md' -g '*.rs' calcular_frete",
            "grep -rn --exclude='*.{rs,md}' calcular_frete src",
            "grep -rn --exclude=*.md calcular_frete",
        ]
        .into_iter()
        .enumerate()
        {
            let reason = refused(run_in(&root, command, Some(&format!("y{n}"))), command);
            assert!(reason.contains("calcular_frete"), "{command}: {reason}");
        }
        assert_eq!(run_in(&root, "rg -g '!src/*.rs' calcular_frete", Some("pasta")), Verdict::Allow);
        std::fs::create_dir_all(root.join("web")).expect("web");
        std::fs::write(root.join("web/app.ts"), "export function renderizar_tela() {}\n").expect("app");
        let two = serde_json::json!({ "modules": [
            { "path": "src/frete.rs", "declarations": [{ "kind": "function", "name": "calcular_frete", "line": 2, "end_line": 6 }] },
            { "path": "web/app.ts", "declarations": [{ "kind": "function", "name": "renderizar_tela", "line": 1, "end_line": 1 }] }
        ] });
        mustard_core::io::project_map::write_text(&root, &two.to_string()).expect("map");
        let reason = refused(run_in(&root, "rg -g '!*.rs' renderizar_tela", Some("dois")), "the filter that leaves the ts");
        assert!(reason.contains("web/app.ts\n  1-1 renderizar_tela (1)"), "{reason}");
        assert_eq!(run_in(&root, "rg -g '!*.rs' -g '!*.ts' renderizar_tela", Some("dois-b")), Verdict::Allow);
        refused(run_in(&root, "rg -g '!*.rs' -g '!*.ts' -g '*.ts' renderizar_tela", Some("dois-c")), "an input filter after the output ones");
    }

    /// A chave `search.answer` desligada, o projeto sem mapa, a linha sem
    /// sessão, um regex de referência de volta, uma opção que muda o que a
    /// busca acha (`-v`), o `-u` do `rg` e um `-f` de arquivo deixam a busca
    /// comum passar, sem erro.
    #[test]
    fn a_search_the_answer_cannot_stand_for_passes_without_error() {
        let (_off, off) = word_search::fixture::repo(r#"{"search":{"answer":false}}"#);
        assert_eq!(run_in(&off, "grep -rn calcular_frete src", Some("off")), Verdict::Allow);
        let (_on, on) = word_search::fixture::repo(r#"{"search":{"answer":true}}"#);
        refused(run_in(&on, "grep -rn calcular_frete src", Some("on")), "the key on");

        let (_bare, bare) = word_search::fixture::repo("{}");
        std::fs::remove_file(mustard_core::io::project_map::model_path(&bare)).expect("no map");
        assert_eq!(run_in(&bare, "grep -rn calcular_frete src", Some("bare")), Verdict::Allow);

        let (_dir, root) = word_search::fixture::repo("{}");
        assert_eq!(run(&root, "grep -rn calcular_frete src"), Verdict::Allow, "no session, no answer");
        for (n, command) in [
            r"grep -rn '\(calcular\)\1' src",
            "grep -rnv calcular_frete src",
            "rg -u calcular_frete",
            "grep -rf patterns.txt src",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(run_in(&root, command, Some(&format!("z{n}"))), Verdict::Allow, "{command}");
        }
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

    /// Numa cópia de trabalho do projeto, a busca lê a árvore da cópia: o
    /// arquivo que a onda mudou vem relido, marcado como mudado e com as
    /// linhas da cópia; o que ela não mexeu vem sem marca.
    #[test]
    fn a_search_inside_a_working_copy_rereads_the_files_the_wave_changed() {
        let (dir, root) = word_search::fixture::repo("{}");
        let copy = dir.path().parent().expect("parent").join(format!("copia-bash-{}", std::process::id()));
        word_search::fixture::git(&root, &["worktree", "add", "-q", &copy.to_string_lossy(), "-b", "onda"]);
        let copy = std::fs::canonicalize(&copy).expect("copy");
        std::fs::write(copy.join("src/frete.rs"), format!("// a\n// b\n{}", word_search::fixture::FRETE)).expect("edit");
        let reason = refused(run_in(&copy, "grep -rn calcular_frete src", Some("copia")), "a search in the working copy");
        assert!(reason.contains("src/frete.rs (mudado nesta onda)\n  4-8 calcular_frete (4)"), "{reason}");
        assert!(reason.contains("src/pedido.rs\n  1-4 fechar_pedido (2)"), "{reason}");
        word_search::fixture::git(&root, &["worktree", "remove", "--force", &copy.to_string_lossy()]);
    }

    /// A busca que passa pelo `mustard.json` com a chave: com a resposta do
    /// mapa no lugar da busca, o arquivo fica fora e a chave nunca aparece;
    /// quando a busca comum roda, a trava da chave recusa como antes.
    #[test]
    fn the_answer_never_carries_the_key_and_the_plain_search_still_hides_it() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = word_search::fixture::repo(&config);
        let answered = refused(run_in(&root, "grep -r calcular_frete .", Some("chave")), "the answer");
        assert!(answered.starts_with("Cravado.") && !answered.contains(fixture::FAKE_KEY), "{answered}");
        let swept = refused(run_in(&root, "grep -r zzznada .", Some("chave")), "the plain search through the key file");
        assert!(swept.contains("--exclude=mustard.json") && !swept.contains(fixture::FAKE_KEY), "{swept}");
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
        let above = format!("grep -rn jev {}", root.parent().expect("a folder above the project").display());
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
}
