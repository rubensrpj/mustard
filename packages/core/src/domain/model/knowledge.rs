//! `knowledge` — the single, unified knowledge record.
//!
//! ## Why this exists
//!
//! Mustard accumulated **five** ad-hoc knowledge stores, each with its own
//! frontmatter and no common type:
//!
//! | On-disk store                         | legacy `kind`              | writer |
//! |---------------------------------------|----------------------------|--------|
//! | `.claude/knowledge/*.md`              | `pattern` / `decision` / `lesson` | `commands::knowledge::memory` + `session_knowledge_observer` |
//! | `.claude/memory/agent/*.md`           | *(none — an agent summary)*| `agent_summary_observer` |
//! | `.claude/memory/decisions/*.md`       | `decision`                 | `memory` + `memory_promote_observer` |
//! | `.claude/memory/lessons/*.md`         | `lesson`                   | `memory` + `memory_promote_observer` |
//! | `.claude/spec/{spec}/memory/*.md`     | `principle` / `process` / `reference` | *(retired `spec-memory` command)* |
//!
//! [`Knowledge`] is the one `serde` type that **subsumes all five without
//! information loss**: the union of every frontmatter field, normalised onto a
//! [`Kind`], a [`Scope`] (origin + reach), an [`Origin`] sidecar, and a
//! [`Status`].
//!
//! ## Purity contract
//!
//! This module lives in `domain/model/` — it is a **public contract** other
//! crates render against, and it is **pure**: no filesystem, no logging, no
//! disk. The on-disk owner is `io::knowledge_store::KnowledgeStore`. The only
//! methods here are total, side-effect-free transforms:
//! [`Knowledge::from_markdown`] / [`Knowledge::to_markdown`] (frontmatter+body
//! round-trip) and [`Knowledge::slug`] (deterministic filename stem).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Kind
// ---------------------------------------------------------------------------

/// What sort of knowledge a record carries.
///
/// Subsumes the legacy `kind` strings of all five stores. The collapse map
/// (applied by [`Kind::from_legacy`]):
///
/// - `decision` → [`Kind::Decision`]
/// - `lesson`   → [`Kind::Lesson`]
/// - `pattern` / `principle` / `process` → [`Kind::Principle`] (all three are
///   reusable "how we do things" records; the exact legacy spelling is the only
///   thing that does not round-trip, and no consumer branched on it)
/// - `reference` → [`Kind::Reference`]
/// - *absent* (the agent-memory store had no `kind`) → [`Kind::Summary`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A chosen course of action ("use X over Y").
    Decision,
    /// A retrospective takeaway ("watch out for Z").
    Lesson,
    /// A reusable convention / pattern / process the project follows.
    Principle,
    /// A subagent's run summary (the agent-memory rows).
    Summary,
    /// A pointer to where something lives / how it is wired.
    Reference,
}

impl Kind {
    /// Normalise a legacy frontmatter `kind` string onto a [`Kind`].
    ///
    /// `None`/empty (the agent-memory store carried no `kind`) → [`Kind::Summary`].
    /// An unrecognised value also falls back to [`Kind::Summary`] (fail-open —
    /// never panic on hostile frontmatter).
    #[must_use]
    pub fn from_legacy(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("decision") => Self::Decision,
            Some("lesson") => Self::Lesson,
            Some("pattern" | "principle" | "process") => Self::Principle,
            Some("reference") => Self::Reference,
            _ => Self::Summary,
        }
    }

    /// The canonical lowercase token written to frontmatter (`kind:`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Lesson => "lesson",
            Self::Principle => "principle",
            Self::Summary => "summary",
            Self::Reference => "reference",
        }
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Lifecycle of a knowledge record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Live and eligible to surface.
    #[default]
    Active,
    /// Consolidated into a permanent decision/lesson (promotion source).
    Promoted,
    /// Retired — kept for history, not surfaced.
    Deprecated,
    /// Replaced by a newer record.
    Superseded,
}

impl Status {
    /// Normalise a legacy `status` string. `None`/unknown → [`Status::Active`].
    #[must_use]
    pub fn from_legacy(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("promoted") => Self::Promoted,
            Some("deprecated") => Self::Deprecated,
            Some("superseded") => Self::Superseded,
            _ => Self::Active,
        }
    }

    /// The canonical lowercase token written to frontmatter (`status:`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Promoted => "promoted",
            Self::Deprecated => "deprecated",
            Self::Superseded => "superseded",
        }
    }
}

// ---------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------

/// Where a record came from and how far it reaches — the axis that lets a later
/// wave/spec query knowledge cross-cuttingly.
///
/// Serialised as an internally-tagged enum so the on-disk frontmatter reads
/// `scope: global` / `scope: spec` / `scope: wave` with the `spec` / `wave`
/// fields flat alongside it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "lowercase")]
pub enum Scope {
    /// Project-wide knowledge — not tied to any spec.
    #[default]
    Global,
    /// Knowledge owned by one spec (reaches every wave of that spec).
    Spec {
        /// The owning spec slug.
        spec: String,
    },
    /// Knowledge owned by one wave of one spec (the narrowest reach).
    Wave {
        /// The owning spec slug.
        spec: String,
        /// The owning wave number.
        wave: u32,
    },
}

impl Scope {
    /// The owning spec slug, if any.
    #[must_use]
    pub fn spec(&self) -> Option<&str> {
        match self {
            Self::Global => None,
            Self::Spec { spec } | Self::Wave { spec, .. } => Some(spec.as_str()),
        }
    }

    /// The owning wave number, if any.
    #[must_use]
    pub fn wave(&self) -> Option<u32> {
        match self {
            Self::Wave { wave, .. } => Some(*wave),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Origin
// ---------------------------------------------------------------------------

/// Provenance of a record — *who* captured it, *when*, and in which run.
///
/// `spec` / `wave` here mirror [`Scope`] for the legacy stores that recorded
/// origin separately from reach; they are kept distinct so a future record can
/// have a Global reach but still remember the run it was born in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// Spec the record was captured during (origin run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    /// Wave the record was captured during.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave: Option<u32>,
    /// Authoring role / agent type (`backend`, `wave-1-foo`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Originating session id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// ISO-8601 capture timestamp.
    pub captured_at: String,
}

// ---------------------------------------------------------------------------
// Knowledge
// ---------------------------------------------------------------------------

/// The single unified knowledge record — the union of all five legacy stores.
///
/// **Public contract.** Other crates (`mustard-rt`, the dashboard) render this
/// shape; change a field only with a migration. Pure data — no IO lives here.
///
/// `Eq` is intentionally not derived — `confidence` is an `f32`. Use
/// [`PartialEq`] (every test and consumer only needs equality, never a total
/// order or hashing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Knowledge {
    /// What sort of knowledge this is.
    pub kind: Kind,
    /// Origin + reach — the cross-wave/spec query axis.
    #[serde(flatten)]
    pub scope: Scope,
    /// A short label / title / summary (the searchable headline).
    pub label: String,
    /// The full knowledge text — the body / search corpus.
    pub content: String,
    /// Provenance sidecar.
    pub origin: Origin,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Lifecycle status.
    pub status: Status,
}

impl Knowledge {
    /// Build a knowledge record from a parsed frontmatter object + body.
    ///
    /// Pure: the caller is responsible for having already split the `---` fence
    /// (the `io` layer does this via the shared frontmatter parser). `fm` is the
    /// frontmatter object; `body` is everything after the fence.
    ///
    /// Total — never panics. Missing fields degrade to sensible defaults
    /// (`confidence` → `0.0`, `status` → `active`, `kind` → `summary`).
    #[must_use]
    pub fn from_markdown(fm: &Map<String, Value>, body: &str) -> Self {
        let get_str = |k: &str| fm.get(k).and_then(Value::as_str);
        // Numbers may arrive as native JSON (legacy writers build the map with
        // `json!`, so `wave`/`confidence` are real numbers) OR as strings (the
        // shared frontmatter parser types every scalar as a `String`). Coerce
        // both so the disk round-trip and the in-memory legacy path agree.
        let get_u32 = |k: &str| value_as_u32(fm.get(k));

        let kind = Kind::from_legacy(get_str("kind"));
        let status = Status::from_legacy(get_str("status"));

        // Scope: prefer an explicit `scope:` tag; else infer from spec/wave —
        // this is what folds the legacy stores (which never wrote `scope:`) in.
        // The spec-memory store spells these `origin_spec` / `origin_wave`.
        let spec = get_str("spec").or_else(|| get_str("origin_spec"));
        let wave = get_u32("wave").or_else(|| value_as_u32(fm.get("origin_wave")));
        let scope = match get_str("scope").map(str::to_ascii_lowercase).as_deref() {
            Some("wave") => Scope::Wave {
                spec: spec.unwrap_or_default().to_string(),
                wave: wave.unwrap_or(0),
            },
            Some("spec") => Scope::Spec {
                spec: spec.unwrap_or_default().to_string(),
            },
            Some("global") => Scope::Global,
            // No explicit scope tag (every legacy store) — infer from fields.
            _ => match (spec, wave) {
                (Some(s), Some(w)) => Scope::Wave {
                    spec: s.to_string(),
                    wave: w,
                },
                (Some(s), None) => Scope::Spec { spec: s.to_string() },
                _ => Scope::Global,
            },
        };

        // Label: the legacy stores spell the headline as `name` / `label` /
        // `summary` / `description`; the agent store put it under `summary`,
        // spec-memory under `name`/`description`, knowledge under `name`. Fall
        // back to the first non-empty body line so a label is always present.
        let label = get_str("label")
            .or_else(|| get_str("name"))
            .or_else(|| get_str("summary"))
            .or_else(|| get_str("description"))
            .map(str::to_string)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| first_line(body));

        let origin = Origin {
            spec: spec.map(str::to_string),
            wave,
            role: get_str("role").map(str::to_string),
            session: get_str("session")
                .or_else(|| get_str("session_id"))
                .map(str::to_string),
            captured_at: get_str("captured_at")
                .or_else(|| get_str("at"))
                .unwrap_or("")
                .to_string(),
        };

        let confidence = value_as_f64(fm.get("confidence"))
            .map_or(0.0, |c| c as f32)
            .clamp(0.0, 1.0);

        Self {
            kind,
            scope,
            label,
            content: body.trim_end_matches('\n').to_string(),
            origin,
            confidence,
            status,
        }
    }

    /// Render this record to `(frontmatter_object, body)`.
    ///
    /// The `io` layer fences the frontmatter and writes the body. Keys are
    /// emitted in a fixed order so serialisation is **byte-stable**. `Option`
    /// fields are omitted when absent. The body is the [`content`], newline-
    /// terminated.
    ///
    /// The on-disk `spec` / `wave` keys are derived from [`scope`] (the
    /// authoritative reach); [`from_markdown`] recovers both [`scope`] and
    /// [`origin`]'s `spec`/`wave` from them. A record whose [`origin`] spec/wave
    /// disagree with its [`scope`] therefore does not round-trip — keep them
    /// consistent (the legacy stores always did).
    ///
    /// [`content`]: Knowledge::content
    /// [`scope`]: Knowledge::scope
    /// [`origin`]: Knowledge::origin
    /// [`from_markdown`]: Knowledge::from_markdown
    #[must_use]
    pub fn to_markdown(&self) -> (Map<String, Value>, String) {
        let mut fm = Map::new();
        // Fixed key order → byte-stable output.
        fm.insert("kind".into(), Value::String(self.kind.as_str().into()));
        match &self.scope {
            Scope::Global => {
                fm.insert("scope".into(), Value::String("global".into()));
            }
            Scope::Spec { spec } => {
                fm.insert("scope".into(), Value::String("spec".into()));
                fm.insert("spec".into(), Value::String(spec.clone()));
            }
            Scope::Wave { spec, wave } => {
                fm.insert("scope".into(), Value::String("wave".into()));
                fm.insert("spec".into(), Value::String(spec.clone()));
                fm.insert("wave".into(), Value::Number((*wave).into()));
            }
        }
        fm.insert("label".into(), Value::String(self.label.clone()));
        if let Some(role) = &self.origin.role {
            fm.insert("role".into(), Value::String(role.clone()));
        }
        if let Some(session) = &self.origin.session {
            fm.insert("session".into(), Value::String(session.clone()));
        }
        fm.insert(
            "captured_at".into(),
            Value::String(self.origin.captured_at.clone()),
        );
        if let Some(num) = serde_json::Number::from_f64(f64::from(self.confidence)) {
            fm.insert("confidence".into(), Value::Number(num));
        }
        fm.insert("status".into(), Value::String(self.status.as_str().into()));

        let body = format!("{}\n", self.content.trim_end_matches('\n'));
        (fm, body)
    }

    /// Whether this record carries enough signal to be worth storing.
    ///
    /// The capture path used to persist sinks of pure noise — an `agent`
    /// "memory" with an empty body and the placeholder summary
    /// `"interrupted mid-task"` (and still stamped `confidence: 0.7`), or an
    /// entry whose body is nothing but echoed harness context (a
    /// `<!-- PREFIX-STABLE -->` marker, a `CONTEXT:` line, a verbatim quote of
    /// the CLAUDE.md Guards). Garbage in → garbage out; this is the single
    /// gate the on-disk owner consults before writing, so the noise is rejected
    /// at the one entry point.
    ///
    /// **Conservative by design** — it targets only the *measured* junk and
    /// must never drop a short legitimate decision. A one-line record like
    /// `"Use atomic_md write because it avoids corruption"` passes. A record is
    /// **non-substantive** (returns `false`) only when:
    ///
    /// 1. **Empty body** — [`content`] is empty or whitespace-only. A "memory"
    ///    with no body transfers nothing regardless of its label. (This alone
    ///    catches the 8/8 sialia agent rows, whose bodies are empty.)
    /// 2. **Placeholder label** — [`label`] (trimmed, case-insensitive) is one
    ///    of the known no-information placeholders: `"interrupted mid-task"`,
    ///    `"interrupted"`, `"interrupted mid task"`, `"(no summary)"`, or empty.
    /// 3. **Context echo** — the body is dominated by harness-context markers:
    ///    it starts with `<!-- PREFIX-STABLE` or `CONTEXT:`, or ≥80% of its
    ///    non-empty lines are marker / `CONTEXT:` / Guards-citation lines. The
    ///    threshold is deliberately high so a real decision with one incidental
    ///    marker line still passes.
    ///
    /// Pure — no IO, total, deterministic.
    ///
    /// [`content`]: Knowledge::content
    /// [`label`]: Knowledge::label
    #[must_use]
    pub fn is_substantive(&self) -> bool {
        // (1) Empty body → nothing to transfer.
        if self.content.trim().is_empty() {
            return false;
        }
        // (2) Placeholder label carries no information.
        if is_placeholder_label(&self.label) {
            return false;
        }
        // (3) Body is dominated by echoed harness context.
        if is_context_echo(&self.content) {
            return false;
        }
        true
    }

    /// Deterministic, filename-safe stem for this record: `{compact_ts}-{hash8}`.
    ///
    /// `captured_at` is reduced to its alphanumeric characters; the hash is the
    /// FNV-1a 64-bit of `{kind}|{scope}|{label}|{content}` (8 hex chars). Pure
    /// and stable per `(timestamp, kind, scope, label, content)` — matches the
    /// existing `slug_for` convention used across the legacy writers.
    #[must_use]
    pub fn slug(&self) -> String {
        let ts_compact: String = self
            .origin
            .captured_at
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        let scope_tag = match &self.scope {
            Scope::Global => "global".to_string(),
            Scope::Spec { spec } => format!("spec:{spec}"),
            Scope::Wave { spec, wave } => format!("wave:{spec}:{wave}"),
        };
        let seed = format!(
            "{}|{}|{}|{}",
            self.kind.as_str(),
            scope_tag,
            self.label,
            self.content
        );
        format!("{ts_compact}-{}", fnv1a8(&seed))
    }
}

/// Coerce a frontmatter value to `u32`, accepting either a native JSON number
/// or a string-encoded one (the frontmatter parser yields strings).
fn value_as_u32(v: Option<&Value>) -> Option<u32> {
    let v = v?;
    if let Some(n) = v.as_u64() {
        return u32::try_from(n).ok();
    }
    v.as_str().and_then(|s| s.trim().parse::<u32>().ok())
}

/// Coerce a frontmatter value to `f64`, accepting either a native JSON number
/// or a string-encoded one.
fn value_as_f64(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    v.as_str().and_then(|s| s.trim().parse::<f64>().ok())
}

/// Known placeholder labels that carry no information — a record whose headline
/// is one of these is not worth storing (see [`Knowledge::is_substantive`]).
/// Matched case-insensitively after trimming.
fn is_placeholder_label(label: &str) -> bool {
    let t = label.trim().to_ascii_lowercase();
    matches!(
        t.as_str(),
        "" | "interrupted"
            | "interrupted mid-task"
            | "interrupted mid task"
            | "(no summary)"
    )
}

/// Whether `body` is dominated by echoed harness context rather than real
/// knowledge. Conservative: a body that merely *contains* a marker line still
/// passes; only a body that *starts with* a marker, or whose non-empty lines are
/// ≥80% marker/context/citation, is treated as an echo.
fn is_context_echo(body: &str) -> bool {
    let trimmed = body.trim_start();
    // Fast path: a body that opens with a context marker is an echo.
    if trimmed.starts_with("<!-- PREFIX-STABLE") || trimmed.starts_with("CONTEXT:") {
        return true;
    }
    let non_empty: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if non_empty.is_empty() {
        return false; // empty body is handled by the content check, not here
    }
    let echo_lines = non_empty.iter().filter(|l| is_echo_line(l)).count();
    // ≥80% of the non-empty lines are markers/context/citation → echo.
    echo_lines * 5 >= non_empty.len() * 4
}

/// Whether a single (trimmed, non-empty) line is a harness-context marker rather
/// than authored knowledge: an HTML sentinel comment, a `CONTEXT:` line, or a
/// verbatim Guards/CLAUDE.md citation (a `## Guards` heading or a quoted block).
fn is_echo_line(line: &str) -> bool {
    line.starts_with("<!--")
        || line.starts_with("CONTEXT:")
        || line.starts_with("## Guards")
        || line.starts_with("> ")
}

/// First non-empty trimmed line of `body`, capped at 200 chars — the label
/// fallback when no headline frontmatter field is present.
fn first_line(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

/// FNV-1a 64-bit of `s` rendered as 8 hex chars. Slug suffix only — not
/// security-relevant. Matches `apps/rt/src/util/slug.rs::fnv1a8` byte-for-byte.
fn fnv1a8(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}").chars().take(8).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    // --- round-trip --------------------------------------------------------

    #[test]
    fn markdown_round_trips_wave_scope() {
        let k = Knowledge {
            kind: Kind::Summary,
            scope: Scope::Wave {
                spec: "demo".into(),
                wave: 2,
            },
            label: "did the thing".into(),
            content: "multi\nline\ndetails".into(),
            origin: Origin {
                spec: Some("demo".into()),
                wave: Some(2),
                role: Some("backend".into()),
                session: Some("s-1".into()),
                captured_at: "2026-06-15T00:00:00.000Z".into(),
            },
            confidence: 0.7,
            status: Status::Active,
        };
        let (fm, body) = k.to_markdown();
        let back = Knowledge::from_markdown(&fm, &body);
        assert_eq!(back, k, "from(to(k)) must equal k");
    }

    #[test]
    fn to_markdown_is_byte_stable() {
        let k = Knowledge {
            kind: Kind::Decision,
            scope: Scope::Global,
            label: "use markdown".into(),
            content: "chose markdown over sqlite".into(),
            origin: Origin {
                captured_at: "2026-06-15T00:00:00.000Z".into(),
                ..Origin::default()
            },
            confidence: 0.0,
            status: Status::Active,
        };
        let a = k.to_markdown();
        let b = k.to_markdown();
        assert_eq!(a, b, "serialisation must be deterministic");
        // Key order is fixed: kind, scope, label, captured_at, confidence, status.
        let keys: Vec<&String> = a.0.keys().collect();
        assert_eq!(
            keys,
            ["kind", "scope", "label", "captured_at", "confidence", "status"]
        );
    }

    #[test]
    fn slug_is_deterministic_and_content_sensitive() {
        let mut k = Knowledge {
            kind: Kind::Lesson,
            scope: Scope::Global,
            label: "a".into(),
            content: "b".into(),
            origin: Origin {
                captured_at: "2026-06-15T00:00:00.000Z".into(),
                ..Origin::default()
            },
            confidence: 0.5,
            status: Status::Active,
        };
        let s1 = k.slug();
        assert_eq!(s1, k.slug(), "slug is deterministic");
        k.content = "c".into();
        assert_ne!(s1, k.slug(), "slug changes with content");
    }

    // --- subsumption of the 5 legacy frontmatter shapes --------------------

    #[test]
    fn subsumes_knowledge_pattern_store() {
        // .claude/knowledge/*.md (run_knowledge): kind=pattern, name, body.
        let fm = obj(json!({
            "kind": "pattern",
            "name": "fail-open",
            "captured_at": "2026-06-15T00:00:00.000Z",
            "confidence": 0.8,
            "source": "spec-1",
            "status": "active",
        }));
        let k = Knowledge::from_markdown(&fm, "hooks never abort user work\n");
        assert_eq!(k.kind, Kind::Principle); // pattern → principle
        assert_eq!(k.label, "fail-open");
        assert_eq!(k.content, "hooks never abort user work");
        assert_eq!(k.scope, Scope::Global);
        assert!((k.confidence - 0.8).abs() < 1e-6);
        assert_eq!(k.status, Status::Active);
    }

    #[test]
    fn subsumes_knowledge_decision_store() {
        // session_knowledge_observer: kind=decision, spec, body.
        let fm = obj(json!({
            "kind": "decision",
            "captured_at": "2026-06-15T00:00:00.000Z",
            "source_event": "spec:demo",
            "spec": "demo",
        }));
        let k = Knowledge::from_markdown(&fm, "Use UUIDv7 for all primary keys\n");
        assert_eq!(k.kind, Kind::Decision);
        assert_eq!(k.scope, Scope::Spec { spec: "demo".into() });
        assert_eq!(k.label, "Use UUIDv7 for all primary keys"); // body fallback
        assert_eq!(k.origin.spec.as_deref(), Some("demo"));
    }

    #[test]
    fn subsumes_agent_memory_store() {
        // .claude/memory/agent/*.md: session_id, spec, wave, role, summary,
        // confidence, status, at, last_used. No `kind` → Summary.
        let fm = obj(json!({
            "session_id": "abc123",
            "spec": "demo",
            "wave": 1,
            "role": "wave-1-badges",
            "summary": "delivered the scan-guards subcommands",
            "confidence": 0.7,
            "status": "active",
            "at": "2026-06-15T00:00:00.000Z",
            "last_used": "2026-06-15T00:00:00.000Z",
        }));
        let k = Knowledge::from_markdown(&fm, "details body here");
        assert_eq!(k.kind, Kind::Summary); // no kind → Summary
        assert_eq!(
            k.scope,
            Scope::Wave { spec: "demo".into(), wave: 1 }
        );
        assert_eq!(k.label, "delivered the scan-guards subcommands");
        assert_eq!(k.origin.role.as_deref(), Some("wave-1-badges"));
        assert_eq!(k.origin.session.as_deref(), Some("abc123"));
        assert_eq!(k.origin.captured_at, "2026-06-15T00:00:00.000Z"); // `at` fallback
    }

    #[test]
    fn subsumes_decisions_lessons_store() {
        // .claude/memory/decisions|lessons/*.md: kind, captured_at, source,
        // context, status.
        let fm = obj(json!({
            "kind": "lesson",
            "captured_at": "2026-06-15T00:00:00.000Z",
            "source": "spec-1",
            "context": "during EXECUTE",
            "status": "active",
        }));
        let k = Knowledge::from_markdown(&fm, "Count artifacts before extrapolating cost\n");
        assert_eq!(k.kind, Kind::Lesson);
        assert_eq!(k.scope, Scope::Global);
        assert_eq!(k.label, "Count artifacts before extrapolating cost");
    }

    #[test]
    fn subsumes_spec_memory_store() {
        // .claude/spec/{spec}/memory/*.md: name, kind(principle|process|
        // reference), origin_spec, origin_wave, description.
        let fm = obj(json!({
            "name": "scan-rust-first",
            "kind": "process",
            "origin_spec": "demo",
            "origin_wave": 3,
            "description": "Scan structural in Rust",
        }));
        let k = Knowledge::from_markdown(&fm, "# Scan structural in Rust\n\nbody\n");
        assert_eq!(k.kind, Kind::Principle); // process → principle
        // origin_spec + origin_wave fold into Wave scope.
        assert_eq!(
            k.scope,
            Scope::Wave { spec: "demo".into(), wave: 3 }
        );
        assert_eq!(k.label, "scan-rust-first"); // name wins over description
        assert_eq!(k.origin.spec.as_deref(), Some("demo"));
    }

    #[test]
    fn reference_kind_round_trips() {
        let fm = obj(json!({
            "name": "where-the-router-lives",
            "kind": "reference",
            "origin_spec": "demo",
            "captured_at": "2026-06-15T00:00:00.000Z",
        }));
        let k = Knowledge::from_markdown(&fm, "see src/route.rs\n");
        assert_eq!(k.kind, Kind::Reference);
        let (fm2, body2) = k.to_markdown();
        let back = Knowledge::from_markdown(&fm2, &body2);
        assert_eq!(back.kind, Kind::Reference);
        assert_eq!(back, k);
    }

    #[test]
    fn status_and_kind_legacy_fallbacks_never_panic() {
        assert_eq!(Kind::from_legacy(None), Kind::Summary);
        assert_eq!(Kind::from_legacy(Some("garbage")), Kind::Summary);
        assert_eq!(Status::from_legacy(None), Status::Active);
        assert_eq!(Status::from_legacy(Some("garbage")), Status::Active);
        assert_eq!(Status::from_legacy(Some("PROMOTED")), Status::Promoted);
    }

    #[test]
    fn confidence_is_clamped() {
        let fm = obj(json!({ "kind": "lesson", "confidence": 9.0 }));
        let k = Knowledge::from_markdown(&fm, "x");
        assert!((k.confidence - 1.0).abs() < 1e-6);
    }

    // --- is_substantive: reject the measured junk, keep real knowledge -------

    /// Build a record with the given label + body, otherwise plausible.
    fn rec(label: &str, content: &str) -> Knowledge {
        Knowledge {
            kind: Kind::Summary,
            scope: Scope::Global,
            label: label.into(),
            content: content.into(),
            origin: Origin {
                captured_at: "2026-06-15T00:00:00.000Z".into(),
                ..Origin::default()
            },
            confidence: 0.7,
            status: Status::Active,
        }
    }

    #[test]
    fn empty_body_is_not_substantive() {
        assert!(!rec("delivered the thing", "").is_substantive());
        assert!(!rec("delivered the thing", "   \n\t ").is_substantive());
    }

    #[test]
    fn placeholder_summary_is_not_substantive() {
        // The exact sialia case: empty body + "interrupted mid-task".
        assert!(!rec("interrupted mid-task", "").is_substantive());
        // Even with a body, a placeholder label is no-information.
        assert!(!rec("interrupted mid-task", "some body").is_substantive());
        assert!(!rec("INTERRUPTED MID-TASK", "some body").is_substantive());
        assert!(!rec("interrupted", "some body").is_substantive());
        assert!(!rec("interrupted mid task", "some body").is_substantive());
        assert!(!rec("(no summary)", "some body").is_substantive());
        assert!(!rec("", "some body").is_substantive());
    }

    #[test]
    fn context_echo_is_not_substantive() {
        assert!(!rec("x", "<!-- PREFIX-STABLE -->").is_substantive());
        assert!(!rec("x", "<!-- PREFIX-STABLE -->\nmore marker noise <!-- y -->").is_substantive());
        assert!(!rec("x", "CONTEXT: the orchestrator routes intent").is_substantive());
        // A body that is entirely Guards citation / markers → echo.
        let guards = "## Guards\n> hooks never abort user work\n> domain stays pure";
        assert!(!rec("x", guards).is_substantive());
    }

    #[test]
    fn real_short_decision_is_substantive() {
        // A genuine one-line decision MUST pass — the conservative line.
        assert!(rec("Use atomic_md write", "Use atomic_md write because it avoids corruption")
            .is_substantive());
        // A real lesson passes.
        assert!(rec(
            "Count artifacts before extrapolating",
            "Count the artifacts before extrapolating cost — sampling lies on small N."
        )
        .is_substantive());
        // A real decision with ONE incidental marker line still passes (not ≥80%).
        let mixed = "We chose the NDJSON channel over SQLite.\nIt is append-only and diffable.\n> note: see route.rs";
        assert!(rec("ndjson over sqlite", mixed).is_substantive());
    }
}
