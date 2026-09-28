//! `reading` — a quarta conferência da trava de comandos: a leitura e a busca
//! pelo terminal que o Mustard responde melhor, ou que mostrariam a chave do
//! Jev.
//!
//! - **O arquivo de configuração com a chave.** Um programa que mostra o
//!   texto de um arquivo ([`READERS`]) com o `mustard.json` nos argumentos,
//!   ou qualquer programa que o recebe por `<`, é recusado quando o arquivo
//!   guarda a chave. O motivo traz o arquivo com a chave trocada por `***`.
//! - **A busca de um nome em pastas.** O `grep` recursivo e o `rg` numa
//!   pasta de código, com um padrão que é nome de declaração do mapa, voltam
//!   com o comando de quem usa o nome ([`code_route::folder_search`]). A
//!   busca num arquivo só, fora do projeto, em pasta sem código do mapa ou
//!   com um filtro de nome de arquivo que deixa só documentos passa.
//!
//! Lê os comandos que [`super::lex::segments`] achou, nunca o texto cru, e
//! segue os `cd` da linha para saber de que pasta cada caminho parte.

use std::path::{Path, PathBuf};

use mustard_core::domain::model::contract::{Ctx, HookInput, Verdict};

use super::lex::Segment;
use crate::shared::{code_route, config_key};

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
        let folders: Vec<String> = searched
            .iter()
            .filter_map(|path| code_route::project_path(&root, &base, path))
            .filter(|path| path.abs.is_dir())
            .map(|path| path.rel)
            .collect();
        if let Some(reason) = code_route::folder_search(Path::new(&root), &search.pattern, &folders, &search.globs, lang)
        {
            return Some(Verdict::Deny { reason });
        }
    }
    None
}

/// A recusa do comando que mostraria o arquivo de configuração com a chave:
/// um leitor com o arquivo nos argumentos, ou qualquer programa com o arquivo
/// entrando por `<`.
fn config_refusal(segment: &Segment, cwd: &Path, lang: mustard_core::platform::i18n::Locale) -> Option<String> {
    let reader = READERS.contains(&segment.name());
    let named = segment.args.iter().filter(|_| reader).map(|word| word.text.as_str());
    let fed = segment.redirects.iter().filter(|redirect| redirect.op == "<").map(|redirect| redirect.target.text.as_str());
    named
        .chain(fed)
        .filter(|path| config_key::is_config_file(path))
        .find_map(|path| config_key::refusal(path, &cwd.join(path), lang))
}

/// Uma busca de texto do terminal: o padrão, os caminhos e os filtros de nome
/// de arquivo.
struct TextSearch {
    pattern: String,
    paths: Vec<String>,
    globs: Vec<String>,
}

/// A busca que `segment` faz, quando é um `grep` recursivo ou um `rg` com um
/// padrão só. `None` em todo o resto: outro programa, `grep` sem recursão,
/// padrões lidos de arquivo, mais de um padrão.
fn text_search(segment: &Segment) -> Option<TextSearch> {
    let (short_value, long_value, rg) = match segment.name() {
        "grep" | "egrep" | "fgrep" => (GREP_SHORT_VALUE, GREP_LONG_VALUE, false),
        "rg" => (RG_SHORT_VALUE, RG_LONG_VALUE, true),
        _ => return None,
    };
    let mut recursive = rg;
    let mut from_file = false;
    let (mut patterns, mut positionals, mut globs) = (Vec::new(), Vec::new(), Vec::new());
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
            let Some((at, short)) = cluster.char_indices().find(|(_, c)| short_value.contains(c)) else {
                recursive |= !rg && cluster.contains(['r', 'R']);
                continue;
            };
            recursive |= !rg && cluster[..at].contains(['r', 'R']);
            let rest = &cluster[at + short.len_utf8()..];
            let value = if rest.is_empty() { args.next().map(str::to_string) } else { Some(rest.to_string()) };
            (short.to_string(), value)
        };
        match (option.as_str(), value) {
            ("e" | "regexp", Some(value)) => patterns.push(value),
            ("f" | "file", _) => from_file = true,
            ("recursive" | "dereference-recursive", _) if !rg => recursive = true,
            ("d" | "directories", Some(value)) if !rg && value == "recurse" => recursive = true,
            ("include", Some(value)) if !rg => globs.push(value),
            ("g" | "glob" | "iglob", Some(value)) if rg => globs.push(value),
            _ => {}
        }
    }
    if from_file || !recursive {
        return None;
    }
    if patterns.is_empty() && !positionals.is_empty() {
        patterns.push(positionals.remove(0));
    }
    let [pattern] = <[String; 1]>::try_from(patterns).ok()?;
    Some(TextSearch { pattern, paths: positionals, globs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lex::segments;
    use crate::shared::code_route::fixture;
    use mustard_core::domain::model::contract::Trigger;

    /// O padrão, os caminhos e os filtros que a busca de uma linha lê.
    fn read(cmd: &str) -> Option<(String, Vec<String>, Vec<String>)> {
        segments(cmd).iter().find_map(text_search).map(|s| (s.pattern, s.paths, s.globs))
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
    /// o nome, também depois de um `cd` e atrás de um envoltório.
    #[test]
    fn a_recursive_search_for_a_declared_name_is_refused_with_its_users() {
        let (_dir, root) = fixture::project("{}", true);
        for command in ["grep -rn Alpha src/", "rtk rg Alpha", "cd src && grep -R alpha .", "rg -g '*.rs' Alpha"] {
            let reason = refused(run(&root, command), command);
            assert!(reason.contains("`mustard-rt run map users --name "), "{command}: {reason}");
        }
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
}
