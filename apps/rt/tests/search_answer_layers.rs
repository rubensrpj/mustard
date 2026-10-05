// Integration tests are separate binary targets and not exempt from
// `clippy::unwrap_used` etc. via `#[cfg(test)]`. Mirror the carve-out from
// `src/main.rs` so test panics on `.unwrap()` remain valid assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A resposta da busca se monta na parte compartilhada.
//!
//! O comando de busca e o gancho da ferramenta de busca dão a mesma resposta,
//! e a peça que a monta mora em `shared`, que os dois usam. Se `shared`
//! importasse do comando ou do gancho, ou um deles do outro, os dois
//! caminhos voltariam a depender um do outro em círculo. O teste lê o código
//! dos arquivos, sem os comentários, e confere as importações.

#[path = "support/manifest_dir.rs"]
mod manifest_dir;

/// As linhas de código de `source`, sem os comentários, que trazem `path`.
fn reaching<'s>(source: &'s str, path: &str) -> Vec<&'s str> {
    source.lines().map(str::trim_start).filter(|line| !line.starts_with("//") && line.contains(path)).collect()
}

/// A porta, a busca por palavra e a triagem não importam de `commands::map`
/// nem de `hooks`; o comando não importa de `hooks`; o gancho não importa de
/// `commands::map`. Os arquivos citados existem: o nome que sumisse deixaria
/// o teste verde sem conferir nada.
#[test]
fn the_search_answer_is_built_in_shared_without_command_and_hook_reaching_each_other() {
    let src = manifest_dir::manifest_dir().join("src");
    let shared = ["shared/search_door.rs", "shared/word_search.rs", "shared/triage_view.rs"];
    let checks = shared
        .iter()
        .map(|file| (*file, ["crate::commands::map", "crate::hooks"].as_slice()))
        .chain([
            ("commands/map.rs", ["crate::hooks"].as_slice()),
            ("hooks/write/write_gate.rs", ["crate::commands::map"].as_slice()),
        ]);
    let mut reached = Vec::new();
    for (file, forbidden) in checks {
        let source = std::fs::read_to_string(src.join(file)).unwrap_or_else(|e| panic!("{file} must be readable: {e}"));
        assert!(source.lines().count() > 50, "{file} is too short to be the file it names");
        for path in forbidden {
            reached.extend(reaching(&source, path).into_iter().map(|line| format!("{file}: {line}")));
        }
    }
    assert!(reached.is_empty(), "the search answer reaches across the layers: {reached:#?}");
}
