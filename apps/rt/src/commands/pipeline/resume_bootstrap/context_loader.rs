//! Spec A v4 / W6 — disciplined resume context (AC-A-10).
//!
//! Reads prior-wave `_summary.md` files, prunes them to the
//! [`super::RESUME_TOKEN_BUDGET`] cap (T6.3) restricted to the wikilink set the
//! operational wave spec declares (T6.4), then renders a `_context.md` for the
//! current wave (T6.5). Every reader fail-opens — a wave with no summary simply
//! does not contribute to the budget; a write error leaves `context_path =
//! None`.

use super::stage_resolver::{extract_objective, parse_header_value, read_first_lines};
use super::wave_progress::find_wave_dir_name;
use super::PRIORITY_BASE;
use crate::commands::economy::token_budget::{prune_to_budget, PrioritizedItem};
use crate::commands::wave::wave_context::{self, WaveContextInput, WaveMapEntry, WikiLink};
use mustard_core::io::atomic_md::find_outgoing_links;
use mustard_core::io::fs as mfs;
use mustard_core::platform::i18n::SupportedLocale as Locale;
use std::path::{Path, PathBuf};

/// Extract the set of `[[name]]` targets appearing inside the operational
/// wave spec body. Used as the filter for which prior `_summary.md` files are
/// allowed into the pruning pool (T6.4: load only summaries the current wave
/// actually references).
///
/// Returns `None` when no wikilinks were declared — the caller treats that as
/// "no filter" and falls back to chronological inheritance (load every prior
/// summary, then let the budget cap do its job).
pub(super) fn wikilinked_summary_targets(
    op_body: &str,
) -> Option<std::collections::HashSet<String>> {
    if op_body.is_empty() {
        return None;
    }
    let links = find_outgoing_links(op_body);
    if links.is_empty() {
        return None;
    }
    // The summaries we want are addressed as `wave-N-{role}/_summary` (or just
    // `wave-N-{role}` when the user dropped the trailing `/_summary`). Normalize
    // both forms by stripping the `/_summary` suffix.
    let mut set = std::collections::HashSet::new();
    for link in links {
        let key = link.strip_suffix("/_summary").unwrap_or(&link).to_string();
        set.insert(key);
    }
    Some(set)
}

/// Walk `spec_dir` for `wave-{0..current}-*/_summary.md` and return the
/// pruned-to-budget prefix.
///
/// Returns `(kept_texts, used_tokens, kept_count)`. Texts are returned in
/// relevance order (strongest match to the current wave's task first; recency
/// breaks ties) so the caller passes them through into `_context.md` rendering.
///
/// Fail-open: missing summaries are skipped silently. A wave that did not
/// produce a `_summary.md` simply does not contribute to the budget.
pub(super) fn load_pruned_prior_summaries(
    spec_dir: &Path,
    current_wave: u32,
    allowed: Option<&std::collections::HashSet<String>>,
    relevance_source: &str,
    budget: usize,
) -> (Vec<String>, usize, usize) {
    if current_wave == 0 {
        return (Vec::new(), 0, 0);
    }
    let mut candidates: Vec<PrioritizedItem> = Vec::new();

    // Iterate from the most recent prior wave down to wave 0 so the highest
    // priority is "wave just before the current one".
    let Ok(entries) = mfs::read_dir(spec_dir) else {
        return (Vec::new(), 0, 0);
    };
    // Build a (wave_id, _summary.md path) map keyed by wave number for
    // deterministic iteration.
    let mut by_wave: Vec<(u32, String, PathBuf)> = Vec::new();
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let dir_name = entry.file_name.clone();
        let Some(rest) = dir_name.strip_prefix("wave-") else {
            continue;
        };
        let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if digit_end == 0 {
            continue;
        }
        let Ok(n) = rest[..digit_end].parse::<u32>() else {
            continue;
        };
        if n >= current_wave {
            continue;
        }
        let summary_path = entry.path.join("_summary.md");
        if !summary_path.exists() {
            continue;
        }
        by_wave.push((n, dir_name, summary_path));
    }
    // Sort descending by wave number so most-recent prior wave is first.
    by_wave.sort_by_key(|a| std::cmp::Reverse(a.0));

    for (n, dir_name, path) in by_wave {
        // T6.4 — when the operational spec declared its inheritance via wikilinks,
        // skip summaries that are not referenced.
        if let Some(filter) = allowed {
            if !filter.contains(&dir_name) {
                continue;
            }
        }
        let Ok(body) = mfs::read_to_string(&path) else {
            continue;
        };
        // Priority decays from PRIORITY_BASE for the most recent wave. Saturating
        // sub keeps every prior wave at priority >= 1.
        let distance = current_wave.saturating_sub(n).saturating_sub(1);
        let priority = PRIORITY_BASE.saturating_sub(distance.min(u32::from(u8::MAX - 1)) as u8);
        candidates.push(PrioritizedItem::new(body, priority));
    }

    // Relevance, not just recency: order candidates by how strongly each prior
    // summary matches the current wave's task BEFORE the budget keeps its prefix
    // — the resume window then holds the most RELEVANT prior context, not merely
    // the most recent. Recency breaks ties. Empty source leaves recency order.
    let terms: std::collections::BTreeSet<String> = relevance_source
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.chars().count() >= 4)
        .map(str::to_ascii_lowercase)
        .collect();
    if !terms.is_empty() {
        let score = |text: &str| -> usize {
            let hay = text.to_ascii_lowercase();
            terms.iter().filter(|t| hay.contains(t.as_str())).count()
        };
        candidates
            .sort_by(|a, b| score(&b.text).cmp(&score(&a.text)).then(b.priority.cmp(&a.priority)));
    }

    let kept = prune_to_budget(&candidates, budget);
    let used_tokens: usize = kept
        .iter()
        .map(|item| crate::commands::economy::token_budget::estimate_tokens(&item.text))
        .sum();
    let texts: Vec<String> = kept.iter().map(|item| item.text.clone()).collect();
    let count = texts.len();
    (texts, used_tokens, count)
}

/// Generate `_context.md` for the current wave on resume (T6.5).
///
/// Builds a [`WaveContextInput`] from filesystem state — the pruned wikilinks,
/// the wave map (every `wave-N-*` dir in `spec_dir`), and an objective line
/// derived from the operational spec's `## Context` section when present.
///
/// `_context.md` is an **internal artefact** (the input handed to the next
/// wave's agent), so its headings are always rendered in EN regardless of the
/// project's natural `language` — per `feedback-mustard-i18n-agnostic`, only
/// the user-facing `spec.md` PRD carries the user's `language`.
///
/// Fail-open: returns `None` on render-cap violation or write IO error.
pub(super) fn generate_context_on_resume(
    spec_dir: &Path,
    current_wave: u32,
    kept_summary_texts: &[String],
    op_body: &str,
) -> Option<PathBuf> {
    let wave_id = find_wave_dir_name(spec_dir, current_wave)?;
    // `_context.md` is an internal artefact → EN, never the user's natural
    // `language` (which only colours the spec-facing PRD). See
    // `feedback-mustard-i18n-agnostic`.
    let locale = Locale::EnUs;

    // Inheritance: render one wikilink per kept summary (we know they fit the
    // budget; the renderer cap is words, not tokens, so listing the addresses
    // is bounded). Address shape: `wave-N-{role}/_summary` — matches the
    // wikilinks the operational spec already declares.
    let inheritance: Vec<WikiLink> = wave_summary_addresses(spec_dir, current_wave)
        .into_iter()
        // Limit to the count we actually loaded (kept count) — for parity with
        // what the agent sees.
        .take(kept_summary_texts.len().max(1))
        .map(WikiLink::new)
        .collect();

    // Position map: every wave dir under spec_dir, sorted by wave number.
    let position = build_wave_map(spec_dir, current_wave);

    // Objective: best-effort first paragraph under `## Contexto` / `## Context`.
    let objective = extract_objective(op_body);

    let input = WaveContextInput {
        spec_slug: spec_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        wave_id: wave_id.clone(),
        objective,
        inheritance,
        // Memory entries — best-effort. Spec memory lives at `<spec>/memory/*.md`.
        // We collect file stems for now; richer typing arrives with W7+.
        memory: list_spec_memory(spec_dir),
        position,
        // Resume does not prescribe concrete next steps — leave empty so the
        // renderer emits the i18n placeholder rather than an opinionated bullet.
        next_steps_suggestion: Vec::new(),
    };

    let body = wave_context::build(&input, locale).ok()?;
    wave_context::write(spec_dir, &wave_id, &body).ok()
}

/// Enumerate `wave-N-{role}/_summary` wikilink addresses for waves strictly
/// less than `current_wave`. Sorted by descending wave number (most recent
/// first) so the rendered inheritance section matches the priority order we
/// loaded summaries in.
fn wave_summary_addresses(spec_dir: &Path, current_wave: u32) -> Vec<String> {
    let mut rows: Vec<(u32, String)> = Vec::new();
    let Ok(entries) = mfs::read_dir(spec_dir) else {
        return Vec::new();
    };
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let Some(rest) = entry.file_name.strip_prefix("wave-") else {
            continue;
        };
        let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if digit_end == 0 {
            continue;
        }
        let Ok(n) = rest[..digit_end].parse::<u32>() else {
            continue;
        };
        if n >= current_wave {
            continue;
        }
        rows.push((n, format!("{}/_summary", entry.file_name)));
    }
    rows.sort_by_key(|a| std::cmp::Reverse(a.0));
    rows.into_iter().map(|(_, addr)| addr).collect()
}

/// Build the wave-map entries for every `wave-N-*` directory in `spec_dir`.
/// Status is best-effort: reads the per-wave `spec.md` header (`### Stage:` /
/// `### Outcome:`) and falls back to `Unknown` when absent. The wave matching
/// `current_wave` is flagged `current = true`.
fn build_wave_map(spec_dir: &Path, current_wave: u32) -> Vec<WaveMapEntry> {
    let Ok(entries) = mfs::read_dir(spec_dir) else {
        return Vec::new();
    };
    let mut rows: Vec<(u32, String, String)> = Vec::new();
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let Some(rest) = entry.file_name.strip_prefix("wave-") else {
            continue;
        };
        let digit_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
        if digit_end == 0 {
            continue;
        }
        let Ok(n) = rest[..digit_end].parse::<u32>() else {
            continue;
        };
        let head = read_first_lines(&entry.path.join("spec.md"), 12).unwrap_or_default();
        let stage = parse_header_value(&head, "stage").unwrap_or_else(|| "Unknown".to_string());
        let outcome = parse_header_value(&head, "outcome").unwrap_or_else(|| "—".to_string());
        let status = format!("{stage} / {outcome}");
        rows.push((n, entry.file_name, status));
    }
    rows.sort_by_key(|r| r.0);
    rows.into_iter()
        .map(|(n, dir_name, status)| WaveMapEntry {
            wave_id: dir_name,
            status,
            current: n == current_wave,
        })
        .collect()
}

/// Surface the spec's `memory/*.md` notes as wikilink targets (one per file).
/// Fail-open: returns empty when the memory directory is missing.
fn list_spec_memory(spec_dir: &Path) -> Vec<WikiLink> {
    let dir = spec_dir.join("memory");
    let Ok(entries) = mfs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<WikiLink> = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        if !entry.file_name.ends_with(".md") {
            continue;
        }
        let stem = entry
            .file_name
            .strip_suffix(".md")
            .unwrap_or(&entry.file_name);
        out.push(WikiLink::new(format!("memory/{stem}")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::RESUME_TOKEN_BUDGET;
    use super::*;

    /// Helper: seed `wave-{0..12}-rt/_summary.md` files with bulky bodies (~30k
    /// chars each ≈ 7.5k tokens), plus a thin `wave-13-rt/spec.md` declaring
    /// the operational wave. Returns the spec dir.
    fn seed_12_wave_spec(spec_dir: &Path, declare_wikilinks: bool) {
        // 13 prior waves, each with a ~30 000-character _summary.md.
        let big_body: String = std::iter::repeat_n("regressao detectada palavra ", 1_200) // 28 chars × 1_200 ≈ 33 600 chars ≈ 8 400 tokens each
            .collect();
        for n in 0..13u32 {
            let wave_dir = spec_dir.join(format!("wave-{n}-rt"));
            std::fs::create_dir_all(&wave_dir).unwrap();
            std::fs::write(wave_dir.join("_summary.md"), &big_body).unwrap();
            // Minimal `spec.md` so the wave map can read its stage/outcome.
            std::fs::write(
                wave_dir.join("spec.md"),
                "### Stage: Close\n### Outcome: Completed\n",
            )
            .unwrap();
        }
        // Current wave 12 → operational dir already created above; add inheritance.
        let op_path = spec_dir.join("wave-12-rt").join("spec.md");
        let mut body = String::from(
            "### Stage: Execute\n### Outcome: Active\n\n## Contexto\n\nResumir o gate.\n\n",
        );
        if declare_wikilinks {
            // Reference only the two most recent prior waves — pruning must
            // exclude the other 11 even before the budget cap kicks in (T6.4).
            body.push_str("Inherits from [[wave-11-rt/_summary]] and [[wave-10-rt/_summary]].\n");
        }
        std::fs::write(&op_path, body).unwrap();
    }

    /// AC-A-10 — with wikilink filter declared, only the referenced summaries
    /// are loaded AND the byte cost stays within 10 000 tokens regardless of
    /// how big each prior summary is.
    #[test]
    fn test_resume_bootstrap_stays_within_10k_tokens_with_12_prior_waves() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_12_wave_spec(spec_dir, /* declare_wikilinks = */ true);

        let op_body = std::fs::read_to_string(spec_dir.join("wave-12-rt").join("spec.md")).unwrap();
        let allowed = wikilinked_summary_targets(&op_body);
        // The wikilink filter must surface both referenced waves.
        let allowed_set = allowed.as_ref().expect("wikilinks parsed");
        assert!(allowed_set.contains("wave-11-rt"));
        assert!(allowed_set.contains("wave-10-rt"));

        let (texts, used_tokens, count) = load_pruned_prior_summaries(
            spec_dir,
            /* current_wave = */ 12,
            allowed.as_ref(),
            &op_body,
            RESUME_TOKEN_BUDGET,
        );
        // AC-A-10: under the 10 000-token cap, no matter what.
        assert!(
            used_tokens <= RESUME_TOKEN_BUDGET,
            "used_tokens={used_tokens} exceeded budget={RESUME_TOKEN_BUDGET}"
        );
        // Each summary is ~8 400 tokens → at most one fits under a 10 000 cap.
        assert!(count <= 2, "expected at most 2 summaries to fit, got {count}");
        // The first kept summary is non-empty (sanity: pruning did NOT degrade
        // to fail-open stub per `feedback_no_stub_fail_open`).
        assert!(!texts.is_empty(), "pruning must keep at least one summary");
        assert!(
            !texts[0].is_empty(),
            "kept summary must carry the original body, not Default::default()"
        );
    }

    /// Even WITHOUT wikilinks declared, the budget alone caps the load.
    #[test]
    fn test_resume_bootstrap_budget_caps_without_wikilinks() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_12_wave_spec(spec_dir, /* declare_wikilinks = */ false);
        let op_body = std::fs::read_to_string(spec_dir.join("wave-12-rt").join("spec.md")).unwrap();
        let allowed = wikilinked_summary_targets(&op_body);
        // No wikilinks → no filter applied.
        assert!(allowed.is_none());

        let (_texts, used_tokens, count) =
            load_pruned_prior_summaries(spec_dir, 12, None, &op_body, RESUME_TOKEN_BUDGET);
        assert!(
            used_tokens <= RESUME_TOKEN_BUDGET,
            "budget breached: {used_tokens} > {RESUME_TOKEN_BUDGET}"
        );
        // Every prior wave was a candidate; budget alone keeps the prefix small.
        assert!(count >= 1, "at least one summary must survive the cap");
    }

    /// T6.5 — `generate_context_on_resume` writes a `_context.md` under the
    /// current wave directory using the i18n-aware renderer.
    #[test]
    fn test_resume_bootstrap_generates_context_md() {
        let dir = tempfile::tempdir().unwrap();
        let spec_dir = dir.path();
        seed_12_wave_spec(spec_dir, /* declare_wikilinks = */ true);
        let op_body = std::fs::read_to_string(spec_dir.join("wave-12-rt").join("spec.md")).unwrap();
        let allowed = wikilinked_summary_targets(&op_body);
        let (texts, _used, _count) =
            load_pruned_prior_summaries(spec_dir, 12, allowed.as_ref(), &op_body, RESUME_TOKEN_BUDGET);

        let written = generate_context_on_resume(spec_dir, 12, &texts, &op_body)
            .expect("context generation must succeed");
        assert!(written.ends_with("_context.md"));
        let body = std::fs::read_to_string(&written).unwrap();
        // The 5 required headings must be present.
        assert!(body.contains("## "), "must contain markdown headings");
        // `_context.md` is an internal artefact → EN headings, never PT.
        assert!(body.contains("## Objective"), "internal artefact must be EN: {body}");
        assert!(!body.contains("## Objetivo"), "internal artefact must not be PT: {body}");
        // Must contain at least one wikilink (inheritance section).
        assert!(body.contains("[[wave-"), "must reference at least one prior wave");
    }

    /// T6.4 — `wikilinked_summary_targets` strips the trailing `/_summary`
    /// suffix so consumers can match against wave dir names directly.
    #[test]
    fn test_wikilinked_summary_targets_normalizes_suffix() {
        let body = "see [[wave-3-rt/_summary]] and [[wave-2-ui]] for details.";
        let set = wikilinked_summary_targets(body).expect("links parsed");
        assert!(set.contains("wave-3-rt"));
        assert!(set.contains("wave-2-ui"));
    }

    /// `wikilinked_summary_targets` returns `None` (no filter) when no wikilinks
    /// appear in the body — the caller falls back to chronological inheritance.
    #[test]
    fn test_wikilinked_summary_targets_returns_none_when_no_links() {
        assert!(wikilinked_summary_targets("plain prose no links").is_none());
        assert!(wikilinked_summary_targets("").is_none());
    }
}
