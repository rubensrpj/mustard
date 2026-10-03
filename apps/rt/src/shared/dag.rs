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
//! ## O backlog
//!
//! Além do nível, o módulo também forma o backlog de despacho: tarefa pronta —
//! toda dependência entregue ou aprovada —, o desempate entre prontas por
//! quantas outras cada uma destrava, e o agrupamento delas em lotes de um
//! assunto só: as tarefas que dividem arquivo, direto ou por uma corrente de
//! outras, caem no mesmo lote, e nenhum arquivo é dividido entre dois lotes.
//! O lote não tem teto de tarefas nem de arquivos, e dois grupos sem arquivo
//! em comum nunca se juntam: quem segura o tamanho da conversa é o limite do
//! agente. A tarefa que espera só por tarefas do mesmo lote, ou já entregues,
//! e divide arquivo com ele entra no lote, depois delas. É o mesmo grafo e o
//! mesmo peel de [`assign_levels`], só que sobre tarefas: nasce aqui para não
//! virar um segundo motor ao lado.
//!
//! Com o julgamento do Jev sobre o backlog ([`Judgement`]), o assunto do lote
//! é o tipo de trabalho ([`TaskKind`]) e não o arquivo: [`pack_by_kind`] junta
//! as tarefas do mesmo tipo, mesmo sem arquivo em comum, e põe as ondas na
//! ordem fixa dos tipos. O arquivo e a dependência seguem exatos, em código:
//! quem decide que duas ondas com arquivo em comum não saem juntas é a rodada.
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

/// Uma tarefa do backlog: o que ela depende, os arquivos que declara e se o
/// trabalho dela já está entregue ou aprovado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BacklogTask<N> {
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

/// As tarefas prontas — todas as dependências entregues ou aprovadas —, na
/// ordem de despacho: quem destrava mais tarefas primeiro, empate pelo
/// próprio número, do menor para o maior. "Destravar" conta só a aresta
/// direta de volta (quantas tarefas do backlog, prontas ou não, apontam esta
/// como dependência) — uma tarefa dois saltos adiante não está presa só
/// nesta.
pub(crate) fn ready_tasks<N: Ord + Clone>(tasks: &[BacklogTask<N>]) -> Vec<N> {
    let done: BTreeSet<&N> = tasks.iter().filter(|t| t.done).map(|t| &t.id).collect();
    let unlocks = |id: &N| -> usize { tasks.iter().filter(|t| t.depends_on.contains(id)).count() };

    let mut ready: Vec<&BacklogTask<N>> =
        tasks.iter().filter(|t| !t.done && t.depends_on.iter().all(|d| done.contains(d))).collect();
    ready.sort_by(|a, b| unlocks(&b.id).cmp(&unlocks(&a.id)).then_with(|| a.id.cmp(&b.id)));
    ready.into_iter().map(|t| t.id.clone()).collect()
}

/// `true` quando `path` é um padrão de arquivo: tem `*`, `?` ou `[`.
fn is_pattern(path: &str) -> bool {
    path.contains(['*', '?', '['])
}

/// `true` quando `path` é o curinga da árvore inteira — só estrelas, como `**`.
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
fn clustered_by_file<N: Ord + Clone>(tasks: &[BacklogTask<N>], order: &[N]) -> Vec<Batch<N>> {
    let by_id: BTreeMap<&N, &BacklogTask<N>> = tasks.iter().map(|t| (&t.id, t)).collect();
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

/// Agrupa `order` (a saída de [`ready_tasks`]) em lotes de despacho, um
/// assunto por lote:
///
/// 1. A tarefa com o curinga da árvore inteira vem antes de tudo, cada uma no
///    seu lote, sem nenhuma outra dentro: ela cruza com todas, e quem a
///    impede de rodar ao lado de outra onda é a rodada. As demais seguem em
///    partes: a tarefa e todas as que o arquivo compartilhado prendeu a ela
///    ([`clustered_by_file`]). Cada parte é um lote inteiro, de qualquer
///    tamanho, com as tarefas na ordem de prontidão de `order`; duas partes
///    sem arquivo em comum nunca se juntam num lote.
/// 2. As partes seguem na ordem em que a primeira tarefa de cada uma aparece
///    em `order`. A que cruza arquivo de uma onda ainda aberta (`busy`) é um
///    lote como os outros, sozinho; quem decide que ela espera é quem solta.
/// 3. Cada lote recebe, no fim, as tarefas de `waiting` que esperam só por
///    ele ([`chain_dependents`]).
pub(crate) fn pack_batches<N: Ord + Clone>(
    tasks: &[BacklogTask<N>],
    order: &[N],
    waiting: &[N],
    busy: &BTreeSet<String>,
) -> Vec<Batch<N>> {
    let readiness = |id: &N| order.iter().position(|ready| ready == id).unwrap_or(usize::MAX);
    let (mut batches, mut groups): (Vec<Batch<N>>, Vec<Batch<N>>) = clustered_by_file(tasks, order)
        .into_iter()
        .map(|mut group| {
            group.tasks.sort_by_key(&readiness);
            group
        })
        .partition(|g| touches_whole_tree(&g.files));
    groups.sort_by_key(|g| g.tasks.iter().map(&readiness).min());
    batches.extend(groups);
    chain_dependents(tasks, &mut batches, waiting, busy);
    batches
}

/// O tipo de trabalho de uma tarefa, como o Jev o diz. A ordem das variantes
/// é a ordem em que as ondas saem: defeito primeiro, recurso novo, texto e, no
/// fim, o que só tira ou enxuga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TaskKind {
    /// Conserta o que está errado para o usuário ou quebra.
    Defect,
    /// Acrescenta ou muda o que o usuário vê.
    Feature,
    /// Conserta comentário, nome ou texto.
    TextFix,
    /// Tira o que nada mais usa.
    RemoveUnused,
    /// Enxuga teste sem mudar o produto.
    TestCleanup,
}

impl TaskKind {
    /// Todos os tipos, na ordem em que as ondas saem.
    pub(crate) const ALL: [Self; 5] =
        [Self::Defect, Self::Feature, Self::TextFix, Self::RemoveUnused, Self::TestCleanup];

    /// O nome do tipo na conversa com o Jev.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Defect => "defect",
            Self::Feature => "feature",
            Self::TextFix => "text_fix",
            Self::RemoveUnused => "remove_unused",
            Self::TestCleanup => "test_cleanup",
        }
    }

    /// O tipo de nome `key`; `None` para o nome que não é de tipo nenhum.
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }
}

/// A confiança no tipo a partir da qual a tarefa vai com as do mesmo tipo;
/// abaixo dela, vai sozinha.
pub(crate) const KIND_SURE_FROM: f64 = 0.5;

/// A chance de a tarefa mudar o mesmo que uma onda em andamento a partir da
/// qual ela espera, mesmo sem arquivo em comum com a onda.
pub(crate) const CLASH_FROM: f64 = 0.5;

/// O que o Jev julgou de uma tarefa do backlog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Judgement {
    pub(crate) kind: TaskKind,
    /// A confiança no tipo, de 0 a 1.
    pub(crate) confidence: f64,
    /// A maior chance, entre as ondas em andamento, de a tarefa mudar o mesmo
    /// que ela, de 0 a 1.
    pub(crate) clash: f64,
}

/// O julgamento da tarefa que o Jev não julgou: tipo incerto, que vai sozinha.
const UNJUDGED: Judgement = Judgement { kind: TaskKind::Feature, confidence: 0.0, clash: 0.0 };

/// Agrupa `order` (a saída de [`ready_tasks`]) em lotes de despacho pelo tipo
/// de trabalho que o Jev julgou (`judged`), um tipo por lote, na ordem em que
/// as ondas saem:
///
/// 1. A tarefa com o curinga da árvore inteira vem antes de tudo, cada uma no
///    seu lote, sem nenhuma outra dentro, como em [`pack_batches`].
/// 2. As tarefas do mesmo tipo vão no mesmo lote, tenham ou não arquivo em
///    comum, sem teto de tarefas. A de tipo incerto — confiança abaixo de
///    [`KIND_SURE_FROM`] — vai sozinha, num lote dela. A que divide arquivo
///    com uma onda aberta (`busy`) ou muda o mesmo que ela ([`CLASH_FROM`])
///    fica fora de todo lote e espera no backlog.
/// 3. Os lotes saem na ordem de [`TaskKind`] e, dentro do tipo, pelo número
///    (`number`) da tarefa mais baixa; as tarefas de cada lote, pelo número.
/// 4. Cada lote recebe, no fim, as tarefas de `waiting` que esperam só por
///    ele ([`chain_dependents`]).
///
/// Dois lotes de tipos diferentes podem dividir arquivo: quem solta só deixa
/// sair, juntos, os que não se cruzam.
pub(crate) fn pack_by_kind<N: Ord + Clone>(
    tasks: &[BacklogTask<N>],
    order: &[N],
    waiting: &[N],
    busy: &BTreeSet<String>,
    judged: &BTreeMap<N, Judgement>,
    number: &dyn Fn(&N) -> u64,
) -> Vec<Batch<N>> {
    let by_id: BTreeMap<&N, &BacklogTask<N>> = tasks.iter().map(|t| (&t.id, t)).collect();
    let mut batches: Vec<Batch<N>> = Vec::new();
    let mut together: BTreeMap<TaskKind, Batch<N>> = BTreeMap::new();
    let mut alone: Vec<(TaskKind, Batch<N>)> = Vec::new();
    for id in order {
        let Some(task) = by_id.get(id).copied() else { continue };
        if touches_whole_tree(&task.files) {
            batches.push(Batch { tasks: vec![id.clone()], files: task.files.clone() });
            continue;
        }
        let verdict = judged.get(id).copied().unwrap_or(UNJUDGED);
        if sets_cross(&task.files, busy) || verdict.clash >= CLASH_FROM {
            continue;
        }
        let single = || Batch { tasks: vec![id.clone()], files: task.files.clone() };
        if verdict.confidence < KIND_SURE_FROM {
            alone.push((verdict.kind, single()));
            continue;
        }
        let batch = together.entry(verdict.kind).or_insert_with(|| Batch { tasks: Vec::new(), files: BTreeSet::new() });
        batch.tasks.push(id.clone());
        batch.files.extend(task.files.iter().cloned());
    }
    let mut groups: Vec<(TaskKind, Batch<N>)> = together.into_iter().chain(alone).collect();
    for (_, group) in &mut groups {
        group.tasks.sort_by_key(|id| (number(id), id.clone()));
    }
    groups.sort_by_key(|(kind, group)| (*kind, group.tasks.iter().map(number).min()));
    batches.extend(groups.into_iter().map(|(_, group)| group));
    chain_dependents(tasks, &mut batches, waiting, busy);
    batches
}

/// Põe no fim de cada lote, um lote de cada vez e na ordem deles, as tarefas
/// de `waiting` que esperam só por ele, na ordem em que `waiting` as traz.
/// Entra a tarefa que: não está feita nem em lote nenhum; tem cada
/// dependência feita ou já dentro deste lote; divide arquivo com o lote
/// ([`sets_cross`]); não declara o curinga da árvore inteira; não cruza
/// arquivo de outro lote desta passada nem de uma onda aberta (`busy`). A cada
/// tarefa que entra a varredura recomeça, porque a que entrou pode ser a dependência
/// que faltava a outra; o lote para quando nenhuma entra. A dependente fica
/// depois das dependências dela, porque só entra quando elas já estão lá. O
/// lote do curinga nunca recebe: ele sai sozinho.
fn chain_dependents<N: Ord + Clone>(
    tasks: &[BacklogTask<N>],
    batches: &mut [Batch<N>],
    waiting: &[N],
    busy: &BTreeSet<String>,
) {
    let by_id: BTreeMap<&N, &BacklogTask<N>> = tasks.iter().map(|t| (&t.id, t)).collect();
    let done: BTreeSet<&N> = tasks.iter().filter(|t| t.done).map(|t| &t.id).collect();
    let mut placed: BTreeSet<N> = batches.iter().flat_map(|b| b.tasks.iter().cloned()).collect();
    for at in 0..batches.len() {
        while let Some(batch) = batches.get(at) {
            if touches_whole_tree(&batch.files) {
                break;
            }
            let crosses_another = |files: &BTreeSet<String>| {
                batches.iter().enumerate().any(|(other, b)| other != at && sets_cross(files, &b.files))
            };
            let entering = waiting.iter().filter_map(|id| by_id.get(id).copied()).find(|task| {
                !task.done
                    && !placed.contains(&task.id)
                    && task.depends_on.iter().all(|d| done.contains(d) || batch.tasks.contains(d))
                    && sets_cross(&task.files, &batch.files)
                    && !touches_whole_tree(&task.files)
                    && !sets_cross(&task.files, busy)
                    && !crosses_another(&task.files)
            });
            let Some(task) = entering else { break };
            placed.insert(task.id.clone());
            if let Some(batch) = batches.get_mut(at) {
                batch.tasks.push(task.id.clone());
                batch.files.extend(task.files.iter().cloned());
            }
        }
    }
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

    /// Uma tarefa do backlog, para os testes: número, dependências, arquivos e
    /// se já está entregue ou aprovada.
    fn task(id: u32, depends_on: &[u32], files: &[&str], done: bool) -> BacklogTask<u32> {
        BacklogTask {
            id,
            depends_on: depends_on.iter().copied().collect(),
            files: files.iter().map(|f| f.to_string()).collect(),
            done,
        }
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

    /// Três tarefas prontas sem arquivo em comum saem em três lotes, um
    /// assunto por lote, na ordem de prontidão: nenhum lote junta partes sem
    /// relação, por maior que seja o espaço que ainda sobraria nele.
    #[test]
    fn groups_with_no_file_in_common_never_share_a_batch() {
        let tasks = [
            task(1, &[], &["a1.rs", "a2.rs", "a3.rs", "a4.rs"], false),
            task(2, &[], &["b1.rs", "b2.rs"], false),
            task(3, &[], &["c1.rs"], false),
        ];
        let order = ready_tasks(&tasks);
        let batches = pack_batches(&tasks, &order, &[], &BTreeSet::new());
        assert_eq!(batch_tasks(&batches), vec![vec![1], vec![2], vec![3]], "{batches:?}");
        assert_eq!(batches[0].files.len(), 4);
    }

    /// Uma tarefa com muitos arquivos sai inteira, no lote dela: o lote não
    /// tem teto de arquivos.
    #[test]
    fn a_task_with_many_files_ships_whole() {
        let files: Vec<String> = (0..60).map(|n| format!("f{n}.rs")).collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        let tasks = [task(1, &[], &files, false)];
        let order = ready_tasks(&tasks);
        let batches = pack_batches(&tasks, &order, &[], &BTreeSet::new());
        assert_eq!(batch_tasks(&batches), vec![vec![1]]);
        assert_eq!(batches[0].files.len(), 60);
    }

    /// Duas tarefas prontas que tocam o mesmo arquivo podem cair no mesmo
    /// lote; o que nunca acontece é caírem em lotes diferentes.
    #[test]
    fn two_ready_tasks_sharing_a_file_never_split_across_batches() {
        let tasks = [task(1, &[], &["shared.rs"], false), task(2, &[], &["shared.rs"], false)];
        let order = ready_tasks(&tasks);
        let batches = pack_batches(&tasks, &order, &[], &BTreeSet::new());
        assert_eq!(batches.len(), 1, "{batches:?}");
        assert_eq!(batches[0].tasks.len(), 2);
    }

    /// Backlog vazio: nenhuma tarefa pronta, nenhum lote.
    #[test]
    fn an_empty_backlog_has_no_ready_task_and_no_batch() {
        let tasks: [BacklogTask<u32>; 0] = [];
        assert!(ready_tasks(&tasks).is_empty());
        assert!(pack_batches(&tasks, &[], &[], &BTreeSet::new()).is_empty());
    }

    /// O casamento de padrão, na divisa: o padrão cruza com o caminho que
    /// ele casa e não com o vizinho que só parece; dois padrões cruzam quando
    /// podem casar o mesmo arquivo; o `**` cruza com tudo.
    #[test]
    fn task_with_a_wildcard_goes_out_alone_by_the_pattern_match() {
        assert!(files_cross("src/**", "src/a.rs"));
        assert!(!files_cross("src/**", "srcx/a.rs"), "o trecho fixo é src/, não src");
        assert!(files_cross("src/**", "src/x/*.rs"));
        assert!(!files_cross("src/**", "docs/*"));
        assert!(files_cross("**", "docs/a.md") && files_cross("docs/*", "**"));
        assert!(!files_cross("a.rs", "b.rs"));
        assert!(files_cross("src/a?.rs", "src/ab.rs"));
        assert!(!files_cross("src/a?.rs", "lib/ab.rs"));
    }

    /// No agrupamento, a tarefa do curinga abre o primeiro lote, sozinha, e
    /// as outras seguem cada uma no seu lote. A tarefa que espera só pela do
    /// curinga não entra no lote dele: ele sai sozinho.
    #[test]
    fn task_with_a_wildcard_goes_out_alone_in_the_packing() {
        let tasks = [
            task(1, &[], &["a.rs"], false),
            task(2, &[], &["**"], false),
            task(3, &[], &["b.rs"], false),
            task(4, &[2], &["c.rs"], false),
        ];
        let order = ready_tasks(&tasks);
        let batches = pack_batches(&tasks, &order, &[4], &BTreeSet::new());
        assert_eq!(batch_tasks(&batches), vec![vec![2], vec![1], vec![3]], "{batches:?}");
        assert!(batches.iter().all(|b| !b.tasks.contains(&4)), "a dependente do curinga fica fora de todos os lotes");
    }

    /// Os lotes de uma passada, só com as tarefas de cada um, na ordem.
    fn batch_tasks(batches: &[Batch<u32>]) -> Vec<Vec<u32>> {
        batches.iter().map(|b| b.tasks.clone()).collect()
    }

    /// Os arquivos `<prefix>1.rs` a `<prefix><count>.rs`, mais `shared.rs`.
    fn own_files(prefix: &str, count: usize) -> Vec<String> {
        std::iter::once("shared.rs".to_string()).chain((1..=count).map(|n| format!("{prefix}{n}.rs"))).collect()
    }

    /// Uma tarefa com arquivos montados em tempo de teste.
    fn task_with(id: u32, depends_on: &[u32], files: &[String]) -> BacklogTask<u32> {
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        task(id, depends_on, &files, false)
    }

    /// A 2 espera a 1 e divide `a.rs` com ela; a 3 espera a 2 e divide `b.rs`
    /// com ela. Só a 1 está pronta, e o lote leva as três, cada dependente
    /// depois da dependência, mesmo com a 3 chegando antes da 2 na espera.
    #[test]
    fn dependent_that_shares_a_file_joins_the_batch_after_its_dependency() {
        let tasks = [
            task(1, &[], &["a.rs"], false),
            task(2, &[1], &["a.rs", "b.rs"], false),
            task(3, &[2], &["b.rs"], false),
        ];
        let batches = pack_batches(&tasks, &[1], &[3, 2], &BTreeSet::new());
        assert_eq!(batch_tasks(&batches), vec![vec![1, 2, 3]], "{batches:?}");
        assert_eq!(batches[0].files, BTreeSet::from(["a.rs".to_string(), "b.rs".to_string()]));
    }

    /// A 2 espera só a 1, mas não divide arquivo com ela; a 3 divide `a.rs`
    /// com a 1, mas espera também a 7, ainda aberta e fora do lote. As duas
    /// ficam fora: o lote leva só a 1.
    #[test]
    fn dependent_without_a_shared_file_or_with_an_open_dependency_stays_out() {
        let tasks = [
            task(1, &[], &["a.rs"], false),
            task(2, &[1], &["z.rs"], false),
            task(3, &[1, 7], &["a.rs"], false),
            task(7, &[], &["q.rs"], false),
        ];
        let batches = pack_batches(&tasks, &[1], &[2, 3], &BTreeSet::new());
        assert_eq!(batch_tasks(&batches), vec![vec![1]], "{batches:?}");
    }

    /// A 1 e a 2 saem em dois lotes. As três dependentes da 1 dividem
    /// `a1.rs` ou `a2.rs` com ela: a 3 também toca
    /// `b1.rs`, do outro lote, e fica fora; a 4 toca `c.rs`, de uma onda
    /// aberta, e fica fora; a 5 entra.
    #[test]
    fn dependent_that_crosses_another_batch_or_open_wave_stays_out() {
        let tasks = [
            task(1, &[], &["a1.rs", "a2.rs"], false),
            task(2, &[], &["b1.rs", "b2.rs"], false),
            task(3, &[1], &["a1.rs", "b1.rs"], false),
            task(4, &[1], &["a1.rs", "c.rs"], false),
            task(5, &[1], &["a2.rs"], false),
        ];
        let busy = BTreeSet::from(["c.rs".to_string()]);
        let batches = pack_batches(&tasks, &[1, 2], &[3, 4, 5], &busy);
        assert_eq!(batch_tasks(&batches), vec![vec![1, 5], vec![2]], "{batches:?}");
    }

    /// A 1 divide `a.rs` com uma onda aberta, a 2 não divide nada com
    /// ninguém: saem em dois lotes, porque a 2 não espera a onda aberta com a
    /// 1.
    #[test]
    fn part_held_by_an_open_wave_does_not_share_a_batch_with_a_free_part() {
        let tasks = [task(1, &[], &["a.rs"], false), task(2, &[], &["b.rs"], false)];
        let busy = BTreeSet::from(["a.rs".to_string()]);
        let batches = pack_batches(&tasks, &[1, 2], &[], &busy);
        assert_eq!(batch_tasks(&batches), vec![vec![1], vec![2]], "{batches:?}");
    }

    /// Seis tarefas em corrente, cada uma com `shared.rs` e cinco arquivos
    /// próprios, do tamanho das tarefas da cadeia do mapa: as seis saem num
    /// lote só, com 31 arquivos, a dependente depois da dependência.
    #[test]
    fn six_chained_tasks_the_size_of_the_map_ones_go_out_in_a_single_batch() {
        let tasks: Vec<BacklogTask<u32>> = (1..=6u32)
            .map(|n| {
                let deps: Vec<u32> = if n == 1 { Vec::new() } else { vec![n - 1] };
                task_with(n, &deps, &own_files(&format!("t{n}_"), 5))
            })
            .collect();
        let batches = pack_batches(&tasks, &[1], &[2, 3, 4, 5, 6], &BTreeSet::new());
        assert_eq!(batch_tasks(&batches), vec![vec![1, 2, 3, 4, 5, 6]], "{batches:?}");
        assert_eq!(batches[0].files.len(), 31);
    }

    /// Dezessete tarefas prontas, todas com `shared.rs`, mais um arquivo
    /// próprio cada. A ordem de prontidão põe a 17 antes das outras. Todas
    /// dividem arquivo, então saem num lote só, sem corte em cinco, na ordem
    /// de prontidão, com os arquivos das dezessete.
    #[test]
    fn more_than_five_tasks_on_one_file_go_out_in_a_single_batch() {
        let tasks: Vec<BacklogTask<u32>> =
            (1..=17u32).map(|n| task_with(n, &[], &own_files(&format!("t{n}_"), 1))).collect();
        let order: Vec<u32> = std::iter::once(17).chain(1..=16).collect();

        let batches = pack_batches(&tasks, &order, &[], &BTreeSet::new());
        assert_eq!(batches.len(), 1, "{batches:?}");
        assert_eq!(batches[0].tasks, order, "todas as dezessete, na ordem de prontidão");
        assert_eq!(batches[0].files.len(), 18, "shared.rs e um arquivo de cada tarefa");
    }

    /// Dois grupos de arquivo, cada um com uma tarefa de número baixo e uma
    /// de número alto, entram na ordem em que a primeira tarefa de cada um
    /// aparece na ordem de prontidão, e as tarefas de cada lote seguem a mesma
    /// ordem, não a ordem em que o arquivo as juntou.
    #[test]
    fn batches_and_their_tasks_follow_the_readiness_order() {
        let tasks = [
            task(1, &[], &["a.rs"], false),
            task(2, &[], &["b.rs"], false),
            task(3, &[], &["a.rs"], false),
            task(4, &[], &["b.rs"], false),
        ];
        let batches = pack_batches(&tasks, &[2, 1, 4, 3], &[], &BTreeSet::new());
        assert_eq!(batch_tasks(&batches), vec![vec![2, 4], vec![1, 3]], "{batches:?}");
    }

    /// O julgamento do Jev de uma tarefa: o tipo, a confiança nele e a chance
    /// de mudar o mesmo que uma onda em andamento.
    fn judged_as(kind: TaskKind, confidence: f64, clash: f64) -> Judgement {
        Judgement { kind, confidence, clash }
    }

    /// Os lotes por tipo de `tasks`, todas prontas, com o julgamento de cada
    /// uma; o número da tarefa é o próprio id.
    fn packed_by_kind(tasks: &[BacklogTask<u32>], judged: &[(u32, Judgement)], busy: &BTreeSet<String>) -> Vec<Vec<u32>> {
        let order: Vec<u32> = tasks.iter().map(|t| t.id).collect();
        let judged: BTreeMap<u32, Judgement> = judged.iter().copied().collect();
        let batches = pack_by_kind(tasks, &order, &[], busy, &judged, &|id| u64::from(*id));
        batch_tasks(&batches)
    }

    /// As tarefas do mesmo tipo vão juntas ainda que nenhum arquivo seja
    /// comum a elas, e a de outro tipo, com arquivo em comum, vai no lote do
    /// tipo dela: o assunto é o tipo, não o arquivo.
    #[test]
    fn tasks_of_one_kind_share_a_batch_without_a_file_in_common() {
        let tasks = [
            task(1, &[], &["a.rs"], false),
            task(2, &[], &["b.rs"], false),
            task(3, &[], &["a.rs"], false),
            task(4, &[], &["c.rs"], false),
        ];
        let sure = |kind| judged_as(kind, 0.9, 0.0);
        let judged = [
            (1, sure(TaskKind::Feature)),
            (2, sure(TaskKind::Feature)),
            (3, sure(TaskKind::Defect)),
            (4, sure(TaskKind::Feature)),
        ];
        assert_eq!(packed_by_kind(&tasks, &judged, &BTreeSet::new()), vec![vec![3], vec![1, 2, 4]]);
    }

    /// Os lotes saem na ordem fixa dos tipos, e não pelo número: o defeito
    /// antes de tudo, a limpeza de teste no fim, ainda que ela tenha o
    /// número mais baixo. Dentro do lote, as tarefas vão pelo número.
    #[test]
    fn batches_leave_in_the_fixed_order_of_the_kinds_and_tasks_by_number() {
        let tasks: Vec<BacklogTask<u32>> = (1..=6).map(|n| task(n, &[], &[], false)).collect();
        let sure = |kind| judged_as(kind, 0.9, 0.0);
        let judged = [
            (1, sure(TaskKind::TestCleanup)),
            (2, sure(TaskKind::RemoveUnused)),
            (3, sure(TaskKind::Feature)),
            (4, sure(TaskKind::TextFix)),
            (5, sure(TaskKind::Defect)),
            (6, sure(TaskKind::Feature)),
        ];
        let mut order: Vec<u32> = (1..=6).collect();
        order.reverse();
        let judged_map: BTreeMap<u32, Judgement> = judged.iter().copied().collect();
        let batches = pack_by_kind(&tasks, &order, &[], &BTreeSet::new(), &judged_map, &|id| u64::from(*id));
        assert_eq!(batch_tasks(&batches), vec![vec![5], vec![3, 6], vec![4], vec![2], vec![1]], "{batches:?}");
    }

    /// A confiança no tipo abaixo de [`KIND_SURE_FROM`] manda a tarefa
    /// sozinha para um lote dela; em cima da linha ela vai com as do tipo.
    #[test]
    fn a_kind_the_jev_is_unsure_of_goes_alone() {
        let tasks = [task(1, &[], &[], false), task(2, &[], &[], false), task(3, &[], &[], false)];
        let judged = [
            (1, judged_as(TaskKind::Feature, 0.9, 0.0)),
            (2, judged_as(TaskKind::Feature, 0.49, 0.0)),
            (3, judged_as(TaskKind::Feature, 0.5, 0.0)),
        ];
        assert_eq!(packed_by_kind(&tasks, &judged, &BTreeSet::new()), vec![vec![1, 3], vec![2]]);
    }

    /// A chance de mudar o mesmo que uma onda em andamento, de [`CLASH_FROM`]
    /// para cima, segura a tarefa que não declara arquivo nenhum; abaixo
    /// disso ela sai. A que divide arquivo com a onda aberta espera ainda que
    /// o Jev não veja choque.
    #[test]
    fn a_clash_with_an_open_wave_holds_a_task_that_declares_no_file() {
        let tasks = [task(1, &[], &[], false), task(2, &[], &[], false), task(3, &[], &["open.rs"], false)];
        let judged = [
            (1, judged_as(TaskKind::Feature, 0.9, 0.5)),
            (2, judged_as(TaskKind::Feature, 0.9, 0.49)),
            (3, judged_as(TaskKind::Feature, 0.9, 0.0)),
        ];
        let busy = BTreeSet::from(["open.rs".to_string()]);
        assert_eq!(packed_by_kind(&tasks, &judged, &busy), vec![vec![2]]);
    }

    /// O curinga da árvore inteira sai antes e sozinho, com o tipo que for, e
    /// a tarefa que espera só por uma do lote e divide arquivo com ele entra
    /// depois dela — salvo com o curinga no meio, que cruza com tudo.
    #[test]
    fn the_wildcard_goes_first_alone_and_a_dependent_joins_after_its_dependency() {
        let tasks = [
            task(1, &[], &["a.rs"], false),
            task(2, &[], &["**"], false),
            task(3, &[], &["b.rs"], false),
            task(4, &[1], &["a.rs"], false),
        ];
        let sure = |kind| judged_as(kind, 0.9, 0.0);
        let judged: BTreeMap<u32, Judgement> =
            [(1, sure(TaskKind::Feature)), (2, sure(TaskKind::Defect)), (3, sure(TaskKind::Feature))].into_iter().collect();
        let number = |id: &u32| u64::from(*id);
        let with_wildcard = pack_by_kind(&tasks, &[1, 2, 3], &[4], &BTreeSet::new(), &judged, &number);
        assert_eq!(batch_tasks(&with_wildcard), vec![vec![2], vec![1, 3]], "{with_wildcard:?}");
        let without = pack_by_kind(&tasks, &[1, 3], &[4], &BTreeSet::new(), &judged, &number);
        assert_eq!(batch_tasks(&without), vec![vec![1, 3, 4]], "{without:?}");
    }

    // O backlog inteiro, com dependência e arquivo compartilhado, despachada
    // de verdade — não só pela função pura — mora agora em
    // `apps/rt/tests/round_dispatch.rs`: o critério fala em despacho pelo
    // binário, num repositório temporário, e não em chamar `pack_batches`
    // duas vezes.
}
