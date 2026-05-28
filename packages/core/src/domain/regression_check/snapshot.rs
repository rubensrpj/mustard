//! Snapshot capture — the *before* and *after* photographs.
//!
//! [`capture_for_spec`] is the only public entry point. It:
//!
//! 1. Parses `## Funções tocadas` via [`crate::domain::spec::touched_functions::parse`]
//!    (W0). Specs without that section resolve to an empty snapshot — the
//!    gate's job is to flag drift in declared functions, so a wave that did
//!    not declare any is by definition clean here.
//! 2. For each declared function, resolves the source file from the
//!    qualifier's path hint. Three shapes are supported (see
//!    [`crate::domain::spec::touched_functions::Qualifier`]):
//!    - `Module(crate::a::run)` — path hint on the enclosing subsection
//!      header points to a directory or file; we scan it for a function
//!      whose final identifier matches.
//!    - `PathHint(foo/bar.rs::run)` — path is split on `::` and the file is
//!      opened directly.
//!    - `Pure(name)` — same as Module, falls back to scanning the path hint.
//! 3. Resolves a language id via [`crate::domain::ast::GrammarLoader::language_id_for_path`]
//!    (W1.5). When the loader returns `Some`, the AST path is attempted:
//!    [`crate::domain::ast::TreeSitterParser::for_language`] +
//!    [`crate::domain::ast::extract_function_signatures`] locate the function and
//!    the surrounding `function_item` (or equivalent) node text is captured.
//! 4. On any `Err(AstError::GrammarNotInstalled)`, falls back to the
//!    textual capture path: the same fallback regex used by `signature.rs`
//!    locates the signature line; brace-balancing or indentation rules
//!    capture the body. The mode is tagged [`CaptureMode::Textual`] and a
//!    `warning` is appended to the returned [`CaptureReport`] so the gate
//!    can emit telemetry — fail-open in runtime, not in design.
//! 5. A declared function whose source file cannot be opened, or whose
//!    signature does not appear in the file, captures with an empty body
//!    and mode [`CaptureMode::Textual`]. The downstream diff then reports
//!    the function as `Added` (when its peer in the other snapshot has
//!    content) or `Unchanged` (when both peers are empty) — never panics.

use super::{CaptureMode, FunctionCapture, Snapshot, TextSpan};
use crate::domain::ast::{AstError, GrammarLoader, TreeSitterParser, extract_function_signatures};
use crate::domain::spec::touched_functions::{self, Qualifier, TouchedFunction, TouchedFunctions};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Typed error surface for the snapshot capture path.
///
/// `#[non_exhaustive]` so later waves can add variants without breaking
/// downstream `match` arms.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RegressionError {
    /// `## Funções tocadas` section parsed but no declared functions —
    /// distinct from "section absent". Callers may demote this to a
    /// warning; the W4 gate currently treats it as a clean snapshot.
    #[error("spec declared no touched functions")]
    NoDeclaredFunctions,

    /// Filesystem error while reading a source file. Wraps the underlying
    /// [`std::io::Error`] verbatim.
    #[error("io error reading source: {0}")]
    Io(#[from] std::io::Error),

    /// The spec markdown could not be read from disk. The path is the
    /// offending file.
    #[error("spec read failed: {0}")]
    SpecRead(PathBuf),
}

/// Non-fatal warning emitted during a capture. Surfaced via
/// [`CaptureReport::warnings`] so the gate can write telemetry without the
/// snapshot itself growing a side-channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureWarning {
    /// Qualifier of the function the warning concerns.
    pub qualifier: String,
    /// Human-readable diagnostic (EN; the gate translates for the operator).
    pub message: String,
}

/// Result of [`capture_for_spec`] — the snapshot plus any warnings.
///
/// Splitting the warnings out of [`Snapshot`] keeps the serialised diff
/// reproducible: the snapshot artefact is stable across runs, while
/// warnings (which carry per-machine grammar-installation state) live on
/// the capture report and never round-trip through serde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureReport {
    /// The captured snapshot.
    pub snapshot: Snapshot,
    /// Warnings emitted during capture (e.g. "grammar not installed for X,
    /// falling back to textual capture"). May be empty.
    pub warnings: Vec<CaptureWarning>,
}

impl Snapshot {
    /// Capture a snapshot of the functions declared in `## Funções tocadas`
    /// of `spec_md`, resolved against `codebase_root`.
    ///
    /// `loader` is consumed by reference: the snapshot does not own the
    /// grammar loader, so the same loader can be reused for the pre- and
    /// post-capture and injected fakes in tests.
    ///
    /// # Errors
    ///
    /// Returns [`RegressionError::Io`] only when reading a source file
    /// fails with a non-`NotFound` error. A `NotFound` is fail-open: the
    /// declared function is captured with an empty body so the downstream
    /// diff reports it.
    ///
    /// Never panics. Specs without `## Funções tocadas` resolve to an
    /// empty snapshot (not an error) — the gate decides what to make of
    /// a wave that declared no functions.
    pub fn capture_for_spec(
        loader: &GrammarLoader,
        spec_md: &str,
        codebase_root: &Path,
        spec_path: PathBuf,
    ) -> Result<CaptureReport, RegressionError> {
        let now = current_iso_ts();
        let mut snapshot = Snapshot::empty(spec_path, now);
        let mut warnings: Vec<CaptureWarning> = Vec::new();

        let Some(declared) = touched_functions::parse(spec_md) else {
            // Section absent — fail-open, empty snapshot. The gate's clean
            // diff against an equivalent empty post-capture says "nothing
            // declared, nothing to regress".
            return Ok(CaptureReport {
                snapshot,
                warnings,
            });
        };

        for tf in declared.all() {
            let capture = capture_one_function(loader, codebase_root, tf, &mut warnings)?;
            snapshot.insert(capture);
        }

        Ok(CaptureReport {
            snapshot,
            warnings,
        })
    }
}

/// Helper bridging [`Snapshot::capture_for_spec`] for callers that already
/// hold a [`TouchedFunctions`] (rare — kept for symmetry with the W0 API).
pub fn capture_for_parsed(
    loader: &GrammarLoader,
    declared: &TouchedFunctions,
    codebase_root: &Path,
    spec_path: PathBuf,
) -> Result<CaptureReport, RegressionError> {
    let now = current_iso_ts();
    let mut snapshot = Snapshot::empty(spec_path, now);
    let mut warnings: Vec<CaptureWarning> = Vec::new();

    for tf in declared.all() {
        let capture = capture_one_function(loader, codebase_root, tf, &mut warnings)?;
        snapshot.insert(capture);
    }

    Ok(CaptureReport {
        snapshot,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// Per-function capture
// ---------------------------------------------------------------------------

/// Capture a single declared function — resolve the source file, then try
/// the AST path; fall back to the textual path on `GrammarNotInstalled` /
/// missing-source.
fn capture_one_function(
    loader: &GrammarLoader,
    codebase_root: &Path,
    declared: &TouchedFunction,
    warnings: &mut Vec<CaptureWarning>,
) -> Result<FunctionCapture, RegressionError> {
    let qualifier = declared.qualifier.as_str();
    let final_name = declared.qualifier.function_name();

    // ── Locate candidate source files ───────────────────────────────────
    let candidates = resolve_candidates(codebase_root, declared);
    if candidates.is_empty() {
        warnings.push(CaptureWarning {
            qualifier: qualifier.clone(),
            message: format!(
                "no source files matched path hint `{}` for qualifier `{qualifier}`",
                declared.path_hint
            ),
        });
        return Ok(empty_capture(&qualifier, CaptureMode::Textual));
    }

    // ── Try each candidate in order. The first that yields a body wins. ─
    for candidate in &candidates {
        let source = match read_source(candidate) {
            Ok(s) => s,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(RegressionError::Io(err)),
        };

        // Resolve the language id agnostically — the loader maps the file
        // extension via tree_sitter_loader's `file_types` glob list.
        let lang_id = loader.language_id_for_path(candidate);

        // ── AST path ───────────────────────────────────────────────────
        if let Some(ref lang) = lang_id {
            match try_capture_via_ast(loader, &source, lang, &final_name) {
                Ok(Some((body, span, signature))) => {
                    return Ok(FunctionCapture {
                        qualifier,
                        mode: CaptureMode::Ast,
                        signature,
                        body,
                        span,
                    });
                }
                Ok(None) => {
                    // AST path ran but did not find the function — fall
                    // through to textual on the same source.
                }
                Err(AstError::GrammarNotInstalled(id)) => {
                    warnings.push(CaptureWarning {
                        qualifier: qualifier.clone(),
                        message: format!(
                            "grammar not installed for `{id}`; using textual capture"
                        ),
                    });
                    // fall through to textual path
                }
                Err(err) => {
                    // Other AST errors (parse failed, set_language ABI
                    // mismatch). Surface as a warning and fall back.
                    warnings.push(CaptureWarning {
                        qualifier: qualifier.clone(),
                        message: format!("ast capture failed: {err}; using textual capture"),
                    });
                }
            }
        }

        // ── Textual path ───────────────────────────────────────────────
        if let Some((body, span)) = capture_via_text(&source, &final_name) {
            return Ok(FunctionCapture {
                qualifier,
                mode: CaptureMode::Textual,
                signature: None,
                body,
                span,
            });
        }
    }

    // Every candidate either lacked the function or could not be read.
    warnings.push(CaptureWarning {
        qualifier: qualifier.clone(),
        message: format!(
            "function `{final_name}` not found in any candidate under `{}`",
            declared.path_hint
        ),
    });
    Ok(empty_capture(&qualifier, CaptureMode::Textual))
}

/// Build an empty placeholder capture. The diff treats an empty body in one
/// snapshot vs a non-empty body in the other as `Added` or `Removed`,
/// depending on which side is empty.
fn empty_capture(qualifier: &str, mode: CaptureMode) -> FunctionCapture {
    FunctionCapture {
        qualifier: qualifier.to_string(),
        mode,
        signature: None,
        body: String::new(),
        span: TextSpan { start: 0, end: 0 },
    }
}

// ---------------------------------------------------------------------------
// Candidate resolution — qualifier → list of files to look in
// ---------------------------------------------------------------------------

/// Resolve the qualifier's path hint into a list of candidate source files.
///
/// - `Qualifier::PathHint { path, .. }`: `path` is a file or directory
///   relative to `codebase_root`. If a file, it is the sole candidate; if a
///   directory, scan its files (one level deep, agnostic to language).
/// - `Qualifier::Module(_)` / `Qualifier::Pure(_)`: use the subsection
///   header's `path_hint`. Same dir/file logic.
fn resolve_candidates(codebase_root: &Path, declared: &TouchedFunction) -> Vec<PathBuf> {
    // The qualifier may carry its own path (PathHint shape); otherwise use
    // the enclosing subsection header's path_hint.
    let header_hint = declared.path_hint.trim();
    let qualifier_path: Option<&str> = match &declared.qualifier {
        Qualifier::PathHint { path, .. } => Some(path.as_str()),
        Qualifier::Module(_) | Qualifier::Pure(_) => None,
    };

    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(qp) = qualifier_path {
        let qp_trim = qp.trim_end_matches('/').trim_end_matches('\\');
        roots.push(codebase_root.join(qp_trim));
    }
    if !header_hint.is_empty() {
        let h_trim = header_hint.trim_end_matches('/').trim_end_matches('\\');
        // Skip logical hints (no path separator) — they cannot be resolved
        // to a directory on disk.
        if h_trim.contains('/') || h_trim.contains('\\') {
            roots.push(codebase_root.join(h_trim));
        }
    }

    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out: Vec<PathBuf> = Vec::new();

    for root in roots {
        if root.is_file() {
            if seen.insert(root.clone()) {
                out.push(root);
            }
            continue;
        }
        if root.is_dir() {
            // Single-level scan. Mustard does not recurse here — a wave
            // declaring a function inside a deep subtree should write a
            // more specific path_hint. Recursive scan would burn budget on
            // every capture for a marginal precision gain.
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && seen.insert(path.clone()) {
                    out.push(path);
                }
            }
        }
    }

    out
}

// ---------------------------------------------------------------------------
// AST capture
// ---------------------------------------------------------------------------

/// Try to capture the function's body via the AST path.
///
/// Returns `Ok(Some((body, span, signature)))` on success, `Ok(None)` when
/// the parser ran but no matching function was found, or `Err` for grammar
/// / parser issues that the caller should surface as a warning before
/// falling back.
fn try_capture_via_ast(
    loader: &GrammarLoader,
    source: &str,
    lang_id: &str,
    final_name: &str,
) -> Result<Option<(String, TextSpan, Option<crate::domain::ast::FunctionSig>)>, AstError> {
    // Reuse the signature extractor — it already handles both query-driven
    // (`function_signature.scm`) and fallback paths. Duplicating its logic
    // here would violate the reuse rule.
    let signatures = extract_function_signatures(loader, source, lang_id);
    let Some(sig) = signatures.into_iter().find(|s| s.name == final_name) else {
        return Ok(None);
    };

    // Re-parse to access the actual `function_item` (or equivalent) node so
    // we can read the *body*, not just the signature span. The signature
    // span only covers the name; the function-declaration node covers the
    // whole declaration including the body.
    let mut parser = TreeSitterParser::for_language(loader, lang_id)?;
    let tree = parser.parse(source)?;
    let root = tree.as_tree_sitter().root_node();

    // Walk the tree and find the smallest named node whose byte span
    // *contains* the signature span AND whose grammar-kind name suggests a
    // function declaration. We do not enumerate kind names per language —
    // we accept any kind whose name contains the substring `function` or
    // `method` or `fn` (agnostic heuristic; matches `function_item`,
    // `method_definition`, `function_declaration`, `fn_signature_item`).
    let body_node = find_enclosing_function_node(root, sig.span.start, sig.span.end);

    let (body, span) = match body_node {
        Some(node) => {
            let start = node.start_byte();
            let end = node.end_byte();
            let text = source.get(start..end).unwrap_or("").to_string();
            (text, TextSpan { start, end })
        }
        None => {
            // Could not find an enclosing function-decl node. Treat as AST
            // miss and return None so the caller falls back to text.
            return Ok(None);
        }
    };

    Ok(Some((body, span, Some(sig))))
}

/// Walk down from `root` and find the smallest named node whose byte range
/// contains `[sig_start, sig_end)` and whose `kind` smells like a function
/// declaration. Agnostic — no language id is checked.
fn find_enclosing_function_node<'tree>(
    root: tree_sitter::Node<'tree>,
    sig_start: usize,
    sig_end: usize,
) -> Option<tree_sitter::Node<'tree>> {
    fn kind_smells_like_function(kind: &str) -> bool {
        let lower = kind.to_ascii_lowercase();
        // `function_item`, `function_declaration`, `function_expression`,
        // `method_definition`, `fn_signature_item`, `arrow_function`, ...
        lower.contains("function") || lower.contains("method") || lower.contains("fn_")
            || lower == "fn"
            || lower == "func"
            || lower == "def"
    }

    let mut best: Option<tree_sitter::Node<'tree>> = None;
    // `descendant_for_byte_range` returns the smallest node containing the
    // signature span; we then walk up until we hit a node that smells like
    // a function declaration.
    let leaf = root.descendant_for_byte_range(sig_start, sig_end)?;
    let mut current = Some(leaf);
    while let Some(node) = current {
        if kind_smells_like_function(node.kind()) {
            best = Some(node);
            break;
        }
        current = node.parent();
    }
    best
}

// ---------------------------------------------------------------------------
// Textual capture
// ---------------------------------------------------------------------------

/// Capture a function's body via the agnostic textual path.
///
/// Strategy: locate a line containing the function's final identifier that
/// looks like a declaration (uses the same lexical shape as
/// [`crate::domain::ast::extract_function_signatures`] fallback). Then capture from
/// the start of that line until brace balance returns to 0, or — for
/// indentation-based languages — until the indentation drops below the
/// declaration line.
fn capture_via_text(source: &str, final_name: &str) -> Option<(String, TextSpan)> {
    // Build a needle that matches a function-like declaration of `final_name`.
    // We do a manual scan to keep this dependency-free — `mustard-core` does
    // not pull `regex` outside the AST module.
    let bytes = source.as_bytes();
    let mut line_start = 0usize;
    let len = bytes.len();

    while line_start <= len {
        // Find end of line.
        let line_end = source[line_start..]
            .find('\n')
            .map_or(len, |i| line_start + i);
        let line = &source[line_start..line_end];

        if line_smells_like_declaration_of(line, final_name) {
            // Capture body. Distinguish brace-style from indentation-style
            // by inspecting the suffix of the declaration line.
            let trimmed = line.trim_end();
            if trimmed.ends_with('{') || source[line_start..].find('{').is_some_and(|off| {
                // The opening brace is on this line or the next non-empty line.
                let cand_offset = line_start + off;
                cand_offset < line_end + 256
            }) {
                if let Some(end) = capture_brace_balanced_body(source, line_start) {
                    let body = source[line_start..end].to_string();
                    return Some((
                        body,
                        TextSpan {
                            start: line_start,
                            end,
                        },
                    ));
                }
            }
            // Indentation-style fallback (Python-like).
            if let Some(end) = capture_indentation_balanced_body(source, line_start) {
                let body = source[line_start..end].to_string();
                return Some((
                    body,
                    TextSpan {
                        start: line_start,
                        end,
                    },
                ));
            }
            // As a last resort, return the declaration line by itself —
            // better than nothing for languages with no consistent block
            // marker.
            return Some((
                line.to_string(),
                TextSpan {
                    start: line_start,
                    end: line_end,
                },
            ));
        }

        if line_end >= len {
            break;
        }
        line_start = line_end + 1;
    }

    None
}

/// `true` when `line` syntactically looks like a public-function declaration
/// for `name`. Agnostic — accepts any prefix recognised by the W1.5 fallback
/// regex (pub fn, export function, def, func, public T, ...).
fn line_smells_like_declaration_of(line: &str, name: &str) -> bool {
    let stripped = line.trim_start();
    // Quick reject: the line must mention the name as a whole identifier,
    // followed by `(` (parameter list).
    let Some(idx) = stripped.find(name) else {
        return false;
    };
    let after = &stripped[idx + name.len()..];
    if !after.trim_start().starts_with('(') {
        return false;
    }
    // Reject method invocations vs declarations: declarations begin with one
    // of a small list of agnostic prefixes.
    const PREFIXES: &[&str] = &[
        "pub ", "pub(", "pub\t", "fn ", "async ", "export ", "function ", "def ", "func ",
        "public ", "private ", "protected ", "internal ", "static ",
    ];
    PREFIXES.iter().any(|p| stripped.starts_with(p))
}

/// Capture from `start` (column 0 of the declaration line) until brace
/// balance returns to 0. Tracks string literals and line comments minimally
/// so braces inside `"…{…}"` and `// foo {` are ignored.
fn capture_brace_balanced_body(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = start;
    let mut depth: i32 = 0;
    let mut started = false;
    let mut in_string: Option<u8> = None;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied().unwrap_or(0);

        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if b == b'*' && next == b'/' {
                in_block_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(q) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_string = None;
            }
            i += 1;
            continue;
        }

        match b {
            b'"' | b'\'' | b'`' => {
                in_string = Some(b);
                i += 1;
                continue;
            }
            b'/' if next == b'/' => {
                in_line_comment = true;
                i += 2;
                continue;
            }
            b'/' if next == b'*' => {
                in_block_comment = true;
                i += 2;
                continue;
            }
            b'{' => {
                depth += 1;
                started = true;
            }
            b'}' => {
                depth -= 1;
                if started && depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }

    if started { Some(bytes.len()) } else { None }
}

/// Capture from `start` until the indentation drops below the declaration's
/// indentation level. Used as the Python-like fallback.
fn capture_indentation_balanced_body(source: &str, start: usize) -> Option<usize> {
    let lines: Vec<(usize, usize)> = line_byte_ranges_from(source, start);
    if lines.is_empty() {
        return None;
    }
    let (decl_start, decl_end) = lines[0];
    let decl_line = &source[decl_start..decl_end];
    let decl_indent = indent_width(decl_line);

    for &(ls, le) in lines.iter().skip(1) {
        let line = &source[ls..le];
        if line.trim().is_empty() {
            continue;
        }
        let ind = indent_width(line);
        if ind <= decl_indent {
            return Some(ls);
        }
    }
    Some(source.len())
}

/// Return `(line_start, line_end_exclusive_of_newline)` ranges starting at
/// `start` through end-of-source.
fn line_byte_ranges_from(source: &str, start: usize) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = start;
    while i <= bytes.len() {
        let end = source[i..].find('\n').map_or(bytes.len(), |off| i + off);
        out.push((i, end));
        if end >= bytes.len() {
            break;
        }
        i = end + 1;
    }
    out
}

/// Width of leading whitespace (spaces and tabs each count as 1).
fn indent_width(line: &str) -> usize {
    line.bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count()
}

// ---------------------------------------------------------------------------
// Misc helpers
// ---------------------------------------------------------------------------

fn read_source(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

/// Best-effort ISO-8601 UTC timestamp without pulling chrono in.
/// Mirrors the parse_iso_ms approach used in dashboard telemetry: builds
/// `YYYY-MM-DDTHH:MM:SS.fffZ` from a Unix epoch ms value. For the regression
/// gate, second-level precision is enough — the timestamp is used to order
/// snapshots, not to compute durations.
fn current_iso_ts() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let secs = ms / 1000;
    let frac = ms % 1000;
    // Convert epoch seconds to a UTC calendar date by the same approximate
    // algorithm telemetry.rs uses — exact day rollover does not matter for
    // our ordering purposes. We still produce a parseable ISO-8601 string.
    let (year, month, day, hour, minute, second) = epoch_to_ymd_hms(secs);
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{frac:03}Z"
    )
}

/// Convert epoch seconds into a `(year, month, day, hour, minute, second)`
/// tuple. Civil-from-days algorithm by Howard Hinnant (public domain). Avoids
/// pulling chrono just for one timestamp.
fn epoch_to_ymd_hms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;

    // Hinnant's civil_from_days: epoch = 1970-01-01.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    (year as i32, m, d, hour, minute, second)
}

// ---------------------------------------------------------------------------
// Tests — exercised at module scope; integration tests live in
// `regression_check::tests` (the parent module's #[cfg(test)] hook).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_root() -> PathBuf {
        // CARGO_MANIFEST_DIR = packages/core/. Walk up to workspace root.
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .ancestors()
            .find(|p| p.join("Cargo.lock").exists())
            .map(|p| p.to_path_buf())
            .unwrap_or(manifest_dir)
    }

    #[test]
    fn capture_returns_empty_snapshot_when_section_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = GrammarLoader::empty(tmp.path());
        let report = Snapshot::capture_for_spec(
            &loader,
            "# Spec\n\nno funcoes section\n",
            tmp.path(),
            PathBuf::from("spec.md"),
        )
        .expect("capture");
        assert!(report.snapshot.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn capture_textual_path_finds_pub_fn_in_rust_source() {
        let tmp = tempfile::tempdir().unwrap();
        // Write a tiny rust file with a function declaration.
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src_file = src_dir.join("mod.rs");
        std::fs::write(
            &src_file,
            "pub fn hello(x: i32) -> i32 {\n    x + 1\n}\n",
        )
        .unwrap();

        let spec = "\
# Spec

## Funções tocadas

### Em `src/mod.rs` (NOVO)
- `src/mod.rs::hello`
";
        let loader = GrammarLoader::empty(tmp.path());
        let report = Snapshot::capture_for_spec(
            &loader,
            spec,
            tmp.path(),
            PathBuf::from("spec.md"),
        )
        .expect("capture");

        // PathHint qualifier — qualifier.as_str() is "src/mod.rs::hello".
        let key = "src/mod.rs::hello";
        let cap = report.snapshot.get(key).expect("captured");
        assert_eq!(cap.mode, CaptureMode::Textual);
        assert!(cap.body.contains("pub fn hello"));
        assert!(cap.body.contains('}'));
        assert!(!cap.body.is_empty());
    }

    #[test]
    fn capture_records_warning_when_source_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = "\
# Spec

## Funções tocadas

### Em `does/not/exist/` (NOVO)
- `does/not/exist/foo.rs::bar`
";
        let loader = GrammarLoader::empty(tmp.path());
        let report = Snapshot::capture_for_spec(
            &loader,
            spec,
            tmp.path(),
            PathBuf::from("spec.md"),
        )
        .expect("capture");
        assert_eq!(report.snapshot.len(), 1);
        // The capture is empty-bodied (fail-open) and a warning was emitted.
        let cap = report
            .snapshot
            .functions
            .values()
            .next()
            .expect("one row");
        assert!(cap.body.is_empty());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn brace_balancer_handles_strings_and_comments() {
        let src = "pub fn x() {\n    let s = \"}\";  // }\n    /* } */\n    return;\n}\n";
        let end = capture_brace_balanced_body(src, 0).expect("balance");
        assert_eq!(&src[..end], src.trim_end_matches('\n'));
    }

    #[test]
    fn indent_capture_handles_python_like() {
        let src = "def foo():\n    return 1\n    return 2\n\nother";
        let end = capture_indentation_balanced_body(src, 0).expect("indent");
        let captured = &src[..end];
        assert!(captured.contains("def foo"));
        assert!(captured.contains("return 2"));
        assert!(!captured.contains("other"));
    }

    #[test]
    fn epoch_to_ymd_known_value() {
        // 1970-01-01T00:00:00Z — the anchor point. Tests the Hinnant
        // civil_from_days conversion at the epoch.
        let (y, m, d, h, mi, s) = epoch_to_ymd_hms(0);
        assert_eq!((y, m, d, h, mi, s), (1970, 1, 1, 0, 0, 0));

        // 2000-01-01T00:00:00Z = 946_684_800 — well-known constant.
        let (y, m, d, h, mi, s) = epoch_to_ymd_hms(946_684_800);
        assert_eq!((y, m, d, h, mi, s), (2000, 1, 1, 0, 0, 0));

        // Add 12h35m45s to the millennium anchor; verifies hh:mm:ss split.
        let (_, _, _, h, mi, s) = epoch_to_ymd_hms(946_684_800 + 12 * 3600 + 35 * 60 + 45);
        assert_eq!((h, mi, s), (12, 35, 45));
    }

    #[test]
    fn fixture_root_resolves() {
        // Sanity: the fixture root contains Cargo.lock.
        assert!(fixture_root().join("Cargo.lock").exists());
    }
}
