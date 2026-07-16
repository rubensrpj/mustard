//! `capability` — the **durable** "what the system does" record.
//!
//! ## Why this is its own type (not transient knowledge)
//!
//! Decisions, lessons and run summaries are *transient, decaying* knowledge —
//! they age out (today they live as `decision` / `lesson` events in the
//! per-spec NDJSON log). A [`Capability`] is the opposite: it is the **living
//! capability spec** — the durable statement of a behaviour the system
//! provides. It must **never decay or be pruned**; it is updated in place as
//! the behaviour changes and deprecated (never deleted) when the behaviour is
//! removed. That different lifecycle is exactly why it does not reuse the
//! transient-knowledge shape (whose decay model would let a pruner drop it).
//! It is, however, held to the **same serde discipline**:
//! every field is `#[serde(default)]`, forward-compatible, and fail-open —
//! unknown / missing fields never break a parse, and nothing here panics.
//!
//! ## Purity contract
//!
//! This module lives under `domain/` — it is a **public contract** other crates
//! render against, and it is **pure**: no filesystem, no logging, no disk. The
//! only methods are total, side-effect-free transforms. In particular
//! [`Capability::acceptance_criteria`] compiles the command-bearing scenarios
//! into the **existing** [`AcceptanceCriterion`] type
//! ([`crate::domain::spec::contract::AcceptanceCriterion`]) — there is no
//! parallel AC type.
//!
//! ## Agnosticism
//!
//! Zero hardcoded language / framework / role names. A [`Requirement`]
//! statement is free prose ("the system SHALL …"); a [`Scenario`] is a
//! when/then pair with an optional runnable `command`. The link fields
//! ([`Capability::covers`] / [`specs`] / [`related`]) carry opaque wikilink ids
//! (`entity.{name}`, `cap.{slug}`) the caller mints — this module never invents
//! or matches them.
//!
//! [`specs`]: Capability::specs
//! [`related`]: Capability::related

use crate::domain::spec::contract::AcceptanceCriterion;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Event name constants — the capability.* channel.
//
// The future emitter (rt-side) writes these; the projection
// (`view::projection::capability`) folds them. Named constants so the producer
// and the consumer can never drift on a string literal.
// ---------------------------------------------------------------------------

/// Records that a [`Capability`] was first declared (its initial full state).
pub const EVENT_CAPABILITY_DECLARED: &str = "capability.declared";
/// Records an in-place change to one requirement of a capability
/// ([`CapabilityUpdate`] — `added` / `modified` / `removed`).
pub const EVENT_CAPABILITY_UPDATE: &str = "capability.update";
/// Records that a capability covers a code entity that no longer exists — the
/// capability is now stale until re-linked ([`CapabilityDrift`]).
pub const EVENT_CAPABILITY_DRIFT: &str = "capability.drift";

// ---------------------------------------------------------------------------
// Scenario
// ---------------------------------------------------------------------------

/// One concrete when/then example of a [`Requirement`].
///
/// A scenario that carries a runnable `command` compiles into an executable
/// [`AcceptanceCriterion`] (see [`Capability::acceptance_criteria`]); a scenario
/// with no command is documentary only and is skipped by that compilation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scenario {
    /// Short handle for this scenario, slugified into the compiled AC id.
    #[serde(default)]
    pub name: String,
    /// The precondition / trigger ("when …").
    #[serde(default)]
    pub when: String,
    /// The expected outcome ("then …").
    #[serde(default)]
    pub then: String,
    /// Optional runnable command — exit 0 ⇒ the scenario holds. When present,
    /// the scenario compiles into an [`AcceptanceCriterion`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

// ---------------------------------------------------------------------------
// Requirement
// ---------------------------------------------------------------------------

/// One normative statement of a [`Capability`] plus its concrete scenarios.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    /// The normative prose (SHALL / MUST …). Free-form, narrative-locale.
    #[serde(default)]
    pub statement: String,
    /// Concrete when/then examples that pin the statement down.
    #[serde(default)]
    pub scenarios: Vec<Scenario>,
}

// ---------------------------------------------------------------------------
// Capability
// ---------------------------------------------------------------------------

/// The durable record of one behaviour the system provides.
///
/// **Public contract.** Other crates (`mustard-rt`, the dashboard) render this
/// shape; change a field only with a migration. Pure data — no IO lives here.
/// Minimal by design: easier to extend than to walk back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// Namespaced handle, e.g. `cap.{slug}`.
    #[serde(default)]
    pub id: String,
    /// Human-readable title (narrative locale).
    #[serde(default)]
    pub title: String,
    /// Lifecycle word, e.g. `"active"` / `"deprecated"`. Free string (kept
    /// agnostic — no enum so a future state needs no migration); `is_active`
    /// reads it leniently.
    #[serde(default)]
    pub status: String,
    /// The normative requirements that make up this capability.
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    /// Wikilink ids of the code entities that realise this capability
    /// (the convention is `entity.{name}`, where `{name}` is the bare
    /// declaration name the grain registry mined). Opaque — minted by the
    /// caller. `capability sync-nodes` materializes a `.claude/graph/{id}.md`
    /// node per covered entity that exists in the registry, so the id resolves.
    #[serde(default)]
    pub covers: Vec<String>,
    /// Spec ids that created or changed this capability.
    #[serde(default)]
    pub specs: Vec<String>,
    /// Ids of related capabilities (`cap.{slug}`).
    #[serde(default)]
    pub related: Vec<String>,
}

impl Capability {
    /// Whether this capability is live. Lenient: any casing of `"active"` (or an
    /// empty / unset status — a freshly declared capability defaults to live)
    /// counts as active; everything else (e.g. `"deprecated"`) does not. Pure,
    /// total, never panics.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let s = self.status.trim();
        s.is_empty() || s.eq_ignore_ascii_case("active")
    }

    /// Compile every command-bearing [`Scenario`] of every [`Requirement`] into
    /// the **existing** [`AcceptanceCriterion`] type.
    ///
    /// Mapping (one AC per scenario that carries a `command`):
    /// - `id` ← `{capability.id}-{slugified scenario.name}` (deterministic).
    /// - `statement` ← the when/then prose joined into one sentence
    ///   (`"when X, then Y"`, agnostic glue words; either half may be empty).
    /// - `command` ← `scenario.command` (the reason this scenario was selected).
    ///
    /// Scenarios **without** a command are documentary and are skipped. The
    /// result preserves requirement-then-scenario declaration order, so the
    /// output is deterministic. Pure — no IO, never panics.
    #[must_use]
    pub fn acceptance_criteria(&self) -> Vec<AcceptanceCriterion> {
        let mut out = Vec::new();
        for req in &self.requirements {
            for scenario in &req.scenarios {
                let Some(command) = scenario
                    .command
                    .as_deref()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                else {
                    continue; // documentary scenario — no runnable command.
                };
                out.push(AcceptanceCriterion {
                    id: format!("{}-{}", self.id, slug(&scenario.name)),
                    statement: scenario_statement(&scenario.when, &scenario.then),
                    command: command.to_string(),
                });
            }
        }
        out
    }
}

/// Compute the per-requirement change-log between two snapshots of a
/// [`Capability`] — the **robust** delta convention: the change-log is
/// *computed by diffing*, never hand-authored as fragile delta lines.
///
/// Requirement **identity** is the normative `statement`, trim-compared (so
/// leading / trailing whitespace edits are not spurious moves). For a matched
/// (trim-equal) pair, **content** equality is the `scenarios` — the substantive
/// payload — so a scenario edit on an unchanged statement surfaces as
/// `Modified`, while a whitespace-only statement edit (already absorbed by the
/// identity rule) does not. (Comparing `scenarios` rather than the whole
/// [`Requirement`] is what keeps that whitespace-only edit silent; the trimmed
/// statements are equal by construction once the pair matches.)
///
/// Rules (operates on `requirements` ONLY — `covers`/`specs`/`related` are
/// snapshot metadata carried by `capability.declared`, never per-requirement
/// updates):
/// - statement in `curr` but not `prev` → [`UpdateOp::Added`] (the `curr` req).
/// - statement in both, but the `Requirement` content differs →
///   [`UpdateOp::Modified`] (the `curr` req).
/// - statement in `prev` but not `curr` → [`UpdateOp::Removed`] (the `prev` req).
/// - `prev = None` (first declaration) → EMPTY: creation is carried by the
///   `capability.declared` snapshot, not by per-requirement `Added` noise.
///
/// Deterministic, byte-stable order: Added-then-Modified follow `curr`'s
/// requirement order; Removed follow `prev`'s order, appended after. The
/// returned [`CapabilityUpdate`]s are stamped with `curr.id`. Pure, total,
/// never panics.
#[must_use]
pub fn diff_requirements(prev: Option<&Capability>, curr: &Capability) -> Vec<CapabilityUpdate> {
    // First declaration: creation is the `capability.declared` snapshot, so we
    // emit no per-requirement Added events.
    let Some(prev) = prev else {
        return Vec::new();
    };

    let mut out = Vec::new();

    // Walk `curr` in declaration order → Added / Modified (byte-stable).
    for req in &curr.requirements {
        let key = req.statement.trim();
        match prev.requirements.iter().find(|p| p.statement.trim() == key) {
            None => out.push(update(&curr.id, UpdateOp::Added, req.clone())),
            // Same statement (trim-equal), scenarios differ ⇒ Modified. A
            // whitespace-only statement edit is NOT a move (identity trims it).
            Some(prior) if prior.scenarios != req.scenarios => {
                out.push(update(&curr.id, UpdateOp::Modified, req.clone()));
            }
            Some(_) => {} // same statement + same scenarios ⇒ no change.
        }
    }

    // Walk `prev` in declaration order → Removed for any statement gone in curr.
    for req in &prev.requirements {
        let key = req.statement.trim();
        if !curr.requirements.iter().any(|c| c.statement.trim() == key) {
            out.push(update(&curr.id, UpdateOp::Removed, req.clone()));
        }
    }

    out
}

/// Build one [`CapabilityUpdate`] for `id` / `op` / `requirement`.
fn update(id: &str, op: UpdateOp, requirement: Requirement) -> CapabilityUpdate {
    CapabilityUpdate { id: id.to_string(), op, requirement }
}

/// Join a scenario's when/then halves into one statement sentence. Agnostic
/// glue (`when` / `then`) — both halves are optional, and an all-empty scenario
/// degrades to an empty string rather than panicking.
///
/// `pub` so the spec drafter can seed EARS-shaped skeleton acceptance criteria
/// through the SAME join the capability compiler uses (`when X, then Y`),
/// instead of reimplementing the glue — one origin for the EARS sentence shape.
#[must_use]
pub fn scenario_statement(when: &str, then: &str) -> String {
    match (when.trim(), then.trim()) {
        ("", "") => String::new(),
        (w, "") => format!("when {w}"),
        ("", t) => format!("then {t}"),
        (w, t) => format!("when {w}, then {t}"),
    }
}

/// Deterministic, agnostic slug of `text`: lowercase ASCII alphanumerics kept
/// verbatim, every other run of characters collapsed to a single `-`, leading /
/// trailing `-` trimmed. A fully non-alphanumeric (or empty) input degrades to
/// `"x"` — mirroring the documented `interpret::slugify` floor. Language-blind
/// by design (no accent folding, no stopwords) so the compiled AC id carries no
/// locale assumption.
fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        "x".to_string()
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// Event payloads — pure serde lenses over `HarnessEvent::payload`.
//
// Same discipline as the pipeline payloads in `domain/model/event.rs`: every
// field `#[serde(default)]`, forward-compatible, fail-open. The future emitter
// builds these; the projection reads them.
// ---------------------------------------------------------------------------

/// Payload for [`EVENT_CAPABILITY_DECLARED`] — the capability's initial full
/// state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDeclared {
    /// The freshly declared capability, in full.
    #[serde(default)]
    pub capability: Capability,
}

/// The kind of in-place change a [`CapabilityUpdate`] records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateOp {
    /// A new requirement was added to the capability.
    #[default]
    Added,
    /// An existing requirement was changed in place.
    Modified,
    /// A requirement was retired from the capability.
    Removed,
}

/// Payload for [`EVENT_CAPABILITY_UPDATE`] — one requirement-level change to an
/// already-declared capability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityUpdate {
    /// Id of the capability being changed (`cap.{slug}`).
    #[serde(default)]
    pub id: String,
    /// What kind of change this is.
    #[serde(default)]
    pub op: UpdateOp,
    /// The requirement that was added / modified / removed.
    #[serde(default)]
    pub requirement: Requirement,
}

/// Payload for [`EVENT_CAPABILITY_DRIFT`] — a capability covers a code entity
/// that no longer exists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDrift {
    /// Id of the capability that has drifted (`cap.{slug}`).
    #[serde(default)]
    pub id: String,
    /// The orphaned code-entity wikilink id (was in [`Capability::covers`] but
    /// no longer resolves).
    #[serde(default)]
    pub entity: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scenario(name: &str, when: &str, then: &str, command: Option<&str>) -> Scenario {
        Scenario {
            name: name.into(),
            when: when.into(),
            then: then.into(),
            command: command.map(str::to_string),
        }
    }

    // --- serde round-trip --------------------------------------------------

    #[test]
    fn capability_serde_round_trips() {
        let cap = Capability {
            id: "cap.living-spec".into(),
            title: "Living capability spec".into(),
            status: "active".into(),
            requirements: vec![Requirement {
                statement: "The system SHALL persist capabilities durably.".into(),
                scenarios: vec![scenario(
                    "round trip",
                    "a capability is declared",
                    "it survives a reload",
                    Some("cargo test -p mustard-core"),
                )],
            }],
            covers: vec!["rt.entity.CapabilityStore".into()],
            specs: vec!["living-capability-spec".into()],
            related: vec!["cap.knowledge".into()],
        };
        let json = serde_json::to_string(&cap).expect("serialise");
        let back: Capability = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, cap, "from(to(cap)) must equal cap");
    }

    #[test]
    fn missing_fields_default_fail_open() {
        // A near-empty object must deserialise (forward-compat / fail-open):
        // every field is `#[serde(default)]`.
        let cap: Capability = serde_json::from_value(json!({ "id": "cap.x" }))
            .expect("partial object deserialises");
        assert_eq!(cap.id, "cap.x");
        assert!(cap.title.is_empty());
        assert!(cap.requirements.is_empty());
        assert!(cap.covers.is_empty());
        // An unknown field is tolerated, not rejected.
        let cap2: Capability =
            serde_json::from_value(json!({ "id": "cap.y", "future_field": 42 }))
                .expect("unknown field tolerated");
        assert_eq!(cap2.id, "cap.y");
    }

    #[test]
    fn is_active_is_lenient() {
        let active = Capability { status: "active".into(), ..Capability::default() };
        let upper = Capability { status: "ACTIVE".into(), ..Capability::default() };
        let empty = Capability::default(); // unset status → live
        let dep = Capability { status: "deprecated".into(), ..Capability::default() };
        assert!(active.is_active());
        assert!(upper.is_active());
        assert!(empty.is_active());
        assert!(!dep.is_active());
    }

    // --- acceptance_criteria: compile only command-bearing scenarios -------

    #[test]
    fn acceptance_criteria_compiles_only_command_scenarios() {
        let cap = Capability {
            id: "cap.demo".into(),
            requirements: vec![
                Requirement {
                    statement: "R1".into(),
                    scenarios: vec![
                        scenario("has cmd", "x happens", "y holds", Some("rtk cargo test")),
                        scenario("no cmd", "z happens", "w holds", None),
                        // present-but-blank command is treated as no command.
                        scenario("blank cmd", "a", "b", Some("   ")),
                    ],
                },
                Requirement {
                    statement: "R2".into(),
                    scenarios: vec![scenario("second", "p", "q", Some("rtk cargo build"))],
                },
            ],
            ..Capability::default()
        };
        let acs = cap.acceptance_criteria();
        // Only the two command-bearing scenarios compile.
        assert_eq!(acs.len(), 2);
        // Declaration order preserved (R1 scenario before R2 scenario).
        assert_eq!(acs[0].id, "cap.demo-has-cmd");
        assert_eq!(acs[0].statement, "when x happens, then y holds");
        assert_eq!(acs[0].command, "rtk cargo test");
        assert_eq!(acs[1].id, "cap.demo-second");
        assert_eq!(acs[1].command, "rtk cargo build");
    }

    #[test]
    fn acceptance_criteria_statement_handles_partial_when_then() {
        let cap = Capability {
            id: "cap.p".into(),
            requirements: vec![Requirement {
                statement: String::new(),
                scenarios: vec![
                    scenario("only-when", "the daemon starts", "", Some("true")),
                    scenario("only-then", "", "the port is open", Some("true")),
                    scenario("neither", "", "", Some("true")),
                ],
            }],
            ..Capability::default()
        };
        let acs = cap.acceptance_criteria();
        assert_eq!(acs[0].statement, "when the daemon starts");
        assert_eq!(acs[1].statement, "then the port is open");
        assert_eq!(acs[2].statement, "");
    }

    #[test]
    fn slug_is_agnostic_and_floors_to_x() {
        assert_eq!(slug("Has Cmd"), "has-cmd");
        assert_eq!(slug("  spaced  out  "), "spaced-out");
        assert_eq!(slug("a/b\\c"), "a-b-c");
        assert_eq!(slug(""), "x");
        assert_eq!(slug("!!!"), "x");
        // No accent folding / no stopword stripping — language-blind.
        assert_eq!(slug("the quick"), "the-quick");
    }

    // --- diff_requirements: the computed change-log ------------------------

    fn req(statement: &str, scenarios: Vec<Scenario>) -> Requirement {
        Requirement { statement: statement.into(), scenarios }
    }

    #[test]
    fn diff_first_declaration_is_empty() {
        // prev = None ⇒ creation is the `capability.declared` snapshot, so no
        // per-requirement Added noise.
        let curr = Capability {
            id: "cap.x".into(),
            requirements: vec![req("R1", vec![]), req("R2", vec![])],
            ..Capability::default()
        };
        assert!(diff_requirements(None, &curr).is_empty());
    }

    #[test]
    fn diff_detects_added_modified_removed() {
        let prev = Capability {
            id: "cap.x".into(),
            requirements: vec![
                req("keep", vec![scenario("s", "w", "t", None)]),
                req("change", vec![]),
                req("drop", vec![]),
            ],
            ..Capability::default()
        };
        let curr = Capability {
            id: "cap.x".into(),
            requirements: vec![
                // "keep" unchanged (identical content) ⇒ no event.
                req("keep", vec![scenario("s", "w", "t", None)]),
                // "change" same statement, scenarios differ ⇒ Modified.
                req("change", vec![scenario("new", "a", "b", Some("true"))]),
                // "add" is new ⇒ Added.
                req("add", vec![]),
                // "drop" gone ⇒ Removed.
            ],
            ..Capability::default()
        };

        let deltas = diff_requirements(Some(&prev), &curr);
        // Added/Modified in curr order, then Removed in prev order.
        let summary: Vec<(UpdateOp, &str)> =
            deltas.iter().map(|d| (d.op, d.requirement.statement.as_str())).collect();
        assert_eq!(
            summary,
            vec![
                (UpdateOp::Modified, "change"),
                (UpdateOp::Added, "add"),
                (UpdateOp::Removed, "drop"),
            ]
        );
        // Every delta is stamped with curr.id.
        assert!(deltas.iter().all(|d| d.id == "cap.x"));
        // Modified / Added carry the CURR requirement; Removed carries PREV.
        let modified = deltas.iter().find(|d| d.op == UpdateOp::Modified).unwrap();
        assert_eq!(modified.requirement.scenarios.len(), 1, "curr content on Modified");
        assert_eq!(modified.requirement.scenarios[0].command.as_deref(), Some("true"));
    }

    #[test]
    fn diff_identity_is_trimmed_statement() {
        // A pure whitespace edit on the statement is NOT a move (identity is the
        // trimmed statement); only the scenario change makes it Modified.
        let prev = Capability {
            id: "cap.t".into(),
            requirements: vec![req("R1", vec![])],
            ..Capability::default()
        };
        let curr_ws_only = Capability {
            id: "cap.t".into(),
            requirements: vec![req("  R1  ", vec![])],
            ..Capability::default()
        };
        assert!(
            diff_requirements(Some(&prev), &curr_ws_only).is_empty(),
            "trim-equal statement + equal content ⇒ no delta"
        );
    }

    #[test]
    fn diff_ignores_snapshot_metadata() {
        // covers / specs / related differ but requirements are identical ⇒ no
        // per-requirement delta (that metadata rides on capability.declared).
        let prev = Capability {
            id: "cap.m".into(),
            requirements: vec![req("R1", vec![])],
            covers: vec!["entity.A".into()],
            specs: vec!["spec.old".into()],
            ..Capability::default()
        };
        let curr = Capability {
            id: "cap.m".into(),
            requirements: vec![req("R1", vec![])],
            covers: vec!["entity.B".into()],
            specs: vec!["spec.old".into(), "spec.new".into()],
            related: vec!["cap.other".into()],
            ..Capability::default()
        };
        assert!(diff_requirements(Some(&prev), &curr).is_empty());
    }

    // --- event payloads round-trip -----------------------------------------

    #[test]
    fn event_payloads_round_trip() {
        let declared = CapabilityDeclared {
            capability: Capability { id: "cap.x".into(), ..Capability::default() },
        };
        let upd = CapabilityUpdate {
            id: "cap.x".into(),
            op: UpdateOp::Modified,
            requirement: Requirement { statement: "R".into(), scenarios: vec![] },
        };
        let drift = CapabilityDrift { id: "cap.x".into(), entity: "rt.entity.Gone".into() };
        for (a, b) in [
            (serde_json::to_value(&declared).unwrap(), json!({ "capability": { "id": "cap.x", "title": "", "status": "", "requirements": [], "covers": [], "specs": [], "related": [] } })),
        ] {
            assert_eq!(a, b);
        }
        // op serialises lowercase.
        assert_eq!(serde_json::to_value(&upd).unwrap()["op"], json!("modified"));
        // Drift round-trips.
        let back: CapabilityDrift =
            serde_json::from_value(serde_json::to_value(&drift).unwrap()).unwrap();
        assert_eq!(back, drift);
        // UpdateOp defaults to `added` and tolerates unknown via default.
        assert_eq!(UpdateOp::default(), UpdateOp::Added);
    }
}
