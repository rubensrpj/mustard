//! Os módulos do fluxo — a rodada, a retomada, o fechamento, o painel e a
//! parte comum que os quatro importam — não se importam em ciclo. A conta lê
//! o código de produção de cada arquivo deles, com os caminhos relativos
//! (`super::`, `self::`) escritos por inteiro, e liga um módulo ao outro por
//! todo caminho que nomeia o outro: na importação, no caminho escrito antes de
//! uma chamada e na lista entre chaves.

#[path = "support/sources.rs"]
mod sources;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sources::{production_code, rust_sources};

/// Cada módulo do fluxo e o caminho dele a partir da raiz do pacote.
const MODULES: [(&str, &str); 5] = [
    ("round", "crate::commands::flow::round"),
    ("resume", "crate::commands::flow::resume"),
    ("close", "crate::commands::flow::close"),
    ("panel", "crate::commands::panel"),
    ("common", "crate::commands::flow::common"),
];

/// Os prefixos cuja lista entre chaves pode nomear mais de um módulo do fluxo.
const LISTED: [&str; 2] = ["crate::commands::flow::{", "crate::commands::{"];

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// O caminho do módulo do arquivo `rel`, escrito a partir de `src/`.
fn module_path(rel: &Path) -> String {
    let text = rel.with_extension("").to_string_lossy().replace('\\', "/");
    let text = text.strip_suffix("/mod").unwrap_or(&text);
    if text == "lib" { "crate".to_string() } else { format!("crate::{}", text.replace('/', "::")) }
}

/// O módulo do fluxo a que o caminho `path` pertence.
fn module_of(path: &str) -> Option<&'static str> {
    MODULES.iter().find(|(_, at)| path == *at || path.starts_with(&format!("{at}::"))).map(|(name, _)| *name)
}

/// A linha sem o comentário do fim: o `//` que não segue um `:` de endereço.
fn without_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    (0..bytes.len().saturating_sub(1))
        .find(|&at| bytes[at] == b'/' && bytes[at + 1] == b'/' && (at == 0 || bytes[at - 1] != b':'))
        .map_or(line, |at| &line[..at])
}

/// Os caminhos relativos da linha escritos a partir da raiz do pacote, no
/// módulo `module`: cada `super::` sobe uma parte, e `self::` fica nele.
fn absolute(line: &str, module: &str) -> String {
    let parts: Vec<&str> = module.split("::").collect();
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("super::").into_iter().chain(rest.find("self::")).min() {
        let starts_word = rest[..at].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == ':'));
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if !starts_word {
            let skip = if rest.starts_with("super::") { "super::".len() } else { "self::".len() };
            out.push_str(&rest[..skip]);
            rest = &rest[skip..];
            continue;
        }
        let mut up = 0;
        if let Some(after) = rest.strip_prefix("self::") {
            rest = after;
        }
        while let Some(after) = rest.strip_prefix("super::") {
            up += 1;
            rest = after;
        }
        out.push_str(&parts[..parts.len().saturating_sub(up)].join("::"));
        out.push_str("::");
    }
    out.push_str(rest);
    out
}

/// O texto com cada lista entre chaves de [`LISTED`] aberta: `p::{a, b}`
/// vira `p::a, p::b`.
fn expanded(code: &str) -> String {
    let mut code = code.to_string();
    while let Some((prefix, at)) = LISTED.iter().find_map(|prefix| code.find(prefix).map(|at| (*prefix, at))) {
        let open = at + prefix.len();
        let mut depth = 1;
        let mut close = code.len();
        let mut items = Vec::new();
        let mut start = open;
        for (offset, c) in code[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + offset;
                        break;
                    }
                }
                ',' if depth == 1 => {
                    items.push(code[start..open + offset].trim().to_string());
                    start = open + offset + 1;
                }
                _ => {}
            }
        }
        items.push(code[start..close].trim().to_string());
        let base = &prefix[..prefix.len() - 1];
        let listed: Vec<String> = items.iter().filter(|item| !item.is_empty()).map(|item| format!("{base}{item}")).collect();
        code.replace_range(at..(close + 1).min(code.len()), &listed.join(", "));
    }
    code
}

/// Cada ligação de um módulo do fluxo a outro, com os arquivos e as linhas
/// que a fazem.
fn edges() -> BTreeMap<(&'static str, &'static str), Vec<String>> {
    let root = source_root();
    let mut files = Vec::new();
    rust_sources(&root, &mut files);
    let mut out: BTreeMap<(&str, &str), Vec<String>> = BTreeMap::new();
    for file in files {
        let rel = file.strip_prefix(&root).unwrap().to_path_buf();
        let module = module_path(&rel);
        let Some(from) = module_of(&module) else { continue };
        let source = std::fs::read_to_string(&file).unwrap();
        let code: String = production_code(&source).lines().map(|line| absolute(without_comment(line), &module) + "\n").collect();
        for line in expanded(&code).lines() {
            for (to, at) in MODULES {
                let named = line.match_indices(at).any(|(start, _)| {
                    line[start + at.len()..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
                });
                if named && to != from {
                    out.entry((from, to)).or_default().push(format!("{}: {}", rel.display(), line.trim()));
                }
            }
        }
    }
    out
}

/// Um ciclo do grafo `graph`, na ordem em que se fecha.
fn cycle(graph: &BTreeMap<&'static str, BTreeSet<&'static str>>) -> Option<Vec<&'static str>> {
    fn walk(
        at: &'static str,
        graph: &BTreeMap<&'static str, BTreeSet<&'static str>>,
        path: &mut Vec<&'static str>,
        done: &mut BTreeSet<&'static str>,
    ) -> Option<Vec<&'static str>> {
        if let Some(start) = path.iter().position(|seen| *seen == at) {
            return Some(path[start..].iter().copied().chain([at]).collect());
        }
        if !done.insert(at) {
            return None;
        }
        path.push(at);
        let found = graph.get(at).into_iter().flatten().find_map(|next| walk(next, graph, path, done));
        path.pop();
        found
    }
    let mut done = BTreeSet::new();
    graph.keys().find_map(|start| walk(start, graph, &mut Vec::new(), &mut done))
}

#[test]
fn the_flow_modules_import_in_no_cycle() {
    let edges = edges();
    assert!(edges.contains_key(&("resume", "round")), "the reading finds the imports: {edges:?}");
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (from, to) in edges.keys() {
        graph.entry(from).or_default().insert(to);
    }
    if let Some(found) = cycle(&graph) {
        let shown: Vec<String> =
            found.windows(2).map(|pair| format!("{} -> {}:\n  {}", pair[0], pair[1], edges[&(pair[0], pair[1])].join("\n  "))).collect();
        panic!("the flow modules import in a cycle:\n{}", shown.join("\n"));
    }
}
