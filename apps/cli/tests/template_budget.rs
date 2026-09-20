//! `template_budget` — o tamanho do texto do Mustard que o modelo lê.
//!
//! Em cada idioma, todo texto que o modelo lê — os comandos e o estilo de
//! resposta do plugin, os agentes e o mapa do início da sessão que o
//! instalador grava — soma menos de 20.480 bytes, e nenhum arquivo passa de
//! 3.072 bytes. O mapa é o texto do início da sessão, e tem até 3.072 bytes.
//!
//! A conta é a do disco, byte a byte, como `find … -printf '%s'` a faz. Um
//! arquivo pertence a um idioma quando o caminho dele diz o idioma (uma pasta
//! `pt-BR/` ou um nome terminado em `-pt-BR`); o que não diz idioma nenhum é
//! lido nos dois e conta nas duas somas.
//!
//! O corte da descrição de um comando é outra conta, que o Claude Code faz: a
//! descrição passa de 1.536 caracteres e é cortada no meio da frase na lista
//! de comandos.

use std::path::{Path, PathBuf};

/// O teto da soma de um idioma, em bytes.
const LANGUAGE_BUDGET: u64 = 20_480;

/// O teto de um arquivo, e do texto do início da sessão, em bytes.
const FILE_CAP: u64 = 3_072;

/// O corte da descrição de um comando na lista do Claude Code, em caracteres.
const DESCRIPTION_CHAR_CAP: usize = 1_536;

/// Os dois idiomas do Mustard, como aparecem nos caminhos.
const LANGUAGES: [&str; 2] = ["pt-BR", "en-US"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Todo `.md` debaixo de `dir`, em ordem.
fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_md(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// O texto que o modelo lê: todo `.md` do plugin, fora a pasta dos binários
/// (o `README.md` dela explica a pasta para quem mantém o projeto e nunca
/// chega a uma janela), e os moldes que o instalador grava no projeto.
fn read_by_the_model() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_md(&root.join("plugin"), &mut files);
    files.retain(|p| !p.starts_with(root.join("plugin/bin")));
    for dir in ["packages/core/templates/mustard", "packages/core/templates/agents"] {
        collect_md(&root.join(dir), &mut files);
    }
    files
}

/// O idioma que o caminho diz, ou `None` para o texto dos dois.
fn language_of(path: &Path) -> Option<&'static str> {
    LANGUAGES.into_iter().find(|lang| {
        path.components().any(|c| c.as_os_str() == *lang)
            || path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.ends_with(&format!("-{lang}")))
    })
}

fn bytes(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display())).len()
}

fn shown(path: &Path) -> String {
    path.strip_prefix(repo_root()).unwrap_or(path).display().to_string()
}

/// Em cada idioma, o texto que o modelo lê soma menos de 20.480 bytes. Não há
/// teto por arquivo: o que prende um texto de agente é o que ele diz, e a
/// soma do idioma é que guarda o tamanho do todo.
#[test]
fn each_language_reads_under_the_prose_budget() {
    let files = read_by_the_model();
    assert!(files.len() >= 8, "the walk found almost nothing to measure: {files:?}");

    for lang in LANGUAGES {
        let read: Vec<&PathBuf> =
            files.iter().filter(|p| language_of(p).is_none_or(|own| own == lang)).collect();
        let own = read.iter().filter(|p| language_of(p) == Some(lang)).count();
        assert!(own >= 5, "{lang} has only {own} texts of its own: the map, the style and three agents");
        let total: u64 = read.iter().map(|p| bytes(p)).sum();
        assert!(
            total < LANGUAGE_BUDGET,
            "the {lang} prose adds up to {total} bytes, over the {LANGUAGE_BUDGET} budget:\n{}",
            read.iter().map(|p| format!("{} {}", bytes(p), shown(p))).collect::<Vec<_>>().join("\n"),
        );
    }
}

/// Cada texto de um idioma tem o seu par no outro: os dois idiomas existem
/// como molde do produto.
#[test]
fn every_text_exists_in_both_languages() {
    let files = read_by_the_model();
    let key = |p: &Path, lang: &str| shown(p).replace(lang, "{lang}");
    for (lang, other) in [(LANGUAGES[0], LANGUAGES[1]), (LANGUAGES[1], LANGUAGES[0])] {
        for path in files.iter().filter(|p| language_of(p) == Some(lang)) {
            let twin = key(path, lang);
            assert!(
                files.iter().any(|p| language_of(p) == Some(other) && key(p, other) == twin),
                "{} has no {other} twin",
                shown(path),
            );
        }
    }
}

/// O texto do início da sessão — o mapa — tem até 3.072 bytes em cada
/// idioma, medido no texto que o binário embute e grava no projeto.
#[test]
fn the_session_start_text_fits_its_cap() {
    for text in [mustard_core::platform::i18n::Locale::PtBr, mustard_core::platform::i18n::Locale::EnUs] {
        let size = mustard_core::session_map(text).len() as u64;
        assert!(size <= FILE_CAP, "the {text} session map is {size} bytes, over {FILE_CAP}");
    }
}

/// A descrição do cabeçalho: um valor numa linha só, ou um bloco dobrado
/// (`>` / `|`). `None` quando o arquivo não tem cabeçalho ou descrição.
fn frontmatter_description(text: &str) -> Option<String> {
    let after_open = text.strip_prefix("---")?;
    let end = after_open.find("\n---")?;
    let lines: Vec<&str> = after_open[..end].lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("description:") else {
            continue;
        };
        let rest = rest.trim();
        if matches!(rest, ">" | "|" | ">-" | "|-") {
            let folded: Vec<&str> = lines[i + 1..]
                .iter()
                .take_while(|cont| cont.trim().is_empty() || cont.starts_with([' ', '\t']))
                .map(|cont| cont.trim())
                .filter(|cont| !cont.is_empty())
                .collect();
            return Some(folded.join(" "));
        }
        return Some(rest.trim_matches(['"', '\'']).to_string());
    }
    None
}

/// A descrição de cada comando cabe no corte de 1.536 caracteres: passado
/// dele, o Claude Code corta o gatilho no meio da frase.
#[test]
fn command_descriptions_fit_the_listing_cap() {
    let mut files = Vec::new();
    collect_md(&repo_root().join("plugin/commands"), &mut files);
    assert!(!files.is_empty(), "no command found under plugin/commands");
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let desc = frontmatter_description(&text).unwrap_or_else(|| panic!("{} has no description", shown(path)));
        let chars = desc.chars().count();
        assert!(chars <= DESCRIPTION_CHAR_CAP, "{}: description is {chars} characters", shown(path));
    }
}
