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

impl<N: Ord + Clone> Levels<N> {
    /// The levels as rounds: `rounds()[0]` holds every node at level 0, and so
    /// on. Nodes within a round have no dependency between them.
    pub fn rounds(&self) -> Vec<Vec<N>> {
        let mut by_level: BTreeMap<u32, Vec<N>> = BTreeMap::new();
        for (node, &lvl) in &self.level {
            by_level.entry(lvl).or_default().push(node.clone());
        }
        by_level.into_values().collect()
    }
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
        assert_eq!(l.rounds(), vec![vec![1], vec![2], vec![3]]);
    }

    #[test]
    fn independent_nodes_share_a_round() {
        let l = assign_levels(&graph(&[(1, &[]), (2, &[1]), (3, &[1])]));
        assert_eq!(l.rounds(), vec![vec![1], vec![2, 3]]);
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
        assert!(l.rounds().is_empty());
    }
}
