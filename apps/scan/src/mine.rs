//! Layer 4 — Convention mining (framework-agnostic).
//!
//! No detector knows what a "Controller" or a "GraphQL resolver" is. We exploit
//! the one thing every convention has: it *repeats*.
//!
//! Two miners, both blind to any framework:
//!   1. Role mining — split every symbol name into tokens; an affix that pairs
//!      with many distinct remainders (Repository ↔ {Order, Product, ...}) is a
//!      role. Suffix-first, so the *entity* is whatever is left over.
//!   2. Slice mining — group symbols by entity, then CLUSTER entities by shape
//!      similarity (Jaccard) so near-identical slices collapse into one
//!      convention with a core (always present) + optional roles. For each
//!      convention we pick three real reference implementations by complexity:
//!      simple, medium, complex.

use crate::model::{CodeExample, Convention, Decl, Exemplar, Module, RoleStat};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use rayon::prelude::*;

/// An affix must pair with at least this many distinct entities to be a role.
const MIN_ROLE_PARTNERS: usize = 2;
/// A folder name must recur under at least this many distinct parent dirs to
/// count as a role folder (DTOs/, Mappers/, Services/ — one under each module).
const MIN_ROLEFOLDER_PARENTS: usize = 3;
/// A bare (single-token) class name must recur across at least this many
/// distinct path-entities to count as a role (e.g. a nested `Validator`).
const MIN_BARE_ENTITIES: usize = 3;
/// A base type must be built on by at least this many distinct entities to be
/// reported as a shared contract.
const MIN_SHARED_CONTRACT: usize = 3;
/// Entities whose role-sets are at least this similar are merged into one convention.
const JACCARD_MERGE: f32 = 0.5;
/// A convention cluster needs at least this many entities to be a convention.
const MIN_CLUSTER: usize = 2;

struct Symbol {
    kind: String,
    path: String,
    line: usize,
    loc: usize,
    tokens: Vec<String>,
    supertypes: Vec<String>,
}

pub(crate) struct Mined {
    pub roles: Vec<RoleStat>,
    pub conventions: Vec<Convention>,
    pub shared_contracts: Vec<crate::model::SharedContract>,
}

pub fn mine(
    modules: &[Module],
    degrees: &HashMap<String, (usize, usize)>,
    content: &HashMap<String, String>,
) -> Mined {
    let loc_by_path: HashMap<&str, usize> = modules.iter().map(|m| (m.path.as_str(), m.loc)).collect();
    let symbols = collect_symbols(modules, &loc_by_path);

    // --- Miner 1: roles by suffix-first frequency ---------------------------
    let mut suffix_partners: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &symbols {
        if s.tokens.len() >= 2 {
            let last = s.tokens.last().unwrap().clone();
            let remainder = s.tokens[..s.tokens.len() - 1].join("");
            suffix_partners.entry(last).or_default().insert(remainder);
        }
    }
    let role_suffixes: HashSet<String> = suffix_partners
        .iter()
        .filter(|(_, p)| p.len() >= MIN_ROLE_PARTNERS)
        .map(|(t, _)| t.clone())
        .collect();

    let mut prefix_partners: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &symbols {
        if s.tokens.len() >= 2 && !role_suffixes.contains(s.tokens.last().unwrap()) {
            let first = s.tokens[0].clone();
            let remainder = s.tokens[1..].join("");
            prefix_partners.entry(first).or_default().insert(remainder);
        }
    }
    let role_prefixes: HashSet<String> = prefix_partners
        .iter()
        .filter(|(t, p)| p.len() >= MIN_ROLE_PARTNERS && t.len() > 1)
        .map(|(t, _)| t.clone())
        .collect();

    // --- Miner 1b: role FOLDERS (convention encoded in directory names) ------
    // A folder name is a role if it recurs as the immediate parent folder under
    // many DISTINCT parent dirs (one per entity): DTOs/, Mappers/, Services/.
    // This catches roles that file-name suffixes miss or fragment. A project
    // that instead centralises a type (e.g. Domain/Entities) has a single parent
    // for that folder, fails this test, and stays on the suffix path — so the
    // rule adapts to each layout without being told which one it is.
    // A folder is a role only if it sits under many DISTINCT parent *names*
    // (DTOs under ApiKeys, Banks, Contracts, …). A module folder like Contracts/
    // sits under only `v1`/`v2` — few distinct parent names — so it is NOT a
    // role, even though it appears under several full paths. Counting parent
    // names rather than paths keeps `Modules/v1` and `Modules/v2` from inflating
    // a module folder into a false role.
    let mut folder_parents: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &symbols {
        let segs = path_segs(&s.path);
        if segs.len() >= 3 {
            let folder = segs[segs.len() - 2].to_string();
            let parent_name = segs[segs.len() - 3].to_string();
            folder_parents.entry(folder).or_default().insert(parent_name);
        }
    }
    let role_folders: HashSet<String> = folder_parents
        .iter()
        .filter(|(_, p)| p.len() >= MIN_ROLEFOLDER_PARENTS)
        .map(|(f, _)| f.clone())
        .collect();

    // Entity inferred from the path is computed inline at assignment time
    // (folder above a role-folder, else the file's own folder).

    // --- Miner 1c: bare recurring class names (e.g. a nested `Validator`) -----
    // A single-token class declared inside a role-folder file, whose name recurs
    // across many path-entities, is a role whose entity lives in the surrounding
    // folder — the only way to surface roles declared as nested types.
    let mut bare_entities: HashMap<String, HashSet<String>> = HashMap::new();
    for s in &symbols {
        if s.tokens.len() == 1 {
            let segs = path_segs(&s.path);
            if segs.len() >= 3 && role_folders.contains(segs[segs.len() - 2]) {
                bare_entities
                    .entry(s.tokens[0].clone())
                    .or_default()
                    .insert(canonical_key(segs[segs.len() - 3]));
            }
        }
    }
    let bare_roles: HashSet<String> = bare_entities
        .iter()
        .filter(|(_, e)| e.len() >= MIN_BARE_ENTITIES)
        .map(|(n, _)| n.clone())
        .collect();

    // --- Assign (role, entity) to every symbol ------------------------------
    let mut entity_display: HashMap<String, String> = HashMap::new();
    let mut slot: HashMap<(String, String), usize> = HashMap::new();
    let mut role_entities: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut role_dirs: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut role_kinds: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut entity_roles: HashMap<String, BTreeSet<String>> = HashMap::new();
    // supertype accumulators (populated only when AST parsing supplies them)
    let mut role_supertypes: HashMap<String, HashMap<String, BTreeSet<String>>> = HashMap::new();
    let mut contract_entities: HashMap<String, BTreeSet<String>> = HashMap::new();
    // collaborator accumulators: which namespaces each role pulls in, per entity
    let imports_by_path: HashMap<&str, &Vec<String>> =
        modules.iter().map(|m| (m.path.as_str(), &m.imports)).collect();
    let mut role_import_entities: HashMap<String, HashMap<String, BTreeSet<String>>> = HashMap::new();
    let mut import_roles: HashMap<String, BTreeSet<String>> = HashMap::new();

    for (i, s) in symbols.iter().enumerate() {
        let segs = path_segs(&s.path);
        let in_role_folder = segs.len() >= 3 && role_folders.contains(segs[segs.len() - 2]);

        let (role, entity): (String, String) =
            if s.tokens.len() == 1 && bare_roles.contains(&s.tokens[0]) && in_role_folder {
                // Nested/bare recurring role (e.g. `Validator` inside a Dto file).
                (s.tokens[0].clone(), segs[segs.len() - 3].to_string())
            } else if in_role_folder {
                // Role taken from the directory; entity is the folder above it.
                (role_from_folder(segs[segs.len() - 2]), segs[segs.len() - 3].to_string())
            } else if s.tokens.len() >= 2 && role_suffixes.contains(s.tokens.last().unwrap()) {
                (s.tokens.last().unwrap().clone(), entity_tokens(&s.tokens[..s.tokens.len() - 1]))
            } else if s.tokens.len() >= 2 && role_prefixes.contains(&s.tokens[0]) {
                (s.tokens[0].clone(), s.tokens[1..].join(""))
            } else {
                ("(core)".to_string(), s.tokens.join(""))
            };

        if entity.is_empty() {
            continue;
        }
        let key = canonical_key(&entity);
        if key.is_empty() {
            continue;
        }
        entity_display
            .entry(key.clone())
            .and_modify(|cur| {
                if entity.len() < cur.len() {
                    *cur = entity.clone();
                }
            })
            .or_insert_with(|| entity.clone());

        role_entities.entry(role.clone()).or_default().insert(key.clone());
        *role_dirs.entry(role.clone()).or_default().entry(abstract_entity(&parent_dir(&s.path), &entity)).or_default() += 1;
        *role_kinds.entry(role.clone()).or_default().entry(s.kind.clone()).or_default() += 1;
        entity_roles.entry(key.clone()).or_default().insert(role.clone());

        for st in &s.supertypes {
            role_supertypes.entry(role.clone()).or_default().entry(st.clone()).or_default().insert(key.clone());
            contract_entities.entry(st.clone()).or_default().insert(key.clone());
        }
        if let Some(imps) = imports_by_path.get(s.path.as_str()) {
            for imp in imps.iter() {
                let short = shorten_ns(imp);
                if short.is_empty() {
                    continue;
                }
                role_import_entities.entry(role.clone()).or_default().entry(short.clone()).or_default().insert(key.clone());
                import_roles.entry(short).or_default().insert(role.clone());
            }
        }

        let entry = slot.entry((key.clone(), role.clone()));
        match entry {
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(i);
            }
            std::collections::hash_map::Entry::Occupied(mut o) => {
                let cur = &symbols[*o.get()];
                let prefer_new = (s.kind == "interface" && cur.kind != "interface")
                    || (s.tokens.len() < cur.tokens.len());
                if prefer_new {
                    o.insert(i);
                }
            }
        }
    }

    // Entity keys, used to keep domain types from masquerading as contracts.
    let entity_keys: HashSet<String> = entity_display.keys().cloned().collect();
    // The family's contract lives in `majority_supertype` (below) so it can be
    // unit-tested in isolation: the winning base type is kept only when a MAJORITY
    // of the family shares it. `role_supertypes` counts DISTINCT entities per
    // supertype, so the winner is weighed apples-to-apples against the family size.
    let top_supertype = |r: &str, family: usize| -> Option<String> {
        majority_supertype(role_supertypes.get(r)?, family, &entity_keys)
    };
    // Collaborators: namespaces this role pulls in across many of its entities,
    // excluding ones imported by most *roles* (framework/base noise like System).
    let num_roles = role_entities.keys().filter(|r| r.as_str() != "(core)").count().max(1);
    let collaborators_for = |r: &str| -> Vec<String> {
        let m = match role_import_entities.get(r) {
            Some(m) => m,
            None => return Vec::new(),
        };
        let role_n = role_entities.get(r).map(|e| e.len()).unwrap_or(0).max(1);
        let mut v: Vec<(String, usize)> = m
            .iter()
            .filter(|(imp, ents)| {
                let diversity = import_roles.get(*imp).map(|s| s.len()).unwrap_or(0);
                ents.len() >= 2
                    && ents.len() as f32 >= 0.30 * role_n as f32
                    && (diversity as f32) <= 0.60 * num_roles as f32
            })
            .map(|(imp, ents)| (imp.clone(), ents.len()))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().take(6).map(|(i, _)| i).collect()
    };

    // --- RoleStat list ------------------------------------------------------
    let mut roles: Vec<RoleStat> = role_entities
        .iter()
        .filter(|(r, _)| r.as_str() != "(core)")
        .map(|(r, ents)| RoleStat {
            affix: r.clone(),
            kind: if role_suffixes.contains(r) {
                "suffix".into()
            } else if role_prefixes.contains(r) {
                "prefix".into()
            } else if bare_roles.contains(r) {
                "nested".into()
            } else {
                "folder".into()
            },
            count: ents.len(),
            common_dir: top_key(role_dirs.get(r)),
            dirs: recurring_dirs(role_dirs.get(r)),
            decl_kind: top_key(role_kinds.get(r)),
            implements: top_supertype(r, ents.len()),
            collaborators: collaborators_for(r),
        })
        .collect();
    roles.sort_by(|a, b| b.count.cmp(&a.count).then(a.affix.cmp(&b.affix)));

    // --- Shared contracts: base types many entities build on ----------------
    let mut shared_contracts: Vec<crate::model::SharedContract> = contract_entities
        .iter()
        .filter(|(name, ents)| ents.len() >= MIN_SHARED_CONTRACT && !entity_keys.contains(&canonical_key(name)))
        .map(|(name, ents)| crate::model::SharedContract { name: name.clone(), implementors: ents.len() })
        .collect();
    shared_contracts.sort_by(|a, b| b.implementors.cmp(&a.implementors).then(a.name.cmp(&b.name)));

    // --- Miner 2: cluster entities by shape similarity ----------------------
    let mut members: Vec<String> = entity_roles
        .iter()
        .filter(|(_, r)| r.len() >= 2)
        .map(|(k, _)| k.clone())
        .collect();
    members.sort(); // determinism: HashMap iteration order is not stable across runs

    // Merge-pair discovery is the O(n^2) hot loop: every entity pair's role-set
    // Jaccard is probed. The probe is READ-ONLY (no shared mutation), so the outer
    // index fans out over rayon; each `i` yields its qualifying `(i, j)` pairs. They
    // are then SORTED and applied to union-find SERIALLY in the exact (i asc, j asc)
    // order the sequential nested loop used — so the union sequence, the component
    // roots and therefore the model bytes are byte-identical.
    let n_members = members.len();
    let roles_ref = &entity_roles;
    let members_ref = &members;
    let mut merge_pairs: Vec<(usize, usize)> = (0..n_members)
        .into_par_iter()
        .flat_map_iter(move |i| {
            ((i + 1)..n_members).filter_map(move |j| {
                (jaccard(&roles_ref[&members_ref[i]], &roles_ref[&members_ref[j]]) >= JACCARD_MERGE)
                    .then_some((i, j))
            })
        })
        .collect();
    merge_pairs.sort_unstable();
    let mut uf = UnionFind::new(n_members);
    for (i, j) in merge_pairs {
        uf.union(i, j);
    }
    let mut clusters: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (i, m) in members.iter().enumerate() {
        clusters.entry(uf.find(i)).or_default().push(m.clone());
    }

    let mut conventions: Vec<Convention> = Vec::new();
    for (_, cluster) in clusters {
        if cluster.len() < MIN_CLUSTER {
            continue;
        }
        if let Some(conv) = build_convention(
            &cluster, &entity_roles, &entity_display, &slot, &symbols, degrees, content,
        ) {
            conventions.push(conv);
        }
    }

    conventions.sort_by(|a, b| {
        (b.roles.len() + b.optional_roles.len())
            .cmp(&(a.roles.len() + a.optional_roles.len()))
            .then(b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.name.cmp(&b.name)) // deterministic tie-break
    });

    Mined { roles, conventions, shared_contracts }
}

#[allow(clippy::too_many_arguments)]
fn build_convention(
    cluster: &[String],
    entity_roles: &HashMap<String, BTreeSet<String>>,
    entity_display: &HashMap<String, String>,
    slot: &HashMap<(String, String), usize>,
    symbols: &[Symbol],
    degrees: &HashMap<String, (usize, usize)>,
    content: &HashMap<String, String>,
) -> Option<Convention> {
    let size = cluster.len();

    // Role frequency within the cluster.
    let mut freq: HashMap<String, usize> = HashMap::new();
    for ent in cluster {
        for r in &entity_roles[ent] {
            *freq.entry(r.clone()).or_default() += 1;
        }
    }
    // Core = present in a majority; optional = recurs (>=2) but not majority.
    let mut core: Vec<String> = freq.iter().filter(|(_, &c)| c * 2 >= size && c >= 2).map(|(r, _)| r.clone()).collect();
    if core.len() < 2 {
        // fall back to the two most frequent roles
        let mut by_freq: Vec<(&String, &usize)> = freq.iter().filter(|(_, &c)| c >= 2).collect();
        by_freq.sort_by(|a, b| b.1.cmp(a.1));
        core = by_freq.iter().take(2).map(|(r, _)| (*r).clone()).collect();
    }
    if core.len() < 2 {
        return None;
    }
    let core_set: HashSet<&String> = core.iter().collect();
    let mut optional: Vec<String> = freq
        .iter()
        .filter(|(r, &c)| c >= 2 && !core_set.contains(*r))
        .map(|(r, _)| r.clone())
        .collect();

    // Complexity score per entity = #distinct files in its slice, tie by LOC.
    let scored: Vec<(String, usize, usize)> = cluster
        .iter()
        .map(|ent| {
            let mut files: Vec<String> = entity_roles[ent]
                .iter()
                .filter(|r| freq[*r] >= 2)
                .filter_map(|r| slot.get(&(ent.clone(), r.clone())))
                .map(|&i| symbols[i].path.clone())
                .collect();
            files.sort();
            files.dedup();
            let loc: usize = entity_roles[ent]
                .iter()
                .filter_map(|r| slot.get(&(ent.clone(), r.clone())))
                .map(|&i| symbols[i].loc)
                .sum();
            (ent.clone(), files.len(), loc)
        })
        .collect();
    // Guard: if no entity implements this slice across at least 2 distinct
    // files, the roles collapse into one file — it isn't a vertical slice.
    if scored.iter().map(|(_, f, _)| *f).max().unwrap_or(0) < 2 {
        return None;
    }
    let mut by_complexity = scored.clone();
    by_complexity.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0))); // tie-break by entity for a stable exemplar

    let simple = by_complexity.first().cloned();
    let complex = by_complexity.last().cloned();
    let medium = if by_complexity.len() >= 3 {
        Some(by_complexity[by_complexity.len() / 2].clone())
    } else {
        None
    };
    let complex_key = complex.as_ref().map(|c| c.0.clone()).unwrap_or_else(|| cluster[0].clone());
    let complex_entity = entity_display.get(&complex_key).cloned().unwrap_or_default();

    // Order core by dependency (most depended-upon first), using the complex exemplar.
    order_roles_by_dependency(&mut core, &complex_key, slot, symbols, degrees);
    optional.sort();

    // Steps (abstracted) — core first, then optional.
    let mut steps = Vec::new();
    for role in &core {
        steps.push(step_line(role, &complex_key, slot, symbols, entity_display, false));
    }
    for role in &optional {
        // source the step from whichever entity uses this role
        let src = cluster.iter().find(|e| entity_roles[*e].contains(role)).cloned().unwrap_or(complex_key.clone());
        steps.push(step_line(role, &src, slot, symbols, entity_display, true));
    }

    // Code snippets from the complex exemplar (core + optional it has), abstracted.
    let mut examples = Vec::new();
    for role in core.iter().chain(optional.iter()) {
        if let Some(&i) = slot.get(&(complex_key.clone(), role.clone())) {
            let sym = &symbols[i];
            if let Some(snip) = snippet(content, &sym.path, sym.line, 9) {
                examples.push(CodeExample {
                    path: sym.path.clone(),
                    start_line: sym.line,
                    snippet: abstract_entity(&snip, &complex_entity),
                    role: role_display(role),
                });
            }
        }
    }

    // Three exemplars by complexity.
    let mut exemplars = Vec::new();
    let push_ex = |level: &str, item: &Option<(String, usize, usize)>, out: &mut Vec<Exemplar>| {
        if let Some((key, _, _)) = item {
            // avoid duplicate entities across levels
            if out.iter().any(|e: &Exemplar| &canonical_key(&e.entity) == key) {
                return;
            }
            let roles_present: Vec<String> =
                entity_roles[key].iter().filter(|r| freq[*r] >= 2).map(|r| role_display(r)).collect();
            let files: Vec<String> = entity_roles[key]
                .iter()
                .filter_map(|r| slot.get(&(key.clone(), r.clone())))
                .map(|&i| symbols[i].path.clone())
                .collect();
            out.push(Exemplar {
                level: level.into(),
                entity: entity_display.get(key).cloned().unwrap_or_default(),
                roles_present,
                files: dedup(files),
            });
        }
    };
    push_ex("simples", &simple, &mut exemplars);
    push_ex("média", &medium, &mut exemplars);
    push_ex("complexa", &complex, &mut exemplars);

    let display_entities: Vec<String> =
        cluster.iter().filter_map(|k| entity_display.get(k).cloned()).collect();
    let core_disp: Vec<String> = core.iter().map(|r| role_display(r)).collect();
    let opt_disp: Vec<String> = optional.iter().map(|r| role_display(r)).collect();

    Some(Convention {
        name: format!("{} slice", core_disp.join("+")),
        roles: core.clone(),
        optional_roles: optional.clone(),
        recurrence: size,
        entities: display_entities.clone(),
        confidence: slice_conf(size, core.len()),
        is_slice: true,
        steps,
        exemplars,
        examples,
        exemplar: complex_entity,
        summary: format!(
            "Vertical slice recurring across {} entities ({}). Core roles: {}.{}",
            size,
            truncate_list(&display_entities, 6),
            core_disp.join(", "),
            if opt_disp.is_empty() { String::new() } else { format!(" Optional roles: {}.", opt_disp.join(", ")) }
        ),
    })
}

fn order_roles_by_dependency(
    core: &mut [String],
    exemplar_key: &str,
    slot: &HashMap<(String, String), usize>,
    symbols: &[Symbol],
    degrees: &HashMap<String, (usize, usize)>,
) {
    core.sort_by(|a, b| {
        let da = slot.get(&(exemplar_key.to_string(), a.clone())).map(|&i| degrees.get(&symbols[i].path).copied().unwrap_or((0, 0))).unwrap_or((0, 0));
        let db = slot.get(&(exemplar_key.to_string(), b.clone())).map(|&i| degrees.get(&symbols[i].path).copied().unwrap_or((0, 0))).unwrap_or((0, 0));
        db.0.cmp(&da.0).then(a.cmp(b))
    });
}

fn step_line(
    role: &str,
    src_key: &str,
    slot: &HashMap<(String, String), usize>,
    symbols: &[Symbol],
    entity_display: &HashMap<String, String>,
    optional: bool,
) -> String {
    let label = role_display(role);
    let tag = if optional { " *(opcional — adicione quando necessário)*" } else { "" };
    if let Some(&i) = slot.get(&(src_key.to_string(), role.to_string())) {
        let sym = &symbols[i];
        let ent = entity_display.get(src_key).cloned().unwrap_or_default();
        let folder = abstract_entity(&parent_dir(&sym.path), &ent);
        let abstract_file = abstract_entity(basename(&sym.path), &ent);
        format!("Adicione o **{}** para `<Name>` em `{}/` (ex.: `{}`, modelado em `{}`){}.", label, folder, abstract_file, sym.path, tag)
    } else {
        format!("Adicione o **{}** para `<Name>`{}.", label, tag)
    }
}

// --- symbol collection -----------------------------------------------------

fn collect_symbols(modules: &[Module], loc_by_path: &HashMap<&str, usize>) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for m in modules {
        for d in &m.declarations {
            if !is_significant(d) || !seen.insert((m.path.clone(), d.name.clone())) {
                continue;
            }
            out.push(Symbol {
                kind: d.kind.clone(),
                path: m.path.clone(),
                line: d.line,
                loc: *loc_by_path.get(m.path.as_str()).unwrap_or(&0),
                tokens: strip_interface_i(split_tokens(&d.name)),
                supertypes: d.supertypes.clone(),
            });
        }
    }
    out
}

fn is_significant(d: &Decl) -> bool {
    matches!(
        d.kind.as_str(),
        "class" | "interface" | "record" | "struct" | "enum" | "trait" | "mixin" | "function" | "const" | "type"
    ) && d.name.len() >= 3
}

// --- union-find ------------------------------------------------------------

struct UnionFind {
    parent: Vec<usize>,
}
impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind { parent: (0..n).collect() }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        let mut c = x;
        while self.parent[c] != r {
            let next = self.parent[c];
            self.parent[c] = r;
            c = next;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    let inter = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 { 0.0 } else { inter as f32 / union as f32 }
}

// --- text helpers ----------------------------------------------------------

fn split_tokens(name: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = name.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c == '_' || c == '-' || c == '.' || c == ' ' {
            if !cur.is_empty() {
                tokens.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let next_lower = chars.get(i + 1).map(|n| n.is_lowercase()).unwrap_or(false);
        let prev_lower = cur.chars().last().map(|p| p.is_lowercase() || p.is_ascii_digit()).unwrap_or(false);
        if !cur.is_empty() && c.is_uppercase() && (prev_lower || next_lower) {
            tokens.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens.into_iter().filter(|t| !t.is_empty()).collect()
}

fn strip_interface_i(tokens: Vec<String>) -> Vec<String> {
    if tokens.len() >= 2 && tokens[0] == "I" && tokens[1].chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        tokens[1..].to_vec()
    } else {
        tokens
    }
}

/// Join entity tokens, dropping a leading ALL-LOWERCASE particle when a
/// capitalized token follows (`useBanksConfig` → entity `Banks`, never
/// `useBanks`): a camelCase head names the naming pattern (a hook/builder
/// verb), not the entity — and keeping it breaks the dir abstraction (the
/// folder `banks/` never matches the token `usebanks`, so every member lands
/// in a distinct literal dir and the recurrence floor drops the whole
/// convention). Case-shape rule only — no curated verb list (agnostic).
fn entity_tokens(tokens: &[String]) -> String {
    let rest = if tokens.len() >= 2
        && !tokens[0].is_empty()
        && tokens[0].chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && tokens[1].chars().next().is_some_and(char::is_uppercase)
    {
        &tokens[1..]
    } else {
        tokens
    };
    rest.join("")
}

fn canonical_key(entity: &str) -> String {
    let lower = entity.to_lowercase();
    if lower.len() > 3 && lower.ends_with('s') && !lower.ends_with("ss") {
        lower[..lower.len() - 1].to_string()
    } else {
        lower
    }
}

fn role_display(role: &str) -> String {
    if role == "(core)" { "tipo base".to_string() } else { role.to_string() }
}

fn abstract_entity(text: &str, entity: &str) -> String {
    if entity.len() < 3 {
        return text.to_string();
    }
    let base = if entity.len() > 3 && entity.ends_with('s') && !entity.ends_with("ss") {
        &entity[..entity.len() - 1]
    } else {
        entity
    };
    let lower = base.to_lowercase();
    let upper = base.to_uppercase();
    let pairs = [
        (format!("{base}s"), "<Name>s".to_string()),
        (base.to_string(), "<Name>".to_string()),
        (format!("{lower}s"), "<name>s".to_string()),
        (lower, "<name>".to_string()),
        (format!("{upper}S"), "<NAME>S".to_string()),
        (upper, "<NAME>".to_string()),
    ];
    let mut out = text.to_string();
    for (from, to) in pairs {
        if from.len() >= 3 {
            out = out.replace(&from, &to);
        }
    }
    out
}

fn snippet(content: &HashMap<String, String>, path: &str, line: usize, span: usize) -> Option<String> {
    let c = content.get(path)?;
    let lines: Vec<&str> = c.lines().collect();
    let start = line.saturating_sub(1);
    let end = (start + span).min(lines.len());
    if start >= end {
        return None;
    }
    Some(lines[start..end].iter().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n"))
}

fn path_segs(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Reduce an import to a readable collaborator label: the last two dotted
/// namespace segments (`A.B.Notification.Services` -> `Notification.Services`),
/// or the last path segment for path-style imports.
fn shorten_ns(imp: &str) -> String {
    let imp = imp.trim();
    if imp.contains('.') {
        let segs: Vec<&str> = imp.split('.').filter(|s| !s.is_empty()).collect();
        let n = segs.len();
        if n >= 2 {
            format!("{}.{}", segs[n - 2], segs[n - 1])
        } else {
            segs.last().map(|s| s.to_string()).unwrap_or_default()
        }
    } else {
        imp.rsplit(['/', '\\']).next().unwrap_or(imp).to_string()
    }
}

/// Singularise a folder name into a role token: DTOs→DTO, Services→Service,
/// Repositories→Repository, Mappers→Mapper, GraphQL→GraphQL.
fn role_from_folder(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.ends_with("ies") && name.len() > 3 {
        format!("{}y", &name[..name.len() - 3])
    } else if (lower.ends_with("ses") || lower.ends_with("xes") || lower.ends_with("zes")
        || lower.ends_with("ches") || lower.ends_with("shes"))
        && name.len() > 3
    {
        name[..name.len() - 2].to_string()
    } else if lower.ends_with('s') && !lower.ends_with("ss") && name.len() > 3 {
        name[..name.len() - 1].to_string()
    } else {
        name.to_string()
    }
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => "(root)".to_string(),
    }
}

fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

fn top_key(m: Option<&HashMap<String, usize>>) -> String {
    // Max count; ties broken by smallest key so the result is deterministic.
    m.and_then(|map| {
        map.iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(k, _)| k.clone())
    })
    .unwrap_or_default()
}

/// How many homes a role may declare — bounds the model against a noise affix
/// smeared over dozens of folders while keeping every real multi-home
/// convention (a handful of parents at most).
const MAX_ROLE_DIRS: usize = 8;

/// EVERY recurring folder of a role: the dirs holding ≥2 of its members,
/// count desc then name asc (deterministic), capped at [`MAX_ROLE_DIRS`]. A
/// convention spread across parents (`configs/` and `(dashboard)/<name>s`)
/// keeps all its homes — `top_key` alone drops everything but the densest one.
fn recurring_dirs(m: Option<&HashMap<String, usize>>) -> Vec<String> {
    let Some(map) = m else {
        return Vec::new();
    };
    let mut dirs: Vec<(&String, &usize)> =
        map.iter().filter(|(d, n)| **n >= 2 && !d.is_empty()).collect();
    dirs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    dirs.into_iter().take(MAX_ROLE_DIRS).map(|(d, _)| d.clone()).collect()
}

fn dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

fn truncate_list(items: &[String], n: usize) -> String {
    if items.len() <= n {
        items.join(", ")
    } else {
        format!("{}, +{} more", items[..n].join(", "), items.len() - n)
    }
}

fn slice_conf(recur: usize, nroles: usize) -> f32 {
    let base = match recur {
        0 | 1 => 0.4,
        2 => 0.68,
        3..=4 => 0.8,
        _ => 0.9,
    };
    (base + 0.02 * nroles as f32).min(0.97)
}

/// The family's contract: the winning supertype, kept ONLY when a MAJORITY of the
/// family's `family` members share it. `per_supertype` maps each candidate base to
/// the DISTINCT entities that extend it, so the winner is weighed apples-to-apples
/// against the family size — a supertype held by a lodged minority of a large,
/// mixed suffix family never speaks for the whole family.
/// Entity-named supertypes are excluded so a domain type can't masquerade as a
/// contract. Agnostic: pure recurrence, no language named.
fn majority_supertype(
    per_supertype: &HashMap<String, BTreeSet<String>>,
    family: usize,
    entity_keys: &HashSet<String>,
) -> Option<String> {
    per_supertype
        .iter()
        .filter(|(name, _)| !entity_keys.contains(&canonical_key(name)))
        .max_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| b.0.cmp(a.0)))
        .filter(|(_, ents)| ents.len() >= 2 && ents.len() * 2 > family)
        .map(|(name, _)| name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ents(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn minority_supertype_does_not_speak_for_the_family() {
        // A base held by 3 of a 10-member family (a lodged minority) is dropped —
        // it must not speak for the whole family. Language-agnostic.
        let mut per = HashMap::new();
        per.insert("SomeBase".to_string(), ents(&["a", "b", "c"]));
        assert_eq!(majority_supertype(&per, 10, &HashSet::new()), None);
    }

    #[test]
    fn exactly_half_is_not_a_majority() {
        let mut per = HashMap::new();
        per.insert("SomeBase".to_string(), ents(&["a", "b", "c", "d", "e"]));
        assert_eq!(majority_supertype(&per, 10, &HashSet::new()), None);
    }

    #[test]
    fn strict_majority_is_the_family_contract() {
        let mut per = HashMap::new();
        per.insert("SomeBase".to_string(), ents(&["a", "b", "c", "d", "e", "f"]));
        assert_eq!(
            majority_supertype(&per, 10, &HashSet::new()),
            Some("SomeBase".to_string())
        );
    }

    #[test]
    fn stronger_supertype_wins_by_distinct_entities() {
        // Two candidates; the one shared by more DISTINCT entities wins and must
        // still clear majority. 6 vs 3 of an 8-member family -> the 6 wins.
        let mut per = HashMap::new();
        per.insert("Weak".to_string(), ents(&["a", "b", "c"]));
        per.insert("Strong".to_string(), ents(&["a", "b", "c", "d", "e", "f"]));
        assert_eq!(
            majority_supertype(&per, 8, &HashSet::new()),
            Some("Strong".to_string())
        );
    }
}
