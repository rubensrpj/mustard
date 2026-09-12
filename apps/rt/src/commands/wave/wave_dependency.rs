//! `mustard-rt run wave-dependency` — a port of `scripts/wave-dependency.js`.
//!
//! Builds a dependency DAG from a list of files (via import/require parsing)
//! and groups files into waves using topological level assignment.
//!
//! Input arrives as JSON on stdin (`{ files, projectRoot }`); output is one
//! JSON object on stdout. Fail-open: an unrecoverable error emits
//! `{ "error": "error-fallback" }`.

use crate::commands::wave::wave_lib::{
    detect_role_with, load_role_patterns, load_wave_layer_order,
};
use mustard_core::{io::fs, RolePattern};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Extensions that can be appended when resolving a relative import.
const RESOLVABLE_EXTENSIONS: &[&str] = &[
    ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".vue", ".svelte", ".py", ".go", ".cs",
];
/// Index basenames probed when an import resolves to a directory.
const INDEX_BASENAMES: &[&str] = &[
    "index.ts",
    "index.tsx",
    "index.js",
    "index.jsx",
    "index.mjs",
    "__init__.py",
];

/// Extract import/require specifiers from file content.
///
/// Mirrors the four JS regexes: ES `import ... from '...'`, bare
/// `import '...'`, `require('...')`, and Python `from x import`.
fn extract_imports(content: &str) -> Vec<String> {
    let mut imports: BTreeSet<String> = BTreeSet::new();

    // Capture every `'...'` / `"..."` string literal that follows `from` or
    // `require(` or a bare `import`, plus Python `from <mod> import`.
    for (idx, _) in content.match_indices("from ") {
        let after = &content[idx + 5..];
        if let Some(spec) = leading_quoted(after) {
            imports.insert(spec);
        } else {
            // Python `from <mod> import` — `[.\w]+`.
            let module: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_')
                .collect();
            let rest = &after[module.len()..];
            if !module.is_empty() && rest.trim_start().starts_with("import ") {
                imports.insert(module);
            }
        }
    }
    for (idx, _) in content.match_indices("import ") {
        let after = &content[idx + 7..];
        if let Some(spec) = leading_quoted(after) {
            imports.insert(spec);
        }
    }
    for (idx, _) in content.match_indices("require") {
        let after = content[idx + 7..].trim_start();
        if let Some(after) = after.strip_prefix('(')
            && let Some(spec) = leading_quoted(after.trim_start()) {
                imports.insert(spec);
            }
    }

    imports.into_iter().collect()
}

/// If `s` begins with a quoted string (`'...'` or `"..."`), return its content.
fn leading_quoted(s: &str) -> Option<String> {
    let q = s.chars().next()?;
    if q != '\'' && q != '"' {
        return None;
    }
    let rest = &s[1..];
    let end = rest.find(q)?;
    Some(rest[..end].to_string())
}

/// Resolve a relative import to an absolute path in `candidate_set`.
fn resolve_import(
    import_path: &str,
    current_file: &Path,
    candidate_set: &BTreeSet<PathBuf>,
) -> Option<PathBuf> {
    if !import_path.starts_with('.') && !import_path.starts_with('/') {
        return None;
    }
    let base_dir = current_file.parent()?;
    let abs_target = normalize(&base_dir.join(import_path));

    if candidate_set.contains(&abs_target) {
        return Some(abs_target);
    }
    for ext in RESOLVABLE_EXTENSIONS {
        let with_ext = PathBuf::from(format!("{}{ext}", abs_target.display()));
        if candidate_set.contains(&with_ext) {
            return Some(with_ext);
        }
    }
    // Strip an existing extension and retry.
    if let Some(stem) = abs_target.file_stem()
        && abs_target.extension().is_some() {
            let stripped = abs_target.with_file_name(stem);
            if candidate_set.contains(&stripped) {
                return Some(stripped);
            }
            for ext in RESOLVABLE_EXTENSIONS {
                let swapped = PathBuf::from(format!("{}{ext}", stripped.display()));
                if candidate_set.contains(&swapped) {
                    return Some(swapped);
                }
            }
        }
    for basename in INDEX_BASENAMES {
        let index_path = abs_target.join(basename);
        if candidate_set.contains(&index_path) {
            return Some(index_path);
        }
    }
    None
}

/// Lexically normalize a path (resolve `.` / `..`) without touching the disk.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Build the dependency graph.
fn build_graph(
    files: &[String],
    project_root: &Path,
) -> BTreeMap<PathBuf, BTreeSet<PathBuf>> {
    let abs_files: Vec<PathBuf> = files
        .iter()
        .map(|f| {
            let p = Path::new(f);
            if p.is_absolute() {
                normalize(p)
            } else {
                normalize(&project_root.join(p))
            }
        })
        .collect();
    let candidate_set: BTreeSet<PathBuf> = abs_files.iter().cloned().collect();
    let mut graph: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();

    for abs_file in &abs_files {
        let deps = graph.entry(abs_file.clone()).or_default();
        let Ok(content) = fs::read_to_string(abs_file) else {
            continue;
        };
        for imp in extract_imports(&content) {
            if let Some(resolved) = resolve_import(&imp, abs_file, &candidate_set)
                && &resolved != abs_file {
                    deps.insert(resolved);
                }
        }
    }
    graph
}

/// Result of the topological pass.
enum TopoResult {
    Waves(Vec<Vec<PathBuf>>),
    Cycle(Vec<PathBuf>),
}

/// Assign files to waves by topological level. Files with no in-graph
/// dependencies are wave 1; cyclic files yield `Cycle`.
fn topological_waves(graph: &BTreeMap<PathBuf, BTreeSet<PathBuf>>) -> TopoResult {
    let levels = crate::shared::dag::assign_levels(graph);
    if levels.cycle.is_empty() {
        TopoResult::Waves(levels.rounds())
    } else {
        TopoResult::Cycle(levels.cycle)
    }
}

/// Relativize `abs` against `project_root`, with forward slashes.
fn to_relative(abs: &Path, project_root: &Path) -> String {
    abs.strip_prefix(project_root).map_or_else(
        |_| abs.to_string_lossy().replace('\\', "/"),
        |p| p.to_string_lossy().replace('\\', "/"),
    )
}

/// Compute the wave-DAG result JSON for a list of files.
///
/// Shared with `exec-rewave-check`, which used to shell to `wave-dependency.js`
/// — it now calls this directly. The shape follows the JS stdout, plus the
/// per-wave `dependsOnOrigin` marker (`imports` here, `layer-order` on the
/// role fallback) so a consumer can tell a derived edge from a declared one.
pub fn compute_waves(files: &[String], project_root: &Path) -> Value {
    if files.is_empty() {
        return json!({ "error": "empty-input" });
    }
    let graph = build_graph(files, project_root);
    // F0-e: role-classification overrides from `mustard.json#rolePatterns`.
    let role_patterns = load_role_patterns(project_root);
    match topological_waves(&graph) {
        TopoResult::Cycle(stuck) => {
            let cycle: Vec<String> = stuck.iter().map(|f| to_relative(f, project_root)).collect();
            json!({ "error": "cyclic-dependency", "cycle": cycle })
        }
        TopoResult::Waves(wave_files) => {
            // Net-new features have no import edges yet, so the DAG flattens to a
            // single level even when the files span multiple architectural
            // layers. When that happens, derive the waves from the files' roles
            // ordered by `mustard.json#waveLayerOrder` (documented default), so a
            // backend->core->ui net-new feature still decomposes deterministically
            // instead of collapsing to one wave.
            if wave_files.len() == 1 {
                let layer_order = load_wave_layer_order(project_root);
                if let Some(fallback) =
                    role_layered_fallback(&wave_files[0], project_root, &role_patterns, &layer_order)
                {
                    return fallback;
                }
            }
            // REAL topological edges: wave number (1-based) per file, so each
            // wave's `dependsOn` names the earlier waves that actually hold a
            // file this wave's files import — the DAG's own topology, never a
            // fabricated `wave N depends on N-1` index chain.
            let mut wave_of: BTreeMap<&PathBuf, usize> = BTreeMap::new();
            for (idx, files) in wave_files.iter().enumerate() {
                for f in files {
                    wave_of.insert(f, idx + 1);
                }
            }
            let mut widest = 0usize;
            let waves: Vec<Value> = wave_files
                .iter()
                .enumerate()
                .map(|(idx, files)| {
                    let rel: Vec<String> =
                        files.iter().map(|f| to_relative(f, project_root)).collect();
                    widest = widest.max(rel.len());
                    let mut roles: Vec<String> = Vec::new();
                    for r in rel.iter().map(|f| detect_role_with(f, &role_patterns)) {
                        if !roles.contains(&r) {
                            roles.push(r);
                        }
                    }
                    // The distinct earlier waves some file of THIS wave imports
                    // from (in-graph deps only) — sorted asc, deduped.
                    let depends: BTreeSet<usize> = files
                        .iter()
                        .filter_map(|f| graph.get(f))
                        .flatten()
                        .filter_map(|dep| wave_of.get(dep).copied())
                        .filter(|&w| w != idx + 1)
                        .collect();
                    json!({
                        "wave": idx + 1,
                        "files": rel,
                        "roles": roles,
                        "dependsOn": depends.into_iter().collect::<Vec<_>>(),
                        // Origin of every edge above: derived from the import
                        // graph (a file here imports a file there).
                        "dependsOnOrigin": "imports",
                    })
                })
                .collect();
            json!({
                "waves": waves,
                "metadata": {
                    "totalWaves": wave_files.len(),
                    "totalFiles": files.len(),
                    "widestWave": widest,
                },
            })
        }
    }
}

/// Deterministic role-layered decomposition for a flat (no-edge) DAG.
///
/// Returns `Some(wavesJson)` ONLY when the files span >= 2 architectural layers
/// — applying the same lib-folding rule the scope decider uses (a lone `lib`
/// bucket is one layer, never split). Otherwise `None` and the caller keeps the
/// single import-DAG wave. Roles are scheduled in `layer_order` (case-insensitive
/// match), each wave depending on the previous — HERE the linear chain is the
/// derivation itself (each layer builds on the one before, per the configured
/// order), so the edges carry origin `layer-order`. Roles absent from the order
/// fall to the tail in lexical order. The emitted shape matches the import-DAG
/// path (`{wave, files, roles, dependsOn, dependsOnOrigin}` + `metadata`).
fn role_layered_fallback(
    files: &[PathBuf],
    project_root: &Path,
    role_patterns: &[RolePattern],
    layer_order: &[String],
) -> Option<Value> {
    let mut by_role: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in files {
        let rel = to_relative(f, project_root);
        let role = detect_role_with(&rel, role_patterns);
        by_role.entry(role).or_default().push(rel);
    }
    // Same lib-folding rule as scope_decompose::decide / exec_rewave_check: a
    // lone "lib" bucket counts as one layer, so a purely-generic net-new slice
    // is not split.
    let layer_count = if by_role.len() == 1 && by_role.contains_key("lib") {
        1
    } else {
        by_role.len()
    };
    if layer_count < 2 {
        return None;
    }
    // Schedule the role buckets: ordered roles first (case-insensitive match
    // against the config/default order), then any remainder in lexical order.
    let mut ordered: Vec<String> = Vec::new();
    for want in layer_order {
        for key in by_role.keys() {
            if key.eq_ignore_ascii_case(want) && !ordered.iter().any(|o| o == key) {
                ordered.push(key.clone());
            }
        }
    }
    for key in by_role.keys() {
        if !ordered.iter().any(|o| o == key) {
            ordered.push(key.clone());
        }
    }
    // Backstop: an architectural decomposition is a handful of layers, not a
    // dozen. If role detection fragments the census into many buckets it is
    // mislabeling (random path prefixes read as bespoke roles), not a real
    // layering — keep the single import-DAG wave rather than emit one wave per
    // noise role (field report: a flat net-new census fanned out to 11 waves).
    const MAX_FALLBACK_LAYERS: usize = 6;
    if ordered.len() > MAX_FALLBACK_LAYERS {
        return None;
    }
    let mut widest = 0usize;
    let mut total_files = 0usize;
    let waves: Vec<Value> = ordered
        .iter()
        .enumerate()
        .map(|(idx, role)| {
            let wave_files = by_role.get(role).cloned().unwrap_or_default();
            widest = widest.max(wave_files.len());
            total_files += wave_files.len();
            json!({
                "wave": idx + 1,
                "files": wave_files,
                "roles": [role],
                "dependsOn": if idx == 0 { json!([]) } else { json!([idx]) },
                // Origin: the configured layer schedule — each layer builds on
                // the previous. The chain IS this path's derivation, not an
                // index fabrication.
                "dependsOnOrigin": "layer-order",
            })
        })
        .collect();
    Some(json!({
        "waves": waves,
        "metadata": {
            "totalWaves": ordered.len(),
            "totalFiles": total_files,
            "widestWave": widest,
        },
    }))
}

/// Extract the file list from a parsed input document.
///
/// Accepts BOTH input shapes — the prose⇄binary drift that made the documented
/// `wave-dependency < plan.json` form answer `empty-input` (the refs said to
/// feed the plan JSON while the binary only parsed `{files}`; first recorded as
/// a follow-up in spec `redesenho-agnostico-indice-termos-digest`):
///
/// - **derivation shape**: top-level `files: [...]` (+ optional `projectRoot`);
/// - **plan JSON** (the same document `plan-materialize --plan` consumes):
///   `waves: [{files: [...]}]` — the per-wave censuses are unioned in wave
///   order, first occurrence wins (dedup), so shared files create no phantom
///   duplicate nodes in the DAG.
fn files_from_value(parsed: &Value) -> Vec<String> {
    if let Some(arr) = parsed.get("files").and_then(Value::as_array) {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for wave in parsed
        .get("waves")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        for f in wave
            .get("files")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
        {
            if seen.insert(f.to_string()) {
                out.push(f.to_string());
            }
        }
    }
    out
}

/// Input position (1-based) a DECLARED dependency reference names: a number
/// (`2`), a numeric string (`"2"`), or a wave name (`"wave-2-backend"` — the
/// spelling `wave-plan.md` / `WavePlanEntry::depends_on` carries). `None` when
/// the reference carries no readable position.
fn declared_ref_position(v: &Value) -> Option<usize> {
    if let Some(n) = v.as_u64() {
        return usize::try_from(n).ok().filter(|n| *n >= 1);
    }
    let s = v.as_str()?.trim();
    let digits: String = s
        .strip_prefix("wave-")
        .unwrap_or(s)
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<usize>().ok().filter(|n| *n >= 1)
}

/// Uma colisão de arquivo entre duas ondas que o despacho solta na MESMA
/// rodada, com o encadeamento mínimo que a zera.
///
/// É a saída que [`passthrough_plan_waves`] descartava: a união deduplicada
/// apaga o arquivo compartilhado antes de qualquer checagem olhar para ele, e
/// a dedup continua valendo na lista `files` (nenhum nó fantasma no grafo de
/// imports) — a interseção passa a viajar ao lado dela, não no lugar dela.
///
/// A ordenação é `(level, waves[0], waves[1])` e `files` sai ordenado (vem de
/// uma interseção de `BTreeSet`), então o array é byte-estável.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct FileCollision {
    /// O nível topológico que as duas ondas COMPARTILHAM. Ondas de níveis
    /// diferentes estão sequenciadas pela aresta que as separa e nunca aparecem
    /// aqui.
    pub(crate) level: u32,
    /// As duas ondas, sempre em ordem crescente — a menor primeiro, que é o que
    /// faz [`FileCollision::chain`] apontar sempre para trás. São os números que
    /// as ondas DECLARAM: o que nomeia o diretório e a linha que o operador
    /// edita.
    pub(crate) waves: [u32; 2],
    /// Os arquivos que AMBAS declaram, ordenados.
    pub(crate) files: Vec<String>,
    /// O encadeamento mínimo que zera a sobreposição, já em prosa acionável: uma
    /// aresta só, na onda de número maior. Sequenciar as duas é a única coisa
    /// que separa dois agentes editando o mesmo arquivo sem nada ordenando-os;
    /// dividir o arquivo entre elas é a outra saída, e continua sendo escolha do
    /// autor do plano.
    pub(crate) chain: String,
}

impl FileCollision {
    /// Monta a colisão a partir do par já ordenado (`a <= b`) e da interseção.
    ///
    /// `a == b` é o plano que numerou duas ondas igual. O encadeamento por número
    /// não existe aí — `depends_on: [1]` numa das duas ondas 1 é ambíguo, quando
    /// não uma auto-aresta —, então a prescrição nomeia o passo que falta antes:
    /// dar número próprio a uma delas. Numerar é do autor do plano; renumerar
    /// sozinho é Não-Objetivo declarado.
    fn new(level: u32, a: u32, b: u32, files: Vec<String>) -> Self {
        let chain = if a == b {
            format!(
                "two waves are both numbered {a} — give one of them its own n, then add wave {a} \
                 to that wave's depends_on"
            )
        } else {
            format!("add wave {a} to wave {b}'s depends_on")
        };
        Self { level, waves: [a, b], files, chain }
    }
}

/// Os pares de ondas do MESMO nível de despacho que declaram o mesmo arquivo.
///
/// Cada entrada de `waves` é `(número que o chamador publica, nível topológico,
/// arquivos declarados)`. O conjunto de arquivos entra SEM dedup entre ondas —
/// deduplicar é exatamente o que apaga a evidência da colisão.
///
/// O agrupamento é por nível porque é por nível que o despacho paraleliza (ver
/// [`crate::commands::pipeline::dispatch_plan`]): duas ondas do mesmo nível não
/// têm aresta nenhuma entre si, logo nada as sequencia. É a mesma leitura que
/// `wave-overlap-check` faz do plano já materializado, agora compartilhada, para
/// que os dois passos não possam mais discordar sobre o mesmo plano.
pub(crate) fn same_level_collisions(
    waves: &[(u32, u32, BTreeSet<String>)],
) -> Vec<FileCollision> {
    let mut by_level: BTreeMap<u32, Vec<(u32, &BTreeSet<String>)>> = BTreeMap::new();
    for (wave, level, files) in waves {
        by_level.entry(*level).or_default().push((*wave, files));
    }
    let mut out: Vec<FileCollision> = Vec::new();
    for (level, mut group) in by_level {
        group.sort_by_key(|(wave, _)| *wave);
        for i in 0..group.len() {
            for j in (i + 1)..group.len() {
                let shared: Vec<String> = group[i].1.intersection(group[j].1).cloned().collect();
                if !shared.is_empty() {
                    out.push(FileCollision::new(level, group[i].0, group[j].0, shared));
                }
            }
        }
    }
    out
}

/// Uma onda como o CENSO a lê — a leitura única do `plan.json` que os dois
/// passos compartilham.
pub(crate) struct DeclaredWave {
    /// O número que a onda DECLARA (`n`), ou a posição de entrada quando ela não
    /// declara nenhum. É o número que nomeia o diretório `wave-{n}-{role}` e a
    /// linha de `depends_on` que o operador vai editar — logo é dele que o
    /// relatório tem de falar.
    pub(crate) number: u32,
    /// O nível de despacho: ondas de mesmo nível saem juntas, sem nada entre
    /// elas.
    pub(crate) level: u32,
    /// Os arquivos que ESTA onda declarou, sem dedup entre ondas — deduplicar é
    /// exatamente o que apaga a evidência da colisão.
    pub(crate) files: BTreeSet<String>,
    /// As POSIÇÕES de entrada (0-based) que o `depends_on` desta onda alcança.
    pub(crate) deps: BTreeSet<usize>,
}

/// Os arquivos que UMA onda do plano declara, na ordem de declaração e já lidos
/// pelo normalizador único.
///
/// A leitura de caminho do `plan.json` mora aqui, e só aqui: o censo do portão,
/// o `sharedFiles` do comando e a lista `files` que ele publica atravessam esta
/// função. Enquanto havia duas leituras, um `./` numa das ondas bastava para o
/// portão não ver a colisão que a auditoria via.
fn declared_files(wave: &Value) -> impl Iterator<Item = String> + '_ {
    wave.get("files")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .map(crate::commands::pipeline::dispatch_plan::normalise_declared_path)
        .filter(|f| !f.is_empty())
}

/// O censo das ondas que um documento de plano declara: uma linha por onda, na
/// ordem de entrada.
///
/// **Uma regra de numeração só**, e é a razão desta função existir. Antes, o
/// portão numerava pelo `n` declarado e o passo do planejador pela posição de
/// entrada, então os dois liam o MESMO plano e discordavam — um plano com ondas
/// 2 e 3, a 3 dependendo de `wave-2-rt`, fazia o planejador acusar colisão entre
/// "ondas 1 e 2" (que não existem) enquanto o portão via o plano correto. Acabar
/// com essa discordância é a unidade inteira; deixar duas contagens vivas seria
/// reabri-la.
///
/// - **identidade = a posição de entrada**, que é única. Chavear pelo número
///   funde duas ondas de mesmo `n` numa só, e a colisão entre elas some do censo
///   — a mesma perda que a dedup causava, por outra porta.
/// - **número publicado = o `n` declarado** (a posição, quando ausente).
/// - **arestas resolvidas contra o número declarado** das outras ondas, nunca
///   contra a posição: `["wave-2-rt"]` e `[2]` alcançam a onda que se chama 2,
///   esteja ela em que posição estiver. Uma referência a número que ninguém
///   declara não alcança onda nenhuma, e uma auto-referência nunca sobrevive.
/// - **caminho lido pelo normalizador ÚNICO**,
///   [`crate::commands::pipeline::dispatch_plan::normalise_declared_path`] (`\`
///   vira `/`, um `./` inicial cai, extremidades aparadas) — o mesmo que a
///   auditoria irmã `wave-overlap-check` usa nos caminhos que lê do disco. Ler o
///   caminho de dois jeitos era a última camada dividida entre os dois passos: um
///   plano com `./src/shared.rs` numa onda e `src/shared.rs` na outra passava
///   pelo portão e era acusado pela auditoria depois. Um caminho que se esvazia
///   na normalização (uma linha em branco no `files`) não é arquivo e não entra
///   no censo — senão duas ondas com uma linha vazia "colidiriam" nela.
pub(crate) fn declared_wave_census(waves_in: &[Value]) -> Vec<DeclaredWave> {
    let numbers: Vec<u32> = waves_in
        .iter()
        .enumerate()
        .map(|(pos, wave)| {
            wave.get("n")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or_else(|| u32::try_from(pos + 1).unwrap_or(0))
        })
        .collect();
    let mut graph: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut rows: Vec<DeclaredWave> = Vec::new();
    for (pos, wave) in waves_in.iter().enumerate() {
        let refs: BTreeSet<u32> = wave
            .get("dependsOn")
            .or_else(|| wave.get("depends_on"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(declared_ref_position)
            .filter_map(|p| u32::try_from(p).ok())
            .collect();
        let deps: BTreeSet<usize> = numbers
            .iter()
            .enumerate()
            .filter(|(other, n)| *other != pos && refs.contains(n))
            .map(|(other, _)| other)
            .collect();
        // Toda onda é nó do grafo, com aresta ou sem — sem isso ela não ganha
        // nível e cairia fora do pareamento.
        graph.insert(pos, deps.clone());
        rows.push(DeclaredWave {
            number: numbers[pos],
            level: 0,
            files: declared_files(wave).collect(),
            deps,
        });
    }
    let levels = crate::shared::dag::assign_levels(&graph);
    for (pos, row) in rows.iter_mut().enumerate() {
        row.level = levels.level.get(&pos).copied().unwrap_or(0);
    }
    rows
}

/// As colisões que um censo já lido carrega — o pareamento, separado da leitura,
/// para que os dois passos façam a MESMA conta sobre a MESMA numeração.
fn census_collisions(rows: &[DeclaredWave]) -> Vec<FileCollision> {
    let census: Vec<(u32, u32, BTreeSet<String>)> =
        rows.iter().map(|w| (w.number, w.level, w.files.clone())).collect();
    same_level_collisions(&census)
}

/// As colisões de arquivo que um PLANO declara entre ondas do mesmo nível, lidas
/// do próprio `plan.json`.
///
/// A porta in-process de [`crate::commands::pipeline::plan_materialize`], irmã
/// de [`validate_plan_dag`]: a checagem roda em toda materialização, sem
/// depender de o orquestrador relatar uma chamada separada. A numeração e as
/// arestas saem de [`declared_wave_census`] — a mesma leitura que o
/// `sharedFiles` do comando publica.
///
/// Um plano ilegível ou inválido devolve lista VAZIA: quem recusa por plano
/// ilegível é o scaffold, com a mensagem que ensina o schema — não este cálculo,
/// que só sabe falar de colisão.
#[must_use]
pub(crate) fn plan_file_collisions(plan_path: &Path) -> Vec<FileCollision> {
    let Ok(raw) = fs::read_to_string(plan_path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let Some(waves_in) = parsed.get("waves").and_then(Value::as_array) else {
        return Vec::new();
    };
    census_collisions(&declared_wave_census(waves_in))
}

/// Trust an explicit plan's wave boundaries (Option D). When the input is the
/// rich PLAN shape (`waves: [{files:[...]}]`), emit the canonical
/// `{waves, metadata}` from the planner's own boundaries — renumbered, with the
/// `dependsOn` edges the author DECLARED (`dependsOn`/`depends_on`, numbers or
/// wave names) and per-wave roles — instead of flattening to a file union and
/// re-deriving (which lets the flat-DAG role fallback fan a 2-wave plan out to
/// one wave per role). A wave that declares nothing emits NO edges (origin
/// `undeclared`) — the old `wave N depends on N-1` chain here was a fabrication
/// that contradicted plans whose waves are independent. Files are deduped
/// across waves (first occurrence wins, matching the DAG's no-phantom-node
/// rule); a wave that declares NO file at all is dropped, and declared
/// references are remapped onto the surviving output numbers (a reference to a
/// dropped or unknown wave is itself dropped). Returns `None` for the bare
/// `{files}` derivation shape (no `waves` key) or an all-empty plan, so the
/// import-DAG path runs.
///
/// A dedup deixou de ser a única saída: `sharedFiles` publica a interseção que
/// ela descarta, por par de ondas do MESMO nível de despacho, com o
/// encadeamento mínimo que a zera — ver [`FileCollision`].
///
/// A leitura do plano é a de [`declared_wave_census`], a MESMA que o portão de
/// [`plan_file_collisions`] usa: uma referência alcança a onda que se CHAMA
/// aquele número, e é desse número que o `sharedFiles` fala. Resolver referência
/// por posição de entrada era o que fazia os dois passos discordarem sobre o
/// mesmo plano — a `depends_on: ["wave-2-rt"]` de uma onda numerada 3, escrita na
/// segunda posição, virava auto-referência e sumia, inventando uma colisão entre
/// duas ondas que não existiam. O campo `wave` continua sendo a posição de saída:
/// o que muda é de qual número o relatório FALA, não qual número a onda tem.
fn passthrough_plan_waves(parsed: &Value, role_patterns: &[RolePattern]) -> Option<Value> {
    let waves_in = parsed.get("waves").and_then(Value::as_array)?;
    if waves_in.is_empty() {
        return None;
    }
    let census = declared_wave_census(waves_in);
    // Pass 1 — dedup files, keep every wave that DECLARED something, remember
    // each survivor's INPUT position (0-based, o índice do censo).
    struct Kept {
        input_pos: usize,
        files: Vec<String>,
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut kept: Vec<Kept> = Vec::new();
    for (pos, wave) in waves_in.iter().enumerate() {
        // A dedup roda sobre o caminho NORMALIZADO, o mesmo que o censo lê:
        // deduplicar a grafia crua deixava `./src/x.rs` e `src/x.rs` passarem
        // como dois nós do mesmo arquivo — a divisão de leitura que este módulo
        // acabou de fechar, reaberta na lista publicada.
        let mut rel: Vec<String> = Vec::new();
        for f in declared_files(wave) {
            if seen.insert(f.clone()) {
                rel.push(f);
            }
        }
        // O corte é sobre o que a onda DECLAROU, não sobre o que sobrou da
        // dedup: uma onda cujos arquivos foram todos declarados antes é a
        // colisão mais total que existe, e sumir com ela era perder justamente
        // o caso que este cálculo tem de relatar.
        if census.get(pos).is_none_or(|w| w.files.is_empty()) {
            continue;
        }
        kept.push(Kept { input_pos: pos, files: rel });
    }
    if kept.is_empty() {
        return None;
    }
    // Input position → surviving output wave number, so a declared reference
    // stays correct after empty/duplicate waves are dropped.
    let out_number_of: BTreeMap<usize, usize> =
        kept.iter().enumerate().map(|(i, k)| (k.input_pos, i + 1)).collect();
    let mut out_waves: Vec<Value> = Vec::new();
    let mut widest = 0usize;
    let mut total_files = 0usize;
    for (idx, k) in kept.iter().enumerate() {
        let mut roles: Vec<String> = Vec::new();
        for r in k.files.iter().map(|f| detect_role_with(f, role_patterns)) {
            if !roles.contains(&r) {
                roles.push(r);
            }
        }
        widest = widest.max(k.files.len());
        total_files += k.files.len();
        // The edges the author declared, remapped onto the surviving output
        // numbers and deduped (sorted asc); a reference to a dropped wave is
        // itself dropped, and a self-reference never survives the census.
        let row = census.get(k.input_pos);
        let depends: BTreeSet<usize> = row
            .map(|w| &w.deps)
            .into_iter()
            .flatten()
            .filter_map(|pos| out_number_of.get(pos).copied())
            .filter(|&n| n != idx + 1)
            .collect();
        let origin = if waves_in[k.input_pos].get("dependsOn").is_some()
            || waves_in[k.input_pos].get("depends_on").is_some()
        {
            "declared"
        } else {
            "undeclared"
        };
        out_waves.push(json!({
            "wave": idx + 1,
            "files": k.files,
            "roles": roles,
            "dependsOn": depends.into_iter().collect::<Vec<_>>(),
            "dependsOnOrigin": origin,
        }));
    }
    // A SEGUNDA saída: a interseção que a dedup acima descartava, pareada só
    // entre ondas do mesmo nível — as que o despacho solta juntas. Sempre
    // presente (vazia quando o plano é disjunto), para o documento manter uma
    // forma só. Sai do censo INTEIRO, uma linha por onda: fundir duas ondas de
    // mesmo `n` numa só faria a colisão entre elas sumir de novo.
    let shared_files = census_collisions(&census);
    let total_waves = out_waves.len();
    Some(json!({
        "waves": out_waves,
        "sharedFiles": shared_files,
        "metadata": {
            "totalWaves": total_waves,
            "totalFiles": total_files,
            "widestWave": widest,
            "source": "input-plan",
        },
    }))
}

/// Dispatch `mustard-rt run wave-dependency [--plan <file>]`.
///
/// `--plan` reads the input JSON from a file — the reliable transport (stdin
/// does not survive the `rtk` wrapper, and a sandboxed/background shell may
/// hand the process a closed stdin; field report 2026-06-12: four wasted calls
/// before the orchestrator gave up). Without the flag, the legacy stdin
/// contract still applies. Both transports accept both input shapes — see
/// [`files_from_value`].
pub fn run(plan: Option<&str>) {
    let raw = match plan {
        Some(path) => match fs::read_to_string(Path::new(path)) {
            Ok(s) => s,
            Err(_) => {
                println!("{}", json!({ "error": "plan-unreadable", "path": path }));
                return;
            }
        },
        None => {
            let mut buf = String::new();
            if std::io::stdin().read_to_string(&mut buf).is_err() {
                println!("{}", json!({ "error": "empty-input" }));
                return;
            }
            buf
        }
    };
    if raw.trim().is_empty() {
        println!("{}", json!({ "error": "empty-input" }));
        return;
    }
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            println!("{}", json!({ "error": "error-fallback" }));
            return;
        }
    };
    let project_root = parsed
        .get("projectRoot")
        .and_then(Value::as_str)
        .map_or_else(
            || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            PathBuf::from,
        );
    let root_abs = if project_root.is_absolute() {
        project_root
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(project_root)
    };
    let root = normalize(&root_abs);

    // Option D — trust an explicit plan's wave boundaries instead of flattening
    // to a file union and re-deriving. The role-layered fallback below would
    // otherwise shred a sensible 2-wave plan into one wave per detected role
    // (field report: a 2-wave plan came back as 11). Bare `{files}` inputs carry
    // no `waves` key and fall through to the import-DAG path.
    if let Some(out) = passthrough_plan_waves(&parsed, &load_role_patterns(&root)) {
        println!("{out}");
        return;
    }

    let files = files_from_value(&parsed);
    if files.is_empty() {
        println!("{}", json!({ "error": "empty-input" }));
        return;
    }
    println!("{}", compute_waves(&files, &root));
}

/// Validate the dependency DAG of a plan's file union, without a separate
/// `wave-dependency` call: read the plan JSON, union its per-wave files (the
/// same [`files_from_value`] the command uses), and run the import-DAG builder
/// to detect a cycle. Returns `{ ok, issues }` — a single WARN `cyclic-dependency`
/// issue (carrying the stuck `cycle` files) when the plan's files import-cycle,
/// else `{ ok: true, issues: [] }`.
///
/// The in-process seam [`crate::commands::pipeline::plan_materialize`] reuses so
/// the cycle check runs as part of materialisation instead of depending on the
/// orchestrator relaying a `wave-dependency` call first (which it may skip).
/// Advisory only: a cycle is a WARN — the planner's explicit wave boundaries
/// still materialise; this names a split the imports say is not executable in
/// order. Fail-open at every step: an unreadable / unparseable / fileless plan
/// makes NO DAG claim (`ok: true`), never an error — a validation seam must not
/// become a new failure mode.
#[must_use]
pub fn validate_plan_dag(plan_path: &Path, project_root: &Path) -> Value {
    let Ok(raw) = fs::read_to_string(plan_path) else {
        return json!({ "ok": true, "issues": [] });
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return json!({ "ok": true, "issues": [] });
    };
    let files = files_from_value(&parsed);
    if files.is_empty() {
        return json!({ "ok": true, "issues": [] });
    }
    let dag = compute_waves(&files, project_root);
    if dag.get("error").and_then(Value::as_str) == Some("cyclic-dependency") {
        let cycle = dag.get("cycle").cloned().unwrap_or_else(|| json!([]));
        return json!({
            "ok": false,
            "issues": [{
                "severity": "WARN",
                "type": "cyclic-dependency",
                "cycle": cycle,
                "message": "the plan's files import-cycle — the declared wave order may not be executable",
            }],
        });
    }
    json!({ "ok": true, "issues": [] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn flat_dag_multi_role_falls_back_to_role_layers() {
        // Net-new files with no imports → flat DAG. Three distinct roles →
        // deterministic role-layered fallback, ordered by the default layer
        // order (schema → api → ui), each wave depending on the previous.
        let dir = tempdir().unwrap();
        let files = vec![
            "src/schema/user.sql".to_string(),
            "src/api/handler.ts".to_string(),
            "src/ui/page.tsx".to_string(),
        ];
        let out = compute_waves(&files, dir.path());
        let waves = out["waves"].as_array().expect("waves array");
        assert_eq!(waves.len(), 3, "flat 3-role net-new must split: {out}");
        assert_eq!(waves[0]["roles"][0].as_str(), Some("schema"));
        assert_eq!(waves[1]["roles"][0].as_str(), Some("api"));
        assert_eq!(waves[2]["roles"][0].as_str(), Some("ui"));
        assert_eq!(waves[0]["dependsOn"].as_array().map(Vec::len), Some(0));
        assert_eq!(waves[1]["dependsOn"][0].as_u64(), Some(1));
        assert_eq!(waves[2]["dependsOn"][0].as_u64(), Some(2));
        // The fallback's chain IS its derivation (each layer builds on the
        // previous) — the edges say where they came from.
        for w in waves {
            assert_eq!(w["dependsOnOrigin"].as_str(), Some("layer-order"), "origin: {out}");
        }
    }

    #[test]
    fn flat_dag_lone_lib_stays_single_wave() {
        // Two net-new files that both fall to the generic "lib" bucket: the
        // lib-folding rule keeps them in one layer — no over-split.
        let dir = tempdir().unwrap();
        let files = vec!["src/util/a.ts".to_string(), "src/util/b.ts".to_string()];
        let out = compute_waves(&files, dir.path());
        assert_eq!(
            out["waves"].as_array().map(Vec::len),
            Some(1),
            "lone-lib net-new must not split: {out}"
        );
    }

    #[test]
    fn rich_plan_waves_are_trusted_not_reinflated() {
        // Option D regression: a planner's explicit 2-wave plan must come back as
        // 2 waves. Before the fix the union was flattened and the flat-DAG role
        // fallback shredded it into one wave per role (field report: 2 → 11).
        let parsed = json!({
            "waves": [
                { "files": ["src/api/x.ts", "src/api/y.ts"] },
                { "files": ["src/ui/z.tsx"] },
            ]
        });
        let out = passthrough_plan_waves(&parsed, &[]).expect("rich plan → Some");
        assert_eq!(
            out["waves"].as_array().map(Vec::len),
            Some(2),
            "explicit 2-wave plan must stay 2: {out}"
        );
        assert_eq!(out["metadata"]["source"].as_str(), Some("input-plan"));
        assert_eq!(out["metadata"]["totalWaves"].as_u64(), Some(2));
        // A plan that declares NO dependencies gets NO edges — the linear
        // `wave N depends on N-1` chain this test once pinned was a
        // fabrication (neither declared by the author nor derived from the
        // import graph), and it contradicted plans whose waves are
        // independent. The origin says so honestly.
        assert_eq!(out["waves"][0]["dependsOn"].as_array().map(Vec::len), Some(0));
        assert_eq!(
            out["waves"][1]["dependsOn"].as_array().map(Vec::len),
            Some(0),
            "no declared edge ⇒ no fabricated chain: {out}"
        );
        assert_eq!(out["waves"][1]["dependsOnOrigin"].as_str(), Some("undeclared"));
    }

    /// The dependency command emits the edges the plan DECLARED — by
    /// number or by wave name, skipping waves the author skipped — and the
    /// import-DAG path emits the REAL derived topology; every edge carries the
    /// origin it came from.
    #[test]
    fn wave_dependency_honours_the_declared_edges() {
        // Declared path: wave 3 depends on wave 1 ONLY (named), wave 2 on 1
        // (numeric). No chain is invented on top of the declaration.
        let parsed = json!({
            "waves": [
                { "files": ["src/schema/m.sql"], "dependsOn": [] },
                { "files": ["src/api/h.ts"], "dependsOn": [1] },
                { "files": ["src/ui/p.tsx"], "depends_on": ["wave-1-schema"] },
            ]
        });
        let out = passthrough_plan_waves(&parsed, &[]).expect("rich plan → Some");
        assert_eq!(out["waves"][0]["dependsOn"], json!([]));
        assert_eq!(out["waves"][1]["dependsOn"], json!([1]));
        assert_eq!(
            out["waves"][2]["dependsOn"],
            json!([1]),
            "the declared edge (wave 1, by name) is honoured — not a 3←2 chain: {out}"
        );
        for w in out["waves"].as_array().expect("waves") {
            assert_eq!(
                w["dependsOnOrigin"].as_str(),
                Some("declared"),
                "every declared edge carries its origin: {out}"
            );
        }

        // Import-DAG path: c imports BOTH a and b, so its wave depends on
        // waves 1 AND 2 — the real topology, which the fabricated index chain
        // ([2] alone) could never express.
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.ts"), "export const a = 1;").unwrap();
        std::fs::write(root.join("b.ts"), "import './a';\nexport const b = 2;").unwrap();
        std::fs::write(root.join("c.ts"), "import './a';\nimport './b';\nexport const c = 3;")
            .unwrap();
        let derived = compute_waves(
            &["a.ts".to_string(), "b.ts".to_string(), "c.ts".to_string()],
            root,
        );
        let waves = derived["waves"].as_array().expect("derived waves");
        assert_eq!(waves.len(), 3, "a ← b ← c is three levels: {derived}");
        assert_eq!(waves[1]["dependsOn"], json!([1]), "b imports a: {derived}");
        assert_eq!(
            waves[2]["dependsOn"],
            json!([1, 2]),
            "c imports a AND b — the real edges, not an index chain: {derived}"
        );
        for w in waves {
            assert_eq!(w["dependsOnOrigin"].as_str(), Some("imports"), "origin: {derived}");
        }
    }

    /// O arquivo declarado por duas ondas sem dependência entre elas é
    /// RELATADO, não descartado em silêncio.
    ///
    /// A dedup continua governando `files` (o motivo dela — não criar nó fantasma
    /// no grafo de imports — segue valendo); o defeito era ela ser a ÚNICA saída.
    /// `sharedFiles` é a segunda, com o encadeamento mínimo já derivado.
    ///
    /// Bilateral: o mesmo par com a aresta declarada não compartilha nível e não
    /// produz entrada nenhuma, então a asserção não pode passar por o campo
    /// listar todo arquivo repetido.
    #[test]
    fn passthrough_reports_the_intersection_it_used_to_drop() {
        let colliding = json!({
            "waves": [
                { "files": ["PayableService.cs", "EffectivationService.cs"] },
                { "files": ["PayableService.cs", "ReversalService.cs"] },
            ]
        });
        let out = passthrough_plan_waves(&colliding, &[]).expect("rich plan → Some");
        assert_eq!(
            out["sharedFiles"],
            json!([{
                "level": 0,
                "waves": [1, 2],
                "files": ["PayableService.cs"],
                "chain": "add wave 1 to wave 2's depends_on",
            }]),
            "the shared file must survive the dedup as its own answer: {out}"
        );
        // A dedup segue de pé na lista de arquivos: o compartilhado sai só na
        // primeira onda, e o grafo continua sem nó duplicado.
        assert_eq!(out["waves"][1]["files"], json!(["ReversalService.cs"]), "{out}");

        // O outro lado — a aresta declarada sequencia as duas, nada colide.
        let chained = json!({
            "waves": [
                { "files": ["PayableService.cs", "EffectivationService.cs"] },
                { "files": ["PayableService.cs", "ReversalService.cs"], "dependsOn": [1] },
            ]
        });
        let out = passthrough_plan_waves(&chained, &[]).expect("rich plan → Some");
        assert_eq!(
            out["sharedFiles"],
            json!([]),
            "waves on different levels are sequenced, never flagged: {out}"
        );
    }

    /// Uma onda cujos arquivos foram TODOS declarados antes é a colisão mais
    /// total que existe — e era a que sumia: o corte olhava o que sobrou da
    /// dedup, então a onda inteira desaparecia do documento.
    #[test]
    fn a_fully_duplicated_wave_is_reported_not_dropped() {
        let parsed = json!({
            "waves": [
                { "files": ["src/shared.rs"] },
                { "files": ["src/shared.rs"] },
            ]
        });
        let out = passthrough_plan_waves(&parsed, &[]).expect("rich plan → Some");
        assert_eq!(out["metadata"]["totalWaves"].as_u64(), Some(2), "{out}");
        assert_eq!(out["waves"][1]["files"], json!([]), "the dedup still stands: {out}");
        assert_eq!(out["sharedFiles"][0]["files"], json!(["src/shared.rs"]), "{out}");
    }

    /// A leitura que `plan-materialize` faz do `plan.json`: numera pelo `n`
    /// DECLARADO (o número que nomeia o diretório) e entende a aresta escrita
    /// como nome de onda.
    #[test]
    fn plan_file_collisions_numbers_by_the_declared_n() {
        let dir = tempdir().unwrap();
        let plan = dir.path().join("plan.json");
        std::fs::write(
            &plan,
            r#"{"waves":[
                {"n":3,"role":"rt","files":["src/shared.rs"]},
                {"n":7,"role":"cli","files":["src/shared.rs","src/other.rs"]}
            ]}"#,
        )
        .unwrap();
        let collisions = plan_file_collisions(&plan);
        assert_eq!(collisions.len(), 1, "{collisions:?}");
        assert_eq!(collisions[0].waves, [3, 7], "the DECLARED numbers: {collisions:?}");
        assert_eq!(collisions[0].chain, "add wave 3 to wave 7's depends_on");

        // A aresta escrita por NOME sequencia o par, exatamente como o número.
        std::fs::write(
            &plan,
            r#"{"waves":[
                {"n":3,"role":"rt","files":["src/shared.rs"]},
                {"n":7,"role":"cli","depends_on":["wave-3-rt"],
                 "files":["src/shared.rs","src/other.rs"]}
            ]}"#,
        )
        .unwrap();
        assert!(plan_file_collisions(&plan).is_empty(), "a named edge sequences the pair");

        // Fail-open na leitura: plano ausente não é colisão nenhuma — quem recusa
        // por plano ilegível é o scaffold.
        assert!(plan_file_collisions(&dir.path().join("nope.json")).is_empty());
    }

    /// As colisões que os DOIS passos veem no MESMO documento: o comando
    /// (`sharedFiles`) e o portão (`plan_file_collisions`), lado a lado.
    ///
    /// Existe para que a asserção não possa passar num só deles — a contradição
    /// entre os dois é o defeito que esta unidade acaba.
    fn both_readings(plan_json: &str) -> (Value, Vec<FileCollision>) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plan.json");
        std::fs::write(&path, plan_json).unwrap();
        let parsed: Value = serde_json::from_str(plan_json).unwrap();
        let out = passthrough_plan_waves(&parsed, &[]).expect("rich plan → Some");
        (out, plan_file_collisions(&path))
    }

    /// Os dois passos leem o mesmo plano e dão a MESMA resposta — a promessa que
    /// abre o `## Contexto` da spec: "rodando os dois sobre o mesmo plano, eles se
    /// contradizem".
    ///
    /// O plano é o do defeito: ondas numeradas 2 e 3, a 3 declarando
    /// `depends_on: ["wave-2-rt"]`. Resolver a referência pela POSIÇÃO de entrada
    /// a transformava em auto-aresta — que some —, e o passo do planejador
    /// acusava colisão entre "ondas 1 e 2", números que aquele plano não tem.
    #[test]
    fn os_dois_passos_concordam_sobre_o_mesmo_plano() {
        let chained = r#"{"waves":[
            {"n":2,"role":"rt","files":["src/shared.rs","src/a.rs"]},
            {"n":3,"role":"cli","depends_on":["wave-2-rt"],
             "files":["src/shared.rs","src/b.rs"]}
        ]}"#;
        let (out, gate) = both_readings(chained);
        assert_eq!(
            out["sharedFiles"],
            json!([]),
            "a aresta declarada sequencia o par — nada colide: {out}"
        );
        assert!(gate.is_empty(), "e o portão lê o mesmo plano do mesmo jeito: {gate:?}");
        // A aresta declarada por NOME sobrevive: ela alcança a onda que se CHAMA
        // 2, não a que está na posição 2 (que é a própria autora da referência).
        assert_eq!(
            out["waves"][1]["dependsOn"],
            json!([1]),
            "a referência alcança a onda de número 2: {out}"
        );

        // O outro lado: sem a aresta, as duas saem juntas e o par é relatado —
        // pelos NÚMEROS DECLARADOS, que são a linha que o operador edita.
        let parallel = chained.replace(r#""depends_on":["wave-2-rt"],"#, "");
        let (out, gate) = both_readings(&parallel);
        assert_eq!(
            out["sharedFiles"],
            json!([{
                "level": 0,
                "waves": [2, 3],
                "files": ["src/shared.rs"],
                "chain": "add wave 2 to wave 3's depends_on",
            }]),
            "os números declarados, não a posição de entrada: {out}"
        );
        assert_eq!(gate.len(), 1, "{gate:?}");
        assert_eq!(gate[0].waves, [2, 3], "e o portão diz o mesmo: {gate:?}");
        assert_eq!(gate[0].chain, out["sharedFiles"][0]["chain"].as_str().unwrap_or_default());
    }

    /// Os dois passos leem o CAMINHO do mesmo jeito.
    ///
    /// Foi a terceira forma do mesmo defeito: primeiro a dedup apagava a colisão,
    /// depois as duas funções numeravam onda por regras diferentes, e por último
    /// elas LIAM O CAMINHO de jeitos diferentes — um `./` numa das ondas bastava
    /// para o portão liberar um plano que a auditoria irmã acusava. Cada conserto
    /// unificou uma camada e deixou a de baixo dividida, então esta asserção
    /// prende as três grafias nos DOIS passos lado a lado, que é o teste que
    /// faltava.
    ///
    /// Bilateral: o par disjunto ao final não emite nada em passo nenhum, logo a
    /// asserção não pode passar por o campo acusar todo plano.
    #[test]
    fn as_tres_grafias_do_mesmo_caminho_dao_a_mesma_resposta() {
        const SPELLINGS: [&str; 3] = ["./src/shared.rs", "src/shared.rs", "src\\shared.rs"];
        for a in SPELLINGS {
            for b in SPELLINGS {
                let plan = serde_json::to_string(&json!({
                    "waves": [
                        { "n": 1, "role": "rt", "files": [a, "src/a.rs"] },
                        { "n": 2, "role": "cli", "files": [b, "src/b.rs"] },
                    ]
                }))
                .unwrap_or_default();
                let (out, gate) = both_readings(&plan);
                assert_eq!(
                    out["sharedFiles"],
                    json!([{
                        "level": 0,
                        "waves": [1, 2],
                        "files": ["src/shared.rs"],
                        "chain": "add wave 1 to wave 2's depends_on",
                    }]),
                    "o comando tem de ler `{a}` e `{b}` como um arquivo só: {out}"
                );
                assert_eq!(gate.len(), 1, "e o portão o mesmo — `{a}` × `{b}`: {gate:?}");
                assert_eq!(gate[0].waves, [1, 2], "{gate:?}");
                assert_eq!(gate[0].files, vec!["src/shared.rs".to_string()], "{gate:?}");
                assert_eq!(
                    gate[0].chain,
                    out["sharedFiles"][0]["chain"].as_str().unwrap_or_default(),
                    "a mesma prescrição nos dois: {gate:?} / {out}"
                );
                // A lista publicada atravessa o MESMO normalizador: a dedup vê um
                // arquivo só, então a onda 2 sai com o que é dela e nada mais.
                assert_eq!(out["waves"][1]["files"], json!(["src/b.rs"]), "{out}");
            }
        }

        // O outro lado: grafias diferentes de arquivos DIFERENTES não colidem.
        let (out, gate) = both_readings(
            r#"{"waves":[
                {"n":1,"role":"rt","files":["./src/a.rs"]},
                {"n":2,"role":"cli","files":["src\\b.rs"]}
            ]}"#,
        );
        assert_eq!(out["sharedFiles"], json!([]), "arquivos distintos não colidem: {out}");
        assert!(gate.is_empty(), "e o portão lê o mesmo: {gate:?}");
    }

    /// Duas ondas de MESMO `n` continuam sendo duas ondas.
    ///
    /// O censo é indexado pela POSIÇÃO, não pelo número: chaveá-lo pelo número
    /// fundia as duas numa só e a colisão entre elas sumia — a mesma perda que a
    /// dedup causava, por outra porta. O despacho cria as duas pastas e solta os
    /// dois agentes, então a colisão é bem real; e como não há como encadear duas
    /// ondas de mesmo número, a prescrição nomeia o passo que falta antes.
    #[test]
    fn duas_ondas_de_mesmo_numero_nao_se_fundem() {
        let (out, gate) = both_readings(
            r#"{"waves":[
                {"n":1,"role":"rt","files":["src/shared.rs"]},
                {"n":1,"role":"cli","files":["src/shared.rs","src/other.rs"]}
            ]}"#,
        );
        assert_eq!(gate.len(), 1, "a colisão não pode sumir na fusão: {gate:?}");
        assert_eq!(gate[0].waves, [1, 1], "{gate:?}");
        assert_eq!(gate[0].files, vec!["src/shared.rs".to_string()], "{gate:?}");
        assert!(
            gate[0].chain.contains("both numbered 1")
                && gate[0].chain.contains("give one of them its own n"),
            "encadear por número é impossível aqui — a prescrição diz o que fazer antes: {}",
            gate[0].chain,
        );
        assert_eq!(
            out["sharedFiles"][0]["waves"],
            json!([1, 1]),
            "e o comando relata a mesma colisão: {out}"
        );
        assert_eq!(out["metadata"]["totalWaves"].as_u64(), Some(2), "{out}");
    }

    #[test]
    fn bare_files_input_is_not_treated_as_a_plan() {
        // The derivation shape ({files} only) has no `waves` key → passthrough
        // declines so the import-DAG path still runs.
        let parsed = json!({ "files": ["a.ts", "b.ts"] });
        assert!(passthrough_plan_waves(&parsed, &[]).is_none());
    }

    #[test]
    fn import_dag_depth_is_not_overridden_by_fallback() {
        // A real import edge gives the DAG depth, so the fallback (guarded on a
        // single flat wave) never fires — the import topology wins.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/schema")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/api")).unwrap();
        std::fs::write(dir.path().join("src/schema/m.ts"), "export const m = 1;").unwrap();
        std::fs::write(dir.path().join("src/api/h.ts"), "import '../schema/m.ts';").unwrap();
        let files = vec!["src/schema/m.ts".to_string(), "src/api/h.ts".to_string()];
        let out = compute_waves(&files, dir.path());
        assert_eq!(
            out["waves"].as_array().map(Vec::len),
            Some(2),
            "import depth preserved: {out}"
        );
    }

    #[test]
    fn extract_imports_handles_es_and_cjs() {
        let src = "import { x } from './a';\nconst y = require('./b');\n";
        let imps = extract_imports(src);
        assert!(imps.contains(&"./a".to_string()));
        assert!(imps.contains(&"./b".to_string()));
    }

    #[test]
    fn topological_waves_orders_dependencies() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.ts"), "export const a = 1;").unwrap();
        std::fs::write(root.join("b.ts"), "import { a } from './a';").unwrap();
        let result = compute_waves(
            &["a.ts".to_string(), "b.ts".to_string()],
            root,
        );
        let waves = result["waves"].as_array().unwrap();
        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0]["files"][0], json!("a.ts"));
        assert_eq!(waves[1]["files"][0], json!("b.ts"));
    }

    #[test]
    fn empty_files_is_error() {
        let dir = tempdir().unwrap();
        assert_eq!(compute_waves(&[], dir.path())["error"], json!("empty-input"));
    }

    #[test]
    fn files_from_value_accepts_derivation_shape() {
        let v = json!({ "files": ["a.ts", "b.ts"], "projectRoot": "." });
        assert_eq!(files_from_value(&v), vec!["a.ts", "b.ts"]);
    }

    /// The documented `< plan.json` form: a plan JSON (`{waves: [{files}]}`)
    /// must yield the union of the per-wave censuses — this was the prose⇄binary
    /// drift that answered `empty-input` to the exact input the refs prescribed
    /// (field report 2026-06-12 + follow-up note in spec
    /// `redesenho-agnostico-indice-termos-digest`).
    #[test]
    fn files_from_value_accepts_plan_json_shape_with_dedup() {
        let v = json!({
            "waves": [
                { "role": "backend", "files": ["src/api/h.ts", "src/shared/t.ts"] },
                { "role": "ui", "files": ["src/ui/p.tsx", "src/shared/t.ts"] },
            ]
        });
        // Union in wave order; the shared file appears once (first occurrence).
        assert_eq!(
            files_from_value(&v),
            vec!["src/api/h.ts", "src/shared/t.ts", "src/ui/p.tsx"],
        );
    }

    #[test]
    fn files_from_value_empty_for_unknown_shape() {
        assert!(files_from_value(&json!({ "foo": 1 })).is_empty());
        assert!(files_from_value(&json!({ "waves": [] })).is_empty());
    }

    /// A plan whose files import each other must surface a WARN `cyclic-dependency`
    /// — the seam `plan-materialize` folds in so the check always runs.
    #[test]
    fn validate_plan_dag_flags_a_cyclic_plan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // a ⇄ b: an import cycle.
        std::fs::write(root.join("a.ts"), "import './b';\nexport const a = 1;").unwrap();
        std::fs::write(root.join("b.ts"), "import './a';\nexport const b = 2;").unwrap();
        let plan = root.join("plan.json");
        std::fs::write(&plan, r#"{"waves":[{"files":["a.ts","b.ts"]}]}"#).unwrap();

        let out = validate_plan_dag(&plan, root);
        assert_eq!(out["ok"], json!(false), "a cyclic plan is not ok: {out}");
        assert_eq!(out["issues"][0]["type"].as_str(), Some("cyclic-dependency"));
        assert_eq!(out["issues"][0]["severity"].as_str(), Some("WARN"));
    }

    /// An acyclic plan (a linear import chain) passes with no issue.
    #[test]
    fn validate_plan_dag_passes_an_acyclic_plan() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.ts"), "export const a = 1;").unwrap();
        std::fs::write(root.join("b.ts"), "import './a';\nexport const b = 2;").unwrap();
        let plan = root.join("plan.json");
        std::fs::write(&plan, r#"{"waves":[{"files":["a.ts","b.ts"]}]}"#).unwrap();

        let out = validate_plan_dag(&plan, root);
        assert_eq!(out["ok"], json!(true), "an acyclic plan is ok: {out}");
        assert!(out["issues"].as_array().is_some_and(|a| a.is_empty()));
    }

    /// A missing / unreadable plan makes NO DAG claim — fail-open, never an error.
    #[test]
    fn validate_plan_dag_fails_open_on_unreadable_plan() {
        let dir = tempdir().unwrap();
        let out = validate_plan_dag(&dir.path().join("nope.json"), dir.path());
        assert_eq!(out["ok"], json!(true), "an unreadable plan makes no claim: {out}");
    }
}
