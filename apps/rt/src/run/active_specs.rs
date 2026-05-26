//! `mustard-rt run active-specs` — filesystem-canonical active-spec discovery.
//!
//! Replaces the LLM-side glob/grep loop that `/mustard:spec` used to run: globs
//! `.claude/spec/*/spec.md` + `.claude/spec/*/wave-plan.md`, parses the header
//! of each, filters to `Outcome=Active` + `Stage ∈ {Analyze, Plan, Execute}`, counts
//! wave progress for wave-plans, extracts a one-line resumo, resolves short
//! aliases for parent specs, and backfills SQLite events when absent (the
//! multi-dev gap: after a `git pull`, a teammate's spec is on disk but has no
//! local events).
//!
//! ## Fail-open contract
//!
//! Every error path prints a warning on stderr and continues. The process exits
//! `0` regardless. Missing directories, unparseable headers, and SQLite failures
//! all degrade to partial results, never to a panic or non-zero exit.

use mustard_core::fs;
use mustard_core::meta;
use mustard_core::model::event::{Actor, ActorKind, HarnessEvent, SCHEMA_VERSION};
use mustard_core::store::event_store::EventSink;
use mustard_core::store::sqlite_store::SqliteEventStore;
use rusqlite::params;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Parsed header fields from a spec.md.
#[derive(Debug, Clone, Default)]
struct SpecHeader {
    stage: Option<String>,
    outcome: Option<String>,
    scope: Option<String>,
    parent: Option<String>,
    checkpoint: Option<String>,
}

/// A discovered spec candidate before filtering.
#[derive(Debug, Clone)]
struct SpecCandidate {
    name: String,
    spec_dir: PathBuf,
    spec_md: PathBuf,
    is_wave_plan: bool,
    header: SpecHeader,
}

/// Progress for a wave-plan spec.
#[derive(Debug, Clone, Serialize)]
pub struct WaveProgress {
    pub done: usize,
    pub total: usize,
}

/// One active spec entry in the final output.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveSpec {
    pub name: String,
    pub stage: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(rename = "parentAlias", skip_serializing_if = "Option::is_none")]
    pub parent_alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<WaveProgress>,
    pub resumo: String,
    pub letter: String,
    pub status: String,
}

/// Full JSON output schema.
#[derive(Debug, Serialize)]
pub struct ActiveSpecsOutput {
    pub specs: Vec<ActiveSpec>,
    #[serde(rename = "parentMap")]
    pub parent_map: HashMap<String, String>,
    #[serde(rename = "backfilledCount")]
    pub backfilled_count: usize,
}

// ---------------------------------------------------------------------------
// Header cap: only read the first 2 KiB of each spec.md to parse the header.
// ---------------------------------------------------------------------------
const HEADER_CAP: usize = 2048;

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

/// Strip `[[wikilink]]` brackets from a value, returning the inner text.
fn strip_wikilink(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")) {
        inner.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Read at most `HEADER_CAP` bytes from a file as lossy UTF-8.
fn read_header_bytes(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; HEADER_CAP];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Try to read the spec header from a `meta.json` sidecar next to `spec_file`.
///
/// Returns `None` when the sidecar is absent, unreadable, or lacks both
/// `stage` and `outcome` (the two fields the picker filter requires).
/// Delegates the IO + lenient parse to [`mustard_core::meta`] — this is
/// just the type-shape conversion from [`meta::Meta`] to the local
/// [`SpecHeader`] (picker uses a narrower projection — `phase` /
/// `isWavePlan` / `totalWaves` are not needed here).
fn parse_header_from_meta(spec_file: &Path) -> Option<SpecHeader> {
    let m = meta::read_meta_beside(spec_file)?;
    let nonempty = |opt: Option<String>| opt.filter(|s| !s.is_empty());
    let header = SpecHeader {
        stage: nonempty(m.stage),
        outcome: nonempty(m.outcome),
        scope: nonempty(m.scope),
        parent: nonempty(m.parent).map(|s| strip_wikilink(&s)),
        checkpoint: nonempty(m.checkpoint),
    };
    // Require at least stage+outcome — otherwise the sidecar is incomplete
    // and we fall through to the .md parser (which may have legacy headers).
    if header.stage.is_none() && header.outcome.is_none() {
        return None;
    }
    Some(header)
}

/// Parse the header fields, preferring `meta.json` sidecar (W3 onward),
/// with fall-back to the legacy `### Key:` markdown header.
///
/// Resolution order:
/// 1. `meta.json` next to `spec_file` — authoritative after the W3
///    `meta-sidecar` migration removed header lines from the `.md`.
/// 2. Legacy header lines in the first 2 KiB of the `.md` — kept so a
///    teammate's un-migrated spec (e.g. pulled from a feature branch)
///    still shows up in the picker.
///
/// Uses the **first occurrence** of each `### Key:` line in the `.md`
/// fallback — any duplicate further down in the body is intentionally
/// ignored (header-drift handling).
fn parse_header(path: &Path) -> SpecHeader {
    if let Some(header) = parse_header_from_meta(path) {
        return header;
    }
    let Some(text) = read_header_bytes(path) else {
        return SpecHeader::default();
    };

    let mut header = SpecHeader::default();
    let mut last_header_line = false;
    let mut past_header = false;

    for (i, line) in text.lines().enumerate() {
        if i > 35 {
            break;
        }
        let trimmed = line.trim();

        if trimmed.starts_with("### ") {
            last_header_line = true;
            past_header = false;
            // Parse the key/value
            if let Some(rest) = trimmed.strip_prefix("### ") {
                if let Some(colon_pos) = rest.find(':') {
                    let key = rest[..colon_pos].trim();
                    let val = rest[colon_pos + 1..].trim().to_string();
                    match key.to_ascii_lowercase().as_str() {
                        "stage"
                            if header.stage.is_none() && !val.is_empty() => {
                                header.stage = Some(val);
                            }
                        "outcome"
                            if header.outcome.is_none() && !val.is_empty() => {
                                header.outcome = Some(val);
                            }
                        "scope"
                            if header.scope.is_none() && !val.is_empty() => {
                                header.scope = Some(val);
                            }
                        "parent"
                            if header.parent.is_none() && !val.is_empty() => {
                                header.parent = Some(strip_wikilink(&val));
                            }
                        "checkpoint"
                            if header.checkpoint.is_none() && !val.is_empty() => {
                                header.checkpoint = Some(val);
                            }
                        _ => {}
                    }
                }
            }
        } else if last_header_line && !trimmed.is_empty() && !trimmed.starts_with('#') {
            // Non-header content after seeing header lines: header block has ended.
            past_header = true;
        }

        if past_header {
            break;
        }
    }

    header
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Discover root-level spec candidates.
///
/// Globs `.claude/spec/*/spec.md` and `.claude/spec/*/wave-plan.md`. Excludes
/// paths that are inside wave subdirectories (contain `/wave-N-*/`) or inside
/// `review/` or `qa/` subdirs. Only spec.md or wave-plan.md at the root of a
/// named spec directory are included.
fn discover_root_specs(root: &Path) -> Vec<SpecCandidate> {
    let spec_root = root.join(".claude").join("spec");
    let Ok(entries) = fs::read_dir(&spec_root) else {
        return Vec::new();
    };

    let mut candidates: Vec<SpecCandidate> = Vec::new();

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        let name = entry.file_name.clone();
        // Skip wave-N-*, review/, qa/ directories at the top level
        // (these are subdirs of spec parents, not top-level spec names).
        // At the top spec level, all entries are valid spec names.
        let dir_path = &entry.path;

        // Check for spec.md first
        let spec_md = dir_path.join("spec.md");
        let wave_plan_md = dir_path.join("wave-plan.md");

        let (use_path, is_wave_plan) = if wave_plan_md.is_file() {
            (wave_plan_md.clone(), true)
        } else if spec_md.is_file() {
            (spec_md.clone(), false)
        } else {
            continue;
        };

        // Parse header from spec.md always (the canonical state header lives there)
        let header_path = if spec_md.is_file() { &spec_md } else { &use_path };
        let header = parse_header(header_path);

        candidates.push(SpecCandidate {
            name,
            spec_dir: dir_path.clone(),
            spec_md: spec_md.clone(),
            is_wave_plan,
            header,
        });
    }

    candidates
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Keep only specs with `Outcome=Active` and `Stage ∈ {Analyze, Plan, Execute}`.
///
/// `Analyze` is included so a spec parked pre-Plan (e.g. a tactical fix with AC
/// drafted but not yet promoted) still shows up in the picker — otherwise it
/// disappears from `/mustard:spec` despite being on-disk and `Outcome=Active`.
fn filter_active(candidates: Vec<SpecCandidate>) -> Vec<SpecCandidate> {
    candidates
        .into_iter()
        .filter(|c| {
            let outcome_ok = c
                .header
                .outcome
                .as_deref()
                .is_some_and(|o| o.eq_ignore_ascii_case("active"));
            let stage_ok = c
                .header
                .stage
                .as_deref()
                .is_some_and(|s| {
                    let lower = s.to_ascii_lowercase();
                    lower == "analyze" || lower == "plan" || lower == "execute"
                });
            outcome_ok && stage_ok
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Wave progress
// ---------------------------------------------------------------------------

/// Count completed waves for a wave-plan spec.
///
/// Globs `<spec_dir>/wave-N-*/spec.md`, reads each header, counts waves with
/// `Stage=Close` AND `Outcome=Completed`. Returns `Some((done, total))` when
/// there is at least one wave subdir, `None` otherwise.
fn count_wave_progress(spec_dir: &Path) -> Option<WaveProgress> {
    let Ok(entries) = fs::read_dir(spec_dir) else {
        return None;
    };

    let mut total = 0usize;
    let mut done = 0usize;

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        // Must match wave-N-* pattern
        if !entry.file_name.starts_with("wave-") {
            continue;
        }
        // Skip review/ qa/ (they don't start with wave-)
        let wave_spec = entry.path.join("spec.md");
        if !wave_spec.is_file() {
            continue;
        }
        // Verify it's a "wave-N-" directory (has digits after "wave-")
        let after_wave = &entry.file_name["wave-".len()..];
        if !after_wave.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        total += 1;
        let hdr = parse_header(&wave_spec);
        let stage_close = hdr
            .stage
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("close"));
        let outcome_completed = hdr
            .outcome
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case("completed"));
        if stage_close && outcome_completed {
            done += 1;
        }
    }

    if total == 0 {
        None
    } else {
        Some(WaveProgress { done, total })
    }
}

// ---------------------------------------------------------------------------
// Resumo extraction
// ---------------------------------------------------------------------------

/// Strip markdown bold/italic markers (`**`, `*`, `__`, `_`) and wikilinks
/// (`[[X]]` → `X`) from a string.
fn strip_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Strip [[wikilink]]
        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(end) = s[i + 2..].find("]]") {
                let inner = &s[i + 2..i + 2 + end];
                out.push_str(inner.trim());
                i += 2 + end + 2;
                continue;
            }
        }
        // Strip ** or __
        if i + 1 < bytes.len()
            && ((bytes[i] == b'*' && bytes[i + 1] == b'*')
                || (bytes[i] == b'_' && bytes[i + 1] == b'_'))
        {
            i += 2;
            continue;
        }
        // Strip * or _
        if bytes[i] == b'*' || bytes[i] == b'_' {
            i += 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Truncate a string to at most `max_chars` Unicode characters, appending `…`
/// when truncated. (Characters, not bytes, to avoid splitting multibyte chars.)
fn truncate_str(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = chars[..max_chars].iter().collect();
        format!("{truncated}…")
    }
}

/// Extract a one-line summary (≤70 chars) from a spec.md.
///
/// Priority: `## Resumo` > `## Contexto` > `## Summary` > `## Context`.
/// Takes the first non-empty line after the heading, then truncates at the
/// first sentence break (`.`, `\n\n`, or `:`). Strips wikilinks and markdown
/// bold/italic. Truncates to 70 chars with `…`.
fn extract_resumo(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else { return String::new() };

    // Try headings in priority order
    let headings = ["## Resumo", "## Contexto", "## Summary", "## Context"];
    for heading in headings {
        if let Some(first) = find_section_first_line(&text, heading) {
            if first.is_empty() {
                continue;
            }
            // Truncate at first sentence break
            let sentence = first_sentence(&first);
            let cleaned = strip_markdown(&sentence);
            let cleaned = cleaned.trim().to_string();
            if !cleaned.is_empty() {
                return truncate_str(&cleaned, 70);
            }
        }
    }
    String::new()
}

/// Find the first non-empty line after a markdown section heading.
/// Scans the document for a line that starts with `heading` (case-insensitive),
/// then returns the first non-empty line that follows it.
fn find_section_first_line(text: &str, heading: &str) -> Option<String> {
    let heading_lower = heading.to_ascii_lowercase();
    let mut found_heading = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if !found_heading {
            if trimmed.to_ascii_lowercase().starts_with(&heading_lower) {
                found_heading = true;
            }
            continue;
        }
        // We are past the heading: return the first non-empty, non-heading line
        if trimmed.is_empty() {
            continue;
        }
        // Stop at the next heading
        if trimmed.starts_with('#') {
            return None;
        }
        return Some(trimmed.to_string());
    }
    None
}

/// Extract the first sentence from a string (until `.`, `:`, or a blank line).
fn first_sentence(s: &str) -> String {
    // Stop at double newline
    let s = if let Some(idx) = s.find("\n\n") {
        &s[..idx]
    } else {
        s
    };
    // Stop at first period or colon (keeping the character for period)
    let mut result = String::new();
    for ch in s.chars() {
        if ch == '.' {
            result.push(ch);
            break;
        }
        if ch == ':' {
            break;
        }
        result.push(ch);
    }
    result.trim().to_string()
}

// ---------------------------------------------------------------------------
// Parent alias resolution
// ---------------------------------------------------------------------------

/// Generate a short alias for each parent spec slug.
///
/// Algorithm: take the last hyphen-separated "word" of the slug, then the
/// first 2 chars. In case of collision, try the last 2 words, then add chars.
/// Guarantees uniqueness.
fn resolve_parent_aliases(parents: &[String]) -> HashMap<String, String> {
    let mut alias_map: HashMap<String, String> = HashMap::new();
    let mut used: HashMap<String, String> = HashMap::new();

    for parent in parents {
        let alias = make_unique_alias(parent, &used);
        used.insert(alias.clone(), parent.clone());
        alias_map.insert(parent.clone(), alias);
    }
    alias_map
}

/// Make a unique alias for `slug` that doesn't collide with anything already
/// in `used` (a map from alias → slug).
fn make_unique_alias(slug: &str, used: &HashMap<String, String>) -> String {
    let words: Vec<&str> = slug.split('-').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return slug.chars().take(2).collect();
    }

    // Skip common date-like prefixes (YYYY)
    let significant: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
        .collect();

    if significant.is_empty() {
        return make_unique_from_chars(slug, used);
    }

    // Strategy 1: first 2 chars of the last significant word
    let last = *significant.last().unwrap_or(&words[words.len() - 1]);
    let candidate = last.chars().take(2).collect::<String>();
    if !used.contains_key(&candidate) {
        return candidate;
    }

    // Strategy 2: first char of second-to-last + first char of last
    if significant.len() >= 2 {
        let second_last = significant[significant.len() - 2];
        let candidate2 = format!(
            "{}{}",
            second_last.chars().next().unwrap_or('_'),
            last.chars().next().unwrap_or('_')
        );
        if !used.contains_key(&candidate2) {
            return candidate2;
        }
    }

    // Strategy 3: initials of last N significant words
    for n in 2..=significant.len() {
        let initials: String = significant[significant.len() - n..]
            .iter()
            .filter_map(|w| w.chars().next())
            .collect();
        if !used.contains_key(&initials) {
            return initials;
        }
    }

    // Strategy 4: keep extending the last word
    make_unique_from_chars(last, used)
}

fn make_unique_from_chars(s: &str, used: &HashMap<String, String>) -> String {
    for n in 2..=s.len().max(2) {
        let candidate: String = s.chars().take(n).collect();
        if !used.contains_key(&candidate) {
            return candidate;
        }
    }
    // Absolute fallback: hash-like suffix
    format!("{}{}", &s.chars().take(2).collect::<String>(), used.len())
}

// ---------------------------------------------------------------------------
// SQLite backfill
// ---------------------------------------------------------------------------

/// For each spec that has no `pipeline.stage` or `pipeline.status` events in
/// the store, emit synthetic backfill events derived from the spec header.
///
/// Returns the count of specs that were backfilled.
///
/// Fail-open: any store error is printed to stderr; 0 backfills reported.
fn backfill_sqlite(specs: &[SpecCandidate], root: &Path) -> usize {
    let store = match SqliteEventStore::for_project(root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("active-specs: WARN: cannot open event store for backfill: {e}");
            return 0;
        }
    };

    let mut backfilled = 0usize;
    let sid = crate::run::env::session_id();

    for spec in specs {
        let has_events = match has_pipeline_events(store.conn(), &spec.name) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "active-specs: WARN: cannot query events for {}: {e}",
                    spec.name
                );
                continue;
            }
        };

        if has_events {
            continue;
        }

        // Determine timestamp: Checkpoint header > mtime of spec.md > now
        let ts = determine_timestamp(&spec.header.checkpoint, &spec.spec_md);
        let stage = spec.header.stage.clone().unwrap_or_else(|| "Plan".to_string());

        // Emit pipeline.stage
        let stage_event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.clone(),
            session_id: sid.clone(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("active-specs-backfill".to_string()),
                actor_type: None,
            },
            event: "pipeline.stage".to_string(),
            payload: json!({
                "stage": stage.to_ascii_lowercase(),
                "source": "backfill-from-filesystem"
            }),
            spec: Some(spec.name.clone()),
        };
        let _ = store.append(&stage_event);

        // Emit pipeline.status
        let status_event = HarnessEvent {
            v: SCHEMA_VERSION,
            ts: ts.clone(),
            session_id: sid.clone(),
            wave: 0,
            actor: Actor {
                kind: ActorKind::Orchestrator,
                id: Some("active-specs-backfill".to_string()),
                actor_type: None,
            },
            event: "pipeline.status".to_string(),
            payload: json!({
                "to": "approved",
                "source": "backfill-from-filesystem"
            }),
            spec: Some(spec.name.clone()),
        };
        let _ = store.append(&status_event);

        backfilled += 1;
    }

    backfilled
}

/// Returns `true` if the spec already has at least one `pipeline.stage` or
/// `pipeline.status` event in the store. W5: lifecycle rows live in
/// `pipeline_events` (column `kind`), not in the retired `events` table.
fn has_pipeline_events(
    conn: &rusqlite::Connection,
    spec_name: &str,
) -> rusqlite::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pipeline_events \
         WHERE spec = ?1 \
         AND kind IN ('pipeline.stage','pipeline.status') \
         LIMIT 1",
        params![spec_name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Determine the best timestamp for a backfill event.
///
/// Priority: `### Checkpoint:` header value (if valid ISO-ish) > mtime of
/// `spec.md` > current time.
fn determine_timestamp(checkpoint: &Option<String>, spec_md: &Path) -> String {
    // Try checkpoint
    if let Some(cp) = checkpoint.as_deref() {
        let cp = cp.trim();
        // Accept anything that looks like YYYY-MM-DDT...
        if cp.len() >= 10 && cp.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return cp.to_string();
        }
    }
    // Try mtime
    if let Ok(meta) = std::fs::metadata(spec_md) {
        if let Ok(mtime) = meta.modified() {
            use std::time::{Duration, UNIX_EPOCH};
            let dur = mtime.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
            let secs = dur.as_secs();
            let millis = dur.subsec_millis();
            // Use the same algorithm as util::now_iso8601
            return format_unix_ts(secs, millis);
        }
    }
    // Fallback to now
    crate::util::now_iso8601()
}

/// Format a Unix timestamp (secs + millis) as ISO-8601 UTC string.
fn format_unix_ts(secs: u64, millis: u32) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{millis:03}Z")
}

// ---------------------------------------------------------------------------
// Status derivation
// ---------------------------------------------------------------------------

/// Derive the display status for a spec row in the table.
fn derive_status(spec: &SpecCandidate, parent_aliases: &HashMap<String, String>) -> String {
    // Tactical fix: has a parent → TF→{alias}
    if let Some(parent) = &spec.header.parent {
        if !parent.is_empty() {
            let alias = parent_aliases
                .get(parent)
                .cloned()
                .unwrap_or_else(|| parent.chars().take(2).collect());
            return format!("TF→{alias}");
        }
    }
    // Wave plan with active waves: derive "W{N} em exec" for the first active wave
    if spec.is_wave_plan {
        if let Some(first_active_wave) = find_first_active_wave(&spec.spec_dir) {
            return format!("W{first_active_wave} em exec");
        }
    }
    "-".to_string()
}

/// Find the number of the first wave-N subdir that has `Outcome=Active`.
fn find_first_active_wave(spec_dir: &Path) -> Option<String> {
    let Ok(entries) = fs::read_dir(spec_dir) else {
        return None;
    };

    let mut active_waves: Vec<(u32, String)> = Vec::new();

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        if !entry.file_name.starts_with("wave-") {
            continue;
        }
        let after_wave = &entry.file_name["wave-".len()..];
        if !after_wave.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        // Extract wave number
        let num_str: String = after_wave.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(num) = num_str.parse::<u32>() else {
            continue;
        };
        let wave_spec = entry.path.join("spec.md");
        if !wave_spec.is_file() {
            continue;
        }
        let hdr = parse_header(&wave_spec);
        let outcome_active = hdr
            .outcome
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case("active"));
        if outcome_active {
            active_waves.push((num, entry.file_name.clone()));
        }
    }

    active_waves.sort_by_key(|(n, _)| *n);
    active_waves.first().map(|(n, _)| n.to_string())
}

// ---------------------------------------------------------------------------
// Scope abbreviation
// ---------------------------------------------------------------------------

fn scope_abbrev(scope: &Option<String>) -> String {
    match scope.as_deref() {
        Some(s) => {
            let lower = s.to_ascii_lowercase();
            if lower.starts_with("light") {
                "lt".to_string()
            } else if lower.starts_with("full") {
                "fl".to_string()
            } else {
                "-".to_string()
            }
        }
        None => "-".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Stage abbreviation
// ---------------------------------------------------------------------------

fn stage_abbrev(stage: &str) -> String {
    match stage.to_ascii_lowercase().as_str() {
        "analyze" => "ANLZ".to_string(),
        "plan" => "PLAN".to_string(),
        "execute" => "EXEC".to_string(),
        other => other.to_ascii_uppercase().chars().take(4).collect(),
    }
}

// ---------------------------------------------------------------------------
// Table rendering
// ---------------------------------------------------------------------------

/// Generate a markdown table from the list of active specs.
///
/// Columns: `#`, `Spec`, `Esc`, `Estágio`, `Prog`, `Status`, `Resumo`
fn render_table(specs: &[ActiveSpec]) -> String {
    // Column headers
    let header = "| #  | Spec                                          | Esc | Estágio | Prog | Status     | Resumo";
    let separator = "|----|-----------------------------------------------|-----|---------|------|------------|-----------------------------------------------------|";

    let mut lines: Vec<String> = Vec::new();
    lines.push(header.to_string());
    lines.push(separator.to_string());

    for spec in specs {
        let prog = spec
            .progress
            .as_ref()
            .map_or_else(|| " - ".to_string(), |p| format!("{}/{}", p.done, p.total));

        let scope_str = spec
            .scope
            .as_ref()
            .map_or_else(|| "-".to_string(), |s| scope_abbrev(&Some(s.clone())));

        let stage_str = stage_abbrev(&spec.stage);

        // Pad/truncate columns for alignment
        let letter = format!("{:<2}", spec.letter);
        let name = format!("{:<45}", &spec.name);
        let esc = format!("{scope_str:<3}");
        let stage_col = format!("{stage_str:<7}");
        let prog_col = format!("{prog:>4}");
        let status_col = format!("{:<10}", spec.status);
        let resumo_col = &spec.resumo;

        lines.push(format!(
            "| {letter} | {name} | {esc} | {stage_col} | {prog_col} | {status_col} | {resumo_col}"
        ));
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// JSON rendering
// ---------------------------------------------------------------------------

/// Serialize the output to pretty JSON.
fn render_json(output: &ActiveSpecsOutput) -> String {
    serde_json::to_string_pretty(output).unwrap_or_else(|_| r#"{"specs":[],"parentMap":{},"backfilledCount":0}"#.to_string())
}

// ---------------------------------------------------------------------------
// Date parsing for sort
// ---------------------------------------------------------------------------

/// Extract the `YYYY-MM-DD` prefix from a spec name for date-descending sort.
/// Returns `"0000-00-00"` for names that don't start with a date.
fn spec_date_prefix(name: &str) -> &str {
    if name.len() >= 10 && name.chars().nth(4) == Some('-') && name.chars().nth(7) == Some('-') {
        &name[..10]
    } else {
        "0000-00-00"
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Options for `mustard-rt run active-specs`.
pub struct ActiveSpecsOpts {
    /// Output format: `table` (default) or `json`.
    pub format: String,
    /// Project root directory (default: cwd).
    pub root: PathBuf,
    /// When `true`, skip the SQLite backfill step.
    pub no_backfill: bool,
}

/// Main entry point for `mustard-rt run active-specs`.
pub fn run(opts: ActiveSpecsOpts) {
    let root = &opts.root;

    // 1. Discover all root-level spec.md / wave-plan.md
    let mut candidates = discover_root_specs(root);

    // 2. Parse headers and filter to active specs
    candidates = filter_active(candidates);

    // 3. Sort by date descending (newest first)
    candidates.sort_by(|a, b| {
        spec_date_prefix(&b.name).cmp(spec_date_prefix(&a.name))
    });

    // 4. Backfill SQLite unless --no-backfill
    let backfilled_count = if opts.no_backfill {
        0
    } else {
        backfill_sqlite(&candidates, root)
    };

    // 5. Collect all unique parents for alias resolution
    let parents: Vec<String> = candidates
        .iter()
        .filter_map(|c| c.header.parent.clone())
        .filter(|p| !p.is_empty())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    // Sort for deterministic alias assignment
    let mut parents_sorted = parents.clone();
    parents_sorted.sort();
    let parent_aliases = resolve_parent_aliases(&parents_sorted);

    // 6. Build ActiveSpec entries (capped at 26)
    let letters: Vec<char> = ('a'..='z').collect();
    let mut specs: Vec<ActiveSpec> = Vec::new();
    let cap = candidates.len().min(26);

    for (i, candidate) in candidates.iter().enumerate().take(cap) {
        let letter = letters[i].to_string();
        let stage = candidate
            .header
            .stage
            .clone()
            .unwrap_or_else(|| "Plan".to_string());
        let outcome = candidate
            .header
            .outcome
            .clone()
            .unwrap_or_else(|| "Active".to_string());

        // Scope — wave plans are always "fl"
        let scope = if candidate.is_wave_plan {
            Some("full".to_string())
        } else {
            candidate.header.scope.clone().map(|s| {
                // Normalize: strip extra annotations like "full (wave N of M)"
                let lower = s.to_ascii_lowercase();
                if lower.starts_with("full") {
                    "full".to_string()
                } else if lower.starts_with("light") {
                    "light".to_string()
                } else {
                    s
                }
            })
        };

        let parent = candidate.header.parent.clone().filter(|p| !p.is_empty());
        let parent_alias = parent
            .as_ref()
            .and_then(|p| parent_aliases.get(p))
            .cloned();

        let progress = if candidate.is_wave_plan {
            count_wave_progress(&candidate.spec_dir)
        } else {
            None
        };

        let resumo = if candidate.spec_md.is_file() {
            extract_resumo(&candidate.spec_md)
        } else {
            String::new()
        };

        let status = derive_status(candidate, &parent_aliases);

        specs.push(ActiveSpec {
            name: candidate.name.clone(),
            stage,
            outcome,
            scope,
            parent,
            parent_alias,
            progress,
            resumo,
            letter,
            status,
        });
    }

    // 7. Note if there were more than 26 specs
    let extra = candidates.len().saturating_sub(26);

    // 8. Emit output
    let output = ActiveSpecsOutput {
        specs,
        parent_map: parent_aliases.into_iter().map(|(k, v)| (v, k)).collect(),
        backfilled_count,
    };

    match opts.format.as_str() {
        "json" => {
            println!("{}", render_json(&output));
        }
        _ => {
            // table (default)
            println!("{}", render_table(&output.specs));
            if extra > 0 {
                println!("\n({extra} specs adicionais)");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::store::sqlite_store::SqliteEventStore;
    use tempfile::tempdir;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_spec_dir(root: &Path, name: &str, body: &str) {
        let dir = root.join(".claude").join("spec").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("spec.md"), body).unwrap();
    }

    fn make_wave_spec(root: &Path, parent: &str, wave: &str, stage: &str, outcome: &str) {
        let dir = root
            .join(".claude")
            .join("spec")
            .join(parent)
            .join(wave);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("spec.md"),
            format!("# Wave {wave}\n\n### Stage: {stage}\n### Outcome: {outcome}\n"),
        )
        .unwrap();
    }

    // -----------------------------------------------------------------------
    // parse_header tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_header_all_four_fields() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n### Stage: Execute\n### Outcome: Active\n### Scope: light\n### Parent: my-parent\n\n## Body\n",
        )
        .unwrap();
        let h = parse_header(&path);
        assert_eq!(h.stage.as_deref(), Some("Execute"));
        assert_eq!(h.outcome.as_deref(), Some("Active"));
        assert_eq!(h.scope.as_deref(), Some("light"));
        assert_eq!(h.parent.as_deref(), Some("my-parent"));
    }

    #[test]
    fn parse_header_strips_wikilink_from_parent() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n### Stage: Plan\n### Parent: [[my-parent-slug]]\n",
        )
        .unwrap();
        let h = parse_header(&path);
        assert_eq!(h.parent.as_deref(), Some("my-parent-slug"));
    }

    #[test]
    fn parse_header_uses_first_occurrence_on_drift() {
        // spec that has duplicate Stage header (body drift)
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n### Stage: Close\n### Outcome: Completed\n\n## Body\n\n### Stage: Execute\n### Outcome: Active\n",
        )
        .unwrap();
        let h = parse_header(&path);
        // Must use Close (first occurrence), NOT Execute
        assert_eq!(h.stage.as_deref(), Some("Close"));
        assert_eq!(h.outcome.as_deref(), Some("Completed"));
    }

    #[test]
    fn parse_header_missing_file_returns_default() {
        let h = parse_header(Path::new("/nonexistent/path/spec.md"));
        assert!(h.stage.is_none());
        assert!(h.outcome.is_none());
    }

    // -----------------------------------------------------------------------
    // filter_active tests
    // -----------------------------------------------------------------------

    fn make_candidate(name: &str, stage: &str, outcome: &str) -> SpecCandidate {
        SpecCandidate {
            name: name.to_string(),
            spec_dir: PathBuf::from(format!(".claude/spec/{name}")),
            spec_md: PathBuf::from(format!(".claude/spec/{name}/spec.md")),
            is_wave_plan: false,
            header: SpecHeader {
                stage: Some(stage.to_string()),
                outcome: Some(outcome.to_string()),
                scope: None,
                parent: None,
                checkpoint: None,
            },
        }
    }

    #[test]
    fn filter_active_keeps_analyze_plan_and_execute_with_active_outcome() {
        let candidates = vec![
            make_candidate("a", "Plan", "Active"),
            make_candidate("b", "Execute", "Active"),
            make_candidate("c", "Close", "Completed"),
            make_candidate("d", "Plan", "Completed"),
            make_candidate("e", "Close", "Active"),
            make_candidate("f", "Analyze", "Active"),
            make_candidate("g", "Analyze", "Completed"),
        ];
        let active = filter_active(candidates);
        let names: Vec<&str> = active.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"a"), "Plan+Active should pass");
        assert!(names.contains(&"b"), "Execute+Active should pass");
        assert!(!names.contains(&"c"), "Close+Completed should be filtered");
        assert!(!names.contains(&"d"), "Plan+Completed should be filtered");
        assert!(!names.contains(&"e"), "Close+Active should be filtered");
        assert!(names.contains(&"f"), "Analyze+Active should pass");
        assert!(!names.contains(&"g"), "Analyze+Completed should be filtered");
    }

    // -----------------------------------------------------------------------
    // count_wave_progress tests
    // -----------------------------------------------------------------------

    #[test]
    fn count_wave_progress_four_close_two_plan() {
        let td = tempdir().unwrap();
        let parent = "my-wave-spec";
        // 4 closed waves
        for i in 1..=4 {
            make_wave_spec(td.path(), parent, &format!("wave-{i}-impl"), "Close", "Completed");
        }
        // 2 active waves
        make_wave_spec(td.path(), parent, "wave-5-impl", "Plan", "Active");
        make_wave_spec(td.path(), parent, "wave-6-impl", "Plan", "Active");

        let spec_dir = td.path().join(".claude").join("spec").join(parent);
        let progress = count_wave_progress(&spec_dir).expect("should have progress");
        assert_eq!(progress.done, 4);
        assert_eq!(progress.total, 6);
    }

    #[test]
    fn count_wave_progress_none_when_no_waves() {
        let td = tempdir().unwrap();
        let spec_dir = td.path().join(".claude").join("spec").join("no-waves");
        std::fs::create_dir_all(&spec_dir).unwrap();
        assert!(count_wave_progress(&spec_dir).is_none());
    }

    // -----------------------------------------------------------------------
    // extract_resumo tests
    // -----------------------------------------------------------------------

    #[test]
    fn extract_resumo_prefers_resumo_over_contexto() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n## Resumo\n\nPrimeira frase do resumo.\n\n## Contexto\n\nDeve ser ignorado.\n",
        )
        .unwrap();
        let r = extract_resumo(&path);
        assert!(r.contains("Primeira frase"), "got: {r:?}");
        assert!(!r.contains("Deve ser ignorado"), "got: {r:?}");
    }

    #[test]
    fn extract_resumo_falls_back_to_contexto() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        std::fs::write(
            &path,
            "# Title\n\n## Contexto\n\nContexto do spec aqui.\n",
        )
        .unwrap();
        let r = extract_resumo(&path);
        assert!(r.contains("Contexto"), "got: {r:?}");
    }

    #[test]
    fn extract_resumo_truncates_at_70_chars() {
        let td = tempdir().unwrap();
        let path = td.path().join("spec.md");
        let long_text = "A".repeat(100);
        std::fs::write(&path, format!("# Title\n\n## Resumo\n\n{long_text}\n")).unwrap();
        let r = extract_resumo(&path);
        // Should be truncated to 70 chars + ellipsis
        let char_count = r.chars().count();
        // The last char should be the ellipsis "…" (1 char) after 70 chars
        assert!(char_count <= 72, "expected ≤72 chars (70 + ellipsis), got {char_count}: {r:?}");
        assert!(r.ends_with('…'), "expected ellipsis suffix, got: {r:?}");
    }

    // -----------------------------------------------------------------------
    // resolve_parent_aliases tests
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_parent_aliases_unique_for_different_last_tokens() {
        let parents = vec![
            "2026-05-23-dashboard-design-system".to_string(),
            "2026-05-23-flatten-spec-layout-and-multi-collab".to_string(),
        ];
        let map = resolve_parent_aliases(&parents);
        let aliases: Vec<&String> = map.values().collect();
        // All aliases must be unique
        let unique: std::collections::HashSet<&&String> = aliases.iter().collect();
        assert_eq!(unique.len(), aliases.len(), "aliases must be unique: {map:?}");
    }

    #[test]
    fn resolve_parent_aliases_no_collisions_with_many_parents() {
        let parents: Vec<String> = (0..10)
            .map(|i| format!("2026-05-23-spec-number-{i:02}"))
            .collect();
        let map = resolve_parent_aliases(&parents);
        let aliases: Vec<&String> = map.values().collect();
        let unique: std::collections::HashSet<&&String> = aliases.iter().collect();
        assert_eq!(unique.len(), aliases.len(), "all aliases unique: {map:?}");
    }

    // -----------------------------------------------------------------------
    // backfill SQLite idempotency test
    // -----------------------------------------------------------------------

    #[test]
    fn backfill_sqlite_idempotent_second_call_emits_zero() {
        let td = tempdir().unwrap();
        // Create a spec on disk
        make_spec_dir(
            td.path(),
            "my-test-spec",
            "# Test\n\n### Stage: Plan\n### Outcome: Active\n",
        );
        let candidates = vec![SpecCandidate {
            name: "my-test-spec".to_string(),
            spec_dir: td.path().join(".claude").join("spec").join("my-test-spec"),
            spec_md: td
                .path()
                .join(".claude")
                .join("spec")
                .join("my-test-spec")
                .join("spec.md"),
            is_wave_plan: false,
            header: SpecHeader {
                stage: Some("Plan".to_string()),
                outcome: Some("Active".to_string()),
                scope: None,
                parent: None,
                checkpoint: None,
            },
        }];

        // First backfill: should insert 2 events for 1 spec
        let count1 = backfill_sqlite(&candidates, td.path());
        assert_eq!(count1, 1, "first backfill should process 1 spec");

        // Second backfill: events already present, should be idempotent
        let count2 = backfill_sqlite(&candidates, td.path());
        assert_eq!(count2, 0, "second backfill must be idempotent (0 new)");

        // Verify the store has exactly 2 events for this spec
        let store = SqliteEventStore::for_project(td.path()).unwrap();
        let events = store.query(Some("my-test-spec")).unwrap();
        assert_eq!(events.len(), 2, "exactly 2 backfill events");
    }

    // -----------------------------------------------------------------------
    // strip_markdown tests
    // -----------------------------------------------------------------------

    #[test]
    fn strip_markdown_removes_bold_and_wikilinks() {
        let input = "This is **bold** and [[a-wikilink]] text";
        let result = strip_markdown(input);
        assert_eq!(result, "This is bold and a-wikilink text");
    }

    // -----------------------------------------------------------------------
    // spec_date_prefix tests
    // -----------------------------------------------------------------------

    #[test]
    fn spec_date_prefix_extracts_date() {
        assert_eq!(spec_date_prefix("2026-05-23-my-spec"), "2026-05-23");
        assert_eq!(spec_date_prefix("no-date-here"), "0000-00-00");
    }
}
