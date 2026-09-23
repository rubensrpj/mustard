//! One topological level assignment, shared by everything in the crate that
//! orders a dependency graph.
//!
//! There were two before this module: `dispatch_plan::assign_levels` over WAVE
//! numbers, and `wave_dependency::topological_waves` over FILE paths. They
//! solved the same problem by different means and disagreed about the answer —
//! one reported the nodes it could not place, the other the nodes actually on a
//! loop, and only the second is right. A node merely WAITING behind a loop is
//! stuck without being contradictory: its own declared dependencies are correct
//! as written, and it becomes orderable the moment they resolve. Naming it
//! sends whoever has to fix the graph to the wrong place.
//!
//! ## What "on a loop" means here
//!
//! Two nodes are on the same loop exactly when each can reach the other through
//! dependency edges, and a node is on a loop exactly when it can reach itself.
//! That is the definition of a strongly connected component, computed here as a
//! transitive closure — O(n³) over the handful of nodes these graphs hold, and
//! correct by construction rather than by heuristic.
//!
//! Collapsing each component to a single node leaves a graph with no loops at
//! all, so ordinary peeling over THAT graph gives every node a real level:
//! nodes on one loop share it (there is no order between them to express), and
//! a node behind a loop lands above it. Every node gets a level, a
//! contradictory graph included — nothing is ever dropped.
//!
//! Deterministic regardless of input order: every collection here is ordered.
//!
//! ## A cesta
//!
//! Além do nível, o módulo também forma a cesta de despacho: tarefa pronta —
//! toda dependência entregue ou aprovada —, o desempate entre prontas por
//! quantas outras cada uma destrava, e o empacotamento delas em lotes com
//! capacidade de arquivos distintos, sem nunca dividir um arquivo entre dois
//! lotes. É o mesmo grafo e o mesmo peel de [`assign_levels`], só que sobre
//! tarefas: nasce aqui para não virar um segundo motor ao lado.
//!
//! Dois arquivos "se cruzam" ([`files_cross`]) quando são o mesmo caminho,
//! ou quando um deles é padrão (tem `*`, `?` ou `[`) e casa o outro. O `**`
//! cruza com tudo: a tarefa que o declara sai sozinha no lote dela, e a
//! rodada nunca a solta junto de outra onda.

use std::collections::{BTreeMap, BTreeSet};

/// The level assignment for one dependency graph, plus the nodes on a loop.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Levels<N> {
    /// Node → topological level, for EVERY node in the graph.
    pub level: BTreeMap<N, u32>,
    /// The nodes ON a dependency loop, in the graph's own order. Empty for a
    /// well-formed graph. NOT every node the peel failed to place — see the
    /// module docs for why the difference matters.
    pub cycle: Vec<N>,
}

/// Assign a topological level to every node of `deps`, and name the nodes on a
/// dependency loop. An edge to a node the graph does not contain is ignored —
/// an out-of-graph reference is not a contradiction.
pub(crate) fn assign_levels<N: Ord + Clone>(deps: &BTreeMap<N, BTreeSet<N>>) -> Levels<N> {
    let known: BTreeSet<&N> = deps.keys().collect();

    // 1. Transitive closure: `reach[a]` is every node `a` depends on, directly
    //    or through others. In-graph edges only.
    let mut reach: BTreeMap<&N, BTreeSet<&N>> = deps
        .iter()
        .map(|(n, d)| (n, d.iter().filter(|x| known.contains(x)).collect()))
        .collect();
    loop {
        let mut grew = false;
        for &node in &known {
            let mut extra: BTreeSet<&N> = BTreeSet::new();
            if let Some(direct) = reach.get(node) {
                for d in direct {
                    if let Some(indirect) = reach.get(d) {
                        extra.extend(indirect.iter().copied());
                    }
                }
            }
            if let Some(cur) = reach.get_mut(node) {
                let before = cur.len();
                cur.extend(extra);
                grew |= cur.len() != before;
            }
        }
        if !grew {
            break;
        }
    }

    let reaches = |a: &N, b: &N| reach.get(a).is_some_and(|r| r.contains(b));

    // 2. Component id = the component's smallest member, so the grouping is
    //    stable and needs no counter.
    let component: BTreeMap<&N, &N> = known
        .iter()
        .map(|&node| {
            let id = known
                .iter()
                .copied()
                .find(|&other| other == node || (reaches(node, other) && reaches(other, node)))
                .unwrap_or(node);
            (node, id)
        })
        .collect();
    // Indexed with `get`, never `map[key]`: a write hook reaches this code, and
    // a panic there does not deny one write, it kills the session.
    fn comp_of<'a, N: Ord>(component: &BTreeMap<&'a N, &'a N>, n: &'a N) -> &'a N {
        component.get(n).copied().unwrap_or(n)
    }

    // 3. Peel the CONDENSED graph, which by construction has no loops left.
    let mut comp_deps: BTreeMap<&N, BTreeSet<&N>> = BTreeMap::new();
    for (node, node_deps) in deps {
        let from = comp_of(&component, node);
        let entry = comp_deps.entry(from).or_default();
        for d in node_deps.iter().filter(|x| known.contains(x)) {
            let to = comp_of(&component, d);
            if to != from {
                entry.insert(to);
            }
        }
    }
    let mut comp_level: BTreeMap<&N, u32> = BTreeMap::new();
    loop {
        let mut placed = false;
        for (&comp, comp_d) in &comp_deps {
            if comp_level.contains_key(comp) || !comp_d.iter().all(|d| comp_level.contains_key(d)) {
                continue;
            }
            let lvl = comp_d
                .iter()
                .filter_map(|d| comp_level.get(d).map(|l| l + 1))
                .max()
                .unwrap_or(0);
            comp_level.insert(comp, lvl);
            placed = true;
        }
        if !placed {
            break;
        }
    }

    let level: BTreeMap<N, u32> = known
        .iter()
        .map(|&n| (n.clone(), comp_level.get(comp_of(&component, n)).copied().unwrap_or(0)))
        .collect();
    let cycle: Vec<N> = known
        .iter()
        .filter(|&&n| reaches(n, n))
        .map(|&n| n.clone())
        .collect();

    Levels { level, cycle }
}

/// A capacidade de um lote: 5 arquivos distintos, fixa no código. A rodada
/// monta os lotes da cesta com ela; a tarefa que sozinha toca mais arquivos
/// sai sozinha, acima dela, porque a capacidade nunca impede o despacho.
pub(crate) const BASKET_CAPACITY: usize = 5;

/// Uma tarefa da cesta: o que ela depende, os arquivos que declara e se o
/// trabalho dela já está entregue ou aprovado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BasketTask<N> {
    pub(crate) id: N,
    pub(crate) depends_on: BTreeSet<N>,
    pub(crate) files: BTreeSet<String>,
    pub(crate) done: bool,
}

/// Um lote de despacho: as tarefas dentro dele, na ordem em que entraram, e
/// todo arquivo que qualquer uma delas declara.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Batch<N> {
    pub(crate) tasks: Vec<N>,
    pub(crate) files: BTreeSet<String>,
}

/// O nível topológico de cada tarefa da cesta, pelo mesmo peelamento que já
/// ordena qualquer grafo do binário — a cesta não é um segundo motor ao lado
/// de [`assign_levels`], é o mesmo, sobre o grafo de dependência das tarefas.
pub(crate) fn task_levels<N: Ord + Clone>(tasks: &[BasketTask<N>]) -> Levels<N> {
    let deps: BTreeMap<N, BTreeSet<N>> =
        tasks.iter().map(|t| (t.id.clone(), t.depends_on.clone())).collect();
    assign_levels(&deps)
}

/// As tarefas prontas — todas as dependências entregues ou aprovadas —, na
/// ordem de despacho: quem destrava mais tarefas primeiro, empate pelo
/// próprio número, do menor para o maior. "Destravar" conta só a aresta
/// direta de volta (quantas tarefas da cesta, prontas ou não, apontam esta
/// como dependência) — uma tarefa dois saltos adiante não está presa só
/// nesta.
pub(crate) fn ready_tasks<N: Ord + Clone>(tasks: &[BasketTask<N>]) -> Vec<N> {
    let done: BTreeSet<&N> = tasks.iter().filter(|t| t.done).map(|t| &t.id).collect();
    let unlocks = |id: &N| -> usize { tasks.iter().filter(|t| t.depends_on.contains(id)).count() };

    let mut ready: Vec<&BasketTask<N>> =
        tasks.iter().filter(|t| !t.done && t.depends_on.iter().all(|d| done.contains(d))).collect();
    ready.sort_by(|a, b| unlocks(&b.id).cmp(&unlocks(&a.id)).then_with(|| a.id.cmp(&b.id)));
    ready.into_iter().map(|t| t.id.clone()).collect()
}

/// `true` quando `path` é um padrão de arquivo: tem `*`, `?` ou `[`.
fn is_pattern(path: &str) -> bool {
    path.contains(['*', '?', '['])
}

/// `true` quando `path` é o curinga da árvore inteira — só estrelas, como o
/// `**` que a conversão da spec antiga grava na tarefa sem arquivo conhecido.
/// É a mesma leitura do "vale para todo arquivo" do `applies_to`.
pub(crate) fn is_whole_tree(path: &str) -> bool {
    !path.is_empty() && path.chars().all(|c| c == '*')
}

/// `true` quando algum arquivo de `files` é o curinga da árvore inteira.
pub(crate) fn touches_whole_tree<'a>(files: impl IntoIterator<Item = &'a String>) -> bool {
    files.into_iter().any(|f| is_whole_tree(f))
}

/// O trecho fixo de um padrão: tudo antes do primeiro `*`, `?` ou `[`.
fn fixed_prefix(pattern: &str) -> &str {
    pattern.find(['*', '?', '[']).map_or(pattern, |at| &pattern[..at])
}

/// `true` quando o padrão `pattern` casa o caminho `path`. O padrão só com
/// `*` usa a mesma leitura do `applies_to` ([`mustard_core::glob_matches`]);
/// essa leitura não conhece `?` nem `[`, então o padrão que os tem casa, por
/// cautela, todo caminho que começa pelo trecho fixo dele — dividir arquivo
/// entre dois lotes em paralelo custa mais que esperar uma vez a mais.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    if pattern.contains(['?', '[']) {
        return path.starts_with(fixed_prefix(pattern));
    }
    mustard_core::glob_matches(pattern, path)
}

/// `true` quando os arquivos `a` e `b` se cruzam: o mesmo caminho, ou um
/// padrão que casa o outro caminho. Dois padrões se cruzam quando o trecho
/// fixo de um começa pelo do outro — `src/**` e `src/x/*.rs` podem casar o
/// mesmo arquivo, `src/**` e `docs/*` nunca. O `**` cruza com todo arquivo.
pub(crate) fn files_cross(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (is_pattern(a), is_pattern(b)) {
        (false, false) => false,
        (true, false) => pattern_matches(a, b),
        (false, true) => pattern_matches(b, a),
        (true, true) => {
            let (pa, pb) = (fixed_prefix(a), fixed_prefix(b));
            pa.starts_with(pb) || pb.starts_with(pa)
        }
    }
}

/// `true` quando algum arquivo de `left` cruza com algum de `right`
/// ([`files_cross`]).
pub(crate) fn sets_cross<'a, 'b>(
    left: impl IntoIterator<Item = &'a String>,
    right: impl IntoIterator<Item = &'b String> + Clone,
) -> bool {
    left.into_iter().any(|a| right.clone().into_iter().any(|b| files_cross(a, b)))
}

/// As partes independentes de `order`: tarefas cujos arquivos se cruzam
/// ([`files_cross`]), direto ou por uma corrente de outras, caem na mesma
/// parte — é o que garante que dois lotes em paralelo nunca dividam arquivo
/// (nenhum empacotamento adiante separa o que já chegou junto aqui). Tarefa
/// sem arquivo declarado forma parte própria, sozinha: sem arquivo não há
/// com que dividir. A tarefa com o curinga da árvore inteira também forma
/// parte própria, sozinha, e nenhuma outra entra nela: ela cruza com todas,
/// e quem a impede de rodar ao lado de outra onda é a rodada, que nunca a
/// solta junto de ninguém.
fn clustered_by_file<N: Ord + Clone>(tasks: &[BasketTask<N>], order: &[N]) -> Vec<Batch<N>> {
    let by_id: BTreeMap<&N, &BasketTask<N>> = tasks.iter().map(|t| (&t.id, t)).collect();
    let mut groups: Vec<Batch<N>> = Vec::new();
    for id in order {
        let Some(task) = by_id.get(id).copied() else { continue };
        if task.files.is_empty() || touches_whole_tree(&task.files) {
            groups.push(Batch { tasks: vec![id.clone()], files: task.files.clone() });
            continue;
        }
        let mut merged = Batch { tasks: vec![id.clone()], files: task.files.clone() };
        let mut rest: Vec<Batch<N>> = Vec::new();
        for group in groups.drain(..) {
            if !touches_whole_tree(&group.files) && sets_cross(&group.files, &task.files) {
                merged.tasks.extend(group.tasks);
                merged.files.extend(group.files);
            } else {
                rest.push(group);
            }
        }
        rest.push(merged);
        groups = rest;
    }
    groups
}

/// Empacota `order` (a saída de [`ready_tasks`]) em lotes de despacho: do
/// maior para o menor, cada tarefa — ou a parte inteira que o arquivo
/// compartilhado a prendeu, ver [`clustered_by_file`] — no primeiro lote
/// onde ainda couber, medindo pelo número de arquivos distintos do lote. A
/// parte que sozinha já passa de `capacity` sai sozinha, acima dela: a
/// capacidade nunca impede o despacho.
///
/// A tarefa com o curinga da árvore inteira vem antes de tudo, cada uma no
/// seu lote, sem nenhuma outra dentro: o lote dela ganha o número de onda
/// mais baixo da leva, e a rodada, que solta pela ordem do número, a solta
/// primeiro e segura as outras até ela entregar.
pub(crate) fn pack_batches<N: Ord + Clone>(
    tasks: &[BasketTask<N>],
    order: &[N],
    capacity: usize,
) -> Vec<Batch<N>> {
    let (mut batches, mut groups): (Vec<Batch<N>>, Vec<Batch<N>>) =
        clustered_by_file(tasks, order).into_iter().partition(|g| touches_whole_tree(&g.files));
    let alone = batches.len();
    // Maior primeiro; `sort_by` é estável, então empate preserva a ordem de
    // prontidão (desempate por destrava, depois por número) que `order` já
    // carrega.
    groups.sort_by_key(|g| std::cmp::Reverse(g.files.len()));

    for group in groups {
        match batches.iter_mut().skip(alone).find(|b| b.files.len() + group.files.len() <= capacity) {
            Some(batch) => {
                batch.tasks.extend(group.tasks);
                batch.files.extend(group.files);
            }
            None => batches.push(group),
        }
    }
    batches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(pairs: &[(u32, &[u32])]) -> BTreeMap<u32, BTreeSet<u32>> {
        pairs.iter().map(|(n, d)| (*n, d.iter().copied().collect())).collect()
    }

    #[test]
    fn a_chain_is_sequential() {
        let l = assign_levels(&graph(&[(1, &[]), (2, &[1]), (3, &[2])]));
        assert!(l.cycle.is_empty());
        assert_eq!(l.level[&1], 0);
        assert_eq!(l.level[&2], 1);
        assert_eq!(l.level[&3], 2);
    }

    #[test]
    fn independent_nodes_share_a_round() {
        let l = assign_levels(&graph(&[(1, &[]), (2, &[1]), (3, &[1])]));
        assert_eq!(l.level[&1], 0);
        assert_eq!(l.level[&2], 1);
        assert_eq!(l.level[&3], 1);
    }

    #[test]
    fn a_loop_is_named_and_its_members_share_a_level() {
        let l = assign_levels(&graph(&[(1, &[2]), (2, &[1])]));
        assert_eq!(l.cycle, vec![1, 2]);
        assert_eq!(l.level[&1], l.level[&2]);
    }

    /// What merely WAITS behind a loop is not on it, and sits above it.
    #[test]
    fn what_waits_behind_a_loop_is_not_named() {
        let l = assign_levels(&graph(&[(1, &[]), (2, &[3]), (3, &[2]), (4, &[3])]));
        assert_eq!(l.cycle, vec![2, 3], "only the loop's own members");
        assert!(l.level[&4] > l.level[&3], "what waits behind sits above");
    }

    /// A node BETWEEN two loops is on neither. No peel of unplaced nodes can
    /// say that — it has an edge in and an edge out inside the stuck set — but
    /// "does it reach itself" answers in one step.
    #[test]
    fn a_node_between_two_loops_is_not_named() {
        let l = assign_levels(&graph(&[(2, &[3]), (3, &[2, 4]), (4, &[5]), (5, &[6]), (6, &[5])]));
        assert_eq!(l.cycle, vec![2, 3, 5, 6]);
        assert!(!l.cycle.contains(&4));
    }

    /// Ordering holds ACROSS two distinct loops.
    #[test]
    fn two_distinct_loops_are_ordered() {
        let l = assign_levels(&graph(&[(2, &[3]), (3, &[2, 5]), (5, &[6]), (6, &[5])]));
        assert_eq!(l.level[&5], l.level[&6]);
        assert_eq!(l.level[&2], l.level[&3]);
        assert!(l.level[&2] > l.level[&5], "the loop that depends sits above");
    }

    #[test]
    fn an_out_of_graph_edge_is_ignored() {
        let l = assign_levels(&graph(&[(1, &[]), (2, &[99])]));
        assert!(l.cycle.is_empty());
        assert_eq!(l.level[&2], 0);
    }

    #[test]
    fn an_empty_graph_is_empty() {
        let l = assign_levels(&graph(&[]));
        assert!(l.level.is_empty());
        assert!(l.cycle.is_empty());
    }

    /// Uma tarefa da cesta, para os testes: número, dependências, arquivos e
    /// se já está entregue ou aprovada.
    fn task(id: u32, depends_on: &[u32], files: &[&str], done: bool) -> BasketTask<u32> {
        BasketTask {
            id,
            depends_on: depends_on.iter().copied().collect(),
            files: files.iter().map(|f| f.to_string()).collect(),
            done,
        }
    }

    /// O nível topológico de uma tarefa da cesta espera a dependência dela:
    /// mesmo grafo do teste de ponta a ponta do despacho
    /// (`apps/rt/tests/round_dispatch.rs`), só que sobre o nível, não o lote.
    #[test]
    fn a_task_waits_the_level_of_its_dependency() {
        let tasks = [
            task(1, &[], &["a.rs", "b.rs"], false),
            task(2, &[], &["c.rs"], false),
            task(3, &[1], &["d.rs"], false),
            task(4, &[], &["e.rs", "f.rs", "g.rs"], false),
            task(5, &[], &["b.rs"], false),
            task(6, &[4], &["h.rs"], false),
        ];
        let levels = task_levels(&tasks);
        assert!(levels.cycle.is_empty());
        assert_eq!(levels.level[&1], 0);
        assert_eq!(levels.level[&3], 1, "a 3 espera a 1");
        assert_eq!(levels.level[&6], 1, "a 6 espera a 4");
    }

    /// O exemplo da regra em código, com os dois lados: a tarefa 7 depende
    /// das tarefas 3 e 5; com a 3 entregue e a 5 aberta, a 7 não entra em
    /// lote (o "antes" falha); entregue a 5 também, a 7 entra na rodada
    /// seguinte.
    #[test]
    fn a_task_is_ready_only_once_every_dependency_is_done() {
        let five_open = [
            task(3, &[], &["a.rs"], true),
            task(5, &[], &["e.rs"], false),
            task(7, &[3, 5], &["b.rs"], false),
        ];
        assert_eq!(ready_tasks(&five_open), vec![5], "3 entregue, mas a 5 ainda está aberta: a 7 espera");

        let five_done = [
            task(3, &[], &["a.rs"], true),
            task(5, &[], &["e.rs"], true),
            task(7, &[3, 5], &["b.rs"], false),
        ];
        assert_eq!(ready_tasks(&five_done), vec![7], "entregue a 5 também, a 7 entra na rodada seguinte");
    }

    /// Entre as prontas, quem destrava mais tarefas vai primeiro; empatando,
    /// o número menor primeiro.
    #[test]
    fn ready_tasks_break_ties_by_how_many_they_unlock_then_by_number() {
        let tasks = [
            task(4, &[], &[], false),
            task(9, &[], &[], false),
            task(1, &[4], &[], false),
            task(2, &[9], &[], false),
        ];
        assert_eq!(ready_tasks(&tasks), vec![4, 9], "a 4 e a 9 destravam uma cada; a 4 vem pelo número");

        let tied = [task(5, &[], &[], false), task(3, &[], &[], false)];
        assert_eq!(ready_tasks(&tied), vec![3, 5], "sem ninguém destravado, decide o número");
    }

    /// O exemplo da regra de empacotamento: A com 4 arquivos, B com 2, C com
    /// 1, capacidade 5. A abre o lote 1; B não cabe e abre o lote 2; C cabe
    /// no lote 1, que fecha com 5.
    #[test]
    fn packing_goes_from_largest_to_smallest_into_the_first_batch_that_fits() {
        let tasks = [
            task(1, &[], &["a1.rs", "a2.rs", "a3.rs", "a4.rs"], false),
            task(2, &[], &["b1.rs", "b2.rs"], false),
            task(3, &[], &["c1.rs"], false),
        ];
        let order = ready_tasks(&tasks);
        let batches = pack_batches(&tasks, &order, BASKET_CAPACITY);
        assert_eq!(batches.len(), 2, "{batches:?}");
        assert_eq!(batches[0].tasks, vec![1, 3], "a maior abre, a menor fecha o lote em 5");
        assert_eq!(batches[0].files.len(), 5);
        assert_eq!(batches[1].tasks, vec![2]);
    }

    /// Uma tarefa sozinha que toque mais arquivo que a capacidade sai
    /// sozinha, acima dela: a capacidade nunca impede o despacho.
    #[test]
    fn a_task_alone_over_capacity_ships_alone() {
        let tasks = [task(1, &[], &["a.rs", "b.rs", "c.rs", "d.rs", "e.rs", "f.rs"], false)];
        let order = ready_tasks(&tasks);
        let batches = pack_batches(&tasks, &order, BASKET_CAPACITY);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].tasks, vec![1]);
        assert_eq!(batches[0].files.len(), 6, "acima da capacidade de 5, e mesmo assim despachada");
    }

    /// Duas tarefas prontas que tocam o mesmo arquivo podem cair no mesmo
    /// lote; o que nunca acontece é caírem em lotes diferentes.
    #[test]
    fn two_ready_tasks_sharing_a_file_never_split_across_batches() {
        let tasks = [task(1, &[], &["shared.rs"], false), task(2, &[], &["shared.rs"], false)];
        let order = ready_tasks(&tasks);
        let batches = pack_batches(&tasks, &order, BASKET_CAPACITY);
        assert_eq!(batches.len(), 1, "{batches:?}");
        assert_eq!(batches[0].tasks.len(), 2);
    }

    /// Cesta vazia: nenhuma tarefa pronta, nenhum lote.
    #[test]
    fn an_empty_basket_has_no_ready_task_and_no_batch() {
        let tasks: [BasketTask<u32>; 0] = [];
        assert!(ready_tasks(&tasks).is_empty());
        assert!(pack_batches(&tasks, &[], BASKET_CAPACITY).is_empty());
    }

    /// O casamento de padrão, na divisa: o padrão cruza com o caminho que
    /// ele casa e não com o vizinho que só parece; dois padrões cruzam quando
    /// podem casar o mesmo arquivo; o `**` cruza com tudo.
    #[test]
    fn a_tarefa_com_curinga_sai_sozinha_pelo_casamento_de_padrao() {
        assert!(files_cross("src/**", "src/a.rs"));
        assert!(!files_cross("src/**", "srcx/a.rs"), "o trecho fixo é src/, não src");
        assert!(files_cross("src/**", "src/x/*.rs"));
        assert!(!files_cross("src/**", "docs/*"));
        assert!(files_cross("**", "docs/a.md") && files_cross("docs/*", "**"));
        assert!(!files_cross("a.rs", "b.rs"));
        assert!(files_cross("src/a?.rs", "src/ab.rs"));
        assert!(!files_cross("src/a?.rs", "lib/ab.rs"));
    }

    /// No empacotamento, a tarefa do curinga abre o primeiro lote, sozinha,
    /// e as outras seguem empacotadas entre si.
    #[test]
    fn a_tarefa_com_curinga_sai_sozinha_no_empacotamento() {
        let tasks = [task(1, &[], &["a.rs"], false), task(2, &[], &["**"], false), task(3, &[], &["b.rs"], false)];
        let order = ready_tasks(&tasks);
        let batches = pack_batches(&tasks, &order, BASKET_CAPACITY);
        assert_eq!(batches.len(), 2, "{batches:?}");
        assert_eq!(batches[0].tasks, vec![2], "o curinga primeiro, sozinho");
        assert_eq!(batches[1].tasks, vec![1, 3]);
    }

    // A cesta inteira, com dependência e arquivo compartilhado, despachada
    // de verdade — não só pela função pura — mora agora em
    // `apps/rt/tests/round_dispatch.rs`: o critério fala em despacho pelo
    // binário, num repositório temporário, e não em chamar `pack_batches`
    // duas vezes.
}
