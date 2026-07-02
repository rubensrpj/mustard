//! `mustard-embed` — local embedding recall for Mustard.
//!
//! Two subcommands, both invoked by the `/scan` orchestrator (never by the hook):
//!
//! - `build  --model <grain.model.json> [--out <vectors>] [--embed-model base]`
//!   Embeds every logic method's BODY (same `slice_body` the enrich reads) with a
//!   local BGE model and writes a compact binary vector sidecar next to the model.
//!   INCREMENTAL: a method whose body is unchanged since the last build (matched
//!   by `(file,name,line)` + a body hash) reuses its stored vector — only new or
//!   changed methods are re-embedded, so a re-scan is near-instant.
//!
//! - `search --intent "<text>" --vectors <file> [--top N]`
//!   Embeds the intent with the SAME model the sidecar was built with, ranks files
//!   by cosine similarity, and prints `{"intent", "files":[{"file","method","line",
//!   "score"}]}` — the per-file best-matching method+line lets the orchestrator
//!   read those candidate spans and pick the one that PERFORMS the action (a cheap
//!   one-read re-rank per miss, no per-method LLM cost). Same `file` key the old
//!   `purpose-search` emitted, so the miss-recovery path is a drop-in swap.
//!
//! Determinism: vectors are fixed once built; search adds no network/LLM. The
//! model is fetched once from HuggingFace into a machine-level cache on first use.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use fastembed::{
    EmbeddingModel, InitOptions, RerankInitOptions, RerankerModel, TextEmbedding, TextRerank,
};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Grain model (only the fields we read)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Model {
    root: Option<String>,
    #[serde(default)]
    modules: Vec<Module>,
}
#[derive(Deserialize)]
struct Module {
    path: String,
    #[serde(default)]
    declarations: Vec<Decl>,
}
#[derive(Deserialize)]
struct Decl {
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    line: usize,
}

/// One indexed method: where it lives, the hash of the body that was embedded
/// (for incremental reuse), and the embedding vector.
#[derive(Clone, PartialEq, Debug)]
struct Record {
    file: String,
    name: String,
    line: u32,
    body_hash: u64,
    vector: Vec<f32>,
}

// ---------------------------------------------------------------------------
// Shared helpers (ported from enrich_purpose / recall_bench so the index is
// apples-to-apples with the rest of the recall pipeline)
// ---------------------------------------------------------------------------

/// ~`cap` lines from `start_line` (1-based), brace-balanced — `slice_body`.
fn slice_body(source: &str, start_line: usize, cap: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = start_line.saturating_sub(1);
    if start >= lines.len() {
        return String::new();
    }
    let mut depth = 0i32;
    let mut end = start;
    let mut found_open = false;
    for (i, line) in lines[start..].iter().enumerate() {
        if i >= cap {
            break;
        }
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    found_open = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        end = start + i;
        if found_open && depth <= 0 {
            break;
        }
    }
    lines[start..=end].join("\n")
}

/// FNV-1a 64-bit — a stable, dependency-free body fingerprint for the
/// incremental reuse decision (change detection, not crypto).
fn body_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const NON_SOURCE_SEGMENTS: &[&str] = &[
    "node_modules", "target", "dist", "build", "bin", "obj", "vendor", ".git",
    "migrations",
];

/// Skip mustard tooling, tests, and build/dep dirs — mirrors `enrich_purpose`.
/// Test detection is ANCHORED (whole dir segments + filename conventions), never
/// a loose substring, so `latest_x.rs` / `binder.rs` are NOT mistaken for tests.
fn is_non_source_path(path: &str) -> bool {
    let s = path.replace('\\', "/");
    if s == ".claude" || s.starts_with(".claude/") || s.contains("/.claude/") {
        return true;
    }
    let segs: Vec<&str> = s.split('/').collect();
    if segs.iter().any(|seg| {
        let l = seg.to_ascii_lowercase();
        l == "test" || l == "tests" || l == "__tests__"
            || NON_SOURCE_SEGMENTS.iter().any(|nss| seg.eq_ignore_ascii_case(nss))
    }) {
        return true;
    }
    let file = segs.last().copied().unwrap_or("").to_ascii_lowercase();
    file.starts_with("test_")
        || file.contains(".test.")
        || file.contains(".spec.")
        || file.contains("_test.")
        || file.contains("_spec.")
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

/// Model `root` wins (Windows `\\?\` stripped); else the model file's dir.
fn workspace_root(model: &Model, model_path: &Path) -> PathBuf {
    if let Some(r) = model.root.as_deref() {
        let r = r.strip_prefix(r"\\?\").unwrap_or(r);
        if !r.is_empty() {
            return PathBuf::from(r);
        }
    }
    model_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
}

fn embed_model(tag: &str) -> EmbeddingModel {
    match tag {
        "small" => EmbeddingModel::BGESmallENV15,
        "large" => EmbeddingModel::BGELargeENV15,
        // Code-specialised: trained on source, embeds raw method BODIES far better
        // than a general TEXT model (the cause of the big-repo recall regression).
        "code" | "jina-code" => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        // Multilingual: handles a pt-BR query against EN code directly (no
        // translation) — relevant for pt-BR projects with mixed-language bodies.
        "e5" | "multi" => EmbeddingModel::MultilingualE5Base,
        "gte" => EmbeddingModel::GTEBaseENV15,
        _ => EmbeddingModel::BGEBaseENV15,
    }
}

/// A STABLE, user-level cache for the downloaded model — so the ~440 MB BGE
/// weights are fetched ONCE per machine, never per working directory. fastembed
/// defaults to `.fastembed_cache` relative to the CWD, which would re-download
/// (and litter a 440 MB folder) into every scanned project; we override it.
fn model_cache_dir() -> PathBuf {
    for var in ["LOCALAPPDATA", "XDG_CACHE_HOME", "HOME", "USERPROFILE"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                let base = PathBuf::from(v);
                return if var == "HOME" || var == "USERPROFILE" {
                    base.join(".cache").join("mustard-embed")
                } else {
                    base.join("mustard-embed").join("models")
                };
            }
        }
    }
    PathBuf::from(".fastembed_cache")
}

/// Load the embedding model, pinned to the machine-level cache dir.
fn init_model(tag: &str) -> anyhow::Result<TextEmbedding> {
    let cache = model_cache_dir();
    let _ = std::fs::create_dir_all(&cache);
    Ok(TextEmbedding::try_new(
        InitOptions::new(embed_model(tag)).with_cache_dir(cache),
    )?)
}

/// Load the cross-encoder reranker (the precision stage). Default is the
/// multilingual jina v2 — it reads (query, code) JOINTLY, so a pt-BR query
/// scores against EN/mixed code directly. `MUSTARD_RERANKER=bge` picks
/// BGE-reranker-v2-m3 instead. Pinned to the same machine-level cache.
fn init_reranker() -> anyhow::Result<TextRerank> {
    let model = match std::env::var("MUSTARD_RERANKER").as_deref() {
        Ok("bge") => RerankerModel::BGERerankerV2M3,
        Ok("jina-en") => RerankerModel::JINARerankerV1TurboEn,
        _ => RerankerModel::JINARerankerV2BaseMultiligual,
    };
    let cache = model_cache_dir();
    let _ = std::fs::create_dir_all(&cache);
    Ok(TextRerank::try_new(
        RerankInitOptions::new(model).with_cache_dir(cache),
    )?)
}

// ---------------------------------------------------------------------------
// Vector sidecar — compact binary. magic "MEV2", model tag, dim, count, then
// per record: file, name, line(u32), body_hash(u64), dim × f32 (all LE).
// ---------------------------------------------------------------------------

const MAGIC: &[u8; 4] = b"MEV2";

fn write_str(f: &mut impl Write, s: &str) -> anyhow::Result<()> {
    let b = s.as_bytes();
    f.write_all(&u16::try_from(b.len())?.to_le_bytes())?;
    f.write_all(b)?;
    Ok(())
}
fn read_str(f: &mut impl Read) -> anyhow::Result<String> {
    let mut len = [0u8; 2];
    f.read_exact(&mut len)?;
    let mut b = vec![0u8; u16::from_le_bytes(len) as usize];
    f.read_exact(&mut b)?;
    Ok(String::from_utf8(b)?)
}

fn write_vectors(path: &Path, model_tag: &str, dim: usize, records: &[Record]) -> anyhow::Result<()> {
    let mut f = BufWriter::new(File::create(path)?);
    f.write_all(MAGIC)?;
    write_str(&mut f, model_tag)?;
    f.write_all(&u32::try_from(dim)?.to_le_bytes())?;
    f.write_all(&u32::try_from(records.len())?.to_le_bytes())?;
    for r in records {
        write_str(&mut f, &r.file)?;
        write_str(&mut f, &r.name)?;
        f.write_all(&r.line.to_le_bytes())?;
        f.write_all(&r.body_hash.to_le_bytes())?;
        for &x in &r.vector {
            f.write_all(&x.to_le_bytes())?;
        }
    }
    f.flush()?;
    Ok(())
}

/// Read a vector sidecar. Returns `None` (not an error) when the file is absent
/// or not a current-format `MEV2` index — the caller then rebuilds from scratch.
fn read_vectors(path: &Path) -> Option<(String, usize, Vec<Record>)> {
    let mut f = BufReader::new(File::open(path).ok()?);
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).ok()?;
    if &magic != MAGIC {
        return None;
    }
    let model_tag = read_str(&mut f).ok()?;
    let mut buf4 = [0u8; 4];
    f.read_exact(&mut buf4).ok()?;
    let dim = u32::from_le_bytes(buf4) as usize;
    f.read_exact(&mut buf4).ok()?;
    let count = u32::from_le_bytes(buf4) as usize;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let file = read_str(&mut f).ok()?;
        let name = read_str(&mut f).ok()?;
        f.read_exact(&mut buf4).ok()?;
        let line = u32::from_le_bytes(buf4);
        let mut buf8 = [0u8; 8];
        f.read_exact(&mut buf8).ok()?;
        let body_hash = u64::from_le_bytes(buf8);
        let mut vector = vec![0f32; dim];
        for v in &mut vector {
            let mut b = [0u8; 4];
            f.read_exact(&mut b).ok()?;
            *v = f32::from_le_bytes(b);
        }
        records.push(Record { file, name, line, body_hash, vector });
    }
    Some((model_tag, dim, records))
}

// ---------------------------------------------------------------------------
// Build progress — a tiny sidecar (`<vectors>.progress`) the `/scan` orchestrator
// and the statusline can poll while the first (slow) build runs. Removed on
// completion so its absence == "no build in flight".
// ---------------------------------------------------------------------------

fn progress_path(out: &Path) -> PathBuf {
    PathBuf::from(format!("{}.progress", out.to_string_lossy()))
}
fn write_progress(out: &Path, done: usize, total: usize) {
    let pct = if total == 0 { 100 } else { done * 100 / total };
    let _ = std::fs::write(
        progress_path(out),
        format!("{{\"phase\":\"embedding\",\"done\":{done},\"total\":{total},\"pct\":{pct}}}"),
    );
}
fn clear_progress(out: &Path) {
    let _ = std::fs::remove_file(progress_path(out));
}

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

fn arg(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}
fn arg_or(flag: &str, default: &str) -> String {
    arg(flag).unwrap_or_else(|| default.to_string())
}

// ---------------------------------------------------------------------------
// build (incremental)
// ---------------------------------------------------------------------------

/// A method to (maybe) embed: its identity + body + the body fingerprint.
struct Candidate {
    file: String,
    name: String,
    line: u32,
    hash: u64,
    body: String,
}

fn build() -> anyhow::Result<()> {
    let model_path = PathBuf::from(arg("--model").ok_or_else(|| anyhow::anyhow!("build: --model <grain.model.json> required"))?);
    let tag = arg_or("--embed-model", "base");
    let out = arg("--out").map(PathBuf::from).unwrap_or_else(|| {
        model_path.parent().unwrap_or_else(|| Path::new(".")).join("grain.vectors")
    });

    let model: Model = serde_json::from_str(&std::fs::read_to_string(&model_path)?)?;
    let root = workspace_root(&model, &model_path);

    // Collect every logic method with a sliceable body + its body fingerprint.
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut cache: HashMap<String, String> = HashMap::new();
    for module in &model.modules {
        if is_non_source_path(&module.path) {
            continue;
        }
        for decl in &module.declarations {
            if (decl.kind != "method" && decl.kind != "function") || decl.name.is_empty() || decl.line == 0 {
                continue;
            }
            let src = cache
                .entry(module.path.clone())
                .or_insert_with(|| std::fs::read_to_string(root.join(&module.path)).unwrap_or_default());
            let body = slice_body(src, decl.line, 55);
            if body.trim().is_empty() {
                continue;
            }
            candidates.push(Candidate {
                file: module.path.clone(),
                name: decl.name.clone(),
                line: decl.line as u32,
                hash: body_hash(&body),
                body,
            });
        }
    }

    // Incremental: reuse a stored vector when (file,name,line) + body hash match
    // the prior index built with the SAME model. Only new/changed bodies embed.
    let prior: HashMap<(String, String, u32), (u64, Vec<f32>)> = match read_vectors(&out) {
        Some((prior_tag, _, recs)) if prior_tag == tag => recs
            .into_iter()
            .map(|r| ((r.file, r.name, r.line), (r.body_hash, r.vector)))
            .collect(),
        _ => HashMap::new(),
    };

    let mut records: Vec<Record> = Vec::with_capacity(candidates.len());
    let mut to_embed: Vec<usize> = Vec::new(); // indices into `candidates`
    for (i, c) in candidates.iter().enumerate() {
        if let Some((h, v)) = prior.get(&(c.file.clone(), c.name.clone(), c.line)) {
            if *h == c.hash {
                records.push(Record { file: c.file.clone(), name: c.name.clone(), line: c.line, body_hash: c.hash, vector: v.clone() });
                continue;
            }
        }
        to_embed.push(i);
    }

    eprintln!(
        "mustard-embed build: {} methods — reuse {}, embed {} (bge-{tag})",
        candidates.len(),
        records.len(),
        to_embed.len()
    );

    let mut dim = prior_dim(&records);
    if !to_embed.is_empty() {
        let mut em = init_model(&tag)?;
        let total = to_embed.len();
        // Embed in CHUNKS so we can emit progress between them — the first build
        // of a large repo is minutes of local CPU and would otherwise be a silent
        // black box. CHUNK matches fastembed's internal micro-batch (64): a larger
        // outer chunk made a heavy model (jina-code) peak ~4.8 GB per call and
        // OOM/stall on a big repo; 64 keeps peak memory at one micro-batch (the
        // same bound the proven single-call path hits) AND gives fine progress.
        const CHUNK: usize = 64;
        let mut done = 0usize;
        write_progress(&out, 0, total); // show 0% immediately
        for chunk in to_embed.chunks(CHUNK) {
            let texts: Vec<String> = chunk.iter().map(|&i| candidates[i].body.clone()).collect();
            let vecs = em.embed(texts, Some(64))?;
            dim = vecs.first().map_or(dim, Vec::len);
            for (&i, v) in chunk.iter().zip(vecs) {
                let c = &candidates[i];
                records.push(Record { file: c.file.clone(), name: c.name.clone(), line: c.line, body_hash: c.hash, vector: v });
            }
            done += chunk.len();
            let pct = done * 100 / total;
            eprintln!("mustard-embed build: embedded {done}/{total} ({pct}%)");
            write_progress(&out, done, total);
        }
    }
    clear_progress(&out); // done — remove the progress sidecar

    write_vectors(&out, &tag, dim, &records)?;
    println!(
        "{}",
        serde_json::json!({ "ok": true, "methods": records.len(), "embedded": to_embed.len(),
            "reused": records.len() - to_embed.len(), "dim": dim, "model": tag, "out": out.to_string_lossy() })
    );
    Ok(())
}

fn prior_dim(records: &[Record]) -> usize {
    records.first().map_or(0, |r| r.vector.len())
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

// --- pipeline: small single-responsibility functions, reused by search + eval -

/// Stage 1 — cosine RETRIEVE: indices into `records`, similarity-desc, top `depth`.
fn retrieve(qv: &[f32], records: &[Record], depth: usize) -> Vec<usize> {
    let mut scored: Vec<(f32, usize)> =
        records.iter().enumerate().map(|(i, r)| (cosine(qv, &r.vector), i)).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(depth).map(|(_, i)| i).collect()
}

/// Read the sliced bodies for `idxs` (root-relative), cached per file.
fn candidate_bodies(records: &[Record], idxs: &[usize], root: &Path) -> Vec<String> {
    let mut cache: HashMap<String, String> = HashMap::new();
    idxs.iter()
        .map(|&i| {
            let r = &records[i];
            let src = cache
                .entry(r.file.clone())
                .or_insert_with(|| std::fs::read_to_string(root.join(&r.file)).unwrap_or_default());
            slice_body(src, r.line as usize, 55)
        })
        .collect()
}

/// Stage 2 — cross-encoder RERANK of `cand` (indices into `records`). Returns the
/// indices reordered by joint (query, body) relevance + score, or `None` on any
/// failure (caller keeps the retrieve order).
fn rerank(rr: &mut TextRerank, intent: &str, records: &[Record], cand: &[usize], root: &Path) -> Option<Vec<(usize, f64)>> {
    let bodies = candidate_bodies(records, cand, root);
    let mut res = rr.rerank(intent.to_string(), bodies, false, None).ok()?;
    res.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    Some(res.into_iter().map(|r| (cand[r.index], f64::from(r.score))).collect())
}

/// Resolve the source root from a `grain.model.json` path (to read candidate
/// bodies for rerank).
fn root_from_model_path(mp: &str) -> Option<PathBuf> {
    let model: Model = serde_json::from_str(&std::fs::read_to_string(mp).ok()?).ok()?;
    Some(workspace_root(&model, Path::new(mp)))
}
fn root_from_model_arg() -> Option<PathBuf> {
    root_from_model_path(&arg("--model")?)
}

/// Collapse ranked `(idx, score)` into DISTINCT files (best-ranked first), top-K,
/// as the `{intent, files:[{file,method,line,score?}]}` shape the orchestrator
/// reads (same as `purpose-search`).
fn format_results(records: &[Record], ranked: &[(usize, f64)], top: usize, intent: &str) -> serde_json::Value {
    let mut seen: Vec<&str> = Vec::new();
    let mut files_out: Vec<serde_json::Value> = Vec::new();
    for &(i, score) in ranked {
        let r = &records[i];
        if seen.iter().any(|s| *s == r.file) {
            continue;
        }
        seen.push(&r.file);
        let mut obj = serde_json::json!({ "file": r.file, "method": r.name, "line": r.line });
        if !score.is_nan() {
            obj["score"] = serde_json::json!((score * 10_000.0).round() / 10_000.0);
        }
        files_out.push(obj);
        if files_out.len() >= top {
            break;
        }
    }
    serde_json::json!({ "intent": intent, "files": files_out })
}

/// Distinct files in rank order (best-ranked method's file first).
fn distinct_files(records: &[Record], ranked: &[(usize, f64)]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for &(i, _) in ranked {
        let f = &records[i].file;
        if !seen.iter().any(|s| s == f) {
            seen.push(f.clone());
        }
    }
    seen
}

/// Separator-/suffix-tolerant path equality (ported from `recall_bench`).
fn path_eq(a: &str, b: &str) -> bool {
    let (a, b) = (norm_path(a), norm_path(b));
    a == b || a.ends_with(&format!("/{b}")) || b.ends_with(&format!("/{a}"))
}
fn norm_path(p: &str) -> String {
    p.trim().replace('\\', "/").trim_start_matches("./").to_string()
}
/// Is any ground-truth file in the first `k` retrieved files?
fn hit_at(files: &[String], truth: &[String], k: usize) -> bool {
    files.iter().take(k).any(|got| truth.iter().any(|w| path_eq(got, w)))
}

#[derive(Deserialize)]
struct Label {
    query: String,
    files: Vec<String>,
}
fn read_labels(path: &Path) -> anyhow::Result<Vec<Label>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//"))
        .filter_map(|l| serde_json::from_str::<Label>(l).ok())
        .collect())
}

// ===========================================================================
// Daemon — load the models ONCE and serve fast over a localhost socket, so the
// per-call cold model-load (~1 s embed, ~20 s reranker) is paid once, not every
// call. `search` auto-starts it and proxies; falls back to cold in-process if it
// can't be reached. Protocol: newline-delimited JSON over TCP (no HTTP / deps).
// This lets the orchestrator use cheap semantic recall pervasively (less LLM).
// ===========================================================================

const DEFAULT_PORT: u16 = 7723;
const DAEMON_IDLE_SECS: u64 = 1800; // self-exit after 30 min idle (free the RAM)

#[derive(serde::Serialize, serde::Deserialize)]
struct SearchReq {
    #[serde(default)]
    op: String,
    #[serde(default)]
    intent: String,
    #[serde(default)]
    vectors: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "def_top")]
    top: usize,
    #[serde(default = "def_depth")]
    depth: usize,
    #[serde(default)]
    rerank: bool,
}
fn def_top() -> usize { 12 }
fn def_depth() -> usize { 50 }

/// A loaded index, tagged with the source file's mtime so a re-scan reloads it.
struct LoadedIndex {
    mtime: u64,
    tag: String,
    dim: usize,
    records: Vec<Record>,
}

/// In-memory model + index caches — loaded once, reused across requests.
#[derive(Default)]
struct Engine {
    embedders: HashMap<String, TextEmbedding>,
    reranker: Option<TextRerank>,
    indexes: HashMap<String, LoadedIndex>,
}
impl Engine {
    fn mtime(p: &Path) -> u64 {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs())
    }
    fn ensure_index(&mut self, path: &str) -> anyhow::Result<()> {
        let mt = Self::mtime(Path::new(path));
        if self.indexes.get(path).map(|ix| ix.mtime) != Some(mt) {
            let (tag, dim, records) = read_vectors(Path::new(path))
                .ok_or_else(|| anyhow::anyhow!("index missing or not current-format: {path}"))?;
            self.indexes.insert(path.to_string(), LoadedIndex { mtime: mt, tag, dim, records });
        }
        Ok(())
    }
    fn search(&mut self, req: &SearchReq) -> anyhow::Result<serde_json::Value> {
        self.ensure_index(&req.vectors)?;
        let tag = self.indexes[&req.vectors].tag.clone();
        if !self.embedders.contains_key(&tag) {
            self.embedders.insert(tag.clone(), init_model(&tag)?);
        }
        let Engine { embedders, reranker, indexes } = self;
        let ix = indexes.get(&req.vectors).expect("index ensured");
        let em = embedders.get_mut(&tag).expect("embedder ensured");
        let qv = em.embed(vec![req.intent.clone()], None)?
            .into_iter().next().ok_or_else(|| anyhow::anyhow!("empty query embedding"))?;
        anyhow::ensure!(qv.len() == ix.dim, "query dim {} != index dim {} (model mismatch)", qv.len(), ix.dim);
        let cand = retrieve(&qv, &ix.records, req.depth);
        let ranked: Vec<(usize, f64)> = if req.rerank {
            match req.model.as_deref().and_then(root_from_model_path) {
                Some(root) => {
                    if reranker.is_none() {
                        *reranker = init_reranker().ok();
                    }
                    match reranker.as_mut() {
                        Some(rr) => rerank(rr, &req.intent, &ix.records, &cand, &root)
                            .unwrap_or_else(|| cand.iter().map(|&i| (i, f64::NAN)).collect()),
                        None => cand.iter().map(|&i| (i, f64::NAN)).collect(),
                    }
                }
                None => cand.iter().map(|&i| (i, f64::NAN)).collect(),
            }
        } else {
            cand.iter().map(|&i| (i, f64::NAN)).collect()
        };
        Ok(format_results(&ix.records, &ranked, req.top, &req.intent))
    }
}

fn serve() -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let port = arg("--port").and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_PORT);
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    eprintln!("mustard-embed serve: 127.0.0.1:{port}");
    // Shared across connection threads: models + indexes load ONCE and serve
    // every session/project (keyed by vectors path). Inference SERIALISES on the
    // Mutex (correct on CPU — concurrent inference would only contend for cores),
    // but connections are accepted concurrently so no session is ever refused.
    let engine = Arc::new(Mutex::new(Engine::default()));
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut idle = 0u64;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                idle = 0;
                let engine = Arc::clone(&engine);
                let shutdown = Arc::clone(&shutdown);
                std::thread::spawn(move || handle_conn(stream, &engine, &shutdown));
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_secs(1));
                idle += 1;
                if idle >= DAEMON_IDLE_SECS {
                    break; // free ~1-2 GB of model RAM after a long idle
                }
            }
            Err(_) => break,
        }
    }
    Ok(())
}

/// Serve ONE connection on its own thread: read a JSON request line, run it
/// (locking the shared engine only for the search itself), write one JSON line.
fn handle_conn(
    mut stream: std::net::TcpStream,
    engine: &std::sync::Mutex<Engine>,
    shutdown: &std::sync::atomic::AtomicBool,
) {
    use std::io::{BufRead, BufReader, Write};
    // The accepted socket INHERITS the listener's non-blocking mode; make it
    // blocking so read_line waits for the request instead of erroring WouldBlock.
    let _ = stream.set_nonblocking(false);
    let Ok(peer) = stream.try_clone() else { return };
    let mut line = String::new();
    if BufReader::new(peer).read_line(&mut line).is_err() {
        return;
    }
    let req: SearchReq = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stream, "{}", serde_json::json!({ "error": format!("bad request: {e}") }));
            return;
        }
    };
    let resp = match req.op.as_str() {
        "ping" => "{\"ok\":true}".to_string(),
        "shutdown" => {
            shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
            "{\"ok\":true}".to_string()
        }
        // Lock held only for the search; a poisoned lock degrades to an error,
        // never a panic (one bad request must not take the daemon down).
        _ => match engine.lock() {
            Ok(mut e) => match e.search(&req) {
                Ok(v) => v.to_string(),
                Err(err) => serde_json::json!({ "error": err.to_string() }).to_string(),
            },
            Err(_) => serde_json::json!({ "error": "engine lock poisoned" }).to_string(),
        },
    };
    let _ = writeln!(stream, "{resp}");
}

/// Send a request to a running daemon; `None` if it can't be reached.
fn try_daemon(port: u16, req_json: &str) -> Option<String> {
    use std::io::{BufRead, BufReader, Write};
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(180))).ok()?;
    writeln!(stream, "{req_json}").ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    let line = line.trim().to_string();
    if line.is_empty() { None } else { Some(line) }
}

/// Spawn the daemon detached (Windows-safe: no inherited handles, no console).
fn spawn_daemon(port: u16) {
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve").arg("--port").arg(port.to_string());
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0800_0000); // DETACHED_PROCESS | CREATE_NO_WINDOW
    }
    let _ = cmd.spawn();
}

fn search() -> anyhow::Result<()> {
    let intent = arg("--intent").ok_or_else(|| anyhow::anyhow!("search: --intent <text> required"))?;
    let vectors = arg("--vectors").ok_or_else(|| anyhow::anyhow!("search: --vectors <file> required"))?;
    let port = arg("--port").and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_PORT);
    let no_daemon = std::env::args().any(|a| a == "--no-daemon");

    let req = SearchReq {
        op: "search".to_string(),
        intent,
        vectors,
        model: arg("--model"),
        top: arg("--top").and_then(|s| s.parse().ok()).unwrap_or_else(def_top),
        depth: arg("--candidates").and_then(|s| s.parse().ok()).unwrap_or_else(def_depth),
        rerank: std::env::args().any(|a| a == "--rerank"),
    };
    let req_json = serde_json::to_string(&req)?;

    // Fast path: a warm daemon. Else auto-start it and poll until it answers.
    if !no_daemon {
        if let Some(resp) = try_daemon(port, &req_json) {
            println!("{resp}");
            return Ok(());
        }
        spawn_daemon(port);
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if let Some(resp) = try_daemon(port, &req_json) {
                println!("{resp}");
                return Ok(());
            }
        }
        eprintln!("mustard-embed: daemon unavailable; cold in-process search");
    }
    // Cold fallback: load in-process, answer once.
    let mut engine = Engine::default();
    println!("{}", engine.search(&req)?);
    Ok(())
}

/// `eval` — the missing measurement: recall@k over a labelled set, separating the
/// RETRIEVE ceiling (target in cosine top-k — what rerank can never exceed) from
/// the FINAL recall (after rerank). Byte-stable JSON.
fn eval() -> anyhow::Result<()> {
    let labels_path = PathBuf::from(arg("--labels").ok_or_else(|| anyhow::anyhow!("eval: --labels <ndjson> required"))?);
    let vectors_path = PathBuf::from(arg("--vectors").ok_or_else(|| anyhow::anyhow!("eval: --vectors <file> required"))?);

    let (model_tag, dim, records) = read_vectors(&vectors_path)
        .ok_or_else(|| anyhow::anyhow!("eval: {} is missing or not a current-format index", vectors_path.display()))?;
    let labels = read_labels(&labels_path)?;
    anyhow::ensure!(!labels.is_empty(), "eval: no labels parsed from {}", labels_path.display());

    let mut em = init_model(&model_tag)?;
    let root = root_from_model_arg();
    let mut reranker = root.as_ref().and_then(|_| init_reranker().ok());

    let (mut r10, mut r50, mut f1, mut f5, mut f10) = (0usize, 0, 0, 0, 0);
    for label in &labels {
        let qv = em.embed(vec![label.query.clone()], None)?
            .into_iter().next().ok_or_else(|| anyhow::anyhow!("empty query embedding"))?;
        anyhow::ensure!(qv.len() == dim, "query dim {} != index dim {dim}", qv.len());
        let cand = retrieve(&qv, &records, 50);
        let ret_files = distinct_files(&records, &cand.iter().map(|&i| (i, 0.0)).collect::<Vec<_>>());
        if hit_at(&ret_files, &label.files, 10) { r10 += 1; }
        if hit_at(&ret_files, &label.files, 50) { r50 += 1; }
        let ranked = match (root.as_ref(), reranker.as_mut()) {
            (Some(rt), Some(rr)) => rerank(rr, &label.query, &records, &cand, rt)
                .unwrap_or_else(|| cand.iter().map(|&i| (i, f64::NAN)).collect()),
            _ => cand.iter().map(|&i| (i, f64::NAN)).collect(),
        };
        let final_files = distinct_files(&records, &ranked);
        if hit_at(&final_files, &label.files, 1) { f1 += 1; }
        if hit_at(&final_files, &label.files, 5) { f5 += 1; }
        if hit_at(&final_files, &label.files, 10) { f10 += 1; }
    }
    let n = labels.len() as f64;
    let pct = |c: usize| (c as f64 / n * 10_000.0).round() / 10_000.0;
    println!("{}", serde_json::json!({
        "n": labels.len(), "model": model_tag, "reranked": reranker.is_some(),
        "retrieveRecall@10": pct(r10), "retrieveRecall@50": pct(r50),
        "finalRecall@1": pct(f1), "finalRecall@5": pct(f5), "finalRecall@10": pct(f10),
    }));
    Ok(())
}

// --- gen-labels: derive a real eval set from source doc-comments --------------

/// Split an identifier (camelCase / PascalCase / snake_case) into lowercase
/// words of length ≥ 3.
fn name_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower && !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            cur.push(c.to_ascii_lowercase());
            prev_lower = c.is_lowercase();
        } else if !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.into_iter().filter(|w| w.len() >= 3).collect()
}

/// True when NO content word of `query` overlaps the identifier — i.e. a genuine
/// recall-HOLE the name index would miss (the only interesting eval case).
fn name_diverges(query: &str, name: &str) -> bool {
    let nw = name_words(name);
    let qw: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4)
        .map(str::to_ascii_lowercase)
        .collect();
    !qw.iter().any(|q| nw.iter().any(|n| q.contains(n.as_str()) || n.contains(q.as_str())))
}

/// Strip XML/`<...>` tags and collapse whitespace.
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract the doc-comment summary ABOVE a declaration (C# `///`), skipping any
/// `[Attribute]` lines between. Returns the first sentence, or `None`. The
/// comment sits above the signature line, so it is NEVER in the embedded body —
/// using it as the query is a non-circular CodeSearchNet-style label.
fn extract_doc(lines: &[String], decl_line: usize) -> Option<String> {
    if decl_line < 2 {
        return None;
    }
    let mut collected: Vec<String> = Vec::new();
    let mut idx = decl_line as i64 - 2; // 0-based index of the line above the signature
    while idx >= 0 {
        let l = lines[idx as usize].trim();
        if let Some(rest) = l.strip_prefix("///") {
            collected.push(rest.to_string());
            idx -= 1;
        } else if l.starts_with('[') && l.ends_with(']') {
            idx -= 1; // C# attribute between doc and signature — keep scanning up
        } else {
            break;
        }
    }
    if collected.is_empty() {
        return None;
    }
    collected.reverse();
    let text = strip_tags(&collected.join(" "));
    let first = text.split(['.', '\n']).next().unwrap_or("").trim().to_string();
    (first.chars().filter(|c| c.is_alphabetic()).count() >= 8).then_some(first)
}

fn gen_labels() -> anyhow::Result<()> {
    let model_path = PathBuf::from(arg("--model").ok_or_else(|| anyhow::anyhow!("gen-labels: --model <grain.model.json> required"))?);
    let out = arg("--out").map(PathBuf::from).unwrap_or_else(|| {
        model_path.parent().unwrap_or_else(|| Path::new(".")).join("grain.labels.ndjson")
    });
    let include_all = std::env::args().any(|a| a == "--all");

    let model: Model = serde_json::from_str(&std::fs::read_to_string(&model_path)?)?;
    let root = workspace_root(&model, &model_path);
    let mut cache: HashMap<String, Vec<String>> = HashMap::new();
    let mut out_lines: Vec<String> = Vec::new();
    for module in &model.modules {
        if is_non_source_path(&module.path) {
            continue;
        }
        for decl in &module.declarations {
            if (decl.kind != "method" && decl.kind != "function") || decl.name.is_empty() || decl.line == 0 {
                continue;
            }
            let lines = cache.entry(module.path.clone()).or_insert_with(|| {
                std::fs::read_to_string(root.join(&module.path)).unwrap_or_default().lines().map(String::from).collect()
            });
            let Some(doc) = extract_doc(lines, decl.line) else {
                continue;
            };
            if !include_all && !name_diverges(&doc, &decl.name) {
                continue;
            }
            out_lines.push(serde_json::json!({ "query": doc, "files": [module.path], "note": decl.name }).to_string());
        }
    }
    std::fs::write(&out, format!("{}\n", out_lines.join("\n")))?;
    println!("{}", serde_json::json!({ "ok": true, "labels": out_lines.len(), "out": out.to_string_lossy(), "divergentOnly": !include_all }));
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("build") => build(),
        Some("search") => search(),
        Some("serve") => serve(),
        Some("eval") => eval(),
        Some("gen-labels") => gen_labels(),
        _ => {
            eprintln!("usage: mustard-embed <build|search|serve|eval|gen-labels> [flags]\n  build      --model <grain.model.json> [--out <vectors>] [--embed-model small|base|large|code]\n  search     --intent \"<text>\" --vectors <file> [--model <grain.model.json>] [--rerank] [--top N] [--no-daemon] [--port N]\n  serve      [--port N]   (daemon: load models once, serve over 127.0.0.1)\n  eval       --labels <ndjson> --vectors <file> [--model <grain.model.json>]\n  gen-labels --model <grain.model.json> [--out <ndjson>] [--all]");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_body_balances_braces() {
        let src = "fn a() {\n    if x {\n        y();\n    }\n}\nfn b() {}\n";
        let body = slice_body(src, 1, 55);
        assert!(body.contains("fn a()") && body.contains("y();"));
        assert!(!body.contains("fn b()"), "stops at the matching close brace");
    }

    #[test]
    fn body_hash_is_stable_and_change_sensitive() {
        assert_eq!(body_hash("abc"), body_hash("abc"), "same input → same hash");
        assert_ne!(body_hash("abc"), body_hash("abd"), "one byte change → different hash");
    }

    #[test]
    fn cosine_is_bounded_and_zero_safe() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0, "zero vector never yields NaN");
    }

    #[test]
    fn non_source_filter_is_anchored() {
        assert!(is_non_source_path(".claude/x.ts"));
        assert!(is_non_source_path("a/.claude/y.ts"));
        assert!(is_non_source_path("src/__tests__/a.ts"));
        assert!(is_non_source_path("pkg/tests/b.rs"));
        assert!(is_non_source_path("node_modules/dep/i.js"));
        assert!(is_non_source_path("app/Migrations/Init.cs"));
        assert!(is_non_source_path("order/test_actions.py"));
        assert!(is_non_source_path("order/actions_test.go"));
        assert!(is_non_source_path("ui/Button.test.tsx"));
        assert!(!is_non_source_path("src/payment.rs"));
        assert!(!is_non_source_path("src/latest_state.rs"));
        assert!(!is_non_source_path("src/binder.rs"));
    }

    #[test]
    fn vectors_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.vectors");
        let records = vec![
            Record { file: "a/b.rs".into(), name: "Foo".into(), line: 12, body_hash: 7, vector: vec![0.1, 0.2, 0.3] },
            Record { file: "c/d.rs".into(), name: "Bar".into(), line: 40, body_hash: 9, vector: vec![-1.0, 0.5, 2.0] },
        ];
        write_vectors(&path, "base", 3, &records).unwrap();
        let (tag, dim, got) = read_vectors(&path).unwrap();
        assert_eq!((tag.as_str(), dim), ("base", 3));
        assert_eq!(got, records, "write -> read is lossless (file/name/line/hash/vector)");
    }

    #[test]
    fn read_vectors_rejects_foreign_or_absent_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_vectors(&dir.path().join("nope.vectors")).is_none(), "absent → None");
        let bad = dir.path().join("bad.vectors");
        std::fs::write(&bad, b"NOPE not a vectors file").unwrap();
        assert!(read_vectors(&bad).is_none(), "wrong magic → None (rebuild from scratch)");
    }
}
