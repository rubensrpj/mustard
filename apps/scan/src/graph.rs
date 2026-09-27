//! Layer 3 — Dependency graph.
//!
//! The declared architecture lives in folder names; the *real* architecture
//! lives in the import edges. We resolve imports to internal modules and ask
//! objective, vocabulary-free questions: are there cycles? god modules? and
//! what is the *emergent* layering — i.e. how deep is each module in the
//! dependency order the code itself defines?
//!
//! Layering is derived, not named: condense cycles into a DAG, then take each
//! module's longest dependency chain as its depth (`L0` = most depended-upon /
//! innermost). The only direction-violation topology can prove without a
//! hardcoded layer vocabulary is a dependency cycle, so that is what we count.
//!
//! Resolution is heuristic and language-agnostic: imports and declared
//! namespaces are first normalized to one canonical segment form (`\`, `::`
//! and the dots of a dotted namespace all become `/`), then every import is
//! tried against a union of resolution shapes and the ones that don't apply
//! return nothing. An import that ends in an extension of the importer's own
//! language (registry data) is a file path: its dots stay dots, and it is read
//! from the importer's folder first.
//!   * namespace/package match — an import that names a namespace declared in
//!     the importer's own language, retried with the final segment dropped
//!     when the import names a TYPE inside a namespace (the
//!     fully-qualified-name shape);
//!   * module-prefixed path — strip a declared module prefix, match a directory;
//!   * apelido de pasta — um import não relativo lido pelos apelidos da
//!     configuração mais próxima de quem importa (`crate::path_aliases`),
//!     só para a língua que o registro dá arquivo de apelidos;
//!   * file path — resolve a relative/path-ish import to a module file. The
//!     path loses its last dotted part only when that part is an extension of
//!     the importer's language, or one its imports write in place of it
//!     (registry data): any other dot is part of the file's name;
//!   * import relativo por separador — só para a língua que o registro dá
//!     `relative_import`: o import que começa pelo separador é lido a partir
//!     da pasta de quem importa (cada separador a mais sobe uma pasta) e liga
//!     ao arquivo com esse caminho ou ao arquivo que responde pela pasta; é
//!     o primeiro caminho tentado, e nenhum outro responde por ele;
//!   * apelido que sobe — só para a língua que o registro dá `parent_alias`:
//!     escrito dentro de N módulos do próprio arquivo (`Module::module_lines`),
//!     as N primeiras repetições só saem deles; as outras sobem, cada uma, uma
//!     pasta a partir da pasta dos módulos de dentro de quem importa. O resto
//!     do caminho é lido ali, tirando do fim quantas partes for preciso até
//!     achar arquivo e, sem nenhuma, ligando ao arquivo que responde pela
//!     pasta; como o import relativo, nenhum outro caminho responde por ele;
//!   * root-alias path — only for imports whose first segment is one of the
//!     importer language's declared `root_aliases` (registry data): drop the
//!     alias segment and probe the tail, and the tail cut from its end as far
//!     as it takes to reach a file, against the importer's ancestor dirs.
//!     Languages that declare no aliases never take this branch, so an external
//!     package path can never be mistaken for an internal module.
//!   * workspace package path — the longest leading run of segments that names
//!     a package the project declares (`@scope/core` included), the rest read
//!     inside that package's folder.
//!     Nothing here switches on a language name, so a new language needs no change.
//!     Imports that resolve to nothing internal are treated as external deps.

use crate::model::{CallSite, Decl, GraphStats, LayerInfo, Module, NodeDegree, Touchpoint, UseSite};
use crate::path_aliases::PathAliases;
use petgraph::graph::{DiGraph, NodeIndex};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Catalog cap for `top_fan_in` / `top_fan_out`: a bounded list (~a few KB of
/// model) ordered strongest first. The map's session summary reads the first
/// hubs of `top_fan_in`.
const TOP_DEGREE_CAP: usize = 64;

/// Longest dependency chain below an SCC = its emergent depth (`L0` = innermost).
fn scc_depth(c: usize, succ: &[HashSet<usize>], memo: &mut [Option<usize>]) -> usize {
    if let Some(d) = memo[c] {
        return d;
    }
    memo[c] = Some(0); // DAG guard; condensed graph has no cycles
    let mut d = 0;
    for &s in &succ[c] {
        d = d.max(1 + scc_depth(s, succ, memo));
    }
    memo[c] = Some(d);
    d
}


/// Resolve the raw import edges to internal module-to-module dependency edges,
/// as `(src_pos, dst_pos, weight_x1024)` triples deduped to the STRONGEST
/// evidence per `(src, dst)` pair. `*_pos` is the module's position in
/// `modules`, which — because [`build`] adds nodes in the same order — equals
/// its `NodeIndex`. Self-edges are excluded (`dst != src`).
///
/// Edge weight ×1024 is resolution SPECIFICITY: an import that lands on ONE
/// module is full evidence (1024); an import that resolves to a bucket of N
/// modules (a package folder, or the files of an imported namespace that
/// declare a name the importer uses) spreads that single import across N
/// files, so each target gets 1/N (floored at 1). A bucket import must not mint N units of
/// centrality — otherwise every file of the most-imported namespace ends up at
/// the same high fan-in, saturating `top_fan_in` with uniform glue.
///
/// The SINGLE resolver both the model graph ([`build`]) and the
/// personalized-PageRank ranker (`pagerank`) consume, so the two can never see
/// a different graph. Output sorted → byte-stable. Nothing switches on a
/// language name.
pub fn resolve_edges(
    modules: &[Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    aliases: &PathAliases,
) -> Vec<(usize, usize, u64)> {
    let mut pos: HashMap<&str, usize> = HashMap::with_capacity(modules.len());
    for (i, m) in modules.iter().enumerate() {
        pos.insert(m.path.as_str(), i);
    }
    let resolver = Resolver::new(modules, go_module, packages, aliases);

    // The strongest evidence per (src, dst) pair wins; re-imports never inflate.
    let mut edge_w: HashMap<(usize, usize), u64> = HashMap::new();
    for (src, m) in modules.iter().enumerate() {
        for imp in &m.imports {
            for nested in m.import_depths(imp, false) {
                let targets = resolver.resolve(imp, m, Reach::Used, nested);
                let w = (1024 / targets.len().max(1) as u64).max(1);
                for t in targets {
                    if let Some(&dst) = pos.get(t.as_str())
                        && dst != src {
                            let e = edge_w.entry((src, dst)).or_insert(0);
                            *e = (*e).max(w);
                        }
                }
            }
        }
    }
    let mut edges: Vec<(usize, usize, u64)> = edge_w.into_iter().map(|((a, b), w)| (a, b, w)).collect();
    edges.sort_unstable();
    edges
}

/// Os arquivos do projeto que o trecho de teste de cada módulo importa
/// (`Module::test_imports`), pela mesma resolução dos imports do corpo, na
/// ordem de `modules`. Guardados à parte: nenhum é aresta do grafo nem entra
/// em `deps`. O trecho de teste é um dos módulos escritos dentro do arquivo
/// (`Module::module_lines`), e o próprio arquivo não conta.
pub fn resolve_test_deps(
    modules: &[Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    aliases: &PathAliases,
) -> Vec<Vec<String>> {
    if modules.iter().all(|m| m.test_imports.is_empty()) {
        return vec![Vec::new(); modules.len()];
    }
    let resolver = Resolver::new(modules, go_module, packages, aliases);
    modules
        .iter()
        .map(|m| {
            let found: BTreeSet<String> = m
                .test_imports
                .iter()
                .flat_map(|imp| {
                    m.import_depths(imp, true).into_iter().flat_map(|nested| resolver.resolve(imp, m, Reach::Used, nested))
                })
                .filter(|target| *target != m.path)
                .collect();
            found.into_iter().collect()
        })
        .collect()
}

/// O que uma varredura do grafo produz: as estatísticas gerais e a camada de
/// cada módulo.
pub type GraphBuild = (GraphStats, HashMap<String, usize>);

pub fn build(
    modules: &[Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    aliases: &PathAliases,
) -> GraphBuild {
    let mut g: DiGraph<String, ()> = DiGraph::new();
    for m in modules {
        g.add_node(m.path.clone());
    }

    // Fan-in / fan-out and centrality read the SAME resolved edges the PageRank
    // ranker does (`resolve_edges`): position i == the i-th added node ==
    // `NodeIndex::new(i)`. The published degree is specificity-weighted (see the
    // resolver): a bucket-broadcast target keeps its 1/N share instead of a
    // minted full count, so real hubs rank above diffuse glue.
    let resolved = resolve_edges(modules, go_module, packages, aliases);
    let mut edge_w: HashMap<(NodeIndex, NodeIndex), u64> = HashMap::new();
    for &(a, b, w) in &resolved {
        edge_w.insert((NodeIndex::new(a), NodeIndex::new(b)), w);
    }
    let edge_set: HashSet<(NodeIndex, NodeIndex)> = edge_w.keys().copied().collect();
    for (a, b) in &edge_set {
        g.add_edge(*a, *b, ());
    }

    // Cycles via SCC.
    let sccs = petgraph::algo::tarjan_scc(&g);
    let has_multi_node_scc = sccs.iter().any(|scc| scc.len() > 1);
    let self_loop = edge_set.iter().any(|(a, b)| a == b);
    let cyclic = has_multi_node_scc || self_loop;

    // Fan-in / fan-out — specificity-weighted (see `edge_w`): the published
    // degree is the rounded sum of edge weights, i.e. "specific-import
    // equivalents". A bucket-broadcast target keeps a small honest degree (its
    // 1/N share of each broadcast import) instead of a minted full count, so
    // real hubs — modules imported by precise evidence — rank above diffuse glue.
    let mut win: HashMap<NodeIndex, u64> = HashMap::new();
    let mut wout: HashMap<NodeIndex, u64> = HashMap::new();
    for ((a, b), w) in &edge_w {
        *wout.entry(*a).or_insert(0) += w;
        *win.entry(*b).or_insert(0) += w;
    }
    // Rounded units, floored at 1 for any node with at least one in/out edge
    // — "has dependents" must survive the rounding of a tiny diffuse weight.
    let units = |x: u64| (((x + 512) >> 10) as usize).max(1);
    let mut fan_in: Vec<(u64, NodeDegree)> = Vec::new();
    let mut fan_out: Vec<(u64, NodeDegree)> = Vec::new();
    for n in g.node_indices() {
        let wi = win.get(&n).copied().unwrap_or(0);
        let wo = wout.get(&n).copied().unwrap_or(0);
        if wi > 0 {
            fan_in.push((wi, NodeDegree { module: g[n].clone(), degree: units(wi) }));
        }
        if wo > 0 {
            fan_out.push((wo, NodeDegree { module: g[n].clone(), degree: units(wo) }));
        }
    }
    // Order by the RAW weighted sum (full discrimination), path asc on ties —
    // deterministic regardless of HashMap iteration order.
    fan_in.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.module.cmp(&b.1.module)));
    fan_out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.module.cmp(&b.1.module)));
    let mut fan_in: Vec<NodeDegree> = fan_in.into_iter().map(|(_, d)| d).collect();
    let mut fan_out: Vec<NodeDegree> = fan_out.into_iter().map(|(_, d)| d).collect();
    fan_in.truncate(TOP_DEGREE_CAP);
    fan_out.truncate(TOP_DEGREE_CAP);

    // Emergent layering: condense cycles into a DAG, then depth = longest
    // dependency chain. No layer names — just the order the imports define.
    let mut scc_of = vec![0usize; g.node_count()];
    for (i, comp) in sccs.iter().enumerate() {
        for &n in comp {
            scc_of[n.index()] = i;
        }
    }
    let mut succ: Vec<HashSet<usize>> = vec![HashSet::new(); sccs.len()];
    for (a, b) in &edge_set {
        let (ca, cb) = (scc_of[a.index()], scc_of[b.index()]);
        if ca != cb {
            succ[ca].insert(cb);
        }
    }
    let mut memo = vec![None; sccs.len()];
    let mut per_depth: BTreeMap<usize, usize> = BTreeMap::new();
    let mut depth_by_path: HashMap<String, usize> = HashMap::new();
    for n in g.node_indices() {
        let d = scc_depth(scc_of[n.index()], &succ, &mut memo);
        *per_depth.entry(d).or_default() += 1;
        depth_by_path.insert(g[n].clone(), d);
    }
    let layers: Vec<LayerInfo> = per_depth
        .into_iter()
        .map(|(d, modules)| LayerInfo { name: format!("L{d}"), modules })
        .collect();

    // Touchpoints: hubs that import across many directories — the registration
    // points you edit when adding an entity (DI container, menu, barrels). Ranked
    // by breadth (distinct dirs imported) then fan-out; tests excluded because
    // they import broadly but register nothing. Frequency-derived, no catalog.
    let mut src_targets: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in &edge_set {
        src_targets.entry(g[*a].as_str()).or_default().push(g[*b].as_str());
    }
    let mut touchpoints: Vec<Touchpoint> = src_targets
        .iter()
        .filter(|(src, _)| !is_test_path(src))
        .map(|(src, tgts)| {
            let breadth = tgts.iter().map(|t| parent_dir(t)).collect::<HashSet<_>>().len();
            Touchpoint { module: (*src).to_string(), fan_out: tgts.len(), breadth }
        })
        .collect();
    touchpoints.sort_by(|a, b| b.breadth.cmp(&a.breadth).then(b.fan_out.cmp(&a.fan_out)).then(a.module.cmp(&b.module)));
    touchpoints.truncate(120); // keep enough so per-project hubs (e.g. a frontend menu) aren't crowded out by a larger project

    let stats = GraphStats {
        nodes: g.node_count(),
        edges: edge_set.len(),
        cyclic,
        top_fan_in: fan_in,
        top_fan_out: fan_out,
        layers,
        touchpoints,
    };
    (stats, depth_by_path)
}

/// A name declared by more declarations than this is a common word (`new`,
/// `build`, `run`), not a link: tying every call of it to all of them would
/// fill the map with noise instead of answers. Same bucket ceiling the import
/// resolution already applies.
const MAX_SAME_NAME: usize = 8;

/// The declaration kinds a use can point to: what is called or built by name.
/// A field or a property is read, not called — `x.kind()` is the call of
/// something else that happens to share the name — and a type alias, an
/// interface or a trait is never the target of a call either.
const CALLABLE_KINDS: &[&str] = &["function", "method", "class", "struct", "record", "enum_member", "const"];

/// The declaration kinds a citation can point to: what is named without being
/// called — a constant compared against, a type written in a parameter, an
/// enum member picked out.
pub(crate) const CITED_KINDS: &[&str] =
    &["const", "constant", "struct", "enum", "enum_member", "type", "trait", "class", "interface", "record"];

/// The named edges BETWEEN DECLARATIONS: for each declaration, which ones it
/// calls and every place that uses it, with the file and the line. Until here
/// the graph only counted file-to-file edges, which cannot answer "who calls
/// this function".
///
/// The call sites and the citations come from what each module carries
/// (`Module::calls`, `Module::cites`), never from reading the file again — so a pass that read only the files that
/// changed links exactly what a full pass links. A chamada ou a citação
/// escrita num trecho de teste do arquivo (`Module::test_lines`, guardado do
/// mesmo jeito) é do teste: não entra em quem usa a declaração.
///
/// A name is resolved only to the declarations that can be called and that the
/// calling file sees: the ones in the file itself, in a file it imports, in a
/// file a global import of its language puts in sight (one written anywhere
/// under the folder of the nearest manifest above the file that writes it, or
/// under that file's own folder when no manifest is above it), or in a file
/// that declares the same namespace in the same language (the languages that
/// group files by namespace see each other that way, without importing a
/// file). How a namespace is seen is registry data: in a language whose
/// namespace holds together with its folder, the same name in another folder
/// is another namespace; in a language whose namespaces nest, a file also sees
/// the namespaces above its own. A qualified name (`q::name`, `q.name`) also reaches the declarations
/// of a file of the same language whose name or folder is `q` — `crate::preco::total(`
/// and `model.User{}` need no import. A chamada escrita por um caminho que
/// nomeia arquivo do projeto (`Module::call_paths`, resolvido pelo mesmo passo
/// que o põe em `deps`) liga só às declarações desses arquivos: o próprio
/// arquivo só entra quando o caminho o nomeia (`super::valor()` dentro de um
/// módulo do arquivo). A name no file in sight declares is an
/// outside call — `.join(` of the
/// standard library, a field read as `x.kind()` — and is simply dropped, never
/// tied to every declaration of that name across the project. The links of
/// every declaration are rewritten from scratch on each pass, so nothing
/// survives a declaration that is gone.
///
/// Cada módulo guarda, em `Module::cites`, só as citações de um nome que algum
/// arquivo à vista dele declara como constante ou tipo, mesmo o nome comum
/// demais para ligar: é essa citação que leva a importação de namespace ao
/// arquivo que o declara. O nome que nenhum arquivo à vista declara, como um
/// tipo da biblioteca padrão, não é uso de nada do projeto e sai do mapa. O
/// que uma passada seguinte ganha com um nome que passa a ser declarado é
/// relido por [`crate::refresh::stale_citers`].
pub fn link_declarations(
    modules: &mut [Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    manifests: &[crate::model::Manifest],
    aliases: &PathAliases,
) {
    let (calls, uses, linked) = resolve_declaration_links(modules, go_module, packages, manifests, aliases);
    for (m, linked) in modules.iter_mut().zip(linked) {
        m.cites = std::mem::take(&mut m.cites)
            .into_iter()
            .enumerate()
            .filter_map(|(i, site)| linked.contains(&i).then_some(site))
            .collect();
    }
    for (m, (module_calls, module_uses)) in modules.iter_mut().zip(calls.into_iter().zip(uses)) {
        for (decl, (called, used)) in m.declarations.iter_mut().zip(module_calls.into_iter().zip(module_uses)) {
            decl.calls = called.into_iter().collect();
            let mut used: Vec<UseSite> = used;
            used.sort();
            used.dedup();
            decl.used_by = used;
        }
    }
}

/// What each declaration calls, per module and per declaration.
type CallsByDecl = Vec<Vec<BTreeSet<String>>>;

/// The uses each declaration receives, per module and per declaration.
type UsesByDecl = Vec<Vec<Vec<UseSite>>>;

/// The citations of each module that linked to a declaration, by position in
/// `Module::cites`.
type LinkedCites = Vec<HashSet<usize>>;

/// The links, per module and per declaration: the names it calls and the uses
/// it receives, and which citations of each module linked. Split out of
/// [`link_declarations`] so the whole project is read before any declaration
/// is written to.
fn resolve_declaration_links(
    modules: &[Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    manifests: &[crate::model::Manifest],
    aliases: &PathAliases,
) -> (CallsByDecl, UsesByDecl, LinkedCites) {
    let index = |kinds: &[&str]| {
        let mut by_name: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
        for (mi, m) in modules.iter().enumerate() {
            for (di, d) in m.declarations.iter().enumerate() {
                if !d.name.is_empty() && kinds.contains(&d.kind.as_str()) {
                    by_name.entry(d.name.as_str()).or_default().push((mi, di));
                }
            }
        }
        by_name
    };
    let callable = index(CALLABLE_KINDS);
    let cited = index(CITED_KINDS);

    // The namespaces each file declares, in the one segment form the import
    // resolution uses, so `Demo.Models` and `Demo::Models` are the same one,
    // each paired with the folder it holds in when the language's namespace
    // holds together with its folder (an empty folder otherwise).
    let declared: Vec<HashSet<(String, String)>> = modules
        .iter()
        .map(|m| {
            let folder = match crate::extract::namespace_scope(&m.language) {
                "folder" => parent_dir(&m.path),
                _ => String::new(),
            };
            m.namespaces.iter().map(|ns| (folder.clone(), canon_segments(ns, &m.language))).collect()
        })
        .collect();
    // The namespaces each file sees: its own, and in a language whose
    // namespaces nest, every one above them (`A/B` sees `A`).
    let in_sight: Vec<HashSet<(String, String)>> = modules
        .iter()
        .zip(&declared)
        .map(|(m, own)| {
            let mut all = own.clone();
            if crate::extract::namespace_scope(&m.language) == "nested" {
                for (folder, ns) in own {
                    let mut cur = ns.as_str();
                    while let Some((above, _)) = cur.rsplit_once('/') {
                        all.insert((folder.clone(), above.to_string()));
                        cur = above;
                    }
                }
            }
            all
        })
        .collect();
    let globals = global_sight(modules, go_module, packages, manifests, aliases);
    // O caminho escrito antes do nome chamado se resolve como o import que ele
    // é: o resolvedor só é montado quando algum arquivo tem um.
    let resolver = modules
        .iter()
        .any(|m| !m.call_paths.is_empty())
        .then(|| Resolver::new(modules, go_module, packages, aliases));
    // The names a qualifier can give a file: its own name and its folder's.
    let own_names: Vec<[String; 2]> = modules
        .iter()
        .map(|m| {
            let dir = parent_dir(&m.path);
            let folder = dir.rsplit('/').next().unwrap_or_default().to_string();
            [file_stem(&m.path), folder]
        })
        .collect();

    let mut calls: CallsByDecl = modules.iter().map(|m| vec![BTreeSet::new(); m.declarations.len()]).collect();
    let mut uses: UsesByDecl = modules.iter().map(|m| vec![Vec::new(); m.declarations.len()]).collect();
    let mut linked: LinkedCites = vec![HashSet::new(); modules.len()];

    for (src, m) in modules.iter().enumerate() {
        if m.calls.is_empty() && m.cites.is_empty() {
            continue;
        }
        let imported: HashSet<&str> = m.deps.iter().map(String::as_str).collect();
        let sees = |mi: usize, qualifier: &str| {
            mi == src
                || imported.contains(modules[mi].path.as_str())
                || globals.sees(src, &modules[mi].path)
                || (modules[mi].language == m.language
                    && (declared[mi].iter().any(|ns| in_sight[src].contains(ns))
                        || (!qualifier.is_empty() && own_names[mi].iter().any(|n| n == qualifier))))
        };
        // Cada chamada escrita por um caminho do projeto, com os caminhos.
        let mut through: BTreeMap<&CallSite, Vec<&str>> = BTreeMap::new();
        for (path, written) in &m.call_paths {
            for call in written {
                through.entry(call).or_default().push(path.as_str());
            }
        }
        let sites = m
            .calls
            .iter()
            .map(|s| (s, &callable, None))
            .chain(m.cites.iter().enumerate().map(|(i, s)| (s, &cited, Some(i))));
        for (site, by_name, cite_at) in sites {
            let is_call = cite_at.is_none();
            let Some(all) = by_name.get(site.name.as_str()) else { continue };
            let from = enclosing(&m.declarations, site.line);
            let named = match (&resolver, through.get(site)) {
                (Some(resolver), Some(paths)) if is_call => path_files(resolver, m, site.line, paths),
                _ => None,
            };
            let chosen: Vec<(usize, usize)> = all
                .iter()
                .copied()
                .filter(|&(mi, _)| match &named {
                    Some(files) => files.contains(modules[mi].path.as_str()),
                    None => sees(mi, &site.qualifier),
                })
                // A type named inside its own body is not a use of it.
                .filter(|&(mi, di)| is_call || !(mi == src && from == Some(di)))
                .collect();
            if chosen.is_empty() {
                continue;
            }
            // A citação de um nome que algum arquivo à vista declara fica no
            // módulo mesmo quando o nome é comum demais para ligar: é ela que
            // leva a importação de namespace ao arquivo que o declara, e a
            // passada que não relê o arquivo precisa dela para ligar igual.
            if let Some(i) = cite_at {
                linked[src].insert(i);
            }
            // O que se chama ou se cita num trecho de teste do arquivo não é
            // uso do código.
            if chosen.len() > MAX_SAME_NAME || m.is_test_line(site.line) {
                continue;
            }
            let from_name = from.map_or(String::new(), |di| m.declarations[di].name.clone());
            for (dst_mi, dst_di) in chosen {
                uses[dst_mi][dst_di].push(UseSite {
                    file: m.path.clone(),
                    line: site.line,
                    from: from_name.clone(),
                });
                if is_call && let Some(di) = from {
                    calls[src][di].insert(site.name.clone());
                }
            }
        }
    }
    (calls, uses, linked)
}

/// Os arquivos que os caminhos escritos antes de uma chamada na linha `line`
/// de `m` nomeiam, resolvidos como imports escritos ali, dentro de tantos
/// módulos do arquivo quanto a linha. `None` quando nenhum nomeia arquivo do
/// projeto: a chamada liga então como qualquer outra.
fn path_files(resolver: &Resolver, m: &Module, line: usize, paths: &[&str]) -> Option<HashSet<String>> {
    let nested = m.module_depth(line);
    let files: HashSet<String> =
        paths.iter().flat_map(|path| resolver.resolve(path, m, Reach::Used, nested)).collect();
    (!files.is_empty()).then_some(files)
}

/// The files the global imports put in sight: each file that writes one gives
/// a group, the files its imports resolve to, and every file of its language
/// under its project folder sees that group.
struct GlobalSight {
    /// The files each group puts in sight.
    groups: Vec<HashSet<String>>,
    /// The groups each module sees, by position in `modules`.
    seen_by: Vec<Vec<usize>>,
}

impl GlobalSight {
    fn sees(&self, src: usize, path: &str) -> bool {
        self.seen_by[src].iter().any(|&g| self.groups[g].contains(path))
    }
}

/// What the global imports of the project put in sight of each file. The
/// scope of a global import is the folder of the nearest manifest above the
/// file that writes it, or that file's own folder when no manifest is above
/// it, and only files of the same language see it.
fn global_sight(
    modules: &[Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    manifests: &[crate::model::Manifest],
    aliases: &PathAliases,
) -> GlobalSight {
    let mut sight = GlobalSight { groups: Vec::new(), seen_by: vec![Vec::new(); modules.len()] };
    if modules.iter().all(|m| m.global_imports.is_empty()) {
        return sight;
    }
    let resolver = Resolver::new(modules, go_module, packages, aliases);
    let manifest_dirs: Vec<String> = manifests.iter().map(|m| parent_dir(&m.path)).collect();
    for g in modules.iter().filter(|g| !g.global_imports.is_empty()) {
        let scope = manifest_dirs
            .iter()
            .filter(|d| is_under(&g.path, d))
            .max_by_key(|d| d.len())
            .cloned()
            .unwrap_or_else(|| parent_dir(&g.path));
        let targets: HashSet<String> =
            g.global_imports
                .iter()
                .flat_map(|imp| {
                    g.import_depths(imp, false)
                        .into_iter()
                        .flat_map(|nested| resolver.resolve(imp, g, Reach::Whole, nested))
                })
                .collect();
        if targets.is_empty() {
            continue;
        }
        let group = sight.groups.len();
        sight.groups.push(targets);
        for (si, m) in modules.iter().enumerate() {
            if m.language == g.language && is_under(&m.path, &scope) {
                sight.seen_by[si].push(group);
            }
        }
    }
    sight
}

/// The path sits somewhere under `dir` (the root holds everything).
fn is_under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// The declaration a line falls inside: the innermost one that starts at or
/// above it and has not ended yet. `None` when the line sits outside every
/// declaration (top-level code). A declaration with no end line recorded
/// covers only what starts after it, which is the best an older map allows.
fn enclosing(declarations: &[Decl], line: usize) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, d) in declarations.iter().enumerate() {
        let covers = d.line <= line && (d.end_line == 0 || d.end_line >= line);
        if covers && best.is_none_or(|b| declarations[b].line <= d.line) {
            best = Some(i);
        }
    }
    best
}

fn build_stem_index(modules: &[Module]) -> HashMap<String, Vec<String>> {
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    for module in modules {
        let stem = strip_ext(&module.path);
        m.entry(stem).or_default().push(module.path.clone());
    }
    m
}

/// O que uma importação de namespace alcança.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Só os arquivos do namespace que declaram um nome que quem importa chama
    /// ou cita: a ligação de um arquivo a outro.
    Used,
    /// O namespace inteiro: a importação global põe o namespace à vista de
    /// muitos arquivos, cada um usando nomes diferentes dele, e o nome de cada
    /// uso é que escolhe a declaração.
    Whole,
}

/// Everything import resolution reads, indexed once for the whole project.
struct Resolver<'a> {
    /// Per language, each declared namespace in canonical segment form -> the
    /// files that declare it. Split by language: a namespace of another
    /// language never answers an import.
    ns_index: HashMap<&'a str, HashMap<String, Vec<String>>>,
    stem_index: HashMap<String, Vec<String>>,
    dir_index: HashMap<String, Vec<String>>,
    module_paths: HashSet<&'a str>,
    /// Cada módulo pelo caminho: o que um arquivo do namespace declara.
    by_path: HashMap<&'a str, &'a Module>,
    go_module: &'a Option<String>,
    packages: &'a [(String, String)],
    /// Os apelidos de pasta das configurações do projeto.
    aliases: &'a PathAliases,
    /// O que cada trio `(língua, pacote, resto)` resolveu, uma vez perguntado:
    /// a língua de quem importa decide que extensão sai do resto.
    package_hits: RefCell<HashMap<(String, String, String), Vec<String>>>,
}

impl<'a> Resolver<'a> {
    fn new(
        modules: &'a [Module],
        go_module: &'a Option<String>,
        packages: &'a [(String, String)],
        aliases: &'a PathAliases,
    ) -> Self {
        let mut ns_index: HashMap<&str, HashMap<String, Vec<String>>> = HashMap::new();
        let mut dir_index: HashMap<String, Vec<String>> = HashMap::new(); // dir -> module paths
        for m in modules {
            for ns in &m.namespaces {
                // Index namespaces in canonical segment form so a lookup never
                // depends on which separator the language writes (`\`, `.`, `::`).
                ns_index
                    .entry(m.language.as_str())
                    .or_default()
                    .entry(canon_segments(ns, &m.language))
                    .or_default()
                    .push(m.path.clone());
            }
            dir_index.entry(parent_dir(&m.path)).or_default().push(m.path.clone());
        }
        Resolver {
            ns_index,
            stem_index: build_stem_index(modules),
            dir_index,
            module_paths: modules.iter().map(|m| m.path.as_str()).collect(),
            by_path: modules.iter().map(|m| (m.path.as_str(), m)).collect(),
            go_module,
            packages,
            aliases,
            package_hits: RefCell::new(HashMap::new()),
        }
    }

    /// The project files one import of `importer` names. Empty when it names
    /// nothing inside the project: an external dependency. `reach` decide o
    /// que a importação de um namespace alcança (veja [`Reach`]); `nested`,
    /// dentro de quantos módulos escritos no arquivo o import é escrito
    /// ([`Module::import_depths`]): zero fora de todos eles.
    fn resolve(&self, imp: &str, importer: &Module, reach: Reach, nested: usize) -> Vec<String> {
        let from = importer.path.as_str();
        let lang = importer.language.as_str();
        let (stem_index, dir_index, module_paths) = (&self.stem_index, &self.dir_index, &self.module_paths);
        // O import relativo escrito com o separador da língua nomeia um lugar
        // a partir da pasta de quem importa, e nenhum outro caminho responde
        // por ele.
        if let Some(hits) = self.separated_relative(imp, importer) {
            return hits;
        }
        // Try every resolution shape; whichever applies wins. No language switch.
        // Lookups run on the canonical segment form so no shape ever cares which
        // separator the import was written with — except for an import that
        // ends in an extension of the importer's own language, which is a file
        // path: its dots are the file's, not separators.
        let file_path = ends_in_own_extension(imp, crate::extract::extensions(&importer.language));
        let canon = if file_path { canon_file_path(imp, lang) } else { canon_segments(imp, lang) };
        // O caminho que começa pelo apelido que sobe um módulo também nomeia
        // um lugar a partir de quem importa, e nenhum outro caminho responde
        // por ele.
        if let Some(hits) = self.climbing(&canon, importer, nested) {
            return hits;
        }
        let cleaned = canon.strip_prefix("package:").unwrap_or(&canon);
        // 0) A file path is read first from the importer's own folder.
        if file_path && !cleaned.starts_with('.') {
            let beside = join_relative(from, cleaned);
            if module_paths.contains(beside.as_str()) {
                return vec![beside];
            }
        }
        let namespaces = self.ns_index.get(importer.language.as_str());
        // 1) Namespace/package match: the import names a namespace declared in
        //    the importer's language (the common case for namespace languages —
        //    a using shared by many files). Liga só aos arquivos do namespace
        //    que declaram um nome que quem importa chama ou cita: o namespace
        //    importado sem nenhum nome usado não liga a nada.
        if let Some(v) = namespaces.and_then(|ix| ix.get(&canon)) {
            return self.narrow(v, importer, reach);
        }
        // 1b) Fully-qualified-name match: the import names a TYPE inside a
        //     declared namespace — retry with the final segment dropped,
        //     narrowed to the file named after the type (the file-per-type
        //     convention) so one FQCN doesn't edge to every file in the
        //     namespace. Quando nenhum arquivo leva o nome do tipo, o mesmo
        //     filtro do passo 1: só os arquivos do namespace que declaram um
        //     nome que quem importa usa.
        if let Some((ns, type_name)) = canon.rsplit_once('/')
            && let Some(v) = namespaces.and_then(|ix| ix.get(ns))
        {
            let named: Vec<String> = v.iter().filter(|p| file_stem(p) == type_name).cloned().collect();
            return if named.is_empty() { self.narrow(v, importer, reach) } else { named };
        }
        // 2) Module-prefixed path: strip a declared module prefix and match the
        //    directory it points at (the import-as-package-path shape). Raw on
        //    both sides: these imports and the declared prefix are already
        //    slash-separated, and canonicalizing a dotted module domain would
        //    corrupt it.
        if let Some(modpath) = self.go_module
            && let Some(rest) = imp.strip_prefix(modpath.as_str())
        {
            let rest = rest.trim_start_matches('/');
            if let Some(v) = dir_index.get(rest) {
                return v.clone();
            }
        }
        // 2b) Apelido de pasta: um import não relativo lido pelos apelidos e
        //     pela pasta base da configuração mais próxima de quem importa. Só
        //     o arquivo que existe responde; o que não cai em nenhum segue para
        //     os passos de baixo como antes.
        for cand in self.aliases.candidates(from, lang, imp) {
            let hits = exact_path_candidate(&cand, lang, stem_index, module_paths);
            if !hits.is_empty() {
                return hits;
            }
        }
        // 3) File path: a relative or path-ish import resolved to a module file.
        //    The canonical form means dotted / `::` module paths take this branch
        //    too — they are paths spelled with another separator.
        if cleaned.starts_with('.') {
            let joined = join_relative(from, cleaned);
            return resolve_path_candidate(&joined, lang, stem_index, dir_index, module_paths);
        }
        if cleaned.contains('/') || file_path {
            let hits = resolve_path_candidate(cleaned, lang, stem_index, dir_index, module_paths);
            if !hits.is_empty() {
                return hits;
            }
        }
        // 4) Root-alias path: only for imports whose FIRST segment is one of the
        //    importer language's declared root aliases (registry data — the engine
        //    never spells one); any other first segment names an external
        //    package, never the project root. Drop the alias and probe the tail
        //    against the importer's ancestor directories, nearest first — and,
        //    because the path may end in an ITEM, a type and a method inside the
        //    module, the tail cut from its end one segment at a time, as far as
        //    it takes to reach a file. Each probe is exact: the root the alias
        //    names is always one of the importer's ancestors, so a file found
        //    only by the end of its path would be another place. The fixed
        //    probe order keeps resolution deterministic. No aliases declared ->
        //    this branch never runs.
        let root_aliases = crate::extract::root_aliases(&importer.language);
        if let Some((alias, tail)) = canon.split_once('/')
            && root_aliases.contains(&alias)
        {
            let parts: Vec<&str> = tail.split('/').collect();
            let tails: Vec<String> = (1..=parts.len()).rev().map(|cut| parts[..cut].join("/")).collect();
            for t in &tails {
                let mut dir = parent_dir(from);
                loop {
                    let cand = if dir.is_empty() { t.clone() } else { format!("{dir}/{t}") };
                    let hits = exact_path_candidate(&cand, lang, stem_index, module_paths);
                    if !hits.is_empty() {
                        return hits;
                    }
                    if dir.is_empty() {
                        break;
                    }
                    dir = parent_dir(&dir);
                }
            }
        }
        // 5) Workspace package path: the longest leading run of segments that
        //    names a package the project itself declares (its manifest's own
        //    name, `-` read as `_`, a scoped `@scope/name` included). The rest
        //    is probed (and the rest minus its last segment, which may name an
        //    item) under that package's directory, the shallowest directory
        //    first; failing that, the package file whose path ends in the rest
        //    answers — the one a manifest that maps the package's paths onto a
        //    deeper folder (`./x` onto `./src/x`) points at. Any other leading
        //    run stays an external package.
        let segments: Vec<&str> = canon.split('/').collect();
        for cut in (1..segments.len()).rev() {
            let name = fold_package(&segments[..cut].join("/"));
            if self.packages.iter().any(|(n, _)| *n == name) {
                return self.in_package(lang, name, segments[cut..].join("/"));
            }
        }
        Vec::new()
    }

    /// Os arquivos de um namespace que a importação de `importer` alcança: com
    /// [`Reach::Used`], só os que declaram um nome que ele chama (uma
    /// declaração que se chama) ou cita (uma que se cita) — as mesmas
    /// declarações que o vínculo por nome liga depois, de modo que a ligação
    /// entre os arquivos e a ligação entre as declarações contam a mesma
    /// história; com [`Reach::Whole`], o namespace inteiro.
    fn narrow(&self, bucket: &[String], importer: &Module, reach: Reach) -> Vec<String> {
        if reach == Reach::Whole {
            return bucket.to_vec();
        }
        let called: HashSet<&str> = importer.calls.iter().map(|c| c.name.as_str()).collect();
        let cited: HashSet<&str> = importer.cites.iter().map(|c| c.name.as_str()).collect();
        if called.is_empty() && cited.is_empty() {
            return Vec::new();
        }
        bucket
            .iter()
            .filter(|path| {
                self.by_path.get(path.as_str()).is_some_and(|m| {
                    m.declarations.iter().any(|d| {
                        (CALLABLE_KINDS.contains(&d.kind.as_str()) && called.contains(d.name.as_str()))
                            || (CITED_KINDS.contains(&d.kind.as_str()) && cited.contains(d.name.as_str()))
                    })
                })
            })
            .cloned()
            .collect()
    }

    /// Os arquivos que um caminho aberto pelo `parent_alias` da língua cita.
    /// Os módulos de dentro de um arquivo moram na pasta que leva o nome dele
    /// ou, no arquivo que responde pela própria pasta, nela mesma; cada
    /// repetição do apelido sobe uma pasta a partir dali, e as `nested`
    /// primeiras só saem dos módulos escritos dentro do arquivo. O resto do
    /// caminho é procurado nessa pasta, tirando do fim quantas partes for
    /// preciso até achar arquivo; sem nenhuma, o alvo é o arquivo que responde
    /// pela pasta. O que sobe além da raiz, ou não existe no projeto, não liga
    /// a nada. `None` quando o caminho não começa pelo apelido ou a língua não
    /// o declara: segue pelos outros caminhos.
    fn climbing(&self, canon: &str, importer: &Module, nested: usize) -> Option<Vec<String>> {
        let alias = crate::extract::parent_alias(&importer.language)?;
        let segments: Vec<&str> = canon.split('/').collect();
        let ups = segments.iter().take_while(|s| **s == alias).count();
        if ups == 0 {
            return None;
        }
        let mut base = inner_folder(&importer.path);
        for _ in nested..ups {
            if base.is_empty() {
                return Some(Vec::new());
            }
            base = parent_dir(&base);
        }
        let rest = &segments[ups..];
        for cut in (0..=rest.len()).rev() {
            let place: Vec<&str> =
                std::iter::once(base.as_str()).chain(rest[..cut].iter().copied()).filter(|s| !s.is_empty()).collect();
            if place.is_empty() {
                continue;
            }
            let hits = exact_path_candidate(&place.join("/"), &importer.language, &self.stem_index, &self.module_paths);
            if !hits.is_empty() {
                return Some(hits);
            }
        }
        Some(Vec::new())
    }

    /// Os arquivos que um import relativo escrito com o separador da língua
    /// cita (`relative_import` no registro): o primeiro separador do começo é
    /// a pasta de quem importa, cada um a mais sobe uma pasta, e o resto,
    /// cortado no separador, é o caminho dentro dela. O alvo é o arquivo da
    /// língua com esse caminho ou, sem ele, o arquivo que responde pela pasta;
    /// o que não existe no projeto, ou sobe além da raiz dele, não liga a
    /// nada. `None` quando o import não começa pelo separador ou a língua não
    /// o declara: segue pelos outros caminhos.
    fn separated_relative(&self, imp: &str, importer: &Module) -> Option<Vec<String>> {
        let rule = crate::extract::relative_import(&importer.language)?;
        let (ups, rest) = rule.leading(imp);
        if ups == 0 {
            return None;
        }
        let base = parent_dir(&importer.path);
        let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
        for _ in 1..ups {
            if parts.pop().is_none() {
                return Some(Vec::new());
            }
        }
        parts.extend(rest.split(rule.separator).filter(|s| !s.is_empty()));
        let place = parts.join("/");
        let extensions = crate::extract::extensions(&importer.language);
        let of_language = |stem: &str| -> Vec<String> {
            self.stem_index
                .get(stem)
                .into_iter()
                .flatten()
                .filter(|p| p.rsplit_once('.').is_some_and(|(_, ext)| extensions.contains(&ext)))
                .cloned()
                .collect()
        };
        if !place.is_empty() {
            let file = of_language(&place);
            if !file.is_empty() {
                return Some(file);
            }
        }
        if rule.package_file.is_empty() {
            return Some(Vec::new());
        }
        let package = if place.is_empty() { rule.package_file.to_string() } else { format!("{place}/{}", rule.package_file) };
        Some(of_language(&package))
    }

    /// The files `tail` names inside the declared package `name` (resolution
    /// shape 5), for an importer of language `lang`. It depends on nothing but
    /// the three, and a package's module is imported by many files, so each
    /// answer is kept for the next import.
    fn in_package(&self, lang: &str, name: String, tail: String) -> Vec<String> {
        let key = (lang.to_string(), name, tail);
        if let Some(hits) = self.package_hits.borrow().get(&key) {
            return hits.clone();
        }
        let (_, name, tail) = &key;
        let (stem_index, dir_index, module_paths) = (&self.stem_index, &self.dir_index, &self.module_paths);
        let dirs: Vec<&String> = self.packages.iter().filter(|(n, _)| n == name).map(|(_, d)| d).collect();
        let mut tails = vec![tail.clone()];
        if let Some((head, _)) = tail.rsplit_once('/') {
            tails.push(head.to_string());
        }
        let probed = dirs.iter().find_map(|dir| {
            let inside = |d: &String| is_under(d, dir) || d == *dir;
            let mut bases: Vec<&String> = dir_index.keys().filter(|d| inside(d)).collect();
            bases.sort_by(|a, b| a.matches('/').count().cmp(&b.matches('/').count()).then_with(|| a.cmp(b)));
            tails.iter().find_map(|t| {
                bases.iter().find_map(|base| {
                    let cand = if base.is_empty() { t.clone() } else { format!("{base}/{t}") };
                    let hits = resolve_path_candidate(&cand, lang, stem_index, dir_index, module_paths);
                    (!hits.is_empty()).then_some(hits)
                })
            })
        });
        let hits = probed.unwrap_or_else(|| {
            let ending = format!("/{}", strip_import_ext(tail, lang));
            let mut hits: Vec<String> = stem_index
                .iter()
                .filter(|(stem, _)| stem.ends_with(&ending) && dirs.iter().any(|d| is_under(stem, d)))
                .flat_map(|(_, v)| v.iter().cloned())
                .collect();
            hits.sort(); // stable output: HashMap iteration order varies per run
            hits
        });
        self.package_hits.borrow_mut().insert(key, hits.clone());
        hits
    }
}

/// The import ends in `.<ext>` for one of the importer language's own
/// extensions, compared case for case: for a language whose extension is
/// `ext`, `conta.ext` is a file while `Acme.Ext` stays a namespace.
fn ends_in_own_extension(imp: &str, extensions: &[&str]) -> bool {
    extensions.iter().any(|ext| {
        imp.strip_suffix(ext).is_some_and(|head| head.len() > 1 && head.ends_with('.'))
    })
}

/// A package name as imports spell it: lowercase, with `-` read as `_`.
fn fold_package(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('-', "_")
}

/// The packages the project declares, as `(folded name, manifest directory)`,
/// in name order — what resolution shape 5 matches an import's first segment
/// against.
pub fn packages(manifests: &[crate::model::Manifest]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = manifests
        .iter()
        .filter_map(|m| m.package.as_deref().map(|p| (fold_package(p), parent_dir(&m.path))))
        .filter(|(name, _)| !name.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The project files a path candidate names, for an importer of language
/// `lang`: the exact candidates first ([`exact_path_candidate`]), then any
/// module whose path ends with it.
fn resolve_path_candidate(
    cand: &str,
    lang: &str,
    stem_index: &HashMap<String, Vec<String>>,
    dir_index: &HashMap<String, Vec<String>>,
    module_paths: &HashSet<&str>,
) -> Vec<String> {
    let exact = exact_path_candidate(cand, lang, stem_index, module_paths);
    if !exact.is_empty() {
        return exact;
    }
    let stem = strip_import_ext(&normalize(cand), lang);
    // package-suffix shape: match any module whose path ends with the candidate
    let mut suffix_matches: Vec<String> = stem_index
        .iter()
        .filter(|(k, _)| k.ends_with(&stem))
        .flat_map(|(_, v)| v.clone())
        .collect();
    if !suffix_matches.is_empty() {
        suffix_matches.sort(); // stable output: HashMap iteration order varies per run
        return suffix_matches;
    }
    let _ = dir_index;
    Vec::new()
}

/// Os arquivos que um caminho candidato cita sem adivinhar: o próprio
/// arquivo, o arquivo de mesmo nome sem a extensão ou o arquivo de índice da
/// pasta. A extensão só sai quando é da língua de quem importa ou uma das que
/// o import dela escreve no lugar (dado do registro), de modo que
/// `x/pedido.service` cita `x/pedido.service.<ext>` e nunca `x/pedido.<ext>`.
fn exact_path_candidate(
    cand: &str,
    lang: &str,
    stem_index: &HashMap<String, Vec<String>>,
    module_paths: &HashSet<&str>,
) -> Vec<String> {
    let cand = normalize(cand);
    if module_paths.contains(cand.as_str()) {
        return vec![cand];
    }
    let stem = strip_import_ext(&cand, lang);
    if let Some(v) = stem_index.get(&stem) {
        return v.clone();
    }
    // directory import -> index file
    for index in INDEX_FILES {
        let probe = format!("{stem}/{index}");
        if let Some(v) = stem_index.get(&probe) {
            return v.clone();
        }
    }
    Vec::new()
}

/// Os nomes, sem extensão, do arquivo que responde pela própria pasta.
const INDEX_FILES: [&str; 3] = ["index", "main", "mod"];

/// A pasta em que moram os módulos escritos dentro de um arquivo: a que leva
/// o nome dele ou, no arquivo que responde pela própria pasta, a pasta dele.
fn inner_folder(path: &str) -> String {
    if INDEX_FILES.contains(&file_stem(path).as_str()) {
        parent_dir(path)
    } else {
        strip_ext(path)
    }
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// Path-segment test detection (language-agnostic): a file under a test/mock/
/// fixture folder, or named `*.test.*`/`*.spec.*`.
fn is_test_path(p: &str) -> bool {
    let l = p.to_lowercase();
    if l.contains(".test.") || l.contains(".spec.") {
        return true;
    }
    l.split('/').any(|s| matches!(s, "test" | "tests" | "__tests__" | "mocks" | "fixtures" | "spec" | "specs"))
}

/// The path without its last dotted part: the extension of a project file,
/// whatever it is — the stem the index keeps each module under.
fn strip_ext(path: &str) -> String {
    match path.rfind('.') {
        Some(i) if !path[i..].contains('/') => path[..i].to_string(),
        _ => path.to_string(),
    }
}

/// O caminho importado sem a extensão, quando a última parte depois do ponto
/// é uma extensão da língua `lang` ou uma das que o import dela escreve no
/// lugar (dado do registro). Qualquer outro ponto é parte do nome e fica.
fn strip_import_ext(path: &str, lang: &str) -> String {
    let known = |ext: &str| {
        crate::extract::extensions(lang).contains(&ext) || crate::extract::import_extensions(lang).contains(&ext)
    };
    match path.rfind('.') {
        Some(i) if i > 0 && !path[i..].contains('/') && !path[..i].ends_with('/') && known(&path[i + 1..]) => {
            path[..i].to_string()
        }
        _ => path.to_string(),
    }
}

/// O nome qualificado na forma única de partes separadas por `/`. Com os
/// separadores que a língua declara (`qualified_separators`), cada um vira
/// `/`, menos no texto que já é caminho: tem `/`, ou começa por `.` como o
/// relativo. Sem o campo, a regra de sempre: `\` e `::` sempre viram `/`, e o
/// ponto só quando o texto não é caminho (um namespace com pontos nunca
/// começa por ponto, e o import relativo sempre começa).
fn canon_segments(s: &str, lang: &str) -> String {
    if let Some(separators) = crate::extract::qualified_separators(lang) {
        return if s.contains('/') || s.starts_with('.') { s.to_string() } else { to_slashes(s, separators) };
    }
    let flat = s.replace('\\', "/").replace("::", "/");
    if !flat.contains('/') && !flat.starts_with('.') && flat.contains('.') {
        flat.replace('.', "/")
    } else {
        flat
    }
}

/// O import que termina na extensão da própria língua é caminho de arquivo: os
/// separadores da língua viram `/`, e a extensão fica como está. Sem o campo,
/// `\` e `::` viram `/`, e os pontos são do arquivo.
fn canon_file_path(imp: &str, lang: &str) -> String {
    match (crate::extract::qualified_separators(lang), imp.rsplit_once('.')) {
        (Some(separators), Some((stem, ext))) => format!("{}.{ext}", to_slashes(stem, separators)),
        (Some(separators), None) => to_slashes(imp, separators),
        (None, _) => imp.replace('\\', "/").replace("::", "/"),
    }
}

/// Cada um dos `separators` trocado por `/`, na ordem em que vêm.
fn to_slashes(s: &str, separators: &[&str]) -> String {
    separators.iter().fold(s.to_string(), |text, sep| text.replace(sep, "/"))
}

/// Final path segment without its extension: `app/models/User.xyz` -> `User`.
fn file_stem(path: &str) -> String {
    let stem = strip_ext(path);
    match stem.rfind('/') {
        Some(i) => stem[i + 1..].to_string(),
        None => stem,
    }
}

fn join_relative(from: &str, rel: &str) -> String {
    let base = parent_dir(from);
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}
