//! `mustard-rt run scope-decompose` — a port of `scripts/scope-decompose.js`.
//!
//! Decides whether a feature spec should be decomposed into **multiple** waves.
//!
//! ## Semantics: 1-vs-N, never 0-vs-≥1
//!
//! For a **Full**-scope spec the [`decide`] verdict means "MULTI-wave (N) vs
//! SINGLE-wave (1)" — it is **not** "wave vs no-wave". The invariant (encoded in
//! [`mustard_core::domain::spec::contract::ContractViolation::FullScopeNoWaves`])
//! is that every Full spec has ≥1 wave: the parent spec is the *orchestrator*,
//! the wave is the executing *subagent*. So `decompose: false` for a Full spec
//! means **one** wave, never zero. Callers map the verdict to a wave count via
//! [`wave_floor_for_full`], which floors a Full spec at 1. (Light is unchanged
//! — a single spec with an inline checklist, no waves.)
//!
//! ## Two input paths
//!
//! 1. **stdin (legacy / override).** Reads a signals JSON object
//!    (`fileCount` / `layerCount` / `newEntityCount` / `estimatedTouchPoints` /
//!    `knowledgeMatches` / `text`) and decides. The caller (a SKILL / the LLM)
//!    pre-computes the counts.
//! 2. **`--from-spec <path>` (deterministic, F5-a item 1).** Computes the
//!    structural signals **in Rust** from the spec itself — no LLM glob/grep:
//!    - `fileCount` / `layerCount` from the spec's `## Files` section via
//!      [`crate::commands::wave::wave_lib::parse_files_section`] +
//!      [`crate::commands::wave::wave_lib::detect_role_with`] (the same
//!      classifier the wave gates use, with `mustard.json#rolePatterns`
//!      overrides);
//!    - `newEntityCount` by **diffing the repo model**: the count of
//!      Create-marked `## Files` bullets (`(create)`/`(new)`/`(novo)`/`(criar)`)
//!      corroborated by a PascalCase prose token that is *not yet* among the
//!      known declaration names of `.claude/grain.model.json` (read via the
//!      scan tool's `facts` command — this crate never parses the model's
//!      schema itself). See [`count_new_entities`]. Prose capitalization alone
//!      — a sentence-initial word, a camelCase split fragment (`GraphQL` →
//!      `Graph`+`QL`) — never counts as an entity.
//!
//!    The spec body is still passed as `text` so the roadmap-signal detection
//!    runs identically. The result is the same [`decide`] verdict the stdin path
//!    would emit for the equivalent signals.
//!
//! Fail-open: any error emits `{ "decompose": false, "reason": "error-fallback" }`.

use crate::commands::spec::prd_build::pascal_tokens;
use crate::commands::spec::spec_sections::is_heading;
use crate::commands::wave::wave_lib::{detect_role_with, load_role_patterns, parse_files_section};
use mustard_core::platform::i18n::{line_has_file_marker, FileMarker};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Roadmap-signal detection across spec text.
struct RoadmapSignal {
    hit: bool,
    matches: Vec<String>,
}

/// Detect roadmap signals in free-text. Mirrors the three JS regex patterns.
fn detect_roadmap_signal(text: &str) -> RoadmapSignal {
    let mut matches: Vec<String> = Vec::new();

    // plans-ref: `\.claude/plans/[^\s"'`)\]]+\.md`
    let lower = text;
    let mut search = 0;
    while let Some(rel) = lower[search..].find(".claude/plans/") {
        let at = search + rel;
        let after = &lower[at..];
        let end = after
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ')' | ']'))
            .unwrap_or(after.len());
        let token = &after[..end];
        if token.ends_with(".md") {
            matches.push(format!("plans-ref:{token}"));
        }
        search = at + ".claude/plans/".len();
    }

    // wave-numbered: `\b(Wave|W|Etapa|Fase|Phase)\s*\d+\b` (case-insensitive).
    for kw in ["wave", "etapa", "fase", "phase", "w"] {
        find_keyword_number(text, kw, "wave-numbered", &mut matches);
    }

    // roadmap-keyword: `\b(roadmap|multi[-\s]?wave)\b` (case-insensitive).
    let tl = text.to_lowercase();
    for (idx, _) in tl.match_indices("roadmap") {
        if word_boundary(&tl, idx, idx + 7) {
            matches.push(format!("roadmap-keyword:{}", &text[idx..idx + 7]));
        }
    }
    for needle in ["multi-wave", "multi wave", "multiwave"] {
        for (idx, _) in tl.match_indices(needle) {
            if word_boundary(&tl, idx, idx + needle.len()) {
                matches.push(format!(
                    "roadmap-keyword:{}",
                    &text[idx..idx + needle.len()]
                ));
            }
        }
    }

    let has_plans_ref = matches.iter().any(|m| m.starts_with("plans-ref:"));
    let other_hits = matches.iter().filter(|m| !m.starts_with("plans-ref:")).count();
    RoadmapSignal {
        hit: has_plans_ref || other_hits >= 2,
        matches,
    }
}

/// Find `<keyword>\s*<digits>` occurrences with word boundaries.
fn find_keyword_number(text: &str, keyword: &str, label: &str, out: &mut Vec<String>) {
    let tl = text.to_lowercase();
    let mut search = 0;
    while let Some(rel) = tl[search..].find(keyword) {
        let at = search + rel;
        let kw_end = at + keyword.len();
        // `\b` before the keyword.
        let boundary_before = at == 0
            || !is_word_char(tl.as_bytes()[at - 1] as char);
        if boundary_before {
            let after = &tl[kw_end..];
            let ws = after.len() - after.trim_start_matches([' ', '\t']).len();
            let digits_part = &after[ws..];
            let dig_end = digits_part
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(digits_part.len());
            if dig_end > 0 {
                let matched = &text[at..kw_end + ws + dig_end];
                out.push(format!("{label}:{matched}"));
            }
        }
        search = kw_end;
    }
}

/// Whether a char is a JS `\w` word character.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether `[start, end)` of `s` is bounded by non-word chars.
fn word_boundary(s: &str, start: usize, end: usize) -> bool {
    let before = start == 0 || !is_word_char(s.as_bytes()[start - 1] as char);
    let after = end >= s.len() || !is_word_char(s.as_bytes()[end] as char);
    before && after
}

/// Build the signals object for the result.
fn signals_obj(
    file_count: i64,
    layer_count: i64,
    new_entity_count: i64,
    touch_points: i64,
    historical: usize,
) -> Value {
    json!({
        "fileCount": file_count,
        "layerCount": layer_count,
        "newEntityCount": new_entity_count,
        "estimatedTouchPoints": touch_points,
        "historicalMatches": historical,
    })
}

/// Compute the decomposition decision for an input JSON value.
pub fn decide(input: &Value) -> Value {
    let file_count = input.get("fileCount").and_then(Value::as_i64).unwrap_or(0);
    let layer_count = input.get("layerCount").and_then(Value::as_i64).unwrap_or(0);
    let new_entity_count = input.get("newEntityCount").and_then(Value::as_i64).unwrap_or(0);
    let touch_points = input
        .get("estimatedTouchPoints")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let knowledge_matches = input
        .get("knowledgeMatches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let text = input.get("text").and_then(Value::as_str).unwrap_or("");

    let roadmap = detect_roadmap_signal(text);
    if roadmap.hit {
        return json!({
            "decompose": true,
            "reason": "roadmap-signal",
            "roadmapMatches": roadmap.matches,
            "signals": signals_obj(file_count, layer_count, new_entity_count, touch_points, 0),
        });
    }

    if !knowledge_matches.is_empty() {
        let id = knowledge_matches[0]
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return json!({
            "decompose": true,
            "reason": format!("history-match:{id}"),
            "signals": signals_obj(
                file_count, layer_count, new_entity_count, touch_points,
                knowledge_matches.len()
            ),
        });
    }

    if layer_count >= 2 {
        return json!({
            "decompose": true,
            "reason": "multi-layer",
            "signals": signals_obj(file_count, layer_count, new_entity_count, touch_points, 0),
        });
    }

    if file_count > 10 && new_entity_count >= 2 {
        return json!({
            "decompose": true,
            "reason": "wide-and-new-entities",
            "signals": signals_obj(file_count, layer_count, new_entity_count, touch_points, 0),
        });
    }

    json!({
        "decompose": false,
        "reason": "single-layer",
        "signals": signals_obj(file_count, layer_count, new_entity_count, touch_points, 0),
    })
}

/// Wave-count floor for a **Full**-scope spec given a [`decide`] verdict.
///
/// Translates the 1-vs-N verdict into the minimum number of waves a Full spec
/// must carry, enforcing the invariant *Full scope ⇒ ≥1 wave* (parent =
/// orchestrator, wave = subagent):
///
/// - `decompose == false` (single-layer / no multi-wave signal) ⇒ **1** wave —
///   NOT zero. A Full spec is never wave-less; "reject decomposition" collapses
///   to a single wave, not to a wave-less parent.
/// - `decompose == true` (multi-layer / roadmap / history / wide-and-new) ⇒ the
///   floor is still ≥ 1; the caller picks the actual N (≥ 2 in practice) from
///   the plan it builds. This helper only guarantees the spec never floors
///   below 1.
///
/// Light scope does not call this — Light is a single spec with an inline
/// checklist and no waves.
#[must_use]
pub fn wave_floor_for_full(decompose: bool) -> u32 {
    // Either way a Full spec floors at 1 wave; a multi-wave decision lets the
    // caller raise N above this floor. The floor is the load-bearing invariant.
    let _ = decompose;
    1
}

/// Compute the deterministic signals JSON for `spec_text`, resolving overrides
/// and the entity registry under `project_root`.
///
/// Mirrors the signals object the stdin path consumes, so the verdict from
/// `decide(&compute_signals_from_spec(...))` equals the stdin verdict for the
/// equivalent inputs. Structural-only; no LLM.
///
/// - `fileCount` = number of paths in the spec's `## Files` section.
/// - `layerCount` = distinct architectural roles across those paths
///   ([`detect_role_with`] with `mustard.json#rolePatterns`). A lone `lib`
///   bucket counts as 1 (matches `exec-rewave-check`).
/// - `newEntityCount` = Create-marked `## Files` bullets corroborated by a
///   PascalCase prose token **not** already in the registry (registry diff via
///   exact key lookup) — see [`count_new_entities`].
/// - `text` = the full spec body, so [`detect_roadmap_signal`] runs unchanged.
#[must_use]
pub fn compute_signals_from_spec(spec_text: &str, project_root: &Path) -> Value {
    let role_patterns = load_role_patterns(project_root);

    let file_paths = parse_files_section(spec_text).unwrap_or_default();
    let file_count = file_paths.len() as i64;

    let roles: BTreeSet<String> = file_paths
        .iter()
        .map(|f| detect_role_with(f, &role_patterns))
        .collect();
    let layer_count: i64 = if roles.len() == 1 && roles.contains("lib") {
        1
    } else {
        roles.len() as i64
    };

    let new_entity_count = new_entity_count_from_model(spec_text, project_root);

    json!({
        "fileCount": file_count,
        "layerCount": layer_count,
        "newEntityCount": new_entity_count,
        "text": spec_text,
    })
}

/// Reduce a spec to its narrative prose for entity-reference extraction.
///
/// Markdown structure carries PascalCase tokens that are **not** entities —
/// headings (`# Spec`, `## Files`), file paths in list bullets
/// (`- src/Schema/User.ts`), and code fences. Running [`pascal_tokens`] over the
/// raw spec would count `Spec` / `Files` / path segments as "new entities". This
/// keeps only prose lines so the registry diff reflects real entity mentions:
/// drops heading lines, bullet/numbered list items, and fenced code blocks.
///
/// `pub(crate)` so `registry-query --for-spec` reuses the exact same prose
/// filter (no facade, no second copy of the heuristic).
pub(crate) fn spec_prose(spec_text: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in spec_text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || trimmed.starts_with('#') {
            continue;
        }
        // Drop list bullets (`- ...`, `* ...`, `+ ...`, `1. ...`) — these hold
        // file paths / checklist items, not entity prose.
        let is_bullet = matches!(trimmed.chars().next(), Some('-' | '*' | '+'))
            && trimmed[1..].starts_with([' ', '\t']);
        let is_numbered = {
            let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
            !digits.is_empty() && trimmed[digits.len()..].starts_with(". ")
        };
        if is_bullet || is_numbered {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Count PascalCase entity tokens referenced in `spec_text` that are **not yet**
/// among the repo model's known declaration names under `project_root`.
///
/// Reuses [`pascal_tokens`] (the same entity-reference heuristic `prd-build`
/// uses) over the spec's narrative prose ([`spec_prose`]) and grain's mined
/// declaration names (read via the scan tool's `facts`), so "new" means
/// "referenced but not yet in the model" — a deterministic stand-in for the
/// `newEntityCount` the LLM used to estimate. A missing model (no scan yet)
/// fails open to empty — but a referenced token still only counts when
/// corroborated by a Create-marked `## Files` bullet ([`count_new_entities`]),
/// so a model-less project does not inflate the signal from prose alone.
fn new_entity_count_from_model(spec_text: &str, project_root: &Path) -> i64 {
    let model = project_root.join(".claude").join("grain.model.json");
    let known: BTreeSet<String> = mustard_core::read_entity_names(&model)
        .iter()
        .map(|n| n.to_ascii_lowercase())
        .collect();
    count_new_entities(spec_text, &known)
}

/// Pure: count the net-new entities of a spec — the **Create-marked `## Files`
/// bullets** corroborated by a net-new PascalCase prose token. A token is
/// net-new when it is not in `known` (lowercased); it corroborates a created
/// file when it equals one of the file's [`entity_keys`].
///
/// The corroboration requirement is what keeps the signal honest:
/// [`pascal_tokens`] extracts every capitalized word, so prose alone yields
/// sentence-initial words (PT "Nenhuma entidade nova" → `Nenhuma`), camelCase
/// split fragments (`GraphQL` → `Graph` + `QL`) and pipeline acronyms (`UI`)
/// — none of which are entities. A *real* net-new entity materializes as a
/// net-new file: the spec's `## Files` section carries a bullet marked
/// `(create)` (any catalogue spelling) whose name matches a referenced token.
/// Requiring that structural witness kills the prose noise with no linguistic
/// stoplist — language-agnostic by construction.
///
/// Counting **files** (not tokens) keeps the count meaningful for compound
/// names: `InvoiceService` splits into `Invoice` + `Service` in the prose, but
/// a created `invoice-service.ts` is still exactly ONE new entity. Split from
/// the model I/O so it is unit-tested without the scan tool.
fn count_new_entities(spec_text: &str, known: &BTreeSet<String>) -> i64 {
    let new_tokens: BTreeSet<String> = pascal_tokens(&spec_prose(spec_text))
        .into_iter()
        .map(|t| t.to_ascii_lowercase())
        .filter(|t| !known.contains(t))
        .collect();
    create_marked_paths(spec_text)
        .iter()
        .filter(|path| entity_keys(path).iter().any(|k| new_tokens.contains(k)))
        .count() as i64
}

/// Paths of every `## Files` bullet carrying a **Create** marker — `(create)`
/// / `(new)` / `(novo)` / `(criar)`, resolved through the i18n marker
/// catalogue ([`mustard_core::platform::i18n::file_marker_synonyms`] via
/// [`line_has_file_marker`]) so a localized drafter marker never drifts out of
/// recognition. Absent `## Files` section ⇒ empty (nothing corroborates).
fn create_marked_paths(spec_text: &str) -> Vec<String> {
    let lines: Vec<&str> = spec_text.split('\n').collect();
    let Some(start) = lines.iter().position(|l| is_heading(l, "files")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in lines.iter().skip(start + 1) {
        let trimmed = line.trim();
        // Next `## ` heading ends the section (same break as the files parse).
        if trimmed
            .strip_prefix("##")
            .is_some_and(|rest| rest.starts_with([' ', '\t']))
        {
            break;
        }
        if !line_has_file_marker(trimmed, FileMarker::Create) {
            continue;
        }
        if let Some(path) = bullet_path(trimmed) {
            out.push(path.to_string());
        }
    }
    out
}

/// Parse the leading `- path` / `` - `path` `` bullet of a Files line. Local
/// mirror of the (private) `wave_lib::parse_bullet`: the captured token stops
/// at whitespace/backtick, so a trailing ` (create)` marker never leaks into
/// the path.
fn bullet_path(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('-')?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim_start_matches([' ', '\t']);
    let rest = rest.strip_prefix('`').unwrap_or(rest);
    let token = rest
        .split(|c: char| c.is_whitespace() || c == '`')
        .next()
        .unwrap_or("");
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// Corroboration keys for a created file's path: every lowercased shape a
/// PascalCase prose token can take when it names this file. From the basename
/// stem (up to the first `.`):
///
/// - the whole stem with non-alphanumerics dropped (`invoice-service` →
///   `invoiceservice`);
/// - each separator-delimited segment (`invoice`, `service`) — so a compound
///   filename is matched by the words the prose splitter produces;
/// - each CamelCase word of the stem via [`pascal_tokens`]
///   (`InvoiceService.cs` → `invoice`, `service`).
///
/// Matching is exact per key — a *fragment* like `Graph` (from `GraphQL`)
/// never equals the segment `graphql`, so split noise cannot corroborate.
fn entity_keys(path: &str) -> BTreeSet<String> {
    let norm = path.replace('\\', "/");
    let base = norm.rsplit('/').next().unwrap_or(norm.as_str());
    let stem = base.split('.').next().unwrap_or(base);
    let mut keys = BTreeSet::new();
    let whole: String = stem
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if !whole.is_empty() {
        keys.insert(whole);
    }
    for seg in stem.split(|c: char| !c.is_ascii_alphanumeric()) {
        if !seg.is_empty() {
            keys.insert(seg.to_ascii_lowercase());
        }
    }
    for tok in pascal_tokens(stem) {
        keys.insert(tok.to_ascii_lowercase());
    }
    keys
}

/// Deterministic scope label (`light` / `extended-light` / `full`) from the
/// structural signals — encodes the `/feature` SKILL's prose thresholds in code
/// so the LLM relays the verdict instead of eyeballing it.
///
/// ## What `slice_match_count` really measures
///
/// `slice_match_count` comes from the `feature` digest's `sliceMatchCount` —
/// the number of recurring slices whose **vocabulary** the query matched,
/// saturated at the digest's per-query slice cap (`Q_MAX_SLICES = 12` in the
/// scan crate). It is a vocabulary-overlap signal: any request that names the
/// project's domain matches ≥2 slices, so by itself it says "there is
/// precedent", NOT "this change crosses layers". It therefore never forces
/// `full` alone — it contributes to `full` only when the structural signals
/// already show layer spread (`layerCount >= 2`); at `layerCount <= 1 &&
/// fileCount <= 8` it acts as precedent evidence for the extended-light band.
///
/// The three scopes (and the prose phrase each numeric condition encodes):
///
/// - **full** when ANY of:
///   - `layerCount >= 3` — the SKILL's "3+ layers";
///   - `newEntityCount >= 1` — "net-new" (a Create-marked `## Files` bullet
///     corroborated by a prose token absent from the repo model — see
///     [`count_new_entities`]);
///   - `sliceMatchCount >= 2 && layerCount >= 2` — "spans multiple slices"
///     *with* actual layer spread (vocabulary overlap alone is not spanning);
///   - `fileCount > 8` — beyond the extended-light file ceiling.
/// - **extended-light** when NOT full AND ALL of:
///   - `fileCount > 5 && fileCount <= 8` — "≤8 files" above the light ceiling;
///   - `newEntityCount == 0` — "modifies existing" (no net-new entity);
///   - `sliceMatchCount >= 1` — "matched slice" (mirrors a precedent).
/// - **light** otherwise — `fileCount <= 5`, `layerCount <= 2`, "mirrors a
///   matched slice".
///
/// The spec-derived signals never carry `slice_match_count`, so it is threaded
/// in separately (defaults to 0 when the digest is absent — the conservative
/// read for the slice conditions).
#[must_use]
pub fn classify(signals: &Value, slice_match_count: i64) -> &'static str {
    let file_count = signals.get("fileCount").and_then(Value::as_i64).unwrap_or(0);
    let layer_count = signals.get("layerCount").and_then(Value::as_i64).unwrap_or(0);
    let new_entity_count = signals
        .get("newEntityCount")
        .and_then(Value::as_i64)
        .unwrap_or(0);

    // full: 3+ layers OR corroborated net-new entity OR multi-slice vocabulary
    // overlap WITH layer spread OR wide.
    if layer_count >= 3
        || new_entity_count >= 1
        || (slice_match_count >= 2 && layer_count >= 2)
        || file_count > 8
    {
        return "full";
    }

    // extended-light: matched slice + modifies existing + 6..=8 files.
    if file_count > 5
        && file_count <= 8
        && new_entity_count == 0
        && slice_match_count >= 1
    {
        return "extended-light";
    }

    // light: mirrors a matched slice (<=5 files, <=2 layers).
    "light"
}

/// Honesty annotation for a [`classify_from_spec`] verdict whose `fileCount`
/// came back 0.
///
/// `fileCount` is derived **exclusively** from the spec's `## Files` /
/// `## Arquivos` section ([`parse_files_section`]). That section is a
/// placeholder when the spec scaffold is freshly drafted — the `/feature` SKILL
/// runs `scope-classify` immediately after `spec-draft`, *before* the file
/// census is filled in. With zero parsed paths the verdict is **arithmetically
/// correct** (0 files ⇒ `light`) but **not yet trustworthy**: the same spec can
/// flip to `full` once its census lands. The classifier still emits `light`
/// (and never a non-zero exit — a genuinely tiny change is legitimately light),
/// but it must say so out loud so the orchestrator does not execute inline off a
/// stale verdict.
///
/// Distinguishes "section absent / placeholder" (`None` or `Some(empty)` ⇒ 0
/// paths) from "section present with ≥1 real path" (`Some(non-empty)`): the
/// warning fires **only** when nothing was parsed. A spec that legitimately
/// touches a single real file is silent (no false alarm).
const FILES_SECTION_EMPTY_WARNING: &str = "## Arquivos vazio/placeholder — \
fileCount=0; classificacao nao-confiavel ate preencher o censo";

/// Classify a spec file's scope deterministically: compute the structural
/// signals via [`compute_signals_from_spec`] (no duplicate computation), then
/// [`classify`]. Returns `{ "scope": ..., "signals": { ... } }`.
///
/// When the `## Files` section parses to **zero** paths (absent or placeholder),
/// the verdict carries `"filesSectionEmpty": true` plus a human `"warning"` so a
/// confident-but-premature `light` is never mistaken for a settled one — see
/// [`FILES_SECTION_EMPTY_WARNING`]. The exit code is unchanged (this stays
/// non-blocking).
///
/// Fail-open: an unreadable spec yields `{ "scope": "full", ... }` — the
/// conservative default, since `full` gets the most pipeline rigor (PLAN +
/// /spec approval + per-wave gates).
#[must_use]
pub fn classify_from_spec(spec_file: &Path, slice_match_count: i64) -> Value {
    let Ok(spec_text) = mustard_core::io::fs::read_to_string(spec_file) else {
        return json!({
            "scope": "full",
            "reason": "error-fallback",
            "signals": signals_obj(0, 0, 0, 0, 0),
        });
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let spec_dir = spec_file.parent().map_or_else(|| cwd.clone(), Path::to_path_buf);
    let project_root =
        mustard_core::io::workspace::workspace_root(&spec_dir).unwrap_or_else(|_| cwd.clone());
    let signals = compute_signals_from_spec(&spec_text, &project_root);
    let scope = classify(&signals, slice_match_count);
    // Reuse `compute_signals_from_spec` verbatim (single signal source), but the
    // classifier never reads `text` (only `decide`'s roadmap detection does), so
    // drop it from the emitted view to keep the relay output lean.
    let mut out = json!({
        "scope": scope,
        "sliceMatchCount": slice_match_count,
        "signals": {
            "fileCount": signals.get("fileCount").cloned().unwrap_or(json!(0)),
            "layerCount": signals.get("layerCount").cloned().unwrap_or(json!(0)),
            "newEntityCount": signals.get("newEntityCount").cloned().unwrap_or(json!(0)),
        },
    });
    // Honest signal: a zero `fileCount` means the `## Files`/`## Arquivos`
    // section was absent or a placeholder (nothing parsed) — distinct from a
    // section holding ≥1 real path. Flag it so the consumer treats this `light`
    // as non-confident, without blocking (legitimate light exists).
    if signals.get("fileCount").and_then(Value::as_i64).unwrap_or(0) == 0 {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("filesSectionEmpty".to_string(), json!(true));
            obj.insert("warning".to_string(), json!(FILES_SECTION_EMPTY_WARNING));
        }
    }
    out
}

/// Dispatch `mustard-rt run scope-classify --from-spec <path>
/// [--slice-match-count N]`.
///
/// Mirrors [`run`]'s `--from-spec` path: resolves the spec path against the cwd,
/// then prints the [`classify_from_spec`] verdict. Fail-open by construction.
pub fn run_classify(from_spec: &str, slice_match_count: i64) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let spec_file = if Path::new(from_spec).is_absolute() {
        PathBuf::from(from_spec)
    } else {
        cwd.join(from_spec)
    };
    println!("{}", classify_from_spec(&spec_file, slice_match_count));
}

/// Decide directly from a spec file: compute the deterministic signals, then
/// [`decide`]. Fail-open — an unreadable spec yields the `error-fallback`
/// verdict.
#[must_use]
pub fn decide_from_spec(spec_file: &Path) -> Value {
    let Ok(spec_text) = mustard_core::io::fs::read_to_string(spec_file) else {
        return json!({ "decompose": false, "reason": "error-fallback" });
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let spec_dir = spec_file.parent().map_or_else(|| cwd.clone(), Path::to_path_buf);
    let project_root =
        mustard_core::io::workspace::workspace_root(&spec_dir).unwrap_or_else(|_| cwd.clone());
    decide(&compute_signals_from_spec(&spec_text, &project_root))
}

/// Dispatch `mustard-rt run scope-decompose`.
///
/// With `--from-spec <path>`, computes the signals deterministically from the
/// spec (Rust); otherwise reads the signals JSON from stdin (legacy/override).
pub fn run(from_spec: Option<&str>) {
    if let Some(spec_arg) = from_spec {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let spec_file = if Path::new(spec_arg).is_absolute() {
            PathBuf::from(spec_arg)
        } else {
            cwd.join(spec_arg)
        };
        println!("{}", decide_from_spec(&spec_file));
        return;
    }

    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    let input: Value = if raw.trim().is_empty() {
        json!({})
    } else {
        match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(_) => {
                println!("{}", json!({ "decompose": false, "reason": "error-fallback" }));
                return;
            }
        }
    };
    println!("{}", decide(&input));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_layer_decomposes() {
        let d = decide(&json!({ "layerCount": 3, "fileCount": 8 }));
        assert_eq!(d["decompose"], json!(true));
        assert_eq!(d["reason"], json!("multi-layer"));
    }

    #[test]
    fn single_layer_keeps() {
        let d = decide(&json!({ "layerCount": 1, "fileCount": 3 }));
        assert_eq!(d["decompose"], json!(false));
        assert_eq!(d["reason"], json!("single-layer"));
    }

    /// Field regression: a census written `- M path` / `- A path` (git-style
    /// status markers) must classify by the PATH. The marker once ate the
    /// path — ten "files" all named "M" classified as one `lib` layer, so a
    /// backend+core+app change came back `layerCount: 1` and steered
    /// scope-decompose to a wrong `single-layer` verdict (contradicting the
    /// wave-dependency graph, which reads the clean plan JSON).
    #[test]
    fn compute_signals_counts_layers_through_status_markers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = "# S\n\n## Files\n\
            - M backend/App/Modules/FinancialTitles/GraphQL/Resolver.cs\n\
            - M packages/core/src/shared/schemas/financial.zod.ts\n\
            - A apps/web/app/financial/_components/titles-table.tsx\n";
        let signals = compute_signals_from_spec(spec, dir.path());
        assert_eq!(signals["fileCount"], json!(3), "paths, not markers: {signals}");
        let layers = signals["layerCount"].as_i64().unwrap_or(0);
        assert!(layers >= 2, "marker-prefixed census must span layers: {signals}");
    }

    /// Invariant: a Full spec floors at 1 wave even when `decompose == false`.
    /// `false` for Full means "single wave", never "no wave".
    #[test]
    fn full_wave_floor_is_one_when_not_decomposed() {
        // decompose:false ⇒ exactly 1 wave (single-wave, not wave-less).
        assert_eq!(wave_floor_for_full(false), 1, "Full + single-layer ⇒ 1 wave");
        // decompose:true ⇒ still floors at ≥ 1 (caller raises N).
        assert!(wave_floor_for_full(true) >= 1, "Full + multi-wave ⇒ floor ≥ 1");
    }

    #[test]
    fn history_match_decomposes() {
        let d = decide(&json!({
            "layerCount": 1,
            "knowledgeMatches": [{ "id": "heavy-pipeline-1" }],
        }));
        assert_eq!(d["reason"], json!("history-match:heavy-pipeline-1"));
    }

    #[test]
    fn roadmap_signal_from_plans_ref() {
        let d = decide(&json!({ "layerCount": 1, "text": "see .claude/plans/roadmap.md" }));
        assert_eq!(d["reason"], json!("roadmap-signal"));
    }

    /// Plant a workspace anchor (`mustard.json` + `.claude/`) so
    /// `workspace_root` accepts `root`. No `grain.model.json` ⇒ `read_entity_names`
    /// fails open to empty (every referenced entity then counts as new); the
    /// known-set diff itself is unit-tested via [`count_new_entities`].
    fn plant_project(root: &std::path::Path) {
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join("mustard.json"), b"{}").unwrap();
    }

    #[test]
    fn from_spec_computes_multi_layer_signals() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path());
        // Two distinct roles (schema + api) ⇒ layerCount 2 ⇒ multi-layer.
        let spec = "# Spec\n\n## Files\n- src/schema/user.ts\n- src/api/users.ts\n";
        let signals = compute_signals_from_spec(spec, dir.path());
        assert_eq!(signals["fileCount"], json!(2));
        assert_eq!(signals["layerCount"], json!(2));

        // The deterministic path agrees with the equivalent stdin path.
        let from_spec_decision = decide(&signals);
        let stdin_equiv = decide(&json!({
            "fileCount": 2, "layerCount": 2, "newEntityCount": 0, "text": spec,
        }));
        assert_eq!(from_spec_decision, stdin_equiv);
        assert_eq!(from_spec_decision["decompose"], json!(true));
        assert_eq!(from_spec_decision["reason"], json!("multi-layer"));
    }

    #[test]
    fn from_spec_single_layer_keeps() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path());
        // All files in one generic bucket ⇒ layerCount 1 ⇒ single-layer.
        let spec = "# Spec\n\n## Files\n- src/util/a.ts\n- src/util/b.ts\n";
        let signals = compute_signals_from_spec(spec, dir.path());
        assert_eq!(signals["layerCount"], json!(1));
        assert_eq!(decide(&signals)["reason"], json!("single-layer"));
    }

    #[test]
    fn count_new_entities_diffs_known_set() {
        // The model knows `User`; the spec references `User` (known) and
        // `Invoice` (new, corroborated by a create-marked bullet) ⇒ count 1.
        // Pure logic — no model file / scan tool.
        let known: BTreeSet<String> = ["user"].into_iter().map(str::to_string).collect();
        let spec = "# Spec\nlink the Invoice to the User entity.\n\n## Files\n\
                    - src/models/invoice.ts (create)\n- src/util/a.ts\n";
        assert_eq!(count_new_entities(spec, &known), 1, "Invoice new+corroborated, User known");
    }

    /// Honest signal: a capitalized prose word with NO create-marked file
    /// witness is not an entity. The payables run counted `Nenhuma` (from the
    /// PT sentence "Nenhuma entidade nova") as a new entity — corroboration
    /// kills it with no language stoplist.
    #[test]
    fn scope_new_entity_ignores_uncorroborated_prose_capitalization() {
        let spec = "# Spec\nNenhuma entidade nova. Ajustar a UI da listagem.\n\n\
                    ## Files\n- src/components/payables/list.tsx\n";
        assert_eq!(
            count_new_entities(spec, &BTreeSet::new()),
            0,
            "Nenhuma/Ajustar/UI are prose capitalization, not entities"
        );
    }

    /// `GraphQL` splits into `Graph` + `QL` at [`pascal_tokens`]'s lower→upper
    /// boundary; neither fragment has a create-marked file, so neither counts.
    /// (The split itself is intentionally untouched — prd-build consumes it
    /// too; corroboration fixes the scope signal without changing it.)
    #[test]
    fn scope_new_entity_ignores_graphql_split_fragments() {
        let spec = "# Spec\nExpose the GraphQL mutation for payables.\n\n\
                    ## Files\n- src/payables/data.ts (edit)\n";
        assert_eq!(
            count_new_entities(spec, &BTreeSet::new()),
            0,
            "Graph/QL fragments are not corroborated by any create-marked file"
        );
    }

    /// A genuine net-new entity IS counted: the prose names it AND a
    /// create-marked `## Files` bullet materializes it (key match,
    /// case-insensitive — see [`entity_keys`]). The PT spelling `(novo)` +
    /// `## Arquivos` heading resolve through the i18n marker/heading
    /// catalogues. Compound names count ONCE: `InvoiceService` splits into
    /// `Invoice` + `Service` in the prose, but the created
    /// `invoice-service.ts` is a single file ⇒ a single new entity.
    #[test]
    fn scope_new_entity_counts_when_corroborated_by_create_marker() {
        let en = "# Spec\nAdd the Invoice entity.\n\n## Files\n- src/models/invoice.ts (create)\n";
        assert_eq!(count_new_entities(en, &BTreeSet::new()), 1, "EN (create) corroborates");

        let pt = "# Spec\nCriar a entidade Invoice.\n\n## Arquivos\n- `src/models/invoice.ts` (novo)\n";
        assert_eq!(count_new_entities(pt, &BTreeSet::new()), 1, "PT (novo) corroborates");

        let kebab = "# Spec\nAdd the InvoiceService.\n\n## Files\n\
                     - src/invoicing/invoice-service.ts (create)\n";
        assert_eq!(
            count_new_entities(kebab, &BTreeSet::new()),
            1,
            "compound name ⇒ one created file ⇒ one entity, not two token fragments"
        );
    }

    #[test]
    fn from_spec_wide_and_new_entities_decomposes() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path()); // no model ⇒ all referenced entities new
        // 11 files in one bucket (layerCount 1) + 2 corroborated new entities
        // (create-marked bullets matching the prose tokens) ⇒ wide-and-new.
        let mut files = String::from("# Spec\nadd the Invoice and the Payment models.\n\n## Files\n");
        files.push_str("- src/util/invoice.ts (create)\n- src/util/payment.ts (create)\n");
        for i in 0..9 {
            files.push_str(&format!("- src/util/f{i}.ts\n"));
        }
        let signals = compute_signals_from_spec(&files, dir.path());
        assert_eq!(signals["fileCount"], json!(11));
        assert_eq!(signals["layerCount"], json!(1));
        assert!(signals["newEntityCount"].as_i64().unwrap() >= 2);
        assert_eq!(decide(&signals)["reason"], json!("wide-and-new-entities"));
    }

    #[test]
    fn spec_prose_strips_headings_bullets_and_fences() {
        let spec = "# Title\nReal prose about Invoice.\n\n## Files\n- src/Foo.ts\n1. step\n```\nlet Bar = 1;\n```\nmore prose.\n";
        let prose = spec_prose(spec);
        assert!(prose.contains("Invoice"));
        assert!(prose.contains("more prose"));
        // Headings, list items, and fenced code dropped.
        assert!(!prose.contains("Title"));
        assert!(!prose.contains("Files"));
        assert!(!prose.contains("Foo"));
        assert!(!prose.contains("Bar"));
        assert!(!prose.contains("step"));
    }

    #[test]
    fn decide_from_spec_unreadable_is_fail_open() {
        let d = decide_from_spec(std::path::Path::new("/no/such/spec.md"));
        assert_eq!(d["decompose"], json!(false));
        assert_eq!(d["reason"], json!("error-fallback"));
    }

    // --- scope-classify ---------------------------------------------------

    /// Helper: build a signals object for the classifier (independent of the
    /// `## Files`-section parse, so the threshold logic is tested in isolation).
    fn sig(file_count: i64, layer_count: i64, new_entity_count: i64) -> Value {
        json!({
            "fileCount": file_count,
            "layerCount": layer_count,
            "newEntityCount": new_entity_count,
        })
    }

    #[test]
    fn scope_classify_light_when_small_and_mirrors_slice() {
        // <=5 files, <=2 layers, no net-new, mirrors one slice ⇒ light.
        assert_eq!(classify(&sig(3, 1, 0), 1), "light");
        assert_eq!(classify(&sig(5, 2, 0), 1), "light");
        // Even with zero matched slices, a small modify-existing change is light.
        assert_eq!(classify(&sig(2, 1, 0), 0), "light");
    }

    #[test]
    fn scope_classify_extended_light_band() {
        // 6..=8 files, modifies existing (newEntityCount==0), exactly 1 matched
        // slice (>=2 would be "spans multiple slices" ⇒ full).
        assert_eq!(classify(&sig(6, 2, 0), 1), "extended-light");
        assert_eq!(classify(&sig(8, 2, 0), 1), "extended-light");
        assert_eq!(classify(&sig(7, 1, 0), 1), "extended-light");
    }

    #[test]
    fn scope_classify_extended_light_falls_back_to_light_without_slice() {
        // 6..=8 files but NO matched slice ⇒ not extended-light; not full
        // either (layers<3, no net-new, <=8 files) ⇒ light.
        assert_eq!(classify(&sig(7, 2, 0), 0), "light");
    }

    #[test]
    fn scope_classify_full_on_layers() {
        // 3+ layers ⇒ full ("3+ layers"), regardless of file/slice counts.
        assert_eq!(classify(&sig(2, 3, 0), 1), "full");
    }

    #[test]
    fn scope_classify_full_on_net_new_entity() {
        // newEntityCount>=1 ⇒ full ("net-new"), even for a tiny change.
        assert_eq!(classify(&sig(2, 1, 1), 1), "full");
    }

    #[test]
    fn scope_classify_full_on_spanning_multiple_slices() {
        // sliceMatchCount>=2 ⇒ full ("spans multiple slices") — but only with
        // actual layer spread (layerCount >= 2).
        assert_eq!(classify(&sig(4, 2, 0), 2), "full");
    }

    /// Honest slice signal: `sliceMatchCount` is vocabulary overlap with the
    /// slice catalogue (saturates at the digest cap), so at layerCount<=1 it
    /// is precedent evidence — never a full trigger by itself.
    #[test]
    fn scope_classify_slice_overlap_alone_does_not_force_full() {
        // Single layer, small ⇒ light even with a saturated slice count.
        assert_eq!(classify(&sig(3, 1, 0), 12), "light");
        // Single layer, 6..=8 files ⇒ the slice match is precedent evidence
        // (extended-light), not "spans multiple slices".
        assert_eq!(classify(&sig(7, 1, 0), 2), "extended-light");
        // Width still escalates regardless of layers.
        assert_eq!(classify(&sig(9, 1, 0), 2), "full");
    }

    /// The payables regression end-to-end: 1 layer, 7 files, saturated
    /// sliceMatchCount 7, PT prose ("Nenhuma entidade nova", GraphQL), two
    /// create-marked files whose stems match no prose token ⇒ extended-light
    /// (the live run misclassified this as full off `Nenhuma`/`Graph`/`QL`).
    #[test]
    fn scope_classify_payables_run_is_extended_light() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path());
        let spec = "# Spec\n\n## Contexto\n\nAjustes na listagem de payables via GraphQL. \
                    Nenhuma entidade nova.\n\n## Arquivos\n\
                    - src/components/payables/payable-constants.ts (criar)\n\
                    - src/components/payables/notes-section.tsx (criar)\n\
                    - src/components/payables/list.tsx\n\
                    - src/components/payables/row.tsx\n\
                    - src/components/payables/filters.tsx\n\
                    - src/components/payables/summary.tsx\n\
                    - src/components/payables/data.ts\n";
        let spec_path = dir.path().join("spec.md");
        std::fs::write(&spec_path, spec).unwrap();
        let d = classify_from_spec(&spec_path, 7);
        assert_eq!(d["signals"]["fileCount"], json!(7));
        assert_eq!(d["signals"]["layerCount"], json!(1));
        assert_eq!(
            d["signals"]["newEntityCount"],
            json!(0),
            "Ajustes/Graph/QL/Nenhuma are prose noise, and the creates match no prose token"
        );
        assert_eq!(d["scope"], json!("extended-light"), "honest signals ⇒ extended-light, not full");
    }

    #[test]
    fn scope_classify_full_on_wide_file_count() {
        // fileCount>8 ⇒ full (beyond the extended-light ceiling).
        assert_eq!(classify(&sig(9, 1, 0), 1), "full");
    }

    /// Boundary: file count at the light ceiling (5) vs the extended-light
    /// floor (6), and the extended-light ceiling (8) vs the full floor (9).
    #[test]
    fn scope_classify_file_count_boundaries() {
        assert_eq!(classify(&sig(5, 2, 0), 1), "light"); // <=5 ⇒ light
        assert_eq!(classify(&sig(6, 2, 0), 1), "extended-light"); // 6 ⇒ ext-light
        assert_eq!(classify(&sig(8, 2, 0), 1), "extended-light"); // 8 ⇒ ext-light
        assert_eq!(classify(&sig(9, 2, 0), 1), "full"); // >8 ⇒ full
    }

    /// Boundary: layer count at 2 (light/ext) vs 3 (full).
    #[test]
    fn scope_classify_layer_count_boundaries() {
        assert_eq!(classify(&sig(4, 2, 0), 1), "light"); // 2 layers
        assert_eq!(classify(&sig(4, 3, 0), 1), "full"); // 3 layers ⇒ full
    }

    /// Boundary: slice match count at 1 (mirrors) vs 2 (spans).
    #[test]
    fn scope_classify_slice_match_boundaries() {
        // 1 matched slice in the ext-light band ⇒ extended-light.
        assert_eq!(classify(&sig(7, 2, 0), 1), "extended-light");
        // 2 matched slices ⇒ full (spans multiple slices) even in that band.
        assert_eq!(classify(&sig(7, 2, 0), 2), "full");
    }

    /// Honesty annotation: a freshly-drafted spec whose `## Arquivos` section is
    /// a placeholder (no real bullet paths) parses to `fileCount=0`. The verdict
    /// stays `light` (0 files is arithmetically light) but MUST carry
    /// `filesSectionEmpty: true` + the warning so the orchestrator does not treat
    /// the premature `light` as settled. A spec with ≥1 real path is silent.
    #[test]
    fn classify_from_spec_flags_empty_files_section() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path());

        // (i) `## Arquivos` heading present but only a placeholder line — no
        // bullet path parses ⇒ fileCount 0 ⇒ flagged non-confident.
        let placeholder = "# Spec\n\n## Contexto\n\nAdicionar algo.\n\n## Arquivos\n\
                           _(a preencher após o censo)_\n";
        let placeholder_path = dir.path().join("placeholder.md");
        std::fs::write(&placeholder_path, placeholder).unwrap();
        let d = classify_from_spec(&placeholder_path, 0);
        assert_eq!(d["signals"]["fileCount"], json!(0));
        assert_eq!(d["scope"], json!("light"), "0 files is arithmetically light");
        assert_eq!(d["filesSectionEmpty"], json!(true), "placeholder ⇒ flagged");
        assert!(
            d["warning"].as_str().is_some_and(|w| !w.is_empty()),
            "non-confident verdict carries a warning"
        );

        // An entirely absent `## Files` section is also 0 paths ⇒ flagged.
        let absent = "# Spec\n\n## Contexto\n\nAlgo sem censo.\n";
        let absent_path = dir.path().join("absent.md");
        std::fs::write(&absent_path, absent).unwrap();
        let da = classify_from_spec(&absent_path, 0);
        assert_eq!(da["filesSectionEmpty"], json!(true), "absent section ⇒ flagged");

        // (ii) Section present with ≥1 real path ⇒ NOT flagged (no false alarm).
        let filled = "# Spec\n\n## Files\n- src/util/a.ts\n";
        let filled_path = dir.path().join("filled.md");
        std::fs::write(&filled_path, filled).unwrap();
        let df = classify_from_spec(&filled_path, 0);
        assert_eq!(df["signals"]["fileCount"], json!(1));
        assert!(df.get("filesSectionEmpty").is_none(), "≥1 real path ⇒ no flag");
        assert!(df.get("warning").is_none(), "≥1 real path ⇒ no warning");
    }

    #[test]
    fn classify_from_spec_unreadable_is_fail_open_to_full() {
        let d = classify_from_spec(std::path::Path::new("/no/such/spec.md"), 0);
        assert_eq!(d["scope"], json!("full"), "unreadable spec ⇒ conservative full");
        assert_eq!(d["reason"], json!("error-fallback"));
    }

    #[test]
    fn classify_from_spec_reuses_compute_signals() {
        let dir = tempfile::tempdir().unwrap();
        plant_project(dir.path());
        // Two distinct roles (schema + api) ⇒ layerCount 2; no model ⇒ entities
        // referenced count as new, but this spec has no entity prose ⇒ 0 new.
        let spec = "# Spec\n\n## Files\n- src/schema/user.ts\n- src/api/users.ts\n";
        let spec_path = dir.path().join("spec.md");
        std::fs::write(&spec_path, spec).unwrap();
        let d = classify_from_spec(&spec_path, 1);
        // layerCount 2, fileCount 2, newEntityCount 0, 1 slice ⇒ light.
        assert_eq!(d["signals"]["fileCount"], json!(2));
        assert_eq!(d["signals"]["layerCount"], json!(2));
        assert_eq!(d["scope"], json!("light"));
        // The signals object is exactly what compute_signals_from_spec emits —
        // no duplicate computation path.
        let direct = compute_signals_from_spec(spec, dir.path());
        assert_eq!(d["signals"]["fileCount"], direct["fileCount"]);
        assert_eq!(d["signals"]["layerCount"], direct["layerCount"]);
    }
}
