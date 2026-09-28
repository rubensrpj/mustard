//! `config_key` — a chave do Jev no arquivo de configuração do Mustard, que
//! nenhuma leitura mostra.
//!
//! A chave mora no `mustard.json` do projeto, no campo [`JEV_KEY_FIELD`] da
//! seção `jev`, o mesmo nome que a configuração lê. Quem lê o arquivo, pela
//! ferramenta de leitura ou pelo terminal, recebe o texto dele com o valor
//! desse campo trocado por `***` ([`masked`]), e nunca o valor. A busca em
//! pastas que passaria pelo arquivo e mostraria as linhas dele é recusada
//! antes de rodar ([`swept`]).

use std::path::{Path, PathBuf};

use mustard_core::domain::config::JEV_KEY_FIELD;
use mustard_core::platform::i18n::{translate, Locale};

/// O nome do arquivo de configuração do Mustard.
pub(crate) const CONFIG_FILE: &str = "mustard.json";

/// O que fica no lugar do valor de cada chave.
const HIDDEN: &str = "***";

/// `true` quando o caminho termina no arquivo de configuração do Mustard.
pub(crate) fn is_config_file(path: &str) -> bool {
    Path::new(&path.replace('\\', "/")).file_name().and_then(|name| name.to_str()) == Some(CONFIG_FILE)
}

/// Um pedaço do texto, para achar o campo de chave: um texto entre aspas,
/// com as posições do que fica dentro delas, os dois-pontos, ou outra coisa.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    Text(usize, usize),
    Colon,
    Other,
}

/// `text` com o valor de cada campo [`JEV_KEY_FIELD`] trocado por `***`,
/// quando há algum com valor; `None` quando não há. Lê só os textos entre aspas e os
/// dois-pontos, e por isso acha a chave também num arquivo que não se lê como
/// JSON.
pub(crate) fn masked(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut hidden: Vec<(usize, usize)> = Vec::new();
    let mut last: [Token; 2] = [Token::Other, Token::Other];
    let mut at = 0;
    while at < bytes.len() {
        let token = match bytes[at] {
            b'"' => {
                let start = at + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(bytes.len());
                at = end + 1;
                Token::Text(start, end)
            }
            b':' => {
                at += 1;
                Token::Colon
            }
            byte if byte.is_ascii_whitespace() => {
                at += 1;
                continue;
            }
            _ => {
                at += 1;
                Token::Other
            }
        };
        if let (Token::Text(name_start, name_end), Token::Colon, Token::Text(start, end)) = (last[0], last[1], token)
            && end > start
            && &text[name_start..name_end] == JEV_KEY_FIELD
        {
            hidden.push((start, end));
        }
        last = [last[1], token];
    }
    if hidden.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for (start, end) in hidden {
        out.push_str(&text[from..start]);
        out.push_str(HIDDEN);
        from = end;
    }
    out.push_str(&text[from..]);
    Some(out)
}

/// A recusa da leitura do arquivo de configuração em `path` (como quem lê o
/// nomeou), quando ele guarda uma chave: o texto dele com a chave escondida.
/// `None` quando o arquivo não se lê ou não guarda chave.
pub(crate) fn refusal(path: &str, on_disk: &Path, lang: Locale) -> Option<String> {
    let shown = masked(&std::fs::read_to_string(on_disk).ok()?)?;
    Some(translate("config_key.hidden", lang).replace("{file}", path).replace("{text}", &shown))
}

/// Como uma busca em pastas escolhe os arquivos que lê.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Walk {
    /// O `grep` recursivo: lê todo arquivo, e só os filtros de nome deixam um
    /// de fora. O último filtro que casa com o nome decide; sem nenhum que
    /// case, o arquivo entra, salvo quando o primeiro filtro é de entrada. As
    /// chaves (`{a,b}`) são texto, não opções.
    Grep,
    /// O `rg` e a ferramenta de busca: deixam de fora o que o git ignora, e o
    /// `mustard.json` com isso, pois o instalador o põe no
    /// `.git/info/exclude`. Com `unignored` (`-u`, `--no-ignore`, …) leem
    /// tudo. O último filtro que casa decide, e o de entrada que casa com o
    /// nome passa por cima do que o git ignora; sem nenhum que case, um
    /// filtro de entrada deixa o arquivo de fora.
    Rg { unignored: bool },
}

/// Um filtro de nome de arquivo de uma busca: o que deixa entrar
/// (`--include`, `-g x`) ou, com `exclude`, o que deixa de fora
/// (`--exclude`, `-g '!x'`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameFilter {
    pub(crate) exclude: bool,
    pub(crate) glob: String,
}

impl NameFilter {
    /// Um filtro do `rg`, em que o `!` do começo deixa de fora.
    pub(crate) fn rg(glob: &str) -> Self {
        match glob.strip_prefix('!') {
            Some(rest) => Self { exclude: true, glob: rest.to_string() },
            None => Self { exclude: false, glob: glob.to_string() },
        }
    }
}

/// O arquivo de configuração que a palavra `text` do terminal alcança: ela
/// mesma, quando nomeia o arquivo, ou o da pasta dela, quando é um curinga
/// (`*`, `?`, `{a,b}`) que o terminal abre e que pode casar com o nome do
/// arquivo. `plain` diz que a palavra veio sem aspas: só assim o terminal a
/// abre. O caminho vem como a palavra o daria.
pub(crate) fn named_by(text: &str, plain: bool) -> Option<String> {
    if is_config_file(text) {
        return Some(text.to_string());
    }
    let (folder, last) = text.rsplit_once('/').map_or((None, text), |(folder, last)| (Some(folder), last));
    let wild = |part: &str| part.contains(['*', '?', '{']);
    if !plain || !wild(last) || folder.is_some_and(wild) || !may_take(last, true) {
        return None;
    }
    Some(folder.map_or_else(|| CONFIG_FILE.to_string(), |folder| format!("{folder}/{CONFIG_FILE}")))
}

/// O arquivo de configuração com a chave pelo qual a busca em `folders`
/// (pastas no disco) passaria: o de cada pasta, e o da raiz `root` quando a
/// pasta o contém, como a busca que parte de uma pasta acima do projeto. Vem
/// relativo à raiz quando mora nela, senão inteiro. `None` quando os filtros
/// deixam o arquivo de fora, quando nenhum desses arquivos guarda a chave e
/// em todo erro de leitura.
pub(crate) fn swept(folders: &[PathBuf], root: &Path, walk: Walk, filters: &[NameFilter]) -> Option<String> {
    if !reads_config(walk, filters) {
        return None;
    }
    let root = std::fs::canonicalize(root).ok();
    let root_file = root.as_ref().and_then(|root| std::fs::canonicalize(root.join(CONFIG_FILE)).ok());
    let file = folders.iter().filter_map(|folder| std::fs::canonicalize(folder).ok()).filter(|folder| folder.is_dir()).find_map(
        |folder| {
            let own = std::fs::canonicalize(folder.join(CONFIG_FILE)).ok();
            own.into_iter().chain(root_file.clone()).find(|file| file.starts_with(&folder) && holds_key(file))
        },
    )?;
    let inside = root.and_then(|root| file.strip_prefix(root).ok().map(Path::to_path_buf));
    Some(inside.unwrap_or(file).to_string_lossy().into_owned())
}

/// `true` quando o arquivo em `file` guarda a chave com valor.
fn holds_key(file: &Path) -> bool {
    std::fs::read_to_string(file).is_ok_and(|text| masked(&text).is_some())
}

/// `true` quando a busca, com os filtros `filters`, lê o arquivo de
/// configuração ao passar pela pasta dele. Na dúvida sobre um filtro, a
/// resposta é a que protege a chave: o de entrada casa, o de saída não.
fn reads_config(walk: Walk, filters: &[NameFilter]) -> bool {
    let braces = walk != Walk::Grep;
    let deciding = filters.iter().rev().find(|filter| {
        if filter.exclude {
            surely_takes(&filter.glob, braces)
        } else {
            may_take(&filter.glob, braces)
        }
    });
    if let Some(filter) = deciding {
        return !filter.exclude;
    }
    match walk {
        Walk::Grep => filters.first().is_none_or(|filter| filter.exclude),
        Walk::Rg { unignored } => unignored && filters.iter().all(|filter| filter.exclude),
    }
}

/// `true` quando o filtro de nome `glob` pode casar com o arquivo de
/// configuração: pela última parte do filtro, sem olhar maiúsculas, e sempre
/// que o filtro usa o que esta leitura não conhece. `braces` diz se `{a,b}`
/// vale por uma das opções, como no `rg` e no terminal.
fn may_take(glob: &str, braces: bool) -> bool {
    if braces && glob.contains('{') && glob.contains('/') {
        return true;
    }
    let last = glob.rsplit('/').next().unwrap_or(glob).to_lowercase();
    glob_fits(&last, CONFIG_FILE, braces).unwrap_or(true)
}

/// `true` só quando o filtro de nome `glob` casa com certeza com o arquivo
/// de configuração em qualquer pasta. Com `braces` (o `rg`), o `**/` do
/// começo vale por qualquer pasta; sem ele (o `grep`, que compara o nome sem
/// a pasta), um filtro com `/` nunca casa.
fn surely_takes(glob: &str, braces: bool) -> bool {
    let mut glob = glob;
    while braces && let Some(rest) = glob.strip_prefix("**/") {
        glob = rest;
    }
    !glob.contains('/') && glob_fits(glob, CONFIG_FILE, braces) == Some(true)
}

/// Se `name` casa com o filtro `glob`: `*` vale por qualquer trecho, `?` por
/// uma letra e, com `braces`, `{a,b}` por uma das opções. `None` quando o
/// filtro usa outra coisa (`[`, `\`, chaves dentro de chaves).
fn glob_fits(glob: &str, name: &str, braces: bool) -> Option<bool> {
    if glob.contains(['[', '\\']) {
        return None;
    }
    if braces && let Some(open) = glob.find('{') {
        let close = open + glob[open..].find('}')?;
        let (head, options, tail) = (&glob[..open], &glob[open + 1..close], &glob[close + 1..]);
        if options.contains('{') {
            return None;
        }
        for option in options.split(',') {
            if glob_fits(&format!("{head}{option}{tail}"), name, braces)? {
                return Some(true);
            }
        }
        return Some(false);
    }
    Some(wildcard(glob.as_bytes(), name.as_bytes()))
}

/// `name` casa com `glob`, de `*` e `?`.
fn wildcard(glob: &[u8], name: &[u8]) -> bool {
    match glob.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|at| wildcard(rest, &name[at..])),
        Some((b'?', rest)) => !name.is_empty() && wildcard(rest, &name[1..]),
        Some((byte, rest)) => name.first() == Some(byte) && wildcard(rest, &name[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma chave inventada para os testes; nenhuma chave de verdade entra
    /// aqui.
    const FAKE: &str = "chave-falsa-0123456789";

    /// O valor do campo que a configuração lê como chave sai trocado, em
    /// qualquer altura do arquivo; um campo de nome parecido e o resto do
    /// texto ficam como estavam.
    #[test]
    fn only_the_field_the_config_reads_as_the_key_is_hidden() {
        let text = format!(
            "{{\n  \"language\": {{\"text\": \"pt-BR\"}},\n  \"jev\": {{ \"key\" : \"{FAKE}\" }},\n  \"apiKey\": \"visivel\",\n  \"x_key\": \"outro\",\n  \"old\": {{\"key\": \"a\\\"b\"}}\n}}\n"
        );
        let shown = masked(&text).expect("the file holds a key");
        assert!(!shown.contains(FAKE), "{shown}");
        assert!(!shown.contains("a\\\"b"), "the escaped quote stays inside the value: {shown}");
        assert_eq!(shown.matches(HIDDEN).count(), 2, "{shown}");
        assert!(shown.contains("\"apiKey\": \"visivel\""), "a lookalike name is not the key: {shown}");
        assert!(shown.contains("\"x_key\": \"outro\""), "a lookalike name is not the key: {shown}");
        assert!(shown.contains("\"language\": {\"text\": \"pt-BR\"}"), "{shown}");
        assert!(shown.contains("\"jev\": { \"key\" : \"***\" }"), "{shown}");
    }

    /// A chave que a configuração lê do arquivo é a mesma que a leitura
    /// esconde: os dois lados procuram o mesmo nome de campo.
    #[test]
    fn the_key_the_config_reads_is_the_one_every_reading_hides() {
        let dir = tempfile::tempdir().expect("tempdir");
        let text = format!("{{\"language\": {{\"text\": \"pt-BR\"}}, \"jev\": {{\"key\": \"{FAKE}\"}}}}");
        std::fs::write(dir.path().join(CONFIG_FILE), &text).expect("config");
        assert_eq!(mustard_core::ProjectConfig::load(dir.path()).jev_key(), Some(FAKE), "the config reads the key");
        let shown = masked(&text).expect("the key the config reads is found");
        assert!(!shown.contains(FAKE) && shown.contains(HIDDEN), "{shown}");
    }

    /// Sem campo de chave, ou com a chave vazia, nada se esconde; um nome que
    /// só contém a palavra no meio não é chave. O arquivo que não fecha como
    /// JSON ainda tem a chave escondida.
    #[test]
    fn only_a_key_field_with_a_value_counts() {
        assert_eq!(masked(r#"{"git": {"flow": {"monkey": "dev"}}, "keys": "x"}"#), None);
        assert_eq!(masked(r#"{"jev": {"key": ""}}"#), None);
        assert_eq!(masked(r#"{"key": 12}"#), None, "only a text value is a key");
        let broken = format!("{{\"jev\": {{\"key\": \"{FAKE}\"");
        assert!(!masked(&broken).expect("the key is found").contains(FAKE));
        let open = format!("{{\"key\": \"{FAKE}");
        assert!(!masked(&open).expect("the key is found").contains(FAKE), "a value left open hides to the end");
    }

    /// O arquivo de configuração é o `mustard.json`, em qualquer pasta.
    #[test]
    fn the_config_file_is_named_mustard_json() {
        assert!(is_config_file("mustard.json"));
        assert!(is_config_file("/p/proj/mustard.json"));
        assert!(is_config_file("C:\\p\\mustard.json"));
        assert!(!is_config_file("mustard.json.bak"));
        assert!(!is_config_file("docs/mustard.md"));
    }
    /// Os filtros escritos como no `rg`: `!` no começo deixa de fora.
    fn reads(walk: Walk, globs: &[&str]) -> bool {
        let filters: Vec<NameFilter> = globs.iter().map(|glob| NameFilter::rg(glob)).collect();
        reads_config(walk, &filters)
    }

    /// Os filtros de nome decidem como o `grep` e o `rg` decidem, conferido
    /// contra os dois programas. No `grep`, o último filtro que casa vale;
    /// sem nenhum, o arquivo entra, salvo quando o primeiro é de entrada; as
    /// chaves são texto. No `rg`, o arquivo que o git ignora só entra com
    /// `-u` ou com um filtro de entrada que casa com o nome, e o último que
    /// casa vale. Um filtro que esta leitura não conhece conta como o que
    /// protege a chave.
    #[test]
    fn the_name_filters_decide_like_grep_and_rg() {
        let grep = |globs: &[&str]| reads(Walk::Grep, globs);
        assert!(grep(&[]));
        assert!(grep(&["*.json"]));
        assert!(grep(&["!foo", "*.rs"]), "the first filter leaves out, so an unmatched file enters");
        assert!(grep(&["!mustard.json", "*.json"]), "the last matching filter wins");
        assert!(grep(&["!*.{json,md}"]), "grep reads braces as text");
        assert!(grep(&["!**/mustard.json"]), "grep compares the name without its folder");
        assert!(!grep(&["!mustard.json"]));
        assert!(!grep(&["*.rs"]));
        assert!(!grep(&["*.json", "!mustard.json"]));
        assert!(!grep(&["*.{json,rs}"]));

        let rg = |globs: &[&str]| reads(Walk::Rg { unignored: false }, globs);
        for globs in [&["*.json"][..], &["*.{json,md}"], &["**/*.json"], &["m*"], &["MUSTARD.JSON"], &["[mM]ustard.json"], &["!mustard.json", "*.json"]] {
            assert!(rg(globs), "{globs:?} matches the name and overrides what git ignores");
        }
        for globs in [&[][..], &["*.rs"], &["!*.rs"], &["*.json", "!mustard.json"]] {
            assert!(!rg(globs), "{globs:?} leaves the ignored file out");
        }

        let unignored = |globs: &[&str]| reads(Walk::Rg { unignored: true }, globs);
        assert!(unignored(&[]));
        assert!(unignored(&["!*.rs"]));
        assert!(unignored(&["!/mustard.json"]), "a filter anchored to the search folder is not sure for every folder");
        assert!(!unignored(&["*.rs"]));
        assert!(!unignored(&["!mustard.json"]));
        assert!(!unignored(&["!**/mustard.json"]));
    }

    /// A busca passa pelo arquivo com a chave quando parte da pasta dele, ou
    /// de uma pasta acima do projeto; o arquivo vem relativo à raiz. A pasta
    /// de dentro, o filtro que o deixa de fora e o arquivo sem a chave não
    /// passam. O arquivo de outro projeto, na pasta buscada, conta também.
    #[test]
    fn a_search_goes_through_the_key_file_from_its_folder_or_above() {
        let outer = tempfile::tempdir().expect("tempdir");
        let root = outer.path().join("proj");
        std::fs::create_dir_all(root.join("src")).expect("src");
        let config = format!("{{\"jev\": {{\"key\": \"{FAKE}\"}}}}");
        std::fs::write(root.join(CONFIG_FILE), &config).expect("config");
        let grep = |folder: PathBuf, filters: &[NameFilter]| swept(&[folder], &root, Walk::Grep, filters);

        assert_eq!(grep(root.clone(), &[]), Some(CONFIG_FILE.to_string()));
        assert_eq!(grep(root.join("src/.."), &[]), Some(CONFIG_FILE.to_string()));
        assert_eq!(grep(outer.path().to_path_buf(), &[]), Some(CONFIG_FILE.to_string()), "from a folder above");
        assert_eq!(grep(root.join("src"), &[]), None);
        assert_eq!(grep(root.clone(), &[NameFilter::rg("!mustard.json")]), None);
        assert_eq!(swept(std::slice::from_ref(&root), &root, Walk::Rg { unignored: false }, &[]), None);

        let other = outer.path().join("other");
        std::fs::create_dir_all(&other).expect("other");
        std::fs::write(other.join(CONFIG_FILE), &config).expect("other config");
        let shown = grep(other.clone(), &[]).expect("the other project's file");
        assert!(shown.ends_with("other/mustard.json") && Path::new(&shown).is_absolute(), "{shown}");

        std::fs::write(root.join(CONFIG_FILE), "{}").expect("config without the key");
        assert_eq!(grep(root.clone(), &[]), None);
    }

    /// A palavra do terminal alcança o arquivo pelo nome, ou por um curinga
    /// sem aspas que o terminal abre e que casa com o nome; o curinga entre
    /// aspas, o que não casa e o de pasta não alcançam.
    #[test]
    fn a_word_reaches_the_config_file_by_name_or_by_an_open_wildcard() {
        assert_eq!(named_by("../mustard.json", false), Some("../mustard.json".to_string()));
        for (word, file) in [
            ("*", "mustard.json"),
            ("*.json", "mustard.json"),
            ("m*", "mustard.json"),
            ("must?rd.json", "mustard.json"),
            ("*.{json,md}", "mustard.json"),
            ("[mM]*", "mustard.json"),
            ("../*.json", "../mustard.json"),
        ] {
            assert_eq!(named_by(word, true).as_deref(), Some(file), "{word}");
        }
        for word in ["*.rs", "src/*.rs", "*/x.json", ".[]", "fn.*", "."] {
            assert_eq!(named_by(word, true), None, "{word}");
        }
        assert_eq!(named_by("*.json", false), None, "a quoted wildcard is not opened");
    }
}
