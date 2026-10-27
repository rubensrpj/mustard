// Os testes de integração são alvos separados e `#[cfg(test)]` não os isenta
// de `clippy::unwrap_used`: espelha a isenção de `src/main.rs`, para que o
// pânico de um `.unwrap()` continue valendo como asserção.
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Os módulos do fluxo — a rodada, a retomada, o fechamento, o painel e a
//! parte comum que os quatro importam — e a gravação de eventos, com a porta
//! `run write` que a chama, não se importam em ciclo, nem por um módulo da
//! pasta da gravação no meio do caminho. A rodada e a parte comum gravam pela
//! gravação de eventos; ela, por isso, não chega a nenhum deles. A conta lê o
//! código de produção de cada arquivo do pacote, sem os comentários e o
//! conteúdo dos textos, com os caminhos relativos (`super::`, `self::`)
//! escritos por inteiro, e liga um módulo ao outro por
//! todo caminho que nomeia o outro: na importação, no caminho escrito antes
//! de uma chamada e na lista entre chaves. O caminho que começa por um filho
//! que o módulo declara fora dos testes (`filho::...`) também liga o módulo a
//! ele. Um módulo da lista chega a outro quando o nomeia, ou quando nomeia um
//! módulo da pasta da gravação ([`MIDDLE`]) que chega a ele. O módulo de fora
//! dessa pasta não liga um módulo da lista a outro: o ciclo que passa por ele
//! fica fora desta conta.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::sources::{code_only, production_code, rust_sources};

/// Cada módulo do fluxo, a gravação de eventos e a porta dela, com o caminho
/// de cada um a partir da raiz do pacote.
const MODULES: [(&str, &str); 7] = [
    ("round", "crate::commands::flow::round"),
    ("resume", "crate::commands::flow::resume"),
    ("close", "crate::commands::flow::close"),
    ("panel", "crate::commands::panel"),
    ("common", "crate::commands::flow::common"),
    ("write", "crate::commands::spec_events::write"),
    ("write_door", "crate::commands::spec_events::write_door"),
];

/// A pasta cujos módulos, no meio do caminho, ligam um módulo da lista a
/// outro: a da gravação de eventos, que divide as regras dela pelos módulos
/// vizinhos. O ciclo que passa por um deles é o da própria gravação.
const MIDDLE: &str = "crate::commands::spec_events::";

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// O caminho do módulo do arquivo `rel`, escrito a partir de `src/`.
fn module_path(rel: &Path) -> String {
    let text = rel.with_extension("").to_string_lossy().replace('\\', "/");
    let text = text.strip_suffix("/mod").unwrap_or(&text);
    if text == "lib" { "crate".to_string() } else { format!("crate::{}", text.replace('/', "::")) }
}

/// O módulo da lista a que o caminho `path` pertence.
fn listed_of(path: &str) -> Option<&'static str> {
    MODULES.iter().find(|(_, at)| path == *at || path.starts_with(&format!("{at}::"))).map(|(name, _)| *name)
}

/// O nó do grafo do módulo `path`: o nome do módulo da lista que o contém ou,
/// para um módulo de fora dela, o próprio caminho.
fn node_of(path: &str) -> String {
    listed_of(path).map_or_else(|| path.to_string(), str::to_string)
}

/// `true` quando o caractere entra num caminho: letra, número, `_` ou `:`.
fn in_path(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == ':'
}

/// Os caminhos relativos da linha escritos a partir da raiz do pacote, no
/// módulo `module`: cada `super::` sobe uma parte, e `self::` fica nele.
fn absolute(line: &str, module: &str) -> String {
    let parts: Vec<&str> = module.split("::").collect();
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("super::").into_iter().chain(rest.find("self::")).min() {
        let starts_word = rest[..at].chars().next_back().is_none_or(|c| !in_path(c));
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

/// O texto com cada lista entre chaves de um caminho do pacote aberta:
/// `crate::p::{a, b}` vira `crate::p::a, crate::p::b`, também a lista que
/// mora dentro de outra.
fn expanded(code: &str) -> String {
    let mut code = code.to_string();
    let mut from = 0;
    while let Some(found) = code[from..].find("::{") {
        let open = from + found + "::{".len();
        let head = &code[..open - 1];
        let start = head.rfind(|c: char| !in_path(c)).map_or(0, |at| at + head[at..].chars().next().map_or(1, char::len_utf8));
        if !code[start..].starts_with("crate::") {
            from = open;
            continue;
        }
        let mut depth = 1;
        let mut close = code.len();
        let mut items = Vec::new();
        let mut item_start = open;
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
                    items.push(code[item_start..open + offset].trim().to_string());
                    item_start = open + offset + 1;
                }
                _ => {}
            }
        }
        items.push(code[item_start..close].trim().to_string());
        let base = code[start..open - 1].to_string();
        let listed: Vec<String> = items.iter().filter(|item| !item.is_empty()).map(|item| format!("{base}{item}")).collect();
        code.replace_range(start..(close + 1).min(code.len()), &listed.join(", "));
        from = start;
    }
    code
}

/// Os módulos que a linha nomeia, cada um o mais longo módulo de `known` que
/// abre um caminho `crate::` escrito nela.
fn named<'a>(line: &str, known: &'a BTreeSet<String>) -> Vec<&'a str> {
    line.match_indices("crate::")
        .filter(|(start, _)| line[..*start].chars().next_back().is_none_or(|c| !in_path(c)))
        .filter_map(|(start, _)| {
            let path: String = line[start..].chars().take_while(|c| in_path(*c)).collect();
            let mut prefix = String::new();
            let mut longest = None;
            for part in path.split("::").filter(|part| !part.is_empty()) {
                if !prefix.is_empty() {
                    prefix.push_str("::");
                }
                prefix.push_str(part);
                if let Some(found) = known.get(&prefix) {
                    longest = Some(found.as_str());
                }
            }
            longest
        })
        .collect()
}

/// O nome do filho que a linha declara (`mod nome;`), com ou sem `pub`.
fn declared_child(line: &str) -> Option<&str> {
    let line = line.trim();
    let line = match line.strip_prefix("pub") {
        Some(rest) if rest.starts_with('(') => rest.split_once(')').map_or(rest, |(_, after)| after),
        Some(rest) => rest,
        None => line,
    };
    let name = line.trim_start().strip_prefix("mod ")?.trim().strip_suffix(';')?.trim();
    (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')).then_some(name)
}

/// `true` quando a linha escreve um caminho que começa pelo filho `child`
/// (`child::...`), sem nada antes dele no caminho.
fn names_child(line: &str, child: &str) -> bool {
    let named = format!("{child}::");
    line.match_indices(&named).any(|(start, _)| line[..start].chars().next_back().is_none_or(|c| !in_path(c)))
}

/// Cada ligação de um módulo do pacote a outro, entre os nós do grafo
/// ([`node_of`]), com os arquivos e as linhas que a fazem.
fn edges() -> BTreeMap<(String, String), Vec<String>> {
    let root = source_root();
    let mut files = Vec::new();
    rust_sources(&root, &mut files);
    let modules: Vec<(PathBuf, String)> = files
        .into_iter()
        .map(|file| {
            let module = module_path(file.strip_prefix(&root).unwrap());
            (file, module)
        })
        .collect();
    let known: BTreeSet<String> = modules.iter().map(|(_, module)| module.clone()).collect();
    let mut out: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (file, module) in &modules {
        let rel = file.strip_prefix(&root).unwrap();
        let from = node_of(module);
        let source = std::fs::read_to_string(file).unwrap();
        let production = String::from_utf8_lossy(&code_only(&production_code(&source))).into_owned();
        let mut link = |to: &str, line: &str| {
            let to = node_of(to);
            if to != from {
                out.entry((from.clone(), to)).or_default().push(format!("{}: {}", rel.display(), line.trim()));
            }
        };
        let children: Vec<&str> = production.lines().filter_map(declared_child).collect();
        for line in production.lines() {
            for child in children.iter().filter(|child| names_child(line, child)) {
                let child = format!("{module}::{child}");
                if known.contains(&child) {
                    link(&child, line);
                }
            }
        }
        let code: String = production.lines().map(|line| absolute(line, module) + "\n").collect();
        for line in expanded(&code).lines() {
            for to in named(line, &known) {
                link(to, line);
            }
        }
    }
    out
}

/// Cada módulo da lista que outro da lista alcança, com o caminho de nós por
/// onde chega: direto, ou pelos módulos da pasta da gravação ([`MIDDLE`]) no
/// meio.
fn reach(edges: &BTreeMap<(String, String), Vec<String>>) -> BTreeMap<(&'static str, &'static str), Vec<String>> {
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (from, to) in edges.keys() {
        graph.entry(from.as_str()).or_default().insert(to.as_str());
    }
    let mut out = BTreeMap::new();
    for (start, _) in MODULES {
        let mut came_from: BTreeMap<&str, &str> = BTreeMap::new();
        let mut queue = VecDeque::from([start]);
        while let Some(at) = queue.pop_front() {
            for next in graph.get(at).into_iter().flatten().copied() {
                if next == start || came_from.contains_key(next) {
                    continue;
                }
                came_from.insert(next, at);
                match MODULES.iter().find(|(name, _)| *name == next) {
                    Some((target, _)) => {
                        let mut path = vec![next.to_string()];
                        let mut back = next;
                        while let Some(prev) = came_from.get(back).copied() {
                            path.push(prev.to_string());
                            back = prev;
                            if back == start {
                                break;
                            }
                        }
                        path.reverse();
                        out.insert((start, *target), path);
                    }
                    None if next.starts_with(MIDDLE) => queue.push_back(next),
                    None => {}
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
    let reach = reach(&edges);
    assert!(reach.contains_key(&("resume", "round")), "the reading finds the imports: {reach:?}");
    assert!(reach.contains_key(&("round", "write")), "the reading finds the round recording through the event writer: {reach:?}");
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (from, to) in reach.keys() {
        graph.entry(from).or_default().insert(to);
    }
    if let Some(found) = cycle(&graph) {
        let shown: Vec<String> = found
            .windows(2)
            .map(|pair| {
                let path = &reach[&(pair[0], pair[1])];
                let hops: Vec<String> = path
                    .windows(2)
                    .map(|hop| format!("  {} -> {}:\n    {}", hop[0], hop[1], edges[&(hop[0].clone(), hop[1].clone())].join("\n    ")))
                    .collect();
                format!("{}:\n{}", path.join(" -> "), hops.join("\n"))
            })
            .collect();
        panic!("the flow modules import in a cycle:\n{}", shown.join("\n"));
    }
}

/// A gravação que chama um módulo da pasta dela que nomeia a rodada chega à
/// rodada, pelo caminho que passa por ele.
#[test]
fn a_module_of_the_writer_folder_in_the_middle_links_the_writer_to_the_round() {
    let rule = "crate::commands::spec_events::rule";
    let edge = |from: &str, to: &str| ((from.to_string(), to.to_string()), vec![format!("{from}: {to}")]);
    let edges: BTreeMap<(String, String), Vec<String>> = [edge("write", rule), edge(rule, "round")].into_iter().collect();
    let path = ["write", rule, "round"].map(str::to_string).to_vec();
    assert_eq!(reach(&edges).get(&("write", "round")), Some(&path));
}

/// O módulo de fora da pasta da gravação não liga a gravação ao painel: a
/// conta segue só os módulos dessa pasta.
#[test]
fn a_module_outside_the_writer_folder_in_the_middle_does_not_link_the_writer_to_the_panel() {
    let summary = "crate::shared::project_summary";
    let edge = |from: &str, to: &str| ((from.to_string(), to.to_string()), vec![format!("{from}: {to}")]);
    let edges: BTreeMap<(String, String), Vec<String>> = [edge("write", summary), edge(summary, "panel")].into_iter().collect();
    assert_eq!(reach(&edges).get(&("write", "panel")), None);
}
