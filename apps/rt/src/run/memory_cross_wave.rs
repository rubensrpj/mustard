//! `mustard-rt run memory cross-wave` — render a markdown summary of
//! `agent.memory` events captured by the waves that ran before the current one.
//!
//! Part of the wave-network spec (`2026-05-20-mustard-wave-network-standard`):
//! the SKILL `/feature` (and `/resume`) embeds the rendered markdown into the
//! agent prompt of wave N so the agent inherits context from waves 1..N-1
//! without re-reading their spec files.
//!
//! ## Wave-name source — two-tier
//!
//! 1. `<spec-dir>/wave-plan.md`'s `## Tabela de Waves` markdown table. Rows
//!    that begin with `|` and whose first cell parses as a wave number
//!    contribute their `Spec` column (wikilink stripped).
//! 2. **Filesystem fallback** — when the table is missing or empty (e.g. the
//!    plan uses an ASCII code-fence diagram instead of a table), scan
//!    `<spec-dir>` for child directories whose name matches `wave-(\d+)-`
//!    and emit them ordered by number. See [`parse_wave_dirs_from_fs`].
//!
//! ## Memory-event schema
//!
//! `agent.memory` events are emitted with the following effective shape — this
//! query matches *both* legacy and canonical attribution to be robust:
//!
//! - `HarnessEvent.spec = Some("{spec-slug}")` (envelope-level — column
//!   `events.spec`). The canonical attribution used by every other emitter.
//! - `HarnessEvent.wave = N` (envelope int — column `events.wave`).
//! - `payload.spec = "{spec-slug}"` *and/or* `payload.wave = N` — present when
//!   the writer is invoked with an explicit JSON payload that mirrors the
//!   envelope fields. We OR these against the envelope columns so writers
//!   that only set one source still match.
//! - `payload.pipeline = "{spec-slug}"` — legacy attribution. The previous
//!   query used `payload.pipeline = <wave-name>` (a different convention)
//!   which never matched: the writer stores the *parent* spec slug there,
//!   not the wave name. The new query no longer relies on `pipeline`.
//! - `payload.summary = "..."` — rendered into the markdown bullet list.
//!
//! Output: markdown only (stdout). Empty string when there are no prior waves
//! or no captured memory rows for them. Exit 0 always (fail-open).

use crate::run::env::project_dir;
use mustard_core::fs;
use mustard_core::projection::read_harness_events_from_ndjson_dir;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// At most this many memory rows per prior wave land in the rendered block —
/// keeps the embedded context bounded.
const MAX_MEMORIES_PER_WAVE: usize = 5;

/// Strip surrounding `[[`/`]]` (and whitespace) from a wikilink token. Returns
/// `None` when the token does not look like a wikilink.
fn strip_wikilink(raw: &str) -> Option<String> {
    let t = raw.trim();
    let inner = t.strip_prefix("[[").and_then(|s| s.strip_suffix("]]"))?;
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    Some(inner.to_string())
}

/// Parse the wave-plan markdown table and return the ordered wave names (the
/// `Spec` column, wikilinks stripped).
///
/// Recognises rows whose first cell parses as a wave number (`1`, `W1`,
/// `Wave 1`, …) — mirrors `wave_tree::parse_wave_plan` for consistency, but
/// returns the *Spec* column instead of the folder column.
pub(crate) fn parse_wave_names(wave_plan_text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw_line in wave_plan_text.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        let Some(rest) = line.strip_prefix('|') else {
            continue;
        };
        let body = rest;
        let cells: Vec<&str> = body.split('|').map(str::trim).collect();
        // Expect at minimum: label | Spec | ... (separator rows are filtered
        // below by the label-cell shape check).
        if cells.len() < 2 {
            continue;
        }
        let label = cells[0].to_lowercase();
        // Skip header & separator rows.
        let label_body: &str = label
            .strip_prefix('w')
            .map_or(&label, str::trim_start);
        let label_body: &str = label_body
            .strip_prefix("ave")
            .map_or(label_body, str::trim_start);
        if label_body.is_empty() || !label_body.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // The Spec column is the next cell (cells[1]). Strip `[[wave-N-...]]`.
        if let Some(name) = strip_wikilink(cells[1]) {
            out.push(name);
        }
    }
    out
}

/// Resolve the per-spec NDJSON events directory:
/// `<project>/.claude/spec/<spec>/events`.
fn spec_events_dir(project: &Path, spec_slug: &str) -> PathBuf {
    project.join(".claude").join("spec").join(spec_slug).join("events")
}

/// Parse the wave number `N` from a wave name like `wave-3-frontend`. Returns
/// `None` when the prefix does not start with `wave-<digits>-`.
fn parse_wave_number(wave_name: &str) -> Option<i64> {
    let rest = wave_name.strip_prefix("wave-")?;
    let n_str: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if n_str.is_empty() {
        return None;
    }
    n_str.parse::<i64>().ok()
}

/// Filesystem fallback: list wave directories under a spec dir, ordered by
/// the numeric prefix `N` of their name (`wave-N-{role}`). Used when the
/// `wave-plan.md` table is missing or empty.
///
/// I/O errors yield an empty `Vec` (fail-open).
pub(crate) fn parse_wave_dirs_from_fs(spec_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(spec_dir) else { return Vec::new() };
    let mut hits: Vec<(i64, String)> = Vec::new();
    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let name = entry.file_name.clone();
        let Some(n) = parse_wave_number(&name) else {
            continue;
        };
        // Require `wave-<digits>-` (a trailing role token) — `wave-1` alone is
        // not a canonical wave dir.
        let prefix = format!("wave-{n}-");
        if !name.starts_with(&prefix) {
            continue;
        }
        hits.push((n, name));
    }
    hits.sort_by_key(|(n, _)| *n);
    hits.into_iter().map(|(_, name)| name).collect()
}

/// Fetch up to [`MAX_MEMORIES_PER_WAVE`] `agent.memory` payloads for a single
/// `(spec, wave)` pair, newest first.
///
/// W5: `agent.memory` events live in the per-spec NDJSON sink under
/// `<project>/.claude/spec/<spec>/events/`. The walker hydrates every NDJSON
/// line into a `HarnessEvent`, then filters in-memory.
///
/// Matches both attribution conventions described in the module docs:
///
/// - envelope-level: `event.spec == spec_slug && event.wave == wave_n`
/// - payload-level:  `payload.spec == spec_slug && payload.wave == wave_n`
///
/// The two are OR'd so a writer that sets only one source still matches.
pub(crate) fn memories_for_spec_wave(
    project: &Path,
    spec_slug: &str,
    wave_n: i64,
) -> Vec<Value> {
    let dir = spec_events_dir(project, spec_slug);
    let mut events = read_harness_events_from_ndjson_dir(&dir);
    // Newest first.
    events.sort_by(|a, b| b.ts.cmp(&a.ts));

    let wave_n_u32 = u32::try_from(wave_n).unwrap_or(0);
    let mut out: Vec<Value> = Vec::new();
    for ev in events {
        if ev.event != "agent.memory" {
            continue;
        }
        let payload_spec = ev.payload.get("spec").and_then(Value::as_str);
        let envelope_spec = ev.spec.as_deref();
        let spec_match = payload_spec == Some(spec_slug) || envelope_spec == Some(spec_slug);
        let payload_wave = ev.payload.get("wave").and_then(Value::as_i64);
        let envelope_wave = i64::from(ev.wave);
        let wave_match = payload_wave == Some(wave_n) || envelope_wave == i64::from(wave_n_u32);
        if !(spec_match && wave_match) {
            continue;
        }
        out.push(ev.payload);
        if out.len() >= MAX_MEMORIES_PER_WAVE {
            break;
        }
    }
    out
}

/// Render the prior-wave memories block. Returns the empty string when there
/// are no prior waves or no memory rows for any of them.
///
/// `spec` is the parent spec slug (e.g. `2026-05-21-dashboard-spec-tabs`). For
/// each `wave_name` we extract the wave number from its `wave-N-` prefix and
/// query `(spec, wave_n)`. Wave names whose prefix does not parse are skipped.
pub(crate) fn render(
    wave_names: &[String],
    project: &Path,
    spec: &str,
) -> String {
    if wave_names.is_empty() {
        return String::new();
    }
    let mut sections: Vec<String> = Vec::new();
    for name in wave_names {
        let Some(wave_n) = parse_wave_number(name) else {
            continue;
        };
        let mems = memories_for_spec_wave(project, spec, wave_n);
        if mems.is_empty() {
            continue;
        }
        let mut block = String::new();
        let _ = writeln!(block, "### [[{name}]]");
        for m in mems {
            // Prefer `summary`, fall back to a compact JSON line.
            if let Some(s) = m.get("summary").and_then(Value::as_str) {
                if !s.is_empty() {
                    let _ = writeln!(block, "- {s}");
                    continue;
                }
            }
            let compact = serde_json::to_string(&m).unwrap_or_default();
            if !compact.is_empty() {
                let _ = writeln!(block, "- {compact}");
            }
        }
        sections.push(block);
    }
    if sections.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Memórias de waves anteriores\n\n");
    out.push_str(&sections.join("\n"));
    out
}

/// Run `mustard-rt run memory cross-wave --spec <name> --wave <N>`.
///
/// Fail-open: a missing wave-plan, missing DB, or unparseable `--wave` all
/// degrade to an empty stdout body.
pub fn run(spec: Option<&str>, wave: Option<u32>) {
    let Some(spec) = spec else {
        eprintln!("Usage: memory cross-wave --spec <name> --wave <N>");
        return;
    };
    let Some(wave) = wave else {
        eprintln!("Usage: memory cross-wave --spec <name> --wave <N>");
        return;
    };
    if wave <= 1 {
        // Wave 1 has no prior waves — empty block.
        return;
    }

    let project = PathBuf::from(project_dir());
    let spec_dir = project.join(".claude").join("spec").join(spec);
    let plan_path = spec_dir.join("wave-plan.md");

    let plan_text = fs::read_to_string(&plan_path).unwrap_or_default();
    let mut all_names = parse_wave_names(&plan_text);
    // Filesystem fallback when the wave-plan table is missing/empty (e.g. the
    // plan uses an ASCII code-fence diagram instead of the canonical table).
    if all_names.is_empty() {
        all_names = parse_wave_dirs_from_fs(&spec_dir);
    }
    // Keep waves 1..N-1 (the first N-1 entries).
    let n_prior = (wave as usize).saturating_sub(1).min(all_names.len());
    let prior: Vec<String> = all_names.into_iter().take(n_prior).collect();

    let rendered = render(&prior, &project, spec);
    if !rendered.is_empty() {
        print!("{rendered}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::event_route;
    use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
    use serde_json::json;
    use tempfile::tempdir;

    /// Build an `agent.memory` fixture using the canonical envelope schema
    /// (`HarnessEvent.spec` + `HarnessEvent.wave`). `payload.pipeline` is also
    /// set to the spec slug to mirror the legacy attribution that writers
    /// currently produce.
    fn mem_event(spec: &str, wave: u32, summary: &str) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T10:00:00.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave,
            actor: Actor {
                kind: ActorKind::Agent,
                id: Some("test".to_string()),
                actor_type: None,
            },
            event: "agent.memory".to_string(),
            payload: json!({ "pipeline": spec, "summary": summary }),
            spec: Some(spec.to_string()),
        }
    }

    /// Build an `agent.memory` fixture using payload-level attribution only
    /// (`payload.spec` + `payload.wave`). The router uses the envelope's `spec`
    /// field to pick the per-spec NDJSON dir, so the envelope still carries
    /// `Some(spec)` here — the assertion is purely about the OR branch in
    /// `memories_for_spec_wave` matching `payload.spec` / `payload.wave`.
    fn mem_event_payload_only(spec: &str, wave: u32, summary: &str) -> HarnessEvent {
        HarnessEvent {
            v: SCHEMA_VERSION,
            ts: "2026-05-20T10:00:00.000Z".to_string(),
            session_id: "s-test".to_string(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Agent,
                id: Some("test".to_string()),
                actor_type: None,
            },
            event: "agent.memory".to_string(),
            payload: json!({ "spec": spec, "wave": wave, "summary": summary }),
            // Envelope still needs `spec` so the router lands the NDJSON file
            // under <project>/.claude/spec/<spec>/events/. The OR branch under
            // test still fires because `wave = 0` here (envelope-wave mismatch).
            spec: Some(spec.to_string()),
        }
    }

    #[test]
    fn parses_wave_names_from_table() {
        let plan = "\
| Wave | Spec                          | Role    |
|------|-------------------------------|---------|
| 1    | [[wave-1-rt-infra]]           | general |
| 2    | [[wave-2-skill-template]]     | general |
| 3    | [[wave-3-dashboard-graph]]    | frontend|
";
        let names = parse_wave_names(plan);
        assert_eq!(
            names,
            vec![
                "wave-1-rt-infra".to_string(),
                "wave-2-skill-template".to_string(),
                "wave-3-dashboard-graph".to_string(),
            ]
        );
    }

    #[test]
    fn strip_wikilink_rejects_non_wikilinks() {
        assert!(strip_wikilink("plain").is_none());
        assert!(strip_wikilink("[[]]").is_none());
        assert_eq!(strip_wikilink("  [[abc]] ").as_deref(), Some("abc"));
    }

    #[test]
    fn reads_prior_waves() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let cwd = project.to_str().unwrap();
        // Two memories for wave 1, one for wave 2 — all under spec "foo".
        event_route::emit(cwd, &mem_event("foo", 1, "rt infra delivered four subcommands"));
        event_route::emit(cwd, &mem_event("foo", 1, "wikilinks table created"));
        event_route::emit(cwd, &mem_event("foo", 2, "SKILLs updated"));
        // Noise: a different spec must not bleed into the rendered block.
        event_route::emit(cwd, &mem_event("other", 1, "should not appear"));

        let prior = vec![
            "wave-1-rt-infra".to_string(),
            "wave-2-skill-template".to_string(),
        ];
        let md = render(&prior, project, "foo");
        assert!(md.starts_with("## Memórias de waves anteriores"));
        assert!(md.contains("### [[wave-1-rt-infra]]"));
        assert!(md.contains("### [[wave-2-skill-template]]"));
        assert!(md.contains("rt infra delivered four subcommands"));
        assert!(md.contains("wikilinks table created"));
        assert!(md.contains("SKILLs updated"));
        assert!(!md.contains("should not appear"));
    }

    #[test]
    fn reads_prior_waves_via_spec_and_wave() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        event_route::emit(
            project.to_str().unwrap(),
            &mem_event_payload_only("foo", 1, "bar"),
        );

        let mems = memories_for_spec_wave(project, "foo", 1);
        assert_eq!(mems.len(), 1);
        assert_eq!(
            mems[0].get("summary").and_then(Value::as_str),
            Some("bar")
        );
    }

    #[test]
    fn parses_wave_dirs_from_fs_when_table_missing() {
        let dir = tempdir().unwrap();
        let spec_dir = dir.path();
        std::fs::create_dir_all(spec_dir.join("wave-1-bar")).unwrap();
        std::fs::create_dir_all(spec_dir.join("wave-2-baz")).unwrap();
        // Noise: non-wave dirs and a stray file must not appear in the result.
        std::fs::create_dir_all(spec_dir.join("review")).unwrap();
        std::fs::write(spec_dir.join("wave-plan.md"), "irrelevant").unwrap();

        let names = parse_wave_dirs_from_fs(spec_dir);
        assert_eq!(
            names,
            vec!["wave-1-bar".to_string(), "wave-2-baz".to_string()]
        );
    }

    #[test]
    fn parse_wave_number_extracts_leading_digits() {
        assert_eq!(parse_wave_number("wave-1-rt-infra"), Some(1));
        assert_eq!(parse_wave_number("wave-12-frontend"), Some(12));
        assert_eq!(parse_wave_number("wave-bar"), None);
        assert_eq!(parse_wave_number("review"), None);
    }

    #[test]
    fn render_empty_when_no_prior_waves() {
        let dir = tempdir().unwrap();
        let md = render(&[], dir.path(), "foo");
        assert!(md.is_empty());
    }

    #[test]
    fn render_empty_when_no_memories_match() {
        let dir = tempdir().unwrap();
        let md = render(&["wave-1-rt-infra".to_string()], dir.path(), "foo");
        assert!(md.is_empty());
    }
}
