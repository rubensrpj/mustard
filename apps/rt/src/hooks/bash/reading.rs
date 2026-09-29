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
//! - **A busca de um nome em pastas.** O `grep` recursivo e o `rg` numa
//!   pasta de código, com um padrão que é nome de declaração do mapa, voltam
//!   com o comando de quem usa o nome ([`code_route::folder_search`]). A
//!   busca num arquivo só, fora do projeto, em pasta sem código do mapa,
//!   com um filtro de nome de arquivo que deixa só documentos ou com filtros
//!   de saída (`-g '!*.rs'`, `--exclude=*.rs`) que tiram todo o código do
//!   mapa das pastas passa.
//!
//! Lê os comandos que [`super::lex::segments`] achou, nunca o texto cru, e
//! segue os `cd` da linha para saber de que pasta cada caminho parte.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Ctx, HookInput, Verdict};

use super::lex::Segment;
use crate::hooks::write::write_gate::say;
use crate::shared::config_key::{self, NameFilter, Walk, CONFIG_FILE};
use crate::shared::code_route;

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
        let searched = if search.paths.is_empty() { vec![".".to_string()] } else { search.paths };
        if let Some(pattern) = &search.pattern {
            let folders: Vec<String> = searched
                .iter()
                .filter_map(|path| code_route::project_path(&root, &base, path))
                .filter(|path| path.abs.is_dir())
                .map(|path| path.rel)
                .collect();
            if let Some(reason) = code_route::folder_search(Path::new(&root), pattern, &folders, &search.filters, search.walk, lang) {
                return Some(Verdict::Deny { reason });
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
    None
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
    /// O padrão, quando há um só e escrito na linha; `None` com mais de um ou
    /// com os padrões lidos de arquivo.
    pattern: Option<String>,
    paths: Vec<String>,
    /// Os filtros de nome de arquivo, de entrada e de saída, na ordem da
    /// linha: a busca de um nome e a da chave os leem.
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
            ("files-with-matches" | "files-without-match" | "count" | "quiet" | "silent", _) => names_only = true,
            ("count-matches" | "files", _) if rg => names_only = true,
            ("no-ignore" | "no-ignore-vcs" | "no-ignore-exclude" | "unrestricted", _) if rg => unignored = true,
            ("include", Some(value)) if !rg => filters.push(NameFilter { exclude: false, glob: value }),
            ("exclude", Some(value)) if !rg => filters.push(NameFilter { exclude: true, glob: value }),
            ("g" | "glob" | "iglob", Some(value)) if rg => filters.push(NameFilter::rg(&value)),
            _ => {}
        }
    }
    if !recursive {
        return None;
    }
    if !from_file && patterns.is_empty() && !positionals.is_empty() {
        patterns.push(positionals.remove(0));
    }
    let pattern = <[String; 1]>::try_from(patterns).ok().map(|[pattern]| pattern).filter(|_| !from_file);
    let walk = if rg { Walk::Rg { unignored } } else { Walk::Grep };
    Some(TextSearch { pattern, paths: positionals, filters, walk, shows_lines: !names_only })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lex::segments;
    use crate::shared::code_route::fixture;
    use mustard_core::domain::model::contract::Trigger;

    /// O padrão, os caminhos e os filtros de nome de arquivo que a busca de
    /// uma linha lê, na ordem; o de saída leva o `!` do começo.
    fn read(cmd: &str) -> Option<(String, Vec<String>, Vec<String>)> {
        let shown = |filter: &NameFilter| if filter.exclude { format!("!{}", filter.glob) } else { filter.glob.clone() };
        segments(cmd).iter().find_map(text_search).and_then(|s| Some((s.pattern?, s.paths, s.filters.iter().map(shown).collect())))
    }

    /// O `grep` só conta como busca em pastas com a recursão; o `rg`, sempre.
    /// As opções com valor não viram padrão nem caminho, e o valor colado na
    /// opção também se lê.
    #[test]
    fn the_search_of_a_line_is_read_like_the_program_reads_it() {
        let owned = |items: &[&str]| items.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert_eq!(read("grep -rn Alpha src/"), Some(("Alpha".into(), owned(&["src/"]), vec![])));
        assert_eq!(read("grep -n Alpha src/a.rs"), None, "without recursion grep reads the files named");
        assert_eq!(read("grep -A 3 -R --include=*.md Alpha docs"), Some(("Alpha".into(), owned(&["docs"]), owned(&["*.md"]))));
        assert_eq!(read("grep -rA3 Alpha"), Some(("Alpha".into(), vec![], vec![])));
        assert_eq!(read("grep -d recurse -e Alpha ."), Some(("Alpha".into(), owned(&["."]), vec![])));
        assert_eq!(read("rtk rg -n -g '*.rs' Alpha apps/scan"), Some(("Alpha".into(), owned(&["apps/scan"]), owned(&["*.rs"]))));
        assert_eq!(read("rg --type rust -C 2 Alpha"), Some(("Alpha".into(), vec![], vec![])));
        assert_eq!(read("rg -g '!*.md' Alpha"), Some(("Alpha".into(), vec![], owned(&["!*.md"]))), "a negated filter leaves files out");
        assert_eq!(
            read("rg --glob='!*.md' -g '*.rs' --iglob '!*.txt' Alpha"),
            Some(("Alpha".into(), vec![], owned(&["!*.md", "*.rs", "!*.txt"])))
        );
        assert_eq!(read("grep -r --exclude=*.rs --include=*.md Alpha"), Some(("Alpha".into(), vec![], owned(&["!*.rs", "*.md"]))));
        assert_eq!(read("rg -e Alpha -e Beta src"), None, "two patterns are not one name");
        assert_eq!(read("grep -rf patterns.txt src"), None, "patterns from a file are unknown");
        assert_eq!(read("cat x | grep Alpha"), None);
        assert_eq!(read("rg -- -Alpha src"), Some(("-Alpha".into(), owned(&["src"]), vec![])));
    }

    /// A resposta do despachante ao comando `command`, rodado de `cwd`: o
    /// caminho que a sessão usa, pelo registro dos ganchos.
    fn run(cwd: &Path, command: &str) -> Verdict {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: serde_json::json!({ "command": command }),
            hook_event_name: Some("PreToolUse".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            ..HookInput::default()
        };
        crate::dispatch::run_event(Some(Trigger::PreToolUse), &input).verdict
    }

    /// O motivo de uma recusa; qualquer outra resposta derruba o teste.
    fn refused(verdict: Verdict, command: &str) -> String {
        match verdict {
            Verdict::Deny { reason } => reason,
            other => panic!("`{command}` is refused, got {other:?}"),
        }
    }

    /// A busca recursiva de um nome de declaração do mapa numa pasta de
    /// código, pelo `grep` ou pelo `rg`, é recusada com o comando de quem usa
    /// o nome, também depois de um `cd`, atrás de um envoltório e com um
    /// filtro do `rg` que só deixa documentos de fora.
    #[test]
    fn a_recursive_search_for_a_declared_name_is_refused_with_its_users() {
        let (_dir, root) = fixture::project("{}", true);
        for command in [
            "grep -rn Alpha src/",
            "rtk rg Alpha",
            "cd src && grep -R alpha .",
            "rg -g '*.rs' Alpha",
            "rg -g '!*.md' Alpha",
            "rg --glob='!*.md' Alpha src",
            "rg -g '*.rs' -g '!*.md' Alpha",
        ] {
            let reason = refused(run(&root, command), command);
            assert!(reason.contains("`mustard-rt run map users --name "), "{command}: {reason}");
        }
    }

    /// O mapa de um projeto de duas linguagens: `Alpha` em `src/big.rs` e
    /// `render` em `web/app.ts`.
    const TWO_LANGUAGES: &str = r#"{"modules":[
        {"path":"src/big.rs","declarations":[{"kind":"struct","name":"Alpha","line":1,"end_line":150}]},
        {"path":"web/app.ts","declarations":[{"kind":"function","name":"render","line":1,"end_line":20}]}
    ]}"#;

    /// A busca de um nome cujos filtros de saída deixam de fora todo o código
    /// do mapa nas pastas buscadas passa: o `rg` com `-g '!*.rs'` num
    /// projeto só de Rust, com chaves, com `**/`, com outro filtro de saída
    /// depois e com um de entrada que o último de saída derruba; o `grep` com
    /// `--exclude`.
    #[test]
    fn a_search_whose_exclusions_leave_out_all_the_mapped_code_passes() {
        let (_dir, root) = fixture::project("{}", true);
        for command in [
            "rg -g '!*.rs' Alpha",
            "rg --glob='!*.{rs,toml}' Alpha src",
            "rg -g '!**/*.rs' Alpha",
            "rg -g '!*.rs' -g '!*.md' Alpha",
            "rg -g '*.rs' -g '!*.rs' Alpha",
            "cd src && rg -g '!*.rs' alpha .",
            "grep -rn --exclude=*.rs Alpha src",
            "grep -rn --include=*.rs --exclude=*.rs Alpha",
        ] {
            assert_eq!(run(&root, command), Verdict::Allow, "{command}");
        }
    }

    /// A busca cujos filtros de saída não deixam o código do mapa de fora
    /// segue recusada: o filtro de outro tipo de arquivo, o de um arquivo só,
    /// o com pasta, o de entrada que vem depois do de saída e traz o código
    /// de volta, as chaves no `grep` (que as lê como texto) e, num projeto de
    /// duas linguagens, o filtro que tira só uma delas.
    #[test]
    fn a_search_whose_exclusions_leave_mapped_code_in_is_still_refused() {
        let (_dir, root) = fixture::project("{}", true);
        for command in [
            "rg -g '!*.md' Alpha",
            "rg -g '!big.rs' Alpha",
            "rg -g '!src/*.rs' Alpha",
            "rg -g '!*.rs' -g '*.rs' Alpha",
            "rg -g '!*.rs' -g 'src/**' Alpha",
            "rg -g '!*.rs' -g '*.md' -g '*.rs' Alpha",
            "grep -rn --exclude='*.{rs,md}' Alpha src",
            "grep -rn --exclude=*.md Alpha",
        ] {
            let reason = refused(run(&root, command), command);
            assert!(reason.contains("`mustard-rt run map users --name "), "{command}: {reason}");
        }
        mustard_core::io::project_map::write_text(&root, TWO_LANGUAGES).expect("map");
        let reason = refused(run(&root, "rg -g '!*.rs' render"), "rg -g '!*.rs' render");
        assert!(reason.contains("`mustard-rt run map users --name render`"), "{reason}");
        assert_eq!(run(&root, "rg -g '!*.rs' -g '!*.ts' render"), Verdict::Allow);
        refused(run(&root, "rg -g '!*.rs' -g '!*.ts' -g '*.ts' render"), "an input filter after the output ones");
    }

    /// A busca que a rota deixa passar continua sob a trava da chave: com
    /// `-g '*.json'` a busca só lê documentos e configuração, e passa pela
    /// rota, mas traz o `mustard.json` com a chave e é recusada sem mostrá-la.
    #[test]
    fn a_search_the_route_lets_pass_still_hides_the_key() {
        let config = format!(r#"{{"jev": {{"key": "{}"}}}}"#, fixture::FAKE_KEY);
        let (_dir, root) = fixture::project(&config, true);
        let command = "rg -g '!*.rs' -g '*.json' -u Alpha";
        let reason = refused(run(&root, command), command);
        assert!(!reason.contains(fixture::FAKE_KEY), "the key leaked: {reason}");
        assert!(reason.contains("mustard.json"), "{reason}");
        assert_eq!(run(&root, "rg -g '!*.rs' Alpha"), Verdict::Allow);
    }

    /// A busca num arquivo só, sem recursão, de um texto que não é nome, de
    /// um nome que o mapa não conhece, só em documentos ou fora do projeto
    /// passa; sem mapa, a busca de um nome também passa.
    #[test]
    fn a_search_in_one_file_or_for_other_text_passes() {
        let (_dir, root) = fixture::project("{}", true);
        let outside = tempfile::tempdir().expect("tempdir");
        let away = format!("rg Alpha {}", outside.path().display());
        for command in [
            "grep -rn Alpha src/big.rs",
            "grep -n Alpha src/big.rs",
            "grep -r \"fn alpha\" src",
            "grep -rn Unknown src",
            "rg -g '*.md' Alpha",
            "grep -rn Alpha docs",
            away.as_str(),
        ] {
            assert_eq!(run(&root, command), Verdict::Allow, "{command}");
        }
        let (_bare, bare) = fixture::project("{}", false);
        assert_eq!(run(&bare, "grep -rn Alpha src/"), Verdict::Allow);
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
