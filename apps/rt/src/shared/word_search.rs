//! Native explorer hints and search-type parsing used by existing guards.
//! Literal searches execute through search_gateway. The former replacement
//! engine survives only as a frozen historical benchmark fixture under cfg(test).
use crate::shared::config_key::NameFilter;
use crate::shared::say::say;
use mustard_core::domain::model::contract::{Ctx, HookInput};
use mustard_core::domain::normalize::Languages;
use mustard_core::domain::triage::Mark;
use mustard_core::io::map_triage::Triaged;
use mustard_core::io::project_map::{self as store, Need};
use mustard_core::io::{map_search, map_triage};
use mustard_core::platform::i18n::Locale;
use std::path::Path;
const SHOWN_FILES: usize = 5;
const RANKED_FILES: usize = 40;
const ASKED_PHRASE: usize = 600;
const PIECES_PER_FILE: usize = 2;
struct Scene<'a> {
    model: &'a Path,
    lang: Locale,
    languages: &'a Languages,
}
/// O jeito como o programa lê o padrão.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    /// A expressão do `rg`, da ferramenta de busca e do `grep -P`.
    Rust,
    /// A expressão básica do `grep`: `\|`, `\(`, `\)` e `\{` são operadores.
    Basic,
    /// A expressão estendida do `grep -E`.
    Extended,
    /// O texto como está, sem operador (`-F`).
    Fixed,
}

/// Os filtros de nome de arquivo de uma busca do `Grep`, lidos do `glob` e do
/// `type` da ferramenta: os do `glob`, e os do tipo. O `glob` pode trazer
/// vários filtros, separados por espaço ou, fora das chaves, por vírgula; o
/// `!` do começo deixa arquivos de fora. O tipo vale por filtros de nome; um
/// tipo que a leitura não conhece, ou junto do `glob`, deixa a busca própria
/// de lado (`None`). A busca do comando `run map search` lê os dois do mesmo
/// jeito.
pub(crate) fn tool_filters(
    glob: Option<&str>,
    kind: Option<&str>,
) -> (Vec<NameFilter>, Option<Vec<NameFilter>>) {
    let filters: Vec<NameFilter> = glob
        .into_iter()
        .flat_map(str::split_whitespace)
        .flat_map(|glob| {
            if glob.contains('{') {
                vec![glob]
            } else {
                glob.split(',').collect()
            }
        })
        .filter(|glob| !glob.is_empty())
        .map(NameFilter::rg)
        .collect();
    let typed = match kind {
        None => Some(Vec::new()),
        Some(kind) if filters.is_empty() => type_filters(kind),
        Some(_) => None,
    };
    (filters, typed)
}

/// Os filtros de nome de arquivo do tipo `kind` do `rg` e da ferramenta de
/// busca; `None` no tipo que esta leitura não conhece.
pub(crate) fn type_filters(kind: &str) -> Option<Vec<NameFilter>> {
    let extensions: &[&str] = match kind {
        "rust" | "rs" => &["rs"],
        "js" | "javascript" => &["js", "jsx", "mjs", "cjs"],
        "ts" | "typescript" => &["ts", "tsx", "mts", "cts"],
        "py" | "python" => &["py"],
        "go" => &["go"],
        "java" => &["java"],
        "kotlin" | "kt" => &["kt", "kts"],
        "cs" | "csharp" => &["cs"],
        "cpp" => &["cpp", "cc", "cxx", "hpp", "hh", "h"],
        "c" => &["c", "h"],
        "php" => &["php"],
        "ruby" | "rb" => &["rb"],
        "swift" => &["swift"],
        "md" | "markdown" => &["md", "markdown"],
        "json" => &["json"],
        "yaml" => &["yml", "yaml"],
        "toml" => &["toml"],
        "html" => &["html", "htm"],
        "css" => &["css", "scss"],
        "sql" => &["sql"],
        "sh" => &["sh", "bash"],
        _ => return None,
    };
    Some(
        extensions
            .iter()
            .map(|ext| NameFilter {
                exclude: false,
                glob: format!("*.{ext}"),
            })
            .collect(),
    )
}

/// O nome sem a extensão: `*.ts`, `pay.service.ts` e `*.{ts,tsx}` perdem o
/// fim; o nome sem ponto fica inteiro.
fn without_extension(name: &str) -> String {
    if name.ends_with('}')
        && let Some(open) = name.rfind(".{")
    {
        return name[..open].to_string();
    }
    match name.rsplit_once('.') {
        Some((stem, ext)) if !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()) => {
            stem.to_string()
        }
        _ => name.to_string(),
    }
}

/// As palavras do pedido `request` a um agente de exploração que a triagem
/// lê: o que ele traz entre crases, os caminhos e nomes de arquivo, e os
/// nomes de código (`snake_case`, `camelCase`, `PascalCase`), na ordem em que
/// aparecem e sem repetir. O texto corrido do pedido vai à triagem como a
/// frase, não como palavra.
pub(crate) fn ask_words(request: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for (at, chunk) in request.split('`').enumerate() {
        // Só o nome sozinho entre crases vale como nome de código; entre
        // crases também vão comandos inteiros.
        let ticked = at % 2 == 1 && chunk.split_whitespace().count() == 1;
        for token in chunk.split_whitespace() {
            let token = token.trim_matches(|c: char| {
                !c.is_alphanumeric() && !matches!(c, '_' | '/' | '.' | '-')
            });
            let token = token.trim_end_matches('.').trim_start_matches("./");
            for word in code_words(token, ticked) {
                if words.len() < MAX_WORDS && !words.contains(&word) {
                    words.push(word);
                }
            }
        }
    }
    words
}

/// As palavras que o trecho `token` do pedido traz, quando ele é nome de
/// código: o caminho de arquivo dá o nome sem extensão, e o de pasta, a última
/// parte (a pasta de um caminho absoluto é da máquina, não do código); o nome
/// com `_` ou com maiúscula no meio fica inteiro. O nome sozinho entre crases
/// (`ticked`) vale como nome de código mesmo sem essas marcas.
fn code_words(token: &str, ticked: bool) -> Vec<String> {
    if token.chars().filter(|c| c.is_alphanumeric()).count() < 3 || token.starts_with("http") {
        return Vec::new();
    }
    let slashes = token.matches('/').count();
    let pathlike = slashes >= 2
        || (slashes == 1 && token.contains(['.', '_', '-']))
        || (slashes == 1 && ticked)
        || token.split_once('.').is_some_and(|(stem, ext)| {
            stem.chars().count() >= 2
                && stem.chars().any(char::is_alphabetic)
                && ext.chars().next().is_some_and(char::is_alphabetic)
                && ext
                    .split('.')
                    .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric()))
        });
    if pathlike {
        let last = token
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or_default();
        let is_file = last.contains('.') && !last.starts_with('.');
        // A pasta de um caminho absoluto é a da máquina, não a do código.
        if token.starts_with('/') && !is_file {
            return Vec::new();
        }
        let text = if is_file {
            without_extension(last)
        } else {
            last.to_string()
        };
        return words_of(&[text], false);
    }
    let coded = token.chars().all(|c| c.is_alphanumeric() || c == '_')
        && (token.contains('_')
            || token
                .chars()
                .zip(token.chars().skip(1))
                .any(|(a, b)| a.is_lowercase() && b.is_uppercase())
            || (ticked && token.chars().any(char::is_alphabetic)));
    if coded {
        vec![token.to_string()]
    } else {
        Vec::new()
    }
}

/// A resposta do gancho ao pedido `request` a um agente de exploração, no
/// projeto `root`: o texto que sobe ao topo do pedido, ou `None` quando o
/// pedido passa como veio — a chave `search.answer` desligada, sem mapa, sem
/// nome de código no pedido ou o mapa sem cravar a resposta.
pub(crate) fn hook_ask(root: &str, _input: &HookInput, ctx: &Ctx, request: &str) -> Option<String> {
    if !ctx.config.search_answer() {
        return None;
    }
    let scene = Scene {
        model: &store::model_path(Path::new(root)),
        lang: ctx.config.language().text_or_default(),
        languages: &Languages::of(&ctx.config),
    };
    ask_reply(&scene, request)
}
/// A resposta curta do mapa ao pedido `request`: a marca, os arquivos que a
/// triagem achou e, de cada arquivo, as primeiras declarações com o começo e o
/// fim. O pedido vai à triagem como frase e como as palavras de
/// [`ask_words`]. Só o cravado responde: o parcial e o não achou devolvem
/// `None`, que deixa o pedido como veio, sem chamar o filtro.
fn ask_reply(scene: &Scene<'_>, request: &str) -> Option<String> {
    let words = ask_words(request);
    if words.is_empty() {
        return None;
    }
    store::read_for_at(scene.model, Need::Paths).ok()?;
    let question = words.join(" ");
    let intent: String = request
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(ASKED_PHRASE)
        .collect();
    let triaged = map_triage::triage_at(
        scene.model,
        (&question, &intent),
        scene.languages,
        RANKED_FILES,
    )
    .ok()?;
    let mark = triaged.mark();
    if mark != Mark::Pinned {
        return None;
    }
    let places = places_of_triage(scene, (&question, &intent), &triaged)?;
    if places.is_empty() {
        return None;
    }
    let mut out = say("map.answer.ask", scene.lang, &[]);
    out.push('\n');
    out.push_str(&header(mark, &triaged, scene.lang));
    for (path, pieces) in places {
        out.push('\n');
        out.push_str(&path);
        for piece in pieces {
            out.push_str("\n  ");
            out.push_str(&piece);
        }
    }
    out.push('\n');
    out.push_str(&say("map.search.use_tools", scene.lang, &[]));
    Some(out)
}

/// Os primeiros arquivos da triagem, cada um com as declarações dele que a
/// ordem única põe na frente, até [`PIECES_PER_FILE`]. `None` quando o mapa
/// não se lê.
fn places_of_triage(
    scene: &Scene<'_>,
    (question, intent): (&str, &str),
    triaged: &Triaged,
) -> Option<Vec<(String, Vec<String>)>> {
    let found = map_search::candidates_at(
        scene.model,
        question,
        intent,
        scene.languages,
        map_search::any_path,
    )
    .ok()?;
    Some(
        triaged
            .files
            .iter()
            .take(SHOWN_FILES)
            .map(|file| {
                let pieces: Vec<String> = found
                    .candidates
                    .iter()
                    .filter(|candidate| candidate.path == file.path)
                    .take(PIECES_PER_FILE)
                    .map(|candidate| {
                        format!(
                            "{}-{} {}",
                            candidate.line, candidate.end_line, candidate.name
                        )
                    })
                    .collect();
                (file.path.clone(), pieces)
            })
            .collect(),
    )
}

/// A frase da marca, com as palavras que a triagem achou ou as que faltam.
fn header(mark: Mark, triaged: &Triaged, lang: Locale) -> String {
    let quoted = |words: &[String]| {
        words
            .iter()
            .map(|word| format!("\"{word}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match mark {
        Mark::Pinned => {
            // Cravado não exige todas as palavras: o texto cita as que o
            // primeiro achado traz em campo forte, e só sem nenhuma delas cita
            // as da pergunta inteira.
            let found: Vec<String> = triaged
                .words
                .iter()
                .filter(|word| !triaged.missing.contains(word))
                .cloned()
                .collect();
            let shown = if found.is_empty() {
                &triaged.words
            } else {
                &found
            };
            say("map.answer.pinned", lang, &[("{words}", &quoted(shown))])
        }
        Mark::Partial if !triaged.missing.is_empty() => say(
            "map.answer.partial",
            lang,
            &[("{missing}", &quoted(&triaged.missing))],
        ),
        _ => say("map.answer.partial_unsure", lang, &[]),
    }
}

use words::{MAX_WORDS, words_of};
#[cfg(test)]
#[path = "word_search/legacy_fixture.rs"]
mod legacy_fixture;
#[path = "word_search/words.rs"]
mod words;
#[cfg(test)]
pub(crate) use legacy_fixture::{fixture, ruler, scoped};
