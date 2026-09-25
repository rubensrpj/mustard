//! `template_budget` — o tamanho do texto do Mustard que o modelo lê.
//!
//! Em cada idioma, todo texto que o modelo lê soma menos de 25.600 bytes, pela
//! medida que mora em `packages/core/tests/support/prose_budget.rs` — a mesma
//! que o teste da instalação dos agentes confere. O mapa do início da sessão,
//! que entra em toda sessão, continua com o teto dele de 3.072 bytes.
//!
//! O corte da descrição de um comando é outra conta, que o Claude Code faz: a
//! descrição passa de 1.536 caracteres e é cortada no meio da frase na lista
//! de comandos.

#[path = "../../../packages/core/tests/support/prose_budget.rs"]
mod prose_budget;

use std::path::Path;

use prose_budget::{collect_md, language_of, read_by_the_model, repo_root, shown, LANGUAGES};

/// O teto de um arquivo, e do texto do início da sessão, em bytes.
const FILE_CAP: u64 = 3_072;

/// O corte da descrição de um comando na lista do Claude Code, em caracteres.
const DESCRIPTION_CHAR_CAP: usize = 1_536;

/// Em cada idioma, o texto que o modelo lê soma menos de 25.600 bytes. Não há
/// teto por arquivo: o que prende um texto de agente é o que ele diz, e a
/// soma do idioma é que guarda o tamanho do todo.
#[test]
fn each_language_reads_under_the_prose_budget() {
    prose_budget::assert_each_language_under_budget();
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
