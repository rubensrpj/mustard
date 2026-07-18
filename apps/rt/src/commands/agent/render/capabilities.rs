//! Capabilities — durable "what the system already does" injection.
//!
//! REUSES only the BM25 ranking arithmetic (`domain::ranking`), so there is no
//! second ranker. It does NOT touch the `Knowledge` store: capabilities are
//! DURABLE — they must never decay or be pruned (no `last_used` write-back, no
//! confidence decay, no mutation). This block only ranks + cuts capability docs
//! for relevance and renders them; it reads the filesystem and writes nothing.

use mustard_core::io::fs as mfs;
use mustard_core::ClaudePaths;
use std::fmt::Write as _;
use std::path::Path;

/// Relevance floor as a fraction of the best score (×1024) — the same anti-bloat
/// bound `context_inject::MEMORY_RELEVANCE_FLOOR_FRACTION` expresses, mirrored
/// here in fixed-point so capabilities get the identical relevance discipline as
/// the memory blocks (an off-topic capability never enters).
const CAPABILITY_RELEVANCE_FLOOR_FRACTION: u64 = 348; // 0.34 ×1024 (rounded)

/// Top-K capabilities injected once the relevance floor has cut the weak tail.
const CAPABILITY_TOP_K: usize = 3;

/// Minimum content-token length: shorter tokens
/// (`the`, `a`, `is`) match too broadly to discriminate, so both the intent query
/// and a capability's searchable text are tokenised on ≥4-char lowercase
/// alphanumeric runs.
const CAPABILITY_MIN_TERM_LEN: usize = 4;

/// Active PUSH of the RELEVANT durable capabilities — the "what the system already
/// does" context at ANALYZE. Loads every `.claude/capabilities/*.md`, parses each
/// into a [`Capability`](mustard_core::domain::capability::Capability) (reusing
/// `capability::parse`; unparseable docs are skipped — fail-open), ranks them
/// against `intent` with the core's byte-stable BM25 (`domain::ranking`, the SAME
/// arithmetic the knowledge recall uses), cuts the weak tail with the SAME
/// relevance floor, takes the top-K (rank desc, then id asc for byte-stability),
/// and renders them under a `## CAPABILITIES` heading.
///
/// Spec-agnostic: it ranks ALL active capabilities against the request intent, so
/// it works for a fresh feature at ANALYZE (no spec, no wave). Empty (so the
/// heading collapses) when there are no capabilities or none clears the floor.
///
/// DURABILITY: this never mutates anything — no `last_used` write-back, no decay,
/// no prune. It does not touch the `Knowledge` store.
pub(crate) fn capability_block(project: &Path, intent: &str) -> String {
    let Ok(dir) = ClaudePaths::for_project(project).map(|p| p.capabilities_dir()) else {
        return String::new();
    };
    let caps = load_capabilities(&dir);
    let selected = rank_capabilities(&caps, intent, CAPABILITY_TOP_K);
    let bullets = render_capability_bullets(&selected);
    if bullets.is_empty() {
        return String::new();
    }
    // Folded into the `{cross_wave_memory}` body, so it carries its OWN heading —
    // a distinct sub-section beside PROJECT KNOWLEDGE / CROSS-WAVE MEMORY.
    format!("## CAPABILITIES\n{bullets}")
}

/// Load + parse every `.claude/capabilities/*.md` into a [`Capability`](mustard_core::domain::capability::Capability),
/// skipping unreadable / unparseable docs (fail-open) and any non-active capability (a
/// deprecated capability is durable history, not current context). The directory
/// listing is sorted by file name so the corpus order is deterministic; never
/// panics.
fn load_capabilities(dir: &Path) -> Vec<mustard_core::domain::capability::Capability> {
    let Ok(mut entries) = mfs::read_dir(dir) else {
        return Vec::new();
    };
    entries.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    let mut out = Vec::new();
    for entry in entries {
        if entry.is_dir || !entry.file_name.ends_with(".md") {
            continue;
        }
        let Ok(md) = mfs::read_to_string(&entry.path) else {
            continue;
        };
        let cap = crate::commands::capability::parse(&md);
        // A capability with no id is a parse miss (no frontmatter) — skip it.
        // A deprecated capability is excluded: ANALYZE wants what the system
        // does NOW, and `is_active` is lenient (empty/unset status counts live).
        if cap.id.trim().is_empty() || !cap.is_active() {
            continue;
        }
        out.push(cap);
    }
    out
}

/// Rank `caps` against `intent` with the core's fixed-point BM25 and return the
/// top-`max` survivors of the relevance floor.
///
/// The rankable text of a capability is its searchable content — the title plus
/// every requirement statement plus every scenario's when/then. The corpus stats
/// (`avgdl`) and per-term `bm25_x1024_default` are the SAME `domain::ranking`
/// arithmetic (no second ranker). The cut is
/// score-desc with id-asc tiebreak (byte-stable),
/// then keep only documents within [`CAPABILITY_RELEVANCE_FLOOR_FRACTION`] of the
/// top score, then truncate to `max`. NO decay weight — capabilities are durable,
/// so unlike the knowledge recall there is no confidence/age attenuation. Pure +
/// deterministic; never panics; never mutates.
fn rank_capabilities<'a>(
    caps: &'a [mustard_core::domain::capability::Capability],
    intent: &str,
    max: usize,
) -> Vec<&'a mustard_core::domain::capability::Capability> {
    use mustard_core::domain::ranking::{avgdl_x1024, bm25_x1024_default};

    if max == 0 || caps.is_empty() {
        return Vec::new();
    }
    let terms = capability_query_terms(intent);
    if terms.is_empty() {
        return Vec::new();
    }

    // Tokenise each capability's searchable text once; corpus stats feed the
    // shared `avgdl` so BM25's length normalisation is corpus-aware (same as
    // recall over the knowledge corpus).
    let docs: Vec<Vec<String>> = caps.iter().map(capability_doc_terms).collect();
    let total_len: usize = docs.iter().map(Vec::len).sum();
    let avgdl = avgdl_x1024(total_len, docs.len());

    // Score every capability: sum the per-term BM25 over the query terms. A term
    // absent from the doc contributes 0 (bm25 of tf=0), so a capability with no
    // intent overlap scores 0 and is dropped. NO confidence/decay weight — the
    // durable injector ranks on textual relevance alone.
    let mut scored: Vec<(u64, &mustard_core::domain::capability::Capability)> =
        Vec::with_capacity(caps.len());
    for (doc, cap) in docs.iter().zip(caps) {
        let dl = doc.len();
        let mut bm25 = 0_u64;
        for term in &terms {
            let tf = doc.iter().filter(|t| *t == term).count();
            bm25 = bm25.saturating_add(bm25_x1024_default(tf, dl, avgdl));
        }
        if bm25 > 0 {
            scored.push((bm25, cap));
        }
    }
    if scored.is_empty() {
        return Vec::new();
    }

    // Best-first; id-asc breaks ties for a byte-stable order independent of the
    // directory enumeration (the same tiebreak shape recall uses on slug).
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));

    // Relevance floor (anti-bloat): keep only capabilities within
    // CAPABILITY_RELEVANCE_FLOOR_FRACTION of the top score. Fixed-point: `score *
    // frac` ×1024 vs `top * SCALE` — no float enters the cut (mirrors recall).
    if let Some(&(top, _)) = scored.first() {
        let floor_x1024 = top.saturating_mul(CAPABILITY_RELEVANCE_FLOOR_FRACTION);
        scored.retain(|(score, _)| {
            score.saturating_mul(mustard_core::domain::ranking::SCALE) >= floor_x1024
        });
    }
    scored.truncate(max);
    scored.into_iter().map(|(_, cap)| cap).collect()
}

/// The distinct intent query terms: ≥[`CAPABILITY_MIN_TERM_LEN`]-char lowercase
/// alphanumeric runs, deduplicated, order-preserving.
fn capability_query_terms(intent: &str) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for tok in intent
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if tok.len() >= CAPABILITY_MIN_TERM_LEN && seen.insert(tok.to_string()) {
            out.push(tok.to_string());
        }
    }
    out
}

/// Tokenise a capability's searchable text into its term bag — the title, every
/// requirement statement, and every scenario when/then, as ≥
/// [`CAPABILITY_MIN_TERM_LEN`]-char lowercase alphanumeric runs WITH duplicates
/// (term frequency is the BM25 signal). The status and the opaque link ids
/// (`covers`/`specs`/`related`) are NOT searchable content — they carry no
/// intent signal and would only add noise.
fn capability_doc_terms(cap: &mustard_core::domain::capability::Capability) -> Vec<String> {
    let mut combined = cap.title.clone();
    for req in &cap.requirements {
        combined.push(' ');
        combined.push_str(&req.statement);
        for sc in &req.scenarios {
            combined.push(' ');
            combined.push_str(&sc.when);
            combined.push(' ');
            combined.push_str(&sc.then);
        }
    }
    let mut out: Vec<String> = Vec::new();
    for tok in combined
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if tok.len() >= CAPABILITY_MIN_TERM_LEN {
            out.push(tok.to_string());
        }
    }
    out
}

/// Render the already-selected (see [`rank_capabilities`]) capabilities as
/// `- **{title}** ([[{id}]]) — {first requirement statement, trimmed}` bullets.
/// A capability with no requirement degrades to `- **{title}** ([[{id}]])` (the
/// link still resolves). Empty (no heading, no bullets) when the slice is empty
/// so the caller can collapse. Byte-stable: the slice order is fixed by
/// [`rank_capabilities`] and nothing here reads a clock or a path.
fn render_capability_bullets(
    caps: &[&mustard_core::domain::capability::Capability],
) -> String {
    if caps.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for cap in caps {
        let summary = cap
            .requirements
            .first()
            .map(|r| r.statement.trim())
            .unwrap_or("")
            .chars()
            .take(160)
            .collect::<String>();
        if summary.is_empty() {
            let _ = writeln!(out, "- **{}** ([[{}]])", cap.title, cap.id);
        } else {
            let _ = writeln!(out, "- **{}** ([[{}]]) — {summary}", cap.title, cap.id);
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Write a capability doc under `<project>/.claude/capabilities/{slug}.md`
    /// via the canonical renderer, so the parse the injector runs is exercised.
    fn write_capability(
        project: &Path,
        slug: &str,
        title: &str,
        requirement: &str,
        when: &str,
        then: &str,
    ) {
        use mustard_core::domain::capability::{Capability, Requirement, Scenario};
        let dir = project.join(".claude").join("capabilities");
        std::fs::create_dir_all(&dir).unwrap();
        let cap = Capability {
            id: format!("cap.{slug}"),
            title: title.into(),
            status: "active".into(),
            requirements: vec![Requirement {
                statement: requirement.into(),
                scenarios: vec![Scenario {
                    name: "s".into(),
                    when: when.into(),
                    then: then.into(),
                    command: None,
                }],
            }],
            ..Capability::default()
        };
        std::fs::write(
            dir.join(format!("{slug}.md")),
            crate::commands::capability::render(&cap),
        )
        .unwrap();
    }

    /// Two capabilities, an intent that matches ONE: the matching capability is
    /// injected, the off-topic one is cut by the relevance floor.
    #[test]
    fn capability_block_injects_relevant_and_cuts_off_topic() {
        let dir = tempdir().unwrap();
        write_capability(
            dir.path(),
            "token-savings",
            "Token savings recorder",
            "The system SHALL record token cost and savings per command.",
            "a command runs through the harness",
            "its token cost and savings are recorded",
        );
        write_capability(
            dir.path(),
            "kubernetes-deploy",
            "Kubernetes deployment manifests",
            "The system SHALL render kubernetes ingress manifests for deployment.",
            "a deployment is requested",
            "ingress manifests are produced",
        );

        let block = capability_block(dir.path(), "improve how we record token savings");
        assert!(block.starts_with("## CAPABILITIES"), "heading present: {block}");
        // The relevant capability is injected, with its `[[id]]` and first req.
        assert!(block.contains("Token savings recorder"), "relevant injected: {block}");
        assert!(block.contains("[[cap.token-savings]]"), "id link present: {block}");
        assert!(
            block.contains("SHALL record token cost and savings"),
            "first requirement rendered: {block}"
        );
        // The off-topic capability is cut by the relevance floor.
        assert!(
            !block.contains("Kubernetes"),
            "off-topic capability must not enter: {block}"
        );
    }

    /// Zero capabilities (no directory) → empty block (the heading collapses).
    #[test]
    fn capability_block_empty_when_no_capabilities() {
        let dir = tempdir().unwrap();
        // No `.claude/capabilities/` at all.
        assert!(capability_block(dir.path(), "anything at all here").is_empty());
        // And with an empty directory present, still empty.
        std::fs::create_dir_all(dir.path().join(".claude").join("capabilities")).unwrap();
        assert!(capability_block(dir.path(), "anything at all here").is_empty());
    }

    /// The injector mutates NOTHING on disk: the capability files (and the whole
    /// `.claude/` tree) are byte-identical before and after a `capability_block`
    /// call — no `last_used` write-back, no decay, no prune.
    #[test]
    fn capability_block_mutates_nothing_on_disk() {
        let dir = tempdir().unwrap();
        write_capability(
            dir.path(),
            "token-savings",
            "Token savings recorder",
            "The system SHALL record token cost and savings per command.",
            "a command runs",
            "savings are recorded",
        );
        let caps = dir.path().join(".claude").join("capabilities");
        let snapshot = |root: &Path| -> Vec<(String, String)> {
            let mut v: Vec<(String, String)> = std::fs::read_dir(root)
                .unwrap()
                .map(|e| {
                    let p = e.unwrap().path();
                    (
                        p.file_name().unwrap().to_string_lossy().into_owned(),
                        std::fs::read_to_string(&p).unwrap(),
                    )
                })
                .collect();
            v.sort();
            v
        };
        let before = snapshot(&caps);
        // A run that injects (matching intent) and one that injects nothing.
        let _ = capability_block(dir.path(), "record token savings");
        let _ = capability_block(dir.path(), "completely unrelated query terms");
        let after = snapshot(&caps);
        assert_eq!(before, after, "capability docs must be byte-identical after injection");
    }

    /// Output is byte-stable across calls, and id-asc breaks ties for an order
    /// independent of directory enumeration.
    #[test]
    fn capability_block_is_byte_stable() {
        let dir = tempdir().unwrap();
        // Two equally-relevant capabilities (identical searchable text) → the
        // id-asc tiebreak fixes their order deterministically.
        write_capability(
            dir.path(), "bbb", "Beta", "The system SHALL handle caching relevance ranking.",
            "x", "y",
        );
        write_capability(
            dir.path(), "aaa", "Alpha", "The system SHALL handle caching relevance ranking.",
            "x", "y",
        );
        let a = capability_block(dir.path(), "caching relevance ranking");
        let b = capability_block(dir.path(), "caching relevance ranking");
        assert_eq!(a, b, "deterministic across calls");
        // id-asc: cap.aaa precedes cap.bbb regardless of file enumeration order.
        let pos_a = a.find("[[cap.aaa]]").expect("aaa present");
        let pos_b = a.find("[[cap.bbb]]").expect("bbb present");
        assert!(pos_a < pos_b, "id-asc tiebreak orders aaa before bbb: {a}");
    }

    /// Deprecated capabilities are excluded — ANALYZE wants what the system does
    /// NOW. A doc with no frontmatter id (parse miss) is also skipped.
    #[test]
    fn capability_block_skips_deprecated_and_unparseable() {
        let dir = tempdir().unwrap();
        let caps = dir.path().join(".claude").join("capabilities");
        std::fs::create_dir_all(&caps).unwrap();
        // Deprecated capability — durable history, not current context.
        use mustard_core::domain::capability::{Capability, Requirement};
        let dep = Capability {
            id: "cap.legacy".into(),
            title: "Legacy caching path".into(),
            status: "deprecated".into(),
            requirements: vec![Requirement {
                statement: "The system SHALL use the old caching path.".into(),
                scenarios: vec![],
            }],
            ..Capability::default()
        };
        std::fs::write(caps.join("legacy.md"), crate::commands::capability::render(&dep)).unwrap();
        // Unparseable garbage (no frontmatter id) — fail-open skip, never panic.
        std::fs::write(caps.join("junk.md"), "not a capability at all, just prose about caching\n").unwrap();

        let block = capability_block(dir.path(), "caching path query");
        assert!(block.is_empty(), "deprecated + unparseable yield nothing: {block}");
    }
}
