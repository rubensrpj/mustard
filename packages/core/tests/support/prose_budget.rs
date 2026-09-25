//! A medida do texto do Mustard que o modelo lê, uma só para todo teste que
//! confere o limite dele: cada um a traz pelo caminho deste arquivo, e a raiz
//! do repositório fica duas pastas acima do pacote que compila o teste.
//!
//! Em cada idioma, todo texto que o modelo lê — os comandos e o estilo de
//! resposta do plugin, os agentes e o mapa do início da sessão que o
//! instalador grava — soma menos de 25.600 bytes. Não há teto por arquivo de
//! agente: o que prende um molde é o que ele diz.
//!
//! A conta é a do disco, byte a byte, como `find … -printf '%s'` a faz. Um
//! arquivo pertence a um idioma quando o caminho dele diz o idioma (uma pasta
//! `pt-BR/` ou um nome terminado em `-pt-BR`); o que não diz idioma nenhum é
//! lido nos dois e conta nas duas somas.

use std::path::{Path, PathBuf};

/// O teto da soma de um idioma, em bytes.
pub const LANGUAGE_BUDGET: u64 = 25_600;

/// Os dois idiomas do Mustard, como aparecem nos caminhos.
pub const LANGUAGES: [&str; 2] = ["pt-BR", "en-US"];

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Todo `.md` debaixo de `dir`, em ordem.
pub fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
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
pub fn read_by_the_model() -> Vec<PathBuf> {
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
pub fn language_of(path: &Path) -> Option<&'static str> {
    LANGUAGES.into_iter().find(|lang| {
        path.components().any(|c| c.as_os_str() == *lang)
            || path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.ends_with(&format!("-{lang}")))
    })
}

fn bytes(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display())).len()
}

pub fn shown(path: &Path) -> String {
    path.strip_prefix(repo_root()).unwrap_or(path).display().to_string()
}

/// Em cada idioma, o texto que o modelo lê soma menos de 25.600 bytes; a
/// falha lista cada arquivo somado, com o tamanho dele.
pub fn assert_each_language_under_budget() {
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
