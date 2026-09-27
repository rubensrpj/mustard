//! Layer 3 — Dependency graph.
//!
//! The declared architecture lives in folder names; the *real* architecture
//! lives in the import edges. We resolve imports to internal modules and ask
//! objective, vocabulary-free questions: are there cycles? god modules? and
//! how deep is each module in the dependency order the code itself defines?
//!
//! Depth is derived, not named: condense cycles into a DAG, then take each
//! module's longest dependency chain as its depth (0 = most depended-upon /
//! innermost); the folder skeleton reads it. The only direction-violation
//! topology can prove without a hardcoded layer vocabulary is a dependency
//! cycle, so that is what we count.
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
//!     inside that package's folder; o resto que não chega a arquivo nenhum
//!     (`Leitor` em `demo_core::Leitor`) cai no arquivo raiz do pacote
//!     (`package_entry` no registro);
//!   * módulo filho — só na língua que declara `root_aliases`: o caminho sem
//!     apelido cuja primeira parte é módulo filho de quem importa se lê na
//!     pasta dos módulos dele, como se o apelido do próprio módulo viesse na
//!     frente;
//!   * repasse — achado o arquivo alvo, cada nome que o import traz e que o
//!     alvo não declara, mas repassa (`Module::reexports`), liga ao arquivo
//!     que o declara, seguindo os repasses sem voltar a um já visto.
//!     Nothing here switches on a language name, so a new language needs no change.
//!     Imports that resolve to nothing internal are treated as external deps.

use crate::model::{CallSite, Decl, DeclAt, GraphStats, Module, NodeDegree, UseSite, RECEIVER};
use crate::path_aliases::PathAliases;
use petgraph::graph::{DiGraph, NodeIndex};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Catalog cap for `top_fan_in`: a bounded list (~a few KB of model) ordered
/// strongest first. The map's session summary reads the first
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
/// The single resolver of the imports: the model graph ([`build`]) and the
/// dependencies of each file (`Module::deps`) both read it, so the two can
/// never see a different graph. Output sorted → byte-stable. Nothing switches
/// on a language name.
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
                let targets = resolver.resolve_through(imp, m, nested);
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
                    m.import_depths(imp, true).into_iter().flat_map(|nested| resolver.resolve_through(imp, m, nested))
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

    // Fan-in reads the same resolved edges the dependencies of each file do
    // (`resolve_edges`): position i == the i-th added node ==
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

    // Fan-in — specificity-weighted (see `edge_w`): the published
    // degree is the rounded sum of edge weights, i.e. "specific-import
    // equivalents". A bucket-broadcast target keeps a small honest degree (its
    // 1/N share of each broadcast import) instead of a minted full count, so
    // real hubs — modules imported by precise evidence — rank above diffuse glue.
    let mut win: HashMap<NodeIndex, u64> = HashMap::new();
    for ((_, b), w) in &edge_w {
        *win.entry(*b).or_insert(0) += w;
    }
    // Rounded units, floored at 1 for any node with at least one in edge
    // — "has dependents" must survive the rounding of a tiny diffuse weight.
    let units = |x: u64| (((x + 512) >> 10) as usize).max(1);
    let mut fan_in: Vec<(u64, NodeDegree)> = Vec::new();
    for n in g.node_indices() {
        let wi = win.get(&n).copied().unwrap_or(0);
        if wi > 0 {
            fan_in.push((wi, NodeDegree { module: g[n].clone(), degree: units(wi) }));
        }
    }
    // Order by the RAW weighted sum (full discrimination), path asc on ties —
    // deterministic regardless of HashMap iteration order.
    fan_in.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.module.cmp(&b.1.module)));
    let mut fan_in: Vec<NodeDegree> = fan_in.into_iter().map(|(_, d)| d).collect();
    fan_in.truncate(TOP_DEGREE_CAP);

    // The depth of each file, for the folder skeleton: condense cycles into a
    // DAG, then depth = longest dependency chain. No layer names — just the
    // order the imports define.
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
    let mut depth_by_path: HashMap<String, usize> = HashMap::new();
    for n in g.node_indices() {
        let d = scc_depth(scc_of[n.index()], &succ, &mut memo);
        depth_by_path.insert(g[n].clone(), d);
    }

    let stats = GraphStats { nodes: g.node_count(), edges: edge_set.len(), cyclic, top_fan_in: fan_in };
    (stats, depth_by_path)
}

/// A call that can reach more declarations than this one is a common word
/// (`new`, `build`, `run`), not a link: tying it to all of them would fill the
/// map with noise instead of answers, so it is only counted. Same bucket
/// ceiling the import resolution already applies.
const MAX_SAME_NAME: usize = 8;

/// The declaration kinds a use can point to: what is called or built by name.
/// A field or a property is read, not called — `x.kind()` is the call of
/// something else that happens to share the name — and a type alias, an
/// interface or a trait is never the target of a call either.
const CALLABLE_KINDS: &[&str] = &["function", "method", "class", "struct", "record", "enum_member", "const"];

/// Os tipos de declaração que só se alcançam pelo objeto ou pelo tipo dono:
/// o nome sozinho só chega a eles na língua que chama o membro do próprio
/// objeto sem escrevê-lo (`implicit_self` no registro).
const MEMBER_KINDS: &[&str] = &["method", "field", "property", "enum_member"];

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
/// Cada ligação diz se é provada ou suspeita ([`UseSite::candidates`]). A
/// provada tem um alvo só; a suspeita traz as declarações que a chamada pode
/// alcançar, e a chamada que pode alcançar mais que o teto só se conta
/// (`Decl::common_calls`). Quem a chamada alcança se procura por degraus,
/// sempre só entre as declarações que se chamam ([`CALLABLE_KINDS`]):
///
/// - O caminho escrito antes do nome que nomeia arquivo do projeto
///   (`Module::call_paths`, resolvido pelo mesmo passo que o põe em `deps`)
///   liga só às declarações desses arquivos: o próprio arquivo só entra
///   quando o caminho o nomeia (`super::valor()` dentro de um módulo do
///   arquivo).
/// - O caminho de duas partes ou mais que não virou import
///   (`Module::other_call_paths`) cuja raiz não é peça do projeto — nenhum
///   apelido, pacote, arquivo ou pasta dele, nem nome que um import do
///   projeto trouxe — é da biblioteca: a chamada escrita por ele não liga a
///   nada do projeto (`std::fs::read()` com uma pasta `fs` no projeto).
/// - O nome sozinho não alcança método, campo nem membro de enum, a não ser na
///   língua que chama o membro do próprio objeto sem escrevê-lo
///   (`implicit_self` no registro): nas outras, o `Ok(` escrito sozinho é o
///   da biblioteca padrão mesmo com um `enum` do projeto que tem um `Ok`.
/// - O nome sozinho procura o que o arquivo tem à vista: ele mesmo, o que
///   ele importa (as arestas já estreitadas pelo tipo usado), o que um import
///   global da língua põe à vista (escrito em qualquer lugar sob a pasta do
///   manifesto mais próximo acima de quem o escreve, ou sob a pasta dele
///   quando não há manifesto acima) e o mesmo módulo — os arquivos que
///   declaram o mesmo namespace na mesma língua. Como o namespace se vê é
///   dado do registro: na língua cujo namespace vai junto com a pasta, o
///   mesmo nome em outra pasta é outro namespace; na língua cujos namespaces
///   se aninham, o arquivo vê também os de cima. Importar é por arquivo, não
///   por nome: o que o arquivo importa e o que ele mesmo declara contam
///   juntos, e dois alvos ali são suspeitos. Sem nada à vista, o nome
///   declarado uma vez na família da língua é provado; mais de uma vez, as
///   declarações da família inteira são suspeitas.
/// - O nome qualificado (`q::nome`, `q.nome`) estreita pelo qualificador: as
///   declarações de um arquivo da mesma língua cujo nome ou pasta é `q`
///   (`crate::preco::total(` e `model.User{}` não precisam de import), e as
///   de um tipo `q` que o arquivo tem à vista (`Pedido::novo()`).
/// - A chamada aberta por um nome que não é peça do projeto nem do arquivo
///   ([`Module::unbound_heads`]: `File` em `File.ReadAllText()`, `std` em
///   `std::fs::read()`) é da biblioteca, e não liga.
/// - O próprio objeto (`self`, `this`: `self_receivers` no registro)
///   estreita pelos membros do tipo em que a chamada está escrita, e o que
///   está à vista vale antes do resto.
/// - Sem estreitar — o qualificador que é um valor (`x.run()`, `f().run()`),
///   ou o próprio objeto sem o método entre os membros —, a chamada é
///   suspeita, só entre as declarações à vista: o nome que o arquivo não vê
///   é uma chamada de fora, como o `.join(` da biblioteca padrão ou o campo
///   lido como `x.kind()`.
///
/// A citação liga só ao que o arquivo tem à vista, com o nome qualificado
/// estreitando do mesmo jeito. Cada módulo guarda, em `Module::cites`, só as
/// citações de um nome que algum arquivo à vista dele declara como constante
/// ou tipo, mesmo o nome comum demais para ligar: é essa citação que leva a
/// importação de namespace ao arquivo que o declara. O nome que nenhum
/// arquivo à vista declara, como um tipo da biblioteca padrão, não é uso de
/// nada do projeto e sai do mapa. O que uma passada seguinte ganha com um
/// nome que passa a ser declarado é relido por
/// [`crate::refresh::stale_citers`]. The links of every declaration are
/// rewritten from scratch on each pass, so nothing survives a declaration
/// that is gone.
pub fn link_declarations(
    modules: &mut [Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    manifests: &[crate::model::Manifest],
    aliases: &PathAliases,
) {
    let DeclLinks { mut calls, mut uses, common, linked } =
        resolve_declaration_links(modules, go_module, packages, manifests, aliases);
    for (m, linked) in modules.iter_mut().zip(linked) {
        m.cites = std::mem::take(&mut m.cites)
            .into_iter()
            .enumerate()
            .filter_map(|(i, site)| linked.contains(&i).then_some(site))
            .collect();
    }
    for (mi, m) in modules.iter_mut().enumerate() {
        for (di, decl) in m.declarations.iter_mut().enumerate() {
            decl.calls = std::mem::take(&mut calls[mi][di]).into_iter().collect();
            let mut used = std::mem::take(&mut uses[mi][di]);
            used.sort();
            // O mesmo lugar ligado duas vezes fica uma só, e a provada, que
            // vem antes na ordem, vence a suspeita.
            used.dedup_by(|next, kept| (&next.file, next.line, &next.from) == (&kept.file, kept.line, &kept.from));
            decl.used_by = used;
            decl.common_calls = common[mi][di];
        }
    }
}

/// As ligações do projeto, por módulo e por declaração: os nomes que cada uma
/// chama, os usos que recebe, as chamadas comuns demais para ligar que
/// podiam alcançá-la, e as citações de cada módulo que ligaram, pela posição
/// em `Module::cites`.
struct DeclLinks {
    calls: Vec<Vec<BTreeSet<String>>>,
    uses: Vec<Vec<Vec<UseSite>>>,
    common: Vec<Vec<usize>>,
    linked: Vec<HashSet<usize>>,
}

/// O que uma chamada ou uma citação alcança.
enum Verdict {
    /// Só esta declaração: a ligação provada.
    Proven(DeclId),
    /// Uma destas, sem como decidir qual: a ligação suspeita.
    Suspect(Vec<DeclId>),
    /// Mais declarações que o teto: a chamada só se conta.
    Common(Vec<DeclId>),
}

impl Verdict {
    /// O veredito sobre as declarações que a chamada pode alcançar: uma só é
    /// provada quando `provable`; mais que o teto, só a contagem. `None` sem
    /// nenhuma.
    fn of(pool: Vec<DeclId>, provable: bool) -> Option<Self> {
        match pool.len() {
            0 => None,
            1 if provable => Some(Self::Proven(pool[0])),
            n if n > MAX_SAME_NAME => Some(Self::Common(pool)),
            _ => Some(Self::Suspect(pool)),
        }
    }
}

/// O que vem escrito antes do nome, como a ligação o lê.
enum Before<'a> {
    /// Nada: o nome sozinho.
    Nothing,
    /// O próprio objeto ou o próprio tipo (`self_receivers` no registro).
    Itself,
    /// Um nome, que pode ser um arquivo, um tipo ou um valor.
    Name(&'a str),
    /// Um valor que não é nome ([`RECEIVER`]).
    Value,
}

impl<'a> Before<'a> {
    fn of(qualifier: &'a str, lang: &str) -> Self {
        match qualifier {
            "" => Self::Nothing,
            RECEIVER => Self::Value,
            q if crate::extract::self_receivers(lang).contains(&q) => Self::Itself,
            q => Self::Name(q),
        }
    }
}

/// Os nomes do tipo em que a linha está escrita: os donos da declaração que a
/// contém e ela mesma, quando é um tipo. Vazio fora de toda declaração.
fn own_types(m: &Module, from: Option<usize>) -> Vec<&str> {
    let Some(d) = from.map(|di| &m.declarations[di]) else { return Vec::new() };
    let mut names: Vec<&str> = d.owner.iter().map(String::as_str).collect();
    if TYPE_KINDS.contains(&d.kind.as_str()) {
        names.push(d.name.as_str());
    }
    names
}

/// As ligações, por módulo e por declaração ([`DeclLinks`]). Split out of
/// [`link_declarations`] so the whole project is read before any declaration
/// is written to.
fn resolve_declaration_links(
    modules: &[Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    manifests: &[crate::model::Manifest],
    aliases: &PathAliases,
) -> DeclLinks {
    let index = |kinds: &[&str]| {
        let mut by_name: HashMap<&str, Vec<DeclId>> = HashMap::new();
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
    // O caminho escrito antes do nome chamado, o import que traz nomes e o
    // import com `*` se resolvem como o import que são.
    let resolver = Resolver::new(modules, go_module, packages, aliases);
    // The names a qualifier can give a file: its own name and its folder's.
    let own_names: Vec<[String; 2]> = modules
        .iter()
        .map(|m| {
            let dir = parent_dir(&m.path);
            let folder = dir.rsplit('/').next().unwrap_or_default().to_string();
            [file_stem(&m.path), folder]
        })
        .collect();
    // Os nomes que são peça declarada do projeto: o de toda declaração e cada
    // parte de um namespace declarado.
    let declared_names: HashSet<&str> = modules
        .iter()
        .flat_map(|m| m.declarations.iter().map(|d| d.name.as_str()))
        .chain(declared.iter().flatten().flat_map(|(_, ns)| ns.split('/')))
        .collect();
    let owner_of = |(mi, di): DeclId| modules[mi].declarations[di].owner.first().map(String::as_str);
    let at = |(mi, di): DeclId| {
        let d = &modules[mi].declarations[di];
        DeclAt { file: modules[mi].path.clone(), line: d.line, name: d.name.clone() }
    };

    let mut links = DeclLinks {
        calls: modules.iter().map(|m| vec![BTreeSet::new(); m.declarations.len()]).collect(),
        uses: modules.iter().map(|m| vec![Vec::new(); m.declarations.len()]).collect(),
        common: modules.iter().map(|m| vec![0; m.declarations.len()]).collect(),
        linked: vec![HashSet::new(); modules.len()],
    };

    for (src, m) in modules.iter().enumerate() {
        if m.calls.is_empty() && m.cites.is_empty() {
            continue;
        }
        let imported: HashSet<&str> = m.deps.iter().map(String::as_str).collect();
        let family = crate::extract::family(&m.language);
        let implicit_self = crate::extract::implicit_self(&m.language);
        let Brought { outside, all: brought } = brought_names(&resolver, m);
        // Os nomes que todo arquivo da língua vê sem import, e os arquivos que
        // este importa com `*`, que podem declarar um deles.
        let prelude = crate::extract::prelude(&m.language);
        let globbed: HashSet<String> = if prelude.is_empty() { HashSet::new() } else { glob_files(&resolver, m) };
        // Na língua que liga o método por um separador próprio, o de nome
        // qualificado só junta caminho.
        let path_only = crate::extract::has_member_separators(&m.language);
        // O que o arquivo tem à vista: ele mesmo, o que importa, o que um
        // import global põe à vista e o mesmo namespace da mesma língua.
        let sees = |mi: usize| {
            mi == src
                || imported.contains(modules[mi].path.as_str())
                || globals.sees(src, &modules[mi].path)
                || (modules[mi].language == m.language && declared[mi].iter().any(|ns| in_sight[src].contains(ns)))
        };
        // O arquivo da mesma língua que o qualificador nomeia.
        let named_by = |mi: usize, q: &str| modules[mi].language == m.language && own_names[mi].iter().any(|n| n == q);
        // Cada chamada escrita por um caminho do projeto, com os caminhos.
        let mut through: BTreeMap<&CallSite, Vec<&str>> = BTreeMap::new();
        for (path, written) in &m.call_paths {
            for call in written {
                through.entry(call).or_default().push(path.as_str());
            }
        }
        // O nome que abre a cadeia escrita antes de uma chamada, que o
        // arquivo não liga (`Module::unbound_heads`), e que não é peça do
        // projeto: nenhuma declaração ou namespace, apelido, pacote, arquivo
        // ou pasta dele, nem nome que um import do projeto trouxe.
        let from_library = |head: &str, written: &str| {
            m.unbound_heads.binary_search_by(|h| h.as_str().cmp(head)).is_ok()
                && !declared_names.contains(head)
                && !(brought.contains(head) && !outside.contains(head))
                && resolver.outside(written, m)
        };
        // Cada chamada escrita por um caminho de duas partes ou mais
        // (`std::fs::read()`), com a raiz de biblioteca ou não. A raiz do
        // caminho decide, e não o nome escrito logo antes da chamada.
        let mut by_path: BTreeMap<&CallSite, bool> = BTreeMap::new();
        for (path, written) in &m.other_call_paths {
            let canon = canon_segments(path, &m.language);
            let first = canon.split('/').next().unwrap_or_default();
            let library = from_library(first, path);
            for site in written {
                *by_path.entry(site).or_default() |= library;
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
            let before = Before::of(&site.qualifier, &m.language);
            // O nome sozinho só alcança um membro na língua que o chama sem
            // escrever o objeto.
            let member_out = matches!(before, Before::Nothing) && !implicit_self;
            // A type named inside its own body is not a use of it.
            let all: Vec<DeclId> = all
                .iter()
                .copied()
                .filter(|&(mi, di)| is_call || !(mi == src && from == Some(di)))
                .filter(|&(mi, di)| !(member_out && MEMBER_KINDS.contains(&modules[mi].declarations[di].kind.as_str())))
                .collect();
            let seen: Vec<DeclId> = all.iter().copied().filter(|&(mi, _)| sees(mi)).collect();
            // O nome que um import de fora do projeto trouxe, escrito sozinho
            // ou antes de outro nome, é de fora e não liga; o caminho do
            // projeto escrito antes do nome vale antes dele.
            let from_outside = match before {
                Before::Name(q) => outside.contains(q),
                Before::Nothing => outside.contains(site.name.as_str()),
                Before::Itself | Before::Value => false,
            };
            // O nome da língua escrito sozinho só liga ao projeto quando o
            // arquivo o declara, o traz pelo nome num import ou importa com
            // `*` o arquivo que o declara.
            let of_the_language = matches!(before, Before::Nothing)
                && prelude.contains(&site.name.as_str())
                && !(m.declarations.iter().any(|d| d.name == site.name)
                    || brought.contains(site.name.as_str())
                    || all.iter().any(|&(mi, _)| globbed.contains(&modules[mi].path)));
            // A chamada aberta por um nome de biblioteca é de fora
            // (`File.ReadAllText()`, `std::fs::read()`), e não liga.
            let by_library = is_call
                && match by_path.get(site) {
                    Some(&library) => library,
                    None => matches!(before, Before::Name(q) if from_library(q, q)),
                };
            let not_ours = from_outside || of_the_language || by_library;
            // Estreita pelo que vem antes do nome: o arquivo que o
            // qualificador nomeia (só o que fica fora de todo tipo nele) ou
            // o tipo que ele nomeia, ou os membros do tipo em que a chamada
            // está escrita. Com o que ficou, se a ligação pode ser provada:
            // o arquivo nomeado que o arquivo não tem à vista pode ser outro
            // de mesmo nome, de fora do projeto. `None` quando nada estreita.
            let narrowed = |pool: &[DeclId]| -> Option<(Vec<DeclId>, bool)> {
                let kept: Vec<DeclId> = match before {
                    Before::Name(q) => pool
                        .iter()
                        .copied()
                        .filter(|&d| match owner_of(d) {
                            None => named_by(d.0, q),
                            Some(owner) => owner == q && sees(d.0),
                        })
                        .collect(),
                    Before::Itself => {
                        let types = own_types(m, from);
                        let own: Vec<DeclId> =
                            pool.iter().copied().filter(|&d| owner_of(d).is_some_and(|o| types.contains(&o))).collect();
                        let own_seen: Vec<DeclId> = own.iter().copied().filter(|&(mi, _)| sees(mi)).collect();
                        if own_seen.is_empty() { own } else { own_seen }
                    }
                    Before::Nothing | Before::Value => Vec::new(),
                };
                let provable = matches!(before, Before::Itself) || kept.iter().all(|&(mi, _)| sees(mi));
                (!kept.is_empty()).then_some((kept, provable))
            };
            let verdict = if is_call {
                let named = through.get(site).and_then(|paths| path_files(&resolver, m, site.line, paths));
                match (named, &before) {
                    (Some(files), _) => Verdict::of(
                        all.iter().copied().filter(|&(mi, _)| files.contains(modules[mi].path.as_str())).collect(),
                        true,
                    ),
                    (None, _) if not_ours => None,
                    (None, Before::Nothing) if !seen.is_empty() => Verdict::of(seen, true),
                    // Sem nada à vista, a família inteira da língua.
                    (None, Before::Nothing) => Verdict::of(
                        all.iter()
                            .copied()
                            .filter(|&(mi, _)| crate::extract::family(&modules[mi].language) == family)
                            .collect(),
                        true,
                    ),
                    (None, _) => match narrowed(&all) {
                        Some((kept, provable)) => Verdict::of(kept, provable),
                        // O nome antes de um separador que só junta caminho
                        // é módulo ou tipo, nunca valor: sem nada do projeto
                        // com esse nome, a chamada é de fora (`Vec::new()`).
                        None if matches!(before, Before::Name(_)) && path_only => None,
                        None => Verdict::of(seen, false),
                    },
                }
            } else {
                // A citação só liga ao que o arquivo tem à vista, com o
                // arquivo que o qualificador nomeia.
                let pool: Vec<DeclId> = match before {
                    _ if not_ours => Vec::new(),
                    Before::Name(q) => all.iter().copied().filter(|&(mi, _)| sees(mi) || named_by(mi, q)).collect(),
                    _ => seen,
                };
                match (narrowed(&pool), &before) {
                    (Some((kept, provable)), _) => Verdict::of(kept, provable),
                    (None, Before::Nothing) => Verdict::of(pool, true),
                    (None, _) => Verdict::of(pool, false),
                }
            };
            let Some(verdict) = verdict else { continue };
            // A citação de um nome que algum arquivo à vista declara fica no
            // módulo mesmo quando o nome é comum demais para ligar: é ela que
            // leva a importação de namespace ao arquivo que o declara, e a
            // passada que não relê o arquivo precisa dela para ligar igual.
            if let Some(i) = cite_at {
                links.linked[src].insert(i);
            }
            // O que se chama ou se cita num trecho de teste do arquivo não é
            // uso do código.
            if m.is_test_line(site.line) {
                continue;
            }
            let from_name = from.map_or(String::new(), |di| m.declarations[di].name.clone());
            let (targets, candidates) = match verdict {
                Verdict::Proven(target) => (vec![target], Vec::new()),
                Verdict::Suspect(targets) => {
                    let mut candidates: Vec<DeclAt> = targets.iter().map(|&t| at(t)).collect();
                    candidates.sort();
                    (targets, candidates)
                }
                Verdict::Common(targets) => {
                    if is_call {
                        for (mi, di) in targets {
                            links.common[mi][di] += 1;
                        }
                    }
                    continue;
                }
            };
            for (dst_mi, dst_di) in targets {
                links.uses[dst_mi][dst_di].push(UseSite {
                    file: m.path.clone(),
                    line: site.line,
                    from: from_name.clone(),
                    candidates: candidates.clone(),
                });
            }
            if is_call && let Some(di) = from {
                links.calls[src][di].insert(site.name.clone());
            }
        }
    }
    links
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

/// Os nomes que os imports de um arquivo trazem ([`Module::brought`]).
struct Brought<'m> {
    /// Os que só um import que não nomeia nada do projeto traz: são de fora.
    outside: HashSet<&'m str>,
    /// Todos, de qualquer import.
    all: HashSet<&'m str>,
}

/// Os arquivos do projeto que um import de `m` nomeia, escrito em qualquer
/// profundidade e em qualquer trecho, inteiro quando nomeia um namespace.
fn import_files(resolver: &Resolver, m: &Module, imp: &str) -> Vec<String> {
    let depths: BTreeSet<usize> = m.import_depths(imp, false).union(&m.import_depths(imp, true)).copied().collect();
    depths.into_iter().flat_map(|nested| resolver.resolve(imp, m, Reach::Whole, nested)).collect()
}

/// Os nomes trazidos pelos imports de `m`, com os que vêm só de fora do
/// projeto à parte.
fn brought_names<'m>(resolver: &Resolver, m: &'m Module) -> Brought<'m> {
    let mut brought = Brought { outside: HashSet::new(), all: HashSet::new() };
    let mut inside: HashSet<&str> = HashSet::new();
    for (imp, names) in &m.brought {
        let names = names.iter().map(String::as_str);
        brought.all.extend(names.clone());
        if import_files(resolver, m, imp).is_empty() && resolver.outside(imp, m) {
            brought.outside.extend(names);
        } else {
            inside.extend(names);
        }
    }
    brought.outside.retain(|name| !inside.contains(name));
    brought
}

/// Os arquivos do projeto que os imports com `*` de `m` nomeiam.
fn glob_files(resolver: &Resolver, m: &Module) -> HashSet<String> {
    m.imports
        .iter()
        .chain(&m.test_imports)
        .filter(|imp| imp.trim_end().ends_with('*'))
        .flat_map(|imp| import_files(resolver, m, imp))
        .collect()
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

/// What the global imports of the project put in sight of each file: the
/// files each import resolves to, seen by the files it reaches (see
/// [`global_reach`]).
fn global_sight(
    modules: &[Module],
    go_module: &Option<String>,
    packages: &[(String, String)],
    manifests: &[crate::model::Manifest],
    aliases: &PathAliases,
) -> GlobalSight {
    let mut sight = GlobalSight { groups: Vec::new(), seen_by: vec![Vec::new(); modules.len()] };
    let reach = global_reach(modules, manifests);
    if reach.is_empty() {
        return sight;
    }
    let resolver = Resolver::new(modules, go_module, packages, aliases);
    for (writer, reached) in reach {
        let g = &modules[writer];
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
        for si in reached {
            sight.seen_by[si].push(group);
        }
    }
    sight
}

/// Each file of `modules` that writes a global import, by its index, with the
/// indexes of the files that import reaches: those of the same language under
/// the folder of the manifest nearest above the writer, or under the writer's
/// own folder when no manifest is above it.
pub(crate) fn global_reach(modules: &[Module], manifests: &[crate::model::Manifest]) -> Vec<(usize, Vec<usize>)> {
    modules
        .iter()
        .enumerate()
        .filter(|(_, g)| !g.global_imports.is_empty())
        .map(|(writer, g)| {
            let scope = nearest_manifest_dir(&g.path, manifests).unwrap_or_else(|| folder_of(&g.path));
            let reached = modules
                .iter()
                .enumerate()
                .filter(|(_, m)| m.language == g.language && is_under(&m.path, scope))
                .map(|(at, _)| at)
                .collect();
            (writer, reached)
        })
        .collect()
}

/// The folder of the manifest nearest above `path`: the deepest folder that
/// holds both a manifest and the path. `None` when no manifest is above it.
fn nearest_manifest_dir<'m>(path: &str, manifests: &'m [crate::model::Manifest]) -> Option<&'m str> {
    manifests.iter().map(|m| folder_of(&m.path)).filter(|dir| is_under(path, dir)).max_by_key(|dir| dir.len())
}

/// What the manifests nearest above `path` depend on: every manifest in the
/// folder [`nearest_manifest_dir`] gives. Empty when no manifest is above it.
pub(crate) fn nearest_manifest_deps(path: &str, manifests: &[crate::model::Manifest]) -> Vec<String> {
    let Some(dir) = nearest_manifest_dir(path, manifests) else {
        return Vec::new();
    };
    manifests.iter().filter(|m| folder_of(&m.path) == dir).flat_map(|m| m.dependencies.iter().cloned()).collect()
}

/// The folder part of `path`, empty at the root.
fn folder_of(path: &str) -> &str {
    path.rfind('/').map_or("", |at| &path[..at])
}

/// The path sits somewhere under `dir` (the root holds everything).
fn is_under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// The declaration a line falls inside: the innermost one that starts at or
/// above it and has not ended yet. `None` when the line sits outside every
/// declaration (top-level code). A declaration with no end line recorded
/// covers only what starts after it, which is the best an older map allows.
pub(crate) fn enclosing(declarations: &[Decl], line: usize) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, d) in declarations.iter().enumerate() {
        let covers = d.line <= line && (d.end_line == 0 || d.end_line >= line);
        if covers && best.is_none_or(|b| declarations[b].line <= d.line) {
            best = Some(i);
        }
    }
    best
}

/// Os tipos que têm membros: a declaração liga ao dono mais interno dela só
/// quando ele é um destes.
const TYPE_KINDS: &[&str] = &["class", "struct", "record", "interface", "trait", "enum", "type"];

/// No enum, só estes são membros: o campo de uma variante não é.
const ENUM_MEMBER_KINDS: &[&str] = &["enum_member", "method", "function", "constant", "const"];

/// O que implementa e o que é implementado: os métodos, que em algumas línguas
/// o mapa grava como função.
const METHOD_KINDS: &[&str] = &["method", "function"];

/// Uma declaração do projeto: o arquivo e a posição dela nele.
type DeclId = (usize, usize);

/// As declarações que cada uma liga, pela posição.
type Links = HashMap<DeclId, Vec<DeclId>>;

/// O que se grava numa declaração: os membros, o que ela implementa e o que a
/// implementa.
#[derive(Default)]
struct Linked {
    members: Vec<DeclAt>,
    implements: Vec<DeclAt>,
    implemented_by: Vec<DeclAt>,
}

/// Os membros de cada tipo e as implementações de cada método, refeitos do
/// zero a cada passada a partir do dono e do contrato que cada declaração traz
/// do arquivo dela (`Decl::owner`, `Decl::contract`). A passada que releu só
/// o que mudou liga o mesmo que a passada inteira: o dono escrito fora da
/// declaração volta do mapa com o arquivo que não mudou.
///
/// - Membros de um tipo: as declarações cujo dono mais interno é ele. O dono
///   que contém a declaração no arquivo é o dela; o escrito fora dela (o tipo
///   do `impl`, o receptor) se acha pelo nome. No enum, só os tipos de
///   [`ENUM_MEMBER_KINDS`]. Os métodos vêm primeiro.
/// - Implementação: o método com o nome de um método do contrato. O contrato
///   é o escrito com o dono (o traço de `impl Traço for Tipo`); sem ele, e
///   quando o tipo dono contém o método, os tipos de que o dono parte
///   (`Decl::supertypes`).
/// - Um nome de tipo repetido no projeto vale o do mesmo arquivo; senão, o de
///   caminho mais parecido (mais pastas em comum no começo); empatado, a
///   ligação não entra.
pub fn link_members(modules: &mut [Module]) {
    let (members, implements) = resolve_members(modules);
    let at = |modules: &[Module], (mi, di): DeclId| {
        let d = &modules[mi].declarations[di];
        DeclAt { file: modules[mi].path.clone(), line: d.line, name: d.name.clone() }
    };
    let mut linked: HashMap<DeclId, Linked> = HashMap::new();
    for (ty, list) in &members {
        linked.entry(*ty).or_default().members = list.iter().map(|&m| at(modules, m)).collect();
    }
    for (method, targets) in &implements {
        for &target in targets {
            linked.entry(*method).or_default().implements.push(at(modules, target));
            linked.entry(target).or_default().implemented_by.push(at(modules, *method));
        }
    }
    for (mi, m) in modules.iter_mut().enumerate() {
        for (di, d) in m.declarations.iter_mut().enumerate() {
            let Linked { members, mut implements, mut implemented_by } = linked.remove(&(mi, di)).unwrap_or_default();
            implements.sort();
            implements.dedup();
            implemented_by.sort();
            implemented_by.dedup();
            d.members = members;
            d.implements = implements;
            d.implemented_by = implemented_by;
        }
    }
}

/// A faixa de `outer` contém a de `inner`, e não é a mesma.
fn contains(outer: &Decl, inner: &Decl) -> bool {
    outer.line <= inner.line
        && inner.end_line <= outer.end_line
        && (outer.line, outer.end_line) != (inner.line, inner.end_line)
}

/// Os membros de cada tipo, na ordem em que o mapa os lista, e o que cada
/// método implementa. Separado de [`link_members`] para ler o projeto inteiro
/// antes de escrever em qualquer declaração.
fn resolve_members(modules: &[Module]) -> (Links, Links) {
    let decl = |(mi, di): DeclId| &modules[mi].declarations[di];
    let mut types: HashMap<&str, Vec<DeclId>> = HashMap::new();
    let mut named: HashMap<(usize, &str), Vec<usize>> = HashMap::new();
    for (mi, m) in modules.iter().enumerate() {
        for (di, d) in m.declarations.iter().enumerate() {
            if TYPE_KINDS.contains(&d.kind.as_str()) {
                types.entry(d.name.as_str()).or_default().push((mi, di));
            }
            named.entry((mi, d.name.as_str())).or_default().push(di);
        }
    }
    // O tipo chamado `name` visto do arquivo `from`: o do mesmo arquivo, ou o
    // de caminho mais parecido; empatado, nenhum.
    let pick = |name: &str, from: usize| -> Option<DeclId> {
        let found = types.get(name)?;
        let same: Vec<DeclId> = found.iter().copied().filter(|&(mi, _)| mi == from).collect();
        if !same.is_empty() {
            return (same.len() == 1).then(|| same[0]);
        }
        let shared = |&(mi, _): &DeclId| common_folders(&modules[from].path, &modules[mi].path);
        let best = found.iter().map(shared).max()?;
        let mut closest = found.iter().copied().filter(|id| shared(id) == best);
        let first = closest.next()?;
        closest.next().is_none().then_some(first)
    };
    // O dono mais interno de cada declaração, quando ele é um tipo: o do
    // mesmo arquivo que a contém, ou o escrito fora dela, pelo nome.
    let mut owner_of: HashMap<DeclId, DeclId> = HashMap::new();
    for (mi, m) in modules.iter().enumerate() {
        for (di, d) in m.declarations.iter().enumerate() {
            let Some(name) = d.owner.first() else { continue };
            let inside = named
                .get(&(mi, name.as_str()))
                .into_iter()
                .flatten()
                .copied()
                .filter(|&oi| contains(&m.declarations[oi], d))
                .min_by_key(|&oi| {
                    let o = &m.declarations[oi];
                    (o.end_line.saturating_sub(o.line), std::cmp::Reverse(o.line))
                });
            let owner = match inside {
                Some(oi) => Some((mi, oi)).filter(|&id| TYPE_KINDS.contains(&decl(id).kind.as_str())),
                None => pick(name, mi),
            };
            if let Some(owner) = owner
                && owner != (mi, di)
                && (decl(owner).kind != "enum" || ENUM_MEMBER_KINDS.contains(&d.kind.as_str()))
            {
                owner_of.insert((mi, di), owner);
            }
        }
    }
    let mut members: Links = HashMap::new();
    for (&member, &owner) in &owner_of {
        members.entry(owner).or_default().push(member);
    }
    for list in members.values_mut() {
        list.sort_by_key(|&(mi, di)| {
            let d = &modules[mi].declarations[di];
            (!METHOD_KINDS.contains(&d.kind.as_str()), modules[mi].path.as_str(), d.line, di)
        });
    }
    let mut implements: Links = HashMap::new();
    for (&method, &owner) in &owner_of {
        let d = decl(method);
        if !METHOD_KINDS.contains(&d.kind.as_str()) {
            continue;
        }
        let contracts: Vec<DeclId> = if !d.contract.is_empty() {
            d.contract.iter().filter_map(|name| pick(name, method.0)).collect()
        } else if owner.0 == method.0 && contains(decl(owner), d) {
            decl(owner).supertypes.iter().filter_map(|name| pick(name, owner.0)).collect()
        } else {
            Vec::new()
        };
        for contract in contracts.into_iter().filter(|&c| c != owner) {
            let same_name = members
                .get(&contract)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&m| METHOD_KINDS.contains(&decl(m).kind.as_str()) && decl(m).name == d.name);
            implements.entry(method).or_default().extend(same_name);
        }
    }
    (members, implements)
}

/// Quantas pastas os dois caminhos têm em comum, a partir do começo.
fn common_folders(a: &str, b: &str) -> usize {
    fn folders(path: &str) -> impl Iterator<Item = &str> {
        path.rsplit_once('/').map(|(dir, _)| dir).into_iter().flat_map(|dir| dir.split('/'))
    }
    folders(a).zip(folders(b)).take_while(|(x, y)| x == y).count()
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
    /// O nome de cada arquivo, sem a extensão, e o de cada pasta do projeto.
    local_names: HashSet<String>,
    go_module: &'a Option<String>,
    packages: &'a [(String, String)],
    /// Os apelidos de pasta das configurações do projeto.
    aliases: &'a PathAliases,
    /// O que cada trio `(língua, pacote, resto)` resolveu, uma vez perguntado:
    /// a língua de quem importa decide que extensão sai do resto.
    package_hits: RefCell<HashMap<(String, String, String), Vec<String>>>,
    /// Os arquivos que declaram cada nome pedido a um arquivo que repassa,
    /// uma vez seguidos os repasses a partir dele.
    through_hits: RefCell<HashMap<(String, String), Vec<String>>>,
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
            local_names: modules
                .iter()
                .flat_map(|m| {
                    let dir = parent_dir(&m.path);
                    let folders: Vec<String> = dir.split('/').filter(|s| !s.is_empty()).map(str::to_string).collect();
                    folders.into_iter().chain([file_stem(&m.path)])
                })
                .collect(),
            go_module,
            packages,
            aliases,
            package_hits: RefCell::new(HashMap::new()),
            through_hits: RefCell::new(HashMap::new()),
        }
    }

    /// Os arquivos do projeto que um import de `importer` liga, seguidos os
    /// repasses: cada nome que ele traz e que o arquivo alvo não declara, mas
    /// repassa ([`Module::reexports`], pelo nome ou por `*`), liga ao arquivo
    /// que o declara. O arquivo alvo fica pelos nomes que ele mesmo declara,
    /// pelos que nenhum arquivo alcançado declara e pelo import que não traz
    /// nome. A única leitura dos imports do corpo e do trecho de teste: as
    /// arestas, o `deps` e, por ele, a ligação das declarações veem o nome
    /// no arquivo que o define.
    fn resolve_through(&self, imp: &str, importer: &Module, nested: usize) -> Vec<String> {
        let targets = self.resolve(imp, importer, Reach::Used, nested);
        let names = importer.brought.get(imp).map(Vec::as_slice).unwrap_or_default();
        if names.is_empty() {
            return targets;
        }
        let mut out: BTreeSet<String> = BTreeSet::new();
        for target in targets {
            let Some(module) = self.by_path.get(target.as_str()).filter(|m| !m.reexports.is_empty()) else {
                out.insert(target);
                continue;
            };
            let mut stays = false;
            for name in names {
                let key = (target.clone(), name.clone());
                let cached = self.through_hits.borrow().get(&key).cloned();
                let found = cached.unwrap_or_else(|| {
                    let found = self.declared_through(module, name, &mut HashSet::new());
                    self.through_hits.borrow_mut().insert(key, found.clone());
                    found
                });
                stays |= found.is_empty();
                out.extend(found);
            }
            if stays {
                out.insert(target);
            }
        }
        out.into_iter().collect()
    }

    /// Os arquivos que declaram `name` a partir de `module`: ele mesmo, quando
    /// o declara; senão, os que cada repasse dele que oferece o nome alcança,
    /// pelo nome de origem. `seen` guarda cada par de arquivo e nome já
    /// visitado, e um repasse que volta a um deles não é seguido de novo.
    fn declared_through(&self, module: &'a Module, name: &str, seen: &mut HashSet<(&'a str, String)>) -> Vec<String> {
        if !seen.insert((module.path.as_str(), name.to_string())) {
            return Vec::new();
        }
        if module.declarations.iter().any(|d| d.name == name) {
            return vec![module.path.clone()];
        }
        let mut found = Vec::new();
        for (imp, offered) in &module.reexports {
            let original = match offered.get(name) {
                Some(original) => original.as_str(),
                None if offered.contains_key("*") => name,
                None => continue,
            };
            for nested in module.import_depths(imp, false) {
                for target in self.resolve(imp, module, Reach::Used, nested) {
                    if let Some(&next) = self.by_path.get(target.as_str()) {
                        found.extend(self.declared_through(next, original, seen));
                    }
                }
            }
        }
        found.sort();
        found.dedup();
        found
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
        // 2c) Módulo filho: na língua que declara `root_aliases`, o caminho
        //     sem apelido cuja primeira parte é módulo filho de quem importa
        //     (`io` em `pub use io::leitor::Leitor`, escrito no `lib.rs`) se lê
        //     como se tivesse o apelido do próprio módulo na frente: na pasta
        //     dos módulos de quem importa, tirando do fim quantas partes for
        //     preciso até achar arquivo. Só fora dos módulos escritos no
        //     arquivo, cujos filhos são outros.
        if nested == 0
            && let Some(hits) = self.child_module(&canon, importer)
        {
            return hits;
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

    /// O import que não nomeia nada do projeto, como `std::fs::{self}`: não é
    /// relativo, não começa por um apelido da raiz, da pasta de cima, das
    /// configurações ou do módulo declarado, nenhum começo dele é pacote do
    /// projeto e a primeira parte não é nome de arquivo nem de pasta do
    /// projeto. Quem chama já sabe que ele não resolve a arquivo nenhum; o
    /// que parece do projeto e não resolve fica de dentro, por cautela.
    fn outside(&self, imp: &str, importer: &Module) -> bool {
        let lang = importer.language.as_str();
        if self.separated_relative(imp, importer).is_some()
            || !self.aliases.candidates(&importer.path, lang, imp).is_empty()
            || self.go_module.as_deref().is_some_and(|module| imp.starts_with(module))
        {
            return false;
        }
        let canon = canon_segments(imp, lang);
        let canon = canon.strip_prefix("package:").unwrap_or(&canon);
        if canon.starts_with('.') {
            return false;
        }
        let segments: Vec<&str> = canon.split('/').collect();
        let first = segments.first().copied().unwrap_or_default();
        let aliased = crate::extract::root_aliases(lang).contains(&first)
            || crate::extract::parent_alias(lang) == Some(first);
        let package = (1..=segments.len())
            .any(|cut| self.packages.iter().any(|(name, _)| *name == fold_package(&segments[..cut].join("/"))));
        !(aliased || package || self.local_names.contains(first))
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

    /// Os arquivos que um caminho sem apelido cita quando a primeira parte
    /// dele é módulo filho de quem importa, na língua que declara
    /// `root_aliases`: o arquivo mais fundo que o caminho nomeia dentro da
    /// pasta dos módulos de quem importa ([`inner_folder`]). `None` quando a
    /// língua não declara apelido, o caminho começa por um deles ou pelo
    /// `parent_alias`, ou a primeira parte não é módulo filho.
    fn child_module(&self, canon: &str, importer: &Module) -> Option<Vec<String>> {
        let lang = importer.language.as_str();
        let aliases = crate::extract::root_aliases(lang);
        let segments: Vec<&str> = canon.split('/').collect();
        let first = segments.first().copied().unwrap_or_default();
        if aliases.is_empty()
            || first.is_empty()
            || aliases.contains(&first)
            || crate::extract::parent_alias(lang) == Some(first)
        {
            return None;
        }
        let base = inner_folder(&importer.path, lang);
        let probe = |cut: usize| {
            let place: Vec<&str> =
                std::iter::once(base.as_str()).chain(segments[..cut].iter().copied()).filter(|s| !s.is_empty()).collect();
            exact_path_candidate(&place.join("/"), lang, &self.stem_index, &self.module_paths)
        };
        if probe(1).is_empty() {
            return None;
        }
        (1..=segments.len()).rev().map(probe).find(|hits| !hits.is_empty())
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
        let mut base = inner_folder(&importer.path, &importer.language);
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
        let mut hits = probed.unwrap_or_else(|| {
            let ending = format!("/{}", strip_import_ext(tail, lang));
            let mut hits: Vec<String> = stem_index
                .iter()
                .filter(|(stem, _)| stem.ends_with(&ending) && dirs.iter().any(|d| is_under(stem, d)))
                .flat_map(|(_, v)| v.iter().cloned())
                .collect();
            hits.sort(); // stable output: HashMap iteration order varies per run
            hits
        });
        // O resto que não chega a arquivo nenhum (`Leitor` em
        // `demo_core::Leitor`) é um nome do arquivo raiz do pacote
        // (`package_entry` no registro), que o declara ou o repassa.
        if hits.is_empty() {
            hits = dirs
                .iter()
                .flat_map(|dir| crate::extract::package_entry(lang).iter().map(move |entry| join_dir(dir, entry)))
                .map(|entry| exact_path_candidate(&entry, lang, stem_index, module_paths))
                .find(|found| !found.is_empty())
                .unwrap_or_default();
        }
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
/// o nome dele ou, no arquivo que responde pela própria pasta e no arquivo
/// raiz de um pacote (`package_entry` no registro, como `src/lib`), a pasta
/// dele.
fn inner_folder(path: &str, lang: &str) -> String {
    let stem = strip_ext(path);
    let entry = crate::extract::package_entry(lang)
        .iter()
        .any(|entry| stem == *entry || stem.strip_suffix(entry).is_some_and(|dir| dir.ends_with('/')));
    if entry || INDEX_FILES.contains(&file_stem(path).as_str()) { parent_dir(path) } else { stem }
}

/// O caminho `rest` dentro da pasta `dir` (vazia na raiz do projeto).
fn join_dir(dir: &str, rest: &str) -> String {
    if dir.is_empty() { rest.to_string() } else { format!("{dir}/{rest}") }
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
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
