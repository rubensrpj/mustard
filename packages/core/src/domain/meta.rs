//! `meta.json` — the sidecar lifecycle metadata file beside each `spec.md`.
//!
//! ## Why a sidecar
//!
//! Before this module, lifecycle metadata (`stage`, `outcome`, `phase`, `scope`,
//! `lang`, `checkpoint`, `parent`, `isWavePlan`, `totalWaves`) lived inside the
//! spec markdown as a run of `### Key:` headers parsed by a bilingual regex
//! table inside `apps/rt/src/run/spec_sections.rs` (PT/EN variants). Every new
//! field grew that table; every consumer paid the parse cost; collaborators on
//! two machines saw header drift because the only way to refresh a status was
//! a markdown rewrite.
//!
//! The sidecar `meta.json` is the canonical home for **machine-parseable**
//! fields. The markdown stays for narrative (the `## Contexto`, `## Tarefas`,
//! `## Critérios de Aceitação` sections). After Wave 3 of the mustard-unification
//! mega-spec lands, the `### Stage:` / `### Outcome:` / `### Phase:` / `### Scope:`
//! / `### Lang:` / `### Checkpoint:` / `### Parent:` headers are removed from
//! `.md` and live only here.
//!
//! ## Design (SOLID)
//!
//! - **Single Responsibility.** This module owns the `meta.json` schema and its
//!   read/write IO. It does not parse `.md`, does not open event stores.
//! - **Open/Closed (forward-compat).** [`Meta`] carries a `#[serde(flatten)]
//!   raw: Value` catch-all so a future field added by a newer Mustard does not
//!   make older consumers fail to deserialize. See the
//!   [`core-lenient-serde-model`] skill.
//! - **Fail-open.** A missing / unreadable / unparseable `meta.json` yields
//!   `None` (never a panic, never an error). The caller falls back to the legacy
//!   `.md` parser ([`crate::domain::spec::parse_state`]).
//!
//! ## Inviolable safety contract
//!
//! - **Atomic writes.** [`write_meta`] routes through [`crate::io::fs::write_atomic`]
//!   (sibling tempfile + rename), so a crash mid-write never leaves a corrupt
//!   `meta.json`.
//! - **Lenient parse.** Unknown fields are preserved under `raw`; required
//!   fields all carry `#[serde(default)]` so partial migrations stay readable.
//! - **BCP-47 locale codes.** `lang` is canonically `pt-BR` / `en-US`. The
//!   migration helpers tolerate the legacy short forms (`pt` / `en`) and emit a
//!   `eprintln!` warning, but writers produced after Wave 3 only emit BCP-47.

use crate::domain::model::view::Flags;
use crate::io::fs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// The canonical schema for `meta.json` alongside a spec's `spec.md`.
///
/// Every field is optional (`#[serde(default)]`) so a partial / future
/// `meta.json` still deserialises. Unknown fields land in `raw` via
/// `#[serde(flatten)]` — round-trip safe under the
/// [`core-lenient-serde-model`] pattern.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Meta {
    /// Canonical lifecycle stage spelling: `Analyze` / `Plan` / `Execute` /
    /// `QaReview` / `Close`. Mirror of [`crate::Stage`]. Case-insensitive on
    /// read; canonical (TitleCase) on write.
    ///
    /// Always serialized (`null` when absent) so AC-W3-1's `(k in j)` check
    /// passes for every spec — these are the canonical six **machine-parseable**
    /// lifecycle fields and the dashboard relies on their key presence.
    pub stage: Option<String>,
    /// Terminal outcome: `Active` / `Completed` / `Cancelled` / `Abandoned` /
    /// `Superseded` / `Absorbed`. Mirror of [`crate::Outcome`]. Always
    /// serialized (`null` when absent).
    ///
    /// `Superseded` and `Absorbed` were added in Wave 4 of the deep-refactor
    /// (2026-05-25): historic specs replaced by a newer one carry
    /// `Superseded`; specs folded into a larger consolidating spec carry
    /// `Absorbed`. The dashboard renders dedicated badges for both.
    pub outcome: Option<String>,
    /// Active pipeline phase token (`ANALYZE`/`PLAN`/`EXECUTE`/`QA`/`CLOSE`).
    /// Surfaced for dashboard cards; the canonical state machine is
    /// `stage` + `outcome` + `flags`. Always serialized (`null` when absent).
    pub phase: Option<String>,
    /// Pipeline scope (`full` / `light` / `wave plan`). Always serialized.
    pub scope: Option<String>,
    /// BCP-47 locale code for the spec's narrative (`pt-BR` / `en-US`). The
    /// legacy short forms (`pt` / `en`) are accepted on read with a warning.
    /// Always serialized (`null` when absent).
    pub lang: Option<String>,
    /// ISO-8601 timestamp of the last meaningful pipeline event for this spec.
    /// Always serialized (`null` when absent).
    pub checkpoint: Option<String>,
    /// Parent spec slug when this `meta.json` lives in a sub-wave or child
    /// directory. `None` for top-level specs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// `true` when this `meta.json` corresponds to the top-level `wave-plan.md`
    /// of a multi-wave epic. Drives dashboard rendering.
    #[serde(rename = "isWavePlan", skip_serializing_if = "Option::is_none")]
    pub is_wave_plan: Option<bool>,
    /// Number of waves under this spec (only set when `isWavePlan = true`).
    #[serde(rename = "totalWaves", skip_serializing_if = "Option::is_none")]
    pub total_waves: Option<u32>,
    /// Orthogonal qualifier flags (`blocked` / `wave_failed` / `followup_open`)
    /// — the canonical home of the [`Flags`] that the legacy `### Flags:`
    /// header used to carry.
    ///
    /// Serialized as a deduplicated, declaration-ordered **array of tokens**
    /// (`["blocked", "wave_failed", "followup_open"]`), matching the token
    /// vocabulary [`Flags::parse`] reads and the `after.flags` array
    /// `migrate-spec-headers` already emits in its audit log. The array shape
    /// (rather than an object of bools) keeps the on-disk JSON compact for the
    /// common all-false case and stays byte-stable under serde declaration
    /// order. Elided entirely (`skip_serializing_if`) when no flag is set, so a
    /// spec with no qualifier produces no `flags` key — preserving the empty
    /// `meta.json` byte shape that pre-dated this field.
    #[serde(default, skip_serializing_if = "MetaFlags::is_empty")]
    pub flags: MetaFlags,
    /// Forward-compatible catch-all. Any field a future Mustard adds lands here
    /// and is preserved on round-trip writes. Per the
    /// [`core-lenient-serde-model`] skill, this is the boundary type's contract.
    #[serde(flatten)]
    pub raw: Value,
}

impl Meta {
    /// Build a `Meta` from the canonical state-machine triple plus optional
    /// scalar fields. The catch-all `raw` is initialised to a `Value::Null`.
    #[must_use]
    pub fn new(
        stage: Option<&str>,
        outcome: Option<&str>,
        phase: Option<&str>,
        scope: Option<&str>,
        lang: Option<&str>,
        checkpoint: Option<&str>,
        parent: Option<&str>,
    ) -> Self {
        Self {
            stage: stage.map(str::to_string),
            outcome: outcome.map(str::to_string),
            phase: phase.map(str::to_string),
            scope: scope.map(str::to_string),
            lang: lang.map(normalise_lang_string),
            checkpoint: checkpoint.map(str::to_string),
            parent: parent.map(str::to_string),
            is_wave_plan: None,
            total_waves: None,
            flags: MetaFlags::default(),
            raw: Value::Null,
        }
    }
}

/// The `meta.json` representation of the qualifier [`Flags`] —
/// `blocked` / `wave_failed` / `followup_open`.
///
/// Serialized as a deduplicated, declaration-ordered array of canonical tokens
/// (`["blocked", "wave_failed", "followup_open"]`). Deserialized leniently via
/// [`Flags::parse`]-equivalent token matching, so any historical / future
/// spelling (`wave-failed`, `closed-followup`, …) still round-trips into the
/// canonical [`Flags`]; unknown tokens are ignored (fail-open). The default is
/// the all-false [`Flags`], serializing to an empty array (which the `Meta`
/// field's `skip_serializing_if` then elides entirely).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetaFlags(pub Flags);

impl MetaFlags {
    /// `true` when no qualifier flag is set (the all-false default).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0 == Flags::default()
    }

    /// The canonical tokens this flag set emits, in declaration order. Empty
    /// when no flag is set.
    #[must_use]
    fn tokens(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.0.blocked {
            out.push("blocked");
        }
        if self.0.wave_failed {
            out.push("wave_failed");
        }
        if self.0.followup_open {
            out.push("followup_open");
        }
        out
    }
}

impl From<Flags> for MetaFlags {
    fn from(flags: Flags) -> Self {
        Self(flags)
    }
}

impl From<MetaFlags> for Flags {
    fn from(meta_flags: MetaFlags) -> Self {
        meta_flags.0
    }
}

impl Serialize for MetaFlags {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.tokens().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MetaFlags {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Lenient: accept an array of tokens. Each token is matched against the
        // canonical vocabulary via the shared `Flags::parse` so legacy /
        // alternate spellings (`wave-failed`, `closed-followup`) still resolve;
        // unknown tokens are ignored. We fold the whole array through one
        // `Flags::parse` call by joining on spaces — its splitter handles
        // commas/whitespace and de-dupes for free.
        let tokens = Vec::<String>::deserialize(deserializer)?;
        Ok(Self(Flags::parse(&tokens.join(" "))))
    }
}

/// BCP-47 locale codes the writers should canonically emit.
const LANG_BCP47_PT: &str = "pt-BR";
const LANG_BCP47_EN: &str = "en-US";

/// Map a free-form `lang` value onto a canonical BCP-47 code when possible.
/// `pt` → `pt-BR`, `en` → `en-US` (with a stderr warning). Any other value is
/// returned unchanged so user-set codes (`pt-PT`, `es-MX`) survive intact.
#[must_use]
fn normalise_lang_string(raw: &str) -> String {
    let trimmed = raw.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "pt" => {
            eprintln!(
                "meta.json: lang={trimmed:?} is a legacy short code; emitting {LANG_BCP47_PT}"
            );
            LANG_BCP47_PT.to_string()
        }
        "en" => {
            eprintln!(
                "meta.json: lang={trimmed:?} is a legacy short code; emitting {LANG_BCP47_EN}"
            );
            LANG_BCP47_EN.to_string()
        }
        _ => trimmed.to_string(),
    }
}

/// Public helper for consumers normalising user-supplied lang strings before
/// constructing a [`Meta`]. Same contract as [`normalise_lang_string`] but
/// available on the public API.
#[must_use]
pub fn normalise_lang(raw: &str) -> String {
    normalise_lang_string(raw)
}

/// Read and parse the `meta.json` at `path`. Fail-open: a missing / unreadable
/// file or an unparseable body yields `None`. Never panics.
#[must_use]
pub fn read_meta(path: &Path) -> Option<Meta> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Meta>(&text).ok()
}

/// Read the `meta.json` sidecar that lives next to `spec_file` (i.e. in the
/// same directory). Convenience wrapper around [`read_meta`] — every consumer
/// that already has the `.md` path on hand calls this instead of recomputing
/// the sidecar location.
///
/// Fail-open: missing parent dir, missing sidecar, or unparseable body all
/// yield `None`.
#[must_use]
pub fn read_meta_beside(spec_file: &Path) -> Option<Meta> {
    let dir = spec_file.parent()?;
    read_meta(&dir.join("meta.json"))
}

/// Project a [`Meta`] onto the lifecycle status word the dashboard / wave-tree
/// icon map keys off. This is the single source of truth for the
/// `stage` + `outcome` → label mapping; every consumer (wave-tree, picker,
/// dashboard) must call this rather than re-implementing the table.
///
/// Mapping precedence (highest first):
/// 1. `flags.blocked` (any stage) → `"blocked"`; `flags.wave_failed` →
///    `"wave-failed"`; `flags.followup_open` → `"closed-followup"`. The
///    qualifier flags (now carried in `meta.json#flags`) win over the bare
///    `stage` word so the dashboard / wave-tree keep the `blocked` /
///    `wave-failed` / `closed-followup` signal — mirroring
///    [`crate::SpecState::status_kebab`].
/// 2. `outcome == Blocked` → `"blocked"` (legacy outcome spelling, any stage).
/// 3. `outcome == Rejected` → `"rejected"`.
/// 4. `outcome == ClosedFollowup` / `closed-followup` / `closed_followup` → `"closed-followup"`.
/// 5. `outcome == Superseded` → `"completed"` (TF's are visually done).
/// 6. `stage == Close && outcome == Completed` → `"completed"`.
/// 7. `stage == Execute` → `"implementing"`.
/// 8. anything else (incl. `Plan` / `Active`, empty fields) → `""` (queued).
///
/// Match is case-insensitive on both sides.
#[must_use]
pub fn status_word(meta: &Meta) -> &'static str {
    // Qualifier flags (from `meta.json#flags`) win over the bare stage word.
    if meta.flags.0.blocked {
        return "blocked";
    }
    if meta.flags.0.wave_failed {
        return "wave-failed";
    }
    if meta.flags.0.followup_open {
        return "closed-followup";
    }
    let stage = meta.stage.as_deref().unwrap_or("").to_ascii_lowercase();
    let outcome = meta.outcome.as_deref().unwrap_or("").to_ascii_lowercase();
    match (stage.as_str(), outcome.as_str()) {
        (_, "blocked") => "blocked",
        (_, "rejected") => "rejected",
        (_, "closedfollowup" | "closed-followup" | "closed_followup") => "closed-followup",
        (_, "superseded") => "completed",
        ("close", "completed") => "completed",
        ("execute", _) => "implementing",
        _ => "",
    }
}

/// Atomically write `meta` to `path` via [`crate::io::fs::write_atomic`] (sibling
/// tempfile + rename).
///
/// # Errors
///
/// Returns the underlying [`std::io::Error`] when the tempfile cannot be written
/// or the rename fails.
pub fn write_meta(path: &Path, meta: &Meta) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(meta)
        .map_err(|e| std::io::Error::other(format!("meta.json serialize: {e}")))?;
    let mut bytes = body.into_bytes();
    // Trailing newline — every editor expects one and `cat` is friendlier.
    bytes.push(b'\n');
    fs::write_atomic(path, &bytes).map_err(|e| std::io::Error::other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips_full_meta() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("meta.json");
        let meta = Meta {
            stage: Some("Execute".into()),
            outcome: Some("Active".into()),
            phase: Some("EXECUTE".into()),
            scope: Some("full".into()),
            lang: Some("pt-BR".into()),
            checkpoint: Some("2026-05-24T19:30:00Z".into()),
            parent: None,
            is_wave_plan: Some(false),
            total_waves: None,
            flags: MetaFlags::default(),
            raw: Value::Null,
        };
        write_meta(&path, &meta).unwrap();
        let back = read_meta(&path).expect("reads");
        assert_eq!(back.stage.as_deref(), Some("Execute"));
        assert_eq!(back.lang.as_deref(), Some("pt-BR"));
        assert_eq!(back.is_wave_plan, Some(false));
        assert!(back.flags.is_empty());
    }

    #[test]
    fn flags_round_trip_as_token_array() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("meta.json");
        let mut meta = Meta {
            stage: Some("Close".into()),
            outcome: Some("Active".into()),
            ..Meta::default()
        };
        meta.flags = MetaFlags(Flags {
            followup_open: true,
            ..Flags::default()
        });
        write_meta(&path, &meta).unwrap();
        // Serialized as a token array.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"flags\""), "{text}");
        assert!(text.contains("\"followup_open\""), "{text}");
        // Round-trips back to the same Flags.
        let back = read_meta(&path).expect("reads");
        assert!(back.flags.0.followup_open);
        assert!(!back.flags.0.blocked);
    }

    #[test]
    fn flags_empty_elides_key() {
        // A spec with no qualifier flag emits no `flags` key — preserving the
        // pre-flags empty `meta.json` byte shape.
        let m = Meta::default();
        let text = serde_json::to_string(&m).unwrap();
        assert!(!text.contains("\"flags\""), "{text}");
    }

    #[test]
    fn flags_legacy_token_spellings_parse() {
        // Forward/back-compat: alternate token spellings still resolve.
        let meta: Meta = serde_json::from_str(
            r#"{"stage":"Execute","outcome":"Active","flags":["wave-failed"]}"#,
        )
        .unwrap();
        assert!(meta.flags.0.wave_failed);
        let meta2: Meta = serde_json::from_str(
            r#"{"stage":"Close","outcome":"Active","flags":["closed-followup"]}"#,
        )
        .unwrap();
        assert!(meta2.flags.0.followup_open);
    }

    #[test]
    fn status_word_honors_flags() {
        let blocked = Meta {
            stage: Some("Execute".into()),
            outcome: Some("Active".into()),
            flags: MetaFlags(Flags { blocked: true, ..Flags::default() }),
            ..Meta::default()
        };
        assert_eq!(status_word(&blocked), "blocked");
        let wf = Meta {
            stage: Some("Execute".into()),
            outcome: Some("Active".into()),
            flags: MetaFlags(Flags { wave_failed: true, ..Flags::default() }),
            ..Meta::default()
        };
        assert_eq!(status_word(&wf), "wave-failed");
        let fu = Meta {
            stage: Some("Close".into()),
            outcome: Some("Active".into()),
            flags: MetaFlags(Flags { followup_open: true, ..Flags::default() }),
            ..Meta::default()
        };
        assert_eq!(status_word(&fu), "closed-followup");
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        assert!(read_meta(&path).is_none());
    }

    #[test]
    fn malformed_body_is_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"not json at all").unwrap();
        assert!(read_meta(&path).is_none());
    }

    #[test]
    fn unknown_fields_survive_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("meta.json");
        // Synthesise a `meta.json` from a future Mustard adding `priority`.
        std::fs::write(
            &path,
            br#"{"stage":"Plan","outcome":"Active","priority":7,"customExt":{"k":"v"}}"#,
        )
        .unwrap();
        let meta = read_meta(&path).expect("reads");
        assert_eq!(meta.stage.as_deref(), Some("Plan"));
        // Unknown fields landed in `raw`.
        assert_eq!(meta.raw.get("priority").and_then(Value::as_i64), Some(7));
        // Round-trip preserves them.
        write_meta(&path, &meta).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"priority\": 7"));
        assert!(text.contains("\"customExt\""));
    }

    #[test]
    fn lang_short_codes_normalise_to_bcp47() {
        assert_eq!(normalise_lang("pt"), "pt-BR");
        assert_eq!(normalise_lang("en"), "en-US");
        // BCP-47 already — left as-is.
        assert_eq!(normalise_lang("pt-BR"), "pt-BR");
        assert_eq!(normalise_lang("en-US"), "en-US");
        // Foreign-but-valid codes also untouched.
        assert_eq!(normalise_lang("es-MX"), "es-MX");
    }

    #[test]
    fn meta_new_sets_fields() {
        let m = Meta::new(
            Some("Execute"),
            Some("Active"),
            Some("EXECUTE"),
            Some("full"),
            Some("pt"),
            Some("2026-05-24T19:30:00Z"),
            None,
        );
        assert_eq!(m.stage.as_deref(), Some("Execute"));
        // `lang: "pt"` was normalised to BCP-47 on construction.
        assert_eq!(m.lang.as_deref(), Some("pt-BR"));
    }

    #[test]
    fn empty_meta_serializes_all_required_keys_as_null() {
        // The six machine-parseable lifecycle fields always serialize (as
        // `null`) so dashboard / AC consumers can rely on `(k in j)` rather
        // than treating an absent key and an explicit null differently. The
        // `parent` / `isWavePlan` / `totalWaves` extras remain
        // `skip_serializing_if`-elided (genuinely optional).
        let m = Meta::default();
        let text = serde_json::to_string(&m).unwrap();
        for k in ["stage", "outcome", "phase", "scope", "lang", "checkpoint"] {
            assert!(text.contains(&format!("\"{k}\":null")), "{k} missing in {text}");
        }
        assert!(!text.contains("\"parent\""));
        assert!(!text.contains("\"isWavePlan\""));
        assert!(!text.contains("\"totalWaves\""));
        assert!(!text.contains("\"flags\""));
    }
}
