//! `mustard-rt run enrich-purpose` — T2 (render) + T3 (apply).
//!
//! **Render** (`--render`): reads the grain model, filters `method`/`function`
//! declarations, and emits a byte-stable JSON WORKLIST of only the declarations
//! that NEED enrichment — `purpose` absent OR the stored `body_hash` no longer
//! matches the hash of the current sliced body (incremental: a re-scan only
//! re-emits changed methods). Shape: `{"lang":"en",
//! "items":[{"id":"path#name#line","body":"<sliced body>"}]}`, items sorted by
//! id. Purpose summaries are ALWAYS English (the machine artifact is
//! English-only, regardless of the project's configured language). The
//! orchestrator chunks `items` for parallel dispatch and builds the per-chunk
//! summarization prompt itself (the English instruction lives in the SKILL); the
//! binary only supplies the worklist + bodies.
//!
//! **Apply** (`--apply <file>`): reads a JSON array
//! `[{"id":"<module_path>#<name>#<line>","purpose":"..."}]` produced by the LLM,
//! finds each declaration in the model at `modules[].declarations[]`, computes a
//! SHA-256 of the current body, and writes `purpose` + `body_hash` back
//! atomically — skipping unchanged bodies (incremental).
//!
//! No LLM/network calls in this binary — pure data in/out. The grain model
//! uses `modules[].path` as the module identifier; declarations carry `kind`,
//! `name`, `line` (+ the new additive `purpose`/`body_hash` fields from T2/T3).

use std::collections::BTreeMap;
use std::path::Path;

use crate::util::sha256::Sha256;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract ~`cap` lines starting at `start_line` (1-based), following braces.
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
                '}' => {
                    depth -= 1;
                }
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

/// SHA-256 hex digest of a sliced body — the incremental identity shared by
/// render (decide stale) and apply (decide skip), so the two agree byte-for-byte
/// on what "unchanged" means.
fn body_hash(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    hasher.hex_digest()
}

// ---------------------------------------------------------------------------
// T2 — render
// ---------------------------------------------------------------------------

/// Resolve the project root for source-file lookups. The scan records the
/// authoritative root in the model's `root` field; the `module_path`s are
/// relative to it. The Windows `\\?\` extended-length prefix is stripped so the
/// forward-slash module paths join cleanly. Falls back to the model file's
/// parent dir (correct when model and sources are co-located, e.g. in tests).
fn workspace_root_from_model(model: &serde_json::Value, model_path: &Path) -> std::path::PathBuf {
    if let Some(r) = model.get("root").and_then(|v| v.as_str()) {
        let r = r.strip_prefix(r"\\?\").unwrap_or(r);
        if !r.is_empty() {
            return std::path::PathBuf::from(r);
        }
    }
    model_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
}

/// Build/dependency directory segments that never hold first-party SOURCE — a
/// declaration mined out of one is tooling/generated/vendored, not a business
/// method to enrich. Structural and AGNOSTIC: these are universal directory
/// conventions, not language/framework names (the scan's ethos). Matched as a
/// whole path SEGMENT (between slashes), so `src/binder.rs` is never mistaken
/// for the `bin` dir nor `rebuild/x.rs` for `build`.
const NON_SOURCE_SEGMENTS: &[&str] = &[
    "node_modules", "target", "dist", "build", "bin", "obj", "vendor", ".git",
    "migrations",
];

/// `true` when `path` is NOT first-party source code — so the worklist skips it.
/// Three agnostic, structural rules (no language/framework names):
/// 1. a `.claude/` segment — mustard's own tooling living inside the scanned repo;
/// 2. [`mustard_core::domain::ast::is_test_path`] — the canonical agnostic test
///    detector (dir-segment + filename convention, polyglot), reused so the
///    enrich set matches every other consumer (digest anchors, samples);
/// 3. a build/dependency dir segment ([`NON_SOURCE_SEGMENTS`]), matched on whole
///    path segments so a real source file is never caught by substring.
///
/// Backslashes are normalised to `/` first (Windows paths), and the `.claude`
/// check accepts it as the leading segment too (`path.starts_with(".claude/")`).
fn is_non_source_path(path: &str) -> bool {
    let slashed = path.replace('\\', "/");
    // 1. mustard's own tooling — a `.claude` segment anywhere (or leading).
    if slashed == ".claude"
        || slashed.starts_with(".claude/")
        || slashed.contains("/.claude/")
    {
        return true;
    }
    // 2. canonical agnostic test detector (reused, never reinvented).
    if mustard_core::domain::ast::is_test_path(&slashed) {
        return true;
    }
    // 3. a build/dependency dir as a whole path segment.
    slashed.split('/').any(|seg| {
        NON_SOURCE_SEGMENTS.iter().any(|nss| seg.eq_ignore_ascii_case(nss))
    })
}

/// One declaration the worklist might enrich: where its body lives + the stored
/// incremental identity (`purpose` present? stored `body_hash`).
struct Candidate {
    module_path: String,
    line: usize,
    has_purpose: bool,
    stored_hash: String,
}

pub fn run_render(model_path: &Path, root: &Path) {
    println!("{}", worklist_json(model_path, root));
}

/// Build the byte-stable worklist JSON for `model_path`. The `lang` field is
/// ALWAYS `"en"` — purpose summaries are English-only machine artifacts,
/// independent of the project's configured language. Pure given the model file:
/// same input → identical string. `run_render` prints this; tests assert on it
/// directly. `_root` is retained for call-site stability (no longer read).
fn worklist_json(model_path: &Path, _root: &Path) -> String {
    let lang = "en";

    // Fail-open: a missing or unparseable model → empty worklist (NOT silence),
    // so a standard /scan step always emits a well-formed `{lang, items:[]}`.
    let raw = std::fs::read_to_string(model_path).unwrap_or_default();
    let model: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);

    // Source files are relative to the project root the scan recorded.
    let workspace_root = workspace_root_from_model(&model, model_path);

    // Collect every method/function declaration with its stored incremental
    // identity. id = "<module_path>#<name>#<line>" — keyed in a BTreeMap so the
    // worklist is sorted by id (byte-stable).
    let mut candidates: BTreeMap<String, Candidate> = BTreeMap::new();
    if let Some(modules) = model.get("modules").and_then(|v| v.as_array()) {
        for module in modules {
            let module_path = module.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if module_path.is_empty() {
                continue;
            }
            // Only REAL source code is enriched: skip mustard's own `.claude/`
            // tooling, tests, and build/dependency dirs (structural + agnostic).
            if is_non_source_path(module_path) {
                continue;
            }
            if let Some(decls) = module.get("declarations").and_then(|v| v.as_array()) {
                for decl in decls {
                    let kind = decl.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                    if kind != "method" && kind != "function" {
                        continue;
                    }
                    let name = decl.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let line = decl.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    if name.is_empty() {
                        continue;
                    }
                    let id = format!("{}#{}#{}", module_path, name, line);
                    candidates.insert(
                        id,
                        Candidate {
                            module_path: module_path.to_string(),
                            line,
                            has_purpose: decl.get("purpose").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()),
                            stored_hash: decl.get("body_hash").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        },
                    );
                }
            }
        }
    }

    // Incremental filter: keep only declarations that NEED enrichment — no
    // `purpose` yet, OR a body that changed since it was last enriched (its
    // current sliced-body hash differs from the stored one). A declaration whose
    // body is unreadable or empty is skipped (fail-open). Same slice + hash as
    // `run_apply`, so render and apply agree on "unchanged".
    let mut items: Vec<serde_json::Value> = Vec::new();
    for (id, cand) in &candidates {
        let src_path = workspace_root.join(&cand.module_path);
        let source = match std::fs::read_to_string(&src_path) {
            Ok(s) => s,
            Err(_) => continue, // fail-open: skip unreadable files
        };
        let snippet = slice_body(&source, cand.line, 55);
        if snippet.is_empty() {
            continue;
        }
        // Stale = never enriched, or the body changed since the stored hash.
        let stale = !cand.has_purpose || body_hash(&snippet) != cand.stored_hash;
        if !stale {
            continue;
        }
        items.push(serde_json::json!({ "id": id, "body": snippet }));
    }

    // Byte-stable worklist: items are already in BTreeMap (id-sorted) order.
    let worklist = serde_json::json!({ "lang": lang, "items": items });
    serde_json::to_string_pretty(&worklist).unwrap_or_else(|_| "{}".into())
}

// ---------------------------------------------------------------------------
// T3 — apply
// ---------------------------------------------------------------------------

pub fn run_apply(apply_path: &Path, model_path: &Path) {
    // Read the apply file.
    let apply_raw = match std::fs::read_to_string(apply_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("enrich-purpose apply: cannot read {}: {e}", apply_path.display());
            return;
        }
    };
    let entries: Vec<serde_json::Value> = match serde_json::from_str(&apply_raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("enrich-purpose apply: bad JSON in {}: {e}", apply_path.display());
            return;
        }
    };

    // Read the current model.
    let model_raw = match std::fs::read_to_string(model_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("enrich-purpose apply: cannot read model {}: {e}", model_path.display());
            return;
        }
    };
    let mut model: serde_json::Value = match serde_json::from_str(&model_raw) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("enrich-purpose apply: cannot parse model {}: {e}", model_path.display());
            return;
        }
    };

    // Source files are relative to the project root the scan recorded.
    let workspace_root = workspace_root_from_model(&model, model_path);

    // Apply each entry.
    for entry in &entries {
        let id = match entry.get("id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let purpose = match entry.get("purpose").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };

        // id = "<module_path>#<name>#<line>"
        let parts: Vec<&str> = id.splitn(3, '#').collect();
        if parts.len() < 3 {
            continue;
        }
        let (module_path, name, line_str) = (parts[0], parts[1], parts[2]);
        let line: usize = match line_str.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Read source to compute current body hash.
        let src_path = workspace_root.join(module_path);
        let source = match std::fs::read_to_string(&src_path) {
            Ok(s) => s,
            Err(_) => continue, // fail-open
        };
        let body = slice_body(&source, line, 55);
        let current_hash = body_hash(&body);

        // Find and update the matching declaration in model.modules[].declarations[].
        let modules = match model.get_mut("modules").and_then(|v| v.as_array_mut()) {
            Some(arr) => arr,
            None => continue,
        };
        let mut found = false;
        'outer: for module in modules.iter_mut() {
            let m_path = module.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if m_path != module_path {
                continue;
            }
            let decls = match module.get_mut("declarations").and_then(|v| v.as_array_mut()) {
                Some(arr) => arr,
                None => continue,
            };
            for decl in decls.iter_mut() {
                let d_kind = decl.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                if d_kind != "method" && d_kind != "function" {
                    continue;
                }
                let d_name = decl.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let d_line = decl.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                if d_name != name || d_line != line {
                    continue;
                }
                // Incremental: skip if body_hash matches current.
                let stored_hash = decl.get("body_hash").and_then(|v| v.as_str()).unwrap_or("");
                if stored_hash == current_hash {
                    found = true;
                    break 'outer;
                }
                // Update purpose and body_hash.
                if let Some(obj) = decl.as_object_mut() {
                    obj.insert("purpose".to_string(), serde_json::Value::String(purpose.to_string()));
                    obj.insert("body_hash".to_string(), serde_json::Value::String(current_hash.clone()));
                }
                found = true;
                break 'outer;
            }
        }
        if !found {
            eprintln!("enrich-purpose apply: id not found in model: {id}");
        }
    }

    // Serialize and write atomically.
    let out = match serde_json::to_string_pretty(&model) {
        Ok(s) => s + "\n",
        Err(e) => {
            eprintln!("enrich-purpose apply: cannot serialize model: {e}");
            return;
        }
    };
    if let Err(e) = mustard_core::io::fs::write_atomic(model_path, out.as_bytes()) {
        eprintln!("enrich-purpose apply: cannot write model {}: {e}", model_path.display());
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn run(render: bool, apply: Option<&Path>, model_path: &Path, root: &Path) {
    if let Some(apply_path) = apply {
        run_apply(apply_path, model_path);
    } else if render {
        run_render(model_path, root);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn enrich_purpose_render() {
        let dir = tempdir().unwrap();
        // Write a source file relative to the tempdir (workspace root).
        let src_path = dir.path().join("src").join("payments.rs");
        fs::create_dir_all(src_path.parent().unwrap()).unwrap();
        fs::write(
            &src_path,
            "fn process_payment(amount: f64) {\n    // stub\n}\n\nstruct PaymentRecord {}\n",
        )
        .unwrap();

        let model = serde_json::json!({
            "root": dir.path().to_str().unwrap(),
            "modules": [
                {
                    "path": "src/payments.rs",
                    "language": "rust",
                    "loc": 5,
                    "imports": [],
                    "namespaces": [],
                    "declarations": [
                        { "kind": "function", "name": "process_payment", "line": 1 },
                        { "kind": "struct",   "name": "PaymentRecord",   "line": 5 }
                    ]
                }
            ]
        });
        let model_path = dir.path().join("model.json");
        fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

        // (a) BYTE-STABLE: two renders are identical strings.
        let json_a = worklist_json(&model_path, dir.path());
        let json_b = worklist_json(&model_path, dir.path());
        assert_eq!(json_a, json_b, "two renders must be byte-identical");
        run(true, None, &model_path, dir.path()); // the run face must not panic
        let parsed: serde_json::Value = serde_json::from_str(&json_a).expect("worklist is valid JSON");

        // Worklist carries ONLY logic decls that need enrichment — the function
        // (no purpose yet) is in; the struct is filtered (not method/function).
        let items = parsed["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1, "only the un-enriched function is a worklist item: {parsed}");
        assert_eq!(items[0]["id"], "src/payments.rs#process_payment#1");
        assert!(
            items[0]["body"].as_str().unwrap().contains("process_payment"),
            "body is the sliced function body: {}",
            items[0]["body"]
        );
        assert!(!parsed["items"].to_string().contains("PaymentRecord"), "struct is excluded");
    }

    #[test]
    fn enrich_purpose_render_incremental_skips_unchanged() {
        let dir = tempdir().unwrap();
        let src_path = dir.path().join("lib.rs");
        fs::write(
            &src_path,
            "fn enriched() {\n    // a\n}\n\nfn stale() {\n    // b\n}\n",
        )
        .unwrap();

        // Compute the stored hash for the ALREADY-enriched decl exactly as render
        // will (slice at its line, hash). `enriched` starts at line 1.
        let src = fs::read_to_string(&src_path).unwrap();
        let enriched_hash = body_hash(&slice_body(&src, 1, 55));

        let model = serde_json::json!({
            "root": dir.path().to_str().unwrap(),
            "modules": [
                {
                    "path": "lib.rs",
                    "language": "rust",
                    "loc": 6,
                    "imports": [],
                    "namespaces": [],
                    "declarations": [
                        // Already enriched AND body_hash matches current body → SKIP.
                        { "kind": "function", "name": "enriched", "line": 1,
                          "purpose": "Does the enriched thing.", "body_hash": enriched_hash },
                        // Never enriched (no purpose) → INCLUDE. `stale` is at line 5.
                        { "kind": "function", "name": "stale", "line": 5 }
                    ]
                }
            ]
        });
        let model_path = dir.path().join("model.json");
        fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

        let json = worklist_json(&model_path, dir.path());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let items = parsed["items"].as_array().expect("items");
        let ids: Vec<&str> = items.iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["lib.rs#stale#5"], "only the stale decl is in the worklist: {ids:?}");
        // The already-enriched-and-unchanged decl is excluded.
        assert!(!json.contains("enriched"), "enriched+matching-hash decl is skipped: {json}");
    }

    #[test]
    fn enrich_purpose_render_skips_non_source_paths() {
        let dir = tempdir().unwrap();
        let body = "fn f() {\n    // x\n}\n";
        // One .claude/ tooling file, one test-path file, one node_modules file,
        // and one REAL source file — all with a readable, sliceable body, so the
        // PATH filter (not the unreadable-skip) is what decides.
        for rel in [
            ".claude/skills/x/gen.py",
            "src/__tests__/payment_test.rs",
            "node_modules/dep/index.js",
            "src/payment.rs",
        ] {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, body).unwrap();
        }

        let decl = |name: &str| serde_json::json!({ "kind": "function", "name": name, "line": 1 });
        let module = |path: &str, name: &str| serde_json::json!({
            "path": path, "language": "rust", "loc": 3, "imports": [], "namespaces": [],
            "declarations": [decl(name)]
        });
        let model = serde_json::json!({
            "root": dir.path().to_str().unwrap(),
            "modules": [
                module(".claude/skills/x/gen.py", "gen"),
                module("src/__tests__/payment_test.rs", "test_pay"),
                module("node_modules/dep/index.js", "dep_fn"),
                module("src/payment.rs", "real_fn"),
            ]
        });
        let model_path = dir.path().join("model.json");
        fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

        let json = worklist_json(&model_path, dir.path());
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let ids: Vec<&str> = parsed["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["src/payment.rs#real_fn#1"], "only REAL source is enriched: {ids:?}");
        assert!(!json.contains(".claude/"), ".claude tooling excluded: {json}");
        assert!(!json.contains("node_modules"), "build/dep dir excluded: {json}");
        assert!(!json.contains("__tests__"), "test path excluded: {json}");
    }

    /// The path filter is structural + agnostic: whole-segment build/dep dirs,
    /// `.claude/` tooling, and the canonical test detector — but a real source
    /// file whose name merely CONTAINS a build word (`binder.rs`, `rebuild/`) is
    /// never caught (substring would be wrong).
    #[test]
    fn is_non_source_path_is_structural() {
        // Excluded.
        assert!(is_non_source_path(".claude/skills/x/gen.py"));
        assert!(is_non_source_path("apps/web/.claude/foo.ts"));
        assert!(is_non_source_path("node_modules/dep/index.js"));
        assert!(is_non_source_path("app/target/debug/build.rs"));
        assert!(is_non_source_path("server/dist/main.js"));
        assert!(is_non_source_path("pkg/vendor/lib.go"));
        assert!(is_non_source_path("db/migrations/0001_init.sql"));
        assert!(is_non_source_path("db/Migrations/Init.cs")); // case-insensitive
        assert!(is_non_source_path("src/components/__tests__/Button.test.tsx"));
        // Kept (real source whose NAME contains a build word as a substring).
        assert!(!is_non_source_path("src/binder.rs"), "binder != bin segment");
        assert!(!is_non_source_path("src/rebuild/engine.rs"), "rebuild != build segment");
        assert!(!is_non_source_path("src/distance.rs"), "distance != dist segment");
        assert!(!is_non_source_path("src/payment.rs"));
    }

    #[test]
    fn enrich_purpose_apply_incremental() {
        let dir = tempdir().unwrap();
        // Source file in the workspace root (same dir as model).
        let src_path = dir.path().join("lib.rs");
        fs::write(&src_path, "fn activate_payment() {\n    // pay\n}\n").unwrap();

        let model = serde_json::json!({
            "root": dir.path().to_str().unwrap(),
            "modules": [
                {
                    "path": "lib.rs",
                    "language": "rust",
                    "loc": 3,
                    "imports": [],
                    "namespaces": [],
                    "declarations": [
                        { "kind": "function", "name": "activate_payment", "line": 1 }
                    ]
                }
            ]
        });
        let model_path = dir.path().join("model.json");
        fs::write(&model_path, serde_json::to_string_pretty(&model).unwrap()).unwrap();

        // First apply: sets purpose + body_hash.
        let apply_data = serde_json::json!([
            { "id": "lib.rs#activate_payment#1", "purpose": "Activates a payment transaction." }
        ]);
        let apply_path = dir.path().join("apply.json");
        fs::write(&apply_path, serde_json::to_string(&apply_data).unwrap()).unwrap();

        run(false, Some(&apply_path), &model_path, dir.path());

        let updated: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&model_path).unwrap()).unwrap();
        let decl = &updated["modules"][0]["declarations"][0];
        assert_eq!(decl["purpose"].as_str().unwrap(), "Activates a payment transaction.");
        let hash1 = decl["body_hash"].as_str().unwrap().to_string();
        assert!(!hash1.is_empty(), "body_hash must be set");

        // Second apply (same body, different purpose text): should be a no-op.
        let apply_data2 = serde_json::json!([
            { "id": "lib.rs#activate_payment#1", "purpose": "DIFFERENT PURPOSE" }
        ]);
        fs::write(&apply_path, serde_json::to_string(&apply_data2).unwrap()).unwrap();
        run(false, Some(&apply_path), &model_path, dir.path());

        let updated2: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&model_path).unwrap()).unwrap();
        let decl2 = &updated2["modules"][0]["declarations"][0];
        // Since body didn't change, hash matches → incremental skip → purpose unchanged.
        assert_eq!(
            decl2["purpose"].as_str().unwrap(),
            "Activates a payment transaction.",
            "incremental: unchanged body must not overwrite purpose"
        );
    }

    /// The worklist's `lang` field is ALWAYS `"en"` — purpose summaries are
    /// English-only machine artifacts. A pt-BR config does NOT change it (the
    /// reverted "generated artifacts follow config-lang" rule for machine
    /// artifacts). The SKILL builds the English prompt instruction.
    #[test]
    fn enrich_purpose_lang_is_always_english() {
        // A minimal model with one un-enriched function (so `items` is non-empty
        // and `lang` is exercised on a real render).
        let model = serde_json::json!({
            "root": "",
            "modules": [{
                "path": "lib.rs", "language": "rust", "loc": 3,
                "imports": [], "namespaces": [],
                "declarations": [{ "kind": "function", "name": "act", "line": 1 }]
            }]
        });

        // EN: no mustard.json.
        let en_dir = tempdir().unwrap();
        fs::write(en_dir.path().join("lib.rs"), "fn act() {\n    // x\n}\n").unwrap();
        let en_model = en_dir.path().join("model.json");
        let mut m = model.clone();
        m["root"] = serde_json::Value::String(en_dir.path().to_str().unwrap().to_string());
        fs::write(&en_model, serde_json::to_string_pretty(&m).unwrap()).unwrap();
        let en: serde_json::Value = serde_json::from_str(&worklist_json(&en_model, en_dir.path())).unwrap();
        assert_eq!(en["lang"], "en", "lang is en: {en}");

        // pt-BR config STILL yields "en" — the machine artifact is English-only.
        let pt_dir = tempdir().unwrap();
        fs::write(pt_dir.path().join("mustard.json"), r#"{"lang":"pt-BR"}"#).unwrap();
        fs::write(pt_dir.path().join("lib.rs"), "fn act() {\n    // x\n}\n").unwrap();
        let pt_model = pt_dir.path().join("model.json");
        let mut m2 = model.clone();
        m2["root"] = serde_json::Value::String(pt_dir.path().to_str().unwrap().to_string());
        fs::write(&pt_model, serde_json::to_string_pretty(&m2).unwrap()).unwrap();
        let pt: serde_json::Value = serde_json::from_str(&worklist_json(&pt_model, pt_dir.path())).unwrap();
        assert_eq!(pt["lang"], "en", "pt-BR config still yields English: {pt}");
    }
}
