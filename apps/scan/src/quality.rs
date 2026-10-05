//! As medidas de qualidade de cada arquivo escrito à mão, fora os de teste:
//! as linhas não vazias do arquivo e de cada função, sem os trechos de teste
//! escritos dentro dele; as importações; as linhas que caem numa janela de
//! linhas seguidas igual à de outro arquivo; e a participação num ciclo de
//! importações.
//!
//! Refeitas do projeto inteiro a cada passada que lê arquivo, depois das
//! importações resolvidas: a repetição e o ciclo dependem dos outros
//! arquivos, e o arquivo tomado do mapa anterior é lido de novo do disco. Só
//! informam: o corte de "grande" e de "repetido", relativo ao projeto, mora
//! no núcleo, e nenhuma medida recusa trabalho.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use mustard_core::domain::ast::is_test_path;
use mustard_core::domain::project_map::{Quality, REPEATED_WINDOW};
use petgraph::graph::{DiGraph, NodeIndex};

use crate::graph::METHOD_KINDS;
use crate::ingest::in_parallel;
use crate::model::Module;

/// O que a leitura de um arquivo dá: o tamanho, o de cada função, e a marca
/// de cada janela de linhas seguidas, com a posição dela entre as linhas
/// contadas.
struct Read {
    size: usize,
    functions: Vec<(u64, usize)>,
    windows: Vec<u64>,
}

/// Mede cada arquivo de `modules`, lido de `root`, e grava as medidas nele.
/// O arquivo escrito por máquina, o de teste (pelo caminho ou declarado por um
/// módulo) e o que não se lê ficam sem medida. A marca do arquivo declarado
/// como teste tem de estar posta nos módulos antes.
pub(crate) fn measure(root: &Path, modules: &mut [Module]) {
    let measured: Vec<usize> = (0..modules.len())
        .filter(|&at| {
            let module = &modules[at];
            module.file_class.is_empty() && !is_test_path(&module.path) && !module.is_declared_test()
        })
        .collect();
    let reads: Vec<Option<Read>> = {
        let items: Vec<&Module> = measured.iter().map(|&at| &modules[at]).collect();
        in_parallel(items, |module| read(root, module))
    };
    let repeated = repeated_lines(&reads);
    let cyclic = in_cycle(modules);
    for module in modules.iter_mut() {
        module.quality = Quality::default();
    }
    for ((&at, found), repeated) in measured.iter().zip(reads).zip(repeated) {
        let Some(found) = found else { continue };
        let module = &mut modules[at];
        module.quality = Quality {
            size: found.size,
            imports: module.imports.len(),
            repeated,
            cycle: cyclic[at],
            functions: found.functions,
        };
    }
}

/// O arquivo `module` lido de `root`: as linhas não vazias fora dos trechos
/// de teste, as de cada função e as janelas; `None` quando ele não se lê.
fn read(root: &Path, module: &Module) -> Option<Read> {
    let content = std::fs::read_to_string(root.join(&module.path)).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let written = |line: usize| -> bool {
        lines.get(line - 1).is_some_and(|text| !text.trim().is_empty()) && !module.is_test_line(line)
    };
    let hashes: Vec<u64> = (1..=lines.len()).filter(|&line| written(line)).map(|line| hash_of(lines[line - 1].trim())).collect();
    let functions = module
        .declarations
        .iter()
        .filter(|d| METHOD_KINDS.contains(&d.kind.as_str()) && d.end_line >= d.line && d.line > 0 && !module.is_test_line(d.line))
        .map(|d| (d.line as u64, (d.line..=d.end_line).filter(|&line| written(line)).count()))
        .collect();
    let windows = hashes.windows(REPEATED_WINDOW).map(hash_of).collect();
    Some(Read { size: hashes.len(), functions, windows })
}

fn hash_of(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Quantas linhas contadas de cada arquivo caem numa janela que outro
/// arquivo também tem. A janela repetida dentro do mesmo arquivo não conta.
fn repeated_lines(reads: &[Option<Read>]) -> Vec<usize> {
    // A marca de cada janela: o primeiro arquivo que a tem, e se outro
    // também a tem.
    let mut owners: HashMap<u64, (usize, bool)> = HashMap::new();
    for (file, found) in reads.iter().enumerate() {
        for &window in found.iter().flat_map(|found| &found.windows) {
            match owners.entry(window) {
                Entry::Vacant(slot) => {
                    slot.insert((file, false));
                }
                Entry::Occupied(mut slot) if slot.get().0 != file => slot.get_mut().1 = true,
                Entry::Occupied(_) => {}
            }
        }
    }
    reads
        .iter()
        .map(|found| {
            let Some(found) = found else { return 0 };
            let mut covered = vec![false; found.size];
            for (start, window) in found.windows.iter().enumerate() {
                if owners.get(window).is_some_and(|&(_, shared)| shared) {
                    covered[start..start + REPEATED_WINDOW].fill(true);
                }
            }
            covered.into_iter().filter(|&line| line).count()
        })
        .collect()
}

/// Cada arquivo de `modules`, pela posição, está num ciclo das importações
/// resolvidas (`deps`).
fn in_cycle(modules: &[Module]) -> Vec<bool> {
    let at: HashMap<&str, usize> = modules.iter().enumerate().map(|(i, m)| (m.path.as_str(), i)).collect();
    let mut graph: DiGraph<(), ()> = DiGraph::new();
    let nodes: Vec<NodeIndex> = modules.iter().map(|_| graph.add_node(())).collect();
    let mut looped = vec![false; modules.len()];
    for (from, module) in modules.iter().enumerate() {
        for to in module.deps.iter().filter_map(|dep| at.get(dep.as_str())) {
            if *to == from {
                looped[from] = true;
            }
            graph.add_edge(nodes[from], nodes[*to], ());
        }
    }
    for component in petgraph::algo::tarjan_scc(&graph).into_iter().filter(|c| c.len() > 1) {
        for node in component {
            looped[node.index()] = true;
        }
    }
    looped
}
